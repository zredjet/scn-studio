//! 構文解析のテスト：fixtureと、サンプル作品のスナップショット。

mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use scn_core::{ParseOptions, Severity, parse};

/// 段階0の構文系の診断（計画書 T0-3 の完了条件）。
const SYNTAX_CODES: [&str; 21] = [
    "W101", "E102", "E103", "E104", "E105", "E106", "W107", "I109", "E110", "E111", "W112", "W113",
    "E114", "E201", "W303", "E304", "W401", "E402", "W405", "W406", "E601",
];

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixtures_dir() -> PathBuf {
    manifest_dir().join("tests/fixtures")
}

fn sample_dir() -> PathBuf {
    manifest_dir().join("../../samples/houkago")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("{} を読めない：{e}", path.display()))
}

fn fixture_files() -> Vec<PathBuf> {
    let mut files: Vec<_> = fs::read_dir(fixtures_dir())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "scn"))
        .collect();
    files.sort();
    files
}

/// 各fixtureで、出た診断のコードの集まりが `// expect:` と一致し、ASTと診断がスナップショットと一致する。
#[test]
fn fixtures() {
    insta::glob!("fixtures/*.scn", |path| {
        let source = read(path);
        let parsed = parse(&source, &ParseOptions::default());
        let expected: BTreeSet<String> = support::expected_codes(&source).into_iter().collect();
        let actual: BTreeSet<String> = parsed
            .diagnostics
            .iter()
            .map(|d| d.code.to_string())
            .collect();
        assert_eq!(
            actual,
            expected,
            "{}：診断のコードが期待と違う\n{}",
            path.display(),
            support::dump_diagnostics(&source, &parsed.diagnostics)
        );
        insta::assert_snapshot!(support::dump(&source, &parsed));
    });
}

/// 段階0の構文系の診断それぞれに、それを期待するfixtureがある。
#[test]
fn every_syntax_code_has_a_fixture() {
    let covered: BTreeSet<String> = fixture_files()
        .iter()
        .flat_map(|path| support::expected_codes(&read(path)))
        .collect();
    let missing: Vec<_> = SYNTAX_CODES
        .iter()
        .filter(|code| !covered.contains(**code))
        .collect();
    assert!(missing.is_empty(), "fixtureの無い診断：{missing:?}");
}

/// fixtureの一覧（README.md）の構文系の表と、置いてあるファイルと、`// expect:` が一致する。
#[test]
fn fixture_list_matches_files() {
    let readme = read(&fixtures_dir().join("README.md"));
    let section = readme
        .split("## 構文系（T0-3）")
        .nth(1)
        .and_then(|rest| rest.split("## 意味系").next())
        .expect("README.md に構文系の節が無い");

    let mut listed: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for row in section.lines().filter(|l| l.starts_with("| `")) {
        // 内容の列の `\|` は区切りではない。
        let row = row.replace("\\|", "\u{0}");
        let cells: Vec<&str> = row.split('|').map(str::trim).collect();
        let name = cells[1].trim_matches('`').to_owned();
        // 異常系の表は3列目に期待する診断がある。正常系は2列だけ。
        let codes = if cells.len() > 4 {
            cells[3].split_whitespace().map(str::to_owned).collect()
        } else {
            BTreeSet::new()
        };
        listed.insert(name, codes);
    }

    let mut actual: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for path in fixture_files() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let codes = support::expected_codes(&read(&path)).into_iter().collect();
        actual.insert(name, codes);
    }
    assert_eq!(listed, actual, "README.md の一覧とfixtureが合わない");
}

/// サンプル作品の各章を構文解析する。エラー（E）が0件で、スナップショットと一致する。
#[test]
fn samples() {
    let options = scn_core::load_project(&sample_dir())
        .expect("サンプルを読めない")
        .parse_options();
    insta::glob!("../../../samples/houkago/story", "*.scn", |path| {
        let source = read(path);
        let parsed = parse(&source, &options);
        let errors: Vec<_> = parsed
            .diagnostics
            .iter()
            .filter(|d| d.severity() == Severity::Error)
            .cloned()
            .collect();
        assert!(
            errors.is_empty(),
            "{}：エラーがある\n{}",
            path.display(),
            support::dump_diagnostics(&source, &errors)
        );
        insta::assert_snapshot!(support::dump(&source, &parsed));
    });
}

/// 既定の括弧（「」だけ）では、サンプルの『』の台詞は台詞候補にならない（仕様書6節）。
#[test]
fn default_brackets_do_not_include_double_quotes() {
    let source = "## a #s1\n灯『……やっぱり、言えないかも』 ^k3fh\n";
    let dump = support::dump(source, &parse(source, &ParseOptions::default()));
    assert!(dump.contains("narration L2 ^k3fh"), "{dump}");
}

/// 診断とASTのスパンが、範囲内で文字の境界にある（BOMとCRLFを含むfixtureも含めて）。
#[test]
fn spans_are_char_boundaries() {
    for path in fixture_files() {
        let source = read(&path);
        let parsed = parse(&source, &ParseOptions::default());
        for diag in &parsed.diagnostics {
            support::assert_span_ok(&source, diag.span, diag.code.as_str());
        }
        support::assert_ast_spans_ok(&source, &parsed.chapter);
    }
}

/// 行IDのスパンは `^` から行IDの終わりまでを指す。
#[test]
fn line_id_span_points_to_caret() {
    let source = "## a #s1\n灯「やあ」 ^k3f9  \n";
    let parsed = parse(source, &ParseOptions::default());
    let scn_core::ast::Stmt::Dialogue(dialogue) = &parsed.chapter.scenes[0].body[0] else {
        panic!("台詞候補にならない");
    };
    let scn_core::ast::LineIdSlot::Valid(id) = &dialogue.line_id else {
        panic!("行IDが無い");
    };
    assert_eq!(&source[id.span.start..id.span.end], "^k3f9");
}

/// 入れ子の深いブロックでも、最後まで解析できる。
#[test]
fn deeply_nested_blocks() {
    let mut source = String::from("## a #s1\n");
    for _ in 0..1000 {
        source.push_str("@if a\n@選択\n- x ^abcd\n");
    }
    let parsed = parse(&source, &ParseOptions::default());
    let unclosed = parsed
        .diagnostics
        .iter()
        .filter(|d| d.code.as_str() == "E103")
        .count();
    assert_eq!(unclosed, 2000);
}
