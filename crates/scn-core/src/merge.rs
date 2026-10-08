//! 外部の変更とのマージ（T2-7、D058）：3方向のマージ（元＝読み込んだときの中身、自分、外）。
//!
//! - 行を対応させる：`.scn` では、行IDのある行は行IDで、無い行は中身で（[`merge_scn`]）。設定ファイルのテキストは
//!   中身だけで（[`merge_text`]）。対応は Myers の差分（`similar`）で求める。行IDで対応した行は、中身が変わって
//!   いても同じ行として扱う（直した行と、隣の行の変更がぶつからない）。
//! - 元から見た自分の変更と外の変更（元の行の範囲を、新しい行に置き換える）を並べ、元の範囲が重なるもの（と、
//!   同じところへの行の追加どうし）を「ぶつかり」にする。隣り合うだけの変更は両方を取り込む。両方が同じに
//!   変えたものは、ぶつからない。
//! - ファイルの終わりの改行の有無は、行と別に3方向で決める（終わりの行の後ろに行を足しても、ぶつからない）。
//! - ぶつかりは、選ぶまで両方を残す：`.scn` では自分の行を残し、外の行は `// ` を付けたコメントにして印で囲む
//!   （行IDが2つにならず、解析も壊れない）。設定ファイルのテキストは `<<<<<<<` の印で囲む。
//! - 片方が行IDのある行を動かし（消して別のところに足し）、もう片方がその行を直していれば、動かした先に直した
//!   中身を置く（元の位置ではぶつけない）。それでも結果に、自分にも外にも無いほど同じ行IDが並べば（両方が動かした、
//!   動かして直したなど）、後のものをコメントにする。行IDが2つにならないため（設計の原則5）。
//! - 行IDは書き換えない（設計の原則5）。結果の行は、元・自分・外のどれかの行そのもの（と印のコメント）。

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use similar::{Algorithm, DiffOp, capture_diff_slices_deadline};

use crate::line::{Piece, RawLineId, is_indent, split_line_id};

/// ぶつかりの選び方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    /// 自分の変更。
    Mine,
    /// 外の変更。
    Theirs,
    /// 両方（自分の行を残し、外の行は印を付けて残す）。
    Both,
}

/// マージの結果の一続き。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Hunk {
    /// まとまった部分（変わらない行と、片方か両方の変更を取り込んだ行）。
    Merged { text: String },
    /// ぶつかり：元・自分・外のその部分。
    Conflict {
        base: String,
        mine: String,
        theirs: String,
    },
}

/// 差分を求める時間の上限（大きく書き換わった長い章で、待たせすぎない。超えたら最短でない差分になるだけ）。
const DIFF_DEADLINE: Duration = Duration::from_secs(2);

/// マージの結果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Merge {
    pub hunks: Vec<Hunk>,
    /// ぶつかりの数。
    pub conflicts: usize,
    /// 入力（元・自分・外）の見分け。画面に見せたマージと、当てるときのマージが同じ入力かを照らす。
    pub token: String,
    /// 行IDごとの、結果に並べてよい数（自分と外の多いほう）。超えたものはコメントにする。
    #[serde(skip)]
    id_limits: BTreeMap<String, usize>,
    /// 終わりの改行を取り除くか（マージした結果のファイルが、終わりに改行の無い形か）。
    #[serde(skip)]
    strip_final_newline: bool,
    /// 印のコメントに使う改行。
    #[serde(skip)]
    newline: &'static str,
    /// `.scn` か（両方を選んだときの印が、コメントか `<<<<<<<` か）。
    #[serde(skip)]
    scn: bool,
}

/// 1行（改行を含む中身と、改行を除いた中身）。終わりの改行の無い最後の行には、改行を補ってある。
#[derive(Debug, Clone)]
struct Line<'a> {
    text: std::borrow::Cow<'a, str>,
    body: &'a str,
}

/// 行の対応に使う見分け。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Key<'a> {
    Id(String),
    Text(&'a str),
}

/// テキストを行に分ける。終わりの改行が無ければ補い、無かったことを返す。
fn split(text: &str) -> (Vec<Line<'_>>, bool, &'static str) {
    let newline = if text
        .find('\n')
        .is_some_and(|at| at > 0 && text.as_bytes().get(at - 1) == Some(&b'\r'))
    {
        "\r\n"
    } else {
        "\n"
    };
    let mut lines = Vec::new();
    let mut missing = false;
    for segment in text.split_inclusive('\n') {
        let body = segment.strip_suffix('\n').unwrap_or(segment);
        let body = body.strip_suffix('\r').unwrap_or(body);
        if segment.ends_with('\n') {
            lines.push(Line {
                text: segment.into(),
                body,
            });
        } else {
            missing = true;
            lines.push(Line {
                text: format!("{segment}{newline}").into(),
                body,
            });
        }
    }
    (lines, missing, newline)
}

