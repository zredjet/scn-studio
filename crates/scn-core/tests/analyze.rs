//! 意味解析とIRのテスト：意味系のfixture、サンプル作品のIR、プロジェクトの読み込み。

mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use scn_core::ir::{Ir, Op};
use scn_core::project::{SourceFile, VarType};
use scn_core::{ProjectError, analyze, load_project};

/// 段階0の意味系の診断（計画書 T0-4 の完了条件）。
const SEMANTIC_CODES: [&str; 11] = [
    "E108", "W202", "E301", "I302", "I403", "W404", "E501", "W502", "W602", "E603", "E604",
];

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn sample_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("{} を読めない：{e}", path.display()))
}

fn sorted_entries(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    paths.sort();
    paths
}

/// 意味系のfixture。1つのファイルか、ファイルをまとめたディレクトリ。
struct SemaFixture {
    name: String,
    files: Vec<SourceFile>,
}

fn sema_fixtures() -> Vec<SemaFixture> {
    let mut fixtures = Vec::new();
    for path in sorted_entries(&fixtures_dir().join("sema")) {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let paths = if path.is_dir() {
            sorted_entries(&path)
        } else {
            vec![path]
        };
        let files = paths
            .iter()
            .filter(|p| p.extension().is_some_and(|e| e == "scn"))
            .map(|p| SourceFile {
                path: p.file_name().unwrap().to_string_lossy().into_owned(),
                text: read(p),
            })
            .collect();
        fixtures.push(SemaFixture { name, files });
    }
    fixtures
}

/// 診断を `ファイル:行:列 コード メッセージ` の形で書き出す。
fn dump_diagnostics(ir: &Ir, files: &[SourceFile]) -> String {
    let mut out = String::new();
    for diag in &ir.diagnostics {
        let source = files
            .iter()
            .find(|f| f.path == diag.file)
            .map(|f| f.text.as_str())
            .unwrap_or("");
        let index = scn_core::LineIndex::new(source);
        out.push_str(&format!(
            "{}:{} {} {}\n",
            diag.file,
            support::range(&index, diag.span),
            diag.code,
            diag.message
        ));
    }
    out
}

fn snapshot_text(ir: &Ir, files: &[SourceFile]) -> String {
    format!(
        "# diagnostics\n{}\n# ir\n{}\n",
        dump_diagnostics(ir, files),
        serde_json::to_string_pretty(ir).unwrap()
    )
}

