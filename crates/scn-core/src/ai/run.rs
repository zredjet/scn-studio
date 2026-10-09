//! AIタスクの実行（T3-7、D068）：文脈を組み立てて LLM を呼び、応答から指摘（行IDつき）やシーンの要約を読む。
//!
//! - 応答の形が崩れていても止めず、読めた分を返す（読めなかった行は `unread`）。指摘は1行に1つの JSON を基本に、
//!   JSON の配列、コードブロックの囲み、箇条書きの印、末尾のカンマも読む。
//! - 指摘の行IDは本文の行IDから位置を引く（無ければ `unresolved`）。行に結びつかない指摘はシーンに付ける。
//! - 今は指摘の段階（`review`：`findings`・`summary`）だけを実行する。提案・生成・自律は T3-8・T3-9。

use std::sync::atomic::AtomicBool;

use serde::Serialize;
use serde_json::{Value, json};

use super::client::{
    AiError, CallOptions, ChatMessage, ChatRequest, Completion, OutputSchema, Received, Role,
    complete, ensure_answer,
};
use super::connection::Connection;
use super::context::{Assembled, ContextSources, assemble};
use super::fielddraft::{ConceptDraft, concept_schema, parse_concept_draft};
use super::generate::generated_body;
use super::plot::{
    BranchProposal, IdeaSuggestion, SkeletonScene, parse_branch_proposals, parse_idea_suggestions,
    parse_skeleton,
};
use super::proposal::{Proposal, proposal_of};
use super::task::{AiTask, ContextItem, TaskLevel, TaskOutput};
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

/// JSON の配列か `{"<key>": [...]}` として読む。読めなければ `None`。
fn items_of_json<T>(
    text: &str,
    key: &str,
    item: &dyn Fn(&Value) -> Option<T>,
) -> Option<(Vec<T>, Vec<String>)> {
    let value = serde_json::from_str::<Value>(text.trim()).ok()?;
    let items = value
        .as_array()
        .or_else(|| value.get(key).and_then(Value::as_array))?;
    let mut found = Vec::new();
    let mut unread = Vec::new();
    for value in items {
        match item(value) {
            Some(f) => found.push(f),
            None => unread.push(value.to_string()),
        }
    }
    Some((found, unread))
}

/// 応答の文から、1行に1つの JSON の項目を読む（指摘と提案に共通）。JSON の配列・`{"<key>": [...]}`・
/// コードブロックの囲み・前置きの文・箇条書きの印・末尾のカンマも読む。読めなかった行（空でないもの）も返す。
pub fn parse_json_items<T>(
    text: &str,
    key: &str,
    item: &dyn Fn(&Value) -> Option<T>,
) -> (Vec<T>, Vec<String>) {
    let text = without_fences(text);
    // まるごと JSON の配列か、`{"<key>": [...]}` なら、それを読む。前置きの文があれば、最初の `[`（`{`）から
    // 最後の `]`（`}`）までを読んでみる（整形した配列を、行ごとには読めないため）。
    if let Some(read) = items_of_json(&text, key, item) {
        return read;
    }
    for (open, close) in [('[', ']'), ('{', '}')] {
        // 項目が1つ以上読めたときだけ採る（文の中の `[…]` を配列と取り違えない）。
        if let (Some(start), Some(end)) = (text.find(open), text.rfind(close))
            && let Some(inner) = text.get(start..=end)
            && let Some(read) =
                items_of_json(inner, key, item).filter(|(found, _)| !found.is_empty())
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
        && let Some(f) = item(&value)
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
            .and_then(|v| item(&v));
        match parsed {
            Some(f) => found.push(f),
            None => unread.push(trimmed.to_owned()),
        }
    }
    (found, unread)
}

/// 応答の文から指摘を読む。読めなかった行（空でないもの）も返す。
pub fn parse_findings(text: &str) -> (Vec<RawFinding>, Vec<String>) {
    parse_json_items(text, "findings", &finding_of)
}

