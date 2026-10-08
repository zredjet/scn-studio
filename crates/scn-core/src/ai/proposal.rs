//! 提案（T3-8、D069）：人の行への変更を、行IDつきの差分（今の文、案、理由）として持ち、人が行ごとに採用する。
//!
//! - アプリの実行（`output: diff`）の応答は1行に1つの JSON、外部のAIエージェントが置く `ai/proposals/*.md` は
//!   Markdown。どちらも [`Proposal`] にそろえる。
//! - 採用は人の操作だけ（原則6）。採用した行だけを置き換え、行頭の字下げと行IDは変えない。今の行が提案の「今」と
//!   違う（本文が変わった）、行IDが無い・重なっている、案に改行がある・空、のときは採用しない。

use std::path::Path;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use similar::{Algorithm, DiffOp, capture_diff_slices_deadline};

use crate::index::build_index;
use crate::ir::Ir;
use crate::project::Project;
use crate::provenance::id_lines;
use crate::{ParseOptions, Span};

/// 外部のAIエージェントが提案を置くフォルダ。
pub const PROPOSALS_DIR: &str = "ai/proposals";

/// 行ごとの提案。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    /// 行ID（`^` を除く）。
    pub line: String,
    /// 今の文（行IDと字下げを除いた行の中身）。採用のときに今の行と照らす。
    pub before: Option<String>,
    /// 案。
    pub after: String,
    pub reason: Option<String>,
}

/// 応答の1つの JSON から提案を読む（`line` と `after` が要る）。
pub fn proposal_of(value: &Value) -> Option<Proposal> {
    let text = |keys: &[&str]| {
        keys.iter().find_map(|key| {
            value
                .get(*key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .filter(|s| !s.trim().is_empty())
        })
    };
    let line = text(&["line"])?.trim().trim_start_matches('^').to_owned();
    Some(Proposal {
        before: text(&["before", "current"]).map(|t| without_own_id(&t, &line)),
        after: without_own_id(&text(&["after", "proposal", "suggestion"])?, &line),
        reason: text(&["reason", "message"]),
        line,
    })
}

/// 値の末尾に、その行の行ID（` ^id`）が付いていれば除く（文脈で行IDつきの行を見せるので、LLM が付けやすい）。
fn without_own_id(text: &str, id: &str) -> String {
    let trimmed = text.trim_end_matches([' ', '\t']);
    match trimmed.strip_suffix(&format!("^{id}")) {
        Some(rest) if rest.ends_with([' ', '\t']) => rest.trim_end_matches([' ', '\t']).to_owned(),
        _ => text.to_owned(),
    }
}

/// `ai/proposals/` の1つのファイル。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProposalFile {
    /// プロジェクトの根からの相対パス。
    pub path: String,
    /// フロントマターの `task`（出自の記録に使う。無ければ `external:proposals`）。
    pub task: Option<String>,
    pub model: Option<String>,
    pub at: Option<String>,
    pub proposals: Vec<Proposal>,
    /// 読めなかったところ。
    pub problems: Vec<String>,
}

/// 値のまわりの半角空白と `` ` `` を除く（全角空白は段落頭の字下げ、`「」` は台詞の一部なので除かない）。
fn unquote(text: &str) -> String {
    let t = text.trim_matches([' ', '\t']);
    t.strip_prefix('`')
        .and_then(|r| r.strip_suffix('`'))
        .unwrap_or(t)
        .to_owned()
}

/// 箇条書きの1行（`- 今：…`）のラベルと値。
fn labeled(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim_start().strip_prefix(['-', '*'])?.trim_start();
    let (label, value) = rest.split_once('：').or_else(|| rest.split_once(':'))?;
    Some((label.trim(), value))
}

/// 読んでいる途中の提案（行ID、今、案、理由）。
#[derive(Default)]
struct Pending {
    line: String,
    before: Option<String>,
    after: Option<String>,
    reason: Option<String>,
}

