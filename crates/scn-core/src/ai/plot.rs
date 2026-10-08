//! プロットとフローへの AI 支援（T3-10、D071）：発散（提案ノード）、生成（ルートの骨格）、レビュー（コアが計算した
//! 構造の検証の結果を文脈に入れる）、同期（シーン要約とメタ情報の `summary` のずれ）、問い返し（企画書とメモへの
//! 質問、メモの分類案）。
//!
//! - 提案ノードの採用とルートの骨格の書き込みは、フロー図からの編集（T2-9）と同じく書き足すだけで、人が書いた行は
//!   変えない。足したシーンは `status: ai-draft` にし、行IDの付く行（`@選択` と選択肢）の出自を `ai` と記録する。
//!   書くのは `workspace`。
//! - 到達できるか、変数が整合するかは、コアが計算した結果（[`structure_text`]）だけを文脈に入れ、AI に推し量らせ
//!   ない。

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::context::{SummaryCache, scene_source, source_hash};
use super::proposal::{DiffPart, char_diff};
use super::run::parse_json_items;
use super::task::AiTask;
use crate::ideas::STATUSES;
use crate::ir::{Ir, IrOption, Op};
use crate::project::Project;

/// シーンIDに使える字（英数字と `_`）か。
pub fn is_scene_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// 見出しの行（`## 題名 #ID`）から、題名とID。IDが無ければ `None`。
fn heading_of(line: &str) -> Option<(String, Option<String>)> {
    let rest = line.trim().strip_prefix("## ")?;
    match rest.rsplit_once(" #") {
        Some((title, id)) if is_scene_id(id.trim()) => {
            Some((title.trim().to_owned(), Some(id.trim().to_owned())))
        }
        _ => Some((rest.trim().to_owned(), None)),
    }
}

/// コードブロックがあれば、すべてのブロックの中身と、ブロックの外の `keep` に合う行（案ごとの見出しなど）。
/// 無ければ全体。
fn fenced_or_all<'a>(text: &'a str, keep: &dyn Fn(&str) -> bool) -> Vec<&'a str> {
    let lines: Vec<&str> = text.lines().collect();
    if !lines.iter().any(|l| l.trim_start().starts_with("```")) {
        return lines;
    }
    let mut out = Vec::new();
    let mut inside = false;
    for line in lines {
        if line.trim_start().starts_with("```") {
            inside = !inside;
            continue;
        }
        if inside || keep(line.trim()) {
            out.push(line);
        }
    }
    out
}

/// 行末の行ID（` ^xxxx`）があるか。
fn has_line_id(line: &str) -> bool {
    let t = line.trim_end_matches([' ', '\t', '\r']);
    t.rsplit_once(" ^").is_some_and(|(_, id)| {
        (4..=12).contains(&id.len())
            && id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    })
}

/// メタ情報の行（`key: value`）の値。
fn meta_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let (k, v) = line.split_once(':')?;
    (k.trim() == key).then(|| v.trim())
}

/// 行の中のジャンプの行き先（`-> ID`）。
fn jump_target(line: &str) -> Option<String> {
    let (_, to) = line.rsplit_once("->")?;
    let to = to.trim();
    is_scene_id(to).then(|| to.to_owned())
}

// ---- 発散：提案ノード ----

/// 提案ノードで足すスタブのシーン。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StubPlan {
    pub id: String,
    pub title: String,
    pub summary: Option<String>,
}

/// 提案ノード（分岐案の1つ）：今のシーンに足す選択肢と、その行き先（今あるシーンか、足すスタブ）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchProposal {
    /// 提案の名前（見出し）。
    pub name: String,
    /// 選択肢を足すシーン。
    pub from: String,
    /// 選択肢の表示文。
    pub label: String,
    /// 選んだときの `@設定` の行（`@設定 …` の形のまま）。
    pub effects: Vec<String>,
    /// 行き先のシーン。
    pub target: String,
    /// 足すスタブのシーン。
    pub stubs: Vec<StubPlan>,
    pub reason: Option<String>,
    /// 採用できないわけ（無ければ採用できる）。
    pub problems: Vec<String>,
}

