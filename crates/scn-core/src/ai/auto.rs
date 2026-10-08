//! 自律（T3-9、D070）：章のスタブのシーンを、上から順に「展開メモ → 本文 → 点検（`check` と AI のレビュー）→
//! 直す」で下書きにし、条件を満たすか反復の上限に達したらスタブに書き込む。トークン数の上限に達したら止める。
//!
//! - 点検と直しは、書き込む前に、下書きを差し込んだ作品の写しで行う。書き込むのは各シーンで1回だけ（人の行は
//!   変えない。書き込みはワークスペースの [`crate::workspace::Workspace::write_draft`]）。
//! - LLM の呼び出し・書き込み・進み具合の知らせは [`AutoHost`] が受け持つ（アプリは、呼び出しの間ワークスペースを
//!   ロックしないように、書き込みのときだけロックする）。
//! - 取り消しとトークン数の上限は、段（呼び出し）の合間ごとに見る。取り消したら、できている下書きも書かない。上限に
//!   達したら、できている下書きは書き込み（点検の残りを知らせる）、次の段を始めない。

use serde::Serialize;

use super::client::{AiError, ChatRequest, Completion};
use super::connection::Connection;
use super::context::{ContextSources, assemble, preamble};
use super::generate::{
    draft_into_scene, generated_body, scene_range, stubs_in, with_line_ids, without_temp_ids,
};
use super::run::{chat_request, parse_findings, resolve_findings};
use super::task::{AiTask, ContextItem, TaskLevel, TaskOutput};
use crate::index::build_index;
use crate::ir::Ir;
use crate::line_id::LineIdGenerator;
use crate::project::Project;

/// 反復の上限を書かないときの既定。
pub const DEFAULT_MAX_ITERATIONS: u32 = 3;

/// 自律の進み具合の知らせ。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AutoEvent {
    /// 下書きにするシーン（本文の順）。
    Planned { scenes: Vec<String> },
    /// シーンの段（`beats`・`draft`・`check`・`fix`・`write`）。
    Step {
        scene: String,
        step: &'static str,
        iteration: u32,
    },
    /// シーンが終わった。
    SceneDone { scene: AutoScene },
    /// 使ったトークン数。
    Tokens { used: u64 },
}

/// シーンの結果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AutoScene {
    pub scene: String,
    pub title: String,
    /// `written`（書き込んだ）・`skipped`（書き込まなかった）・`failed`（呼び出しに失敗した）。
    pub status: &'static str,
    /// 書き込んだ本文の字数。
    pub chars: usize,
    /// 直した回数。
    pub iterations: u32,
    /// 書き込んだときに残っていた点検の結果。
    pub issues: Vec<String>,
    pub message: Option<String>,
    /// 書き込んだ行の行ID。
    pub ids: Vec<String>,
}

/// 自律の結果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AutoOutcome {
    pub task: String,
    pub file: String,
    pub scenes: Vec<AutoScene>,
    /// 使ったトークン数と上限。
    pub tokens: u64,
    pub budget: Option<u64>,
    /// 途中で止めたわけ（上限、取り消し、呼び出しの失敗）。
    pub stopped: Option<String>,
}

/// 自律の実行を支えるもの。
pub trait AutoHost {
    /// 今の作品（保存していない本文も）。
    fn project(&mut self) -> Result<Project, String>;
    fn sources(&mut self) -> ContextSources;
    /// タスクの接続先とキー。
    fn connection(&mut self, task: &AiTask) -> Result<(Connection, Option<String>), String>;
    fn complete(
        &mut self,
        conn: &Connection,
        key: Option<&str>,
        request: &ChatRequest,
    ) -> Result<Completion, AiError>;
    /// スタブに書き込む（書き込んだ行の行ID。書き込まなかったらわけ）。
    fn write(&mut self, file: &str, scene: &str, body: &[String]) -> Result<Vec<String>, String>;
    fn event(&mut self, event: AutoEvent);
    fn cancelled(&self) -> bool;
}

/// 自律の中で使う、1つのシーンに書くタスク（自律のタスクの文脈に、前のシーンまでの要約と今のシーンを足す）。
fn scene_task(task: &AiTask, prompt: String) -> AiTask {
    let mut t = task.clone();
    t.level = TaskLevel::Generate;
    t.output = TaskOutput::Draft;
    for item in [ContextItem::PreviousSummaries, ContextItem::Scene] {
        if !t.context.contains(&item) {
            t.context.push(item);
        }
    }
    t.prompt = Some(prompt);
    t
}

/// 点検のための写し：下書きを差し込んだ作品、差し込んだ範囲、仮の行ID（書き込むときは付け直す）。
struct Candidate {
    project: Project,
    range: (usize, usize),
    temp_ids: Vec<String>,
}