/// `ai/proposals/*.md` を読む：フロントマター（`task`・`model`・`at`）と、`## ^行ID` の見出しごとの
/// `- 今：`・`- 案：`・`- 理由：`。
pub fn parse_proposal_file(path: &str, text: &str) -> ProposalFile {
    let text = text.trim_start_matches('\u{feff}');
    let mut file = ProposalFile {
        path: path.to_owned(),
        task: None,
        model: None,
        at: None,
        proposals: Vec::new(),
        problems: Vec::new(),
    };
    let mut body = text;
    if let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
        && let Some(end) = rest.find("\n---")
    {
        for line in rest.get(..end).unwrap_or_default().lines() {
            if let Some((key, value)) = line.split_once(':') {
                let value =
                    Some(value.trim().trim_matches('"').to_owned()).filter(|v| !v.is_empty());
                match key.trim() {
                    "task" => file.task = value,
                    "model" => file.model = value,
                    "at" => file.at = value,
                    _ => {}
                }
            }
        }
        body = rest
            .get(end + 4..)
            .map(|r| r.trim_start_matches(['\r', '\n']))
            .unwrap_or_default();
    }
    let mut current: Option<Pending> = None;
    let finish = |current: Option<Pending>, file: &mut ProposalFile| {
        let Some(p) = current else {
            return;
        };
        match p.after {
            Some(after) => file.proposals.push(Proposal {
                line: p.line,
                before: p.before,
                after,
                reason: p.reason,
            }),
            None => file.problems.push(format!(
                "## ^{} に「案」がありません。読み飛ばします",
                p.line
            )),
        }
    };
    for line in body.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            let id = heading.trim().trim_start_matches('^').to_owned();
            finish(current.take(), &mut file);
            if id.is_empty()
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            {
                file.problems.push(format!(
                    "見出し「{}」は行ID（## ^xxxx）ではありません。読み飛ばします",
                    heading.trim()
                ));
                continue;
            }
            current = Some(Pending {
                line: id,
                ..Pending::default()
            });
            continue;
        }
        let (Some(slot), Some((label, value))) = (current.as_mut(), labeled(line)) else {
            continue;
        };
        let value = Some(unquote(value))
            .filter(|v| !v.is_empty())
            .map(|v| without_own_id(&v, &slot.line));
        match label {
            "今" | "今の文" | "before" => slot.before = value,
            "案" | "after" => slot.after = value,
            "理由" | "reason" => slot.reason = value,
            _ => {}
        }
    }
    finish(current, &mut file);
    file
}

/// `ai/proposals/` の `*.md`（名前の順）。読めないファイルは知らせに入れて続ける。
pub fn read_proposal_files(root: &Path) -> Vec<ProposalFile> {
    let dir = root.join(PROPOSALS_DIR);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "md"))
        .collect();
    paths.sort();
    paths
        .iter()
        .filter_map(|p| {
            let name = p.file_name()?.to_string_lossy().into_owned();
            let rel = format!("{PROPOSALS_DIR}/{name}");
            Some(match std::fs::read(p).map(String::from_utf8) {
                Ok(Ok(text)) => parse_proposal_file(&rel, &text),
                Ok(Err(_)) => ProposalFile {
                    path: rel,
                    task: None,
                    model: None,
                    at: None,
                    proposals: Vec::new(),
                    problems: vec!["UTF-8で書かれていません".to_owned()],
                },
                Err(e) => ProposalFile {
                    path: rel,
                    task: None,
                    model: None,
                    at: None,
                    proposals: Vec::new(),
                    problems: vec![format!("読めません：{e}")],
                },
            })
        })
        .collect()
}

/// 本文の1行を、字下げ（行頭の半角空白とタブ）・中身・行IDの後ろ（中身の後ろの空白、行ID、行末）に分ける。
/// 行末が ` ^id`（行IDの前に半角空白。仕様書9節）でなければ `None`。
fn split_id_line<'a>(line: &'a str, id: &str) -> Option<(&'a str, &'a str, &'a str)> {
    let body = line.strip_suffix('\n').unwrap_or(line);
    let body = body.strip_suffix('\r').unwrap_or(body);
    let trimmed = body.trim_end_matches([' ', '\t']);
    let before_id = trimmed.strip_suffix(&format!("^{id}"))?;
    if !before_id.ends_with(' ') {
        return None;
    }
    // 中身の後ろの空白は、すべて行IDの側に残す（採用で行のほかのバイトを変えない）。
    let content_end = before_id.trim_end_matches([' ', '\t']).len();
    let indent_end = body.len() - body.trim_start_matches([' ', '\t']).len();
    let indent = line.get(..indent_end)?;
    let content = line.get(indent_end..content_end)?;
    let tail = line.get(content_end..)?;
    Some((indent, content, tail))
}

