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

// ---- scn fmt ----

/// `scn fmt` の1行（`ファイル:行:列 ^ID`。`--check` では `ファイル:行:列 行IDがありません`）。
struct FmtLine {
    file: String,
    line: usize,
    id: Option<String>,
}

fn fmt_lines(stdout: &str) -> Vec<FmtLine> {
    stdout
        .lines()
        .map(|l| {
            let (place, rest) = l.split_once(' ').expect("空白が無い");
            let mut parts = place.split(':');
            let file = parts.next().unwrap().to_owned();
            let line = parts.next().unwrap().parse().unwrap();
            parts.next().unwrap().parse::<usize>().unwrap();
            let id = rest.strip_prefix('^').map(str::to_owned);
            FmtLine { file, line, id }
        })
        .collect()
}

/// ディレクトリの中のファイルと中身（バイト列）。
fn snapshot_dir(dir: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    let mut files = std::collections::BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in fs::read_dir(&d).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let bytes = fs::read(&path).unwrap();
                files.insert(path.strip_prefix(dir).unwrap().to_path_buf(), bytes);
            }
        }
    }
    files
}

/// 完了条件：サンプルの写しに `scn fmt` を実行すると、I302 の5行にだけ行IDが付き、
/// ほかのバイトは変わらない。2回目は何も変えない。
#[test]
fn fmt_sample_copy() {
    let dir = temp_dir("fmt-sample", &[]);
    copy_dir(&repo_root().join("samples/houkago"), &dir);
    let before = snapshot_dir(&dir);

    let out = scn(&["fmt", "."], &dir);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    let lines = fmt_lines(&out.stdout);
    assert_eq!(lines.len(), 5, "{}", out.stdout);
    assert!(out.stderr.contains("5件"), "{}", out.stderr);

    let after = snapshot_dir(&dir);
    assert_eq!(
        after.keys().collect::<Vec<_>>(),
        before.keys().collect::<Vec<_>>()
    );
    let changed: Vec<String> = after
        .iter()
        .filter(|(path, bytes)| before[*path] != **bytes)
        .map(|(path, _)| path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert_eq!(changed, ["story/chapter01.scn", "story/chapter02.scn"]);

    // 出力した行の末尾に ` ^ID` が付き、それを除けば元の本文に戻る。
    for file in &changed {
        let old = String::from_utf8(before[Path::new(file)].clone()).unwrap();
        let new = String::from_utf8(after[Path::new(file)].clone()).unwrap();
        let mut restored = new.clone();
        for l in lines.iter().filter(|l| &l.file == file) {
            let id = l.id.as_ref().expect("IDが無い");
            let text = new.lines().nth(l.line - 1).unwrap();
            assert!(
                text.ends_with(&format!(" ^{id}")),
                "{file}:{}：{text}",
                l.line
            );
            restored = restored.replacen(&format!(" ^{id}"), "", 1);
        }
        assert_eq!(restored, old, "{file}");
    }

    let again = scn(&["fmt", "."], &dir);
    assert_eq!(again.code, SUCCESS, "{}", again.stderr);
    assert!(again.stdout.is_empty(), "{}", again.stdout);
    assert_eq!(snapshot_dir(&dir), after);
    let _ = fs::remove_dir_all(&dir);
}

/// `--check` は書き換えず、行IDの無い行があれば終了コード1。無ければ0。
#[test]
fn fmt_check() {
    let dir = temp_dir("fmt-check", &[]);
    copy_dir(&repo_root().join("samples/houkago"), &dir);
    let before = snapshot_dir(&dir);

    let out = scn(&["fmt", "--check", "."], &dir);
    assert_eq!(out.code, HAS_ERRORS, "{}", out.stderr);
    let lines = fmt_lines(&out.stdout);
    assert_eq!(lines.len(), 5, "{}", out.stdout);
    assert!(lines.iter().all(|l| l.id.is_none()), "{}", out.stdout);
    assert_eq!(snapshot_dir(&dir), before);

    assert_eq!(scn(&["fmt", "."], &dir).code, SUCCESS);
    let out = scn(&["fmt", "--check", "."], &dir);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    assert!(out.stdout.is_empty(), "{}", out.stdout);
    let _ = fs::remove_dir_all(&dir);
}

/// `--json` は、付けた行IDを、ファイル・行・列と一緒にJSONで出す（AIの出自の記録に使う。D041）。
#[test]
fn fmt_json() {
    let dir = temp_dir("fmt-json", &[]);
    copy_dir(&repo_root().join("samples/houkago"), &dir);
    let out = scn(&["fmt", "--json", "."], &dir);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    let list: Value = serde_json::from_str(&out.stdout).expect("JSONでない");
    let list = list.as_array().unwrap();
    assert_eq!(list.len(), 5);
    for item in list {
        let file = item["file"].as_str().unwrap();
        let line = item["line"].as_u64().unwrap() as usize;
        let id = item["id"].as_str().unwrap();
        assert!(item["column"].as_u64().unwrap() >= 1);
        let text = fs::read_to_string(dir.join(file)).unwrap();
        assert!(
            text.lines()
                .nth(line - 1)
                .unwrap()
                .ends_with(&format!(" ^{id}"))
        );
    }

    let out = scn(&["fmt", "--check", "--json", "."], &dir);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    assert_eq!(
        serde_json::from_str::<Value>(&out.stdout).unwrap(),
        Value::Array(vec![])
    );
    let _ = fs::remove_dir_all(&dir);
}

/// BOM、CRLF、全角空白、エラー（E）を含む本文でも付け、足した部分のほかのバイトは変えない。
/// 本文のエラーで止めない（壊れたまま書ける）。
#[test]
fn fmt_keeps_bytes() {
    let source = "\u{feff}## a #s001\r\n　地の文　\r\n@if (\r\n灯「台詞」\r\n";
    let dir = temp_dir(
        "fmt-bytes",
        &[
            ("project.toml", "format_version = 0\n"),
            ("story/a.scn", source),
        ],
    );
    let out = scn(&["fmt", "--json", "."], &dir);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    let list: Value = serde_json::from_str(&out.stdout).unwrap();
    let ids: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap())
        .collect();
    let [a, b] = ids.as_slice() else {
        panic!("2行に付くはず：{ids:?}");
    };
    let expected = format!("\u{feff}## a #s001\r\n　地の文　 ^{a}\r\n@if (\r\n灯「台詞」 ^{b}\r\n");
    assert_eq!(
        fs::read_to_string(dir.join("story/a.scn")).unwrap(),
        expected
    );
    let _ = fs::remove_dir_all(&dir);
}

