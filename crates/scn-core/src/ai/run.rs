//! AIタスクの実行（T3-7、D068）：文脈を組み立てて LLM を呼び、応答から指摘（行IDつき）やシーンの要約を読む。
//!
//! - 応答の形が崩れていても止めず、読めた分を返す（読めなかった行は `unread`）。指摘は1行に1つの JSON を基本に、
//!   JSON の配列、コードブロックの囲み、箇条書きの印、末尾のカンマも読む。
//! - 指摘の行IDは本文の行IDから位置を引く（無ければ `unresolved`）。行に結びつかない指摘はシーンに付ける。
//! - 今は指摘の段階（`review`：`findings`・`summary`）だけを実行する。提案・生成・自律は T3-8・T3-9。

use std::sync::atomic::AtomicBool;

use serde::Serialize;
use serde_json::Value;

use super::client::{AiError, CallOptions, ChatMessage, ChatRequest, Completion, Role, complete};
use super::connection::Connection;
use super::context::{Assembled, ContextSources, assemble};
use super::task::{AiTask, TaskLevel, TaskOutput};
use crate::Span;
use crate::index::build_index;
use crate::ir::Ir;
use crate::project::Project;

/// 指摘の重さ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FindingSeverity {
    Error,
    Warning,
    Info,
}

impl FindingSeverity {
    fn parse(text: &str) -> Self {
        match text.trim().to_ascii_lowercase().as_str() {
            "error" | "エラー" | "誤り" => Self::Error,
            "warning" | "warn" | "警告" => Self::Warning,
            _ => Self::Info,
        }
    }
}

/// 応答から読んだ指摘（位置を引く前）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFinding {
    /// 行ID（`^` を除く）。
    pub line: Option<String>,
    pub scene: Option<String>,
    pub severity: FindingSeverity,
    pub message: String,
}

/// 指摘。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub line: Option<String>,
    /// 指摘のシーン（行IDのシーン、書かれたシーン、無ければ実行の対象のシーン）。
    pub scene: Option<String>,
    pub severity: FindingSeverity,
    pub message: String,
    /// 本文の位置（行IDの行、シーンの見出し）。
    pub file: Option<String>,
    pub span: Option<Span>,
    /// 書かれた行IDやシーンが本文に無い。
    pub unresolved: bool,
}