/// 本文の中の、行ID `id` の行（何行目か、字下げ、中身）。コメントの行と `@raw` の中は数えない（行IDを
/// 切り出さないため。仕様書4節）。無ければ空、重なっていれば2つ以上。
fn find_id_lines<'a>(text: &'a str, id: &str) -> Vec<(usize, &'a str, &'a str)> {
    let mut raw = false;
    let mut out = Vec::new();
    for (n, line) in text.split_inclusive('\n').enumerate() {
        let head = line.trim_start_matches([' ', '\t']).trim_end();
        if raw {
            // `@end` か、字下げの無い見出しで生コードの範囲が閉じる（仕様書15節。閉じ忘れの E103）。
            if head == "@end" {
                raw = false;
                continue;
            }
            if !line.starts_with("## ") {
                continue;
            }
            raw = false;
        }
        if head.starts_with("@raw") {
            raw = true;
            continue;
        }
        if head.starts_with("//") {
            continue;
        }
        if let Some((indent, content, _)) = split_id_line(line, id) {
            out.push((n, indent, content));
        }
    }
    out
}

/// 前後の空白（全角空白も）を除いて同じか（提案の「今」と今の行を照らす）。
fn same(a: &str, b: &str) -> bool {
    a.trim() == b.trim()
}

/// 当てる案：前後の半角空白を除き、今の行が段落頭の全角空白で始まっていて案に無ければ、それを残す
/// （段落頭の字下げ。AI が落としやすい）。
fn effective_after(current: &str, after: &str) -> String {
    let after = after.trim_matches([' ', '\t']);
    let lead: String = current.chars().take_while(|c| *c == '\u{3000}').collect();
    if !lead.is_empty() && !after.starts_with('\u{3000}') {
        format!("{lead}{after}")
    } else {
        after.to_owned()
    }
}

/// 行の種類（採用で変えない）。台詞と地の文は、設定DBが無いと分けられないので同じ種類にする。
fn line_kind(content: &str) -> &'static str {
    let t = content.trim_start_matches([' ', '\t']);
    if t.starts_with("//") {
        "comment"
    } else if t.starts_with('#') {
        "heading"
    } else if t.starts_with("->") {
        "jump"
    } else if t.starts_with('@') {
        "directive"
    } else if t.starts_with("- ") || t == "-" {
        "option"
    } else if t == "---" {
        "meta"
    } else {
        "text"
    }
}

/// 採用できるか。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProposalStatus {
    /// 採用できる。
    Ready,
    /// 今の行がもう案と同じ（採用済み）。
    Same,
    /// 採用できない（理由）。
    Blocked { reason: String },
}

/// 文字ごとの差分の一続き。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffPart {
    /// `equal`・`delete`・`insert`。
    pub kind: &'static str,
    pub text: String,
}

/// 文字ごとの差分（今の文から案へ）。
pub fn char_diff(before: &str, after: &str) -> Vec<DiffPart> {
    let old: Vec<char> = before.chars().collect();
    let new: Vec<char> = after.chars().collect();
    let deadline = Instant::now().checked_add(Duration::from_millis(200));
    let mut out: Vec<DiffPart> = Vec::new();
    let mut push = |kind: &'static str, chars: &[char]| {
        if chars.is_empty() {
            return;
        }
        let text: String = chars.iter().collect();
        match out.last_mut() {
            Some(last) if last.kind == kind => last.text.push_str(&text),
            _ => out.push(DiffPart { kind, text }),
        }
    };
    for op in capture_diff_slices_deadline(Algorithm::Myers, &old, &new, deadline) {
        match op {
            DiffOp::Equal { old_index, len, .. } => {
                push(
                    "equal",
                    old.get(old_index..old_index + len).unwrap_or_default(),
                );
            }
            DiffOp::Delete {
                old_index, old_len, ..
            } => push(
                "delete",
                old.get(old_index..old_index + old_len).unwrap_or_default(),
            ),
            DiffOp::Insert {
                new_index, new_len, ..
            } => push(
                "insert",
                new.get(new_index..new_index + new_len).unwrap_or_default(),
            ),
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                push(
                    "delete",
                    old.get(old_index..old_index + old_len).unwrap_or_default(),
                );
                push(
                    "insert",
                    new.get(new_index..new_index + new_len).unwrap_or_default(),
                );
            }
        }
    }
    out
}

/// 提案の見せ方（今の本文と照らしたもの）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProposalView {
    pub proposal: Proposal,
    /// 行IDの行のファイル・シーン・位置。
    pub file: Option<String>,
    pub scene: Option<String>,
    pub span: Option<Span>,
    /// 今の行の中身（字下げと行IDを除く）。
    pub current: Option<String>,
    pub status: ProposalStatus,
    /// 今の文（無ければ提案の「今」）から案への、文字ごとの差分。
    pub diff: Vec<DiffPart>,
}

