//! 表記の検査（検証の段階2。T3-3、D064）：fixture（`wording/<名前>/`。それぞれが1つのプロジェクト）と、サンプル。

mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use scn_core::check::check;
use scn_core::index::build_index;
use scn_core::ir::IrDiagnostic;
use scn_core::wording::wording;
use scn_core::{analyze, load_project};

/// 表記の検査の診断。
const WORDING_CODES: [&str; 3] = ["I204", "I205", "I206"];

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn wording_fixtures() -> Vec<(String, PathBuf)> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(fixtures_dir().join("wording"))
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

/// 段階0と1の診断と、表記の検査の診断。
fn run(dir: &Path) -> (Vec<IrDiagnostic>, Vec<IrDiagnostic>) {
    let project = load_project(dir).unwrap_or_else(|e| panic!("{}：{e}", dir.display()));
    let ir = analyze(&project);
    let index = build_index(&project, &ir);
    let mut diagnostics = ir.diagnostics.clone();
    diagnostics.extend(check(&project, &ir, &index));
    (diagnostics, wording(&project, &ir))
}

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
fn wording_fixtures_report_diagnostics() {
    let mut failures = Vec::new();
    for (name, dir) in wording_fixtures() {
        let (mut diagnostics, words) = run(&dir);
        diagnostics.extend(words);
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

/// 表記の検査の診断それぞれに、それを期待するfixtureがある。
#[test]
fn every_wording_code_has_a_fixture() {
    let covered: BTreeSet<String> = wording_fixtures()
        .iter()
        .flat_map(|(_, dir)| expected(dir))
        .collect();
    let missing: Vec<_> = WORDING_CODES
        .iter()
        .filter(|code| !covered.contains(**code))
        .collect();
    assert!(missing.is_empty(), "fixtureの無い診断：{missing:?}");
}

/// fixtureの一覧（README.md）の表記の表と、`wording/` に置いてあるものと、`// expect:` が一致する。
#[test]
fn wording_fixture_list_matches_files() {
    let readme = fs::read_to_string(fixtures_dir().join("README.md")).unwrap();
    let section = readme
        .split("## 表記の検査（T3-3、`wording/`）")
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
        .expect("README.md に表記の節が無い");
    let mut listed: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for row in section.lines().filter(|l| l.starts_with("| `")) {
        let cells: Vec<&str> = row.split('|').map(str::trim).collect();
        let name = cells[1].trim_matches('`').trim_end_matches('/').to_owned();
        let codes = cells[3].split_whitespace().map(str::to_owned).collect();
        listed.insert(name, codes);
    }
    let actual: BTreeMap<String, BTreeSet<String>> = wording_fixtures()
        .into_iter()
        .map(|(name, dir)| (name, expected(&dir)))
        .collect();
    assert_eq!(listed, actual, "README.md の一覧と表記のfixtureが合わない");
}

/// 完了条件：サンプルで意図しない診断が出ない。ゲートの短編は0件、houkago は README の「段階2で出る診断」の
/// 一人称の表記ゆれ（`^k3ff` の「私」）だけ。
#[test]
fn samples() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples");
    let (_, words) = run(&root.join("amayadori"));
    assert!(words.is_empty(), "{words:?}");
    let dir = root.join("houkago");
    let (_, words) = run(&dir);
    insta::assert_snapshot!("sample_houkago", dump(&words, &dir));
    assert_eq!(words.iter().map(|d| d.code).collect::<Vec<_>>(), ["I204"]);
}

/// 大きな作品でも速く終わる（行ごとに、かなをそろえた形を1度だけ作り、用語は最初の文字で引く）。
#[test]
fn large_project_is_fast() {
    let dir = std::env::temp_dir().join(format!("scn-core-wording-large-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("story")).unwrap();
    fs::create_dir_all(dir.join("bible/entries/character")).unwrap();
    fs::write(dir.join("project.toml"), "format_version = 0\n").unwrap();
    let kana = "あいうえおかきくけこさしすせそたちつてとなにぬねの";
    let kana: Vec<char> = kana.chars().collect();
    for i in 0..100 {
        let alias: String = [
            kana[i % 25],
            kana[(i * 7 + 3) % 25],
            kana[(i * 11 + 5) % 25],
        ]
        .iter()
        .collect();
        fs::write(
            dir.join(format!("bible/entries/character/c{i:03}.md")),
            format!("---\nid: c{i:03}\nname: 人物{i}\naliases: [{alias}]\nfirst_person: わたし\nvariants: [人物{i}さま]\n---\n"),
        )
        .unwrap();
    }
    let mut text = String::from("## a #s1\n");
    for i in 0..5_000 {
        text.push_str(&format!(
            "人物{}「わたしは、きのうアイウエオの店で{}を見た。そこへ行こう」 ^{:04x}\n",
            i % 100,
            "カキクケコ",
            i + 0x1000
        ));
    }
    text.push_str("@エンディング end1\n");
    fs::write(dir.join("story/a.scn"), text).unwrap();
    let project = load_project(&dir).unwrap();
    let ir = analyze(&project);
    let started = std::time::Instant::now();
    let words = wording(&project, &ir);
    let elapsed = started.elapsed();
    let _ = fs::remove_dir_all(&dir);
    assert!(elapsed < std::time::Duration::from_secs(10), "{elapsed:?}");
    assert!(
        words.iter().all(|d| d.code == "I206"),
        "{:?}",
        words.first()
    );
}
