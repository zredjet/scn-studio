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

/// `scn ir --compact`：整形しない1行のJSON（中身は整形したものと同じ。T2-10）。
#[test]
fn ir_compact() {
    let pretty = scn(&["ir", "samples/houkago"], &repo_root());
    let compact = scn(&["ir", "samples/houkago", "--compact"], &repo_root());
    assert_eq!(compact.code, SUCCESS, "{}", compact.stderr);
    assert_eq!(compact.stdout.trim_end().lines().count(), 1);
    assert!(compact.stdout.len() < pretty.stdout.len());
    let a: Value = serde_json::from_str(&pretty.stdout).unwrap();
    let b: Value = serde_json::from_str(&compact.stdout).unwrap();
    assert_eq!(a, b);
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

// ---- scn check ----

/// 完了条件：`scn check` の端から端までのテスト。サンプルでは、意図して残した診断だけが出て、
/// エラーが無いので終了コード0。行IDのある行の診断には、行IDを添える。
#[test]
fn check_sample() {
    let out = scn(&["check", "samples/houkago"], &repo_root());
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    let lines: Vec<&str> = out.stdout.lines().collect();
    let codes: Vec<&str> = lines.iter().map(|l| l.split(' ').nth(1).unwrap()).collect();
    assert_eq!(
        codes,
        [
            "I403", "W203", "I302", "I302", "I302", "W503", "I302", "I302", "I403", "W701", "W605",
            "W605"
        ]
    );
    assert!(lines.contains(
        &"samples/houkago/story/chapter01.scn:73:1 W203 メッセージウィンドウ（3行×24字）に収まりません（4行になります） ^k3ff"
    ));
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("samples/houkago/concept.md:10:11 W701 "))
    );
    assert!(
        out.stderr.contains("エラー 0件、警告 5件、情報 7件"),
        "{}",
        out.stderr
    );
}

/// `--stage 2`：自動巡回（T3-2、D063）の診断も出し、カバレッジとエンディングごとの到達条件を標準エラーに出す。
/// `--json` では診断と結果の対応表。既定（段階1）の出力は変わらない。段階は1と2だけ。
#[test]
fn check_stage2() {
    let out = scn(
        &["check", "samples/amayadori", "--stage", "2"],
        &repo_root(),
    );
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    assert_eq!(out.stdout, "");
    assert!(
        out.stderr
            .contains("自動巡回：状態 3、周回の始め方 1、シーン 12/12、選べた選択肢 5/5"),
        "{}",
        out.stderr
    );
    assert!(
        out.stderr
            .contains("エンディング stay_end（s10）：着く（状態 1。例：選択肢 「話しかける」 → 「行かなくていいんじゃないか」）"),
        "{}",
        out.stderr
    );
    let out = scn(&["check", "samples/houkago", "--stage", "2"], &repo_root());
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    assert!(out.stdout.contains(
        "samples/houkago/story/chapter01.scn:72:1 W506 この選択肢は、条件（勇気 >= 2）が真にならないので選べません ^k3fe"
    ));
    assert!(
        out.stderr
            .contains("エンディング bad_end（s061）：着かない"),
        "{}",
        out.stderr
    );
    let out = scn(
        &["check", "samples/houkago", "--stage", "2", "--json"],
        &repo_root(),
    );
    let value: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    let codes: Vec<&str> = value["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap())
        .collect();
    assert!(
        codes.contains(&"W506") && codes.contains(&"W509"),
        "{codes:?}"
    );
    assert_eq!(value["playthrough"]["complete"], true);
    assert_eq!(value["playthrough"]["endings"][0]["id"], "akari_end");
    assert_eq!(
        scn(&["check", "samples/houkago", "--stage", "3"], &repo_root()).code,
        2
    );
}

/// `--json` は、行と列と、あれば行IDを足した診断の配列。プロジェクトのフォルダを省くと今のフォルダ。
#[test]
fn check_json_in_current_dir() {
    let out = scn(&["check", "--json"], &repo_root().join("samples/houkago"));
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    let list: Value = serde_json::from_str(&out.stdout).expect("JSONでない");
    let w203 = list
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "W203")
        .unwrap();
    assert_eq!(w203["file"], "story/chapter01.scn");
    assert_eq!(w203["line"], 73);
    assert_eq!(w203["line_id"], "k3ff");
    let w605 = list
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "W605")
        .unwrap();
    assert_eq!(w605["file"], "variables.yaml");
    assert!(w605.get("line_id").is_none());
}

/// エラー（E）があれば終了コード1。段階1の検証の診断（W503 など）も出す。
#[test]
fn check_with_errors() {
    let dir = temp_dir(
        "check-errors",
        &[
            ("project.toml", "format_version = 0\nstart_scene = \"s9\"\n"),
            ("story/a.scn", "## a #s001\n地の文 ^aaaa\n"),
        ],
    );
    let out = scn(&["check", "."], &dir);
    assert_eq!(out.code, HAS_ERRORS, "{}", out.stderr);
    assert!(
        out.stdout.contains("project.toml:2:1 E501 "),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("story/a.scn:1:1 W503 "),
        "{}",
        out.stdout
    );
    let _ = fs::remove_dir_all(&dir);
}

/// 入れ子の深い本文でもスタックがあふれない（大きなスタックのスレッドで実行する。Windowsのメインスレッドは1MB）。
#[test]
fn deep_nesting_does_not_overflow() {
    let mut source = String::from("## a #s001\n");
    for _ in 0..3000 {
        source.push_str("@if a\n");
    }
    let dir = temp_dir(
        "deep",
        &[
            ("project.toml", "format_version = 0\n"),
            ("story/a.scn", &source),
        ],
    );
    // `ir` は整形したJSONを出すので、入れ子が深いと字下げで出力が大きくなる（3000段で約800MB）。ここでは試さない。
    for args in [&["check", "."][..], &["parse", "story/a.scn"][..]] {
        let out = scn(args, &dir);
        assert_eq!(out.code, HAS_ERRORS, "{args:?}：{}", out.stderr);
    }
    let _ = fs::remove_dir_all(&dir);
}

