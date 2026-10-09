//! LLM の呼び出し（T3-5、D066）：OpenAI 互換のチャットの補完と、Anthropic の Messages API。
//!
//! - どちらもストリーミング（SSE）で頼み、届いた文字を順に渡す。サーバーがストリーミングで返さなければ
//!   （`text/event-stream` でなければ）、まとめての応答として読む。
//! - タイムアウトは、つなぐまでと、次のデータが届くまで（全体の時間は区切らない）。
//! - 取り消しは、データの合間に確かめる。
//! - トークン数は応答の `usage`。無ければ、送った文字と受け取った文字から見積もる（`estimated`）。

use std::io::{BufRead, BufReader, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};

use super::connection::{Connection, Provider, Via};

/// Messages API で `max_tokens` を書かないときの上限（Messages API では必須）。
pub const ANTHROPIC_MAX_TOKENS: u32 = 4096;
/// Messages API の版（`anthropic-version`）。
pub const ANTHROPIC_VERSION: &str = "2023-06-01";
/// 受け取る量の上限（壊れたサーバーが送り続けても止める）。
const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
/// 誤りの応答から読む量の上限。
const MAX_ERROR_BYTES: u64 = 64 * 1024;
/// モデルの一覧の応答から読む量の上限。
const MAX_LIST_BYTES: u64 = 4 * 1024 * 1024;

/// 発言の役。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

/// 頼む中身。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChatRequest {
    /// 指示（OpenAI 互換では最初の `system` の発言、Messages API では `system`）。
    pub system: Option<String>,
    pub messages: Vec<ChatMessage>,
    /// 返す長さの上限。Messages API では無ければ [`ANTHROPIC_MAX_TOKENS`]。
    pub max_tokens: Option<u32>,
    pub temperature: Option<f64>,
    /// 返す形（構造化出力。接続先で有効にしたときだけ送る。T4-2）。
    pub schema: Option<OutputSchema>,
}

/// 返す形の JSON Schema（OpenAI 互換の `response_format` の `json_schema`。T4-2）。
#[derive(Debug, Clone, PartialEq)]
pub struct OutputSchema {
    pub name: String,
    pub schema: Value,
}

/// 届いた文字（進み具合に使う）。考えた中身（`<think>` の中、`reasoning_content`）は本文に入れない（T4-2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Received<'a> {
    Text(&'a str),
    Thinking(&'a str),
}

impl<'a> Received<'a> {
    pub fn text(&self) -> &'a str {
        match self {
            Self::Text(t) | Self::Thinking(t) => t,
        }
    }

    pub fn thinking(&self) -> bool {
        matches!(self, Self::Thinking(_))
    }
}

/// タイムアウト。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallOptions {
    /// つなぐまで。
    pub connect_timeout: Duration,
    /// 次のデータが届くまで（応答の始まりを待つ間も含む。ローカルのモデルの読み込みを待てる長さ）。
    pub idle_timeout: Duration,
}

impl Default for CallOptions {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            idle_timeout: Duration::from_secs(180),
        }
    }
}

/// トークン数。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// 応答に数が無く、文字数から見積もった。
    pub estimated: bool,
}

/// 応答。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Completion {
    pub text: String,
    pub usage: Usage,
    /// 終わった理由（`stop`、`length`、`end_turn`、`max_tokens` など。サーバーの書いたまま）。
    pub stop_reason: Option<String>,
    /// 考えた中身の字数（本文から除いたもの。T4-2）。
    pub thinking_chars: u64,
}

/// 呼び出しの誤り。文にキーを含めない。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AiError {
    #[error(
        "接続先「{0}」はクラウドです。環境設定で、この端末からクラウドへ送ることを許可してから使います"
    )]
    CloudNotAllowed(String),
    #[error("接続先「{0}」のキーがありません。環境設定でキーを保存します")]
    NoKey(String),
    #[error("暗号化しない http の URL では、キーをクラウドへ送りません（https にします）")]
    Insecure,
    #[error("接続先につなげません：{0}")]
    Connect(String),
    #[error("応答がありません（タイムアウト）")]
    Timeout,
    #[error("取り消しました")]
    Cancelled,
    #[error("認証に失敗しました（{status}）：{message}")]
    Auth { status: u16, message: String },
    #[error("接続先が誤りを返しました（{status}）：{message}")]
    Http { status: u16, message: String },
    #[error("接続先が途中で誤りを返しました：{0}")]
    Server(String),
    #[error("応答を読めません：{0}")]
    Protocol(String),
    /// 考えるだけで返す分の上限に達し、答えが無い（T4-2）。`max_tokens` は送った返す分の上限（T4-10）。
    #[error("{}", thinking_only_message(*.chars, *.max_tokens))]
    ThinkingOnly { chars: u64, max_tokens: Option<u32> },
}

