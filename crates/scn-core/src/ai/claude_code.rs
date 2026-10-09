//! Claude Code（CLI）で呼ぶ（T4-26、N3）：Messages API の接続先を、この端末では `claude -p`（ログイン済みの CLI）
//! で呼ぶ。キーはこのアプリに置かない。
//!
//! - ツールは使わせない（`--tools ""`）。人の Claude Code の設定（`~/.claude/CLAUDE.md`、フック、プラグイン、MCP
//!   など）を読まない（`--safe-mode`・`--strict-mcp-config`。ログインとモデルの選び方はそのまま）。会話を残さない
//!   （`--no-session-persistence`）。作品フォルダの外の、空の作業用のフォルダで動かす（`CLAUDE.md` を拾わない）。
//!   始まりの行（`system`・`init`）にツールか MCP のサーバーがあれば、送る中身が見せたものと違いうるので止める。
//! - npm で入れた CLI（node のスクリプト）も動くよう、子の PATH の先に CLI のフォルダとよく入る場所を足す（macOS の
//!   アプリはシェルの PATH を受け継がない）。Windows ではコンソールの窓を出さず、止めるときは子のプロセスまで止める。
//! - 指示はファイル（`--system-prompt-file`）、依頼は標準入力で渡す（長さと文字の引数の制限を避ける）。
//! - 応答は `--output-format stream-json --verbose --include-partial-messages` で行ごとに読み、届いた文字と考えた
//!   中身を順に渡す（今の呼び出しにそろえる）。取り消しと、次のデータが届くまでの時間の区切りで CLI を止める。
//! - 返す分の上限（`max_tokens`）と温度は CLI に渡せないので渡さない。

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::client::{
    AiError, CallOptions, ChatRequest, Completion, Received, Role, Usage, estimate_tokens,
    strip_thinking,
};
use super::connection::{Connection, Effort};

/// CLI の名前。
pub const CLI_NAME: &str = "claude";
/// 標準エラーから読む量の上限。
const MAX_STDERR_BYTES: u64 = 64 * 1024;
/// 終わりの行（`result`）の後、CLI が終わるのを待つ長さ（出力をつかんだ子が残っても止まらないように）。
const AFTER_RESULT: Duration = Duration::from_secs(2);
/// Windows の `CREATE_NO_WINDOW`（コンソールの窓を出さない）。
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// CLI の場所：書いた場所があればそこ（無ければ見つからない）、無ければ PATH と、よく入る場所を探す（macOS の
/// アプリはシェルの PATH を受け継がないため）。
pub fn find_cli(written: Option<&str>) -> Option<PathBuf> {
    if let Some(path) = written.map(str::trim).filter(|p| !p.is_empty()) {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    let names: &[&str] = if cfg!(windows) {
        &["claude.exe", "claude.cmd"]
    } else {
        &[CLI_NAME]
    };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    dirs.extend(known_dirs());
    dirs.iter()
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|path| path.is_file())
}

/// CLI と node がよく入る場所。
fn known_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
        let home = PathBuf::from(home);
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join(".claude/local"));
        dirs.push(home.join(".npm-global/bin"));
        dirs.push(home.join(".volta/bin"));
        if cfg!(windows) {
            dirs.push(home.join("AppData/Roaming/npm"));
        }
    }
    if !cfg!(windows) {
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
    }
    dirs
}