/// 提案ノードの応答（`output: proposals`）を読む。`from` は対象のシーン。提案ごとに `### ` の見出しで区切り、
/// その中の最初の選択肢（`- 表示文`。字下げした `@設定` と `-> 行き先`）と、スタブのシーン（`## 題名 #ID` と
/// メタ情報の `summary`）と、`理由：` の行を読む。本文の行は読まない。コードブロックがあれば、その中の行と、
/// ブロックの外の `### ` の見出しと `理由：` の行を読む（案ごとに .scn をブロックで囲む応答）。選択肢もスタブも
/// 無い節（`### まとめ` など）は案にしない。
pub fn parse_branch_proposals(text: &str, ir: &Ir, from: &str) -> Vec<BranchProposal> {
    let keep = |t: &str| t.starts_with("### ") || t.starts_with("理由：") || t.starts_with("理由:");
    let mut sections: Vec<(String, Vec<&str>)> = Vec::new();
    for line in fenced_or_all(text, &keep) {
        let t = line.trim();
        if let Some(name) = t.strip_prefix("### ") {
            sections.push((proposal_name(name), Vec::new()));
        } else if let Some((_, lines)) = sections.last_mut() {
            lines.push(line);
        }
    }
    sections
        .into_iter()
        .filter_map(|(name, lines)| read_proposal(name, &lines, ir, from))
        .collect()
}

/// `提案 1：灯を追いかける` → `灯を追いかける`（名前が無ければ見出しのまま）。
fn proposal_name(heading: &str) -> String {
    let heading = heading.trim();
    for sep in ['：', ':'] {
        if let Some((head, rest)) = heading.split_once(sep)
            && head.trim_start().starts_with("提案")
            && !rest.trim().is_empty()
        {
            return rest.trim().to_owned();
        }
    }
    heading.to_owned()
}

fn read_proposal(name: String, lines: &[&str], ir: &Ir, from: &str) -> Option<BranchProposal> {
    let mut label: Option<String> = None;
    let mut target: Option<String> = None;
    let mut effects: Vec<String> = Vec::new();
    let mut stubs: Vec<StubPlan> = Vec::new();
    let mut reason = None;
    let mut problems = Vec::new();
    let mut in_option = false;
    let mut in_meta = false;
    let mut after_heading = false;
    for raw in lines {
        let line = raw.trim_end();
        let t = line.trim_start_matches([' ', '\t']);
        if in_meta {
            if t == "---" {
                in_meta = false;
            } else if let (Some(stub), Some(v)) = (stubs.last_mut(), meta_value(t, "summary"))
                && !v.is_empty()
            {
                stub.summary = Some(v.to_owned());
            }
            continue;
        }
        if let Some(r) = t.strip_prefix("理由：").or_else(|| t.strip_prefix("理由:")) {
            reason = Some(r.trim().to_owned()).filter(|r| !r.is_empty());
            in_option = false;
            continue;
        }
        if t.starts_with("## ") {
            in_option = false;
            match heading_of(t) {
                Some((title, Some(id))) => {
                    stubs.push(StubPlan {
                        id,
                        title,
                        summary: None,
                    });
                    after_heading = true;
                }
                Some((title, None)) => {
                    problems.push(format!("スタブ「{title}」にシーンIDがありません"))
                }
                None => {}
            }
            continue;
        }
        if after_heading && t == "---" {
            in_meta = true;
            after_heading = false;
            continue;
        }
        if !t.is_empty() {
            after_heading = false;
        }
        let indented = line.starts_with([' ', '\t']);
        if label.is_none() && t.starts_with("- ") && stubs.is_empty() {
            let body = t.trim_start_matches("- ").trim();
            let (text, to) = match body.rsplit_once("->") {
                Some((text, to)) if is_scene_id(to.trim()) => {
                    (text.trim(), Some(to.trim().to_owned()))
                }
                _ => (body, None),
            };
            label = Some(text.to_owned());
            if to.is_some() {
                target = to;
            }
            in_option = true;
            continue;
        }
        if in_option && indented {
            if t.starts_with("@設定 ") || t.starts_with("@set ") {
                effects.push(t.to_owned());
            } else if t.starts_with("->") && target.is_none() {
                target = jump_target(t);
            }
            continue;
        }
        if !t.is_empty() {
            in_option = false;
        }
    }
    // 選択肢もスタブも無い節は、案ではない。
    if label.is_none() && stubs.is_empty() && problems.is_empty() {
        return None;
    }
    // 選択肢が無ければ、最初のスタブへの選択肢にする（表示文はスタブの題名）。
    if label.is_none()
        && let Some(first) = stubs.first()
    {
        label = Some(first.title.clone());
        target.get_or_insert_with(|| first.id.clone());
    }
    if target.is_none()
        && let Some(first) = stubs.first()
    {
        target = Some(first.id.clone());
    }
    let label = label.unwrap_or_default();
    if label.is_empty() {
        problems.push("選択肢の表示文がありません".to_owned());
    } else if label.contains(['\n', '\r']) || label.contains("->") {
        problems.push("選択肢の表示文に改行か -> があります".to_owned());
    }
    let target = target.unwrap_or_default();
    let existing: BTreeSet<&str> = ir.scenes.iter().map(|s| s.id.as_str()).collect();
    if !existing.contains(from) {
        problems.push(format!("シーン {from} がありません"));
    }
    if target.is_empty() {
        problems.push("行き先がありません".to_owned());
    } else if !existing.contains(target.as_str()) && !stubs.iter().any(|s| s.id == target) {
        problems.push(format!("行き先 {target} がありません"));
    }
    let mut seen = BTreeSet::new();
    for stub in &stubs {
        if existing.contains(stub.id.as_str()) {
            problems.push(format!("シーンID {} はもう使われています", stub.id));
        } else if !seen.insert(stub.id.as_str()) {
            problems.push(format!("シーンID {} が2つあります", stub.id));
        }
        if stub.title.is_empty() {
            problems.push(format!("スタブ {} に題名がありません", stub.id));
        }
    }
    Some(BranchProposal {
        name,
        from: from.to_owned(),
        label,
        effects,
        target,
        stubs,
        reason,
        problems,
    })
}

