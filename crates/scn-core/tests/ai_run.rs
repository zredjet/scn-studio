//! AIタスクの実行（T3-7）：応答から指摘を読み（崩れた応答を含む）、行IDで本文の行へ対応させる。模擬のサーバーで
//! 指摘のタスクとシーン要約のタスクを実行する。実際の LLM は呼ばない。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use scn_core::ai::{
    self, CallOptions, Connection, FindingSeverity, Provider, RawFinding, RunError,
};
use scn_core::{Project, analyze, load_project};

fn houkago() -> Project {
    load_project(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago")).unwrap()
}

fn raw(
    line: Option<&str>,
    scene: Option<&str>,
    severity: FindingSeverity,
    message: &str,
) -> RawFinding {
    RawFinding {
        line: line.map(str::to_owned),
        scene: scene.map(str::to_owned),
        severity,
        message: message.to_owned(),
    }
}

#[test]
fn parse_json_lines_and_broken_responses() {
    let text = r#"以下が指摘です。
```json
{"line": "^k3fg", "severity": "warning", "message": "口調メモより硬い"}
- {"line": "k3f9", "severity": "エラー", "message": "一人称が違う"},
{"scene": "s012", "severity": "info", "message": "全体に説明が多い"}
{"line": "k3fa", "severity": "warning"}
{"line": "k3fb", "message": "壊れた
```
"#;
    let (found, unread) = ai::parse_findings(text);
    assert_eq!(
        found,
        [
            raw(
                Some("k3fg"),
                None,
                FindingSeverity::Warning,
                "口調メモより硬い"
            ),
            raw(Some("k3f9"), None, FindingSeverity::Error, "一人称が違う"),
            raw(
                None,
                Some("s012"),
                FindingSeverity::Info,
                "全体に説明が多い"
            ),
        ]
    );
    assert_eq!(
        unread,
        [
            "以下が指摘です。",
            r#"{"line": "k3fa", "severity": "warning"}"#,
            r#"{"line": "k3fb", "message": "壊れた"#,
        ]
    );

    // まるごと JSON の配列、`{"findings": [...]}`、1つのオブジェクト。
    let (found, unread) =
        ai::parse_findings(r#"[{"line":"a1","message":"x","severity":"bogus"},{"message":""}]"#);
    assert_eq!(found, [raw(Some("a1"), None, FindingSeverity::Info, "x")]);
    assert_eq!(unread.len(), 1);
    let (found, _) = ai::parse_findings(r#"{"findings":[{"line":"a2","comment":"y"}]}"#);
    assert_eq!(found, [raw(Some("a2"), None, FindingSeverity::Info, "y")]);
    let (found, unread) = ai::parse_findings("");
    assert!(found.is_empty() && unread.is_empty());

    assert_eq!(
        ai::parse_summary("```\n「灯が転校を打ち明けかける。」\n```"),
        Some("灯が転校を打ち明けかける。".to_owned())
    );
    assert_eq!(ai::parse_summary("  \n"), None);
}

#[test]
fn resolve_by_line_id() {
    let project = houkago();
    let ir = analyze(&project);
    let found = ai::resolve_findings(
        &project,
        &ir,
        vec![
            raw(Some("k3ff"), None, FindingSeverity::Warning, "a"),
            raw(Some("zzzz"), None, FindingSeverity::Warning, "b"),
            raw(None, Some("s013a"), FindingSeverity::Info, "c"),
            raw(None, None, FindingSeverity::Info, "d"),
            raw(None, Some("s999"), FindingSeverity::Info, "e"),
        ],
        Some("s012"),
    );
    let chapter = project
        .files
        .iter()
        .find(|f| f.path == "story/chapter01.scn")
        .unwrap();
    let k3ff = found.first().unwrap();
    assert_eq!(
        (k3ff.scene.as_deref(), k3ff.file.as_deref(), k3ff.unresolved),
        (Some("s012"), Some("story/chapter01.scn"), false)
    );
    let span = k3ff.span.unwrap();
    assert!(
        chapter
            .text
            .get(span.start..span.end)
            .unwrap()
            .contains("^k3ff")
    );
    let missing = found.get(1).unwrap();
    assert!(missing.unresolved);
    assert_eq!(missing.scene.as_deref(), Some("s012"));
    let heading = found.get(2).unwrap();
    assert_eq!(heading.scene.as_deref(), Some("s013a"));
    assert!(
        chapter
            .text
            .get(heading.span.unwrap().start..)
            .unwrap()
            .starts_with("## 打ち明ける")
    );
    let target = found.get(3).unwrap();
    assert_eq!(
        (target.scene.as_deref(), target.unresolved),
        (Some("s012"), false)
    );
    let unknown = found.get(4).unwrap();
    assert!(unknown.unresolved);
    // 本文に無いシーンを書いた指摘も、対象のシーンに付ける（文脈パネルに出す）。
    assert_eq!(unknown.scene.as_deref(), Some("s012"));
}

/// 模擬の OpenAI 互換サーバー：頼みを1つ受け取り、決めた本文をまとめて返す。
fn serve(body: &str) -> (String, Arc<Mutex<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let received = Arc::new(Mutex::new(String::new()));
    let slot = Arc::clone(&received);
    let body = body.to_owned();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line.trim().is_empty() {
                break;
            }
            if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = v.trim().parse().unwrap();
            }
        }
        let mut request = vec![0; length];
        reader.read_exact(&mut request).unwrap();
        *slot.lock().unwrap() = String::from_utf8(request).unwrap();
        let reply = serde_json::json!({
            "choices": [{"message": {"content": body}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 900, "completion_tokens": 40}
        })
        .to_string();
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
            reply.len()
        );
    });
    (format!("http://{addr}/v1"), received)
}

