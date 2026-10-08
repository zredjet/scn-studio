//! フロー図からの編集（`flowedit`、T2-9、D060）：スタブのシーン、ジャンプ、選択肢の書き足し。
//! 完了条件：書き足しのテスト（ほかの行が変わらない）。

use std::fs;
use std::path::{Path, PathBuf};

use scn_core::analyze;
use scn_core::flowedit::FlowEdge;
use scn_core::ir::Op;
use scn_core::workspace::Workspace;

fn sample(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples")
        .join(name)
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn copy_sample(name: &str, tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scn-core-flow-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    copy_dir(&sample(name), &dir);
    dir
}

fn temp_project(tag: &str, story: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scn-core-flow-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("story")).unwrap();
    fs::write(dir.join("project.toml"), "format_version = 0\n").unwrap();
    fs::write(dir.join("story/a.scn"), story).unwrap();
    dir
}

/// `after` は `before` に行を足しただけ（足した行を除くと、前の行が同じ順に並ぶ）。足した行を返す。
fn added_lines(before: &str, after: &str) -> Vec<String> {
    let old: Vec<&str> = before.split_inclusive('\n').collect();
    let new: Vec<&str> = after.split_inclusive('\n').collect();
    let mut added = Vec::new();
    let mut i = 0;
    for line in new {
        if old.get(i) == Some(&line) {
            i += 1;
        } else {
            added.push(line.to_owned());
        }
    }
    assert_eq!(i, old.len(), "前の行が残っていない：{after}");
    added
}

fn edges(root: &Path) -> Vec<(String, String)> {
    let ws = Workspace::open(root).unwrap();
    analyze(ws.project())
        .edges
        .iter()
        .map(|e| (e.from.clone(), e.to.clone()))
        .collect()
}

/// ジャンプ：シーンの最後の中身の行の後ろに `-> 行き先` を足す。ほかの行は変わらない。
#[test]
fn add_jump() {
    let root = copy_sample("houkago", "jump");
    let before = fs::read_to_string(root.join("story/chapter01.scn")).unwrap();
    let mut ws = Workspace::open(&root).unwrap();
    let written = ws
        .add_edge(&FlowEdge::Jump {
            from: "s014".to_owned(),
            to: "s040".to_owned(),
        })
        .unwrap();
    assert_eq!(written.file, "story/chapter01.scn");
    let after = fs::read_to_string(root.join("story/chapter01.scn")).unwrap();
    assert_eq!(after, written.text);
    assert_eq!(added_lines(&before, &after), ["-> s040\n"]);
    // スタブ（メタ情報だけ）の閉じの `---` の後ろ。
    assert!(after.contains("route: 灯\n---\n-> s040\n"), "{after}");
    assert!(edges(&root).contains(&("s014".to_owned(), "s040".to_owned())));
    assert_eq!(ws.text("story/chapter01.scn"), Some(after.as_str()));
    // 行き先の決まった終わり方のシーン（どの枝もジャンプする @if）、無いシーンには足さない。
    for edge in [
        FlowEdge::Jump {
            from: "s012".to_owned(),
            to: "s040".to_owned(),
        },
        FlowEdge::Jump {
            from: "s014".to_owned(),
            to: "s999".to_owned(),
        },
        FlowEdge::Jump {
            from: "s999".to_owned(),
            to: "s040".to_owned(),
        },
    ] {
        assert!(ws.add_edge(&edge).is_err(), "{edge:?}");
    }
    let _ = fs::remove_dir_all(&root);
}

/// 選択肢：シーンの最後が選択ブロックなら `@end` の前に足し（字下げをそろえる）、そうでなければブロックを足す。
#[test]
fn add_choice() {
    let story = "## a #s1\r\n灯「どうする？」 ^aaaa\r\n@選択\r\n  - 行く -> s2 ^aaab\r\n@end\r\n\r\n## b #s2\r\n　b。 ^aaac\r\n\r\n## c #s3\r\n　c。 ^aaad\r\n";
    let root = temp_project("choice", story);
    let mut ws = Workspace::open(&root).unwrap();
    let written = ws
        .add_edge(&FlowEdge::Choice {
            from: "s1".to_owned(),
            to: "s3".to_owned(),
            label: "行かない".to_owned(),
        })
        .unwrap();
    assert_eq!(
        added_lines(story, &written.text),
        ["  - 行かない -> s3\r\n"]
    );
    assert!(written.text.contains("  - 行かない -> s3\r\n@end\r\n"));
    let ir = analyze(ws.project());
    let options = ir
        .scenes
        .iter()
        .find(|s| s.id == "s1")
        .unwrap()
        .ops
        .iter()
        .find_map(|op| match op {
            Op::Choice { options, .. } => Some(options.len()),
            _ => None,
        });
    assert_eq!(options, Some(2));
    // 選択ブロックで終わらないシーン：ブロックを足す（空行の前、次の見出しより前）。
    let before = written.text.clone();
    let written = ws
        .add_edge(&FlowEdge::Choice {
            from: "s2".to_owned(),
            to: "s3".to_owned(),
            label: "進む".to_owned(),
        })
        .unwrap();
    assert_eq!(
        added_lines(&before, &written.text),
        ["@選択\r\n", "- 進む -> s3\r\n", "@end\r\n"]
    );
    assert!(written.text.contains("　b。 ^aaac\r\n@選択\r\n"));
    // 表示文の誤り。
    for label in ["", "a -> b", "a if x", "a\nb"] {
        assert!(
            ws.add_edge(&FlowEdge::Choice {
                from: "s3".to_owned(),
                to: "s1".to_owned(),
                label: label.to_owned(),
            })
            .is_err(),
            "{label:?}"
        );
    }
    let _ = fs::remove_dir_all(&root);
}