fn thinking_only_message(chars: u64, max_tokens: Option<u32>) -> String {
    let limit = max_tokens.map_or_else(String::new, |n| format!("（{n} トークン）"));
    format!(
        "考えるだけで返す分の上限{limit}に達し、答えがありません（考えた分 {chars} 字）。環境設定で接続先の「返す分の上限」を増やすか、考えない設定のモデルを使います"
    )
}

/// 考えた中身の開きと閉じのタグ（T4-2）。
const THINK_TAGS: [(&str, &str); 2] = [("<think>", "</think>"), ("<thinking>", "</thinking>")];

/// 本文から考えた中身を除く（T4-2）：初めの `<think>…</think>`（閉じが無ければ終わりまで）か、開きが無く閉じだけが
/// あるときはその前まで。除いた字数（前後の空白を除いて数える。考えを止めたモデルの空の `<think></think>` は0）も
/// 返す。
pub fn strip_thinking(text: &str) -> (String, u64) {
    let count = |t: &str| chars_of(t.trim());
    let lead = text.trim_start();
    for (open, close) in THINK_TAGS {
        if let Some(rest) = lead.strip_prefix(open) {
            return match rest.split_once(close) {
                Some((thought, answer)) => (answer.trim_start().to_owned(), count(thought)),
                None => (String::new(), count(rest)),
            };
        }
    }
    for (open, close) in THINK_TAGS {
        if let Some((thought, answer)) = text.split_once(close)
            && !thought.contains(open)
        {
            return (answer.trim_start().to_owned(), count(thought));
        }
    }
    (text.to_owned(), 0)
}

/// 答えがあるか（T4-2）：考えた中身を除くと空で、長さの上限で終わったら、答えの無い誤り。タスクの実行で使う
/// （「接続を試す」では誤りにしない）。`max_tokens` は送った返す分の上限（誤りの文に添える。T4-10）。
pub fn ensure_answer(completion: &Completion, max_tokens: Option<u32>) -> Result<(), AiError> {
    let ended_by_length = matches!(
        completion.stop_reason.as_deref(),
        Some("length" | "max_tokens")
    );
    if completion.text.trim().is_empty() && completion.thinking_chars > 0 && ended_by_length {
        return Err(AiError::ThinkingOnly {
            chars: completion.thinking_chars,
            max_tokens,
        });
    }
    Ok(())
}

/// ストリームの途中で、考えている間かを見分ける（進み具合の見せ方だけ。本文は終わってから [`strip_thinking`]）。
#[derive(Debug, Default)]
struct ThinkTracker {
    state: ThinkState,
    /// 見分けるまでの文字（初めの空白とタグ）か、考えている間の後ろの数文字（閉じのタグが分かれても見つける）。
    buffer: String,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum ThinkState {
    #[default]
    Undecided,
    Thinking,
    Answer,
}

impl ThinkTracker {
    /// この差分は考えている間のものか。
    fn thinking(&mut self, delta: &str) -> bool {
        match self.state {
            ThinkState::Answer => false,
            ThinkState::Undecided => {
                self.buffer.push_str(delta);
                let lead = self.buffer.trim_start();
                if lead.is_empty() {
                    return false;
                }
                if let Some(rest) = THINK_TAGS
                    .iter()
                    .find_map(|(open, _)| lead.strip_prefix(open))
                {
                    let rest = rest.to_owned();
                    self.state = ThinkState::Thinking;
                    self.buffer.clear();
                    self.after_thought(&rest);
                    return true;
                }
                if THINK_TAGS.iter().any(|(open, _)| open.starts_with(lead)) {
                    return true;
                }
                self.state = ThinkState::Answer;
                self.buffer.clear();
                false
            }
            ThinkState::Thinking => {
                self.after_thought(delta);
                true
            }
        }
    }

    /// 考えている間の文字を足し、閉じのタグがあれば答えへ移る。
    fn after_thought(&mut self, delta: &str) {
        self.buffer.push_str(delta);
        if THINK_TAGS
            .iter()
            .any(|(_, close)| self.buffer.contains(close))
        {
            self.state = ThinkState::Answer;
            self.buffer.clear();
            return;
        }
        // 閉じのタグ（12バイトまで）が分かれて届いても見つけられるだけ、後ろを残す。
        let keep = self
            .buffer
            .char_indices()
            .rev()
            .map(|(i, _)| i)
            .find(|&i| self.buffer.len() - i >= 12)
            .unwrap_or(0);
        self.buffer.drain(..keep);
    }
}

/// URL にパス（`/v1` など）が無いときの知らせ（T3-1）。LM Studio の「Reachable at」のようなサーバーの根を貼ると、
/// 頼む先が `/chat/completions` になり、「その場所は無い」と返される。環境設定の画面と、そう返されたときの誤りの
/// 文に添える。
pub fn missing_path_hint(conn: &Connection) -> Option<&'static str> {
    let url = reqwest::Url::parse(&conn.base_url).ok()?;
    (url.path().is_empty() || url.path() == "/").then_some(
        "URL にパスがありません。多くの接続先は /v1 まで書きます（例：http://127.0.0.1:1234/v1）",
    )
}