fn candidate(
    project: &Project,
    ir: &Ir,
    file: &str,
    scene: &str,
    draft: &[String],
) -> Option<Candidate> {
    let (scene_file, start, end) = scene_range(project, ir, scene)?;
    if scene_file != file {
        return None;
    }
    let text = &project.files.iter().find(|f| f.path == file)?.text;
    let drafted = draft_into_scene(text.get(start..end)?, draft).ok()?;
    let new_text = format!("{}{}{}", text.get(..start)?, drafted.text, text.get(end..)?);
    let range = (start + drafted.inserted.0, start + drafted.inserted.1);
    let (with_ids, temp_ids) = with_line_ids(
        project,
        file,
        &new_text,
        range,
        &mut LineIdGenerator::with_seed(0x5ca1_ab1e),
    );
    let mut copy = project.clone();
    let f = copy.files.iter_mut().find(|f| f.path == file)?;
    f.text = with_ids;
    // 行IDを付けた分だけ、範囲の終わりがずれる。
    let grown = f.text.len().saturating_sub(new_text.len());
    Some(Candidate {
        project: copy,
        range: (range.0, range.1 + grown),
        temp_ids,
    })
}

/// 止めたわけ。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Halt {
    /// 人が取り消した（下書きを書かない）。
    Cancelled,
    /// トークン数の上限（できている下書きは書く）。
    Budget(String),
    /// 呼び出しや文脈の誤り（できている下書きは書く）。
    Error(String),
}

impl Halt {
    fn message(&self) -> String {
        match self {
            Halt::Cancelled => "取り消しました".to_owned(),
            Halt::Budget(m) | Halt::Error(m) => m.clone(),
        }
    }
}

/// 呼び出しを数え、上限と取り消しを見る。
struct Meter<'a> {
    host: &'a mut dyn AutoHost,
    used: u64,
    budget: Option<u64>,
}

impl Meter<'_> {
    fn over(&self) -> Option<Halt> {
        if self.host.cancelled() {
            return Some(Halt::Cancelled);
        }
        match self.budget {
            Some(budget) if self.used >= budget => Some(Halt::Budget(format!(
                "トークン数の上限（{budget}）に達しました（使った {}）",
                self.used
            ))),
            _ => None,
        }
    }

    /// 上限と取り消しを見てから呼ぶ。
    fn call(&mut self, task: &AiTask, request: &ChatRequest) -> Result<Completion, Halt> {
        if let Some(halt) = self.over() {
            return Err(halt);
        }
        let (conn, key) = self.host.connection(task).map_err(Halt::Error)?;
        let done = self
            .host
            .complete(&conn, key.as_deref(), request)
            .map_err(|e| {
                if e == AiError::Cancelled {
                    Halt::Cancelled
                } else {
                    Halt::Error(e.to_string())
                }
            })?;
        self.used = self
            .used
            .saturating_add(done.usage.input_tokens)
            .saturating_add(done.usage.output_tokens);
        self.host.event(AutoEvent::Tokens { used: self.used });
        Ok(done)
    }

    fn context_tokens(&mut self, task: &AiTask) -> Result<u64, Halt> {
        self.host
            .connection(task)
            .map(|(c, _)| c.context_tokens)
            .map_err(Halt::Error)
    }
}

/// 下書きの点検：`check`（構文・意味・検証の段階1と、段階2の表記の検査（I204〜I206）のうち、下書きの行の範囲の
/// もの）と、`checks` の指摘のタスク（下書きの行への指摘だけ）。
#[allow(clippy::too_many_arguments)]
fn inspect(
    meter: &mut Meter<'_>,
    task: &AiTask,
    all: &[AiTask],
    sources: &ContextSources,
    candidate: &Candidate,
    file: &str,
    scene: &str,
) -> Result<Vec<String>, Halt> {
    let project = &candidate.project;
    let ir = crate::analyze(project);
    let (start, end) = candidate.range;
    let mut issues = Vec::new();
    if task.checks.iter().any(|c| c == "check") {
        let index = build_index(project, &ir);
        let mut diags = ir.diagnostics.clone();
        diags.extend(crate::check::check(project, &ir, &index));
        diags.extend(crate::wording::wording(project, &ir));
        for d in diags {
            let inside = d.file == file && d.span.start >= start && d.span.start < end;
            let counted = matches!(d.code.chars().next(), Some('E' | 'W'))
                || matches!(d.code, "I204" | "I205" | "I206");
            if inside && counted {
                issues.push(format!("{} {}", d.code, d.message));
            }
        }
    }
    for name in task.checks.iter().filter(|c| c.as_str() != "check") {
        let Some(check) = all.iter().find(|t| &t.id == name) else {
            issues.push(format!("（点検のタスク {name} がありません）"));
            continue;
        };
        if check.level != TaskLevel::Review || check.output != TaskOutput::Findings {
            continue;
        }
        let ctx = meter.context_tokens(check)?;
        let assembled = assemble(project, &ir, sources, check, Some(scene), ctx);
        if let Some(problem) = assembled.problem {
            issues.push(format!("（{} を実行できません：{problem}）", check.name));
            continue;
        }
        let done = meter.call(check, &chat_request(&assembled))?;
        let (raw, _) = parse_findings(&done.text);
        for f in resolve_findings(project, &ir, raw, Some(scene)) {
            if let Some(line) = f.line.as_ref().filter(|l| candidate.temp_ids.contains(l)) {
                issues.push(format!("^{line} {}（{}）", f.message, check.name));
            }
        }
    }
    Ok(issues)
}