/// スタブ：章の末尾に見出しと要約。保存していない変更のある章には、辺もスタブも足さない。
#[test]
fn add_stub_and_dirty() {
    let story = "## a #s1\n　a。 ^aaaa\n";
    let root = temp_project("stub", story);
    let mut ws = Workspace::open(&root).unwrap();
    let written = ws
        .add_stub(
            "story/a.scn",
            "s2",
            "雨の日",
            Some("  二人が図書室で会う  "),
        )
        .unwrap();
    assert_eq!(
        written.text,
        "## a #s1\n　a。 ^aaaa\n\n## 雨の日 #s2\n---\nsummary: 二人が図書室で会う\n---\n"
    );
    let ir = analyze(ws.project());
    assert!(ir.scenes.iter().any(|s| s.id == "s2" && s.is_stub));
    // 終わりの改行の無い章、要約なし。
    assert!(ws.set_buffer("story/a.scn", &format!("{}x", written.text)));
    assert!(ws.add_stub("story/a.scn", "s3", "題", None).is_err());
    assert!(
        ws.add_edge(&FlowEdge::Jump {
            from: "s1".to_owned(),
            to: "s2".to_owned(),
        })
        .is_err()
    );
    let _ = fs::remove_dir_all(&root);
}

/// 終わりの改行の無い章のシーンにジャンプを足す。
#[test]
fn add_jump_without_final_newline() {
    let root = temp_project("eof", "## a #s1\n　a。 ^aaaa\n## b #s2\n　b。");
    let mut ws = Workspace::open(&root).unwrap();
    let written = ws
        .add_edge(&FlowEdge::Jump {
            from: "s2".to_owned(),
            to: "s1".to_owned(),
        })
        .unwrap();
    assert_eq!(
        written.text,
        "## a #s1\n　a。 ^aaaa\n## b #s2\n　b。\n-> s1\n"
    );
    let written = ws
        .add_edge(&FlowEdge::Jump {
            from: "s1".to_owned(),
            to: "s2".to_owned(),
        })
        .unwrap();
    assert_eq!(
        written.text,
        "## a #s1\n　a。 ^aaaa\n-> s2\n## b #s2\n　b。\n-> s1\n"
    );
    let _ = fs::remove_dir_all(&root);
}

/// 行き先の決まった文が途中にあるシーン（後ろの行は実行されない）、最後のブロックが閉じていないシーン、`if ` で
/// 始まる表示文には足さない。シーンの末尾のコメントの行の前に足す。
#[test]
fn edge_refusals_and_comments() {
    let story = "## a #s1\n-> s2\n　dead。\n\n## b #s2\n　b。\n// 次は第2部\n\n## c #s3\n　c。\n@raw renpy\n  $ x = 1\n\n## d #s4\n　d。\n@選択\n- 行く -> s1\n";
    let root = temp_project("refuse", story);
    let mut ws = Workspace::open(&root).unwrap();
    for edge in [
        FlowEdge::Jump {
            from: "s1".to_owned(),
            to: "s2".to_owned(),
        },
        FlowEdge::Jump {
            from: "s3".to_owned(),
            to: "s1".to_owned(),
        },
        FlowEdge::Choice {
            from: "s4".to_owned(),
            to: "s2".to_owned(),
            label: "戻る".to_owned(),
        },
        FlowEdge::Choice {
            from: "s2".to_owned(),
            to: "s1".to_owned(),
            label: "if x".to_owned(),
        },
    ] {
        assert!(ws.add_edge(&edge).is_err(), "{edge:?}");
    }
    assert_eq!(fs::read_to_string(root.join("story/a.scn")).unwrap(), story);
    // 末尾のコメントの前に足す。
    let written = ws
        .add_edge(&FlowEdge::Jump {
            from: "s2".to_owned(),
            to: "s1".to_owned(),
        })
        .unwrap();
    assert!(
        written.text.contains("　b。\n-> s1\n// 次は第2部\n"),
        "{}",
        written.text
    );
    assert_eq!(added_lines(story, &written.text), ["-> s1\n"]);
    let _ = fs::remove_dir_all(&root);
}