// ---- scn graph ----

/// 完了条件：`scn graph samples/houkago` が、粒度ごとに期待した一覧を出す。
#[test]
fn graph_levels() {
    let root = repo_root();
    let out = scn(&["graph", "samples/houkago"], &root);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    let lines: Vec<&str> = out.stdout.lines().collect();
    let at = lines
        .iter()
        .position(|l| l.starts_with("s012 放課後の屋上"))
        .unwrap();
    assert_eq!(
        &lines[at..at + 6],
        [
            "s012 放課後の屋上（samples/houkago/story/chapter01.scn、ルート 共通、draft）",
            "  -> s014（選択肢「黙ってうなずく」 if 勇気 >= 2）",
            "  -> s013a（条件 灯.好感度 >= 3）",
            "  -> s013b（@else の枝）",
            "  読む：akari.好感度、flag.勇気",
            "  書く：akari.好感度",
        ]
    );
    assert!(lines.contains(
        &"s014 沈黙のあと（samples/houkago/story/chapter01.scn、ルート 灯、draft、スタブ）"
    ));
    assert!(lines.contains(&"  @エンディング akari_end"));
    assert_eq!(lines.iter().filter(|l| !l.starts_with(' ')).count(), 13);

    let out = scn(
        &["graph", "--level", "chapter"],
        &root.join("samples/houkago"),
    );
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    assert_eq!(
        out.stdout.lines().take(5).collect::<Vec<_>>(),
        [
            "story/chapter01.scn（シーン s010、s011、s012、s013a、s013b、s014）",
            "  s013a -> s020 → story/chapter02.scn",
            "  s013b -> s030 → story/chapter03.scn",
            "  読む：akari.好感度、flag.勇気",
            "  書く：akari.好感度、flag.ルート、flag.勇気、tsumugi.好感度",
        ]
    );

    let out = scn(&["graph", "--level", "route", "samples/houkago"], &root);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    let headers: Vec<&str> = out.stdout.lines().filter(|l| !l.starts_with(' ')).collect();
    assert_eq!(
        headers,
        [
            "ルート 共通（シーン s010、s011、s012、s013b）",
            "ルート 灯（シーン s013a、s014、s020、s021、s022、s040）",
            "ルート 紬（シーン s030、s060、s061）",
        ]
    );
    assert!(out.stdout.contains("  s013b -> s030 → ルート 紬\n"));
    assert!(
        out.stdout
            .contains("  @エンディング tsumugi_end、bad_end\n")
    );
}

/// `--json` は、粒度と、まとまりのリスト。行き先の無い辺も出す。
#[test]
fn graph_json() {
    let dir = temp_dir(
        "graph-json",
        &[
            ("project.toml", "format_version = 0\n"),
            (
                "story/a.scn",
                "## a #s1\n@呼出 s2\n-> s9\n\n## b #s2\n@戻る\n",
            ),
        ],
    );
    let out = scn(&["graph", "--json", "."], &dir);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    let graph: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(graph["level"], "scene");
    let exits = graph["groups"][0]["exits"].as_array().unwrap();
    assert_eq!(exits[0]["kind"], "call");
    assert_eq!(exits[1]["to"], "s9");
    assert_eq!(exits[1]["to_exists"], false);
    let text = scn(&["graph", "."], &dir).stdout;
    assert!(
        text.contains("  @呼出 s2\n  -> s9 ※シーンがありません\n"),
        "{text}"
    );

    // `@elif` の枝も「条件」と書く（`@if` と書かない）。
    fs::write(
        dir.join("story/a.scn"),
        "## a #s1\n@if flag.y == 1\n  -> s1\n@elif flag.y == 2\n  -> s1\n@else\n  -> s1\n@end\n",
    )
    .unwrap();
    let text = scn(&["graph", "."], &dir).stdout;
    assert!(
        text.contains(
            "  -> s1（条件 flag.y == 1）\n  -> s1（条件 flag.y == 2）\n  -> s1（@else の枝）\n"
        ),
        "{text}"
    );
    let _ = fs::remove_dir_all(&dir);
}

// ---- scn new、scn format-doc ----

/// 完了条件：新しく作った作品フォルダで、`scn check` がエラー0件で終わる（スタブの行き止まりの警告だけ）。
/// 空でないフォルダには作らない。
#[test]
fn new_project() {
    let base = temp_dir("new", &[]);
    fs::create_dir_all(&base).unwrap();
    let out = scn(&["new", "my-novel", "--title", "夏の終わり"], &base);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    let dir = base.join("my-novel");
    let toml = fs::read_to_string(dir.join("project.toml")).unwrap();
    assert!(toml.contains("title = \"夏の終わり\""), "{toml}");
    assert!(
        fs::read_to_string(dir.join("CLAUDE.md"))
            .unwrap()
            .starts_with("# 夏の終わり（作品フォルダ）")
    );

    let out = scn(&["check"], &dir);
    assert_eq!(out.code, SUCCESS, "{}{}", out.stdout, out.stderr);
    assert!(
        out.stdout.starts_with("story/chapter01.scn:3:1 W503 "),
        "{}",
        out.stdout
    );
    assert!(
        out.stderr.contains("エラー 0件、警告 1件、情報 0件"),
        "{}",
        out.stderr
    );

    // 作品名を省くとフォルダ名。空でないフォルダには作らない。
    assert_eq!(scn(&["new", "other"], &base).code, SUCCESS);
    assert!(
        fs::read_to_string(base.join("other/project.toml"))
            .unwrap()
            .contains("title = \"other\"")
    );
    // `scn new .` でも、作品名は今のフォルダの名前。
    fs::create_dir_all(base.join("here")).unwrap();
    assert_eq!(scn(&["new", "."], &base.join("here")).code, SUCCESS);
    assert!(
        fs::read_to_string(base.join("here/project.toml"))
            .unwrap()
            .contains("title = \"here\"")
    );
    let out = scn(&["new", "my-novel"], &base);
    assert_eq!(out.code, FAILURE);
    assert!(
        out.stderr.contains("空のフォルダではありません"),
        "{}",
        out.stderr
    );
    let _ = fs::remove_dir_all(&base);
}

