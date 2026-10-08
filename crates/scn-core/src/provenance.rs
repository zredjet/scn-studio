//! 出自の記録（T3-4、D065）：`ai/provenance.jsonl`（AIが書いた行の出自。1行に1つのJSON）を読み、書き足し、
//! 行IDごとの今の出自（いちばん後ろの記録）を求める（`docs/spec/project-files.md` の9節、`docs/design.md` の
//! 「出力の扱いと出自の記録」）。
//!
//! - 書き足すときは、今の中身の後ろに行を足すだけで、ほかの行は1バイトも変えない（終わりに改行が無ければ足す。
//!   改行は今のファイルにそろえる）。
//! - 読めない行（JSON でない、`line`・`origin` が無い）は読み飛ばして、行の番号とわけを返す。
//! - 人が AI の行を直したかは、保存の前後の本文を、行IDごとの文字（台詞・地の文・選択肢の行の範囲）で比べて
//!   決める（アプリの保存。外部のエディタの変更は、誰が直したか分からないので数えない）。
//! - 記録のない行は人が書いた行とみなす（`human` の記録は書かなくてよい）。

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::ParseOptions;
use crate::ast::{LineIdSlot, Stmt};
use crate::diag::Span;
use crate::ir::Ir;
use crate::project::{Project, ProjectError, write_source};
use crate::script::csv_table;
use crate::syntax::parse;

/// 出自の記録の置き場所。
pub const PROVENANCE_FILE: &str = "ai/provenance.jsonl";

/// AIが書いた。
pub const AI: &str = "ai";
/// AIが書いたものを人が直した。
pub const AI_EDITED: &str = "ai-edited";
/// 人が書いた。
pub const HUMAN: &str = "human";

/// 記録の1行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// 行ID（`^` を除く）。
    pub line: String,
    /// `ai`・`ai-edited`・`human`。
    pub origin: String,
    /// AIタスクの名前か、`external:<エージェント名>`。
    #[serde(default)]
    pub task: String,
    #[serde(default)]
    pub model: String,
    /// 日時（ISO 8601、時差つき）。
    #[serde(default)]
    pub at: String,
}

/// 読んだ記録。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Provenance {
    pub records: Vec<Record>,
    /// 読めなかった行（1から数えた行の番号と、わけ）。
    pub problems: Vec<(usize, String)>,
}

impl Provenance {
    /// 行IDごとの今の出自（いちばん後ろの記録）。
    pub fn latest(&self) -> BTreeMap<&str, &Record> {
        let mut out = BTreeMap::new();
        for record in &self.records {
            out.insert(record.line.as_str(), record);
        }
        out
    }
}

/// 記録を読む（無ければ空）。
pub fn read_provenance(root: &Path) -> Result<Provenance, ProjectError> {
    let path = root.join(PROVENANCE_FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Provenance::default()),
        Err(source) => return Err(ProjectError::Io { path, source }),
    };
    Ok(parse_provenance(&text))
}

/// 記録の中身を読む。
pub fn parse_provenance(text: &str) -> Provenance {
    let mut out = Provenance::default();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Record>(line) {
            Ok(record) if !record.line.is_empty() && !record.origin.is_empty() => {
                out.records.push(record);
            }
            Ok(_) => out
                .problems
                .push((i + 1, "line と origin が要ります".to_owned())),
            Err(e) => out.problems.push((i + 1, e.to_string())),
        }
    }
    out
}

/// 記録を書き足す（今の中身の後ろに足すだけ）。`ai/` が無ければ作る。
pub fn append_provenance(root: &Path, records: &[Record]) -> Result<(), ProjectError> {
    if records.is_empty() {
        return Ok(());
    }
    let path = root.join(PROVENANCE_FILE);
    let current = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(source) => return Err(ProjectError::Io { path, source }),
    };
    let nl = if current.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut text = current;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push_str(nl);
    }
    for record in records {
        let line =
            serde_json::to_string(record).map_err(|e| ProjectError::Refused(e.to_string()))?;
        text.push_str(&line);
        text.push_str(nl);
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|source| ProjectError::Write {
            path: dir.to_owned(),
            source,
        })?;
    }
    write_source(root, PROVENANCE_FILE, &text)
}

