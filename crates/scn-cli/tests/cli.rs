//! `scn` の端から端までのテスト（`assert_cmd`）。サンプル作品と、scn-core のfixtureを使う。

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;

/// 終了コード：成功、エラー（E）の診断あり、使い方や入出力の誤り。
const SUCCESS: i32 = 0;
const HAS_ERRORS: i32 = 1;
const FAILURE: i32 = 2;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixtures_dir() -> PathBuf {
    repo_root().join("crates/scn-core/tests/fixtures")
}

struct Output {
    code: i32,
    stdout: String,
    stderr: String,
}

fn scn(args: &[&str], dir: &Path) -> Output {
    let output = Command::cargo_bin("scn")
        .unwrap()
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    Output {
        code: output.status.code().expect("シグナルで終わった"),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

fn expected_codes(source: &str) -> BTreeSet<String> {
    let first = source
        .trim_start_matches('\u{feff}')
        .lines()
        .next()
        .unwrap_or("");
    match first.strip_prefix("// expect:") {
        Some(codes) => codes.split_whitespace().map(str::to_owned).collect(),
        None => BTreeSet::new(),
    }
}

/// 一時ディレクトリに、ファイルを置く。
fn temp_dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scn-cli-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    for (path, text) in files {
        let path = dir.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    dir
}

/// ディレクトリを丸ごと写す。
fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let path = entry.unwrap().path();
        let target = to.join(path.file_name().unwrap());
        if path.is_dir() {
            copy_dir(&path, &target);
        } else {
            fs::copy(&path, &target).unwrap();
        }
    }
}

// ---- scn ir ----

