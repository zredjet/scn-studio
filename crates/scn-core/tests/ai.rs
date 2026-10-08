//! AIの接続（T3-5）：模擬のサーバー（標準ライブラリの `TcpListener`）で、OpenAI 互換と Messages API の呼び出し、
//! 誤り（つなげない、認証、タイムアウト、途中の誤り）、取り消し、トークン数、クラウドの規則、キーの置き場所を確かめる。
//! 実際の LLM は呼ばない。

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use scn_core::ai::{
    self, AiError, CallOptions, ChatRequest, Completion, Connection, KeyStore, MachineSettings,
    MemoryKeys, Provider,
};
use scn_core::load_project;
use scn_core::project::AiConnectionDecl;
use serde_json::Value;

/// 模擬のサーバーが受け取った頼み。
#[derive(Debug, Clone, Default)]
struct Received {
    /// 1行目（`POST /v1/chat/completions HTTP/1.1`）。
    line: String,
    /// ヘッダー（名前は小文字）。
    headers: Vec<(String, String)>,
    body: String,
}

impl Received {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap()
    }
}

/// 返し方。
enum Reply {
    /// ヘッダーと本文を、分けて書く（間に `delay` を置く）。
    Chunks(Vec<Vec<u8>>, Duration),
    /// 頼みを読んだ後、何も返さずに待つ。
    Silent(Duration),
}

struct Mock {
    addr: SocketAddr,
    received: Arc<Mutex<Option<Received>>>,
}

impl Mock {
    fn base_url(&self) -> String {
        format!("http://{}/v1", self.addr)
    }

    /// 頼みが届いていれば、その中身（少し待つ）。
    fn try_received(&self, wait: Duration) -> Option<Received> {
        let until = Instant::now() + wait;
        loop {
            if let Some(r) = self.received.lock().unwrap().clone() {
                return Some(r);
            }
            if Instant::now() > until {
                return None;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn received(&self) -> Received {
        for _ in 0..200 {
            if let Some(r) = self.received.lock().unwrap().clone() {
                return r;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("模擬のサーバーに頼みが届いていない");
    }
}

fn read_request(stream: &mut TcpStream) -> Received {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        reader.read_line(&mut h).unwrap();
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        let (name, value) = h.split_once(':').unwrap();
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
    }
    let length: usize = headers
        .iter()
        .find(|(n, _)| n == "content-length")
        .map_or(0, |(_, v)| v.parse().unwrap());
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    Received {
        line: line.trim_end().to_owned(),
        headers,
        body: String::from_utf8(body).unwrap(),
    }
}

fn serve(reply: Reply) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let received = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&received);
    thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let request = read_request(&mut stream);
        *slot.lock().unwrap() = Some(request);
        match reply {
            Reply::Chunks(chunks, delay) => {
                for (i, chunk) in chunks.iter().enumerate() {
                    if i > 0 {
                        thread::sleep(delay);
                    }
                    if stream.write_all(chunk).is_err() {
                        return;
                    }
                    let _ = stream.flush();
                }
            }
            Reply::Silent(wait) => thread::sleep(wait),
        }
    });
    Mock { addr, received }
}

fn sse_head() -> Vec<u8> {
    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n".to_vec()
}

/// SSE の応答（ヘッダーと、出来事ごとの書き込み）。
fn sse(events: &[&str], delay: Duration) -> Reply {
    let mut chunks = vec![sse_head()];
    chunks.extend(events.iter().map(|e| e.as_bytes().to_vec()));
    Reply::Chunks(chunks, delay)
}

fn json_reply(status: &str, body: &str) -> Reply {
    let text = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    Reply::Chunks(vec![text.into_bytes()], Duration::ZERO)
}

/// 模擬のサーバーにつなぐ接続先（テストでは、どの種類もローカルとして扱う）。
fn local(provider: Provider, base_url: String) -> Connection {
    Connection {
        name: "test".to_owned(),
        provider,
        endpoint: format!("{} {base_url}", provider.name()),
        base_url,
        model: "test-model".to_owned(),
        cloud: false,
        allowed: true,
        context_tokens: ai::DEFAULT_CONTEXT_TOKENS,
    }
}

fn request() -> ChatRequest {
    ai::test_request()
}

fn quick() -> CallOptions {
    CallOptions {
        connect_timeout: Duration::from_secs(2),
        idle_timeout: Duration::from_millis(400),
    }
}