/// 1970-01-01T00:00:00Z からの秒を、ISO 8601 の日時（協定世界時、`Z`）にする。
pub fn iso8601_utc(secs: u64) -> String {
    let days = secs / 86_400;
    let rest = secs % 86_400;
    // 日数から年月日（Howard Hinnant の civil_from_days）。
    let z = i64::try_from(days).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        (rest % 3600) / 60,
        rest % 60
    )
}

/// 今の日時（ISO 8601、協定世界時）。
pub fn now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    iso8601_utc(secs)
}

/// 本文の、行IDのある行（行IDと、台詞・地の文・`@選択` の行・選択肢の行の範囲）。文書の順。
pub fn id_lines(text: &str, options: &ParseOptions) -> Vec<(String, Span)> {
    fn put(out: &mut Vec<(String, Span)>, slot: &LineIdSlot, span: Span) {
        if let LineIdSlot::Valid(id) = slot {
            out.push((id.value.clone(), span));
        }
    }
    fn walk(stmts: &[Stmt], out: &mut Vec<(String, Span)>) {
        for stmt in stmts {
            match stmt {
                Stmt::Dialogue(d) => put(out, &d.line_id, d.span),
                Stmt::Narration(n) => put(out, &n.line_id, n.span),
                Stmt::Choice(c) => {
                    put(out, &c.line_id, c.span);
                    for option in &c.options {
                        put(out, &option.line_id, option.span);
                        walk(&option.body, out);
                    }
                }
                Stmt::If(block) => {
                    for branch in &block.branches {
                        walk(&branch.body, out);
                    }
                    if let Some(e) = &block.else_branch {
                        walk(&e.body, out);
                    }
                }
                Stmt::Break(_) | Stmt::Directive(_) | Stmt::Jump(_) | Stmt::Raw(_) => {}
            }
        }
    }
    let parsed = parse(text, options);
    let mut out = Vec::new();
    for scene in &parsed.chapter.scenes {
        walk(&scene.body, &mut out);
    }
    out
}