/// 完了条件：`scn ir samples/houkago` がJSONを出し、終了コード0で終わる。
#[test]
fn ir_of_sample() {
    let out = scn(&["ir", "samples/houkago"], &repo_root());
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    let ir: Value = serde_json::from_str(&out.stdout).expect("JSONでない");
    assert_eq!(ir["ir_version"], 1);
    assert_eq!(ir["start_scene"], "s010");
    assert_eq!(ir["scenes"].as_array().unwrap().len(), 13);
    let codes: Vec<&str> = ir["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap())
        .collect();
    assert_eq!(codes.iter().filter(|c| **c == "I302").count(), 5);
    assert_eq!(codes.iter().filter(|c| **c == "I403").count(), 2);
    assert_eq!(codes.len(), 7);
}

/// エラー（E）の診断があれば、IRを出したうえで終了コード1。
#[test]
fn ir_with_errors() {
    let dir = temp_dir(
        "ir-errors",
        &[
            ("project.toml", "format_version = 0\n"),
            ("story/a.scn", "## a #s001\n-> s404\n"),
        ],
    );
    let out = scn(&["ir", "."], &dir);
    assert_eq!(out.code, HAS_ERRORS);
    let ir: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(ir["diagnostics"][0]["code"], "E501");
    let _ = fs::remove_dir_all(&dir);
}

// ---- scn parse ----

/// サンプルの章を解析すると、そのファイルの診断だけを `ファイル:行:列 コード メッセージ` の形で出す。
#[test]
fn parse_sample_chapter() {
    let out = scn(
        &["parse", "samples/houkago/story/chapter01.scn"],
        &repo_root(),
    );
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    let lines: Vec<&str> = out.stdout.lines().collect();
    let path = "samples/houkago/story/chapter01.scn";
    assert_eq!(
        lines,
        [
            format!(
                "{path}:34:5 I403 素材カタログに無い効果音です：チャイム（仮素材として扱います）"
            ),
            format!("{path}:105:1 I302 行IDがありません"),
            format!("{path}:106:1 I302 行IDがありません"),
            format!("{path}:108:1 I302 行IDがありません"),
        ]
    );
    assert!(
        out.stderr.contains("エラー 0件、警告 0件、情報 4件"),
        "{}",
        out.stderr
    );

    // 別の章の診断は混ざらない。
    let out = scn(
        &["parse", "samples/houkago/story/chapter02.scn"],
        &repo_root(),
    );
    assert_eq!(out.code, SUCCESS);
    let codes: Vec<&str> = out
        .stdout
        .lines()
        .map(|l| l.split(' ').nth(1).unwrap())
        .collect();
    assert_eq!(codes, ["I302", "I302", "I403"]);
}

/// 完了条件：エラーを含むfixtureで、期待した診断と終了コード1を返す。
/// 構文系のfixtureすべてで、出た診断のコードが `// expect:` と一致し、終了コードがエラーの有無と合う。
#[test]
fn parse_syntax_fixtures() {
    let mut checked = 0;
    for entry in fs::read_dir(fixtures_dir()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "scn") {
            continue;
        }
        let expected = expected_codes(&fs::read_to_string(&path).unwrap());
        let out = scn(&["parse", path.to_str().unwrap(), "--json"], &repo_root());
        let diagnostics: Value = serde_json::from_str(&out.stdout)
            .unwrap_or_else(|e| panic!("{}：JSONでない（{e}）\n{}", path.display(), out.stderr));
        let actual: BTreeSet<String> = diagnostics
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["code"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(actual, expected, "{}", path.display());
        let has_errors = expected.iter().any(|c| c.starts_with('E'));
        let want = if has_errors { HAS_ERRORS } else { SUCCESS };
        assert_eq!(out.code, want, "{}", path.display());
        checked += 1;
    }
    assert!(checked >= 50, "fixtureが少なすぎる：{checked}");
}

/// エラーを含むfixtureの、文字での出力と、JSONの行と列。
#[test]
fn parse_reports_line_and_column() {
    let file = "crates/scn-core/tests/fixtures/expr_syntax_error.scn";
    let out = scn(&["parse", file], &repo_root());
    assert_eq!(out.code, HAS_ERRORS);
    let first = out.stdout.lines().next().unwrap();
    assert_eq!(
        first,
        format!("{file}:3:5 E601 式の構文エラー：( に対応する ) がありません")
    );

    let out = scn(&["parse", file, "--json"], &repo_root());
    let diagnostics: Value = serde_json::from_str(&out.stdout).unwrap();
    let d = &diagnostics[0];
    assert_eq!(
        (&d["line"], &d["column"], &d["end_line"], &d["end_column"]),
        (
            &Value::from(3),
            &Value::from(5),
            &Value::from(3),
            &Value::from(6)
        )
    );
    assert_eq!(d["severity"], "error");
    assert_eq!(d["file"], file);
}

/// 警告だけなら終了コード0。
#[test]
fn warnings_do_not_fail() {
    let out = scn(
        &[
            "parse",
            "crates/scn-core/tests/fixtures/directive_unknown.scn",
        ],
        &repo_root(),
    );
    assert_eq!(out.code, SUCCESS);
    assert_eq!(out.stdout.lines().count(), 2);
}

/// プロジェクトの中のファイルは、プロジェクト全体で意味解析する（設定DB・変数・別のファイルのシーン）。
#[test]
fn parse_in_project_runs_semantic_analysis() {
    let dir = std::env::temp_dir().join(format!("scn-cli-{}-sema", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    copy_dir(&fixtures_dir().join("project"), &dir);
    fs::create_dir_all(dir.join("story")).unwrap();
    fs::copy(
        fixtures_dir().join("sema/jump_unknown_scene.scn"),
        dir.join("story/a.scn"),
    )
    .unwrap();
    fs::write(dir.join("story/b.scn"), "## 別のファイル #s002\n@回想\n").unwrap();

    let out = scn(&["parse", "story/a.scn"], &dir);
    assert_eq!(out.code, HAS_ERRORS, "{}", out.stderr);
    let lines: Vec<&str> = out.stdout.lines().collect();
    assert_eq!(
        lines,
        [
            "story/a.scn:4:9 E501 シーン s998 がありません",
            "story/a.scn:6:5 E501 シーン s997 がありません",
            "story/a.scn:7:4 E501 シーン s999 がありません",
        ]
    );

    // story/ の外のファイルも、プロジェクトの本文に加えて解析する。別のファイルのシーンへ飛べる。
    fs::create_dir_all(dir.join("drafts")).unwrap();
    fs::write(
        dir.join("drafts/c.scn"),
        "## 下書き #s003\n@呼出 s002\n-> s404\n",
    )
    .unwrap();
    let out = scn(&["parse", "drafts/c.scn"], &dir);
    assert_eq!(out.code, HAS_ERRORS);
    assert_eq!(
        out.stdout,
        "drafts/c.scn:3:4 E501 シーン s404 がありません\n"
    );

    // 括弧の設定もプロジェクトのものを使う（『』が台詞になる）。
    fs::write(dir.join("story/d.scn"), "## 心の声 #s004\n灯『言えない』\n").unwrap();
    let out = scn(&["parse", "story/d.scn", "--json"], &dir);
    let diagnostics: Value = serde_json::from_str(&out.stdout).unwrap();
    let codes: Vec<&str> = diagnostics
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap())
        .collect();
    assert_eq!(codes, ["I302"], "『』の台詞が台詞として扱われていない");
    let _ = fs::remove_dir_all(&dir);
}

// ---- 使い方や入出力の誤り ----

/// 使い方の誤り、無いファイル、プロジェクトでないフォルダ、壊れた設定ファイルは、終了コード2。
#[test]
fn usage_and_io_errors() {
    let root = repo_root();
    for args in [
        &[][..],
        &["unknown"][..],
        &["parse"][..],
        &["parse", "no-such-file.scn"][..],
        &["ir", "crates"][..],
    ] {
        let out = scn(args, &root);
        assert_eq!(out.code, FAILURE, "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        assert!(!out.stderr.is_empty(), "{args:?}");
    }

    let dir = temp_dir(
        "broken",
        &[
            ("project.toml", "format_version = 0\n"),
            ("variables.yaml", "flag.a: [\n"),
            ("story/a.scn", "## a #s001\n"),
        ],
    );
    for args in [&["ir", "."][..], &["parse", "story/a.scn"][..]] {
        let out = scn(args, &dir);
        assert_eq!(out.code, FAILURE, "{args:?}");
        assert!(out.stderr.contains("variables.yaml"), "{}", out.stderr);
    }
    let _ = fs::remove_dir_all(&dir);
}

/// 出力先のパイプが閉じても、終了コードは診断に従う（D037）。
#[test]
fn closed_pipe_keeps_exit_code() {
    use std::process::{Command as StdCommand, Stdio};

    let mut source = String::from("## a #s001\n");
    for _ in 0..20_000 {
        source.push_str("@if (\n@end\n");
    }
    let dir = temp_dir("pipe", &[("many.scn", &source)]);
    for json in [false, true] {
        let mut command = StdCommand::new(assert_cmd::cargo::cargo_bin("scn"));
        command.arg("parse").arg("many.scn").current_dir(&dir);
        if json {
            command.arg("--json");
        }
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        // 読まずに閉じる。
        drop(child.stdout.take());
        let status = child.wait().unwrap();
        assert_eq!(status.code(), Some(HAS_ERRORS), "json={json}");
    }
    let _ = fs::remove_dir_all(&dir);
}