fn call(
    conn: &Connection,
    key: Option<&str>,
    req: &ChatRequest,
) -> (Result<Completion, AiError>, Vec<String>) {
    let cancel = AtomicBool::new(false);
    let mut deltas = Vec::new();
    let result = ai::complete(conn, key, req, &quick(), &cancel, &mut |t| {
        deltas.push(t.to_owned());
    });
    (result, deltas)
}

#[test]
fn openai_streaming() {
    // 「は」の UTF-8 の途中で書き込みを分ける。
    let ha = "は".as_bytes();
    let mut chunks = vec![sse_head()];
    chunks.push(b"data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n".to_vec());
    let mut part = b"data: {\"choices\":[{\"delta\":{\"content\":\"".to_vec();
    part.push(ha[0]);
    chunks.push(part);
    let mut rest = ha[1..].to_vec();
    rest.extend_from_slice(b"\"}}]}\n\n");
    chunks.push(rest);
    chunks.push(
        b"data: {\"choices\":[{\"delta\":{\"content\":\"\xe3\x81\x84\"},\"finish_reason\":\"stop\"}]}\r\n\r\n"
            .to_vec(),
    );
    chunks.push(
        b": keep-alive\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":2}}\n\ndata: [DONE]\n\n"
            .to_vec(),
    );
    let mock = serve(Reply::Chunks(chunks, Duration::from_millis(20)));
    let conn = local(Provider::OpenaiCompatible, mock.base_url());
    let (result, deltas) = call(&conn, Some("sk-test"), &request());
    let done = result.unwrap();
    assert_eq!(done.text, "はい");
    assert_eq!(deltas, ["は", "い"]);
    assert_eq!(done.stop_reason.as_deref(), Some("stop"));
    assert_eq!(
        (
            done.usage.input_tokens,
            done.usage.output_tokens,
            done.usage.estimated
        ),
        (12, 2, false)
    );

    let got = mock.received();
    assert_eq!(got.line, "POST /v1/chat/completions HTTP/1.1");
    assert_eq!(got.header("authorization"), Some("Bearer sk-test"));
    let body = got.json();
    assert_eq!(body["model"], "test-model");
    assert_eq!(body["stream"], true);
    assert_eq!(body["stream_options"]["include_usage"], true);
    assert_eq!(body["max_tokens"], 32);
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][1]["role"], "user");
    assert_eq!(
        body["messages"][1]["content"],
        "「はい」とだけ答えてください。"
    );
    assert!(body.get("temperature").is_none());
}

/// キーが無ければ Authorization を送らない（ローカルのサーバー）。ストリーミングで返さないサーバーは、まとめて読む。
/// 応答に数が無ければ、文字数から見積もる。
#[test]
fn openai_whole_and_estimated() {
    let mock = serve(json_reply(
        "200 OK",
        r#"{"choices":[{"message":{"role":"assistant","content":"はい"},"finish_reason":"stop"}],"usage":{"prompt_tokens":7,"completion_tokens":1}}"#,
    ));
    let conn = local(Provider::OpenaiCompatible, mock.base_url());
    let (result, deltas) = call(&conn, None, &request());
    let done = result.unwrap();
    assert_eq!(done.text, "はい");
    assert_eq!(deltas, ["はい"]);
    assert_eq!((done.usage.input_tokens, done.usage.output_tokens), (7, 1));
    assert!(!done.usage.estimated);
    assert_eq!(mock.received().header("authorization"), None);

    let mock = serve(sse(
        &[
            "data: {\"choices\":[{\"delta\":{\"content\":\"はい\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        ],
        Duration::ZERO,
    ));
    let conn = local(Provider::OpenaiCompatible, mock.base_url());
    let done = call(&conn, None, &request()).0.unwrap();
    assert!(done.usage.estimated);
    // 「接続の確認です。」8字 ＋「「はい」とだけ答えてください。」15字、「はい」2字。
    assert_eq!((done.usage.input_tokens, done.usage.output_tokens), (23, 2));
}

#[test]
fn anthropic_streaming() {
    let mock = serve(sse(
        &[
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":25,\"output_tokens\":1}}}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: ping\ndata: {\"type\":\"ping\"}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"は\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"い\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":5}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        ],
        Duration::from_millis(5),
    ));
    let conn = local(Provider::Anthropic, mock.base_url());
    let mut req = request();
    req.max_tokens = None;
    let (result, deltas) = call(&conn, Some("ak-test"), &req);
    let done = result.unwrap();
    assert_eq!(done.text, "はい");
    assert_eq!(deltas, ["は", "い"]);
    assert_eq!(done.stop_reason.as_deref(), Some("end_turn"));
    assert_eq!((done.usage.input_tokens, done.usage.output_tokens), (25, 5));
    assert!(!done.usage.estimated);

    let got = mock.received();
    assert_eq!(got.line, "POST /v1/messages HTTP/1.1");
    assert_eq!(got.header("x-api-key"), Some("ak-test"));
    assert_eq!(got.header("anthropic-version"), Some(ai::ANTHROPIC_VERSION));
    assert_eq!(got.header("authorization"), None);
    let body = got.json();
    assert_eq!(body["system"], "接続の確認です。");
    assert_eq!(body["max_tokens"], ai::ANTHROPIC_MAX_TOKENS);
    assert_eq!(body["messages"].as_array().unwrap().len(), 1);
    assert_eq!(body["messages"][0]["role"], "user");

    // まとめての応答。
    let mock = serve(json_reply(
        "200 OK",
        r#"{"content":[{"type":"text","text":"は"},{"type":"text","text":"い"}],"stop_reason":"end_turn","usage":{"input_tokens":9,"output_tokens":2}}"#,
    ));
    let conn = local(Provider::Anthropic, mock.base_url());
    let done = call(&conn, Some("ak-test"), &request()).0.unwrap();
    assert_eq!(done.text, "はい");
    assert_eq!((done.usage.input_tokens, done.usage.output_tokens), (9, 2));
}