/// 点検の結果の仮の行IDを、書き込んだ行の行IDに付け替える（差し込んだ順で対応する）。
fn rename_ids(issues: &[String], temp: &[String], written: &[String]) -> Vec<String> {
    if temp.len() != written.len() {
        return issues.to_vec();
    }
    issues
        .iter()
        .map(|issue| {
            let mut out = issue.clone();
            for (t, w) in temp.iter().zip(written) {
                out = out.replace(&format!("^{t}"), &format!("^{w}"));
            }
            out
        })
        .collect()
}

/// 自律で、章（`file`）のスタブのシーンを下書きにする。`all` は作品のすべてのタスク（`checks` が指す）。
pub fn run_auto(host: &mut dyn AutoHost, task: &AiTask, file: &str, all: &[AiTask]) -> AutoOutcome {
    let max_iterations = task.max_iterations.unwrap_or(DEFAULT_MAX_ITERATIONS);
    let mut outcome = AutoOutcome {
        task: task.id.clone(),
        file: file.to_owned(),
        scenes: Vec::new(),
        tokens: 0,
        budget: task.budget_tokens,
        stopped: None,
    };
    let project = match host.project() {
        Ok(p) => p,
        Err(e) => {
            outcome.stopped = Some(e);
            return outcome;
        }
    };
    let ir = crate::analyze(&project);
    let scenes = stubs_in(&ir, file);
    host.event(AutoEvent::Planned {
        scenes: scenes.clone(),
    });
    let mut meter = Meter {
        host,
        used: 0,
        budget: task.budget_tokens,
    };
    let base_prompt = task.prompt.clone().unwrap_or_default();
    for scene in scenes {
        if let Some(halt) = meter.over() {
            outcome.stopped = Some(halt.message());
            break;
        }
        // 書いている間に人が書き足したかもしれないので、シーンごとに今の作品を読み直す。
        let project = match meter.host.project() {
            Ok(p) => p,
            Err(e) => {
                outcome.stopped = Some(e);
                break;
            }
        };
        let ir = crate::analyze(&project);
        let mut result = AutoScene {
            scene: scene.clone(),
            title: ir
                .scenes
                .iter()
                .find(|s| s.id == scene)
                .map(|s| s.title.clone())
                .unwrap_or_default(),
            status: "skipped",
            chars: 0,
            iterations: 0,
            issues: Vec::new(),
            message: None,
            ids: Vec::new(),
        };
        if !ir.scenes.iter().any(|s| s.id == scene && s.is_stub) {
            result.message = Some("もうスタブではないので書き込みません".to_owned());
            meter.host.event(AutoEvent::SceneDone {
                scene: result.clone(),
            });
            outcome.scenes.push(result);
            continue;
        }
        let sources = meter.host.sources();
        let (draft, halt) = draft_scene(
            &mut meter,
            task,
            all,
            &project,
            &ir,
            &sources,
            file,
            &scene,
            &base_prompt,
            max_iterations,
            &mut result,
        );
        // 書き込む（取り消したときは書かない。上限や誤りで止めたときは、できている下書きを書く）。
        if halt == Some(Halt::Cancelled) {
            result.message = Some("取り消したので書き込みませんでした".to_owned());
        } else if draft.is_empty() {
            if result.message.is_none() {
                result.message = Some("書き込む本文の行がありません".to_owned());
            }
            if matches!(halt, Some(Halt::Error(_))) {
                result.status = "failed";
            }
        } else {
            meter.host.event(AutoEvent::Step {
                scene: scene.clone(),
                step: "write",
                iteration: result.iterations,
            });
            match meter.host.write(file, &scene, &draft.lines) {
                Ok(ids) => {
                    result.status = "written";
                    result.chars = draft.lines.iter().map(|l| l.chars().count()).sum();
                    result.issues = rename_ids(&result.issues, &draft.temp_ids, &ids);
                    result.ids = ids;
                    if halt.is_some() && result.message.is_none() {
                        result.message = Some("点検の途中で止めました".to_owned());
                    }
                }
                Err(e) => result.message = Some(e),
            }
        }
        meter.host.event(AutoEvent::SceneDone {
            scene: result.clone(),
        });
        outcome.scenes.push(result);
        if let Some(halt) = halt {
            outcome.stopped = Some(halt.message());
            break;
        }
    }
    // 最後のシーンで上限に達していたら、それも知らせる。
    if outcome.stopped.is_none()
        && let Some(halt) = meter.over()
    {
        outcome.stopped = Some(halt.message());
    }
    outcome.tokens = meter.used;
    outcome
}