/// 頼む先の URL。
pub fn request_url(conn: &Connection) -> String {
    match conn.provider {
        Provider::OpenaiCompatible => format!("{}/chat/completions", conn.base_url),
        Provider::Anthropic => format!("{}/messages", conn.base_url),
    }
}

/// 送る本文（JSON）。送る前に画面で見せるのにも使う。構造化出力は OpenAI 互換で、接続先で有効にし、頼みに返す形
/// があるときだけ付ける（T4-2）。
pub fn request_body(conn: &Connection, request: &ChatRequest) -> Value {
    match conn.provider {
        Provider::OpenaiCompatible => {
            let mut messages = Vec::new();
            if let Some(system) = &request.system {
                messages.push(json!({"role": "system", "content": system}));
            }
            for m in &request.messages {
                messages.push(json!({"role": m.role, "content": m.content}));
            }
            let mut body = json!({
                "model": conn.model,
                "messages": messages,
                "stream": true,
                "stream_options": {"include_usage": true},
            });
            if let Some(object) = body.as_object_mut() {
                if let Some(max) = request.max_tokens {
                    object.insert("max_tokens".to_owned(), json!(max));
                }
                if let Some(t) = request.temperature {
                    object.insert("temperature".to_owned(), json!(t));
                }
                // 考える量（T4-28）。
                if let Some(effort) = conn.effort {
                    object.insert("reasoning_effort".to_owned(), json!(effort.name()));
                }
                if conn.structured_output
                    && let Some(schema) = &request.schema
                {
                    object.insert(
                        "response_format".to_owned(),
                        json!({
                            "type": "json_schema",
                            "json_schema": {"name": schema.name, "schema": schema.schema},
                        }),
                    );
                }
            }
            body
        }
        Provider::Anthropic => {
            let mut body = json!({
                "model": conn.model,
                "max_tokens": request.max_tokens.unwrap_or(ANTHROPIC_MAX_TOKENS),
                "messages": request.messages,
                "stream": true,
            });
            if let Some(object) = body.as_object_mut() {
                if let Some(system) = &request.system {
                    object.insert("system".to_owned(), json!(system));
                }
                if let Some(t) = request.temperature {
                    object.insert("temperature".to_owned(), json!(t));
                }
                // 考える量（T4-28。`output_config.effort`。beta のヘッダーは要らない）。考える機能も入れる
                // （4.6〜4.8 の系は、送らなければ考えない。5 の系は送らなくても考えるので同じ）。
                if let Some(effort) = conn.effort {
                    object.insert("output_config".to_owned(), json!({"effort": effort.name()}));
                    object.insert("thinking".to_owned(), json!({"type": "adaptive"}));
                }
            }
            body
        }
    }
}

/// 送る中身を人が読める形で（URL、キー以外のヘッダー、本文）。
pub fn request_preview(conn: &Connection, request: &ChatRequest) -> String {
    if conn.via == Via::ClaudeCode {
        return super::claude_code::preview(conn, request);
    }
    let body = serde_json::to_string_pretty(&request_body(conn, request)).unwrap_or_default();
    let auth = match conn.provider {
        Provider::OpenaiCompatible => "Authorization: Bearer （キー。保存してあれば）",
        Provider::Anthropic => "x-api-key: （キー）",
    };
    format!("POST {}\n{auth}\n\n{body}", request_url(conn))
}

/// 「接続を試す」の返す長さの上限（人の確認 H33 の報告で 32 から上げた。32 では考えるモデルが答えの前に使い切った）。
pub const TEST_MAX_TOKENS: u32 = 1024;

/// 環境設定の「接続を試す」で送る中身（作品の中身は送らない）。
pub fn test_request() -> ChatRequest {
    ChatRequest {
        system: Some("接続の確認です。".to_owned()),
        messages: vec![ChatMessage {
            role: Role::User,
            content: "「はい」とだけ答えてください。".to_owned(),
        }],
        // 考えるモデルが考え終えて答えまで届くだけの上限（答えは「はい」なので、費用はほとんど変わらない）。
        max_tokens: Some(TEST_MAX_TOKENS),
        temperature: None,
        schema: None,
    }
}