// ---- 生成：ルートの骨格 ----

/// ルートの骨格の1つのシーン（スタブ）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkeletonScene {
    pub id: String,
    pub title: String,
    pub summary: Option<String>,
    pub route: Option<String>,
    /// 流れの行（`@選択`・選択肢・`@end`・`-> 行き先`・`@設定`・`@if`・`@elif`・`@else`・`@エンディング`）。
    /// 字下げはそのまま。
    pub lines: Vec<String>,
}

/// 骨格に書いてよい流れの行か。
fn is_skeleton_line(t: &str) -> bool {
    if t.starts_with("->") || t.starts_with("- ") || t == "-" {
        return true;
    }
    let Some(directive) = t.strip_prefix('@') else {
        return false;
    };
    let name = directive.split_whitespace().next().unwrap_or_default();
    [
        "選択",
        "choice",
        "end",
        "設定",
        "set",
        "if",
        "elif",
        "else",
        "エンディング",
        "ending",
    ]
    .contains(&name)
}

/// ルートの骨格の応答（生成の段階で、文脈に今のシーンの無いタスク）から、スタブのシーンを読む。コードブロックが
/// あればその中だけ。見出しとメタ情報の `summary`・`route`、流れの行だけを読み、本文・演出・コメント・行IDの
/// 付いた行は読まない。シーンIDの無い見出しのシーンは読まずに知らせる。
pub fn parse_skeleton(text: &str) -> (Vec<SkeletonScene>, Vec<String>) {
    let mut scenes: Vec<SkeletonScene> = Vec::new();
    let mut unread = Vec::new();
    let mut skipping = false;
    let mut in_meta = false;
    let mut after_heading = false;
    let keep = |t: &str| t.starts_with("## ") && heading_of(t).is_some_and(|(_, id)| id.is_some());
    for raw in fenced_or_all(text, &keep) {
        let line = raw.trim_end();
        let t = line.trim_start_matches([' ', '\t']);
        if in_meta {
            if t == "---" {
                in_meta = false;
            } else if let Some(scene) = scenes.last_mut().filter(|_| !skipping) {
                if let Some(v) = meta_value(t, "summary").filter(|v| !v.is_empty()) {
                    scene.summary = Some(v.to_owned());
                } else if let Some(v) = meta_value(t, "route").filter(|v| !v.is_empty()) {
                    scene.route = Some(v.to_owned());
                }
            }
            continue;
        }
        if t.starts_with("## ") {
            match heading_of(t) {
                // 同じ見出しをくり返した応答（ブロックの外と中）は、1つのシーンとして読む。
                Some((_, Some(id)))
                    if scenes.last().is_some_and(|s| {
                        s.id == id && s.summary.is_none() && s.lines.is_empty()
                    }) =>
                {
                    skipping = false;
                }
                Some((title, Some(id))) => {
                    scenes.push(SkeletonScene {
                        id,
                        title,
                        summary: None,
                        route: None,
                        lines: Vec::new(),
                    });
                    skipping = false;
                }
                _ => {
                    unread.push(format!("シーンIDの無い見出し：{t}"));
                    skipping = true;
                }
            }
            after_heading = true;
            continue;
        }
        if after_heading && t == "---" {
            in_meta = true;
            after_heading = false;
            continue;
        }
        if !t.is_empty() {
            after_heading = false;
        }
        if skipping || t.is_empty() || has_line_id(t) || !is_skeleton_line(t) {
            continue;
        }
        if let Some(scene) = scenes.last_mut() {
            scene.lines.push(line.to_owned());
        }
    }
    (scenes, unread)
}