/// CLI を起動する `Command`：子の PATH の先に、CLI のフォルダ（リンクならリンク先のフォルダも）とよく入る場所を
/// 足す（npm で入れた CLI は隣の node を使うため）。Windows ではコンソールの窓を出さない。
fn command(cli: &Path) -> Command {
    let mut command = Command::new(cli);
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(dir) = cli.parent() {
        dirs.push(dir.to_owned());
    }
    if let Some(dir) = std::fs::canonicalize(cli)
        .ok()
        .and_then(|p| p.parent().map(Path::to_owned))
    {
        dirs.push(dir);
    }
    dirs.extend(known_dirs());
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    if let Ok(path) = std::env::join_paths(dirs) {
        command.env("PATH", path);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// 接続先の CLI（見つからなければ、わけ）。
fn cli_of(conn: &Connection) -> Result<PathBuf, AiError> {
    find_cli(conn.cli.as_deref()).ok_or_else(|| {
        AiError::Connect(match conn.cli.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
            Some(path) => format!(
                "Claude Code の CLI（{path}）が見つかりません。環境設定で CLI の場所を確かめます"
            ),
            None => "Claude Code の CLI（claude）が見つかりません。インストールするか、環境設定で CLI の場所を書きます".to_owned(),
        })
    })
}

/// CLI の版（`claude --version`）。見つかるか・動くかを確かめる（ログインは [`cli_logged_in`]）。
pub fn cli_version(cli: &Path, timeout: Duration) -> Result<String, AiError> {
    let mut child = command(cli)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| spawn_error(cli, &e))?;
    let mut stdout = child.stdout.take();
    let reader = thread::spawn(move || {
        let mut text = String::new();
        if let Some(out) = stdout.as_mut() {
            let _ = out.take(MAX_STDERR_BYTES).read_to_string(&mut text);
        }
        text
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let text = reader.join().unwrap_or_default();
                let version = text.lines().next().unwrap_or_default().trim().to_owned();
                return if status.success() && !version.is_empty() {
                    Ok(version)
                } else {
                    Err(AiError::Server(format!(
                        "Claude Code の CLI（{}）が版を返しませんでした（{status}）",
                        cli.display()
                    )))
                };
            }
            Ok(None) if started.elapsed() > timeout => {
                stop(&mut child);
                return Err(AiError::Timeout);
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(AiError::Connect(e.to_string())),
        }
    }
}

