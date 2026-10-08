//! 書き出しのプラグイン（`plugin`、T3-14、D075）：テンプレートと外部コマンドで、サンプルを書き出す（完了条件）。
//!
//! 外部コマンドは、このテストの実行ファイルそのものを、引数 `scn-plugin-mode=<動き>` を付けて呼ぶ（Rust の
//! 小さなコマンド。Windows と macOS で同じに動く）。ふつうにテストを走らせたときの `child_command` は何もしない。

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use scn_core::book::read_profile;
use scn_core::plugin::{
    PluginError, PluginKind, command_line, export_command, list_plugins, read_plugin,
    render_template, split_command, typeset,
};
use scn_core::{analyze, load_project};

fn sample(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples")
        .join(name)
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/plugin")
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
    let dir = std::env::temp_dir().join(format!("scn-core-plugin-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    copy_dir(&sample(name), &dir);
    dir
}

/// 外部コマンドとして呼ぶときの `command`（YAML の流れの形の列。JSON の文字列は YAML の文字列として読める）。
fn child(mode: &str, extra: &[&str]) -> String {
    let mut argv = vec![
        std::env::current_exe().unwrap().display().to_string(),
        "child_command".to_owned(),
        "--exact".to_owned(),
        "--nocapture".to_owned(),
        "--test-threads=1".to_owned(),
        format!("scn-plugin-mode={mode}"),
    ];
    argv.extend(extra.iter().map(|s| (*s).to_owned()));
    serde_json::to_string(&argv).unwrap()
}

/// 外部コマンドとして呼ばれたときの動き。ふつうのテストの実行では何もしない。
#[test]
fn child_command() {
    let Some(mode) =
        std::env::args().find_map(|a| a.strip_prefix("scn-plugin-mode=").map(str::to_owned))
    else {
        return;
    };
    let code = match mode.as_str() {
        // IR の JSON を読み、書き出し先にシーンの一覧と、受け取った環境を書く。
        "write" => {
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input).unwrap();
            let ir: serde_json::Value = serde_json::from_str(&input).unwrap();
            let output = PathBuf::from(std::env::var("SCN_OUTPUT").unwrap());
            let project = PathBuf::from(std::env::var("SCN_PROJECT").unwrap());
            let mut text = format!("ir_version {}\n", ir["ir_version"]);
            for scene in ir["scenes"].as_array().unwrap() {
                text.push_str(&format!(
                    "{} {}\n",
                    scene["id"].as_str().unwrap(),
                    scene["title"].as_str().unwrap()
                ));
            }
            let cwd = fs::canonicalize(std::env::current_dir().unwrap()).unwrap();
            text.push_str(&format!(
                "cwd_is_project {}\nname {}\nprofile_exists {}\nversion_set {}\noutput_arg {}\n",
                cwd == fs::canonicalize(&project).unwrap(),
                std::env::var("SCN_NAME").unwrap(),
                Path::new(&std::env::var("SCN_PROFILE").unwrap()).is_file(),
                !std::env::var("SCN_VERSION").unwrap().is_empty(),
                std::env::args().any(|a| a == format!("out={}", output.display())),
            ));
            fs::write(output.join("scenes.txt"), text).unwrap();
            println!("書きました");
            eprintln!("知らせです");
            0
        }
        "fail" => {
            eprintln!("壊れた入力です");
            3
        }
        "sleep" => {
            std::thread::sleep(Duration::from_secs(30));
            0
        }
        // 標準入力を読まずに終わる。
        "ignore" => 0,
        // 印刷用の HTML（SCN_INPUT）を読み、PDF の代わりのファイル（SCN_OUTPUT）を書く。引数の置き換えも確かめる。
        "typeset" => {
            let input = std::env::var("SCN_INPUT").unwrap();
            let output = std::env::var("SCN_OUTPUT").unwrap();
            let html = fs::read_to_string(&input).unwrap();
            let args_ok = std::env::args().any(|a| a == format!("in={input}"))
                && std::env::args().any(|a| a == format!("out={output}"));
            fs::write(&output, format!("PDF {} {args_ok}\n", html.len())).unwrap();
            0
        }
        _ => 2,
    };
    std::process::exit(code);
}

fn write_profile(root: &Path, name: &str, text: &str) {
    fs::create_dir_all(root.join("export")).unwrap();
    fs::write(root.join(format!("export/{name}.yaml")), text).unwrap();
}

/// テンプレートで、両サンプルを書き出す（スナップショット）。
#[test]
fn template_samples() {
    for (sample_name, snapshot) in [
        ("houkago", "houkago_scenes"),
        ("amayadori", "amayadori_scenes"),
    ] {
        let root = copy_sample(sample_name, snapshot);
        fs::create_dir_all(root.join("tools")).unwrap();
        fs::copy(fixture("scenes.md.j2"), root.join("tools/scenes.md.j2")).unwrap();
        write_profile(
            &root,
            "scenes",
            "template: tools/scenes.md.j2\nheading: 台詞の一覧\n",
        );
        let plugin = read_plugin(&root, "scenes").unwrap();
        assert_eq!(plugin.output, "export/scenes/scenes.md");
        assert!(plugin.problems.is_empty(), "{:?}", plugin.problems);
        let project = load_project(&root).unwrap();
        let ir = analyze(&project);
        let text = render_template(&root, &plugin, &project, &ir).unwrap();
        insta::assert_snapshot!(snapshot, text);
        let _ = fs::remove_dir_all(&root);
    }
}

/// 外部コマンドで、両サンプルを書き出す（IR の JSON を標準入力で渡し、コマンドが書き出し先に書く）。
#[test]
fn command_samples() {
    for (sample_name, snapshot) in [
        ("houkago", "houkago_command"),
        ("amayadori", "amayadori_command"),
    ] {
        let root = copy_sample(sample_name, snapshot);
        write_profile(
            &root,
            "list",
            &format!("command: {}\n", child("write", &["out={output}"])),
        );
        let plugin = read_plugin(&root, "list").unwrap();
        assert_eq!(plugin.output, "export/list");
        let project = load_project(&root).unwrap();
        let ir = analyze(&project);
        let output = root.join(&plugin.output);
        let result = export_command(&root, &plugin, &ir, &output, true).unwrap();
        assert!(result.stdout.contains("書きました"), "{result:?}");
        assert!(result.stderr.contains("知らせです"), "{result:?}");
        let written = fs::read_to_string(output.join("scenes.txt")).unwrap();
        insta::assert_snapshot!(snapshot, written);
        let _ = fs::remove_dir_all(&root);
    }
}

/// 失敗（終了コード）、タイムアウト、見つからないコマンド、標準入力を読まないコマンド、作品フォルダから数える
/// コマンドの名前。
#[test]
fn command_failures() {
    let root = copy_sample("amayadori", "failures");
    let project = load_project(&root).unwrap();
    let ir = analyze(&project);
    let run = |name: &str, command: &str| {
        write_profile(&root, name, command);
        let plugin = read_plugin(&root, name).unwrap();
        export_command(&root, &plugin, &ir, &root.join(&plugin.output), true)
    };
    match run("fail", &format!("command: {}\n", child("fail", &[]))) {
        Err(PluginError::Failed { status, output, .. }) => {
            assert_eq!(status, "終了コード 3");
            assert!(output.stderr.contains("壊れた入力です"), "{output:?}");
        }
        other => panic!("{other:?}"),
    }
    let started = Instant::now();
    match run(
        "sleep",
        &format!("command: {}\ntimeout: 1\n", child("sleep", &[])),
    ) {
        Err(PluginError::Timeout { seconds: 1, .. }) => {}
        other => panic!("{other:?}"),
    }
    assert!(started.elapsed() < Duration::from_secs(20));
    assert!(matches!(
        run("missing", "command: scn-no-such-command-for-test --x\n"),
        Err(PluginError::Spawn { .. })
    ));
    run("ignore", &format!("command: {}\n", child("ignore", &[]))).unwrap();
    // 区切りを含むコマンドの名前は作品フォルダから数える。
    let exe = std::env::current_exe().unwrap();
    let file = format!(
        "child{}",
        exe.extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default()
    );
    fs::create_dir_all(root.join("tools")).unwrap();
    fs::copy(&exe, root.join("tools").join(&file)).unwrap();
    let relative = child("ignore", &[]).replacen(
        &serde_json::to_string(&exe.display().to_string()).unwrap(),
        &serde_json::to_string(&format!("tools/{file}")).unwrap(),
        1,
    );
    run("relative", &format!("command: {relative}\n")).unwrap();
    let _ = fs::remove_dir_all(&root);
}

/// テンプレートの決まり：文中の記法のフィルタ `plain`、HTML の自動のエスケープ、同じフォルダの `include`、
/// 未定義の値の誤り（行つき）、末尾の改行。
#[test]
fn template_rules() {
    let root = std::env::temp_dir().join(format!("scn-core-plugin-{}-rules", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("story")).unwrap();
    fs::create_dir_all(root.join("tools")).unwrap();
    fs::write(
        root.join("project.toml"),
        "format_version = 0\ntitle = \"<題> & 名\"\n",
    )
    .unwrap();
    fs::write(
        root.join("story/a.scn"),
        "## 始まり #s1\n|灯《あかり》は《《確か》》に{=flag.n}と言った。\n@エンディング end\n",
    )
    .unwrap();
    let project = load_project(&root).unwrap();
    let ir = analyze(&project);
    let render = |name: &str, file: &str, template: &str| {
        fs::write(root.join("tools").join(file), template).unwrap();
        write_profile(&root, name, &format!("template: tools/{file}\n"));
        let plugin = read_plugin(&root, name).unwrap();
        render_template(&root, &plugin, &project, &ir)
    };
    let text = render(
        "plain",
        "plain.txt.j2",
        "{% for op in ir.scenes[0].ops if op.kind == \"narrate\" %}\n\
         {{ op.text | plain }}\n{{ op.text | plain(\"paren\") }}\n{{ op.text | plain(\"narou\") }}\n\
         {% endfor %}\n",
    )
    .unwrap();
    assert_eq!(
        text,
        "灯は確かに{=flag.n}と言った。\n\
         灯（あかり）は確かに{=flag.n}と言った。\n\
         ｜灯《あかり》は《《確か》》に{=flag.n}と言った。\n"
    );
    let html = render("page", "page.html.j2", "<h1>{{ project.title }}</h1>\n").unwrap();
    assert_eq!(html, "<h1>&lt;題&gt; &amp; 名</h1>\n");
    let txt = render("raw", "raw.txt", "{{ project.title }}").unwrap();
    assert_eq!(txt, "<題> & 名", "HTML 以外はエスケープしない");
    fs::write(root.join("tools/part.j2"), "部品：{{ ir.scenes[0].id }}\n").unwrap();
    let included = render("inc", "inc.j2", "{% include \"part.j2\" %}").unwrap();
    assert_eq!(included, "部品：s1\n");
    match render("outside", "outside.j2", "{% include \"../story/a.scn\" %}") {
        Err(PluginError::Template { .. }) => {}
        other => panic!("フォルダの外は読まない：{other:?}"),
    }
    match render("undefined", "undefined.j2", "一行目\n{{ nothing }}\n") {
        Err(PluginError::Template { message, .. }) => {
            assert!(message.contains("2 行目"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    let _ = fs::remove_dir_all(&root);
}

/// プロファイルの読み方：一覧、既定の書き出し先、誤り。
#[test]
fn profiles() {
    let root =
        std::env::temp_dir().join(format!("scn-core-plugin-{}-profiles", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    write_profile(
        &root,
        "b",
        "command: python tools/b.py --out \"{output}\"\ntimeout: 0\n",
    );
    write_profile(
        &root,
        "a",
        "template: tools/cast.csv.j2\noutput: ../x.csv\n",
    );
    write_profile(&root, "renpy", "command: echo\n");
    write_profile(&root, "notes", "title: プラグインではない\n");
    write_profile(&root, "both", "template: a.j2\ncommand: echo\n");
    write_profile(&root, "broken", "command: [\n");
    write_profile(&root, "self", "template: t.j2\noutput: export/self.yaml\n");
    write_profile(&root, "story", "template: t.j2\noutput: story/x.scn\n");
    let list = list_plugins(&root);
    let names: Vec<&str> = list.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["a", "b", "both", "broken", "self", "story"]);
    let a = read_plugin(&root, "a").unwrap();
    assert_eq!(
        a.kind,
        PluginKind::Template {
            template: "tools/cast.csv.j2".to_owned()
        }
    );
    assert_eq!(
        a.output, "export/a/cast.csv",
        "作品フォルダの外は既定に戻す"
    );
    assert_eq!(a.problems.len(), 1, "{:?}", a.problems);
    let b = read_plugin(&root, "b").unwrap();
    assert_eq!(
        b.kind,
        PluginKind::Command {
            argv: vec![
                "python".to_owned(),
                "tools/b.py".to_owned(),
                "--out".to_owned(),
                "{output}".to_owned()
            ]
        }
    );
    assert_eq!(b.timeout, 300, "0 秒は既定に戻す");
    assert_eq!(b.output, "export/b");
    assert!(matches!(
        read_plugin(&root, "renpy"),
        Err(PluginError::Profile { .. })
    ));
    assert!(matches!(
        read_plugin(&root, "notes"),
        Err(PluginError::Profile { .. })
    ));
    assert!(matches!(
        read_plugin(&root, "both"),
        Err(PluginError::Profile { .. })
    ));
    assert!(matches!(
        read_plugin(&root, "broken"),
        Err(PluginError::Profile { .. })
    ));
    assert!(matches!(
        read_plugin(&root, "none"),
        Err(PluginError::Missing(_))
    ));
    assert!(matches!(
        read_plugin(&root, "../a"),
        Err(PluginError::Missing(_))
    ));
    let own = read_plugin(&root, "self").unwrap();
    assert_eq!(own.output, "export/self/t", "プロファイルを上書きしない");
    let story = read_plugin(&root, "story").unwrap();
    assert_eq!(
        story.output, "export/story/t",
        "export/ の外（本文）には書かない"
    );
    let _ = fs::remove_dir_all(&root);
}

/// コマンドの文字列の区切り方と見せ方。
#[test]
fn command_words() {
    assert_eq!(
        split_command("python  tools/x.py \"a b\" 'c \"d\"' C:\\dir\\x \"\"").unwrap(),
        ["python", "tools/x.py", "a b", "c \"d\"", "C:\\dir\\x", ""]
    );
    assert!(split_command("echo \"open").is_err());
    assert_eq!(split_command("全角　空白").unwrap(), ["全角　空白"]);
    let argv: Vec<String> = ["npx", "vivliostyle", "a b.html", ""]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    assert_eq!(command_line(&argv), "npx vivliostyle \"a b.html\" \"\"");
}

/// 縦書きPDFを組むコマンド（`export/book.yaml` の `pdf_command`）：印刷用の HTML を渡して PDF を書かせる。
/// PDF が書き換わらなければ失敗。
#[test]
fn typeset_pdf() {
    let root = copy_sample("amayadori", "typeset");
    fs::create_dir_all(root.join("export/pdf")).unwrap();
    let input = root.join("export/pdf/通し.html");
    fs::write(&input, "<html>本文</html>").unwrap();
    write_profile(
        &root,
        "book",
        &format!(
            "pdf_command: {}\npdf_timeout: 30\n",
            child("typeset", &["in={input}", "out={output}"])
        ),
    );
    let (profile, problems) = read_profile(&root);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(profile.pdf_timeout, 30);
    let argv = profile.pdf_command.unwrap();
    let output = root.join("export/pdf/通し.pdf");
    typeset(&root, &argv, 30, &input, &output, true).unwrap();
    let pdf = fs::read_to_string(&output).unwrap();
    assert_eq!(pdf, format!("PDF {} true\n", "<html>本文</html>".len()));
    // 書かないコマンド（PDF が前のまま）は失敗。
    let quiet: Vec<String> = serde_json::from_str(&child("ignore", &[])).unwrap();
    match typeset(&root, &quiet, 30, &input, &output, true) {
        Err(PluginError::Failed { status, .. }) => {
            assert!(status.contains("できていません"), "{status}")
        }
        other => panic!("{other:?}"),
    }
    let _ = fs::remove_dir_all(&root);
}

/// 置き換えは1回だけ：入れた値の中の `{…}` は置き換えない（引数と環境変数が同じ値になる）。
#[test]
fn placeholders_once() {
    let root = copy_sample("amayadori", "once");
    write_profile(
        &root,
        "list",
        &format!("command: {}\n", child("write", &["out={output}"])),
    );
    let plugin = read_plugin(&root, "list").unwrap();
    let project = load_project(&root).unwrap();
    let ir = analyze(&project);
    let output = root.join("export/{name}-{version}");
    export_command(&root, &plugin, &ir, &output, true).unwrap();
    let written = fs::read_to_string(output.join("scenes.txt")).unwrap();
    assert!(written.contains("output_arg true"), "{written}");
    let _ = fs::remove_dir_all(&root);
}

/// テンプレートのフォルダの中のシンボリックリンクから、外のファイルを読まない（Unix だけ。Windows のリンクは
/// 作るのに権限が要る）。
#[cfg(unix)]
#[test]
fn template_symlink_stays_inside() {
    let root = copy_sample("amayadori", "symlink");
    fs::create_dir_all(root.join("tools")).unwrap();
    std::os::unix::fs::symlink(root.join("project.toml"), root.join("tools/link.j2")).unwrap();
    fs::write(root.join("tools/main.j2"), "{% include \"link.j2\" %}").unwrap();
    write_profile(&root, "link", "template: tools/main.j2\n");
    let plugin = read_plugin(&root, "link").unwrap();
    let project = load_project(&root).unwrap();
    let ir = analyze(&project);
    assert!(matches!(
        render_template(&root, &plugin, &project, &ir),
        Err(PluginError::Template { .. })
    ));
    let _ = fs::remove_dir_all(&root);
}
