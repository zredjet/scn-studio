//! Claude Code（CLI）で呼ぶ（T4-26、N3）：模擬の CLI で確かめる。実際の CLI もログインも使わない。
//!
//! このテストの実行ファイルを `claude` という名前でつなぎ（ハードリンク、できなければ写す）、その名前で起動された
//! ときは模擬の CLI として動く（`harness = false`。Windows と macOS で同じに動く）。模擬の CLI は、同じフォルダの
//! `mode.txt` の返し方で返し、受け取った引数・標準入力・指示のファイル・作業のフォルダを書き残す。

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use scn_core::ai::{
    self, AiError, CallOptions, ChatMessage, ChatRequest, Connection, Provider, Received, Role, Via,
};

/// テストの名前と中身（模擬の CLI の場所を受け取る）。
type Case = (&'static str, fn(&Path));

fn main() {
    let exe = std::env::current_exe().unwrap();
    if exe.file_stem().and_then(|s| s.to_str()) == Some("claude") {
        fake(exe.parent().unwrap());
        return;
    }
    let tests: [Case; 9] = [
        ("streams_text_and_thinking", streams_text_and_thinking),
        ("whole_reply_without_deltas", whole_reply_without_deltas),
        ("login_and_crash_errors", login_and_crash_errors),
        (
            "refuses_tools_and_stops_after_result",
            refuses_tools_and_stops_after_result,
        ),
        ("timeout_and_cancel", timeout_and_cancel),
        ("missing_cli_and_cloud_rules", missing_cli_and_cloud_rules),
        ("version", version),
        ("preview_and_machine_settings", preview_and_machine_settings),
        ("find_written_cli", find_written_cli),
    ];
    let dir = std::env::temp_dir().join(format!("scn-core-claude-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let cli = dir.join(format!("claude{}", std::env::consts::EXE_SUFFIX));
    if fs::hard_link(&exe, &cli).is_err() {
        fs::copy(&exe, &cli).unwrap();
    }
    for (name, test) in tests {
        print!("test {name} ... ");
        test(&cli);
        println!("ok");
    }
    let _ = fs::remove_dir_all(&dir);
    println!(
        "\ntest result: ok. {} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out",
        tests.len()
    );
}

// ---- 模擬の CLI ----

fn fake(dir: &Path) {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version") {
        println!("9.9.9 (Claude Code)");
        return;
    }
    if args.first().map(String::as_str) == Some("auth") {
        println!(r#"{{"loggedIn":true,"authMethod":"claude.ai","apiProvider":"firstParty"}}"#);
        return;
    }
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    fs::write(dir.join("args.txt"), args.join("\n")).unwrap();
    fs::write(dir.join("stdin.txt"), &input).unwrap();
    fs::write(
        dir.join("cwd.txt"),
        std::env::current_dir().unwrap().display().to_string(),
    )
    .unwrap();
    fs::write(
        dir.join("path.txt"),
        std::env::var("PATH").unwrap_or_default(),
    )
    .unwrap();
    if let Some(at) = args.iter().position(|a| a == "--system-prompt-file") {
        let system = fs::read_to_string(&args[at + 1]).unwrap();
        fs::write(dir.join("system.txt"), system).unwrap();
    }
    let mode = fs::read_to_string(dir.join("mode.txt")).unwrap();
    let mut out = std::io::stdout();
    let mut line = |text: &str| {
        writeln!(out, "{text}").unwrap();
        out.flush().unwrap();
    };
    match mode.trim() {
        "stream" => {
            // 実際の CLI（--safe-mode）の始まりの行と同じく、ツールと MCP は空で、組み込みのプラグインは並ぶ。
            line(
                r#"{"type":"system","subtype":"init","model":"m","tools":[],"mcp_servers":[],"plugins":[{"name":"cc-plugin-telemetry","path":"builtin"},{"name":"rust-analyzer-lsp","path":"/p"}]}"#,
            );
            line(
                r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"考え中"}}}"#,
            );
            line(
                r#"{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"は"}}}"#,
            );
            line(
                r#"{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"い"}}}"#,
            );
            line(
                r#"{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"end_turn"}}}"#,
            );
            line(
                r#"{"type":"assistant","message":{"content":[{"type":"text","text":"はい"}],"stop_reason":"end_turn"}}"#,
            );
            line("壊れた行");
            line(
                r#"{"type":"result","subtype":"success","is_error":false,"result":"はい","usage":{"input_tokens":10,"cache_read_input_tokens":5,"output_tokens":3}}"#,
            );
        }
        "whole" => {
            line(
                r#"{"type":"assistant","message":{"content":[{"type":"text","text":"<think>こっそり</think>まとめて"}]}}"#,
            );
            line(
                r#"{"type":"result","subtype":"success","is_error":false,"result":"<think>こっそり</think>まとめて"}"#,
            );
        }
        "login" => {
            line(
                r#"{"type":"result","subtype":"success","is_error":true,"result":"Invalid API key · Please run /login"}"#,
            );
            std::process::exit(1);
        }
        "crash" => {
            eprintln!("こわれました");
            std::process::exit(2);
        }
        "slow" => {
            line(r#"{"type":"system","subtype":"init"}"#);
            thread::sleep(Duration::from_secs(5));
        }
        "tools" => {
            line(r#"{"type":"system","subtype":"init","tools":["Bash"],"mcp_servers":[]}"#);
            thread::sleep(Duration::from_secs(5));
        }
        "linger" => {
            line(r#"{"type":"result","subtype":"success","is_error":false,"result":"はい"}"#);
            thread::sleep(Duration::from_secs(5));
        }
        other => panic!("知らない返し方：{other}"),
    }
}

// ---- テスト ----

fn connection(cli: &Path) -> Connection {
    Connection {
        name: "cloud".to_owned(),
        provider: Provider::Anthropic,
        base_url: "https://api.anthropic.com/v1".to_owned(),
        model: "claude-test".to_owned(),
        cloud: true,
        endpoint: "anthropic https://api.anthropic.com/v1".to_owned(),
        allowed: true,
        context_tokens: 200_000,
        output_tokens: None,
        structured_output: false,
        via: Via::ClaudeCode,
        cli: Some(cli.display().to_string()),
    }
}

fn request() -> ChatRequest {
    ChatRequest {
        system: Some("指示の文。\n2行目".to_owned()),
        messages: vec![ChatMessage {
            role: Role::User,
            content: "「はい」とだけ答えてください。".to_owned(),
        }],
        max_tokens: Some(1024),
        temperature: None,
        schema: None,
    }
}

fn quick() -> CallOptions {
    CallOptions {
        connect_timeout: Duration::from_secs(2),
        idle_timeout: Duration::from_millis(600),
    }
}

fn mode(cli: &Path, mode: &str) -> PathBuf {
    let dir = cli.parent().unwrap().to_owned();
    for name in ["args.txt", "stdin.txt", "cwd.txt", "system.txt", "path.txt"] {
        let _ = fs::remove_file(dir.join(name));
    }
    fs::write(dir.join("mode.txt"), mode).unwrap();
    dir
}

fn call(conn: &Connection) -> (Result<ai::Completion, AiError>, Vec<(bool, String)>) {
    let cancel = AtomicBool::new(false);
    let mut received = Vec::new();
    let result = ai::complete(
        conn,
        None,
        &request(),
        &quick(),
        &cancel,
        &mut |r: Received<'_>| {
            received.push((r.thinking(), r.text().to_owned()));
        },
    );
    (result, received)
}

fn streams_text_and_thinking(cli: &Path) {
    let dir = mode(cli, "stream");
    let (result, received) = call(&connection(cli));
    let done = result.unwrap();
    assert_eq!(done.text, "はい");
    assert_eq!(done.thinking_chars, 3);
    assert_eq!(done.stop_reason.as_deref(), Some("end_turn"));
    assert_eq!((done.usage.input_tokens, done.usage.output_tokens), (15, 3));
    assert!(!done.usage.estimated);
    // 届いた順に渡す（まとまった返事の文字は重ねない）。
    assert_eq!(
        received,
        [
            (true, "考え中".to_owned()),
            (false, "は".to_owned()),
            (false, "い".to_owned())
        ]
    );
    // ツールを使わせず、指示はファイル、依頼は標準入力で渡す。
    let args = fs::read_to_string(dir.join("args.txt")).unwrap();
    let args: Vec<&str> = args.split('\n').collect();
    for expected in [
        "-p",
        "--safe-mode",
        "--strict-mcp-config",
        "--no-session-persistence",
        "--include-partial-messages",
    ] {
        assert!(args.contains(&expected), "{args:?}");
    }
    let at = |flag: &str| args.iter().position(|a| *a == flag).unwrap();
    assert_eq!(args[at("--tools") + 1], "");
    assert_eq!(args[at("--model") + 1], "claude-test");
    assert_eq!(args[at("--output-format") + 1], "stream-json");
    assert_eq!(
        fs::read_to_string(dir.join("stdin.txt")).unwrap(),
        "「はい」とだけ答えてください。"
    );
    assert_eq!(
        fs::read_to_string(dir.join("system.txt")).unwrap(),
        "指示の文。\n2行目"
    );
    // 作品フォルダの外の、決まった作業用のフォルダで動かす。指示のファイルは終わったら消す。
    let cwd = PathBuf::from(fs::read_to_string(dir.join("cwd.txt")).unwrap());
    assert_eq!(cwd.file_name().unwrap(), "scn-studio-claude-work");
    let system_file = PathBuf::from(args[at("--system-prompt-file") + 1]);
    assert!(!system_file.parent().unwrap().exists());
    // 子の PATH の先に CLI のフォルダを足す（npm で入れた CLI が隣の node を使えるように）。
    let path = fs::read_to_string(dir.join("path.txt")).unwrap();
    let first = std::env::split_paths(&path).next().unwrap();
    assert_eq!(first, cli.parent().unwrap());
}

fn whole_reply_without_deltas(cli: &Path) {
    mode(cli, "whole");
    let (result, received) = call(&connection(cli));
    let done = result.unwrap();
    // 考えた中身（<think>）は本文から除き、トークン数が無ければ見積もる。
    assert_eq!(done.text, "まとめて");
    assert_eq!(done.thinking_chars, 4);
    assert!(done.usage.estimated);
    assert_eq!(received.len(), 1);
}

fn login_and_crash_errors(cli: &Path) {
    mode(cli, "login");
    let error = call(&connection(cli)).0.unwrap_err();
    assert!(
        matches!(&error, AiError::Auth { message, .. } if message.contains("ログイン")),
        "{error:?}"
    );
    mode(cli, "crash");
    let error = call(&connection(cli)).0.unwrap_err();
    assert!(
        matches!(&error, AiError::Server(message) if message.contains("こわれました")),
        "{error:?}"
    );
}

fn refuses_tools_and_stops_after_result(cli: &Path) {
    // 始まりの行にツールがあれば、送る中身が見せたものと違いうるので止める。
    mode(cli, "tools");
    let started = Instant::now();
    let error = call(&connection(cli)).0.unwrap_err();
    assert!(
        matches!(&error, AiError::Server(message) if message.contains("tools")),
        "{error:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(3));
    // 終わりの行の後に出力が閉じなくても、少し待って止め、応答にする。
    mode(cli, "linger");
    let started = Instant::now();
    assert_eq!(call(&connection(cli)).0.unwrap().text, "はい");
    assert!(started.elapsed() < Duration::from_secs(4));
}

fn timeout_and_cancel(cli: &Path) {
    mode(cli, "slow");
    let started = Instant::now();
    assert_eq!(call(&connection(cli)).0.unwrap_err(), AiError::Timeout);
    assert!(started.elapsed() < Duration::from_secs(3));

    mode(cli, "slow");
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(200));
        flag.store(true, Ordering::Relaxed);
    });
    let options = CallOptions {
        connect_timeout: Duration::from_secs(2),
        idle_timeout: Duration::from_secs(10),
    };
    let started = Instant::now();
    let error = ai::complete(
        &connection(cli),
        None,
        &request(),
        &options,
        &cancel,
        &mut |_| {},
    )
    .unwrap_err();
    assert_eq!(error, AiError::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(3));
}

