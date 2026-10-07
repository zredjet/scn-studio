//! 検証の段階1（T1-4、`scn check`）：fixture（`check/<名前>/`。それぞれが1つのプロジェクト）。

mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use scn_core::check::check;
use scn_core::index::build_index;
use scn_core::ir::IrDiagnostic;
use scn_core::{analyze, load_project};

/// 検証の段階1の診断（仕様書16節の「段階」が1のもの）。
const CHECK_CODES: [&str; 9] = [
    "W203", "W503", "W504", "W505", "W605", "W606", "W701", "W702", "W703",
];

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn check_fixtures() -> Vec<(String, PathBuf)> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(fixtures_dir().join("check"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs.into_iter()
        .map(|p| (p.file_name().unwrap().to_string_lossy().into_owned(), p))
        .collect()
}

fn expected(dir: &Path) -> BTreeSet<String> {
    support::expected_codes(&fs::read_to_string(dir.join("story/a.scn")).unwrap())
        .into_iter()
        .collect()
}

/// プロジェクトの診断すべて（読み込み・構文解析・意味解析と、検証の段階1）。
fn all_diagnostics(dir: &Path) -> Vec<IrDiagnostic> {
    let project = load_project(dir).unwrap_or_else(|e| panic!("{}：{e}", dir.display()));
    let ir = analyze(&project);
    let index = build_index(&project, &ir);
    let mut diagnostics = ir.diagnostics.clone();
    diagnostics.extend(check(&project, &ir, &index));
    diagnostics
}

/// 診断を `ファイル:行:列 コード メッセージ` の形で書き出す。位置は、ディスクのファイルで数える。
fn dump(diagnostics: &[IrDiagnostic], dir: &Path) -> String {
    let mut out = String::new();
    for diag in diagnostics {
        let text = fs::read(dir.join(&diag.file))
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        let index = scn_core::LineIndex::new(&text);
        out.push_str(&format!(
            "{}:{} {} {}\n",
            diag.file,
            support::range(&index, diag.span),
            diag.code,
            diag.message
        ));
    }
    if out.is_empty() {
        out.push_str("診断なし\n");
    }
    out
}

/// 出た診断のコードの集まりが `// expect:` と一致し、診断がスナップショットと一致する。
#[test]
fn check_fixtures_report_diagnostics() {
    let mut failures = Vec::new();
    for (name, dir) in check_fixtures() {
        let diagnostics = all_diagnostics(&dir);
        let expected = expected(&dir);
        let actual: BTreeSet<String> = diagnostics.iter().map(|d| d.code.to_owned()).collect();
        if actual != expected {
            failures.push(format!(
                "{name}：期待 {expected:?}、実際 {actual:?}\n{}",
                dump(&diagnostics, &dir)
            ));
        }
        insta::assert_snapshot!(name.as_str(), dump(&diagnostics, &dir));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// 検証の段階1の診断それぞれに、それを期待するfixtureがある。
#[test]
fn every_check_code_has_a_fixture() {
    let covered: BTreeSet<String> = check_fixtures()
        .iter()
        .flat_map(|(_, dir)| expected(dir))
        .collect();
    let missing: Vec<_> = CHECK_CODES
        .iter()
        .filter(|code| !covered.contains(**code))
        .collect();
    assert!(missing.is_empty(), "fixtureの無い診断：{missing:?}");
}

/// fixtureの一覧（README.md）の検証の表と、`check/` に置いてあるものと、`// expect:` が一致する。
#[test]
fn check_fixture_list_matches_files() {
    let readme = fs::read_to_string(fixtures_dir().join("README.md")).unwrap();
    let section = readme
        .split("## 検証の段階1（T1-4、`check/`）")
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
        .expect("README.md に検証の節が無い");
    let mut listed: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for row in section.lines().filter(|l| l.starts_with("| `")) {
        let cells: Vec<&str> = row.split('|').map(str::trim).collect();
        let name = cells[1].trim_matches('`').trim_end_matches('/').to_owned();
        let codes = cells[3].split_whitespace().map(str::to_owned).collect();
        listed.insert(name, codes);
    }
    let actual: BTreeMap<String, BTreeSet<String>> = check_fixtures()
        .into_iter()
        .map(|(name, dir)| (name, expected(&dir)))
        .collect();
    assert_eq!(listed, actual, "README.md の一覧と検証のfixtureが合わない");
}

/// 完了条件：サンプル作品の検証で、README の「フェーズ1以降の検証で出る予定」とした段階1の診断
/// （`s014` の W503、`true_end` の W701、`^k3ff` のはみ出し W203）が出る。
#[test]
fn sample_check() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago");
    let diagnostics = all_diagnostics(&dir);
    insta::assert_snapshot!("sample_houkago", dump(&diagnostics, &dir));
    let has = |code: &str, needle: &str| {
        diagnostics
            .iter()
            .any(|d| d.code == code && d.message.contains(needle))
    };
    assert!(has("W503", "s014"));
    assert!(has("W701", "true_end"));
    let k3ff = fs::read_to_string(dir.join("story/chapter01.scn")).unwrap();
    let k3ff_line = k3ff.lines().position(|l| l.ends_with("^k3ff")).unwrap() + 1;
    assert!(diagnostics.iter().any(|d| {
        d.code == "W203"
            && d.file == "story/chapter01.scn"
            && scn_core::LineIndex::new(&k3ff).line_col(d.span.start).0 == k3ff_line
    }));
}