/// 途中のファイルで書き込みに失敗しても、書き終えたファイルの行IDは出してから、終了コード2で終える。
/// 読み取り専用のファイルは書き換えない。
#[test]
fn fmt_write_failure_reports_written_files() {
    for json in [false, true] {
        let dir = temp_dir(
            &format!("fmt-fail-{json}"),
            &[
                ("project.toml", "format_version = 0\n"),
                ("story/a.scn", "## a #s001\n地の文\n"),
                ("story/b.scn", "## b #s002\n地の文\n"),
            ],
        );
        let locked = dir.join("story/b.scn");
        let mut permissions = fs::metadata(&locked).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&locked, permissions.clone()).unwrap();

        let args: &[&str] = if json {
            &["fmt", "--json", "."]
        } else {
            &["fmt", "."]
        };
        let out = scn(args, &dir);
        assert_eq!(out.code, FAILURE, "{}", out.stderr);
        assert!(out.stderr.contains("b.scn"), "{}", out.stderr);
        let id = if json {
            let list: Value = serde_json::from_str(&out.stdout).expect("JSONでない");
            let list = list.as_array().unwrap();
            assert_eq!(list.len(), 1, "{}", out.stdout);
            assert_eq!(list[0]["file"], "story/a.scn");
            list[0]["id"].as_str().unwrap().to_owned()
        } else {
            let lines = fmt_lines(&out.stdout);
            assert_eq!(lines.len(), 1, "{}", out.stdout);
            assert_eq!(lines[0].file, "story/a.scn");
            lines[0].id.clone().unwrap()
        };
        assert_eq!(
            fs::read_to_string(dir.join("story/a.scn")).unwrap(),
            format!("## a #s001\n地の文 ^{id}\n")
        );
        assert_eq!(fs::read_to_string(&locked).unwrap(), "## b #s002\n地の文\n");

        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(&locked, permissions).unwrap();
        let _ = fs::remove_dir_all(&dir);
    }
}

/// 出力するパスは、引数のフォルダにプロジェクトの中の相対パスをつなげたもの（`parse` と同じく、
/// 実行した場所から開ける）。引数が `.` なら、相対パスだけ。
#[test]
fn fmt_paths_follow_argument() {
    let dir = temp_dir(
        "fmt-paths",
        &[
            ("work/project.toml", "format_version = 0\n"),
            ("work/story/a.scn", "## a #s001\n地の文\n"),
        ],
    );
    let out = scn(&["fmt", "--check", "work"], &dir);
    assert_eq!(out.code, HAS_ERRORS, "{}", out.stderr);
    let lines = fmt_lines(&out.stdout);
    assert_eq!(
        Path::new(&lines[0].file),
        Path::new("work").join("story/a.scn")
    );
    let out = scn(&["fmt", "--check", "."], &dir.join("work"));
    assert_eq!(fmt_lines(&out.stdout)[0].file, "story/a.scn");
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
        &["fmt"][..],
        &["fmt", "crates"][..],
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
    for args in [
        &["ir", "."][..],
        &["parse", "story/a.scn"][..],
        &["fmt", "."][..],
    ] {
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