/// 応答の文から、まわりのコードブロックの印（```json など）を除く。
fn without_fences(text: &str) -> String {
    text.lines()
        .filter(|l| !l.trim_start().starts_with("```"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn finding_of(value: &Value) -> Option<RawFinding> {
    let text = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let message = text("message")
        .or_else(|| text("text"))
        .or_else(|| text("comment"))?;
    Some(RawFinding {
        line: text("line").map(|l| l.trim_start_matches('^').to_owned()),
        scene: text("scene"),
        severity: FindingSeverity::parse(&text("severity").unwrap_or_default()),
        message,
    })
}

/// JSON の配列か `{"findings": [...]}` として読む。読めなければ `None`。
fn findings_of_json(text: &str) -> Option<(Vec<RawFinding>, Vec<String>)> {
    let value = serde_json::from_str::<Value>(text.trim()).ok()?;
    let items = value
        .as_array()
        .or_else(|| value.get("findings").and_then(Value::as_array))?;
    let mut found = Vec::new();
    let mut unread = Vec::new();
    for item in items {
        match finding_of(item) {
            Some(f) => found.push(f),
            None => unread.push(item.to_string()),
        }
    }
    Some((found, unread))
}

/// 応答の文から指摘を読む。読めなかった行（空でないもの）も返す。
pub fn parse_findings(text: &str) -> (Vec<RawFinding>, Vec<String>) {
    let text = without_fences(text);
    // まるごと JSON の配列か、`{"findings": [...]}` なら、それを読む。前置きの文があれば、最初の `[`（`{`）から
    // 最後の `]`（`}`）までを読んでみる（整形した配列を、行ごとには読めないため）。
    if let Some(read) = findings_of_json(&text) {
        return read;
    }
    for (open, close) in [('[', ']'), ('{', '}')] {
        // 指摘が1件以上読めたときだけ採る（指摘の文の中の `[…]` を配列と取り違えない）。
        if let (Some(start), Some(end)) = (text.find(open), text.rfind(close))
            && let Some(inner) = text.get(start..=end)
            && let Some(read) = findings_of_json(inner).filter(|(found, _)| !found.is_empty())
        {
            let before = text.get(..start).unwrap_or_default().trim();
            let after = text.get(end + 1..).unwrap_or_default().trim();
            let (found, mut unread) = read;
            for rest in [before, after] {
                unread.extend(
                    rest.lines()
                        .map(str::trim)
                        .filter(|l| !l.is_empty())
                        .map(str::to_owned),
                );
            }
            return (found, unread);
        }
    }
    if let Ok(value) = serde_json::from_str::<Value>(text.trim())
        && let Some(f) = finding_of(&value)
    {
        return (vec![f], Vec::new());
    }
    let mut found = Vec::new();
    let mut unread = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == "[" || trimmed == "]" {
            continue;
        }
        let body = trimmed
            .trim_start_matches(['-', '*', '・'])
            .trim_start()
            .trim_end_matches(',');
        let parsed = body
            .starts_with('{')
            .then(|| serde_json::from_str::<Value>(body).ok())
            .flatten()
            .and_then(|v| finding_of(&v));
        match parsed {
            Some(f) => found.push(f),
            None => unread.push(trimmed.to_owned()),
        }
    }
    (found, unread)
}

/// 指摘の行IDとシーンから、本文の位置を引く。`target` は実行の対象のシーン。
pub fn resolve_findings(
    project: &Project,
    ir: &Ir,
    raw: Vec<RawFinding>,
    target: Option<&str>,
) -> Vec<Finding> {
    let index = build_index(project, ir);
    raw.into_iter()
        .map(|f| {
            let at_line = f
                .line
                .as_deref()
                .and_then(|id| index.line_ids.iter().find(|l| l.id == id));
            let named_scene = f
                .scene
                .as_deref()
                .and_then(|id| ir.scenes.iter().find(|s| s.id == id));
            let (scene, file, span) = match (at_line, named_scene) {
                (Some(l), _) => (
                    Some(l.location.scene.clone()),
                    Some(l.location.file.clone()),
                    Some(l.location.span),
                ),
                (None, Some(s)) => (Some(s.id.clone()), Some(s.file.clone()), Some(s.span)),
                // 行もシーンも本文に無い：対象のシーンに付ける（書かれたシーンが無いときも）。
                (None, None) => {
                    let fallback = target.and_then(|id| ir.scenes.iter().find(|s| s.id == id));
                    (
                        target.map(str::to_owned),
                        fallback.map(|s| s.file.clone()),
                        fallback.map(|s| s.span),
                    )
                }
            };
            let unresolved = (f.line.is_some() && at_line.is_none())
                || (f.scene.is_some() && named_scene.is_none() && at_line.is_none());
            Finding {
                line: f.line,
                scene,
                severity: f.severity,
                message: f.message,
                file,
                span,
                unresolved,
            }
        })
        .collect()
}

/// シーンの要約の応答から、要約の文を取る（コードブロックの印と、まわりの括弧・引用符を除く）。
pub fn parse_summary(text: &str) -> Option<String> {
    let text = without_fences(text);
    let text = text
        .trim()
        .trim_matches(|c| matches!(c, '"' | '「' | '」'))
        .trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// 実行の結果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunOutcome {
    pub task: String,
    pub scene: Option<String>,
    /// 使った接続先の名前とモデル。
    pub connection: String,
    pub model: String,
    pub findings: Vec<Finding>,
    /// シーンの要約（`output: summary`）。
    pub summary: Option<String>,
    /// 応答のうち、読めなかった行。
    pub unread: Vec<String>,
    /// 応答の文（そのまま）。
    pub text: String,
    pub completion: Completion,
}

/// 実行できなかった。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RunError {
    /// 文脈を組み立てられない（シーンが要る、入りきらないなど）。
    #[error("{0}")]
    Context(String),
    /// まだ実行できない段階（提案・生成・自律）。
    #[error("{0}")]
    Unsupported(String),
    /// クラウドへ送る中身が、人が確かめた中身と違う（確かめていない、本文や設定が変わった）。
    #[error(
        "クラウドへ送る中身が、確かめた中身と違います（本文や設定が変わったかもしれません）。もう一度「送る中身を見る」で確かめてから送ります"
    )]
    Unconfirmed,
    #[error("{0}")]
    Ai(#[from] AiError),
}

