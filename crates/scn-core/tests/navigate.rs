//! 言語サーバーのためのコアの言語サービス（T3-15）：定義へ移動・参照・ホバー（`navigate`）と補完（`complete`）。
//! 入力は houkago のサンプル。位置は本文の中の文字列から探す。

use std::path::{Path, PathBuf};

use scn_core::complete::{CompletionKind, complete_at, suggest_scene_ids};
use scn_core::navigate::{Place, definition, hover, references};
use scn_core::{Ir, Project, analyze, load_project};

fn sample(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples")
        .join(name)
}

const CHAPTER: &str = "story/chapter01.scn";

fn houkago() -> (Project, Ir) {
    let project = load_project(&sample("houkago")).unwrap();
    let ir = analyze(&project);
    (project, ir)
}

fn text<'p>(project: &'p Project, file: &str) -> &'p str {
    &project.files.iter().find(|f| f.path == file).unwrap().text
}

/// `needle` の始まり（`skip` 文字目から）のバイト位置。
fn at(project: &Project, needle: &str, skip: usize) -> usize {
    let source = text(project, CHAPTER);
    let start = source
        .find(needle)
        .unwrap_or_else(|| panic!("{needle} が無い"));
    start + needle.chars().take(skip).map(char::len_utf8).sum::<usize>()
}

fn slice<'p>(project: &'p Project, place: &Place) -> &'p str {
    text(project, &place.file)
        .get(place.span.start..place.span.end)
        .unwrap()
}

#[test]
fn definitions() {
    let (project, ir) = houkago();
    // ジャンプの行き先から、シーンの見出しへ。
    let def = definition(&project, &ir, CHAPTER, at(&project, "-> s013a", 4)).unwrap();
    assert_eq!(def.file, "story/chapter01.scn");
    assert!(
        slice(&project, &def).starts_with("## 打ち明ける #s013a"),
        "{def:?}"
    );
    // 話者から、設定DBのエントリの name の項目へ。
    let def = definition(&project, &ir, CHAPTER, at(&project, "灯「やっと来た", 0)).unwrap();
    assert_eq!(def.file, "bible/entries/character/akari.md");
    // 変数（名前空間がキャラクター）から、variables.yaml の宣言へ。
    let def = definition(&project, &ir, CHAPTER, at(&project, "灯.好感度 >= 3", 3)).unwrap();
    assert_eq!(def.file, "variables.yaml");
    let variables = std::fs::read_to_string(sample("houkago").join("variables.yaml")).unwrap();
    assert!(
        variables
            .get(def.span.start..def.span.end)
            .is_some_and(|k| k.contains("好感度")),
        "{def:?}"
    );
    // 変数の名前空間のところは、キャラクター。
    let def = definition(&project, &ir, CHAPTER, at(&project, "灯.好感度 >= 3", 0)).unwrap();
    assert_eq!(def.file, "bible/entries/character/akari.md");
    // 地の文の中は何も無い。
    assert!(definition(&project, &ir, CHAPTER, at(&project, "坂道の途中", 1)).is_none());
}

#[test]
fn reference_lists() {
    let (project, ir) = houkago();
    let with = references(&project, &ir, CHAPTER, at(&project, "-> s012", 3), true);
    let texts: Vec<&str> = with.iter().map(|p| slice(&project, p)).collect();
    assert!(texts.iter().all(|t| *t == "s012"), "{texts:?}");
    assert!(with.len() >= 2, "見出しとジャンプ：{with:?}");
    // 定義を含めないときは、見出しのIDを除く。
    let without = references(&project, &ir, CHAPTER, at(&project, "-> s012", 3), false);
    assert_eq!(without.len(), with.len() - 1);
    // キャラクター：話者・立ち絵・変数の名前空間（書き方によらない）。定義を含めるとエントリが先頭。
    let refs = references(
        &project,
        &ir,
        CHAPTER,
        at(&project, "灯「やっと来た", 0),
        true,
    );
    assert_eq!(
        refs.first().map(|p| p.file.as_str()),
        Some("bible/entries/character/akari.md")
    );
    let rest: Vec<&str> = refs.iter().skip(1).map(|p| slice(&project, p)).collect();
    assert!(rest.iter().all(|t| *t == "灯"), "{rest:?}");
    assert!(rest.len() >= 5, "{rest:?}");
    // 変数：書いた変数全体。
    let refs = references(
        &project,
        &ir,
        CHAPTER,
        at(&project, "灯.好感度 += 2", 3),
        false,
    );
    let texts: Vec<&str> = refs.iter().map(|p| slice(&project, p)).collect();
    assert!(texts.contains(&"灯.好感度"), "{texts:?}");
}