#[test]
fn http_errors() {
    // 認証の誤り。サーバーが文にキーを入れて返しても、知らせる文にはキーを出さない。
    let mock = serve(json_reply(
        "401 Unauthorized",
        r#"{"error":{"message":"Incorrect API key provided: sk-secret-123","type":"invalid_request_error"}}"#,
    ));
    let conn = local(Provider::OpenaiCompatible, mock.base_url());
    let error = call(&conn, Some("sk-secret-123"), &request())
        .0
        .unwrap_err();
    assert!(
        matches!(&error, AiError::Auth { status: 401, .. }),
        "{error:?}"
    );
    assert!(!error.to_string().contains("sk-secret-123"), "{error}");
    assert!(error.to_string().contains("Incorrect API key"), "{error}");

    let mock = serve(json_reply(
        "401 Unauthorized",
        r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#,
    ));
    let conn = local(Provider::Anthropic, mock.base_url());
    let error = call(&conn, Some("ak"), &request()).0.unwrap_err();
    assert_eq!(
        error,
        AiError::Auth {
            status: 401,
            message: "invalid x-api-key".to_owned()
        }
    );

    // Ollama の形（`error` が文字列）。
    let mock = serve(json_reply(
        "404 Not Found",
        r#"{"error":"model \"test-model\" not found, try pulling it first"}"#,
    ));
    let conn = local(Provider::OpenaiCompatible, mock.base_url());
    let error = call(&conn, None, &request()).0.unwrap_err();
    assert!(
        matches!(&error, AiError::Http { status: 404, message } if message.contains("not found")),
        "{error:?}"
    );

    // 途中の誤り（Messages API の error の出来事）。
    let mock = serve(sse(
        &[
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":3}}}\n\n",
            "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n",
        ],
        Duration::ZERO,
    ));
    let conn = local(Provider::Anthropic, mock.base_url());
    assert_eq!(
        call(&conn, Some("ak"), &request()).0.unwrap_err(),
        AiError::Server("Overloaded".to_owned())
    );

    // 終わりの印が無いまま切れた。
    let mock = serve(sse(
        &["data: {\"choices\":[{\"delta\":{\"content\":\"は\"}}]}\n\n"],
        Duration::ZERO,
    ));
    let conn = local(Provider::OpenaiCompatible, mock.base_url());
    assert!(matches!(
        call(&conn, None, &request()).0.unwrap_err(),
        AiError::Protocol(_)
    ));

    // JSON でない。
    let mock = serve(sse(&["data: {broken\n\n"], Duration::ZERO));
    let conn = local(Provider::OpenaiCompatible, mock.base_url());
    assert!(matches!(
        call(&conn, None, &request()).0.unwrap_err(),
        AiError::Protocol(_)
    ));
}

