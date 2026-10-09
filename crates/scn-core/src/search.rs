//! 作品全体の検索の材料（T4-22、N1）：章の中の本文の範囲（「本文だけ」で探すとき、一致をこの範囲に絞る）と、
//! 行IDの範囲（執筆表示では行IDを隠すので、その中の一致を除く）。探すこと自体はアプリが行う（保存していない本文も、
//! 検索の欄と同じ問い合わせで探すため）。

use std::collections::BTreeMap;

use serde::Serialize;

use crate::ast::{Chapter, Dialogue, LineIdSlot, Stmt};
use crate::diag::{Code, Diagnostic};
use crate::inline::{Inline, Text};
use crate::project::{Character, Project};
use crate::sema::{Resolution, resolve_character_in};
use crate::syntax::parse;

/// 章の検索の範囲（バイト位置、前から順）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SearchRanges {
    /// 本文（台詞と地の文）の範囲。重ならない。
    pub text: Vec<(usize, usize)>,
    /// 行IDの範囲（`^` から終わりまで。行IDを持てない行に付いたもの（W303）も）。
    pub line_ids: Vec<(usize, usize)>,
}

/// 作品のすべての章の検索の範囲（章のファイルごと。保存していない本文も）。
pub fn story_search_ranges(project: &Project) -> BTreeMap<String, SearchRanges> {
    let options = project.parse_options();
    project
        .files
        .iter()
        .map(|f| {
            let parsed = parse(&f.text, &options);
            (
                f.path.clone(),
                search_ranges(
                    &f.text,
                    &parsed.chapter,
                    &parsed.diagnostics,
                    &project.characters,
                ),
            )
        })
        .collect()
}

/// 章の検索の範囲。本文は、台詞の括弧の中と地の文（仕様書6節：閉じ括弧の後ろに文が続く行と、話者を解決できない
/// 行は、話者から行の終わりまでを地の文として読む）。話者の名前（台詞として読む行）・行ID・演出・ジャンプ・選択肢の
/// 表示文と問いかけ文・メタ情報・コメント・文中の命令（`{w}` など）は入れない（R222）。
pub fn search_ranges(
    source: &str,
    chapter: &Chapter,
    diagnostics: &[Diagnostic],
    characters: &[Character],
) -> SearchRanges {
    let mut walker = Walker {
        source,
        characters,
        id_candidates: diagnostics
            .iter()
            .filter(|d| matches!(d.code, Code::W303 | Code::E304))
            .map(|d| (d.span.start, d.span.end))
            .collect(),
        text: Vec::new(),
        line_ids: Vec::new(),
    };
    for scene in &chapter.scenes {
        walker.stmts(&scene.body);
    }
    walker.line_ids.extend(
        diagnostics
            .iter()
            .filter(|d| d.code == Code::W303)
            .map(|d| (d.span.start, d.span.end)),
    );
    let mut text = walker.text;
    text.sort_unstable();
    // 隣り合う範囲（文字とルビの間など）はまとめる。
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in text {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    let mut line_ids = walker.line_ids;
    line_ids.sort_unstable();
    line_ids.dedup();
    SearchRanges {
        text: merged,
        line_ids,
    }
}

struct Walker<'a> {
    source: &'a str,
    characters: &'a [Character],
    /// 行IDを持てない行の行ID（W303）と、形の合わない行IDの候補（E304）。どちらも本文に入れない（仕様書9節）。
    id_candidates: Vec<(usize, usize)>,
    text: Vec<(usize, usize)>,
    line_ids: Vec<(usize, usize)>,
}

