//! 任意の入力でパニックしないことを確かめる（`proptest`）。
//!
//! 行の断片をでたらめに組み合わせた入力、途中で切った入力、任意の文字列を解析し、
//! パニックしないこと、診断とASTのスパンが範囲内で文字の境界にあることを確かめる。
//! 構文解析だけでなく、意味解析とIR（`analyze`）も通す。

mod support;

use std::path::PathBuf;
use std::sync::LazyLock;

use proptest::prelude::*;
use proptest::sample::Index;
use scn_core::project::{Project, SourceFile};
use scn_core::{DialogueBracket, ParseOptions, analyze, load_project, parse};

/// 意味系のfixtureで使う共通のプロジェクト。本文だけを差し替えて、意味解析とIRも通す。
static PROJECT: LazyLock<Project> = LazyLock::new(|| {
    load_project(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project"))
        .expect("共通のプロジェクトを読めない")
});

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