/// URL にパス（`/v1` など）が無いまま「その場所は無い」と返されたら、`/v1` を足すよう知らせる（LM Studio の
/// 「Reachable at」をそのまま貼ったときなど。T3-1）。パスがあれば知らせない。
#[test]
fn hint_missing_v1() {
    let lm_studio = r#"{"error":"Unexpected endpoint or method. (POST /chat/completions)"}"#;
    for status in ["200 OK", "404 Not Found"] {
        let mock = serve(json_reply(status, lm_studio));
        let root = format!("http://{}", mock.addr);
        let conn = local(Provider::OpenaiCompatible, root);
        let error = call(&conn, None, &request()).0.unwrap_err();
        assert!(error.to_string().contains("Unexpected endpoint"), "{error}");
        assert!(error.to_string().contains("/v1 まで"), "{error}");
    }
    let mock = serve(json_reply("404 Not Found", lm_studio));
    let conn = local(Provider::OpenaiCompatible, mock.base_url());
    let error = call(&conn, None, &request()).0.unwrap_err();
    assert!(!error.to_string().contains("/v1 まで"), "{error}");
}

/// 別の URL へ移るよう返されても、ついて行かない（キーと本文を移り先へ送らない）。
#[test]
fn redirects_are_not_followed() {
    let target = serve(json_reply(
        "200 OK",
        r#"{"choices":[{"message":{"content":"はい"},"finish_reason":"stop"}]}"#,
    ));
    let location = format!("{}/chat/completions", target.base_url());
    let text = format!(
        "HTTP/1.1 307 Temporary Redirect\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    let mock = serve(Reply::Chunks(vec![text.into_bytes()], Duration::ZERO));
    let conn = local(Provider::Anthropic, mock.base_url());
    let error = call(&conn, Some("ak-secret"), &request()).0.unwrap_err();
    assert!(
        matches!(&error, AiError::Http { status: 307, message } if message.contains(&location)),
        "{error:?}"
    );
    assert!(target.try_received(Duration::from_millis(300)).is_none());
}

#[test]
fn cannot_connect() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let conn = local(Provider::OpenaiCompatible, format!("http://{addr}/v1"));
    let error = call(&conn, None, &request()).0.unwrap_err();
    assert!(matches!(error, AiError::Connect(_)), "{error:?}");
}