/// 各fixtureで、出た診断のコードの集まりが `// expect:` と一致し、IRと診断がスナップショットと一致する。
#[test]
fn semantic_fixtures() {
    let base = load_project(&fixtures_dir().join("project")).expect("共通のプロジェクトを読めない");
    assert!(base.files.is_empty(), "共通のプロジェクトに本文を置かない");
    let mut failures = Vec::new();
    for fixture in sema_fixtures() {
        let mut project = base.clone();
        project.files = fixture.files;
        let ir = analyze(&project);
        let expected: BTreeSet<String> = project
            .files
            .iter()
            .flat_map(|f| support::expected_codes(&f.text))
            .collect();
        let actual: BTreeSet<String> = ir.diagnostics.iter().map(|d| d.code.to_owned()).collect();
        if actual != expected {
            failures.push(format!(
                "{}：期待 {expected:?}、実際 {actual:?}\n{}",
                fixture.name,
                dump_diagnostics(&ir, &project.files)
            ));
        }
        insta::assert_snapshot!(
            format!("sema__{}", fixture.name),
            snapshot_text(&ir, &project.files)
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// 段階0の意味系の診断それぞれに、それを期待するfixtureがある。
#[test]
fn every_semantic_code_has_a_fixture() {
    let covered: BTreeSet<String> = sema_fixtures()
        .iter()
        .flat_map(|f| {
            f.files
                .iter()
                .flat_map(|file| support::expected_codes(&file.text))
        })
        .collect();
    let missing: Vec<_> = SEMANTIC_CODES
        .iter()
        .filter(|code| !covered.contains(**code))
        .collect();
    assert!(missing.is_empty(), "fixtureの無い診断：{missing:?}");
}

/// fixtureの一覧（README.md）の意味系の表と、`sema/` に置いてあるものと、`// expect:` が一致する。
#[test]
fn semantic_fixture_list_matches_files() {
    let readme = read(&fixtures_dir().join("README.md"));
    let section = readme
        .split("## 意味系（T0-4、`sema/`）")
        .nth(1)
        .and_then(|rest| rest.split("## 段階0の診断との対応").next())
        .expect("README.md に意味系の節が無い");
    let mut listed: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for row in section.lines().filter(|l| l.starts_with("| `")) {
        let row = row.replace("\\|", "\u{0}");
        let cells: Vec<&str> = row.split('|').map(str::trim).collect();
        let name = cells[1].trim_matches('`').trim_end_matches('/').to_owned();
        let codes = if cells.len() > 4 {
            cells[3].split_whitespace().map(str::to_owned).collect()
        } else {
            BTreeSet::new()
        };
        listed.insert(name, codes);
    }
    let actual: BTreeMap<String, BTreeSet<String>> = sema_fixtures()
        .into_iter()
        .map(|f| {
            let codes = f
                .files
                .iter()
                .flat_map(|file| support::expected_codes(&file.text))
                .collect();
            (f.name, codes)
        })
        .collect();
    assert_eq!(
        listed, actual,
        "README.md の一覧と意味系のfixtureが合わない"
    );
}

fn sample_ir() -> (Ir, Vec<SourceFile>) {
    let project = load_project(&sample_dir()).expect("サンプルを読めない");
    let ir = analyze(&project);
    (ir, project.files)
}

/// サンプル作品のIR。エラーと警告が0件で、情報は意図して残したもの（I302 が5件、I403 が2件）だけ。
#[test]
fn sample_project() {
    let (ir, files) = sample_ir();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for diag in &ir.diagnostics {
        *counts.entry(diag.code).or_default() += 1;
    }
    let expected = BTreeMap::from([("I302", 5), ("I403", 2)]);
    assert_eq!(
        counts,
        expected,
        "意図していない診断がある\n{}",
        dump_diagnostics(&ir, &files)
    );
    let unknown_assets: Vec<&str> = ir
        .diagnostics
        .iter()
        .filter(|d| d.code == "I403")
        .map(|d| d.message.as_str())
        .collect();
    assert!(unknown_assets.iter().any(|m| m.contains("チャイム")));
    assert!(unknown_assets.iter().any(|m| m.contains("駅/夕方")));

    insta::assert_snapshot!("sample_houkago", snapshot_text(&ir, &files));
}

/// サンプルのIRの要所：話者のID、変数名、選択肢、条件分岐、辺、エンディング、スタブ。
#[test]
fn sample_project_essentials() {
    let (ir, _) = sample_ir();
    let scene = |id: &str| ir.scenes.iter().find(|s| s.id == id).unwrap();

    let s012 = scene("s012");
    assert!(s012.ops.iter().any(
        |op| matches!(op, Op::Say { speaker, style, .. } if speaker == "akari" && style == "thought")
    ));
    let json = serde_json::to_string(&s012.ops).unwrap();
    assert!(json.contains(r#""var":"akari.好感度""#), "{json}");
    assert!(json.contains(r#""name":"flag.勇気""#), "{json}");
    assert!(
        s012.ops
            .iter()
            .any(|op| matches!(op, Op::Choice { options, .. } if options.len() == 2))
    );
    assert!(s012.ops.iter().any(|op| matches!(op, Op::If { .. })));

    let edge = |from: &str, to: &str| ir.edges.iter().find(|e| e.from == from && e.to == to);
    let to_s014 = edge("s012", "s014").expect("s012 → s014 の辺が無い");
    assert_eq!(
        (to_s014.via, to_s014.cond_text.as_deref()),
        ("choice", Some("勇気 >= 2"))
    );
    let to_s013a = edge("s012", "s013a").expect("s012 → s013a の辺が無い");
    assert_eq!(
        (to_s013a.via, to_s013a.cond_text.as_deref()),
        ("if", Some("灯.好感度 >= 3"))
    );
    assert_eq!(edge("s012", "s013b").map(|e| e.via), Some("else"));

    let endings: BTreeSet<String> = ir
        .scenes
        .iter()
        .flat_map(|s| &s.ops)
        .filter_map(|op| match op {
            Op::Ending { id, .. } => id.clone(),
            _ => None,
        })
        .collect();
    let expected: BTreeSet<String> = ["akari_end", "tsumugi_end", "bad_end"]
        .map(str::to_owned)
        .into();
    assert_eq!(endings, expected);

    let stubs: Vec<&str> = ir
        .scenes
        .iter()
        .filter(|s| s.is_stub)
        .map(|s| s.id.as_str())
        .collect();
    // サンプルの README の「スタブのシーン」と同じ。
    assert_eq!(stubs, ["s014", "s022", "s030", "s060", "s061"]);
    assert_eq!(ir.start_scene.as_deref(), Some("s010"));
}

#[test]
fn loads_shared_fixture_project() {
    let project = load_project(&fixtures_dir().join("project")).unwrap();
    let ids: Vec<&str> = project.characters.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["akari", "minato", "tsumugi"]);
    let ruta = project
        .variables
        .iter()
        .find(|v| v.key == "flag.ルート")
        .unwrap();
    assert_eq!(
        ruta.ty,
        VarType::Enum(vec!["共通".into(), "灯".into(), "紬".into()])
    );
    assert!(project.catalog.characters["akari"].contains("笑顔"));
    assert_eq!(project.settings.brackets.len(), 2);
}

/// サンプル作品の設定DBを、型定義にもとづいて読む（D042）。キャラクターは `builtin: character` の型のエントリ。
#[test]
fn loads_sample_bible() {
    let project = load_project(&sample_dir()).unwrap();
    assert!(project.load_diagnostics.is_empty());
    let types: Vec<(&str, Option<&str>)> = project
        .bible
        .types
        .iter()
        .map(|t| (t.id.as_str(), t.builtin.as_deref()))
        .collect();
    assert_eq!(
        types,
        [
            ("character", Some("character")),
            ("foreshadow", None),
            ("organization", None)
        ]
    );
    let organization = &project.bible.types[2];
    assert_eq!(organization.fields[0].ty, "select");
    assert_eq!(
        organization.fields[0].options,
        ["学校", "部活", "店", "その他"]
    );
    let hairpin = project
        .bible
        .entries
        .iter()
        .find(|e| e.id == "hairpin")
        .unwrap();
    assert_eq!(hairpin.type_id, "foreshadow");
    assert_eq!(hairpin.path, "bible/entries/foreshadow/hairpin.md");
    assert_eq!(hairpin.fields["payoff"], "s040");
    assert_eq!(hairpin.fields["setup"], serde_json::json!(["s021"]));
    assert!(hairpin.body.starts_with("灯がいつも着けている髪留め"));
    let ids: Vec<&str> = project.characters.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["akari", "minato", "miura", "tsumugi"]);
}

/// 一時ディレクトリにプロジェクトを作る。
fn temp_project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scn-core-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    for (path, text) in files {
        let path = dir.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    dir
}

/// `project.toml` の無いフォルダは、プロジェクトではないので読み込みのエラーになる。
/// 無い設定ファイルは空として扱い、診断も出さない。
#[test]
fn project_without_toml() {
    let dir = temp_project("minimal", &[("project.toml", "format_version = 0\n")]);
    let project = load_project(&dir).unwrap();
    assert!(
        project.variables.is_empty() && project.characters.is_empty() && project.files.is_empty()
    );
    assert!(project.load_diagnostics.is_empty());
    let _ = fs::remove_dir_all(&dir);

    let dir = temp_project("no_toml", &[("story/a.scn", "## a #s1\n")]);
    let error = load_project(&dir).expect_err("project.toml が無い");
    assert!(
        matches!(&error, ProjectError::Io { path, .. } if path.ends_with("project.toml")),
        "{error}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// 設定ファイルの診断（E801〜W804）。
const SETTINGS_CODES: [&str; 5] = ["E801", "E802", "E803", "W804", "W805"];

/// 設定ファイルのfixture（`settings/<名前>/`。それぞれが1つのプロジェクト）。
fn settings_fixtures() -> Vec<(String, PathBuf)> {
    sorted_entries(&fixtures_dir().join("settings"))
        .into_iter()
        .filter(|p| p.is_dir())
        .map(|p| (p.file_name().unwrap().to_string_lossy().into_owned(), p))
        .collect()
}

/// 期待する診断（`story/a.scn` の1行目の `// expect:`）。
fn settings_expected(dir: &Path) -> BTreeSet<String> {
    support::expected_codes(&read(&dir.join("story/a.scn")))
        .into_iter()
        .collect()
}

/// 診断を `ファイル:行:列 コード メッセージ` の形で書き出す。位置は、ディスクのファイルで数える。
fn dump_with_disk(ir: &Ir, dir: &Path) -> String {
    let mut out = String::new();
    for diag in &ir.diagnostics {
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

/// 設定ファイルの誤りは、読み込みで止めずに診断にし、本文の解析を続ける（D042）。
/// 出た診断のコードの集まりが `// expect:` と一致し、診断がスナップショットと一致する。
#[test]
fn settings_fixtures_report_diagnostics() {
    let mut failures = Vec::new();
    for (name, dir) in settings_fixtures() {
        let project = load_project(&dir).unwrap_or_else(|e| panic!("{name}：{e}"));
        assert!(
            project.files.iter().any(|f| f.path == "story/a.scn"),
            "{name}：本文が読まれていない"
        );
        let ir = analyze(&project);
        let expected = settings_expected(&dir);
        let actual: BTreeSet<String> = ir.diagnostics.iter().map(|d| d.code.to_owned()).collect();
        if actual != expected {
            failures.push(format!(
                "{name}：期待 {expected:?}、実際 {actual:?}\n{}",
                dump_with_disk(&ir, &dir)
            ));
        }
        insta::assert_snapshot!(format!("settings__{name}"), dump_with_disk(&ir, &dir));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// 行IDの付与を止めるのは、本文の読み方に関わる設定（`format_version`、`[dialogue]`）を読めないときと、
/// `project.toml` そのものを読めないときだけ（D045）。
#[test]
fn reading_errors_only_for_format_version_and_dialogue() {
    let expected = [
        ("toml_broken", true),
        ("toml_version", true),
        ("toml_items", true),
        ("toml_brackets", true),
        ("toml_dialogue_scalar", true),
        ("toml_title", false),
        ("toml_editor", false),
        ("toml_screen", false),
        ("ok_brackets_empty", false),
        ("variables_broken", false),
    ];
    for (name, reading) in expected {
        let project = load_project(&fixtures_dir().join("settings").join(name)).unwrap();
        assert_eq!(project.has_reading_errors(), reading, "{name}");
    }
}

/// 設定ファイルの診断それぞれに、それを期待するfixtureがある。
#[test]
fn every_settings_code_has_a_fixture() {
    let covered: BTreeSet<String> = settings_fixtures()
        .iter()
        .flat_map(|(_, dir)| settings_expected(dir))
        .collect();
    let missing: Vec<_> = SETTINGS_CODES
        .iter()
        .filter(|code| !covered.contains(**code))
        .collect();
    assert!(missing.is_empty(), "fixtureの無い診断：{missing:?}");
}

/// fixtureの一覧（README.md）の設定ファイルの表と、`settings/` に置いてあるものと、`// expect:` が一致する。
#[test]
fn settings_fixture_list_matches_files() {
    let readme = read(&fixtures_dir().join("README.md"));
    let section = readme
        .split("## 設定ファイル（T1-3、`settings/`）")
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
        .expect("README.md に設定ファイルの節が無い");
    let mut listed: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for row in section.lines().filter(|l| l.starts_with("| `")) {
        let cells: Vec<&str> = row.split('|').map(str::trim).collect();
        let name = cells[1].trim_matches('`').trim_end_matches('/').to_owned();
        let codes = cells[3].split_whitespace().map(str::to_owned).collect();
        listed.insert(name, codes);
    }
    let actual: BTreeMap<String, BTreeSet<String>> = settings_fixtures()
        .into_iter()
        .map(|(name, dir)| (name, settings_expected(&dir)))
        .collect();
    assert_eq!(
        listed, actual,
        "README.md の一覧と設定ファイルのfixtureが合わない"
    );
}

/// キャラクターのIDは、フロントマターに無ければファイル名（D036）。本文は `story/*.scn` だけを読む。
#[test]
fn character_id_and_story_paths() {
    let dir = temp_project(
        "ids",
        &[
            ("project.toml", "format_version = 0\n"),
            ("bible/entries/character/akari.md", "---\nname: 灯\n---\n"),
            ("story/b.scn", "## b #s2\n"),
            ("story/a/c.scn", "## c #s3\n"),
            ("story/a.scn", "## a #s1\n"),
        ],
    );
    let project = load_project(&dir).unwrap();
    assert_eq!(project.characters[0].id, "akari");
    let paths: Vec<&str> = project.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["story/a.scn", "story/b.scn"]);
    let _ = fs::remove_dir_all(&dir);
}