/// `format-doc --check` は、`FORMAT.md` が今の版と同じなら0、違えば（無ければ）1。`format-doc` で書き直す。
#[test]
fn format_doc() {
    let base = temp_dir("format-doc", &[]);
    fs::create_dir_all(&base).unwrap();
    assert_eq!(scn(&["new", "w"], &base).code, SUCCESS);
    let dir = base.join("w");
    assert_eq!(scn(&["format-doc", "--check"], &dir).code, SUCCESS);
    let original = fs::read_to_string(dir.join("FORMAT.md")).unwrap();
    assert!(original.contains("## 16. 診断の一覧"));
    assert!(original.contains("# 作品フォルダの規約"));
    fs::remove_file(dir.join("FORMAT.md")).unwrap();
    assert_eq!(scn(&["format-doc", "--check"], &dir).code, HAS_ERRORS);
    assert!(!dir.join("FORMAT.md").exists());
    assert_eq!(scn(&["format-doc"], &dir).code, SUCCESS);
    assert_eq!(fs::read_to_string(dir.join("FORMAT.md")).unwrap(), original);
    // 作品フォルダでなければ書かない。
    assert_eq!(scn(&["format-doc", "."], &base).code, FAILURE);
    assert!(!base.join("FORMAT.md").exists());
    let _ = fs::remove_dir_all(&base);
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

/// `scn fmt --provenance <タスク> --model <モデル>`：付けた行IDを、AIが書いた行として出自の記録に書き足す
/// （T3-4、D065）。今の記録は変えない。`--check` では書かない。`--model` が無ければ使い方の誤り。`scn provenance` は
/// AIの行の一覧と数を出し、`-o` で CSV に書く。
#[test]
fn fmt_provenance_and_listing() {
    let dir = temp_dir("fmt-provenance", &[]);
    copy_dir(&repo_root().join("samples/houkago"), &dir);
    let path = dir.join("ai/provenance.jsonl");
    let before = fs::read_to_string(&path).unwrap();
    assert_eq!(
        scn(&["fmt", ".", "--provenance", "external:claude-code"], &dir).code,
        2
    );
    let out = scn(
        &["fmt", ".", "--check", "--provenance", "t", "--model", "m"],
        &dir,
    );
    assert_eq!(out.code, 1, "{}", out.stderr);
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    let out = scn(
        &[
            "fmt",
            ".",
            "--provenance",
            "external:claude-code",
            "--model",
            "test-model",
        ],
        &dir,
    );
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    assert!(
        out.stderr.contains("出自を記録しました：5件"),
        "{}",
        out.stderr
    );
    let after = fs::read_to_string(&path).unwrap();
    assert!(after.starts_with(&before), "{after}");
    let added: Vec<&str> = after[before.len()..].lines().collect();
    assert_eq!(added.len(), 5);
    let ids: Vec<String> = fmt_lines(&out.stdout)
        .into_iter()
        .filter_map(|l| l.id)
        .collect();
    for (line, id) in added.iter().zip(&ids) {
        let v: Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["line"], id.as_str());
        assert_eq!(v["origin"], "ai");
        assert_eq!(v["task"], "external:claude-code");
        assert_eq!(v["model"], "test-model");
        assert!(v["at"].as_str().unwrap().ends_with('Z'));
    }
    // 企画書の AI の項目（T4-3）。
    fs::write(dir.join("concept.md"), "---\nlogline: AIの一文\n---\n").unwrap();
    let mut records = fs::read_to_string(&path).unwrap();
    records.push_str("{\"file\":\"concept.md\",\"key\":\"logline\",\"origin\":\"ai\",\"task\":\"draft_concept\",\"model\":\"m\",\"at\":\"2026-10-09T00:00:00Z\"}\n");
    fs::write(&path, records).unwrap();
    // 一覧：付けた5行と、もとの ai-edited の1行と、企画書の項目。
    let out = scn(&["provenance", "."], &dir);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    assert!(
        out.stderr.contains("AIの行 6（ai 5、ai-edited 1）"),
        "{}",
        out.stderr
    );
    assert!(
        out.stderr.contains("AIの項目 1（ai 1、ai-edited 0）"),
        "{}",
        out.stderr
    );
    assert_eq!(out.stdout.lines().count(), 7);
    assert!(
        out.stdout
            .contains("concept.md logline ai draft_concept m "),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains(" ^k3fa ai-edited draft_stub "),
        "{}",
        out.stdout
    );
    let out = scn(&["provenance", ".", "-o", "out/p.csv"], &dir);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    let csv = fs::read_to_string(dir.join("out/p.csv")).unwrap();
    assert!(csv.starts_with("\u{feff}行ID,ファイル,項目,シーン,出自,タスク,モデル,日時,本文\r\n"));
    assert!(
        csv.contains(",concept.md,logline,,ai,draft_concept,"),
        "{csv}"
    );
    let out = scn(&["provenance", ".", "--json"], &dir);
    let list: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 7);
    let field = list.as_array().unwrap().last().unwrap();
    assert_eq!(
        (field["key"].as_str(), field["text"].as_str()),
        (Some("logline"), Some("AIの一文"))
    );
    assert!(list[0].get("key").is_none());
    let _ = fs::remove_dir_all(&dir);
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

/// 使い方の誤り、無いファイル、プロジェクトでない（`project.toml` の無い）フォルダは、終了コード2。
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
}