/// CLI に渡す引数（指示のファイルの場所を除く）。
pub fn cli_args(model: &str, effort: Option<Effort>) -> Vec<String> {
    let mut args: Vec<String> = [
        "-p",
        "--safe-mode",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--tools",
        "",
        "--strict-mcp-config",
        "--no-session-persistence",
        "--model",
        model,
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    // 考える量（T4-28）。
    if let Some(effort) = effort {
        args.push("--effort".to_owned());
        args.push(effort.name().to_owned());
    }
    args
}

/// 標準入力で渡す依頼（発言が1つならそのまま。前の返事があれば印を付けてつなぐ）。
pub fn cli_prompt(request: &ChatRequest) -> String {
    let prompt = match request.messages.as_slice() {
        [only] if only.role == Role::User => only.content.clone(),
        messages => messages
            .iter()
            .map(|m| match m.role {
                Role::User => m.content.clone(),
                Role::Assistant => format!("（あなたの前の返事）\n{}", m.content),
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
    };
    // `/` で始まると、CLI がスラッシュコマンドとして読むことがあるので、見出しを前に置く（見せる中身にも入る）。
    if prompt.trim_start().starts_with('/') {
        format!("依頼：\n{prompt}")
    } else {
        prompt
    }
}

/// 送る中身の見せ方（クラウドへ送る前の確かめ）。
pub fn preview(conn: &Connection, request: &ChatRequest) -> String {
    let cli = conn.cli.as_deref().unwrap_or(CLI_NAME);
    let args: Vec<String> = cli_args(&conn.model, conn.effort)
        .iter()
        .map(|a| {
            if a.is_empty() {
                "\"\"".to_owned()
            } else {
                a.clone()
            }
        })
        .collect();
    format!(
        "{cli} {} --system-prompt-file （指示のファイル）\n（Claude Code の CLI のログインで、CLI に設定された行き先へ送ります。送ることの許可は {} のもの）\n\n【指示】\n{}\n\n【依頼（標準入力）】\n{}",
        args.join(" "),
        conn.endpoint,
        request.system.as_deref().unwrap_or("（なし）"),
        cli_prompt(request)
    )
}

/// CLI を動かす、決まった空のフォルダ（作品フォルダの外。呼ぶたびに変えると、CLI がフォルダごとに覚える状態が
/// 増えるので、決まったものにする）。
fn work_dir() -> Result<PathBuf, AiError> {
    let dir = std::env::temp_dir().join("scn-studio-claude-work");
    std::fs::create_dir_all(&dir)
        .map_err(|e| AiError::Connect(format!("作業用のフォルダを作れません：{e}")))?;
    Ok(dir)
}

/// 指示のファイルを置くフォルダ（呼ぶたびに作り、終わったら消す）。
struct PromptDir(PathBuf);

impl PromptDir {
    fn new() -> Result<Self, AiError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "scn-studio-claude-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir)
            .map_err(|e| AiError::Connect(format!("作業用のフォルダを作れません：{e}")))?;
        Ok(Self(dir))
    }
}

impl Drop for PromptDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn spawn_error(cli: &Path, e: &std::io::Error) -> AiError {
    if e.kind() == std::io::ErrorKind::NotFound {
        AiError::Connect(format!(
            "Claude Code の CLI（{}）が見つかりません",
            cli.display()
        ))
    } else {
        AiError::Connect(format!(
            "Claude Code の CLI（{}）を起動できません：{e}",
            cli.display()
        ))
    }
}

/// CLI を止める。Windows では、`claude.cmd` から起きた node まで止める（`taskkill /T`）。
fn stop(child: &mut Child) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// ログインしているか（`claude auth status --json` の `loggedIn`。LLM は呼ばない）。分からなければ `None`
/// （古い CLI など。そのときは「接続を試す」で確かめる）。
pub fn cli_logged_in(cli: &Path, timeout: Duration) -> Option<bool> {
    let mut child = command(cli)
        .args(["auth", "status", "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = (&mut stdout).take(MAX_STDERR_BYTES).read_to_end(&mut bytes);
        let _ = tx.send(bytes);
    });
    let Ok(bytes) = rx.recv_timeout(timeout) else {
        stop(&mut child);
        return None;
    };
    let _ = child.wait();
    serde_json::from_slice::<Value>(&bytes)
        .ok()?
        .get("loggedIn")
        .and_then(Value::as_bool)
}

/// Claude Code で呼ぶ。届いた文字は `on_text` に順に渡す。`cancel` が真になったら CLI を止める。
pub fn complete(
    conn: &Connection,
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
    let cli = cli_of(conn)?;
    let work = work_dir()?;
    let prompt_dir = PromptDir::new()?;
    let mut args = cli_args(&conn.model, conn.effort);
    if let Some(system) = request.system.as_deref().filter(|s| !s.is_empty()) {
        let file = prompt_dir.0.join("system.md");
        std::fs::write(&file, system)
            .map_err(|e| AiError::Connect(format!("指示のファイルを書けません：{e}")))?;
        args.push("--system-prompt-file".to_owned());
        args.push(file.display().to_string());
    }
    let mut child = command(&cli)
        .args(&args)
        .current_dir(&work)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| spawn_error(&cli, &e))?;
    // 依頼を書いて閉じる（読み終える前に応答を書く CLI でも詰まらないよう、別のスレッドで）。
    if let Some(mut stdin) = child.stdin.take() {
        let prompt = cli_prompt(request);
        thread::spawn(move || {
            let _ = stdin.write_all(prompt.as_bytes());
        });
    }
    let (tx, rx) = mpsc::channel::<String>();
    if let Some(stdout) = child.stdout.take() {
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
    }
    // 標準エラーは、UTF-8 でなくても読める分を読む（Windows の cmd.exe の誤りは CP932 のことがある）。
    let (err_tx, err_rx) = mpsc::channel::<String>();
    if let Some(mut err) = child.stderr.take() {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = (&mut err).take(MAX_STDERR_BYTES).read_to_end(&mut bytes);
            let _ = err_tx.send(String::from_utf8_lossy(&bytes).into_owned());
        });
    }
    let mut stream = Stream::default();
    let mut last = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            stop(&mut child);
            return Err(AiError::Cancelled);
        }
        if let Some(message) = stream.refused.take() {
            stop(&mut child);
            return Err(AiError::Server(message));
        }
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(line) => {
                last = Instant::now();
                stream.line(&line, on_text);
            }
            Err(RecvTimeoutError::Timeout) => {
                // 終わりの行が届いた後に出力が閉じなければ、少し待って止め、応答にする（時間切れにはしない）。
                if stream.result.is_some() {
                    if last.elapsed() > AFTER_RESULT.min(options.idle_timeout) {
                        stop(&mut child);
                        break;
                    }
                } else if last.elapsed() > options.idle_timeout {
                    stop(&mut child);
                    return Err(AiError::Timeout);
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    let success = wait_briefly(&mut child);
    let stderr = err_rx
        .recv_timeout(AFTER_RESULT)
        .unwrap_or_default()
        .trim()
        .to_owned();
    stream.finish(request, success.0, &success.1, &stderr)
}

/// 出力が閉じた後、CLI が終わるのを少しだけ待つ（終わらなければ止める）。成功したかと、終わり方の文。
fn wait_briefly(child: &mut Child) -> (bool, String) {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return (status.success(), status.to_string()),
            Ok(None) if started.elapsed() > AFTER_RESULT => {
                stop(child);
                return (true, "止めました".to_owned());
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(e) => return (false, e.to_string()),
        }
    }
}