fn missing_cli_and_cloud_rules(cli: &Path) {
    let dir = mode(cli, "stream");
    // 書いた場所に CLI が無い。
    let mut conn = connection(cli);
    conn.cli = Some(dir.join("no-such-claude").display().to_string());
    let error = call(&conn).0.unwrap_err();
    assert!(
        matches!(&error, AiError::Connect(message) if message.contains("見つかりません")),
        "{error:?}"
    );
    // クラウドへ送ることを許可していなければ、CLI を起動しない。
    let mut conn = connection(cli);
    conn.allowed = false;
    assert!(matches!(
        call(&conn).0.unwrap_err(),
        AiError::CloudNotAllowed(_)
    ));
    assert!(!dir.join("args.txt").exists());
}

fn version(cli: &Path) {
    assert_eq!(
        ai::cli_version(cli, Duration::from_secs(10)).unwrap(),
        "9.9.9 (Claude Code)"
    );
    // ログインの様子（`claude auth status --json`。LLM は呼ばない）。
    assert_eq!(ai::cli_logged_in(cli, Duration::from_secs(10)), Some(true));
}

fn preview_and_machine_settings(cli: &Path) {
    // 送る前の確かめには、CLI の起動の仕方と指示と依頼を見せる（キーは出さない）。
    let preview = ai::request_preview(&connection(cli), &request());
    assert!(
        preview.contains("-p --safe-mode --output-format stream-json"),
        "{preview}"
    );
    assert!(preview.contains("--tools \"\""), "{preview}");
    assert!(preview.contains("指示の文。") && preview.contains("「はい」とだけ"));
    assert!(!preview.contains("x-api-key"));
    // `/` で始まる依頼は、スラッシュコマンドとして読まれないよう見出しを前に置く。
    let slash = ChatRequest {
        messages: vec![ChatMessage {
            role: Role::User,
            content: "/login して".to_owned(),
        }],
        ..request()
    };
    assert_eq!(ai::cli_prompt(&slash), "依頼：\n/login して");
    assert_eq!(ai::cli_prompt(&request()), "「はい」とだけ答えてください。");

    // 端末の設定：呼び方と CLI の場所は Messages API の接続先だけに効き、ほかの上書きを変えても残る。
    let decl = scn_core::project::AiConnectionDecl {
        name: "cloud".to_owned(),
        provider: Some("anthropic".to_owned()),
        model: Some("c".to_owned()),
        ..Default::default()
    };
    let mut machine = ai::MachineSettings::default();
    machine.set_via("cloud", Via::ClaudeCode, Some(" /opt/claude "));
    machine.set_override(
        "cloud",
        ai::Override {
            model: Some("c2".to_owned()),
            ..ai::Override::default()
        },
    );
    let conn = ai::resolve(&decl, &machine).unwrap();
    assert_eq!(conn.via, Via::ClaudeCode);
    // URL を変えた Messages API の接続先では API で呼ぶ（CLI は自分に設定された行き先へ送るので）。
    let proxied = scn_core::project::AiConnectionDecl {
        base_url: Some("https://proxy.example.com/v1".to_owned()),
        ..decl.clone()
    };
    assert_eq!(ai::resolve(&proxied, &machine).unwrap().via, Via::Api);
    assert_eq!(conn.cli.as_deref(), Some("/opt/claude"));
    assert_eq!(conn.model, "c2");
    assert!(conn.destination().ends_with("claude-code"));
    // キーは要らない。
    let (statuses, _) = ai::connection_statuses(
        std::slice::from_ref(&decl),
        &machine,
        &ai::MemoryKeys::default(),
    );
    assert!(!statuses[0].needs_key && statuses[0].via == Via::ClaudeCode);
    assert!(statuses[0].claude_code);
    // OpenAI 互換の接続先では、書いてあっても API で呼ぶ。
    let local = scn_core::project::AiConnectionDecl {
        name: "cloud".to_owned(),
        base_url: Some("http://localhost:1234/v1".to_owned()),
        model: Some("m".to_owned()),
        ..Default::default()
    };
    assert_eq!(ai::resolve(&local, &machine).unwrap().via, Via::Api);
    // API に戻すと、上書きのほかは残る。
    machine.set_via("cloud", Via::Api, None);
    let kept = &machine.connections["cloud"];
    assert!(kept.via.is_none() && kept.cli.is_none() && kept.model.as_deref() == Some("c2"));
}

fn find_written_cli(cli: &Path) {
    assert_eq!(
        ai::find_cli(Some(&cli.display().to_string())).as_deref(),
        Some(cli)
    );
    assert_eq!(ai::find_cli(Some("/no/such/claude")), None);
}