/// 壊れた設定ファイルは、読み込みのエラーで止めずに診断（E801）にして解析を続ける（D042）。
/// エラー（E）の診断なので終了コード1。`parse` は、そのファイルの診断に加えて、設定ファイルの診断を出す。
/// 位置は設定ファイルの中で数え、パスは引数と同じ書き方でプロジェクトの根からつなげる。
#[test]
fn broken_settings_are_diagnostics() {
    let dir = temp_dir(
        "broken",
        &[
            ("work/project.toml", "format_version = 0\n"),
            ("work/variables.yaml", "flag.a:\n  type: int\nflag.b: [\n"),
            ("work/story/a.scn", "## a #s001\n@設定 flag.zz = 1\n"),
        ],
    );
    let work = dir.join("work");

    let out = scn(&["ir", "."], &work);
    assert_eq!(out.code, HAS_ERRORS, "{}", out.stderr);
    let ir: Value = serde_json::from_str(&out.stdout).expect("JSONでない");
    let codes: Vec<(&str, &str)> = ir["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| (d["code"].as_str().unwrap(), d["file"].as_str().unwrap()))
        .collect();
    // 宣言を丸ごと読めないので、宣言の無い変数（W602）は出さない。
    assert_eq!(codes, [("E801", "variables.yaml")]);

    let out = scn(&["parse", "story/a.scn"], &work);
    assert_eq!(out.code, HAS_ERRORS, "{}", out.stderr);
    assert!(
        out.stdout.starts_with("variables.yaml:4:1 E801 "),
        "{}",
        out.stdout
    );

    let out = scn(&["parse", "work/story/a.scn"], &dir);
    assert_eq!(out.code, HAS_ERRORS, "{}", out.stderr);
    let expected = Path::new("work").join("variables.yaml");
    assert!(
        out.stdout
            .starts_with(&format!("{}:4:1 E801 ", expected.display())),
        "{}",
        out.stdout
    );

    let out = scn(&["parse", "--json", "story/a.scn"], &work);
    let list: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(list[0]["code"], "E801");
    assert_eq!(list[0]["line"], 4);

    // 行IDの付与は、診断では止めない。
    assert_eq!(scn(&["fmt", "."], &work).code, SUCCESS);
    let _ = fs::remove_dir_all(&dir);
}

/// `project.toml` にエラーがあれば、台詞の括弧を書かれたとおりに読めないので、`fmt` は何も書かずに
/// 終了コード2で終える（D042）。`--check` も同じ。
#[test]
fn fmt_refuses_broken_project_toml() {
    let source = "## a #s001\n灯『一行目\n二行目』\n";
    let dir = temp_dir(
        "fmt-refuse",
        &[
            ("project.toml", "format_version = 0\n[dialogue\n"),
            ("story/a.scn", source),
        ],
    );
    for args in [&["fmt", "."][..], &["fmt", "--check", "."][..]] {
        let out = scn(args, &dir);
        assert_eq!(out.code, FAILURE, "{args:?}");
        assert!(out.stdout.is_empty(), "{}", out.stdout);
        assert!(out.stderr.contains("project.toml:2:"), "{}", out.stderr);
        assert!(out.stderr.contains("E801"), "{}", out.stderr);
    }
    assert_eq!(fs::read_to_string(dir.join("story/a.scn")).unwrap(), source);
    let _ = fs::remove_dir_all(&dir);
}

/// `parse` は、設定ファイルの診断を出すが、ほかの章の診断は、読み込みの診断（UTF-8でない章の E801）も出さない。
#[test]
fn parse_does_not_show_other_chapters() {
    let dir = temp_dir("other-chapters", &[]);
    copy_dir(&fixtures_dir().join("settings/story_not_utf8"), &dir);
    let out = scn(&["parse", "story/a.scn"], &dir);
    assert_eq!(out.code, SUCCESS, "{}{}", out.stdout, out.stderr);
    assert!(out.stdout.is_empty(), "{}", out.stdout);
    let out = scn(&["ir", "."], &dir);
    assert_eq!(out.code, HAS_ERRORS, "{}", out.stderr);
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

/// `export script`：既定では作品フォルダの `export/script.csv` に書き、パスと行数を出す。`-o -` は標準出力。
/// 行IDの無い台詞の数を標準エラーに出す（終了コードは変えない）。
#[test]
fn export_script() {
    let dir = std::env::temp_dir().join(format!("scn-cli-export-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    copy_dir(&repo_root().join("samples/houkago"), &dir);
    let _ = fs::remove_file(dir.join("export/script.csv"));
    let out = scn(&["export", "script"], &dir);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    let csv = fs::read_to_string(dir.join("export/script.csv")).unwrap();
    assert!(
        csv.starts_with("\u{feff}行ID,シーン,キャラ,台詞,直前の地の文,ボイスファイル名,備考\r\n")
    );
    assert!(csv.contains("k3f9,s012 放課後の屋上,灯,やっと来た。遅いよ,,akari_k3f9,\r\n"));
    assert!(out.stdout.contains("script.csv（16行）"), "{}", out.stdout);
    assert!(out.stderr.contains("台詞が 2 行"), "{}", out.stderr);
    let piped = scn(&["export", "script", ".", "-o", "-"], &dir);
    assert_eq!(piped.stdout, csv);
    // 無いフォルダへの書き出しは、フォルダを作る。作品フォルダの export/ が無くても作る。
    let out = scn(&["export", "script", ".", "-o", "out/sub/x.csv"], &dir);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    assert_eq!(fs::read_to_string(dir.join("out/sub/x.csv")).unwrap(), csv);
    fs::remove_dir_all(dir.join("export")).unwrap();
    assert_eq!(scn(&["export", "script"], &dir).code, SUCCESS);
    assert!(dir.join("export/script.csv").exists());
    // 作品フォルダでなければ2。
    assert_eq!(scn(&["export", "script", "story"], &dir).code, 2);
    let _ = fs::remove_dir_all(&dir);
}

/// `export script --format xlsx`：Excel 形式を `export/script.xlsx` に書く。`--baseline` で収録の基準を記録し、
/// 次からの書き出しで、変わった台詞の備考に「再収録候補」と書いて、数を標準エラーに出す（T2-10、D061）。
#[test]
fn export_script_xlsx_and_baseline() {
    let dir = std::env::temp_dir().join(format!("scn-cli-export-xlsx-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    copy_dir(&repo_root().join("samples/houkago"), &dir);
    let _ = fs::remove_file(dir.join("export/script-baseline.json"));
    let out = scn(&["export", "script", "--format", "xlsx"], &dir);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    assert!(out.stdout.contains("script.xlsx（16行）"), "{}", out.stdout);
    let bytes = fs::read(dir.join("export/script.xlsx")).unwrap();
    assert!(bytes.starts_with(b"PK\x03\x04"));
    // 基準が無ければ、再収録候補は付けない。
    let out = scn(&["export", "script", "--baseline"], &dir);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    assert!(!out.stderr.contains("再収録候補"), "{}", out.stderr);
    assert!(out.stderr.contains("収録の基準を"), "{}", out.stderr);
    assert!(dir.join("export/script-baseline.json").exists());
    // 台詞を直すと、次の書き出しで再収録候補になる。
    let chapter = dir.join("story/chapter01.scn");
    let text = fs::read_to_string(&chapter).unwrap();
    fs::write(
        &chapter,
        text.replace(
            "灯「やっと来た。遅いよ」 ^k3f9",
            "灯「やっと来たね。遅いよ」 ^k3f9",
        ),
    )
    .unwrap();
    let out = scn(&["export", "script"], &dir);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    assert!(
        out.stderr
            .contains("変わった台詞 1 行（再収録候補）、足した台詞 0 行、無くなった台詞 0 行"),
        "{}",
        out.stderr
    );
    let csv = fs::read_to_string(dir.join("export/script.csv")).unwrap();
    assert!(
        csv.contains("k3f9,s012 放課後の屋上,灯,やっと来たね。遅いよ,,akari_k3f9,再収録候補（収録の基準の後に変わった）\r\n"),
        "{csv}"
    );
    // 読めない基準は知らせて、再収録候補を付けずに書き出す。
    fs::write(dir.join("export/script-baseline.json"), "{").unwrap();
    let out = scn(&["export", "script"], &dir);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    assert!(
        out.stderr.contains("script-baseline.json を読めません"),
        "{}",
        out.stderr
    );
    let _ = fs::remove_dir_all(&dir);
}

/// `scn export orders`：まだファイルの無い素材の表を書き出し、パスと行数を出す。`-o -` で標準出力。
#[test]
fn export_orders() {
    let dir = std::env::temp_dir().join(format!("scn-cli-orders-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    copy_dir(&repo_root().join("samples/houkago"), &dir);
    let _ = fs::remove_file(dir.join("export/orders.csv"));
    let out = scn(&["export", "orders"], &dir);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    let csv = fs::read_to_string(dir.join("export/orders.csv")).unwrap();
    assert!(csv.starts_with("\u{feff}種別,名前,キャラ,差分,使用回数,初出,使用シーン,備考\r\n"));
    assert!(
        csv.contains("SE,チャイム,,,1,s011,s011,カタログに無い\r\n"),
        "{csv}"
    );
    assert!(out.stdout.contains("orders.csv（21行）"), "{}", out.stdout);
    let piped = scn(&["export", "orders", ".", "-o", "-"], &dir);
    assert_eq!(piped.stdout, csv);
    assert_eq!(scn(&["export", "orders", "story"], &dir).code, 2);
    let _ = fs::remove_dir_all(&dir);
}

/// `scn export text`：共通とルートごとに export/text/ に書く。`--route`・`--ending` で1つの道、`-o -` で標準出力。
#[test]
fn export_text() {
    let dir = std::env::temp_dir().join(format!("scn-cli-text-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    copy_dir(&repo_root().join("samples/houkago"), &dir);
    let out = scn(&["export", "text"], &dir);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    for name in ["共通のみ", "灯ルート", "紬ルート"] {
        assert!(
            dir.join(format!("export/text/{name}.txt")).exists(),
            "{name}"
        );
    }
    let akari = fs::read_to_string(dir.join("export/text/灯ルート.txt")).unwrap();
    assert!(akari.starts_with("放課後の灯（灯ルート）\n"));
    assert!(akari.contains("〔エンディング：akari_end〕"));
    let piped = scn(&["export", "text", ".", "--route", "灯", "-o", "-"], &dir);
    assert_eq!(piped.stdout, akari);
    let bare = scn(
        &[
            "export",
            "text",
            ".",
            "--ending",
            "tsumugi_end",
            "--no-notes",
            "-o",
            "-",
        ],
        &dir,
    );
    assert!(!bare.stdout.contains("〔選択"), "{}", bare.stdout);
    // 指定した道が無い・着けないときは、知らせて書かずに2。
    let bad = scn(&["export", "text", ".", "--ending", "bad_end"], &dir);
    assert_eq!(bad.code, 2, "{}", bad.stderr);
    assert!(bad.stderr.contains("bad_end"), "{}", bad.stderr);
    assert!(
        !dir.join("export/text/エンディング bad_end まで.txt")
            .exists()
    );
    assert_eq!(
        scn(&["export", "text", ".", "--route", "無い"], &dir).code,
        2
    );
    // -o は1つの道だけ。
    assert_eq!(scn(&["export", "text", ".", "-o", "x.txt"], &dir).code, 2);
    let _ = fs::remove_dir_all(&dir);
    // ルートの無い作品は、通しで書く。
    let fresh = std::env::temp_dir().join(format!("scn-cli-text-new-{}", std::process::id()));
    let _ = fs::remove_dir_all(&fresh);
    let parent = fresh.parent().unwrap().to_owned();
    let made = scn(&["new", fresh.to_str().unwrap()], &parent);
    assert_eq!(made.code, SUCCESS, "{}", made.stderr);
    let out = scn(&["export", "text"], &fresh);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    assert!(
        fresh.join("export/text/通し.txt").exists(),
        "{}",
        out.stdout
    );
    let _ = fs::remove_dir_all(&fresh);
}

/// `scn export renpy`：書き出し先（既定はプロファイルの output）の game/scn/ を作り、書き出し先とファイルの
/// 数を出す。知らせとプロファイルの誤りは標準エラー。終了コードはエラーの診断で決まる。
#[test]
fn export_renpy() {
    let dir = std::env::temp_dir().join(format!("scn-cli-renpy-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    copy_dir(&repo_root().join("samples/amayadori"), &dir);
    let _ = fs::remove_dir_all(dir.join("export/renpy"));
    let out = scn(&["export", "renpy"], &dir);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    assert!(out.stdout.contains("renpy（4ファイル）"), "{}", out.stdout);
    assert!(
        out.stderr.contains("SourceHanSansLite.ttf"),
        "{}",
        out.stderr
    );
    let story = fs::read_to_string(dir.join("export/renpy/game/scn/story_chapter02.rpy")).unwrap();
    assert!(story.contains("label s07:"), "{story}");
    // 書き出し先を指定する。プロファイルの誤りは知らせて、既定で書き出す。
    fs::write(dir.join("export/renpy.yaml"), "output: ../x\n").unwrap();
    let out = scn(&["export", "renpy", ".", "-o", "out/rp"], &dir);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    assert!(dir.join("out/rp/game/scn/scn_defs.rpy").exists());
    assert!(
        out.stderr
            .contains("export/renpy.yaml：output は作品フォルダの中を指す"),
        "{}",
        out.stderr
    );
    // 人の置いた game/scn/ は消さずに止める（2）。
    fs::create_dir_all(dir.join("mine/game/scn")).unwrap();
    fs::write(dir.join("mine/game/scn/a.rpy"), "# mine\n").unwrap();
    let out = scn(&["export", "renpy", ".", "-o", "mine"], &dir);
    assert_eq!(out.code, 2, "{}", out.stderr);
    assert!(dir.join("mine/game/scn/a.rpy").exists());
    // 作品フォルダでなければ2。
    assert_eq!(scn(&["export", "renpy", "story"], &dir).code, 2);
    let _ = fs::remove_dir_all(&dir);
}

/// `scn export tyrano`：書き出し先（既定はプロファイルの output）の data/scenario/scn/ と素材の data/*/scn/ を
/// 作り、書き出し先とファイルの数を出す。人の置いたフォルダは消さずに止める（2）。
#[test]
fn export_tyrano() {
    let dir = std::env::temp_dir().join(format!("scn-cli-tyrano-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    copy_dir(&repo_root().join("samples/amayadori"), &dir);
    let _ = fs::remove_dir_all(dir.join("export/tyrano"));
    let out = scn(&["export", "tyrano"], &dir);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    assert!(out.stdout.contains("tyrano（"), "{}", out.stdout);
    assert!(out.stderr.contains("SVG"), "{}", out.stderr);
    let story =
        fs::read_to_string(dir.join("export/tyrano/data/scenario/scn/story_chapter02.ks")).unwrap();
    assert!(story.contains("*s07\n"), "{story}");
    assert!(
        dir.join("export/tyrano/data/scenario/scn/scn_start.ks")
            .exists()
    );
    // 書き出し先を指定する。
    let out = scn(
        &["export", "tyrano", ".", "-o", "out/ty", "--no-ai-drafts"],
        &dir,
    );
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    assert!(dir.join("out/ty/data/scenario/scn/scn_start.ks").exists());
    // 人の置いた data/scenario/scn/ は消さずに止める（2）。
    fs::create_dir_all(dir.join("mine/data/scenario/scn")).unwrap();
    fs::write(dir.join("mine/data/scenario/scn/a.ks"), "; mine\n").unwrap();
    let out = scn(&["export", "tyrano", ".", "-o", "mine"], &dir);
    assert_eq!(out.code, 2, "{}", out.stderr);
    assert!(dir.join("mine/data/scenario/scn/a.ks").exists());
    let _ = fs::remove_dir_all(&dir);
}

/// `scn export utage`：書き出し先（既定はプロファイルの output）に、シナリオと設定のブックと素材を書き、
/// 書き出し先とファイルの数を出す。印の無い、空でないフォルダは消さずに止める（2）。
#[test]
fn export_utage() {
    let dir = std::env::temp_dir().join(format!("scn-cli-utage-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    copy_dir(&repo_root().join("samples/amayadori"), &dir);
    let _ = fs::remove_dir_all(dir.join("export/utage"));
    let out = scn(&["export", "utage"], &dir);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    assert!(out.stdout.contains("utage（"), "{}", out.stdout);
    assert!(out.stderr.contains("PNG"), "{}", out.stderr);
    assert!(dir.join("export/utage/scn.xlsx").is_file());
    assert!(dir.join("export/utage/scn_settings.xlsx").is_file());
    // 印の無い、空でないフォルダは消さずに止める（2）。
    fs::create_dir_all(dir.join("mine")).unwrap();
    fs::write(dir.join("mine/a.xlsx"), "mine").unwrap();
    let out = scn(&["export", "utage", ".", "-o", "mine"], &dir);
    assert_eq!(out.code, 2, "{}", out.stderr);
    assert!(dir.join("mine/a.xlsx").exists());
    let _ = fs::remove_dir_all(&dir);
}

/// `scn export epub`・`scn export pdf`：道ごとに export/epub/<道>.epub と export/pdf/<道>.html を書く。
/// `-o` は1つの道のときだけ。着けない道を指定したら2。
#[test]
fn export_epub_and_pdf() {
    let dir = std::env::temp_dir().join(format!("scn-cli-book-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    copy_dir(&repo_root().join("samples/houkago"), &dir);
    let out = scn(&["export", "epub"], &dir);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    assert!(
        dir.join("export/epub/灯ルート.epub").is_file(),
        "{}",
        out.stdout
    );
    let out = scn(
        &[
            "export",
            "pdf",
            ".",
            "--route",
            "灯",
            "--horizontal",
            "-o",
            "out/a.html",
        ],
        &dir,
    );
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    let html = fs::read_to_string(dir.join("out/a.html")).unwrap();
    assert!(html.contains("writing-mode: horizontal-tb;"), "{html}");
    assert!(out.stderr.contains("vivliostyle"), "{}", out.stderr);
    // `-o -` は標準出力（「-」という名前のファイルを作らない）。
    let out = scn(&["export", "pdf", ".", "--route", "灯", "-o", "-"], &dir);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    assert!(out.stdout.starts_with("<!DOCTYPE html>"), "{}", out.stdout);
    assert!(!dir.join("-").exists());
    assert_eq!(scn(&["export", "epub", ".", "-o", "x.epub"], &dir).code, 2);
    assert_eq!(
        scn(&["export", "epub", ".", "--route", "無い"], &dir).code,
        2
    );
    let _ = fs::remove_dir_all(&dir);
}

/// 外部コマンドとして呼ぶときの `command`（このテストの実行ファイルを `scn-plugin-mode=<動き>` で呼ぶ。YAML の
/// 流れの形の列）。
fn child(mode: &str) -> String {
    serde_json::to_string(&[
        std::env::current_exe().unwrap().display().to_string(),
        "child_command".to_owned(),
        "--exact".to_owned(),
        "--nocapture".to_owned(),
        format!("scn-plugin-mode={mode}"),
    ])
    .unwrap()
}

/// 外部コマンドとして呼ばれたときの動き（T3-14）。ふつうのテストの実行では何もしない。
#[test]
fn child_command() {
    let Some(mode) =
        std::env::args().find_map(|a| a.strip_prefix("scn-plugin-mode=").map(str::to_owned))
    else {
        return;
    };
    let code = match mode.as_str() {
        "write" => {
            let ir: Value = serde_json::from_reader(std::io::stdin()).unwrap();
            let output = PathBuf::from(std::env::var("SCN_OUTPUT").unwrap());
            let scenes = ir["scenes"].as_array().unwrap().len();
            fs::write(output.join("n.txt"), scenes.to_string()).unwrap();
            println!("{scenes}シーンを書きました");
            0
        }
        "fail" => {
            eprintln!("壊れた入力です");
            3
        }
        "typeset" => {
            let input = std::env::var("SCN_INPUT").unwrap();
            let html = fs::read_to_string(input).unwrap();
            fs::write(
                std::env::var("SCN_OUTPUT").unwrap(),
                format!("PDF {}", html.len()),
            )
            .unwrap();
            0
        }
        _ => 2,
    };
    std::process::exit(code);
}

/// `scn export <プロファイル名>`：テンプレートと外部コマンドのプラグイン（T3-14）。
#[test]
fn export_plugins() {
    let dir = std::env::temp_dir().join(format!("scn-cli-plugin-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    copy_dir(&repo_root().join("samples/amayadori"), &dir);
    fs::create_dir_all(dir.join("tools")).unwrap();
    fs::write(
        dir.join("tools/titles.txt.j2"),
        "{% for scene in ir.scenes %}{{ scene.id }} {{ scene.title }}\n{% endfor %}",
    )
    .unwrap();
    fs::write(
        dir.join("export/titles.yaml"),
        "template: tools/titles.txt.j2\n",
    )
    .unwrap();
    fs::write(
        dir.join("export/count.yaml"),
        format!("command: {}\n", child("write")),
    )
    .unwrap();
    fs::write(
        dir.join("export/broken.yaml"),
        format!("command: {}\n", child("fail")),
    )
    .unwrap();

    let out = scn(&["export", "titles"], &dir);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    let text = fs::read_to_string(dir.join("export/titles/titles.txt")).unwrap();
    assert!(text.starts_with("s01 改札前\ns02 待合室\n"), "{text}");
    let out = scn(&["export", "titles", ".", "-o", "-"], &dir);
    assert_eq!(out.stdout, text);

    let out = scn(&["export", "count", "."], &dir);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    assert!(
        out.stdout.contains("12シーンを書きました"),
        "{}",
        out.stdout
    );
    assert!(out.stderr.contains("scn: 実行します："), "{}", out.stderr);
    assert_eq!(
        fs::read_to_string(dir.join("export/count/n.txt")).unwrap(),
        "12"
    );

    let out = scn(&["export", "broken"], &dir);
    assert_eq!(out.code, FAILURE);
    assert!(out.stderr.contains("壊れた入力です"), "{}", out.stderr);
    assert!(out.stderr.contains("終了コード 3"), "{}", out.stderr);

    let out = scn(&["export", "nothing"], &dir);
    assert_eq!(out.code, FAILURE);
    assert!(
        out.stderr
            .contains("プラグインのプロファイル：broken、count、titles"),
        "{}",
        out.stderr
    );
    assert_eq!(scn(&["export", "titles", "--bad-flag"], &dir).code, FAILURE);

    // 縦書きPDFを組むコマンド（export/book.yaml の pdf_command）。
    let out = scn(
        &["export", "pdf", ".", "--route", "志乃", "--typeset"],
        &dir,
    );
    assert_eq!(out.code, FAILURE, "pdf_command が無い");
    fs::write(
        dir.join("export/book.yaml"),
        format!("pdf_command: {}\n", child("typeset")),
    )
    .unwrap();
    // --typeset が無ければ、pdf_command は実行しない。
    let out = scn(&["export", "pdf", ".", "--route", "志乃"], &dir);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    assert!(!dir.join("export/pdf/志乃ルート.pdf").exists());
    assert!(out.stderr.contains("--typeset"), "{}", out.stderr);
    let out = scn(
        &["export", "pdf", ".", "--route", "志乃", "--typeset"],
        &dir,
    );
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    let pdf = fs::read_to_string(dir.join("export/pdf/志乃ルート.pdf")).unwrap();
    assert!(pdf.starts_with("PDF "), "{pdf}");
    assert_eq!(
        scn(&["export", "epub", ".", "--typeset"], &dir).code,
        FAILURE
    );
    let _ = fs::remove_dir_all(&dir);
}

/// 作品フォルダを相対のパスで渡しても、区切りを含むコマンドの名前は作品フォルダから数える（OS によらない）。
#[test]
fn export_plugin_relative_project() {
    let base = std::env::temp_dir().join(format!("scn-cli-plugin-rel-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let dir = base.join("proj");
    copy_dir(&repo_root().join("samples/amayadori"), &dir);
    let exe = std::env::current_exe().unwrap();
    let file = match exe.extension() {
        Some(ext) => format!("child.{}", ext.to_string_lossy()),
        None => "child".to_owned(),
    };
    fs::create_dir_all(dir.join("tools")).unwrap();
    fs::copy(&exe, dir.join("tools").join(&file)).unwrap();
    let mut argv: Vec<String> = serde_json::from_str(&child("write")).unwrap();
    argv[0] = format!("tools/{file}");
    fs::write(
        dir.join("export/rel.yaml"),
        format!("command: {}\n", serde_json::to_string(&argv).unwrap()),
    )
    .unwrap();
    let out = scn(&["export", "rel", "proj"], &base);
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    assert_eq!(
        fs::read_to_string(dir.join("export/rel/n.txt")).unwrap(),
        "12"
    );
    let _ = fs::remove_dir_all(&base);
}

/// `scn ai context`：タスクの文脈を組み立てて出す（LLM は呼ばない）。実行できなければ1、無いタスクは2。
#[test]
fn ai_context() {
    let dir = repo_root().join("samples/houkago");
    let out = scn(
        &["ai", "context", "tone_check", ".", "--scene", "s012"],
        &dir,
    );
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    assert!(out.stdout.starts_with("=== system ===\n"), "{}", out.stdout);
    assert!(out.stdout.contains("# 依頼"), "{}", out.stdout);
    assert!(
        out.stdout.contains("灯「やっと来た。遅いよ」 ^k3f9"),
        "{}",
        out.stdout
    );
    assert!(out.stderr.contains("文脈の長さ 8192"), "{}", out.stderr);

    let out = scn(&["ai", "context", "tone_check", "."], &dir);
    assert_eq!(out.code, HAS_ERRORS);
    assert!(out.stderr.contains("シーンを選びます"), "{}", out.stderr);
    assert_eq!(out.stdout, "");

    let out = scn(
        &[
            "ai",
            "context",
            "draft_stub",
            ".",
            "--scene",
            "s014",
            "--tokens",
            "1200",
        ],
        &dir,
    );
    assert_eq!(out.code, HAS_ERRORS, "{}", out.stderr);
    assert!(
        out.stderr.contains("文体（文体見本）を削りません"),
        "{}",
        out.stderr
    );
    assert!(
        out.stderr.contains("削った：企画書（concept）"),
        "{}",
        out.stderr
    );

    let out = scn(
        &[
            "ai",
            "context",
            "setting_check",
            ".",
            "--scene",
            "s012",
            "--json",
        ],
        &dir,
    );
    assert_eq!(out.code, SUCCESS, "{}", out.stderr);
    let value: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(value["task"], "setting_check");
    assert_eq!(value["sections"][0]["item"], "previous_summaries");

    let out = scn(&["ai", "context", "nothing", "."], &dir);
    assert_eq!(out.code, FAILURE);
    assert!(
        out.stderr.contains("ai/nothing.yaml がありません"),
        "{}",
        out.stderr
    );
}

/// `--no-ai-drafts`（T3-9）：AI下書きのシーンの台詞と地の文を省く（台本の行、通読の本文）。
#[test]
fn export_without_ai_drafts() {
    let dir = repo_root().join("samples/houkago");
    let all = scn(&["export", "script", ".", "-o", "-"], &dir);
    assert_eq!(all.code, SUCCESS, "{}", all.stderr);
    assert!(all.stdout.contains("s021"), "{}", all.stdout);
    let without = scn(
        &["export", "script", ".", "--no-ai-drafts", "-o", "-"],
        &dir,
    );
    assert_eq!(without.code, SUCCESS, "{}", without.stderr);
    assert!(!without.stdout.contains("s021"), "{}", without.stdout);
    let text = scn(
        &[
            "export",
            "text",
            ".",
            "--route",
            "灯",
            "--no-ai-drafts",
            "-o",
            "-",
        ],
        &dir,
    );
    assert_eq!(text.code, SUCCESS, "{}", text.stderr);
    assert!(
        text.stdout.contains("AI下書きのため省いたシーン"),
        "{}",
        text.stdout
    );
}

/// `scn lsp`：標準入出力の言語サーバー（T3-15）。初期化して作品の診断を受け取り、終わらせると0で終わる。
#[test]
fn lsp_over_stdio() {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::process::{Command as Process, Stdio};

    let root = fs::canonicalize(repo_root().join("samples/amayadori")).unwrap();
    let mut child = Process::new(env!("CARGO_BIN_EXE_scn"))
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut send = |message: Value| {
        let body = message.to_string();
        write!(stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        stdin.flush().unwrap();
    };
    let mut receive = || -> Value {
        let mut length = 0;
        loop {
            let mut line = String::new();
            stdout.read_line(&mut line).unwrap();
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(n) = line.strip_prefix("Content-Length: ") {
                length = n.parse().unwrap();
            }
        }
        let mut body = vec![0; length];
        stdout.read_exact(&mut body).unwrap();
        serde_json::from_slice(&body).unwrap()
    };
    // file の URI（Windows は `file:///C:/…`。canonicalize の `\\?\` は外す）。
    let path = root.display().to_string();
    let path = path
        .strip_prefix(r"\\?\")
        .unwrap_or(&path)
        .replace('\\', "/");
    let folder = if path.starts_with('/') {
        format!("file://{path}")
    } else {
        format!("file:///{path}")
    };
    send(serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "capabilities": {}, "workspaceFolders": [{ "uri": folder, "name": "amayadori" }] },
    }));
    let response = receive();
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["serverInfo"]["name"], "scn");
    send(serde_json::json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));
    send(serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "shutdown" }));
    // amayadori は診断0件なので、知らせは来ずに shutdown の応答が来る。
    let response = receive();
    assert_eq!(response["id"], 2, "{response}");
    send(serde_json::json!({ "jsonrpc": "2.0", "method": "exit" }));
    assert!(child.wait().unwrap().success());
}