/// 応答の行を読んだもの。
#[derive(Default)]
struct Stream {
    out: Completion,
    /// 少しずつの文字が届いた（まとまった返事の文字を重ねない）。
    deltas: bool,
    /// 終わりの行（`type: result`）。
    result: Option<Value>,
    /// 始まりの行にツールか MCP のサーバーがあったので、止めるわけ。
    refused: Option<String>,
}

impl Stream {
    fn line(&mut self, line: &str, on_text: &mut dyn FnMut(Received<'_>)) {
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
            return;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("system") if value.get("subtype").and_then(Value::as_str) == Some("init") => {
                let present = |key: &str| {
                    value
                        .get(key)
                        .and_then(Value::as_array)
                        .is_some_and(|items| !items.is_empty())
                };
                // プラグイン（`plugins`）は見ない：CLI の組み込みのもの（`builtin`）がいつも並び、入れたものも
                // 並ぶ。ツールが無ければ、プラグインのスキル・エージェント・コマンドは動かず、MCP のサーバーは
                // `mcp_servers` で、フックは `--safe-mode` で止まる。
                if let Some(key) = ["tools", "mcp_servers"].into_iter().find(|k| present(k)) {
                    self.refused = Some(format!(
                        "Claude Code の CLI が {key} を使える状態で始まったので、止めました（送る中身が見せたものと違いうるため）。CLI を新しくします（claude update）"
                    ));
                }
            }
            Some("stream_event") => self.event(value.get("event"), on_text),
            Some("assistant") => {
                let message = value.get("message");
                if !self.deltas {
                    for block in content_blocks(message) {
                        if block.get("type").and_then(Value::as_str) == Some("text")
                            && let Some(text) = block.get("text").and_then(Value::as_str)
                        {
                            self.out.text.push_str(text);
                            on_text(Received::Text(text));
                        }
                    }
                }
                if let Some(reason) = message
                    .and_then(|m| m.get("stop_reason"))
                    .and_then(Value::as_str)
                {
                    self.out.stop_reason = Some(reason.to_owned());
                }
            }
            Some("result") => self.result = Some(value),
            _ => {}
        }
    }