/// 文字数からトークン数を見積もる（英数字は4文字で1、それ以外は1文字で1。多めに見積もる）。
pub fn estimate_tokens(text: &str) -> u64 {
    let mut ascii = 0u64;
    let mut other = 0u64;
    for c in text.chars() {
        if c.is_ascii() {
            ascii += 1;
        } else {
            other += 1;
        }
    }
    other + ascii.div_ceil(4)
}

/// 考える量を送ったときに断られたら添える知らせ（T4-28）。送っていなければ空。
fn effort_hint(conn: &Connection) -> String {
    match (conn.effort, conn.provider) {
        (None, _) => String::new(),
        (Some(_), Provider::OpenaiCompatible) => "。接続先かモデルが考える量（reasoning_effort）に対応していないかもしれません。環境設定で、この接続先の「考える量」を既定にします".to_owned(),
        (Some(_), Provider::Anthropic) => "。モデルが考える量（effort）に対応していないかもしれません。環境設定で、この接続先の「考える量」を既定にします".to_owned(),
    }
}

/// 呼び出す。届いた文字は `on_text` に順に渡す。`cancel` が真になったら、次のデータの合間で止める。
pub fn complete(
    conn: &Connection,
    key: Option<&str>,
    request: &ChatRequest,
    options: &CallOptions,
    cancel: &AtomicBool,
    on_text: &mut dyn FnMut(Received<'_>),
) -> Result<Completion, AiError> {
    let key = key.map(str::trim).filter(|k| !k.is_empty());
    let redact = |text: String| match key {
        Some(key) => text.replace(key, "（キー）"),
        None => text,
    };
    let result = call(conn, key, request, options, cancel, on_text);
    let hint = |text: String| match missing_path_hint(conn) {
        Some(hint) => format!("{text}。{hint}"),
        None => text,
    };
    // 構造化出力を付けて送り、断られたとき（T4-2）。
    let structured = conn.provider == Provider::OpenaiCompatible
        && conn.structured_output
        && request.schema.is_some();
    result.map_err(|e| match e {
        AiError::Connect(m) => AiError::Connect(redact(m)),
        AiError::Auth { status, message } => AiError::Auth {
            status,
            message: redact(message),
        },
        AiError::Http { status, message } if status == 404 || status == 405 => AiError::Http {
            status,
            message: hint(redact(message)),
        },
        AiError::Http { status, message } if structured && (status == 400 || status == 422) => {
            AiError::Http {
                status,
                message: format!(
                    "{}。接続先が構造化出力（response_format）に対応していないかもしれません。環境設定で、この接続先の「構造化出力」を切ります{}",
                    redact(message),
                    effort_hint(conn)
                ),
            }
        }
        AiError::Http { status, message } if conn.effort.is_some() && (status == 400 || status == 422) => {
            AiError::Http {
                status,
                message: format!("{}{}", redact(message), effort_hint(conn)),
            }
        }
        AiError::Http { status, message } => AiError::Http {
            status,
            message: redact(message),
        },
        AiError::Server(m) => AiError::Server(hint(redact(m))),
        AiError::Protocol(m) => AiError::Protocol(redact(m)),
        other => other,
    })
}

/// 接続先に入っているモデルの一覧（OpenAI 互換の `<base_url>/models`。Ollama・LM Studio・llama.cpp のサーバー
/// など。T4-23、N4）。ローカルの OpenAI 互換の接続先だけ（クラウドへは、送ることを確かめていない頼みを送らない）。
/// 名前の順に、重ねずに返す。別の URL へ移るよう返されても、ついて行かない。
pub fn list_models(
    conn: &Connection,
    key: Option<&str>,
    options: &CallOptions,
) -> Result<Vec<String>, AiError> {
    if conn.cloud || conn.provider != Provider::OpenaiCompatible {
        return Err(AiError::Protocol(
            "モデルの一覧は、ローカルの OpenAI 互換の接続先だけで読めます".to_owned(),
        ));
    }
    let key = key.map(str::trim).filter(|k| !k.is_empty());
    let redact = |text: String| match key {
        Some(key) => text.replace(key, "（キー）"),
        None => text,
    };
    fetch_models(conn, key, options).map_err(|e| match e {
        AiError::Connect(m) => AiError::Connect(redact(m)),
        AiError::Auth { status, message } => AiError::Auth {
            status,
            message: redact(message),
        },
        AiError::Http { status, message } if status == 404 || status == 405 => {
            let message = redact(message);
            AiError::Http {
                status,
                message: match missing_path_hint(conn) {
                    Some(hint) => format!("{message}。{hint}"),
                    None => message,
                },
            }
        }
        AiError::Http { status, message } => AiError::Http {
            status,
            message: redact(message),
        },
        AiError::Protocol(m) => AiError::Protocol(redact(m)),
        other => other,
    })
}

fn fetch_models(
    conn: &Connection,
    key: Option<&str>,
    options: &CallOptions,
) -> Result<Vec<String>, AiError> {
    let url = reqwest::Url::parse(&format!("{}/models", conn.base_url))
        .map_err(|e| AiError::Connect(format!("URL を読めません：{e}")))?;
    // ローカルの行き先は、OS のプロキシを通さない。
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(options.connect_timeout)
        .timeout(options.idle_timeout)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .map_err(|e| AiError::Connect(error_text(&e)))?;
    let mut http = client.get(url).header("accept", "application/json");
    if let Some(key) = key {
        http = http.bearer_auth(key);
    }
    let response = http.send().map_err(|e| {
        if e.is_timeout() {
            AiError::Timeout
        } else {
            AiError::Connect(error_text(&e))
        }
    })?;
    let status = response.status();
    if status.is_redirection() {
        return Err(AiError::Http {
            status: status.as_u16(),
            message: "別の URL へ移るよう返されました。移り先へは頼みません。base_url を確かめます"
                .to_owned(),
        });
    }
    if !status.is_success() {
        let mut text = String::new();
        let _ = response.take(MAX_ERROR_BYTES).read_to_string(&mut text);
        let message = error_message(&text);
        let code = status.as_u16();
        return Err(if code == 401 || code == 403 {
            AiError::Auth {
                status: code,
                message,
            }
        } else {
            AiError::Http {
                status: code,
                message,
            }
        });
    }
    let mut text = String::new();
    response
        .take(MAX_LIST_BYTES)
        .read_to_string(&mut text)
        .map_err(|e| io_error(&e))?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|e| AiError::Protocol(format!("JSON ではありません（{e}）")))?;
    let mut names: Vec<String> = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| AiError::Protocol("モデルの一覧（data）がありません".to_owned()))?
        .iter()
        .filter_map(|m| m.get("id").and_then(Value::as_str))
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

