//! 任意の入力でパニックしないことを確かめる（`proptest`）。
//!
//! 行の断片をでたらめに組み合わせた入力、途中で切った入力、任意の文字列を解析し、
//! パニックしないこと、診断とASTのスパンが範囲内で文字の境界にあることを確かめる。
//! 構文解析だけでなく、意味解析とIR（`analyze`）、索引、検証の段階1（`check`）も通す。
//! 行IDの付与（`scn fmt`）も、同じ入力で確かめる（足した部分のほかはバイトが変わらない、など）。
//! 素材の一覧と発注リスト（`assets`）、カーソル位置から再生の最短経路（`route`）、Ren'Py への書き出し（`renpy`）も、
//! 同じ入力でパニックしないことを確かめる。

mod support;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::LazyLock;

use proptest::prelude::*;
use proptest::sample::Index;
use scn_core::assets::{AssetState, asset_list, orders};
use scn_core::check::check as check_project;
use scn_core::index::build_index;
use scn_core::line_id::{LineIdGenerator, assign_line_ids};
use scn_core::plaintext::plain_text;
use scn_core::project::{MessageWindow, Project, SourceFile};
use scn_core::renpy::{RenpyProfile, export_renpy};
use scn_core::route::{ReadingTarget, read, reading_choices, route_at};
use scn_core::{DialogueBracket, ParseOptions, analyze, load_project, parse};

/// 意味系のfixtureで使う共通のプロジェクト。本文だけを差し替えて、意味解析とIRも通す。
static PROJECT: LazyLock<Project> = LazyLock::new(|| {
    load_project(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project"))
        .expect("共通のプロジェクトを読めない")
});

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project")
}

fn options() -> ParseOptions {
    ParseOptions {
        brackets: vec![
            DialogueBracket {
                open: '「',
                close: '」',
                style: "normal".into(),
            },
            DialogueBracket {
                open: '『',
                close: '』',
                style: "thought".into(),
            },
        ],
    }
}

fn check(source: &str) {
    for options in [ParseOptions::default(), options()] {
        let parsed = parse(source, &options);
        for diag in &parsed.diagnostics {
            support::assert_span_ok(source, diag.span, diag.code.as_str());
        }
        support::assert_ast_spans_ok(source, &parsed.chapter);
        // 書き出し（ASTを辿り、診断の位置を行と列にする）でもパニックしない。
        support::dump(source, &parsed);
    }

    // 意味解析とIR。JSONにもできる。
    let mut project = PROJECT.clone();
    project.files = vec![SourceFile {
        path: "story/a.scn".into(),
        text: source.to_owned(),
    }];
    let ir = analyze(&project);
    for diag in &ir.diagnostics {
        support::assert_span_ok(source, diag.span, diag.code);
    }
    serde_json::to_string(&ir).expect("IRをJSONにできない");

    // 索引と検証の段階1（はみ出しも調べるよう、メッセージウィンドウを決める）。
    let mut windowed = project.clone();
    windowed.settings.message_window = Some(MessageWindow {
        lines: 3,
        chars_per_line: 24,
    });
    let ir = analyze(&windowed);
    let index = build_index(&windowed, &ir);
    serde_json::to_string(&index).expect("索引をJSONにできない");
    for diag in check_project(&windowed, &ir, &index) {
        if diag.file == "story/a.scn" {
            support::assert_span_ok(source, diag.span, diag.code);
        }
    }

    // 素材の一覧と発注リスト。
    let list = asset_list(&windowed, &fixture_root(), &index);
    assert_eq!(
        orders(&list).len(),
        list.rows
            .iter()
            .filter(|r| r.state != AssetState::File)
            .count()
    );

    // カーソル位置から再生の最短経路（行の始まりごと。探す数に上限があるので、必ず終わる）。
    let mut at = 0;
    for line in source.split_inclusive('\n') {
        if let Some(route) = route_at(&windowed, &ir, "story/a.scn", at, &Default::default()) {
            assert!(route.reachable || route.message.is_some());
        }
        at += line.len();
    }

    // 通読とプレーンテキスト（道の候補のすべて）。
    for choice in reading_choices(&ir) {
        let reading = read(&windowed, &ir, &choice.target, &Default::default());
        let _ = plain_text(&windowed, &reading, &choice.label, true);
    }
    let reading = read(&windowed, &ir, &ReadingTarget::Common, &Default::default());
    let _ = plain_text(&windowed, &reading, "共通", false);

    // Ren'Py への書き出し（定義のファイルは必ず書く）。
    let export = export_renpy(&windowed, &fixture_root(), &ir, &RenpyProfile::default());
    assert!(!export.files.is_empty());

    check_line_ids(&project);
}

/// I302 のほかの診断のコードと件数。
fn codes_without_i302(project: &Project) -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::new();
    for diag in analyze(project).diagnostics {
        if diag.code != "I302" {
            *counts.entry(diag.code).or_default() += 1;
        }
    }
    counts
}

