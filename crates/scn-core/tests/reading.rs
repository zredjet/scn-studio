//! 通読とプレーンテキスト（`route::read`・`plaintext`、T2-5、D056）：サンプルのルートの解決と、プレーンテキストの
//! スナップショット（完了条件）。

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use scn_core::ir::IrInline;
use scn_core::plaintext::plain_text;
use scn_core::route::{
    Reading, ReadingItem, ReadingTarget, RouteStep, Variables, read, reading_choices, target_label,
};
use scn_core::{analyze, load_project};

fn sample(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples")
        .join(name)
}

fn text(inlines: &[IrInline]) -> String {
    inlines
        .iter()
        .map(|i| match i {
            IrInline::Text { text } | IrInline::Emphasis { text } => text.clone(),
            IrInline::Ruby { base, .. } => base.clone(),
            IrInline::Interpolation { expr_text, .. } => format!("{{={expr_text}}}"),
            _ => String::new(),
        })
        .collect()
}

/// 通読を、人が読める形にする（シーン、選んだ選択肢、エンディング、止まった理由、台詞と地の文の数）。
fn outline(reading: &Reading) -> String {
    let mut out = String::new();
    let mut lines = 0;
    for item in &reading.items {
        match item {
            ReadingItem::Scene { id, route, .. } => {
                if lines > 0 {
                    let _ = writeln!(out, "  ({lines}行)");
                    lines = 0;
                }
                let _ = writeln!(out, "{id} [{}]", route.as_deref().unwrap_or("-"));
            }
            ReadingItem::Say { .. } | ReadingItem::Narrate { .. } => lines += 1,
            ReadingItem::Break => {}
            ReadingItem::Choice {
                options, chosen, ..
            } => {
                if lines > 0 {
                    let _ = writeln!(out, "  ({lines}行)");
                    lines = 0;
                }
                let _ = writeln!(
                    out,
                    "  選択：{}",
                    options
                        .get(*chosen)
                        .map(|o| text(&o.text))
                        .unwrap_or_default()
                );
            }
            ReadingItem::Ending { id } => {
                if lines > 0 {
                    let _ = writeln!(out, "  ({lines}行)");
                    lines = 0;
                }
                let _ = writeln!(out, "  エンディング：{}", id.as_deref().unwrap_or(""));
            }
        }
    }
    if lines > 0 {
        let _ = writeln!(out, "  ({lines}行)");
    }
    let _ = writeln!(
        out,
        "stop: {} {}",
        reading.stop,
        reading.message.as_deref().unwrap_or("")
    );
    out
}

fn readings(name: &str) -> String {
    let root = sample(name);
    let project = load_project(&root).unwrap();
    let ir = analyze(&project);
    let mut out = String::new();
    for choice in reading_choices(&ir) {
        let reading = read(&project, &ir, &choice.target, &Variables::new());
        let _ = writeln!(out, "=== {}", choice.label);
        out.push_str(&outline(&reading));
    }
    out
}

#[test]
fn houkago() {
    let outlines = readings("houkago");
    insta::assert_snapshot!("houkago_outlines", outlines);
    let root = sample("houkago");
    let project = load_project(&root).unwrap();
    let ir = analyze(&project);
    let target = ReadingTarget::Route {
        name: "灯".to_owned(),
    };
    let reading = read(&project, &ir, &target, &Variables::new());
    assert_eq!(reading.stop, "ending");
    let label = target_label(&ir, &target);
    assert_eq!(label, "灯ルート");
    insta::assert_snapshot!(
        "houkago_akari_text",
        plain_text(&project, &reading, &label, true)
    );
}