fn call(
    conn: &Connection,
    key: Option<&str>,
    request: &ChatRequest,
    options: &CallOptions,
    cancel: &AtomicBool,
    on_text: &mut dyn FnMut(Received<'_>),
) -> Result<Completion, AiError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(AiError::Cancelled);
    }
    if conn.cloud && !conn.allowed {
        return Err(AiError::CloudNotAllowed(conn.name.clone()));
    }
    // この端末では Claude Code の CLI で呼ぶ（キーは使わない。T4-26）。
    if conn.via == Via::ClaudeCode {
        return super::claude_code::complete(conn, request, options, cancel, on_text);
    }
    if conn.provider == Provider::Anthropic && key.is_none() {
        return Err(AiError::NoKey(conn.name.clone()));
    }
    let url = reqwest::Url::parse(&request_url(conn))
        .map_err(|e| AiError::Connect(format!("URL を読めません：{e}")))?;
    if key.is_some() && conn.cloud && url.scheme() != "https" {
        return Err(AiError::Insecure);
    }
    // 別の URL へ移るよう返されても、ついて行かない（許可していない行き先や http へ、キーと本文を送らないため）。
    let mut builder = reqwest::blocking::Client::builder()
        .connect_timeout(options.connect_timeout)
        .timeout(options.idle_timeout)
        .redirect(reqwest::redirect::Policy::none());
    if !conn.cloud {
        // ローカルの行き先は、OS のプロキシを通さない。
        builder = builder.no_proxy();
    }
    let client = builder
        .build()
        .map_err(|e| AiError::Connect(error_text(&e)))?;
    let body = serde_json::to_vec(&request_body(conn, request))
        .map_err(|e| AiError::Protocol(e.to_string()))?;
    let mut http = client
        .post(url)
        .header("content-type", "application/json")
        .header("accept", "text/event-stream, application/json")
        .body(body);
    match conn.provider {
        Provider::OpenaiCompatible => {
            if let Some(key) = key {
                http = http.bearer_auth(key);
            }
        }
        Provider::Anthropic => {
            http = http
                .header("x-api-key", key.unwrap_or_default())
                .header("anthropic-version", ANTHROPIC_VERSION);
        }
    }
    let response = http.send().map_err(|e| {
        if e.is_timeout() {
            AiError::Timeout
        } else {
            AiError::Connect(error_text(&e))
        }
    })?;
    let status = response.status();
    if status.is_redirection() {
        let to = response
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("（なし）")
            .to_owned();
        return Err(AiError::Http {
            status: status.as_u16(),
            message: format!(
                "別の URL（{to}）へ移るよう返されました。移り先へは送りません。base_url を確かめます"
            ),
        });
    }
    if !status.is_success() {
        let mut text = String::new();
        let _ = response.take(MAX_ERROR_BYTES).read_to_string(&mut text);
        let message = error_message(&text);
        let code = status.as_u16();
        return Err(if code == 401 || code == 403 {
            AiError::Auth {
                status: code,
                message,
            }
        } else {
            AiError::Http {
                status: code,
                message,
            }
        });
    }
    let streaming = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.to_ascii_lowercase().starts_with("text/event-stream"));
    let mut completion = if streaming {
        let mut reader = SseReader::new(BufReader::new(response));
        match conn.provider {
            Provider::OpenaiCompatible => openai_stream(&mut reader, cancel, on_text)?,
            Provider::Anthropic => anthropic_stream(&mut reader, cancel, on_text)?,
        }
    } else {
        let mut text = String::new();
        response
            .take(MAX_RESPONSE_BYTES as u64)
            .read_to_string(&mut text)
            .map_err(|e| io_error(&e))?;
        let value: Value = serde_json::from_str(&text)
            .map_err(|e| AiError::Protocol(format!("JSON ではありません（{e}）")))?;
        match conn.provider {
            Provider::OpenaiCompatible => openai_whole(&value)?,
            Provider::Anthropic => anthropic_whole(&value)?,
        }
    };
    if completion.usage == Usage::default() {
        let mut sent = request.system.clone().unwrap_or_default();
        for m in &request.messages {
            sent.push_str(&m.content);
        }
        completion.usage = Usage {
            input_tokens: estimate_tokens(&sent),
            output_tokens: estimate_tokens(&completion.text),
            estimated: true,
        };
    }
    // 考えた中身を除く（T4-2）。
    let (text, thought) = strip_thinking(&completion.text);
    completion.text = text;
    completion.thinking_chars = completion.thinking_chars.saturating_add(thought);
    if !streaming && !completion.text.is_empty() {
        on_text(Received::Text(&completion.text));
    }
    Ok(completion)
}