impl Walker<'_> {
    fn stmts(&mut self, body: &[Stmt]) {
        for stmt in body {
            match stmt {
                Stmt::Dialogue(d) => {
                    self.line_id(&d.line_id);
                    self.dialogue(d);
                }
                Stmt::Narration(n) => {
                    self.line_id(&n.line_id);
                    self.parts(&n.text);
                }
                Stmt::Choice(c) => {
                    self.line_id(&c.line_id);
                    for option in &c.options {
                        self.line_id(&option.line_id);
                        self.stmts(&option.body);
                    }
                }
                Stmt::If(block) => {
                    for branch in &block.branches {
                        self.stmts(&branch.body);
                    }
                    if let Some(e) = &block.else_branch {
                        self.stmts(&e.body);
                    }
                }
                Stmt::Break(_) | Stmt::Directive(_) | Stmt::Jump(_) | Stmt::Raw(_) => {}
            }
        }
    }

    fn line_id(&mut self, slot: &LineIdSlot) {
        if let LineIdSlot::Valid(id) = slot {
            self.line_ids.push((id.span.start, id.span.end));
        }
    }

    /// 台詞の形の行。台詞として読む行は括弧の中だけ、地の文として読む行は話者から行の終わり（行IDの前）まで
    /// （途中のコメントの行、文中の命令、形の合わない行IDの候補は除く）。
    fn dialogue(&mut self, d: &Dialogue) {
        let resolved = matches!(
            resolve_character_in(self.characters, &d.speaker.value),
            Resolution::Found(_)
        );
        if resolved && !d.has_trailing_text() {
            for line in &d.lines {
                self.parts(line);
            }
            return;
        }
        let end = match &d.line_id {
            LineIdSlot::Valid(id) => id.span.start,
            LineIdSlot::Absent | LineIdSlot::Malformed => d.span.end,
        };
        // 除くもの：文中の命令と、途中の行を含む行IDの候補（W303・E304）。
        let mut holes: Vec<(usize, usize)> = d
            .lines
            .iter()
            .chain(&d.trailing)
            .flat_map(|t| &t.parts)
            .filter_map(command_span)
            .chain(
                self.id_candidates
                    .iter()
                    .copied()
                    .filter(|&(s, e)| d.span.start <= s && e <= d.span.end),
            )
            .collect();
        holes.sort_unstable();
        for range in self.whole_lines(d.span.start, end) {
            for (from, to) in without(range, &holes) {
                // 候補の前の空白などは除く（行ごとの範囲と同じ）。
                if let Some(piece) = self.source.get(from..to) {
                    let lead = piece.len() - piece.trim_start_matches([' ', '\t']).len();
                    let kept = piece.trim_matches([' ', '\t']).len();
                    if kept > 0 {
                        self.text.push((from + lead, from + lead + kept));
                    }
                }
            }
        }
    }

    /// `start` から `end` までの行ごとの範囲（行頭と行末の半角空白とタブ、改行、コメントの行は除く）。
    fn whole_lines(&self, start: usize, end: usize) -> Vec<(usize, usize)> {
        let Some(slice) = self.source.get(start..end) else {
            return Vec::new();
        };
        let mut lines = Vec::new();
        let mut at = start;
        for line in slice.split('\n') {
            let line_start = at;
            at += line.len() + 1;
            let content = line.trim_end_matches('\r');
            let leading = content.len() - content.trim_start_matches([' ', '\t']).len();
            let trimmed = content.trim_matches([' ', '\t']);
            if !trimmed.is_empty() && !trimmed.starts_with("//") {
                let from = line_start + leading;
                lines.push((from, from + trimmed.len()));
            }
        }
        lines
    }

    fn parts(&mut self, text: &Text) {
        for part in &text.parts {
            match part {
                Inline::Text { span, .. }
                | Inline::Ruby { span, .. }
                | Inline::Emphasis { span, .. } => {
                    if span.start < span.end {
                        self.text.push((span.start, span.end));
                    }
                }
                Inline::Wait { .. }
                | Inline::PageBreak { .. }
                | Inline::Se { .. }
                | Inline::Speed { .. }
                | Inline::Interpolation { .. } => {}
            }
        }
    }
}

/// 文中の命令（`{w}`・`{p}`・`{se}`・`{speed}`・`{=式}`）の範囲。
fn command_span(part: &Inline) -> Option<(usize, usize)> {
    match part {
        Inline::Wait { span, .. }
        | Inline::PageBreak { span }
        | Inline::Se { span, .. }
        | Inline::Speed { span, .. }
        | Inline::Interpolation { span, .. } => Some((span.start, span.end)),
        Inline::Text { .. } | Inline::Ruby { .. } | Inline::Emphasis { .. } => None,
    }
}

/// `range` から `holes`（前から順）を除いた残り（空のものは除く）。
fn without(range: (usize, usize), holes: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut rest = Vec::new();
    let mut from = range.0;
    for &(start, end) in holes {
        if end <= from || start >= range.1 {
            continue;
        }
        if start > from {
            rest.push((from, start));
        }
        from = from.max(end);
    }
    if from < range.1 {
        rest.push((from, range.1));
    }
    rest
}