/// 行の見分け：`ids` なら、コメントでない行の行ID（形の正しいもの）。無ければ改行を除いた中身。
fn key<'a>(line: &Line<'a>, ids: bool) -> Key<'a> {
    if ids && !line.body.trim_start_matches(is_indent).starts_with("//") {
        let trimmed = line.body.trim_end_matches(is_indent);
        if let (_, RawLineId::Valid(id)) = split_line_id(Piece::new(trimmed, 0)) {
            return Key::Id(id.value);
        }
    }
    Key::Text(line.body)
}

/// 自分か外の、ある行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LineRef {
    mine: bool,
    index: usize,
}

/// 元から見た片方の変更：元の行の範囲 `start..end` を、行 `lines` に置き換える。
#[derive(Debug, Clone)]
struct Change {
    start: usize,
    end: usize,
    lines: Vec<LineRef>,
    mine: bool,
}

/// 元と片方の差分：変更と、元の行ごとの、対応した片方の行。
struct SideDiff {
    changes: Vec<Change>,
    matched: Vec<Option<usize>>,
}

/// 元と片方の差分から、変更を作る。行IDで対応した行の中身が変わっていれば、その1行の変更にする。
fn changes(
    base: &[Line<'_>],
    base_keys: &[Key<'_>],
    side: &[Line<'_>],
    side_keys: &[Key<'_>],
    mine: bool,
) -> SideDiff {
    let at = |index: usize| LineRef { mine, index };
    let mut matched = vec![None; base.len()];
    let mut out = Vec::new();
    let mut pending: Option<Change> = None;
    let flush = |pending: &mut Option<Change>, out: &mut Vec<Change>| {
        if let Some(change) = pending.take() {
            out.push(change);
        }
    };
    let deadline = Instant::now().checked_add(DIFF_DEADLINE);
    for op in capture_diff_slices_deadline(Algorithm::Myers, base_keys, side_keys, deadline) {
        let (old_index, old_len, new_index, new_len) = match op {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => {
                flush(&mut pending, &mut out);
                for t in 0..len {
                    let (b, s) = (old_index + t, new_index + t);
                    if let Some(slot) = matched.get_mut(b) {
                        *slot = Some(s);
                    }
                    if base.get(b).map(|l| &l.text) != side.get(s).map(|l| &l.text) {
                        out.push(Change {
                            start: b,
                            end: b + 1,
                            lines: vec![at(s)],
                            mine,
                        });
                    }
                }
                continue;
            }
            DiffOp::Delete {
                old_index,
                old_len,
                new_index,
            } => (old_index, old_len, new_index, 0),
            DiffOp::Insert {
                old_index,
                new_index,
                new_len,
            } => (old_index, 0, new_index, new_len),
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => (old_index, old_len, new_index, new_len),
        };
        match &mut pending {
            Some(change) if change.end == old_index => {
                change.end = old_index + old_len;
                change
                    .lines
                    .extend((new_index..new_index + new_len).map(at));
            }
            _ => {
                flush(&mut pending, &mut out);
                pending = Some(Change {
                    start: old_index,
                    end: old_index + old_len,
                    lines: (new_index..new_index + new_len).map(at).collect(),
                    mine,
                });
            }
        }
    }
    flush(&mut pending, &mut out);
    SideDiff {
        changes: out,
        matched,
    }
}

/// 行の移動：`mover` が行IDのある行を（中身を変えずに）別のところへ動かし、`other` がその行を直していれば、
/// `other` の直した行を元の位置から外し、動かした先の行を `other` の直した行にする（元の位置ではぶつけない）。
/// `other_mine` は `other` が自分の側か。
fn follow_moves(
    base: &[Line<'_>],
    base_keys: &[Key<'_>],
    mover: (&mut SideDiff, &[Line<'_>], &[Key<'_>]),
    other: (&mut SideDiff, &[Line<'_>]),
    other_mine: bool,
) {
    let (mover, mover_lines, mover_keys) = mover;
    let (other, other_lines) = other;
    let kept: BTreeSet<usize> = mover.matched.iter().flatten().copied().collect();
    let mut used = BTreeSet::new();
    for (b, base_key) in base_keys.iter().enumerate() {
        if !matches!(base_key, Key::Id(_)) || mover.matched.get(b).copied().flatten().is_some() {
            continue;
        }
        // 動かした先（対応していない行のうち、同じ行IDのもの）。
        let Some(moved) = mover_keys
            .iter()
            .enumerate()
            .position(|(i, k)| k == base_key && !kept.contains(&i) && !used.contains(&i))
        else {
            continue;
        };
        let Some(other_index) = other.matched.get(b).copied().flatten() else {
            continue;
        };
        let (Some(base_line), Some(moved_line), Some(other_line)) = (
            base.get(b),
            mover_lines.get(moved),
            other_lines.get(other_index),
        ) else {
            continue;
        };
        if moved_line.text != base_line.text || other_line.text == base_line.text {
            continue;
        }
        used.insert(moved);
        let edited = LineRef {
            mine: other_mine,
            index: other_index,
        };
        other
            .changes
            .retain(|c| !(c.start == b && c.end == b + 1 && c.lines == [edited]));
        for change in &mut mover.changes {
            for line in &mut change.lines {
                if line.mine == !other_mine && line.index == moved {
                    *line = edited;
                }
            }
        }
    }
}

/// 行を順につなぐ（前が改行で終わっていなければ、改行を足す）。
fn push_text(out: &mut String, text: &str, newline: &str) {
    if text.is_empty() {
        return;
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str(newline);
    }
    out.push_str(text);
}

/// 結果を組み立てる。
struct Builder {
    hunks: Vec<Hunk>,
    conflicts: usize,
    newline: &'static str,
}

impl Builder {
    fn merged(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(Hunk::Merged { text: last }) = self.hunks.last_mut() {
            push_text(last, text, self.newline);
        } else {
            self.hunks.push(Hunk::Merged {
                text: text.to_owned(),
            });
        }
    }

    fn conflict(&mut self, base: &str, mine: &str, theirs: &str) {
        // 隣り合うぶつかりは1つにまとめる。
        if let Some(Hunk::Conflict {
            base: b,
            mine: m,
            theirs: t,
        }) = self.hunks.last_mut()
        {
            push_text(b, base, self.newline);
            push_text(m, mine, self.newline);
            push_text(t, theirs, self.newline);
            return;
        }
        self.conflicts += 1;
        self.hunks.push(Hunk::Conflict {
            base: base.to_owned(),
            mine: mine.to_owned(),
            theirs: theirs.to_owned(),
        });
    }
}

/// 元の範囲 `start..end` に、片方の変更を当てた行。
fn version(
    base: &[Line<'_>],
    mine: &[Line<'_>],
    theirs: &[Line<'_>],
    start: usize,
    end: usize,
    changes: &[&Change],
) -> String {
    let mut out = String::new();
    let mut at = start;
    for change in changes {
        for line in base.get(at..change.start).unwrap_or_default() {
            out.push_str(&line.text);
        }
        for r in &change.lines {
            let side = if r.mine { mine } else { theirs };
            if let Some(line) = side.get(r.index) {
                out.push_str(&line.text);
            }
        }
        at = change.end;
    }
    for line in base.get(at..end).unwrap_or_default() {
        out.push_str(&line.text);
    }
    out
}

/// 改行を除いた中身の並び（改行の違いだけなら、同じとみなす）。
fn bodies(text: &str) -> Vec<&str> {
    text.lines().collect()
}

fn merge3(base: &str, mine: &str, theirs: &str, ids: bool) -> Merge {
    let (base_lines, base_missing, base_nl) = split(base);
    let (mine_lines, mine_missing, mine_nl) = split(mine);
    let (theirs_lines, theirs_missing, theirs_nl) = split(theirs);
    let newline = if mine.contains('\n') {
        mine_nl
    } else if theirs.contains('\n') {
        theirs_nl
    } else {
        base_nl
    };
    let base_keys: Vec<Key<'_>> = base_lines.iter().map(|l| key(l, ids)).collect();
    let mine_keys: Vec<Key<'_>> = mine_lines.iter().map(|l| key(l, ids)).collect();
    let theirs_keys: Vec<Key<'_>> = theirs_lines.iter().map(|l| key(l, ids)).collect();
    let mut mine_diff = changes(&base_lines, &base_keys, &mine_lines, &mine_keys, true);
    let mut theirs_diff = changes(&base_lines, &base_keys, &theirs_lines, &theirs_keys, false);
    if ids {
        follow_moves(
            &base_lines,
            &base_keys,
            (&mut theirs_diff, &theirs_lines, &theirs_keys),
            (&mut mine_diff, &mine_lines),
            true,
        );
        follow_moves(
            &base_lines,
            &base_keys,
            (&mut mine_diff, &mine_lines, &mine_keys),
            (&mut theirs_diff, &theirs_lines),
            false,
        );
    }
    let mut all = mine_diff.changes;
    all.extend(theirs_diff.changes);
    // 元の位置の順。同じところなら、行の追加（範囲の空のもの）を先に、自分を先に。
    all.sort_by_key(|c| (c.start, c.end, !c.mine));

    let mut builder = Builder {
        hunks: Vec::new(),
        conflicts: 0,
        newline,
    };
    let text_of = |lines: &[Line<'_>]| -> String { lines.iter().map(|l| &*l.text).collect() };
    let mut pos = 0;
    let mut i = 0;
    while let Some(first) = all.get(i) {
        let start = first.start;
        let mut end = first.end;
        let mut members: Vec<&Change> = vec![first];
        i += 1;
        while let Some(c) = all.get(i) {
            // 元の範囲が重なる、または同じところに行を足す。
            let insertion_clash = c.start == c.end
                && members
                    .iter()
                    .any(|m| m.start == m.end && m.start == c.start);
            if c.start < end || insertion_clash {
                end = end.max(c.end);
                members.push(c);
                i += 1;
            } else {
                break;
            }
        }
        builder.merged(&text_of(base_lines.get(pos..start).unwrap_or_default()));
        let mine_changes: Vec<&Change> = members.iter().copied().filter(|c| c.mine).collect();
        let theirs_changes: Vec<&Change> = members.iter().copied().filter(|c| !c.mine).collect();
        let mine_text = version(
            &base_lines,
            &mine_lines,
            &theirs_lines,
            start,
            end,
            &mine_changes,
        );
        let theirs_text = version(
            &base_lines,
            &mine_lines,
            &theirs_lines,
            start,
            end,
            &theirs_changes,
        );
        if theirs_changes.is_empty()
            || mine_text == theirs_text
            || (!mine_changes.is_empty() && bodies(&mine_text) == bodies(&theirs_text))
        {
            builder.merged(&mine_text);
        } else if mine_changes.is_empty() {
            builder.merged(&theirs_text);
        } else {
            let base_text = text_of(base_lines.get(start..end).unwrap_or_default());
            builder.conflict(&base_text, &mine_text, &theirs_text);
        }
        pos = end;
    }
    builder.merged(&text_of(base_lines.get(pos..).unwrap_or_default()));
    // 終わりの改行の有無：自分が変えていなければ外のもの、変えていれば自分のもの。
    let strip_final_newline = if mine_missing == base_missing {
        theirs_missing
    } else {
        mine_missing
    };
    // 結果に並べてよい行IDの数（自分と外の多いほう。元から重なっていたものは、そのまま残す）。
    let mut id_limits: BTreeMap<String, usize> = BTreeMap::new();
    if ids {
        for keys in [&mine_keys, &theirs_keys] {
            let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
            for k in keys.iter() {
                if let Key::Id(id) = k {
                    *counts.entry(id.as_str()).or_default() += 1;
                }
            }
            for (id, n) in counts {
                let limit = id_limits.entry(id.to_owned()).or_default();
                *limit = (*limit).max(n);
            }
        }
    }
    // 入力と、できたマージ（差分の時間の上限で粗い差分に切り替わると、同じ入力でも切れ目が変わりうる）。
    let mut hasher = DefaultHasher::new();
    (base, mine, theirs, ids, &builder.hunks).hash(&mut hasher);
    Merge {
        hunks: builder.hunks,
        conflicts: builder.conflicts,
        token: format!("{:016x}", hasher.finish()),
        id_limits,
        strip_final_newline,
        newline,
        scn: ids,
    }
}

/// `.scn` の3方向のマージ（行IDのある行は行IDで、無い行は中身で対応させる）。
pub fn merge_scn(base: &str, mine: &str, theirs: &str) -> Merge {
    merge3(base, mine, theirs, true)
}

/// 設定ファイルなど、行IDを見ないテキストの3方向のマージ（中身で対応させる）。
pub fn merge_text(base: &str, mine: &str, theirs: &str) -> Merge {
    merge3(base, mine, theirs, false)
}

impl Merge {
    /// ぶつかりを選んだ結果のテキスト。`choices` はぶつかりの順に1つずつ（数が合わなければ `None`）。
    pub fn resolve(&self, choices: &[Choice]) -> Option<String> {
        if choices.len() != self.conflicts {
            return None;
        }
        let mut out = String::new();
        let mut chosen = choices.iter();
        for hunk in &self.hunks {
            match hunk {
                Hunk::Merged { text } => push_text(&mut out, text, self.newline),
                Hunk::Conflict { mine, theirs, .. } => match chosen.next()? {
                    Choice::Mine => push_text(&mut out, mine, self.newline),
                    Choice::Theirs => push_text(&mut out, theirs, self.newline),
                    Choice::Both if self.scn => {
                        push_text(&mut out, mine, self.newline);
                        push_text(&mut out, &self.comment_block(theirs), self.newline);
                    }
                    Choice::Both => {
                        push_text(&mut out, &self.markers(mine, theirs), self.newline);
                    }
                },
            }
        }
        Some(self.finish(self.unique_ids(out)))
    }

    /// 自分にも外にも無いほど同じ行IDが並んだ行を、後のものからコメントにする（行の移動とぶつかりの選び方の
    /// 組み合わせで起きる）。
    fn unique_ids(&self, text: String) -> String {
        if !self.scn {
            return text;
        }
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();
        let mut out = String::with_capacity(text.len());
        let nl = self.newline;
        for segment in text.split_inclusive('\n') {
            let body = segment.strip_suffix('\n').unwrap_or(segment);
            let body = body.strip_suffix('\r').unwrap_or(body);
            let line = Line {
                text: segment.into(),
                body,
            };
            if let Key::Id(id) = key(&line, true) {
                let count = seen.entry(id.clone()).or_default();
                *count += 1;
                if *count > self.id_limits.get(&id).copied().unwrap_or(1) {
                    push_text(
                        &mut out,
                        &format!(
                            "// ▼ マージ：行ID ^{id} が2つになるので、コメントにした{nl}// {body}{nl}// ▲ マージ{nl}"
                        ),
                        nl,
                    );
                    continue;
                }
            }
            out.push_str(segment);
        }
        out
    }

    /// すべてのぶつかりを「両方」にした結果（マージの既定）。
    pub fn default_text(&self) -> String {
        self.resolve(&vec![Choice::Both; self.conflicts])
            .unwrap_or_default()
    }

    /// ぶつかりを `<<<<<<< 自分の変更`・`=======`・`>>>>>>> 外の変更` の印で囲んだテキスト（ソース表示の下書き）。
    pub fn with_markers(&self) -> String {
        let mut out = String::new();
        for hunk in &self.hunks {
            match hunk {
                Hunk::Merged { text } => push_text(&mut out, text, self.newline),
                Hunk::Conflict { mine, theirs, .. } => {
                    push_text(&mut out, &self.markers(mine, theirs), self.newline);
                }
            }
        }
        self.finish(out)
    }

    /// `<<<<<<<` の印で囲んだぶつかり。
    fn markers(&self, mine: &str, theirs: &str) -> String {
        let nl = self.newline;
        let mut out = format!("<<<<<<< 自分の変更{nl}");
        push_text(&mut out, mine, nl);
        push_text(&mut out, &format!("======={nl}"), nl);
        push_text(&mut out, theirs, nl);
        push_text(&mut out, &format!(">>>>>>> 外の変更{nl}"), nl);
        out
    }

    /// 両方を選んだときの、外の行のコメント（外で消されていれば、そのことの印）。
    fn comment_block(&self, theirs: &str) -> String {
        let nl = self.newline;
        if theirs.is_empty() {
            return format!("// ▲ マージ：外の変更では、上の行が消されていた{nl}");
        }
        let mut out = format!("// ▼ マージ：外の変更（コメントにして残した）{nl}");
        for line in theirs.lines() {
            if line.is_empty() {
                out.push_str("//");
            } else {
                out.push_str("// ");
                out.push_str(line);
            }
            out.push_str(nl);
        }
        out.push_str("// ▲ マージ");
        out.push_str(nl);
        out
    }

    /// 終わりの改行を、3方向で決めた形にする。
    fn finish(&self, mut out: String) -> String {
        if self.strip_final_newline {
            if out.ends_with("\r\n") {
                out.truncate(out.len() - 2);
            } else if out.ends_with('\n') {
                out.truncate(out.len() - 1);
            }
        }
        out
    }
}