/// reqwest の誤りの文（原因をたどって、いちばん詳しいものを添える）。
fn error_text(e: &reqwest::Error) -> String {
    let mut text = e.to_string();
    let mut source = std::error::Error::source(e);
    while let Some(inner) = source {
        text = format!("{text}（{inner}）");
        source = inner.source();
    }
    text
}

fn io_error(e: &std::io::Error) -> AiError {
    let timed_out = e.kind() == std::io::ErrorKind::TimedOut
        || e.get_ref()
            .and_then(|inner| inner.downcast_ref::<reqwest::Error>())
            .is_some_and(reqwest::Error::is_timeout);
    if timed_out {
        AiError::Timeout
    } else {
        AiError::Protocol(e.to_string())
    }
}

/// 誤りの応答の本文から、知らせる文を取る（`error.message`、`error` の文字列、`message`、なければ本文の先頭）。
fn error_message(body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        let error = value.get("error");
        let found = error
            .and_then(|e| e.get("message"))
            .and_then(Value::as_str)
            .or_else(|| error.and_then(Value::as_str))
            .or_else(|| value.get("message").and_then(Value::as_str));
        if let Some(message) = found {
            return message.to_owned();
        }
    }
    let body = body.trim();
    if body.is_empty() {
        return "（本文なし）".to_owned();
    }
    body.chars().take(300).collect()
}

/// SSE の1つの出来事。
#[derive(Debug, Default)]
struct SseEvent {
    event: Option<String>,
    data: String,
}

/// SSE を行ごとに読む。
struct SseReader<R> {
    reader: R,
    read: usize,
}

impl<R: BufRead> SseReader<R> {
    fn new(reader: R) -> Self {
        Self { reader, read: 0 }
    }

    /// 次の出来事。終わりなら `None`。
    fn next(&mut self, cancel: &AtomicBool) -> Result<Option<SseEvent>, AiError> {
        let mut event = SseEvent::default();
        let mut has_data = false;
        let mut line = Vec::new();
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(AiError::Cancelled);
            }
            line.clear();
            let n = self
                .reader
                .read_until(b'\n', &mut line)
                .map_err(|e| io_error(&e))?;
            self.read = self.read.saturating_add(n);
            if self.read > MAX_RESPONSE_BYTES {
                return Err(AiError::Protocol("応答が大きすぎます".to_owned()));
            }
            if n == 0 {
                return Ok((has_data || event.event.is_some()).then_some(event));
            }
            let text = std::str::from_utf8(&line)
                .map_err(|_| AiError::Protocol("UTF-8 ではありません".to_owned()))?;
            let text = text.trim_end_matches(['\n', '\r']);
            if text.is_empty() {
                if has_data || event.event.is_some() {
                    return Ok(Some(event));
                }
                continue;
            }
            if text.starts_with(':') {
                continue;
            }
            let (field, value) = text.split_once(':').unwrap_or((text, ""));
            let value = value.strip_prefix(' ').unwrap_or(value);
            match field {
                "event" => event.event = Some(value.to_owned()),
                "data" => {
                    if has_data {
                        event.data.push('\n');
                    }
                    event.data.push_str(value);
                    has_data = true;
                }
                _ => {}
            }
        }
    }
}