    fn event(&mut self, event: Option<&Value>, on_text: &mut dyn FnMut(Received<'_>)) {
        let Some(event) = event else { return };
        match event.get("type").and_then(Value::as_str) {
            Some("content_block_delta") => {
                let delta = event.get("delta");
                match delta.and_then(|d| d.get("type")).and_then(Value::as_str) {
                    Some("text_delta") => {
                        if let Some(text) =
                            delta.and_then(|d| d.get("text")).and_then(Value::as_str)
                        {
                            self.deltas = true;
                            self.out.text.push_str(text);
                            on_text(Received::Text(text));
                        }
                    }
                    Some("thinking_delta") => {
                        if let Some(thought) = delta
                            .and_then(|d| d.get("thinking"))
                            .and_then(Value::as_str)
                        {
                            self.out.thinking_chars = self
                                .out
                                .thinking_chars
                                .saturating_add(thought.chars().count() as u64);
                            on_text(Received::Thinking(thought));
                        }
                    }
                    _ => {}
                }
            }
            Some("message_delta") => {
                if let Some(reason) = event
                    .get("delta")
                    .and_then(|d| d.get("stop_reason"))
                    .and_then(Value::as_str)
                {
                    self.out.stop_reason = Some(reason.to_owned());
                }
            }
            _ => {}
        }
    }

    /// 終わりの行と終了の様子から、応答か誤りにする。
    fn finish(
        mut self,
        request: &ChatRequest,
        success: bool,
        status: &str,
        stderr: &str,
    ) -> Result<Completion, AiError> {
        let Some(result) = self.result.take() else {
            let detail = if stderr.is_empty() {
                "（標準エラーなし）"
            } else {
                stderr
            };
            return Err(if success {
                AiError::Protocol(format!(
                    "Claude Code の CLI が終わりの行（result）を返しませんでした：{detail}"
                ))
            } else if detail.to_ascii_lowercase().contains("unknown option") {
                AiError::Server(format!(
                    "Claude Code の CLI が知らない引数がありました。CLI を新しくします（claude update）：{detail}"
                ))
            } else {
                login_or_server(format!(
                    "Claude Code の CLI が止まりました（{status}）：{detail}"
                ))
            });
        };
        let text = result
            .get("result")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if result.get("is_error").and_then(Value::as_bool) == Some(true)
            || result
                .get("subtype")
                .and_then(Value::as_str)
                .is_some_and(|s| s != "success")
        {
            let subtype = result
                .get("subtype")
                .and_then(Value::as_str)
                .unwrap_or("error");
            let message = if text.is_empty() { subtype } else { text };
            return Err(login_or_server(format!(
                "Claude Code が誤りを返しました：{message}"
            )));
        }
        if !self.deltas && self.out.text.is_empty() {
            self.out.text = text.to_owned();
        }
        if let Some(usage) = result.get("usage") {
            let n = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
            self.out.usage = Usage {
                input_tokens: n("input_tokens")
                    + n("cache_creation_input_tokens")
                    + n("cache_read_input_tokens"),
                output_tokens: n("output_tokens"),
                estimated: false,
            };
        }
        if self.out.usage == Usage::default() {
            let mut sent = request.system.clone().unwrap_or_default();
            sent.push_str(&cli_prompt(request));
            self.out.usage = Usage {
                input_tokens: estimate_tokens(&sent),
                output_tokens: estimate_tokens(&self.out.text),
                estimated: true,
            };
        }
        // 考えた中身（`<think>`）を除く（今の呼び出しにそろえる）。
        let (text, thought) = strip_thinking(&self.out.text);
        self.out.text = text;
        self.out.thinking_chars = self.out.thinking_chars.saturating_add(thought);
        Ok(self.out)
    }
}

fn content_blocks(message: Option<&Value>) -> impl Iterator<Item = &Value> {
    message
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

/// ログインしていない（`/login` を求める）なら認証の誤り、ほかは途中の誤り。
fn login_or_server(message: String) -> AiError {
    let lower = message.to_ascii_lowercase();
    if lower.contains("/login")
        || lower.contains("not logged in")
        || lower.contains("invalid api key")
    {
        AiError::Auth {
            status: 401,
            message: format!(
                "{message}。ターミナルで claude を起動してログインしてから、もう一度試します"
            ),
        }
    } else {
        AiError::Server(message)
    }
}