/// 骨格の行の行き先（ジャンプと `@呼出` は書かない）。
pub fn skeleton_targets(scene: &SkeletonScene) -> Vec<String> {
    scene
        .lines
        .iter()
        .filter(|l| l.contains("->"))
        .filter_map(|l| jump_target(l))
        .collect()
}

/// 骨格のシーンの `.scn` の本文（見出し、メタ情報（`summary`・`route`・`status: ai-draft`）、流れの行）。
/// `route` はシーンに書かれていなければ使うルート。
pub fn skeleton_scene_text(scene: &SkeletonScene, route: Option<&str>, nl: &str) -> String {
    let mut out = format!("## {} #{}{nl}---{nl}", scene.title, scene.id);
    if let Some(summary) = scene
        .summary
        .as_deref()
        .and_then(crate::ideas::summary_line)
    {
        out.push_str(&format!("summary: {summary}{nl}"));
    }
    if let Some(route) = scene.route.as_deref().or(route) {
        out.push_str(&format!("route: {route}{nl}"));
    }
    out.push_str(&format!("status: ai-draft{nl}---{nl}"));
    for line in &scene.lines {
        out.push_str(line);
        out.push_str(nl);
    }
    out
}

/// 骨格の依頼に、対象のルートを書き足したタスク。
pub fn with_route(task: &AiTask, route: &str) -> AiTask {
    let mut t = task.clone();
    let prompt = t.prompt.unwrap_or_default();
    t.prompt = Some(format!(
        "{}\n\n対象のルート：{}（足すシーンのメタ情報の route に書きます）",
        prompt.trim_end(),
        route.trim()
    ));
    t
}

// ---- レビュー：構造の検証の結果 ----

/// 選択肢の結果（行き先と、変数の書き込み）。中に分岐や呼び出しのある選択肢は比べない（`None`）。
fn option_outcome(option: &IrOption) -> Option<(Option<String>, Vec<String>)> {
    let mut jump = option.jump.clone();
    let mut sets = Vec::new();
    for op in &option.body {
        match op {
            Op::Set { var, op, value, .. } => sets.push(format!("{var} {op} {value:?}")),
            Op::Jump { target, .. } => jump = Some(target.clone()),
            Op::Choice { .. }
            | Op::If { .. }
            | Op::Call { .. }
            | Op::Ending { .. }
            | Op::Return { .. } => {
                return None;
            }
            _ => {}
        }
    }
    sets.sort();
    Some((jump, sets))
}

fn visit<'a>(ops: &'a [Op], f: &mut dyn FnMut(&'a Op)) {
    for op in ops {
        f(op);
        match op {
            Op::Choice { options, .. } => {
                for o in options {
                    visit(&o.body, f);
                }
            }
            Op::If {
                branches,
                else_body,
                ..
            } => {
                for b in branches {
                    visit(&b.body, f);
                }
                if let Some(body) = else_body {
                    visit(body, f);
                }
            }
            _ => {}
        }
    }
}

/// どれを選んでも、行き先と変数の書き込みが同じ選択（シーン、選択の行ID、行き先）。
pub fn same_outcome_choices(ir: &Ir) -> Vec<(String, Option<String>, Option<String>)> {
    let mut out = Vec::new();
    for scene in &ir.scenes {
        visit(&scene.ops, &mut |op| {
            if let Op::Choice {
                line_id, options, ..
            } = op
                && options.len() >= 2
            {
                let outcomes: Vec<_> = options.iter().map(option_outcome).collect();
                if let Some(Some(first)) = outcomes.first()
                    && outcomes.iter().all(|o| o.as_ref() == Some(first))
                {
                    out.push((scene.id.clone(), line_id.clone(), first.0.clone()));
                }
            }
        });
    }
    out
}