#[test]
fn hovers() {
    let (project, ir) = houkago();
    let (text, span) = hover(&project, &ir, CHAPTER, at(&project, "-> s012", 4)).unwrap();
    assert!(text.starts_with("**放課後の屋上** `#s012`"), "{text}");
    assert!(text.contains("灯が転校を打ち明けかける"), "{text}");
    assert_eq!(
        slice(
            &project,
            &Place {
                file: CHAPTER.to_owned(),
                span
            }
        ),
        "s012"
    );
    let (text, _) = hover(&project, &ir, CHAPTER, at(&project, "灯「やっと来た", 0)).unwrap();
    assert!(text.starts_with("**灯** `akari`"), "{text}");
    let (text, _) = hover(&project, &ir, CHAPTER, at(&project, "勇気 += 1", 0)).unwrap();
    assert!(text.starts_with("**flag.勇気**"), "{text}");
    assert!(text.contains("型：int"), "{text}");
}

/// 本文の `line` の行末に `typed` を足したものとして補完する（カーソルは足した文字の後ろ）。
fn complete(project: &Project, ir: &Ir, typed: &str) -> Vec<(String, String, Option<usize>)> {
    let mut source = text(project, CHAPTER).to_owned();
    if !source.ends_with('\n') {
        source.push('\n');
    }
    source.push_str(typed);
    let offset = source.len();
    complete_at(project, ir, &source, offset)
        .into_iter()
        .map(|c| {
            assert_eq!(c.span.end, offset);
            (c.label, c.insert, c.cursor)
        })
        .collect()
}

fn labels(items: &[(String, String, Option<usize>)]) -> Vec<&str> {
    items.iter().map(|(l, _, _)| l.as_str()).collect()
}

#[test]
fn completions() {
    let (project, ir) = houkago();
    let items = complete(&project, &ir, "@背");
    assert_eq!(labels(&items), ["@背景"]);
    assert_eq!(items[0].1, "@背景 ");
    // 全角の ＠ も受ける。英語名でも引ける。
    assert_eq!(labels(&complete(&project, &ir, "＠bg")), ["@背景", "@BGM"]);
    let bgs = complete(&project, &ir, "@背景 屋");
    assert!(labels(&bgs).iter().all(|l| l.starts_with('屋')), "{bgs:?}");
    assert!(!bgs.is_empty());
    let exprs = complete(&project, &ir, "@立ち絵 灯 ");
    assert!(labels(&exprs).contains(&"笑顔"), "{exprs:?}");
    let positions = complete(&project, &ir, "@立ち絵 灯 笑顔　");
    assert_eq!(labels(&positions), ["左", "中", "右"]);
    let scenes = complete(&project, &ir, "-> s01");
    assert!(labels(&scenes).contains(&"s013a"), "{scenes:?}");
    assert!(labels(&scenes).iter().all(|l| l.starts_with("s01")));
    let vars = complete(&project, &ir, "@if 灯.好");
    assert_eq!(labels(&vars), ["灯.好感度"]);
    let ses = complete(&project, &ir, "風が{se ");
    assert!(labels(&ses).contains(&"風"), "{ses:?}");
    let ids = complete(&project, &ir, "## 新しい場面 #");
    assert!(!ids.is_empty());
    // 題名の無い見出し（`## ` の空白の後ろの `#`）では出さない（アプリと同じ）。
    assert!(complete(&project, &ir, "## #s0").is_empty());
    // `{se ` の後ろの全角の空白で止める。
    assert!(complete(&project, &ir, "{se 風　").is_empty());
    // 行頭のキャラ名：`名前「」`、カーソルは括弧の中。1文字の前方一致では出さない。
    let speaker = complete(&project, &ir, "灯");
    assert_eq!(
        speaker,
        [("灯".to_owned(), "灯「」".to_owned(), Some("灯「".len()))]
    );
    assert!(complete(&project, &ir, "あ").is_empty());
    assert_eq!(
        labels(&complete(&project, &ir, "あか")),
        ["灯"],
        "呼び名の前方一致"
    );
    let kinds: Vec<CompletionKind> = complete_at(&project, &ir, "@", 1)
        .into_iter()
        .map(|c| c.kind)
        .collect();
    assert!(kinds.iter().all(|k| *k == CompletionKind::Directive));
}

#[test]
fn scene_id_suggestions() {
    let used = ["s010", "s011", "s012", "s013a"].into_iter().collect();
    assert_eq!(suggest_scene_ids(&used, Some("s011")), ["s013"]);
    assert_eq!(suggest_scene_ids(&used, None), ["s013"]);
    assert_eq!(suggest_scene_ids(&used, Some("a7")), ["a8", "s013"]);
}