fn connection(base_url: String) -> Connection {
    Connection {
        name: "local".to_owned(),
        provider: Provider::OpenaiCompatible,
        endpoint: format!("openai-compatible {base_url}"),
        base_url,
        model: "test-model".to_owned(),
        cloud: false,
        allowed: true,
        context_tokens: 8192,
    }
}

fn options() -> CallOptions {
    CallOptions {
        connect_timeout: Duration::from_secs(2),
        idle_timeout: Duration::from_secs(5),
    }
}

#[test]
fn run_review_task() {
    let project = houkago();
    let ir = analyze(&project);
    let sources =
        ai::read_sources(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago"));
    let task = project
        .ai_tasks
        .iter()
        .find(|t| t.id == "tone_check")
        .unwrap();
    let (url, received) = serve(
        "{\"line\": \"k3fg\", \"severity\": \"warning\", \"message\": \"「君には言っておく」は硬い\"}\nよく分かりませんでした",
    );
    let cancel = AtomicBool::new(false);
    let mut chunks = 0;
    let outcome = ai::run_task(
        &project,
        &ir,
        &sources,
        task,
        Some("s012"),
        &connection(url),
        None,
        None,
        &options(),
        &cancel,
        &mut |_| chunks += 1,
    )
    .unwrap();
    assert_eq!(chunks, 1);
    assert_eq!(outcome.findings.len(), 1);
    let f = outcome.findings.first().unwrap();
    assert_eq!(
        (f.line.as_deref(), f.scene.as_deref(), f.unresolved),
        (Some("k3fg"), Some("s012"), false)
    );
    assert_eq!(outcome.unread, ["よく分かりませんでした"]);
    assert_eq!(
        (
            outcome.completion.usage.input_tokens,
            outcome.completion.usage.output_tokens
        ),
        (900, 40)
    );
    // 送ったのは組み立てた文脈（本文と行ID）と、返す分の上限。
    let sent: serde_json::Value = serde_json::from_str(&received.lock().unwrap()).unwrap();
    let user = sent["messages"][1]["content"].as_str().unwrap();
    assert!(user.contains("灯「やっと来た。遅いよ」 ^k3f9"), "{user}");
    assert_eq!(sent["max_tokens"], 2048);
}

#[test]
fn run_summary_task() {
    let project = houkago();
    let ir = analyze(&project);
    let sources =
        ai::read_sources(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago"));
    let task = project
        .ai_tasks
        .iter()
        .find(|t| t.id == "scene_summary")
        .unwrap();
    let (url, _) = serve("灯が転校を打ち明けかけて、言えずに終わる。");
    let cancel = AtomicBool::new(false);
    let outcome = ai::run_task(
        &project,
        &ir,
        &sources,
        task,
        Some("s012"),
        &connection(url),
        None,
        None,
        &options(),
        &cancel,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(
        outcome.summary.as_deref(),
        Some("灯が転校を打ち明けかけて、言えずに終わる。")
    );
    assert!(outcome.findings.is_empty() && outcome.unread.is_empty());
}

/// 実行できない：提案ノード（T3-10）、生成・自律（T3-9）、シーンが要るのに無い。送る前に断る。
#[test]
fn run_refuses_before_sending() {
    let project = houkago();
    let ir = analyze(&project);
    let sources = ai::ContextSources::default();
    let conn = connection("http://127.0.0.1:9/v1".to_owned());
    let cancel = AtomicBool::new(false);
    for (id, scene) in [
        ("branch_ideas", Some("s012")),
        ("route_skeleton", None),
        ("draft_chapter", None),
    ] {
        let task = project.ai_tasks.iter().find(|t| t.id == id).unwrap();
        let error = ai::run_task(
            &project,
            &ir,
            &sources,
            task,
            scene,
            &conn,
            None,
            None,
            &options(),
            &cancel,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(matches!(error, RunError::Unsupported(_)), "{id}：{error:?}");
    }
    let task = project
        .ai_tasks
        .iter()
        .find(|t| t.id == "tone_check")
        .unwrap();
    let error = ai::run_task(
        &project,
        &ir,
        &sources,
        task,
        None,
        &conn,
        None,
        None,
        &options(),
        &cancel,
        &mut |_| {},
    )
    .unwrap_err();
    assert!(
        matches!(&error, RunError::Context(m) if m.contains("シーン")),
        "{error:?}"
    );
}

/// 前置きの文とコードブロックの中の、整形した JSON の配列も読む。
#[test]
fn parse_pretty_array_after_preamble() {
    let text = "以下が指摘です。\n```json\n[\n  {\n    \"line\": \"k3f9\",\n    \"severity\": \"warning\",\n    \"message\": \"硬い\"\n  }\n]\n```\n以上です。";
    let (found, unread) = ai::parse_findings(text);
    assert_eq!(
        found,
        [raw(Some("k3f9"), None, FindingSeverity::Warning, "硬い")]
    );
    assert_eq!(unread, ["以下が指摘です。", "以上です。"]);
    let (found, _) =
        ai::parse_findings("結果：{\"findings\": [{\"line\": \"a1\", \"message\": \"x\"}]}");
    assert_eq!(found, [raw(Some("a1"), None, FindingSeverity::Info, "x")]);
    // 指摘の文の中の `[…]` を配列と取り違えない。
    let (found, unread) = ai::parse_findings(
        "{\"line\": \"k3f9\", \"message\": \"選択肢 [1] の前\"}\n{\"line\": \"k3fa\", \"message\": \"硬い\"}",
    );
    assert_eq!(
        found,
        [
            raw(Some("k3f9"), None, FindingSeverity::Info, "選択肢 [1] の前"),
            raw(Some("k3fa"), None, FindingSeverity::Info, "硬い"),
        ]
    );
    assert!(unread.is_empty());
    let (found, _) = ai::parse_findings("{\"line\": \"k3f9\", \"message\": \"「[]」は要らない\"}");
    assert_eq!(
        found,
        [raw(
            Some("k3f9"),
            None,
            FindingSeverity::Info,
            "「[]」は要らない"
        )]
    );
}

/// クラウドの接続先には、人が確かめた中身（要約値）と同じときだけ送る。
#[test]
fn cloud_runs_need_confirmed_content() {
    let project = houkago();
    let ir = analyze(&project);
    let sources =
        ai::read_sources(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago"));
    let task = project
        .ai_tasks
        .iter()
        .find(|t| t.id == "tone_check")
        .unwrap();
    let (url, received) = serve("");
    let mut conn = connection(url);
    conn.cloud = true;
    let cancel = AtomicBool::new(false);
    for confirmed in [None, Some("0000000000000000")] {
        let error = ai::run_task(
            &project,
            &ir,
            &sources,
            task,
            Some("s012"),
            &conn,
            None,
            confirmed,
            &options(),
            &cancel,
            &mut |_| {},
        )
        .unwrap_err();
        assert_eq!(error, RunError::Unconfirmed);
    }
    assert_eq!(*received.lock().unwrap(), "", "確かめていない中身を送った");
    let assembled = ai::assemble(
        &project,
        &ir,
        &sources,
        task,
        Some("s012"),
        conn.context_tokens,
    );
    let digest = ai::content_digest(&conn, &assembled);
    // 別のシーンの中身を確かめても、送らない。
    let other = ai::assemble(
        &project,
        &ir,
        &sources,
        task,
        Some("s013a"),
        conn.context_tokens,
    );
    assert_ne!(ai::content_digest(&conn, &other), digest);
    let outcome = ai::run_task(
        &project,
        &ir,
        &sources,
        task,
        Some("s012"),
        &conn,
        None,
        Some(&digest),
        &options(),
        &cancel,
        &mut |_| {},
    )
    .unwrap();
    assert!(outcome.findings.is_empty());
    assert!(received.lock().unwrap().contains("s012"));
}

/// 提案のタスク（`output: diff`。T3-8）：応答から行IDつきの差分を読む。
#[test]
fn run_diff_task() {
    let project = houkago();
    let ir = analyze(&project);
    let sources =
        ai::read_sources(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago"));
    let task = project
        .ai_tasks
        .iter()
        .find(|t| t.id == "rephrase")
        .unwrap();
    let (url, _) = serve(
        "{\"line\": \"k3fg\", \"before\": \"灯「やっぱり、君には言っておく」\", \"after\": \"灯「やっぱり、君には言っとく」\", \"reason\": \"軽口\"}",
    );
    let cancel = AtomicBool::new(false);
    let outcome = ai::run_task(
        &project,
        &ir,
        &sources,
        task,
        Some("s012"),
        &connection(url),
        None,
        None,
        &options(),
        &cancel,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(outcome.proposals.len(), 1);
    assert_eq!(outcome.proposals.first().unwrap().line, "k3fg");
    assert!(outcome.findings.is_empty() && outcome.unread.is_empty());
}

/// 生成のタスク（スタブの本文。T3-9）：応答から書き込む本文の行を読む（書き込みはワークスペース）。
#[test]
fn run_generate_task() {
    let project = houkago();
    let ir = analyze(&project);
    let sources =
        ai::read_sources(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago"));
    let task = project
        .ai_tasks
        .iter()
        .find(|t| t.id == "draft_stub")
        .unwrap();
    let (url, received) = serve("## 沈黙のあと #s014\n灯「……」\n　風が止んだ。\n-> s020");
    let cancel = AtomicBool::new(false);
    let outcome = ai::run_task(
        &project,
        &ir,
        &sources,
        task,
        Some("s014"),
        &connection(url),
        None,
        None,
        &options(),
        &cancel,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(outcome.draft, ["灯「……」", "　風が止んだ。"]);
    // 生成のタスクの文脈には文体（文体見本）が入っている。
    assert!(received.lock().unwrap().contains("文体見本"));
}