/// 診断のあるシーン（ファイルの中で、診断の位置より前の見出しのうち、いちばん後ろ）。
fn scene_at(ir: &Ir, file: &str, at: usize) -> Option<String> {
    ir.scenes
        .iter()
        .filter(|s| s.file == file && s.span.start <= at)
        .max_by_key(|s| s.span.start)
        .map(|s| s.id.clone())
}

/// 構造の検証の結果（文脈の `structure`）：ルートごとの分量、エンディングへの到達とその例、カバレッジ、結果が
/// 同じになる選択、流れと伏線の診断（検証の段階1と自動巡回）。どれもコアが計算したもの。
pub fn structure_text(project: &Project, ir: &Ir) -> String {
    let mut out = String::from(
        "## 構造の検証の結果（ツールが計算したもの。到達できるか、変数が整合するかは、これを正とします）\n",
    );
    out.push_str("\n### ルートごとの分量（道に沿って読む台詞と地の文の文字数）\n");
    let stats = crate::route::route_stats(project, ir);
    if stats.is_empty() {
        out.push_str("（なし）\n");
    }
    for s in &stats {
        out.push_str(&format!(
            "- {}：{}字（約{}分）\n",
            s.label, s.chars, s.minutes
        ));
    }
    let report = crate::playthrough::playthrough(project, ir);
    out.push_str("\n### エンディングへの到達（自動巡回）\n");
    if report.endings.is_empty() {
        out.push_str("（エンディングがありません）\n");
    }
    for e in &report.endings {
        if e.reached {
            let example = if e.example.is_empty() {
                "選択なし".to_owned()
            } else {
                e.example
                    .iter()
                    .map(|c| format!("「{c}」"))
                    .collect::<Vec<_>>()
                    .join(" → ")
            };
            out.push_str(&format!("- {}（{}）：着く。例：{example}\n", e.id, e.scene));
        } else {
            out.push_str(&format!("- {}（{}）：着かない\n", e.id, e.scene));
        }
    }
    out.push_str(&format!(
        "\n### カバレッジ\n- シーン {}/{}、選択肢 {}/{}（{}）\n",
        report.scenes.reached,
        report.scenes.total,
        report.options.reached,
        report.options.total,
        match &report.limit {
            None => "調べきった".to_owned(),
            Some(why) => format!("上限で止めた：{why}"),
        }
    ));
    out.push_str("\n### 結果が同じになる選択\n");
    let same = same_outcome_choices(ir);
    if same.is_empty() {
        out.push_str("（なし）\n");
    }
    for (scene, line, to) in &same {
        let id = line
            .as_deref()
            .map(|l| format!("（^{l}）"))
            .unwrap_or_default();
        let to = to.as_deref().unwrap_or("続き");
        out.push_str(&format!(
            "- {scene} の選択{id}：どれを選んでも {to} へ進み、変数の書き込みも同じ\n"
        ));
    }
    out.push_str("\n### 流れと伏線の診断\n");
    let index = crate::index::build_index(project, ir);
    let mut diags: Vec<_> = ir
        .diagnostics
        .iter()
        .cloned()
        .chain(crate::check::check(project, ir, &index))
        .chain(report.diagnostics)
        .filter(|d| {
            d.code.starts_with("W5") || d.code.starts_with("W7") || d.code.starts_with("E5")
        })
        .collect();
    diags.sort_by(|a, b| (&a.file, a.span.start, a.code).cmp(&(&b.file, b.span.start, b.code)));
    diags.dedup_by(|a, b| a.file == b.file && a.span.start == b.span.start && a.code == b.code);
    if diags.is_empty() {
        out.push_str("（なし）\n");
    }
    for d in &diags {
        let scene = scene_at(ir, &d.file, d.span.start).unwrap_or_else(|| d.file.clone());
        out.push_str(&format!("- {} {scene}：{}\n", d.code, d.message));
    }
    out.trim_end().to_owned()
}