/// 比べるための行の文字：行ごとに、行頭と行末の半角空白・タブ（字下げ、整形用）を除き、行IDを除く。
fn comparable(raw: &str, id: &str) -> String {
    let suffix = format!("^{id}");
    raw.lines()
        .map(|line| {
            let line = line.trim_matches([' ', '\t', '\r']);
            line.strip_suffix(suffix.as_str())
                .unwrap_or(line)
                .trim_end_matches([' ', '\t'])
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 本文の、行IDごとの文字（台詞・地の文・`@選択` の行・選択肢の行の範囲。比べるために、字下げと行IDを除く）。
pub fn line_texts(text: &str, options: &ParseOptions) -> BTreeMap<String, String> {
    id_lines(text, options)
        .into_iter()
        .map(|(id, span)| {
            let line = comparable(text.get(span.start..span.end).unwrap_or_default(), &id);
            (id, line)
        })
        .collect()
}

// ---- AI の行の一覧（`scn provenance`、アプリの「出力」） ----

/// AI の行の一覧の1行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProvenanceRow {
    pub id: String,
    pub file: String,
    pub span: Span,
    /// シーンID（と題名）。
    pub scene: String,
    pub origin: String,
    pub task: String,
    pub model: String,
    pub at: String,
    /// 行の文字（行IDを除く。字下げの半角空白とタブも除く）。
    pub text: String,
}

/// 一覧の数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct ProvenanceCounts {
    /// 本文の、行IDのある行。
    pub lines: usize,
    pub ai: usize,
    pub ai_edited: usize,
}

/// 本文にある行のうち、今の出自が `ai` か `ai-edited` の行（ファイルとその中の順）。
pub fn provenance_rows(
    project: &Project,
    ir: &Ir,
    provenance: &Provenance,
) -> (Vec<ProvenanceRow>, ProvenanceCounts) {
    let latest = provenance.latest();
    let options = project.parse_options();
    let mut rows = Vec::new();
    let mut counts = ProvenanceCounts::default();
    for file in &project.files {
        for (id, span) in id_lines(&file.text, &options) {
            counts.lines += 1;
            let Some(record) = latest.get(id.as_str()) else {
                continue;
            };
            match record.origin.as_str() {
                AI => counts.ai += 1,
                AI_EDITED => counts.ai_edited += 1,
                _ => continue,
            }
            let scene = ir
                .scenes
                .iter()
                .filter(|s| s.file == file.path && s.span.start <= span.start)
                .max_by_key(|s| s.span.start)
                .map(|s| {
                    if s.title.is_empty() {
                        s.id.clone()
                    } else {
                        format!("{} {}", s.id, s.title)
                    }
                })
                .unwrap_or_default();
            let raw = file.text.get(span.start..span.end).unwrap_or_default();
            let suffix = format!("^{id}");
            // 行IDと、行末の半角空白・タブを除く（全角空白は本文の一部なので残す）。
            let text = raw
                .trim_end_matches([' ', '\t', '\r', '\n'])
                .strip_suffix(suffix.as_str())
                .unwrap_or(raw)
                .trim_end_matches([' ', '\t', '\r', '\n'])
                .trim_start_matches([' ', '\t'])
                .to_owned();
            rows.push(ProvenanceRow {
                id,
                file: file.path.clone(),
                span,
                scene,
                origin: record.origin.clone(),
                task: record.task.clone(),
                model: record.model.clone(),
                at: record.at.clone(),
                text,
            });
        }
    }
    (rows, counts)
}

/// 一覧の列の見出し。
pub const PROVENANCE_COLUMNS: [&str; 8] = [
    "行ID",
    "ファイル",
    "シーン",
    "出自",
    "タスク",
    "モデル",
    "日時",
    "本文",
];

/// 一覧の CSV（UTF-8 の BOM 付き、CRLF。台本と同じ形）。
pub fn provenance_csv(rows: &[ProvenanceRow]) -> String {
    let table: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            vec![
                r.id.clone(),
                r.file.clone(),
                r.scene.clone(),
                r.origin.clone(),
                r.task.clone(),
                r.model.clone(),
                r.at.clone(),
                r.text.clone(),
            ]
        })
        .collect();
    csv_table(&PROVENANCE_COLUMNS, &table)
}

/// 一覧の既定の置き場所。
pub const PROVENANCE_EXPORT_FILE: &str = "export/provenance.csv";

/// 一覧を作品フォルダの `export/provenance.csv` に書く。
pub fn write_provenance_csv(root: &Path, rows: &[ProvenanceRow]) -> Result<PathBuf, ProjectError> {
    let path = root.join(PROVENANCE_EXPORT_FILE);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|source| ProjectError::Write {
            path: dir.to_owned(),
            source,
        })?;
    }
    write_source(root, PROVENANCE_EXPORT_FILE, &provenance_csv(rows))?;
    Ok(path)
}