#[test]
fn amayadori() {
    let outlines = readings("amayadori");
    insta::assert_snapshot!("amayadori_outlines", outlines);
    let root = sample("amayadori");
    let project = load_project(&root).unwrap();
    let ir = analyze(&project);
    let target = ReadingTarget::Ending {
        id: "stay_end".to_owned(),
    };
    let reading = read(&project, &ir, &target, &Variables::new());
    assert_eq!(reading.stop, "ending");
    let label = target_label(&ir, &target);
    insta::assert_snapshot!(
        "amayadori_stay_text",
        plain_text(&project, &reading, &label, true)
    );
    // 注記なし。
    let bare = plain_text(&project, &reading, &label, false);
    assert!(!bare.contains("〔選択："));
    assert!(!bare.contains("〔エンディング："));
}

/// 選んだ選択肢の順（プレビューで選んだ道）。使い切ったら選べる最初の選択肢を選ぶ。
#[test]
fn steps() {
    let root = sample("amayadori");
    let project = load_project(&root).unwrap();
    let ir = analyze(&project);
    let all = read(&project, &ir, &ReadingTarget::Common, &Variables::new());
    // 共通だけ：最初の選択肢で志乃のシーンに入る手前で止まる。
    assert_eq!(all.stop, "route_changed");
    let choice_span = all
        .items
        .iter()
        .find_map(|i| match i {
            ReadingItem::Choice { span, .. } => Some(*span),
            _ => None,
        })
        .unwrap();
    let steps = vec![RouteStep {
        scene: "s02".to_owned(),
        choice: choice_span,
        option: 1,
    }];
    let reading = read(
        &project,
        &ir,
        &ReadingTarget::Steps { steps },
        &Variables::new(),
    );
    let outline = outline(&reading);
    assert!(outline.contains("選択：本を開く"), "{outline}");
    assert!(outline.contains("s04"), "{outline}");
    // 道を使い切った後は、選べる最初の選択肢（傘を差し出す）。
    assert!(outline.contains("選択：傘を差し出す"), "{outline}");
    assert_eq!(reading.stop, "ending");
}

/// 道がずれたとき（ADV のデバッグで飛んだ、本文が変わった）：今の選択肢に合う道を残りから探し、その前の道は捨てる。
/// 合う道が無ければ、選べる最初の選択肢を選び、道は残す。
#[test]
fn steps_skip_mismatched() {
    let root = sample("amayadori");
    let project = load_project(&root).unwrap();
    let ir = analyze(&project);
    let all = read(&project, &ir, &ReadingTarget::Common, &Variables::new());
    let choice_span = all
        .items
        .iter()
        .find_map(|i| match i {
            ReadingItem::Choice { span, .. } => Some(*span),
            _ => None,
        })
        .unwrap();
    let stray = RouteStep {
        scene: "s99".to_owned(),
        choice: choice_span,
        option: 0,
    };
    let real = RouteStep {
        scene: "s02".to_owned(),
        choice: choice_span,
        option: 1,
    };
    let reading = read(
        &project,
        &ir,
        &ReadingTarget::Steps {
            steps: vec![stray.clone(), real],
        },
        &Variables::new(),
    );
    let text = outline(&reading);
    assert!(text.contains("選択：本を開く"), "{text}");
    assert_eq!(reading.stop, "ending");
    // 合う道が無い：選べる最初の選択肢（傘を差し出す）で進む。
    let reading = read(
        &project,
        &ir,
        &ReadingTarget::Steps { steps: vec![stray] },
        &Variables::new(),
    );
    let text = outline(&reading);
    assert!(text.contains("選択：傘を差し出す"), "{text}");
}

/// `@呼出` で呼ばれたシーンには見出しを出さない（呼び出し元の本文に挟まる）。
#[test]
fn called_scene_has_no_heading() {
    let root = sample("amayadori");
    let project = load_project(&root).unwrap();
    let ir = analyze(&project);
    let target = ReadingTarget::Ending {
        id: "stay_end".to_owned(),
    };
    let reading = read(&project, &ir, &target, &Variables::new());
    let scenes: Vec<&str> = reading
        .items
        .iter()
        .filter_map(|i| match i {
            ReadingItem::Scene { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert!(!scenes.contains(&"s90"), "{scenes:?}");
}
