//! 検証の段階2（自動巡回。T3-2、D063）：fixture（`playthrough/<名前>/`。それぞれが1つのプロジェクト）と、
//! サンプル、状態の上限。

mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use scn_core::check::check;
use scn_core::index::build_index;
use scn_core::ir::IrDiagnostic;
use scn_core::playthrough::{Playthrough, example_text, playthrough};
use scn_core::{analyze, load_project};

/// 検証の段階2の診断（仕様書16節の「段階」が2のもの）。
const STAGE2_CODES: [&str; 5] = ["W506", "W507", "W508", "W509", "W704"];

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn sample(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples")
        .join(name)
}

fn playthrough_fixtures() -> Vec<(String, PathBuf)> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(fixtures_dir().join("playthrough"))
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

/// プロジェクトの診断すべて（段階0・1・2）と、自動巡回の結果。
fn run(dir: &Path) -> (Vec<IrDiagnostic>, Playthrough) {
    let project = load_project(dir).unwrap_or_else(|e| panic!("{}：{e}", dir.display()));
    let ir = analyze(&project);
    let index = build_index(&project, &ir);
    let mut diagnostics = ir.diagnostics.clone();
    diagnostics.extend(check(&project, &ir, &index));
    let report = playthrough(&project, &ir);
    diagnostics.extend(report.diagnostics.iter().cloned());
    (diagnostics, report)
}

/// 診断と、自動巡回の結果の要約（カバレッジとエンディングごとの到達条件）を書き出す。
fn dump(diagnostics: &[IrDiagnostic], report: &Playthrough, dir: &Path) -> String {
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
    if diagnostics.is_empty() {
        out.push_str("診断なし\n");
    }
    out.push_str(&format!(
        "調べきった：{}　状態 {}　周回の始め方 {}　シーン {}/{}　選べた選択肢 {}/{}\n",
        report.complete,
        report.states,
        report.starts,
        report.scenes.reached,
        report.scenes.total,
        report.options.reached,
        report.options.total
    ));
    for ending in &report.endings {
        let values: Vec<String> = ending
            .values
            .iter()
            .map(|(k, v)| format!("{k}={}", serde_json::to_string(v).unwrap()))
            .collect();
        out.push_str(&format!(
            "エンディング {}（{}）：着く {}　状態 {}　{}　{}\n",
            ending.id,
            ending.scene,
            ending.reached,
            ending.states,
            if ending.reached {
                example_text(&ending.example)
            } else {
                "—".to_owned()
            },
            values.join(" ")
        ));
    }
    out
}