/// 案そのものの誤り（採用できない理由）。
fn after_problem(proposal: &Proposal, current: &str) -> Option<String> {
    let after = proposal.after.trim_matches([' ', '\t']);
    if after.trim().is_empty() {
        return Some("案が空です（行を消す提案は採用しません）".to_owned());
    }
    if after.contains(['\n', '\r']) {
        return Some("案に改行があります（行を分ける提案は採用しません）".to_owned());
    }
    if line_kind(after) != line_kind(current) {
        return Some(
            "行の種類が変わる案です（コメント・見出し・ジャンプ・命令・選択肢との入れ替えは採用しません）"
                .to_owned(),
        );
    }
    if after.contains(" ^")
        && after.trim_end().rsplit(" ^").next().is_some_and(|tail| {
            !tail.is_empty()
                && tail
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
    {
        return Some("案に行IDが入っています".to_owned());
    }
    None
}

/// 本文の中で、提案が採用できるか。
fn status_in(text: &str, proposal: &Proposal) -> (Option<String>, ProposalStatus) {
    let found = find_id_lines(text, &proposal.line);
    let blocked = |reason: String| ProposalStatus::Blocked { reason };
    match found.as_slice() {
        [] => (
            None,
            blocked(format!("本文に行ID ^{} がありません", proposal.line)),
        ),
        [(_, _, content)] => {
            let current = (*content).to_owned();
            if current == effective_after(&current, &proposal.after) {
                return (Some(current), ProposalStatus::Same);
            }
            if let Some(reason) = after_problem(proposal, &current) {
                return (Some(current), blocked(reason));
            }
            match &proposal.before {
                None => (
                    Some(current),
                    blocked(
                        "提案に「今」の文がありません（今の行と照らせないので採用しません）"
                            .to_owned(),
                    ),
                ),
                Some(before) if !same(before, &current) => (
                    Some(current),
                    blocked("本文が変わっています（提案の「今」と今の行が違います）".to_owned()),
                ),
                Some(_) => (Some(current), ProposalStatus::Ready),
            }
        }
        _ => (
            None,
            blocked(format!("行ID ^{} が本文で重なっています", proposal.line)),
        ),
    }
}

/// 本文全体で見て採用できない行ID（行IDの重なり（E301）、複数行の台詞）と理由。
pub fn project_blocks(project: &Project, ir: &Ir, ids: &[&str]) -> Vec<(String, String)> {
    let index = build_index(project, ir);
    let mut out = Vec::new();
    for id in ids {
        let found: Vec<_> = index.line_ids.iter().filter(|l| l.id == *id).collect();
        if found.len() > 1 {
            out.push((
                (*id).to_owned(),
                format!("行ID ^{id} が本文で重なっています（E301）"),
            ));
            continue;
        }
        let multi = found.first().is_some_and(|l| {
            project
                .files
                .iter()
                .find(|f| f.path == l.location.file)
                .and_then(|f| f.text.get(l.location.span.start..l.location.span.end))
                .is_some_and(|t| t.trim_end().contains('\n'))
        });
        if multi {
            out.push((
                (*id).to_owned(),
                "複数行の台詞の提案は採用しません（行IDのある最後の行だけでは照らせないため）"
                    .to_owned(),
            ));
        }
    }
    out
}

/// 提案を今の本文と照らす（ファイル・シーン・今の文・採用できるか・差分）。
pub fn view_proposals(project: &Project, ir: &Ir, proposals: &[Proposal]) -> Vec<ProposalView> {
    let index = build_index(project, ir);
    let ids: Vec<&str> = proposals.iter().map(|p| p.line.as_str()).collect();
    let blocks = project_blocks(project, ir, &ids);
    let options = project.parse_options();
    proposals
        .iter()
        .map(|p| {
            let location = index
                .line_ids
                .iter()
                .find(|l| l.id == p.line)
                .map(|l| &l.location);
            let file_text = location
                .and_then(|l| project.files.iter().find(|f| f.path == l.file))
                .map(|f| f.text.as_str());
            let (current, mut status) = match file_text {
                Some(text) => status_in(text, p),
                None => (
                    None,
                    ProposalStatus::Blocked {
                        reason: format!("本文に行ID ^{} がありません", p.line),
                    },
                ),
            };
            if let Some((_, reason)) = blocks.iter().find(|(id, _)| *id == p.line) {
                status = ProposalStatus::Blocked {
                    reason: reason.clone(),
                };
            }
            // 当ててみて、行IDが効かなくなる案なら採用できない。
            if status == ProposalStatus::Ready
                && let Some(text) = file_text
                && let Some((_, reason)) =
                    apply_proposals(text, std::slice::from_ref(p), &[], &options)
                        .skipped
                        .into_iter()
                        .next()
            {
                status = ProposalStatus::Blocked { reason };
            }
            let from = current
                .as_deref()
                .or(p.before.as_deref())
                .unwrap_or_default();
            ProposalView {
                diff: char_diff(from, &effective_after(from, &p.after)),
                proposal: p.clone(),
                file: location.map(|l| l.file.clone()),
                scene: location.map(|l| l.scene.clone()),
                span: location.map(|l| l.span),
                current,
                status,
            }
        })
        .collect()
}

/// 採用の結果（本文に当てたもの）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Applied {
    pub text: String,
    /// 置き換えた行ID。
    pub applied: Vec<String>,
    /// 採用しなかった行IDと理由。
    pub skipped: Vec<(String, String)>,
}

/// 章の有効な行ID（行IDを持てる行の、形の合う行ID）の並びと、構文の誤り（E）と警告（W）の数。
fn shape(text: &str, options: &ParseOptions) -> (Vec<String>, usize) {
    let mut ids: Vec<String> = id_lines(text, options)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    ids.sort();
    let problems = crate::parse(text, options)
        .diagnostics
        .iter()
        .filter(|d| d.severity() != crate::Severity::Info)
        .count();
    (ids, problems)
}

/// 採用する提案を本文に当てる。採用できる行（[`ProposalStatus::Ready`]）だけを、字下げ・段落頭の全角空白・
/// 行IDとその前後の空白を残して置き換える。`blocked` は本文全体で見て採用できない行ID（[`project_blocks`]）。
/// 当てた後の章を解析して、有効な行IDの集まりが変わる案（台詞の括弧を落とす、`@選択` をほかの命令にするなど、
/// 行IDが効かなくなる案）は採用しない。
pub fn apply_proposals(
    text: &str,
    adopted: &[Proposal],
    blocked: &[(String, String)],
    options: &ParseOptions,
) -> Applied {
    let mut lines: Vec<String> = text.split_inclusive('\n').map(str::to_owned).collect();
    let mut applied = Vec::new();
    let mut skipped = Vec::new();
    for proposal in adopted {
        if let Some((_, reason)) = blocked.iter().find(|(id, _)| *id == proposal.line) {
            skipped.push((proposal.line.clone(), reason.clone()));
            continue;
        }
        let current = lines.concat();
        match status_in(&current, proposal) {
            (_, ProposalStatus::Ready) => {}
            (_, ProposalStatus::Same) => {
                skipped.push((proposal.line.clone(), "もう案と同じ文です".to_owned()));
                continue;
            }
            (_, ProposalStatus::Blocked { reason }) => {
                skipped.push((proposal.line.clone(), reason));
                continue;
            }
        }
        let Some((n, _, _)) = find_id_lines(&current, &proposal.line).first().copied() else {
            continue;
        };
        let Some(line) = lines.get(n).cloned() else {
            continue;
        };
        let Some((indent, content, tail)) = split_id_line(&line, &proposal.line) else {
            continue;
        };
        let replaced = format!(
            "{indent}{}{tail}",
            effective_after(content, &proposal.after)
        );
        let mut trial = lines.clone();
        if let Some(slot) = trial.get_mut(n) {
            *slot = replaced;
        }
        let (before_ids, before_problems) = shape(&current, options);
        let (after_ids, after_problems) = shape(&trial.concat(), options);
        if after_ids != before_ids {
            skipped.push((
                proposal.line.clone(),
                "採用すると行IDが効かなくなります（台詞の括弧や命令の種類が変わる案）".to_owned(),
            ));
            continue;
        }
        if after_problems > before_problems {
            skipped.push((
                proposal.line.clone(),
                "採用すると本文の構文に誤りが出ます（閉じていない括弧など）".to_owned(),
            ));
            continue;
        }
        lines = trial;
        applied.push(proposal.line.clone());
    }
    Applied {
        text: lines.concat(),
        applied,
        skipped,
    }
}