/// OpenAI 互換の考えた中身（`reasoning_content`、無ければ `reasoning`）。
fn reasoning_of(value: Option<&Value>) -> Option<&str> {
    ["reasoning_content", "reasoning"]
        .iter()
        .find_map(|key| value.and_then(|v| v.get(*key)).and_then(Value::as_str))
}

fn chars_of(text: &str) -> u64 {
    u64::try_from(text.chars().count()).unwrap_or(u64::MAX)
}

fn json_data(data: &str) -> Result<Value, AiError> {
    serde_json::from_str(data).map_err(|e| AiError::Protocol(format!("JSON ではありません（{e}）")))
}

fn u64_at(value: &Value, path: &[&str]) -> Option<u64> {
    let mut v = value;
    for key in path {
        v = v.get(key)?;
    }
    v.as_u64()
}

fn openai_usage(value: &Value) -> Option<Usage> {
    let usage = value.get("usage").filter(|u| u.is_object())?;
    Some(Usage {
        input_tokens: u64_at(usage, &["prompt_tokens"]).unwrap_or(0),
        output_tokens: u64_at(usage, &["completion_tokens"]).unwrap_or(0),
        estimated: false,
    })
}

fn openai_error(value: &Value) -> Option<String> {
    let error = value.get("error")?;
    Some(
        error
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| error.as_str())
            .unwrap_or("（内容なし）")
            .to_owned(),
    )
}

fn openai_stream<R: BufRead>(
    reader: &mut SseReader<R>,
    cancel: &AtomicBool,
    on_text: &mut dyn FnMut(Received<'_>),
) -> Result<Completion, AiError> {
    let mut out = Completion::default();
    let mut done = false;
    let mut tracker = ThinkTracker::default();
    while let Some(event) = reader.next(cancel)? {
        let data = event.data.trim();
        if data == "[DONE]" {
            done = true;
            break;
        }
        if data.is_empty() {
            continue;
        }
        let value = json_data(data)?;
        if let Some(message) = openai_error(&value) {
            return Err(AiError::Server(message));
        }
        if let Some(usage) = openai_usage(&value) {
            out.usage = usage;
        }
        if let Some(choice) = value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|c| c.first())
        {
            let delta = choice.get("delta");
            // 考えた中身（`reasoning_content`、無ければ `reasoning`）は本文に入れない（T4-2。両方に同じ中身を
            // 入れるサーバーで二重に数えない）。
            if let Some(thought) = reasoning_of(delta)
                && !thought.is_empty()
            {
                out.thinking_chars = out.thinking_chars.saturating_add(chars_of(thought));
                on_text(Received::Thinking(thought));
            }
            if let Some(text) = delta.and_then(|d| d.get("content")).and_then(Value::as_str)
                && !text.is_empty()
            {
                out.text.push_str(text);
                on_text(if tracker.thinking(text) {
                    Received::Thinking(text)
                } else {
                    Received::Text(text)
                });
            }
            if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                out.stop_reason = Some(reason.to_owned());
            }
        }
    }
    if !done && out.stop_reason.is_none() {
        return Err(AiError::Protocol("応答が途中で切れました".to_owned()));
    }
    Ok(out)
}

