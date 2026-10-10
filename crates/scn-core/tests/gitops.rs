//! Git の操作（T3-9）：ブランチを作って書き、コミットし、基のブランチに戻って差分を見て、取り込む・捨てる。
//! `git` が無い環境では確かめない。

use std::fs;
use std::process::Command;

use scn_core::gitops;

fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn run(dir: &std::path::Path, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "git {args:?}");
}

#[test]
fn branch_commit_diff_merge() {
    if !git_available() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("scn-git-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("story")).unwrap();
    run(&dir, &["init", "-q", "-b", "main"]);
    // Windows の Git の既定（core.autocrlf=true）で、取り出したファイルの改行が CRLF にならないように。
    run(&dir, &["config", "core.autocrlf", "false"]);
    fs::write(dir.join("story/a.scn"), "## a #s1\n").unwrap();
    run(&dir, &["add", "-A"]);
    run(&dir, &["commit", "-q", "-m", "init"]);
    assert!(gitops::is_repo(&dir));
    assert_eq!(gitops::current_branch(&dir).unwrap(), "main");
    assert!(gitops::uncommitted(&dir).unwrap().is_empty());

    let name = gitops::branch_name(&dir, "ai/{task}-{date}", "draft_chapter", "20261008-1200");
    assert_eq!(name, "ai/draft_chapter-20261008-1200");
    gitops::start_branch(&dir, &name).unwrap();
    assert_eq!(gitops::current_branch(&dir).unwrap(), name);
    fs::write(dir.join("story/a.scn"), "## a #s1\n灯「やあ」 ^aaaa\n").unwrap();
    assert_eq!(gitops::uncommitted(&dir).unwrap().len(), 1);
    assert!(gitops::commit_all(&dir, "ai: 下書き").unwrap());
    assert!(!gitops::commit_all(&dir, "ai: 空").unwrap());
    gitops::switch(&dir, "main").unwrap();
    assert_eq!(
        fs::read_to_string(dir.join("story/a.scn")).unwrap(),
        "## a #s1\n"
    );
    // 同じ名前があれば -2 を足す。
    assert_eq!(
        gitops::branch_name(&dir, "ai/{task}-{date}", "draft_chapter", "20261008-1200"),
        "ai/draft_chapter-20261008-1200-2"
    );
    assert_eq!(
        gitops::ai_branches(&dir).unwrap(),
        std::slice::from_ref(&name)
    );
    let (stat, patch) = gitops::diff(&dir, "main", &name).unwrap();
    assert!(stat.contains("story/a.scn"), "{stat}");
    assert!(patch.contains("+灯「やあ」 ^aaaa"), "{patch}");
    gitops::merge(&dir, &name).unwrap();
    assert!(
        fs::read_to_string(dir.join("story/a.scn"))
            .unwrap()
            .contains("灯「やあ」")
    );
    gitops::delete_branch(&dir, &name).unwrap();
    assert!(gitops::ai_branches(&dir).unwrap().is_empty());
    let stamp = gitops::date_stamp();
    assert_eq!(stamp.len(), 13, "{stamp}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn commit_paths_and_merge_abort() {
    if !git_available() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("scn-git-paths-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("story")).unwrap();
    run(&dir, &["init", "-q", "-b", "main"]);
    // Windows の Git の既定（core.autocrlf=true）で、取り出したファイルの改行が CRLF にならないように。
    run(&dir, &["config", "core.autocrlf", "false"]);
    fs::write(dir.join("story/a.scn"), "## a #s1\n").unwrap();
    fs::write(dir.join("story/b.scn"), "## b #s2\n").unwrap();
    run(&dir, &["add", "-A"]);
    run(&dir, &["commit", "-q", "-m", "init"]);

    gitops::start_branch(&dir, "ai/x").unwrap();
    // 書いたファイル（と新しいファイル）だけをコミットし、人が直したファイルは作業ツリーに残す。
    fs::write(dir.join("story/a.scn"), "## a #s1\nAI ^aaaa\n").unwrap();
    fs::create_dir_all(dir.join("ai")).unwrap();
    fs::write(dir.join("ai/provenance.jsonl"), "{}\n").unwrap();
    fs::write(dir.join("story/b.scn"), "## b #s2\n人\n").unwrap();
    let paths = vec![
        "story/a.scn".to_owned(),
        "ai/provenance.jsonl".to_owned(),
        "ai/none.txt".to_owned(),
    ];
    assert!(gitops::commit_paths(&dir, &paths, "ai: 下書き").unwrap());
    assert!(!gitops::commit_paths(&dir, &paths, "ai: 空").unwrap());
    let left = gitops::uncommitted(&dir).unwrap();
    assert_eq!(left.len(), 1, "{left:?}");
    assert!(left[0].contains("story/b.scn"), "{left:?}");
    run(&dir, &["checkout", "--", "story/b.scn"]);

    // ぶつかる取り込みは、やめて元に戻す。
    gitops::switch(&dir, "main").unwrap();
    fs::write(dir.join("story/a.scn"), "## a #s1\n人 ^bbbb\n").unwrap();
    run(&dir, &["commit", "-q", "-am", "human"]);
    let err = gitops::merge(&dir, "ai/x").unwrap_err();
    assert!(err.contains("元に戻しました"), "{err}");
    assert!(gitops::uncommitted(&dir).unwrap().is_empty());
    assert_eq!(
        fs::read_to_string(dir.join("story/a.scn")).unwrap(),
        "## a #s1\n人 ^bbbb\n"
    );
    let _ = fs::remove_dir_all(&dir);
}