/// 応答の文から提案（行IDつきの差分。T3-8）を読む。読めなかった行も返す。
pub fn parse_diff(text: &str) -> (Vec<Proposal>, Vec<String>) {
    parse_json_items(text, "proposals", &proposal_of)
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
    /// 提案（`output: diff`。T3-8）。
    pub proposals: Vec<Proposal>,
    /// 生成の下書き（`output: draft`。書き込む本文の行。T3-9）。
    pub draft: Vec<String>,
    /// 提案ノード（`output: proposals`。T3-10）。
    pub branches: Vec<BranchProposal>,
    /// ルートの骨格（生成の段階で、今のシーンを文脈に入れないタスク。足すスタブのシーン。T3-10）。
    pub skeleton: Vec<SkeletonScene>,
    /// アイデアメモの分類案（`output: ideas`。T3-10）。
    pub ideas: Vec<IdeaSuggestion>,
    /// 企画書の下書き（`output: concept`。T4-4）。書くのは別（`Workspace::write_concept_draft`）。
    pub concept: Option<ConceptDraft>,
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

/// 実行できる段階か。自律は章を選んで `run_auto` で実行する。
pub fn runnable(task: &AiTask) -> Result<(), String> {
    match (task.level, task.output) {
        (TaskLevel::Review, _) | (TaskLevel::Propose, TaskOutput::Diff) => Ok(()),
        // 提案ノードは、対象のシーンの後に足す選択肢の案（T3-10）。
        (TaskLevel::Propose, _) if task.context.contains(&ContextItem::Scene) => Ok(()),
        (TaskLevel::Propose, _) => Err(format!(
            "{} は提案ノード（proposals）を出すタスクです。対象のシーンを文脈に入れます（context に scene）",
            task.name
        )),
        // 生成は、今のシーン（スタブ）に書くタスク（T3-9）と、シーンを足すルートの骨格（T3-10）。
        (TaskLevel::Generate, _) => Ok(()),
        (TaskLevel::Auto, _) => Err(format!(
            "{} は自律のタスクです。章を選んで自律で実行します",
            task.name
        )),
    }
}

/// ルートの骨格を作るタスク（生成の段階で本文に書き、今のシーンを文脈に入れないもの）か。
pub fn is_skeleton(task: &AiTask) -> bool {
    task.level == TaskLevel::Generate
        && task.output == TaskOutput::Draft
        && !task.context.contains(&ContextItem::Scene)
}

/// 出力の形ごとの、返す形の JSON Schema（構造化出力。接続先で有効にしたときだけ送る。T4-2）。JSON で返す形
/// （指摘・提案・メモの分類案）だけ。読み方（[`parse_json_items`]）は `{"<名前>": [...]}` も読む。
pub fn output_schema(output: TaskOutput) -> Option<OutputSchema> {
    let text = || json!({"type": "string"});
    let (name, item) = match output {
        TaskOutput::Findings => (
            "findings",
            json!({
                "type": "object",
                "properties": {
                    "line": text(),
                    "scene": text(),
                    "severity": {"type": "string", "enum": ["error", "warning", "info"]},
                    "message": text(),
                },
                "required": ["severity", "message"],
            }),
        ),
        TaskOutput::Diff => (
            "proposals",
            json!({
                "type": "object",
                "properties": {
                    "line": text(),
                    "before": text(),
                    "after": text(),
                    "reason": text(),
                },
                "required": ["line", "after"],
            }),
        ),
        TaskOutput::Ideas => (
            "ideas",
            json!({
                "type": "object",
                "properties": {
                    "file": text(),
                    "status": {"type": "string", "enum": ["未整理", "採用", "保留", "没"]},
                    "tags": {"type": "array", "items": text()},
                    "reason": text(),
                },
                "required": ["file"],
            }),
        ),
        TaskOutput::Concept => return Some(concept_schema()),
        TaskOutput::Summary | TaskOutput::Proposals | TaskOutput::Draft => return None,
    };
    Some(OutputSchema {
        name: name.to_owned(),
        schema: json!({
            "type": "object",
            "properties": {name: {"type": "array", "items": item}},
            "required": [name],
        }),
    })
}

/// 組み立てた文脈を、LLM への頼みにする（返す形があれば添える。T4-2）。
pub fn chat_request(task: &AiTask, assembled: &Assembled) -> ChatRequest {
    ChatRequest {
        system: Some(assembled.system.clone()),
        messages: vec![ChatMessage {
            role: Role::User,
            content: assembled.user.clone(),
        }],
        // 返す分（文脈の長さから文脈の上限を除いたもの）。
        max_tokens: u32::try_from(assembled.context_tokens.saturating_sub(assembled.limit)).ok(),
        temperature: None,
        schema: output_schema(task.output),
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
    on_text: &mut dyn FnMut(Received<'_>),
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
        &chat_request(task, &assembled),
        options,
        cancel,
        on_text,
    )?;
    ensure_answer(&completion)?;
    let text = completion.text.clone();
    let mut proposals = Vec::new();
    let mut draft = Vec::new();
    let mut branches = Vec::new();
    let mut skeleton = Vec::new();
    let mut ideas = Vec::new();
    let mut concept = None;
    let unread_if = |empty: bool| {
        if empty && !text.trim().is_empty() {
            vec![text.trim().to_owned()]
        } else {
            Vec::new()
        }
    };
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
        TaskOutput::Diff => {
            let (found, unread) = parse_diff(&text);
            proposals = found;
            (Vec::new(), None, unread)
        }
        TaskOutput::Draft if is_skeleton(task) => {
            let (scenes, unread) = parse_skeleton(&text);
            skeleton = scenes;
            let mut unread = unread;
            unread.extend(unread_if(skeleton.is_empty()));
            (Vec::new(), None, unread)
        }
        TaskOutput::Draft => {
            draft = generated_body(&text);
            (Vec::new(), None, unread_if(draft.is_empty()))
        }
        TaskOutput::Proposals => {
            branches = parse_branch_proposals(&text, ir, scene.unwrap_or_default());
            (Vec::new(), None, unread_if(branches.is_empty()))
        }
        TaskOutput::Ideas => {
            let (found, unread) = parse_idea_suggestions(&text, &sources.ideas);
            ideas = found;
            (Vec::new(), None, unread)
        }
        TaskOutput::Concept => {
            // 読まなかった項目のわけも、読めなかった行として見せる。
            let (draft, unread) = parse_concept_draft(&text);
            concept = draft;
            (Vec::new(), None, unread)
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
        proposals,
        draft,
        branches,
        skeleton,
        ideas,
        concept,
        unread,
        text,
        completion,
    })
}
