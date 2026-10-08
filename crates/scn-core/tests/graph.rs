//! 構造の一覧（`scn graph`、T1-5）と、作品フォルダのひな形（`scn new`、`scn format-doc`）。

use std::fs;
use std::path::PathBuf;

use scn_core::check::check;
use scn_core::graph::{Level, build_graph};
use scn_core::index::build_index;
use scn_core::scaffold::{
    claude_md, create_project, format_doc, format_doc_is_stale, write_format_doc,
};
use scn_core::{ProjectError, analyze, load_project};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// 完了条件：サンプルの構造の一覧が、粒度ごとに期待したものになる（スナップショット）。
#[test]
fn sample_graph() {
    let project = load_project(&repo_root().join("samples/houkago")).unwrap();
    let ir = analyze(&project);
    let index = build_index(&project, &ir);
    for (name, level) in [
        ("sample_graph_scene", Level::Scene),
        ("sample_graph_chapter", Level::Chapter),
        ("sample_graph_route", Level::Route),
    ] {
        let graph = build_graph(&ir, &index, level);
        insta::assert_snapshot!(name, serde_json::to_string_pretty(&graph).unwrap());
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scn-core-new-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

/// 完了条件：新しく作った作品フォルダは、検証でエラーが0件になる。出るのは、スタブの行き止まり（W503）だけ。
#[test]
fn new_project_checks_clean() {
    let dir = temp_dir("clean");
    create_project(&dir, "テスト\"作品\"").unwrap();
    let project = load_project(&dir).unwrap();
    let ir = analyze(&project);
    let index = build_index(&project, &ir);
    let mut codes: Vec<&str> = ir.diagnostics.iter().map(|d| d.code).collect();
    codes.extend(check(&project, &ir, &index).iter().map(|d| d.code));
    assert_eq!(codes, ["W503"]);
    assert_eq!(project.settings.start_scene.as_deref(), Some("s001"));
    assert!(project.settings.message_window.is_some());
    assert!(project.load_diagnostics.is_empty());
    let toml: toml::Table =
        toml::from_str(&fs::read_to_string(dir.join("project.toml")).unwrap()).unwrap();
    assert_eq!(toml["title"].as_str(), Some("テスト\"作品\""));
    assert_eq!(
        fs::read_to_string(dir.join("FORMAT.md")).unwrap(),
        format_doc()
    );
    assert_eq!(
        fs::read_to_string(dir.join("CLAUDE.md")).unwrap(),
        claude_md("テスト\"作品\"")
    );
    for path in [
        "concept.md",
        "style.md",
        "variables.yaml",
        "story/chapter01.scn",
        "bible/types/character.yaml",
        "ai/provenance.jsonl",
        ".gitignore",
        ".gitattributes",
    ] {
        assert!(dir.join(path).is_file(), "{path}");
    }
    assert!(dir.join("bible/entries/character").is_dir());
    let _ = fs::remove_dir_all(&dir);
}

/// 空でないフォルダには作らない（何も書かない）。空のフォルダには作る。
#[test]
fn new_project_needs_empty_dir() {
    let dir = temp_dir("not-empty");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("memo.txt"), "メモ").unwrap();
    let error = create_project(&dir, "x").unwrap_err();
    assert!(matches!(error, ProjectError::NotEmpty { .. }), "{error}");
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
    fs::remove_file(dir.join("memo.txt")).unwrap();
    create_project(&dir, "x").unwrap();
    assert!(dir.join("project.toml").is_file());
    let _ = fs::remove_dir_all(&dir);
}

/// `FORMAT.md` が古ければ書き直し、同じなら書かない。
#[test]
fn format_doc_rewrite() {
    let dir = temp_dir("format");
    create_project(&dir, "x").unwrap();
    assert!(!format_doc_is_stale(&dir));
    assert!(!write_format_doc(&dir).unwrap());
    // 改行が `\r\n` になっただけなら、古くない。
    fs::write(dir.join("FORMAT.md"), format_doc().replace('\n', "\r\n")).unwrap();
    assert!(!format_doc_is_stale(&dir));
    fs::write(dir.join("FORMAT.md"), "古い").unwrap();
    assert!(format_doc_is_stale(&dir));
    assert!(write_format_doc(&dir).unwrap());
    assert_eq!(
        fs::read_to_string(dir.join("FORMAT.md")).unwrap(),
        format_doc()
    );
    let _ = fs::remove_dir_all(&dir);
}

/// サンプルの `CLAUDE.template.md` は、共通のひな形（作品名「放課後の灯」）に、この作品のきまりを足したもの。
#[test]
fn sample_template_follows_common_template() {
    let sample =
        fs::read_to_string(repo_root().join("samples/houkago/CLAUDE.template.md")).unwrap();
    let common = claude_md("放課後の灯");
    let marker = "## この作品のきまり\n";
    let common_head = &common[..common.find(marker).unwrap() + marker.len()];
    assert!(
        sample.starts_with(common_head),
        "samples/houkago/CLAUDE.template.md が共通のひな形とずれている"
    );
}