#[test]
fn timeouts() {
    // 応答の始まりが来ない。
    let mock = serve(Reply::Silent(Duration::from_secs(3)));
    let conn = local(Provider::OpenaiCompatible, mock.base_url());
    let started = Instant::now();
    assert_eq!(
        call(&conn, None, &request()).0.unwrap_err(),
        AiError::Timeout
    );
    assert!(started.elapsed() < Duration::from_secs(2));

    // 途中で止まった（最初の出来事はヘッダーと一緒に書く）。
    let mut first = sse_head();
    first
        .extend_from_slice("data: {\"choices\":[{\"delta\":{\"content\":\"は\"}}]}\n\n".as_bytes());
    let mock = serve(Reply::Chunks(
        vec![first, b"data: [DONE]\n\n".to_vec()],
        Duration::from_secs(3),
    ));
    let conn = local(Provider::OpenaiCompatible, mock.base_url());
    let started = Instant::now();
    let (result, deltas) = call(&conn, None, &request());
    assert_eq!(result.unwrap_err(), AiError::Timeout);
    assert_eq!(deltas, ["は"]);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn cancel_between_data() {
    let mock = serve(sse(
        &[
            "data: {\"choices\":[{\"delta\":{\"content\":\"は\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"い\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
        ],
        Duration::from_millis(300),
    ));
    let conn = local(Provider::OpenaiCompatible, mock.base_url());
    let cancel = AtomicBool::new(false);
    let mut deltas = Vec::new();
    let options = CallOptions {
        connect_timeout: Duration::from_secs(2),
        idle_timeout: Duration::from_secs(5),
    };
    let result = ai::complete(&conn, None, &request(), &options, &cancel, &mut |t| {
        deltas.push(t.to_owned());
        cancel.store(true, Ordering::Relaxed);
    });
    assert_eq!(result.unwrap_err(), AiError::Cancelled);
    assert_eq!(deltas, ["は"]);

    // 始める前に取り消してあれば、送らない。
    let cancel = AtomicBool::new(true);
    let result = ai::complete(&conn, None, &request(), &options, &cancel, &mut |_| {});
    assert_eq!(result.unwrap_err(), AiError::Cancelled);
}

fn decl(
    name: &str,
    provider: Option<&str>,
    base_url: Option<&str>,
    model: Option<&str>,
) -> AiConnectionDecl {
    AiConnectionDecl {
        name: name.to_owned(),
        provider: provider.map(str::to_owned),
        base_url: base_url.map(str::to_owned),
        model: model.map(str::to_owned),
        context_tokens: None,
    }
}

#[test]
fn resolve_connections() {
    let machine = MachineSettings::default();
    let conn = ai::resolve(
        &decl(
            "local",
            Some("openai-compatible"),
            Some("http://localhost:11434/v1/"),
            Some("qwen3"),
        ),
        &machine,
    )
    .unwrap();
    assert_eq!(conn.base_url, "http://localhost:11434/v1");
    assert_eq!(conn.endpoint, "openai-compatible http://localhost:11434/v1");
    assert!(!conn.cloud && conn.allowed);

    // provider を書かなければ OpenAI 互換。
    let conn = ai::resolve(
        &decl("x", None, Some("http://127.0.0.1:1234/v1"), Some("m")),
        &machine,
    )
    .unwrap();
    assert_eq!(conn.provider, Provider::OpenaiCompatible);

    // Messages API は、URL を書かなくてよく、いつもクラウド。
    let conn = ai::resolve(
        &decl("cloud", Some("anthropic"), None, Some("claude")),
        &machine,
    )
    .unwrap();
    assert_eq!(conn.base_url, ai::ANTHROPIC_BASE_URL);
    assert!(conn.cloud && !conn.allowed);

    for (decl, needle) in [
        (
            decl("a", Some("gemini"), Some("https://x/v1"), Some("m")),
            "gemini",
        ),
        (decl("a", None, None, Some("m")), "base_url"),
        (decl("a", None, Some("http://localhost/v1"), None), "model"),
        (
            decl("a", None, Some("http://localhost/v1"), Some("  ")),
            "model",
        ),
        (decl("a", None, Some("localhost:11434"), Some("m")), "http"),
        (
            decl("a", None, Some("ftp://localhost/v1"), Some("m")),
            "http",
        ),
    ] {
        let problem = ai::resolve(&decl, &machine).unwrap_err();
        assert!(problem.contains(needle), "{problem}");
    }

    // 端末の上書き（名前ごと）と、クラウドの許可（行き先ごと）。
    let mut machine = MachineSettings::default();
    machine.set_override("local", Some(" http://localhost:1234/v1 "), Some("gemma"));
    machine.set_cloud_allowed("anthropic https://api.anthropic.com/v1", true);
    let conn = ai::resolve(
        &decl(
            "local",
            None,
            Some("http://localhost:11434/v1"),
            Some("qwen3"),
        ),
        &machine,
    )
    .unwrap();
    assert_eq!(
        (conn.base_url.as_str(), conn.model.as_str()),
        ("http://localhost:1234/v1", "gemma")
    );
    let conn = ai::resolve(
        &decl("cloud", Some("anthropic"), None, Some("claude")),
        &machine,
    )
    .unwrap();
    assert!(conn.allowed);
    machine.set_override("local", Some(""), None);
    assert!(machine.connections.is_empty());
}

/// 文脈の長さ（T3-6）：書いた値、無ければ接続の種類の既定。
#[test]
fn context_tokens() {
    let machine = MachineSettings::default();
    let mut local = decl("local", None, Some("http://localhost:11434/v1"), Some("m"));
    assert_eq!(
        ai::resolve(&local, &machine).unwrap().context_tokens,
        ai::DEFAULT_CONTEXT_TOKENS
    );
    local.context_tokens = Some(32_768);
    assert_eq!(
        ai::resolve(&local, &machine).unwrap().context_tokens,
        32_768
    );
    let cloud = decl("cloud", Some("anthropic"), None, Some("c"));
    assert_eq!(
        ai::resolve(&cloud, &machine).unwrap().context_tokens,
        ai::ANTHROPIC_CONTEXT_TOKENS
    );
    assert_eq!(ai::context_limit(8192), 6144);
    assert_eq!(ai::context_limit(200_000), 195_904);
}

#[test]
fn local_or_cloud() {
    let machine = MachineSettings::default();
    for (url, cloud) in [
        ("http://localhost:11434/v1", false),
        ("http://LOCALHOST/v1", false),
        ("http://lm.localhost/v1", false),
        ("http://gpu-box.local:8080/v1", false),
        ("http://127.0.0.1:8080/v1", false),
        ("http://[::1]:11434/v1", false),
        ("http://10.0.0.2/v1", false),
        ("http://172.16.5.1/v1", false),
        ("http://192.168.1.5:1234/v1", false),
        ("http://169.254.0.9/v1", false),
        ("http://[fd00::1]/v1", false),
        ("http://172.32.0.1/v1", true),
        ("http://8.8.8.8/v1", true),
        ("https://api.openai.com/v1", true),
        ("https://localhost.example.com/v1", true),
    ] {
        let conn = ai::resolve(&decl("a", None, Some(url), Some("m")), &machine).unwrap();
        assert_eq!(conn.cloud, cloud, "{url}");
    }
}

/// クラウドへは、許可した行き先にだけ送る。キーの要る接続でキーが無い、http でクラウドへキーを送る、は送る前に断る。
#[test]
fn cloud_rules() {
    // つなげない行き先（送れば Connect の誤りになる）。
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let mut conn = local(Provider::OpenaiCompatible, format!("http://{addr}/v1"));
    conn.cloud = true;
    conn.allowed = false;
    assert_eq!(
        call(&conn, Some("k"), &request()).0.unwrap_err(),
        AiError::CloudNotAllowed("test".to_owned())
    );
    conn.allowed = true;
    assert_eq!(
        call(&conn, Some("k"), &request()).0.unwrap_err(),
        AiError::Insecure
    );

    let conn = local(Provider::Anthropic, format!("http://{addr}/v1"));
    assert_eq!(
        call(&conn, None, &request()).0.unwrap_err(),
        AiError::NoKey("test".to_owned())
    );

    // URL の書き方を変えても、http のクラウドへはキーを送らない（読んだ URL の書き方にそろえる）。
    for written in [
        "HTTP://api.example.com/v1",
        "http:/api.example.com/v1",
        "http:api.example.com/v1",
        "http:\\\\api.example.com\\v1",
    ] {
        let mut machine = MachineSettings::default();
        machine.set_cloud_allowed("openai-compatible http://api.example.com/v1", true);
        let conn = ai::resolve(&decl("c", None, Some(written), Some("m")), &machine).unwrap();
        assert_eq!(conn.base_url, "http://api.example.com/v1", "{written}");
        assert!(conn.cloud && conn.allowed, "{written}");
        assert_eq!(
            call(&conn, Some("k"), &request()).0.unwrap_err(),
            AiError::Insecure,
            "{written}"
        );
    }
    assert_eq!(
        call(&conn, Some("  "), &request()).0.unwrap_err(),
        AiError::NoKey("test".to_owned())
    );
}

#[test]
fn defaults_and_preview() {
    let machine = MachineSettings::default();
    let decls = [
        decl("cloud", Some("anthropic"), None, Some("c")),
        decl("lan", None, Some("http://192.168.0.2/v1"), Some("m")),
    ];
    assert_eq!(ai::default_connection(&decls, &machine), Some("lan"));
    let decls = [
        decl("cloud", Some("anthropic"), None, Some("c")),
        decl("local", None, None, None),
    ];
    assert_eq!(ai::default_connection(&decls, &machine), Some("local"));
    assert_eq!(ai::default_connection(&decls[..1], &machine), Some("cloud"));
    assert_eq!(ai::default_connection(&[], &machine), None);

    let conn = ai::resolve(&decls[0], &machine).unwrap();
    let preview = ai::request_preview(&conn, &request());
    assert!(
        preview.starts_with("POST https://api.anthropic.com/v1/messages\n"),
        "{preview}"
    );
    assert!(
        preview.contains("「はい」とだけ答えてください。"),
        "{preview}"
    );
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scn-ai-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn files_under(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(files_under(&path));
        } else {
            out.push(path);
        }
    }
    out
}