/// 出た診断のコードの集まりが `// expect:` と一致し、診断と結果がスナップショットと一致する。
#[test]
fn playthrough_fixtures_report_diagnostics() {
    let mut failures = Vec::new();
    for (name, dir) in playthrough_fixtures() {
        let (diagnostics, report) = run(&dir);
        let expected = expected(&dir);
        let actual: BTreeSet<String> = diagnostics.iter().map(|d| d.code.to_owned()).collect();
        if actual != expected {
            failures.push(format!(
                "{name}：期待 {expected:?}、実際 {actual:?}\n{}",
                dump(&diagnostics, &report, &dir)
            ));
        }
        assert!(report.complete, "{name}");
        insta::assert_snapshot!(name.as_str(), dump(&diagnostics, &report, &dir));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// 検証の段階2の診断それぞれに、それを期待するfixtureがある。
#[test]
fn every_stage2_code_has_a_fixture() {
    let covered: BTreeSet<String> = playthrough_fixtures()
        .iter()
        .flat_map(|(_, dir)| expected(dir))
        .collect();
    let missing: Vec<_> = STAGE2_CODES
        .iter()
        .filter(|code| !covered.contains(**code))
        .collect();
    assert!(missing.is_empty(), "fixtureの無い診断：{missing:?}");
}

/// fixtureの一覧（README.md）の段階2の表と、`playthrough/` に置いてあるものと、`// expect:` が一致する。
#[test]
fn playthrough_fixture_list_matches_files() {
    let readme = fs::read_to_string(fixtures_dir().join("README.md")).unwrap();
    let section = readme
        .split("## 検証の段階2（T3-2、`playthrough/`）")
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
        .expect("README.md に段階2の節が無い");
    let mut listed: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for row in section.lines().filter(|l| l.starts_with("| `")) {
        let cells: Vec<&str> = row.split('|').map(str::trim).collect();
        let name = cells[1].trim_matches('`').trim_end_matches('/').to_owned();
        let codes = cells[3].split_whitespace().map(str::to_owned).collect();
        listed.insert(name, codes);
    }
    let actual: BTreeMap<String, BTreeSet<String>> = playthrough_fixtures()
        .into_iter()
        .map(|(name, dir)| (name, expected(&dir)))
        .collect();
    assert_eq!(listed, actual, "README.md の一覧と段階2のfixtureが合わない");
}

/// 完了条件：ゲートの短編の3つのエンディングに選択肢だけで着き、段階2の診断は出ない（`scn check` で0件の
/// 約束）。サンプル「放課後の灯」の bad_end には選択肢だけでは着かない（D050）。houkago の README の
/// 「段階2で出る診断」のとおりに出る。
#[test]
fn samples() {
    let dir = sample("amayadori");
    let (_, report) = run(&dir);
    assert!(report.complete);
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    let reached: Vec<&str> = report
        .endings
        .iter()
        .filter(|e| e.reached)
        .map(|e| e.id.as_str())
        .collect();
    assert_eq!(reached, ["bus_end", "rain_end", "stay_end"]);
    assert_eq!(report.scenes.reached, report.scenes.total);

    let dir = sample("houkago");
    let (diagnostics, report) = run(&dir);
    assert!(report.complete);
    let ending = |id: &str| report.endings.iter().find(|e| e.id == id).unwrap();
    assert!(ending("akari_end").reached && ending("tsumugi_end").reached);
    assert!(!ending("bad_end").reached);
    let stage2: Vec<(&str, &str)> = report
        .diagnostics
        .iter()
        .map(|d| (d.code, d.file.as_str()))
        .collect();
    assert_eq!(
        stage2,
        [
            ("W506", "story/chapter01.scn"),
            ("W509", "story/chapter01.scn"),
            ("W509", "story/chapter03.scn")
        ]
    );
    insta::assert_snapshot!("sample_houkago", dump(&diagnostics, &report, &dir));
}

/// 状態の上限：分かれ道が多すぎる作品でも止まらずに終わり、調べきれなかったことを書く。「無い」と言う診断
/// （W506・W507・W509）は出さない。
#[test]
fn state_limit() {
    let dir = std::env::temp_dir().join(format!("scn-core-playthrough-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("story")).unwrap();
    fs::write(dir.join("project.toml"), "format_version = 0\n").unwrap();
    // 20 個の2択。それぞれ別の変数に書き、最後に全部を条件で読む（状態は 2^20 通り）。
    let mut vars = String::new();
    let mut text = String::from("## 始まり #s1\n");
    let mut all = Vec::new();
    for i in 0..20 {
        vars.push_str(&format!("flag.v{i}:\n  type: bool\n  default: false\n"));
        text.push_str(&format!(
            "@選択 {i} ^q{i:03}\n- はい ^y{i:03}\n  @設定 flag.v{i} = true\n- いいえ ^n{i:03}\n@end\n"
        ));
        all.push(format!("flag.v{i}"));
    }
    text.push_str(&format!(
        "@if {}\n  @エンディング all\n@end\n@if flag.v0 and not flag.v0\n  @エンディング never\n@end\n@エンディング end1\n",
        all.join(" and ")
    ));
    fs::write(dir.join("variables.yaml"), vars).unwrap();
    fs::write(dir.join("story/a.scn"), text).unwrap();
    let project = load_project(&dir).unwrap();
    let ir = analyze(&project);
    let report = playthrough(&project, &ir);
    assert!(!report.complete);
    assert!(
        report
            .limit
            .as_deref()
            .unwrap()
            .contains("分かれ道が多すぎる")
    );
    assert!(
        report
            .diagnostics
            .iter()
            .all(|d| !matches!(d.code, "W506" | "W507" | "W509")),
        "{:?}",
        report.diagnostics
    );
    let _ = fs::remove_dir_all(&dir);
}

/// 作品フォルダを一時に作って自動巡回する。
fn temp_playthrough(tag: &str, variables: &str, story: &str) -> Playthrough {
    let dir =
        std::env::temp_dir().join(format!("scn-core-playthrough-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("story")).unwrap();
    fs::write(dir.join("project.toml"), "format_version = 0\n").unwrap();
    fs::write(dir.join("variables.yaml"), variables).unwrap();
    fs::write(dir.join("story/a.scn"), story).unwrap();
    let project = load_project(&dir).unwrap();
    let ir = analyze(&project);
    let report = playthrough(&project, &ir);
    let _ = fs::remove_dir_all(&dir);
    report
}

/// よくある形で上限に当たる作品（ハブを回るたびに数える）でも、上限まで調べて速く終わる（エンディングに
/// 着くたびに道の例を作り直さない、伏線の道を探すときに道を複製しない）。
#[test]
fn hub_counting_stops_quickly() {
    let started = std::time::Instant::now();
    let report = temp_playthrough(
        "hub",
        "akari.好感度:\n  type: int\n  default: 0\n",
        "## 放課後 #s1\n@選択 どうする ^aaaa\n- 灯に話しかける ^aaab\n  @設定 akari.好感度 += 1\n  -> s1\n- 帰る -> s2 ^aaac\n@end\n\n## 結末 #s2\n@if akari.好感度 >= 5\n  @エンディング good\n@else\n  @エンディング normal\n@end\n",
    );
    assert!(!report.complete);
    assert!(
        report
            .limit
            .as_deref()
            .unwrap()
            .contains("分かれ道が多すぎる")
    );
    let good = report.endings.iter().find(|e| e.id == "good").unwrap();
    assert!(good.reached && good.example.len() == 6, "{good:?}");
    // 値は20個まで書き、書ききれなかった変数を知らせる。
    assert_eq!(good.values["akari.好感度"].len(), 20);
    assert_eq!(good.more, ["akari.好感度"]);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "{:?}",
        started.elapsed()
    );
}

/// `@呼出` の再帰：呼び出しの深さの上限で止めて、調べきれなかったことを書く（メモリを使い続けない）。
#[test]
fn call_recursion_stops() {
    let report = temp_playthrough(
        "call",
        "",
        "## 始まり #s1\n@選択 どうする ^aaaa\n- もう一度 ^aaab\n  @呼出 s1\n- やめる ^aaac\n  @エンディング end1\n@end\n@戻る\n",
    );
    assert!(!report.complete);
    assert!(
        report.limit.as_deref().unwrap().contains("呼び出し"),
        "{report:?}"
    );
    assert!(report.endings.iter().any(|e| e.id == "end1" && e.reached));
}