/// できた下書き（書き込む本文の行と、最後の点検で付けた仮の行ID）。
#[derive(Default)]
struct Draft {
    lines: Vec<String>,
    temp_ids: Vec<String>,
}

impl Draft {
    fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// 1つのシーンの、展開メモ → 本文 → 点検 → 直す。止めたら、そのわけも返す。
#[allow(clippy::too_many_arguments)]
fn draft_scene(
    meter: &mut Meter<'_>,
    task: &AiTask,
    all: &[AiTask],
    project: &Project,
    ir: &Ir,
    sources: &ContextSources,
    file: &str,
    scene: &str,
    base_prompt: &str,
    max_iterations: u32,
    result: &mut AutoScene,
) -> (Draft, Option<Halt>) {
    let step = |meter: &mut Meter<'_>, step: &'static str, iteration: u32| {
        meter.host.event(AutoEvent::Step {
            scene: scene.to_owned(),
            step,
            iteration,
        });
    };
    let ask = |meter: &mut Meter<'_>,
               on: &Project,
               prompt: String,
               beats: bool|
     -> Result<String, Halt> {
        let t = scene_task(task, prompt);
        let ctx = meter.context_tokens(&t)?;
        let on_ir = crate::analyze(on);
        let mut assembled = assemble(on, &on_ir, sources, &t, Some(scene), ctx);
        if let Some(problem) = assembled.problem.take() {
            return Err(Halt::Error(problem));
        }
        if beats {
            assembled.system = format!(
                "{}\n\n対象のシーンの展開メモ（出来事の流れ）だけを、5〜8行の箇条書きで返してください。本文はまだ書きません。",
                preamble()
            );
        }
        Ok(meter.call(&t, &chat_request(&assembled))?.text)
    };
    let _ = ir;
    let mut draft = Draft::default();
    // 1. 展開メモ。
    step(meter, "beats", 0);
    let beats = match ask(meter, project, base_prompt.to_owned(), true) {
        Ok(text) => text,
        Err(halt) => {
            result.message = Some(halt.message());
            return (draft, Some(halt));
        }
    };
    // 2. 本文。
    step(meter, "draft", 0);
    match ask(
        meter,
        project,
        format!(
            "{base_prompt}\n\n展開メモ：\n{}\n\n対象のシーンの本文を、展開メモに沿って書いてください。",
            beats.trim()
        ),
        false,
    ) {
        Ok(text) => draft.lines = generated_body(&text),
        Err(halt) => {
            result.message = Some(halt.message());
            return (draft, Some(halt));
        }
    }
    // 3. 点検と直し（反復の上限まで）。
    // 直した後、点検し直す前に止めたら、前の点検の結果は今の下書きのものではないので捨てる。
    let unchecked = |result: &mut AutoScene, halt: Halt| {
        if result.iterations > 0 {
            result.issues.clear();
            result.message = Some(format!(
                "直した後の点検の前に止めました（{}）",
                halt.message()
            ));
        }
        Some(halt)
    };
    loop {
        if draft.is_empty() {
            return (draft, None);
        }
        if let Some(halt) = meter.over() {
            return (draft, unchecked(result, halt));
        }
        step(meter, "check", result.iterations);
        let Some(cand) = candidate(project, ir, file, scene, &draft.lines) else {
            return (draft, None);
        };
        draft.temp_ids = cand.temp_ids.clone();
        match inspect(meter, task, all, sources, &cand, file, scene) {
            Ok(issues) => result.issues = issues,
            Err(halt) => return (draft, unchecked(result, halt)),
        }
        if result.issues.is_empty() || result.iterations >= max_iterations {
            return (draft, None);
        }
        result.iterations += 1;
        step(meter, "fix", result.iterations);
        let fixed = ask(
            meter,
            &cand.project,
            format!(
                "{base_prompt}\n\n対象のシーンの下書きについて、次の点を直した本文を、同じ形で返してください。\n{}",
                result.issues.join("\n")
            ),
            false,
        );
        match fixed {
            Ok(text) => {
                // 点検のために付けた仮の行IDは、応答から除いて読む（行IDの付いた行は書かないため）。
                let lines = generated_body(&without_temp_ids(&text, &cand.temp_ids));
                if !lines.is_empty() {
                    draft.lines = lines;
                }
            }
            Err(halt) => return (draft, Some(halt)),
        }
    }
}