/// キーは、キーの置き場所（キーチェーン）にだけ置く。画面に渡す様子にも、端末の設定にも、作品フォルダにも出ない。
#[test]
fn keys_stay_in_keystore() {
    let key = "sk-ant-secret-0123456789";
    let project = temp_dir("project");
    fs::write(
        project.join("project.toml"),
        "format_version = 0\n\n[ai.cloud]\nprovider = \"anthropic\"\nmodel = \"claude\"\n\n[ai.local]\nbase_url = \"http://localhost:11434/v1\"\nmodel = \"qwen3\"\n",
    )
    .unwrap();
    let loaded = load_project(&project).unwrap();
    let decls = &loaded.settings.ai;
    assert_eq!(decls.len(), 2);

    let keys = MemoryKeys::default();
    let mut machine = MachineSettings::default();
    let endpoint = ai::resolve(&decls[0], &machine).unwrap().endpoint;
    keys.set(&endpoint, &format!("  {key}\n")).unwrap();
    assert_eq!(keys.get(&endpoint).unwrap().as_deref(), Some(key));
    machine.set_cloud_allowed(&endpoint, true);
    machine.set_override("local", None, Some("gemma"));

    let (statuses, problem) = ai::connection_statuses(decls, &machine, &keys);
    assert_eq!(problem, None);
    let cloud = statuses.iter().find(|s| s.name == "cloud").unwrap();
    assert!(cloud.has_key && cloud.needs_key && cloud.cloud && cloud.allowed && !cloud.is_default);
    let local = statuses.iter().find(|s| s.name == "local").unwrap();
    assert!(!local.has_key && !local.needs_key && !local.cloud && local.is_default);
    assert_eq!(local.model.as_deref(), Some("gemma"));
    assert_eq!(local.project_model.as_deref(), Some("qwen3"));
    assert_eq!(cloud.url_hint, None);
    assert_eq!(local.url_hint, None);
    // 端末の上書きで、パスの無い URL（LM Studio の「Reachable at」）にすると知らせる（T3-1）。
    let mut root_url = machine.clone();
    root_url.set_override("local", Some("http://127.0.0.1:1234/"), None);
    let (statuses, _) = ai::connection_statuses(decls, &root_url, &keys);
    let local = statuses.iter().find(|s| s.name == "local").unwrap();
    assert!(local.url_hint.as_deref().unwrap().contains("/v1 まで"));
    let json = serde_json::to_string(&statuses).unwrap();
    assert!(!json.contains(key));
    assert!(!json.contains("secret"));
    assert!(
        cloud
            .test_preview
            .as_deref()
            .unwrap()
            .contains("x-api-key: （キー）")
    );

    let settings_dir = temp_dir("machine");
    let path = settings_dir.join("app").join(ai::MACHINE_SETTINGS_FILE);
    ai::write_machine_settings(&path, &machine).unwrap();
    let written = fs::read_to_string(&path).unwrap();
    assert!(!written.contains(key));
    assert!(written.contains(&endpoint));
    assert_eq!(ai::read_machine_settings(&path), (machine.clone(), None));

    for file in files_under(&project) {
        let text = fs::read(&file).unwrap();
        assert!(
            !String::from_utf8_lossy(&text).contains(key),
            "{}",
            file.display()
        );
    }

    keys.delete(&endpoint).unwrap();
    keys.delete(&endpoint).unwrap();
    assert_eq!(keys.get(&endpoint).unwrap(), None);
    assert!(keys.set(&endpoint, "  ").is_err());
    assert!(keys.set(&endpoint, "a\nb").is_err());

    // 読めない端末の設定は、既定で読んで知らせる。
    fs::write(&path, "cloud = 3\n").unwrap();
    let (read, problem) = ai::read_machine_settings(&path);
    assert_eq!(read, MachineSettings::default());
    assert!(problem.is_some());
    assert_eq!(
        ai::read_machine_settings(&settings_dir.join("none.toml")),
        (MachineSettings::default(), None)
    );
    let _ = fs::remove_dir_all(&project);
    let _ = fs::remove_dir_all(&settings_dir);
}

#[test]
fn sample_connections() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago");
    let project = load_project(&root).unwrap();
    let names: Vec<_> = project
        .settings
        .ai
        .iter()
        .map(|d| (d.name.as_str(), d.provider.as_deref()))
        .collect();
    assert_eq!(
        names,
        [
            ("cloud", Some("anthropic")),
            ("local", Some("openai-compatible"))
        ]
    );
    let machine = MachineSettings::default();
    assert_eq!(
        ai::default_connection(&project.settings.ai, &machine),
        Some("local")
    );
}