/// 書き足した後の検証（T3-10）：検証の段階1の、`scenes` のシーンの中の誤りと警告（`コード シーン：メッセージ`）。
pub fn scene_checks(project: &Project, ir: &Ir, scenes: &[String]) -> Vec<String> {
    let index = crate::index::build_index(project, ir);
    ir.diagnostics
        .iter()
        .cloned()
        .chain(crate::check::check(project, ir, &index))
        .filter(|d| d.code.starts_with('E') || d.code.starts_with('W'))
        .filter_map(|d| {
            let scene = scene_at(ir, &d.file, d.span.start)?;
            scenes
                .contains(&scene)
                .then(|| format!("{} {scene}：{}", d.code, d.message))
        })
        .collect()
}

/// 文脈に使うアイデアメモ（`ideas/*.md`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdeaNote {
    pub file: String,
    pub status: String,
    pub tags: Vec<String>,
    pub body: String,
}

/// 作品フォルダのアイデアメモ（名前の順）。
pub fn read_idea_notes(root: &std::path::Path) -> Vec<IdeaNote> {
    crate::ideas::idea_files(root)
        .into_iter()
        .map(|(file, text)| {
            let idea = crate::ideas::read_idea(&file, text.as_deref());
            IdeaNote {
                file: idea.file,
                status: idea.status,
                tags: idea.tags,
                body: idea.body,
            }
        })
        .collect()
}

/// アイデアメモの一覧（文脈の `ideas`）：ファイル、状態、タグ、本文の初めの3行。
pub fn ideas_text(ideas: &[IdeaNote]) -> Option<String> {
    if ideas.is_empty() {
        return None;
    }
    let blocks: Vec<String> = ideas
        .iter()
        .map(|idea| {
            let tags = if idea.tags.is_empty() {
                String::new()
            } else {
                format!("、タグ：{}", idea.tags.join("・"))
            };
            let body: Vec<&str> = idea
                .body
                .lines()
                .map(str::trim_end)
                .filter(|l| !l.trim().is_empty())
                .take(3)
                .collect();
            format!(
                "### {}（状態：{}{tags}）\n{}",
                idea.file,
                idea.status,
                body.join("\n")
            )
        })
        .collect();
    Some(format!("## アイデアメモ\n\n{}", blocks.join("\n\n")))
}

// ---- 同期：シーン要約と summary のずれ ----

/// 複数行の要約を1行にする（行の前後の空白を除いてつなぐ。英数字どうしの間には半角空白を入れる）。
pub fn one_line(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let ascii = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
        if ascii(out.chars().last()) && ascii(line.chars().next()) {
            out.push(' ');
        }
        out.push_str(line);
    }
    out
}

/// シーン要約（キャッシュ）と、メタ情報の `summary` のずれ。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SummaryDrift {
    pub scene: String,
    pub title: String,
    pub file: String,
    /// 今のメタ情報の `summary`（無ければ `None`）。
    pub current: Option<String>,
    /// 本文から作り直した要約。
    pub proposed: String,
    /// 文字ごとの差分（今の summary から要約へ）。
    pub diff: Vec<DiffPart>,
}

/// 本文から作り直した要約（キャッシュの、本文の変わっていないもの）が、メタ情報の `summary` と違うシーン（IR の順）。
pub fn summary_drift(project: &Project, ir: &Ir, cache: &SummaryCache) -> Vec<SummaryDrift> {
    ir.scenes
        .iter()
        .filter_map(|s| {
            let cached = cache.scenes.get(&s.id)?;
            let source = scene_source(project, ir, &s.id)?;
            if source_hash(source) != cached.hash {
                return None;
            }
            let proposed = crate::ideas::summary_line(&one_line(&cached.summary))?;
            let current = s.meta.summary.clone();
            if current.as_deref().map(str::trim) == Some(proposed.trim()) {
                return None;
            }
            Some(SummaryDrift {
                scene: s.id.clone(),
                title: s.title.clone(),
                file: s.file.clone(),
                diff: char_diff(current.as_deref().unwrap_or_default(), &proposed),
                current,
                proposed,
            })
        })
        .collect()
}

/// シーンのキャッシュの要約のうち、今の本文から作ったもの（本文が変わっていれば `None`）。
pub fn fresh_summary(
    project: &Project,
    ir: &Ir,
    cache: &SummaryCache,
    scene: &str,
) -> Option<String> {
    let source = scene_source(project, ir, scene)?;
    cache
        .scenes
        .get(scene)
        .filter(|c| c.hash == source_hash(source))
        .map(|c| c.summary.clone())
}