/// 保存の前後で文字が変わった行のうち、今の出自が `ai` の行IDの記録（`ai-edited`。タスクとモデルは前の記録から）。
///
/// - `before` は人が直し始めた元の本文（読んだか、保存したか）、`external` は今のディスクの中身（外で変わって
///   いて、自分の変更を残したときは外の中身）、`after` は保存する本文。元とも外の中身とも違う行だけを、人が
///   直した行に数える（外で変わった行を、人が直したと数えない）。字下げと行IDの前後の空白の違いは数えない。
/// - 行をつないだとき：元にあった AI の行（`ai`・`ai-edited`）が無くなり、元でそのすぐ前にあった行が残って
///   文字が変わり、無くなった行の文字（字下げを除く）を含めば、つないだとみなし、残った行が人の行でも
///   `ai-edited` にする（AI の文を含むため）。
pub fn edited_ai_lines(
    before: &str,
    external: Option<&str>,
    after: &str,
    options: &ParseOptions,
    provenance: &Provenance,
    at: &str,
) -> Vec<Record> {
    let order: Vec<String> = id_lines(before, options)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let before = line_texts(before, options);
    let external = external.map(|t| line_texts(t, options)).unwrap_or_default();
    let after = line_texts(after, options);
    let latest = provenance.latest();
    let edited = |id: &str| -> bool {
        match (before.get(id), after.get(id)) {
            (Some(old), Some(new)) => old != new && external.get(id) != Some(new),
            _ => false,
        }
    };
    let is_ai = |id: &str| {
        latest
            .get(id)
            .is_some_and(|r| r.origin == AI || r.origin == AI_EDITED)
    };
    let mut out: Vec<Record> = Vec::new();
    let mut push = |id: &str, from: Option<&Record>| {
        if out.iter().any(|r| r.line == id) {
            return;
        }
        let (task, model) = from.map_or((String::new(), String::new()), |r| {
            (r.task.clone(), r.model.clone())
        });
        out.push(Record {
            line: id.to_owned(),
            origin: AI_EDITED.to_owned(),
            task,
            model,
            at: at.to_owned(),
        });
    };
    for id in &order {
        let record = latest.get(id.as_str()).copied();
        // 直した AI の行。
        if edited(id) && record.is_some_and(|r| r.origin == AI) {
            push(id, record);
        }
    }
    for (i, id) in order.iter().enumerate() {
        // つないだ行：無くなった AI の行の、すぐ前の行が残って文字が変わり、無くなった行の文字を含む。
        if after.contains_key(id) || !is_ai(id) {
            continue;
        }
        let Some(prev) = i.checked_sub(1).and_then(|p| order.get(p)) else {
            continue;
        };
        let removed = before.get(id).map(|t| t.trim_matches([' ', '\t', '　']));
        let absorbed = match (removed, after.get(prev)) {
            (Some(removed), Some(now)) => !removed.is_empty() && now.contains(removed),
            _ => false,
        };
        let prev_record = latest.get(prev.as_str()).copied();
        if absorbed && edited(prev) && prev_record.is_none_or(|r| r.origin != AI_EDITED) {
            push(prev, latest.get(id.as_str()).copied());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(iso8601_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso8601_utc(1_791_417_600 + 3_723), "2026-10-08T01:02:03Z");
    }

    #[test]
    fn reads_and_skips() {
        let p = parse_provenance(
            "{\"line\":\"aaaa\",\"origin\":\"ai\",\"task\":\"t\",\"model\":\"m\",\"at\":\"x\"}\nnot json\n{\"line\":\"\",\"origin\":\"ai\"}\n\n{\"line\":\"aaaa\",\"origin\":\"ai-edited\"}\n",
        );
        assert_eq!(p.records.len(), 2);
        assert_eq!(
            p.problems.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            [2, 3]
        );
        assert_eq!(p.latest()["aaaa"].origin, "ai-edited");
    }

    #[test]
    fn edited_lines() {
        let options = ParseOptions::default();
        let before = "## a #s1\n灯「やあ」 ^aaaa\n　地の文。 ^aaab\n　人の行。 ^aaac\n";
        let after =
            "## a #s1\n灯「やあ、来たね」 ^aaaa\n　地の文。 ^aaab\n　人の行を直した。 ^aaac\n";
        let p = parse_provenance(
            "{\"line\":\"aaaa\",\"origin\":\"ai\",\"task\":\"t\",\"model\":\"m\",\"at\":\"x\"}\n{\"line\":\"aaab\",\"origin\":\"ai\"}\n",
        );
        let edited = edited_ai_lines(before, Some(before), after, &options, &p, "now");
        assert_eq!(edited.len(), 1);
        assert_eq!(edited[0].line, "aaaa");
        assert_eq!(edited[0].origin, "ai-edited");
        assert_eq!(
            (edited[0].task.as_str(), edited[0].at.as_str()),
            ("t", "now")
        );
    }
}