fn openai_whole(value: &Value) -> Result<Completion, AiError> {
    if let Some(message) = openai_error(value) {
        return Err(AiError::Server(message));
    }
    let choice = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .ok_or_else(|| AiError::Protocol("choices がありません".to_owned()))?;
    let message = choice.get("message");
    let thinking_chars = reasoning_of(message).map_or(0, chars_of);
    Ok(Completion {
        text: message
            .and_then(|m| m.get("content"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        usage: openai_usage(value).unwrap_or_default(),
        stop_reason: choice
            .get("finish_reason")
            .and_then(Value::as_str)
            .map(str::to_owned),
        thinking_chars,
    })
}

fn anthropic_error(value: &Value) -> Option<String> {
    if value.get("type").and_then(Value::as_str) != Some("error") {
        return None;
    }
    Some(
        value
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("（内容なし）")
            .to_owned(),
    )
}

fn anthropic_stream<R: BufRead>(
    reader: &mut SseReader<R>,
    cancel: &AtomicBool,
    on_text: &mut dyn FnMut(Received<'_>),
) -> Result<Completion, AiError> {
    let mut out = Completion::default();
    let mut done = false;
    while let Some(event) = reader.next(cancel)? {
        let data = event.data.trim();
        if data.is_empty() {
            continue;
        }
        let value = json_data(data)?;
        if let Some(message) = anthropic_error(&value) {
            return Err(AiError::Server(message));
        }
        match value.get("type").and_then(Value::as_str) {
            Some("message_start") => {
                if let Some(usage) = value.get("message").and_then(|m| m.get("usage")) {
                    out.usage.input_tokens = u64_at(usage, &["input_tokens"]).unwrap_or(0);
                    out.usage.output_tokens = u64_at(usage, &["output_tokens"]).unwrap_or(0);
                }
            }
            Some("content_block_delta") => {
                let delta = value.get("delta");
                match delta.and_then(|d| d.get("type")).and_then(Value::as_str) {
                    Some("text_delta") => {
                        if let Some(text) =
                            delta.and_then(|d| d.get("text")).and_then(Value::as_str)
                            && !text.is_empty()
                        {
                            out.text.push_str(text);
                            on_text(Received::Text(text));
                        }
                    }
                    // 考えた中身は本文に入れない（T4-2）。
                    Some("thinking_delta") => {
                        if let Some(thought) = delta
                            .and_then(|d| d.get("thinking"))
                            .and_then(Value::as_str)
                            && !thought.is_empty()
                        {
                            out.thinking_chars =
                                out.thinking_chars.saturating_add(chars_of(thought));
                            on_text(Received::Thinking(thought));
                        }
                    }
                    _ => {}
                }
            }
            Some("message_delta") => {
                if let Some(reason) = value
                    .get("delta")
                    .and_then(|d| d.get("stop_reason"))
                    .and_then(Value::as_str)
                {
                    out.stop_reason = Some(reason.to_owned());
                }
                if let Some(tokens) = value
                    .get("usage")
                    .and_then(|u| u64_at(u, &["output_tokens"]))
                {
                    out.usage.output_tokens = tokens;
                }
            }
            Some("message_stop") => {
                done = true;
                break;
            }
            _ => {}
        }
    }
    if !done && out.stop_reason.is_none() {
        return Err(AiError::Protocol("応答が途中で切れました".to_owned()));
    }
    Ok(out)
}

fn anthropic_whole(value: &Value) -> Result<Completion, AiError> {
    if let Some(message) = anthropic_error(value) {
        return Err(AiError::Server(message));
    }
    let content = value
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| AiError::Protocol("content がありません".to_owned()))?;
    let text: String = content
        .iter()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|b| b.get("text").and_then(Value::as_str))
        .collect();
    let usage = value.get("usage").map_or(Usage::default(), |u| Usage {
        input_tokens: u64_at(u, &["input_tokens"]).unwrap_or(0),
        output_tokens: u64_at(u, &["output_tokens"]).unwrap_or(0),
        estimated: false,
    });
    Ok(Completion {
        text,
        usage,
        stop_reason: value
            .get("stop_reason")
            .and_then(Value::as_str)
            .map(str::to_owned),
        thinking_chars: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(text: &str) -> Vec<(Option<String>, String)> {
        let cancel = AtomicBool::new(false);
        let mut reader = SseReader::new(BufReader::new(text.as_bytes()));
        let mut out = Vec::new();
        while let Some(e) = reader.next(&cancel).unwrap() {
            out.push((e.event, e.data));
        }
        out
    }

    #[test]
    fn sse_lines() {
        let got = events(
            ": ping\r\nevent: a\r\ndata: {\"x\":1}\r\n\r\ndata: 1\ndata: 2\n\n\n\ndata: last",
        );
        assert_eq!(
            got,
            vec![
                (Some("a".to_owned()), "{\"x\":1}".to_owned()),
                (None, "1\n2".to_owned()),
                (None, "last".to_owned()),
            ]
        );
    }

    #[test]
    fn estimates() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        assert_eq!(estimate_tokens("灯「やあ」"), 5);
    }

    #[test]
    fn error_messages() {
        assert_eq!(
            error_message(r#"{"error":{"message":"bad key","type":"x"}}"#),
            "bad key"
        );
        assert_eq!(
            error_message(r#"{"error":"model not found"}"#),
            "model not found"
        );
        assert_eq!(error_message("  "), "（本文なし）");
        assert_eq!(error_message("plain"), "plain");
    }
}