/// 要約を作り直すシーン（キャッシュに無いか、本文が変わったもの。本文の順）。`file` があれば、その章だけ。
pub fn summaries_to_refresh(
    project: &Project,
    ir: &Ir,
    cache: &SummaryCache,
    file: Option<&str>,
) -> Vec<String> {
    ir.scenes
        .iter()
        .filter(|s| file.is_none_or(|f| s.file == f))
        .filter(|s| {
            let hash = scene_source(project, ir, &s.id).map(source_hash);
            cache
                .scenes
                .get(&s.id)
                .is_none_or(|c| Some(&c.hash) != hash.as_ref())
        })
        .map(|s| s.id.clone())
        .collect()
}

/// シーンの本文（見出しから）のメタ情報の `key` の値を `value` にする（無ければ足し、メタ情報が無ければ作る）。
/// ほかの行は1バイトも変えない。メタ情報が閉じていない（E102）なら、そのわけ。
pub fn set_meta_line(scene_text: &str, key: &str, value: &str) -> Result<String, String> {
    let nl = if scene_text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let lines: Vec<&str> = scene_text.split_inclusive('\n').collect();
    let Some(heading) = lines.first() else {
        return Err("シーンがありません".to_owned());
    };
    let mut out = String::from(*heading);
    if !heading.ends_with('\n') {
        out.push_str(nl);
    }
    let is_fence = |l: &str| l.trim_end_matches(['\r', '\n']).trim() == "---";
    let rest = lines.get(1..).unwrap_or_default();
    if !rest.first().is_some_and(|l| is_fence(l)) {
        out.push_str(&format!("---{nl}{key}: {value}{nl}---{nl}"));
        for line in rest {
            out.push_str(line);
        }
        return Ok(out);
    }
    if !rest.iter().skip(1).any(|l| is_fence(l)) {
        return Err("メタ情報が閉じていない（E102）ので、書きません".to_owned());
    }
    let mut wrote = false;
    let mut closed = false;
    for (i, line) in rest.iter().enumerate() {
        if closed || i == 0 {
            out.push_str(line);
            continue;
        }
        if is_fence(line) {
            if !wrote {
                out.push_str(&format!("{key}: {value}{nl}"));
            }
            out.push_str(line);
            closed = true;
            continue;
        }
        if !wrote && line.split(':').next().is_some_and(|k| k.trim() == key) {
            let end = if line.ends_with("\r\n") {
                "\r\n"
            } else if line.ends_with('\n') {
                "\n"
            } else {
                ""
            };
            // キーの前の字下げとキーの書き方は残す。
            let colon = line.find(':').unwrap_or(0);
            out.push_str(line.get(..=colon).unwrap_or(line));
            out.push_str(&format!(" {value}{end}"));
            wrote = true;
            continue;
        }
        out.push_str(line);
    }
    Ok(out)
}

// ---- 問い返し：メモの分類案 ----

/// アイデアメモの分類案（`output: ideas`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdeaSuggestion {
    /// メモのファイル（`ideas/…`）。
    pub file: String,
    /// 状態の案（`STATUSES` のどれか）。
    pub status: Option<String>,
    /// 足すタグの案。
    pub tags: Vec<String>,
    pub reason: Option<String>,
}

fn suggestion_of(value: &Value) -> Option<IdeaSuggestion> {
    let text = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let file = text("file")?;
    let status = text("status").filter(|s| STATUSES.contains(&s.as_str()));
    let tags: Vec<String> = value
        .get("tags")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|t| !t.is_empty() && !t.contains([',', '[', ']', '\n']))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    if status.is_none() && tags.is_empty() {
        return None;
    }
    Some(IdeaSuggestion {
        file,
        status,
        tags,
        reason: text("reason"),
    })
}

/// メモの分類案の応答を読む（1行に1つの JSON。崩れていても読めた分）。`ideas` に無いメモへの案は読まない。
pub fn parse_idea_suggestions(
    text: &str,
    ideas: &[IdeaNote],
) -> (Vec<IdeaSuggestion>, Vec<String>) {
    let (found, mut unread) = parse_json_items(text, "ideas", &suggestion_of);
    let mut out = Vec::new();
    for s in found {
        if ideas.iter().any(|i| i.file == s.file) {
            out.push(s);
        } else {
            unread.push(format!("無いメモへの案：{}", s.file));
        }
    }
    (out, unread)
}
