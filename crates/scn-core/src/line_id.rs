//! 行IDの付与（仕様書9節、D041）。`scn fmt` と、保存のときのエディタ（T1-6）が使う。
//!
//! 付けるのは I302 の出る行（行IDを持てる行で、行IDも形の合わない候補も無い行）だけ。
//! その行の末尾（改行の直前）に ` ^` と新しいIDを足し、ほかのバイトは変えない。

use std::collections::HashSet;
use std::hash::{BuildHasher, RandomState};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::ast::{LineIdSlot, Stmt};
use crate::line::{RawLineId, is_indent, split_line_id, split_lines};
use crate::project::Project;
use crate::syntax::parse;

/// 新しい行IDの長さ（仕様書9節）。
pub const NEW_LINE_ID_LEN: usize = 6;

/// 付けた1つの行ID。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignedLineId {
    /// 行IDを足した位置（付ける前の本文での、UTF-8のバイト位置）。その行の改行の直前。
    pub offset: usize,
    /// 新しいID（`^` を含まない）。
    pub id: String,
}

/// 行IDを付けたファイル。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileLineIds {
    /// プロジェクトの根からの相対パス（`Project::files` の `path`）。
    pub path: String,
    /// 行IDを付けた後の本文。
    pub text: String,
    /// 付けた行ID。位置の順。
    pub assigned: Vec<AssignedLineId>,
}

/// 新しい行IDを作る乱数（SplitMix64）。
pub struct LineIdGenerator {
    state: u64,
}

impl LineIdGenerator {
    /// 種を、プロセスごとに変わる値（`RandomState`）と時刻から取る。
    pub fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        Self::with_seed(RandomState::new().hash_one(nanos))
    }

    /// 決まった種から作る。同じ種なら、同じIDの並びになる（テスト用）。
    pub fn with_seed(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// `[0-9a-z]` の6文字。
    fn next_id(&mut self) -> String {
        let mut bits = self.next_u64();
        let mut id = String::with_capacity(NEW_LINE_ID_LEN);
        for _ in 0..NEW_LINE_ID_LEN {
            if let Some(c) = char::from_digit((bits % 36) as u32, 36) {
                id.push(c);
            }
            bits /= 36;
        }
        id
    }
}

impl Default for LineIdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// 行IDの無い行（I302 の出る行）に行IDを付ける。行IDを付けたファイルだけを、
/// プロジェクトのファイルの順に返す。どの行にも付けなければ空。
pub fn assign_line_ids(project: &Project, generator: &mut LineIdGenerator) -> Vec<FileLineIds> {
    assign_with(project, || generator.next_id())
}

fn assign_with(project: &Project, mut next_id: impl FnMut() -> String) -> Vec<FileLineIds> {
    let mut taken = existing_ids(project);
    let options = project.parse_options();
    let mut results = Vec::new();
    for file in &project.files {
        let parsed = parse(&file.text, &options);
        let mut offsets = Vec::new();
        for scene in &parsed.chapter.scenes {
            missing_in(&scene.body, &mut offsets);
        }
        if offsets.is_empty() {
            continue;
        }
        offsets.sort_unstable();
        offsets.dedup();
        let assigned: Vec<AssignedLineId> = offsets
            .into_iter()
            .map(|offset| {
                let id = loop {
                    let id = next_id();
                    if taken.insert(id.clone()) {
                        break id;
                    }
                };
                AssignedLineId { offset, id }
            })
            .collect();
        results.push(FileLineIds {
            path: file.path.clone(),
            text: insert(&file.text, &assigned),
            assigned,
        });
    }
    results
}

/// プロジェクトの本文にあるIDの候補。行IDを持てない行に付いたもの（W303）や、
/// 形の合わないもの（E304）も含めて、新しいIDと重ならないようにする。
fn existing_ids(project: &Project) -> HashSet<String> {
    let mut ids = HashSet::new();
    for file in &project.files {
        for line in split_lines(&file.text) {
            let trimmed = line.piece.trim_end_by(is_indent);
            if let (_, RawLineId::Valid(id) | RawLineId::Malformed(id)) = split_line_id(trimmed) {
                ids.insert(id.value);
            }
        }
    }
    ids
}

/// 行IDの無い行の、行IDを足す位置（その行の改行の直前）を集める。`sema` が I302 を出す行と同じ。
fn missing_in(stmts: &[Stmt], offsets: &mut Vec<usize>) {
    for stmt in stmts {
        match stmt {
            // 複数行の台詞では、スパンの終わりが最後の行の終わりになる。
            Stmt::Dialogue(dialogue) => {
                push_if_absent(&dialogue.line_id, dialogue.span.end, offsets)
            }
            Stmt::Narration(narration) => {
                push_if_absent(&narration.line_id, narration.span.end, offsets);
            }
            Stmt::Choice(choice) => {
                push_if_absent(&choice.line_id, choice.span.end, offsets);
                for option in &choice.options {
                    push_if_absent(&option.line_id, option.span.end, offsets);
                    missing_in(&option.body, offsets);
                }
            }
            Stmt::If(block) => {
                for branch in &block.branches {
                    missing_in(&branch.body, offsets);
                }
                if let Some(else_branch) = &block.else_branch {
                    missing_in(&else_branch.body, offsets);
                }
            }
            Stmt::Break(_) | Stmt::Directive(_) | Stmt::Jump(_) | Stmt::Raw(_) => {}
        }
    }
}

fn push_if_absent(slot: &LineIdSlot, end: usize, offsets: &mut Vec<usize>) {
    if matches!(slot, LineIdSlot::Absent) {
        offsets.push(end);
    }
}

/// 本文の決まった位置に ` ^ID` を足す。`assigned` は位置の順。
fn insert(text: &str, assigned: &[AssignedLineId]) -> String {
    let mut out = String::with_capacity(text.len() + assigned.len() * (NEW_LINE_ID_LEN + 2));
    let mut from = 0;
    for a in assigned {
        out.push_str(text.get(from..a.offset).unwrap_or_default());
        out.push_str(" ^");
        out.push_str(&a.id);
        from = a.offset;
    }
    out.push_str(text.get(from..).unwrap_or_default());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::SourceFile;

    fn project(text: &str) -> Project {
        let mut project = Project::default();
        project.files.push(SourceFile {
            path: "story/a.scn".into(),
            text: text.into(),
        });
        project
    }

    /// 既存のID（行IDを持てない行に付いたもの、形の合わないものを含む）と、付けたばかりのIDは使わない。
    #[test]
    fn skips_taken_ids() {
        let project = project("## a #s1 ^head01\n地の文 ^ABC123\n一\n二\n");
        let mut script = ["head01", "ABC123", "new001", "new001", "new002"].into_iter();
        let results = assign_with(&project, || script.next().unwrap().to_owned());
        let ids: Vec<&str> = results[0].assigned.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["new001", "new002"]);
    }

    #[test]
    fn ids_have_the_new_form() {
        let mut generator = LineIdGenerator::with_seed(0);
        for _ in 0..1000 {
            let id = generator.next_id();
            assert_eq!(id.len(), NEW_LINE_ID_LEN);
            assert!(
                id.chars()
                    .all(|c| c.is_ascii_digit() || c.is_ascii_lowercase())
            );
        }
    }
}