/// 実行できる段階か（今は指摘の段階だけ）。
pub fn runnable(task: &AiTask) -> Result<(), String> {
    match task.level {
        TaskLevel::Review => Ok(()),
        TaskLevel::Propose => Err(format!(
            "{} は提案（propose）のタスクです。提案の実行は T3-8 で作ります",
            task.name
        )),
        TaskLevel::Generate | TaskLevel::Auto => Err(format!(
            "{} は生成・自律のタスクです。生成と自律の実行は T3-9 で作ります",
            task.name
        )),
    }
}

/// 組み立てた文脈を、LLM への頼みにする。
pub fn chat_request(assembled: &Assembled) -> ChatRequest {
    ChatRequest {
        system: Some(assembled.system.clone()),
        messages: vec![ChatMessage {
            role: Role::User,
            content: assembled.user.clone(),
        }],
        // 返す分（文脈の長さから文脈の上限を除いたもの）。
        max_tokens: u32::try_from(assembled.context_tokens.saturating_sub(assembled.limit)).ok(),
        temperature: None,
    }
}

/// 送る中身の要約値（行き先、モデル、指示と依頼の FNV-1a 64 ビットの16進）。クラウドへは、人が確かめた中身と
/// 同じときだけ送る（[`run_task`] の `confirmed`）。
pub fn content_digest(conn: &Connection, assembled: &Assembled) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in [
        conn.endpoint.as_str(),
        conn.model.as_str(),
        assembled.system.as_str(),
        assembled.user.as_str(),
    ] {
        for b in part.bytes().chain(std::iter::once(0x1f)) {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{hash:016x}")
}

/// タスクを実行する（指摘の段階）。文脈を組み立てて呼び、応答を読む。クラウドの接続先では、組み立てた中身の
/// 要約値（[`content_digest`]）が `confirmed`（人が確かめた中身）と同じでなければ送らない。
#[allow(clippy::too_many_arguments)]
pub fn run_task(
    project: &Project,
    ir: &Ir,
    sources: &ContextSources,
    task: &AiTask,
    scene: Option<&str>,
    conn: &Connection,
    key: Option<&str>,
    confirmed: Option<&str>,
    options: &CallOptions,
    cancel: &AtomicBool,
    on_text: &mut dyn FnMut(&str),
) -> Result<RunOutcome, RunError> {
    runnable(task).map_err(RunError::Unsupported)?;
    let assembled = assemble(project, ir, sources, task, scene, conn.context_tokens);
    if let Some(problem) = assembled.problem {
        return Err(RunError::Context(problem));
    }
    if conn.cloud && confirmed != Some(content_digest(conn, &assembled).as_str()) {
        return Err(RunError::Unconfirmed);
    }
    let completion = complete(
        conn,
        key,
        &chat_request(&assembled),
        options,
        cancel,
        on_text,
    )?;
    let text = completion.text.clone();
    let (findings, summary, unread) = match task.output {
        TaskOutput::Summary => {
            let summary = parse_summary(&text);
            let unread = if summary.is_none() && !text.trim().is_empty() {
                vec![text.trim().to_owned()]
            } else {
                Vec::new()
            };
            (Vec::new(), summary, unread)
        }
        _ => {
            let (raw, unread) = parse_findings(&text);
            (resolve_findings(project, ir, raw, scene), None, unread)
        }
    };
    Ok(RunOutcome {
        task: task.id.clone(),
        scene: scene.map(str::to_owned),
        connection: conn.name.clone(),
        model: conn.model.clone(),
        findings,
        summary,
        unread,
        text,
        completion,
    })
}