/// 行IDの付与（仕様書9節、D041）。I302 の出る行にだけ付け、足した ` ^ID` のほかはバイトが変わらない。
/// 付けた後は I302 が無くなり、ほかの診断は変わらず、もう一度付けても何も変えない。
fn check_line_ids(project: &Project) {
    let source = &project.files[0].text;
    let results = assign_line_ids(project, &mut LineIdGenerator::with_seed(0));
    let i302_ends: Vec<usize> = analyze(project)
        .diagnostics
        .iter()
        .filter(|d| d.code == "I302")
        .map(|d| d.span.end)
        .collect();
    let Some(result) = results.first() else {
        assert!(i302_ends.is_empty(), "I302 があるのに付けなかった");
        return;
    };
    let mut offsets: Vec<usize> = result.assigned.iter().map(|a| a.offset).collect();
    let mut expected_offsets = i302_ends;
    expected_offsets.sort_unstable();
    offsets.sort_unstable();
    assert_eq!(
        offsets, expected_offsets,
        "付けた位置が I302 の行と合わない"
    );

    let mut rebuilt = String::new();
    let mut from = 0;
    for a in &result.assigned {
        rebuilt.push_str(&source[from..a.offset]);
        rebuilt.push_str(" ^");
        rebuilt.push_str(&a.id);
        from = a.offset;
    }
    rebuilt.push_str(&source[from..]);
    assert_eq!(result.text, rebuilt, "足した部分のほかのバイトが変わった");

    let mut after = project.clone();
    after.files[0].text.clone_from(&result.text);
    assert!(
        analyze(&after).diagnostics.iter().all(|d| d.code != "I302"),
        "付けた後に I302 が残る"
    );
    assert_eq!(codes_without_i302(&after), codes_without_i302(project));
    assert!(
        assign_line_ids(&after, &mut LineIdGenerator::with_seed(1)).is_empty(),
        "2回目に何かを変えた"
    );
}

/// `.scn` によく現れる行の断片。
fn fragment() -> impl Strategy<Value = String> {
    let fixed = prop::sample::select(vec![
        "",
        "# 第1章",
        "## 見出し #s1",
        "## 見出し #s2",
        "## 見出し",
        "## 見出し #帰り道",
        "---",
        "summary: 要約",
        "status: draft",
        "status: x",
        "tags: [a, b]",
        "summary 崩れ",
        "// コメント",
        "@背景 屋上/夕方 フェード",
        "@立ち絵 灯 笑顔 左 フェード",
        "@立ち絵 灯",
        "@BGM 停止",
        "@待機 長め",
        "@効果 揺れ 0.5",
        "@設定 灯.好感度 += 1",
        "@設定 (",
        "@選択 どうする{w 0.5}",
        "@選択",
        "@if 勇気 >= 2",
        "@if (",
        "@if",
        "@elif not a",
        "@else",
        "@end",
        "@raw renpy",
        "@raw",
        "@エンディング end1",
        "@エンディング 灯",
        "@回想",
        "@bg \"屋上",
        "- 行く if 勇気 >= 2 -> s2",
        "- ",
        "-",
        "- if",
        "- ->",
        "-> s2",
        "->",
        "灯「やあ」",
        "灯「開いたまま",
        "灯（？？？）「誰？」",
        "灯（「",
        "灯『心の声』",
        "閉じる」",
        "」",
        "「「「",
        "彼は「行こう」と言った。",
        "　地の文|乱《みだ》れ《《傍点》》{p}{se ドア}{speed 遅}{=x + 1}{{}}",
        "漢字《かんじ》《",
        "|《",
        "{w",
        "{=",
        "{= (}",
        "\\@背景",
        "\\",
        "^abcd",
        " ^abcd",
        "\u{feff}",
        "\r",
        "\t",
        "　",
    ])
    .prop_map(str::to_owned);
    let indent = prop::sample::select(vec!["", "  ", "\t", "　"]);
    let line_id = prop::sample::select(vec!["", " ^k3f9", " ^K3F9", " ^ab", " ^_^", " ^abcd  "]);
    prop_oneof![
        4 => (indent, fixed, line_id).prop_map(|(i, f, id)| format!("{i}{f}{id}")),
        1 => "\\PC{0,12}",
    ]
}

// 件数は proptest の既定（256件）。多く試すときは環境変数 `PROPTEST_CASES` で増やす。
proptest! {
    #[test]
    fn random_lines(lines in prop::collection::vec(fragment(), 0..40), cut in any::<Index>()) {
        let source = lines.join("\n");
        check(&source);
        let mut at = cut.index(source.len() + 1);
        while !source.is_char_boundary(at) {
            at -= 1;
        }
        check(&source[..at]);
        check(&lines.join("\r\n"));
    }

    #[test]
    fn arbitrary_text(source in "\\PC{0,200}") {
        check(&source);
    }

    #[test]
    fn arbitrary_chars(source in "[ -~\n\r\t　「」『』（）《》|｜{}@#^\\-\"]{0,200}") {
        check(&source);
    }
}
