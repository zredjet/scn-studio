//! Git の操作（T3-9、D070）：自律の大きな実行を、作品フォルダの Git のブランチ（`ai/<タスク>-<日時>`）で行い、
//! 人が確かめてから取り込む。依存を足さずに `git` コマンドを呼ぶ。

use std::path::Path;
use std::process::Command;

/// `git` を作品フォルダで実行し、標準出力を返す。失敗したら標準エラー（無ければ終了の様子）。
fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|e| format!("git を実行できません：{e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_owned())
    } else {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(if err.is_empty() {
            format!("git {} が失敗しました（{}）", args.join(" "), output.status)
        } else {
            err
        })
    }
}

/// 作品フォルダが Git の作業ツリーの中か。
pub fn is_repo(root: &Path) -> bool {
    git(root, &["rev-parse", "--is-inside-work-tree"]).is_ok_and(|out| out == "true")
}

/// 今のブランチ。
pub fn current_branch(root: &Path) -> Result<String, String> {
    git(root, &["rev-parse", "--abbrev-ref", "HEAD"])
}

/// 作品フォルダの中の、コミットしていない変更（`git status --porcelain` の行）。
pub fn uncommitted(root: &Path) -> Result<Vec<String>, String> {
    Ok(git(root, &["status", "--porcelain", "--", "."])?
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_owned)
        .collect())
}

/// ブランチがあるか。
pub fn branch_exists(root: &Path, name: &str) -> bool {
    git(
        root,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{name}"),
        ],
    )
    .is_ok()
}

/// ブランチの名前（`branch` のひな形の `{task}`・`{date}` を埋める。あれば `-2`・`-3` を足す）。
pub fn branch_name(root: &Path, template: &str, task: &str, date: &str) -> String {
    let base = template.replace("{task}", task).replace("{date}", date);
    let mut name = base.clone();
    let mut n = 2;
    while branch_exists(root, &name) {
        name = format!("{base}-{n}");
        n += 1;
    }
    name
}

/// 新しいブランチを今のコミットから作り、切り替える。
pub fn start_branch(root: &Path, name: &str) -> Result<(), String> {
    git(root, &["switch", "-c", name]).map(|_| ())
}

/// ブランチを切り替える。
pub fn switch(root: &Path, name: &str) -> Result<(), String> {
    git(root, &["switch", name]).map(|_| ())
}

/// Git の名前とメールアドレスが両方とも設定されていなければ、`scn-studio` の名前を使う引数。
fn identity(root: &Path) -> Vec<&'static str> {
    let set = |key: &str| git(root, &["config", key]).is_ok_and(|v| !v.is_empty());
    if set("user.name") && set("user.email") {
        Vec::new()
    } else {
        vec![
            "-c",
            "user.name=scn-studio",
            "-c",
            "user.email=scn-studio@localhost",
        ]
    }
}

/// 作品フォルダの変更をすべてコミットする（変更が無ければ何もしないで `false`）。Git の名前が設定されていなければ、
/// `scn-studio` の名前でコミットする。
pub fn commit_all(root: &Path, message: &str) -> Result<bool, String> {
    git(root, &["add", "-A", "--", "."])?;
    commit_staged(root, message)
}

/// 指定したファイル（作品フォルダからの相対パス）の変更だけをコミットする（変更が無ければ `false`）。ほかの
/// ファイルの変更は、ステージにあっても入れない。無いファイルは飛ばす。
pub fn commit_paths(root: &Path, paths: &[String], message: &str) -> Result<bool, String> {
    let paths: Vec<String> = paths
        .iter()
        .filter(|p| root.join(p.as_str()).is_file())
        .cloned()
        .collect();
    if paths.is_empty() {
        return Ok(false);
    }
    let mut add: Vec<&str> = vec!["add", "--"];
    add.extend(paths.iter().map(String::as_str));
    git(root, &add)?;
    let mut changed: Vec<&str> = vec!["diff", "--cached", "--quiet", "--"];
    changed.extend(paths.iter().map(String::as_str));
    if git(root, &changed).is_ok() {
        return Ok(false);
    }
    let mut args = identity(root);
    args.extend(["commit", "-m", message, "--only", "--"]);
    args.extend(paths.iter().map(String::as_str));
    git(root, &args).map(|_| true)
}

fn commit_staged(root: &Path, message: &str) -> Result<bool, String> {
    if git(root, &["diff", "--cached", "--quiet"]).is_ok() {
        return Ok(false);
    }
    let mut args = identity(root);
    args.extend(["commit", "-m", message]);
    git(root, &args).map(|_| true)
}

/// ブランチと基のブランチの差分のあらまし（`git diff --stat 基...ブランチ`）と、差分の本文。
pub fn diff(root: &Path, base: &str, branch: &str) -> Result<(String, String), String> {
    let range = format!("{base}...{branch}");
    Ok((
        git(root, &["diff", "--stat", &range])?,
        git(root, &["diff", &range])?,
    ))
}

/// ブランチを今のブランチに取り込む（マージのコミットを作る）。ぶつかって取り込めなければ、取り込みをやめて
/// （`git merge --abort`）、作業ツリーを元に戻す。
pub fn merge(root: &Path, branch: &str) -> Result<(), String> {
    let mut args = identity(root);
    args.extend(["merge", "--no-ff", "--no-edit", branch]);
    git(root, &args)
        .map(|_| ())
        .map_err(|e| match git(root, &["merge", "--abort"]) {
            Ok(_) => format!("{e}\n（取り込みをやめて、元に戻しました）"),
            Err(_) => e,
        })
}

/// ブランチを消す（取り込まずに捨てる）。
pub fn delete_branch(root: &Path, branch: &str) -> Result<(), String> {
    git(root, &["branch", "-D", branch]).map(|_| ())
}

/// AI のブランチ（`ai/` で始まるもの）。
pub fn ai_branches(root: &Path) -> Result<Vec<String>, String> {
    Ok(git(
        root,
        &["branch", "--list", "ai/*", "--format=%(refname:short)"],
    )?
    .lines()
    .filter(|l| !l.trim().is_empty())
    .map(str::to_owned)
    .collect())
}

/// 日時の文字列（`20261008-1205`。協定世界時）。
pub fn date_stamp() -> String {
    let now = crate::provenance::now();
    // `2026-10-08T12:05:00Z` から。
    let digits: String = now.chars().filter(char::is_ascii_digit).collect();
    let date = digits.get(..8).unwrap_or_default();
    let time = digits.get(8..12).unwrap_or_default();
    format!("{date}-{time}")
}
