//! 補完の候補（言語サーバー。T3-15）：カーソルより前の行の文字から文脈を決め、候補を作る。決まりはアプリの
//! エディタの補完（`app/src/editor/complete.ts`。T1-8、D047）と同じ：
//!
//! - シーン見出しの ` #` の後ろ → 空いているシーンID
//! - `->` の後ろ → シーンID
//! - 文中の `{se ` の後ろ → 効果音
//! - 行頭の `@`（全角の `＠` も）→ 命令。命令の後ろ → 引数（背景名、キャラ、差分、位置、曲名、効果音、シーンID、
//!   変数）
//! - 行頭のキャラ名 → `名前「」`（カーソルは括弧の中）

use std::collections::BTreeSet;

use serde::Serialize;

use crate::Span;
use crate::ir::Ir;
use crate::project::Project;
use crate::syntax::{BGM_STOP, DIRECTIVES, POSITIONS};

/// 補完の候補の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionKind {
    Scene,
    Character,
    Variable,
    Asset,
    Directive,
    Keyword,
}

/// 補完の候補。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Completion {
    pub label: String,
    pub detail: String,
    pub kind: CompletionKind,
    /// 置き換える範囲（今打っている語。本文のバイト位置）。
    pub span: Span,
    /// 入れる文字。`cursor` は入れた後のカーソルの位置（入れる文字の頭からのバイト数。無ければ末尾）。
    pub insert: String,
    pub cursor: Option<usize>,
}

/// 前方一致（英字の大文字と小文字は区別しない）。
fn matches(candidate: &str, typed: &str) -> bool {
    candidate.to_lowercase().starts_with(&typed.to_lowercase())
}

fn is_id_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// 行末の行ID（` ^xxxx`）を除いた後ろが空か。
fn rest_empty(after: &str) -> bool {
    let trimmed = after.trim_end_matches([' ', '\t']);
    let without_id = match trimmed.rfind(" ^") {
        Some(i)
            if trimmed.get(i + 2..).is_some_and(|id| {
                !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric())
            }) =>
        {
            trimmed.get(..i).unwrap_or_default()
        }
        _ => trimmed,
    };
    without_id.trim().is_empty()
}

/// 空いているシーンIDの提案（アプリの `suggestSceneIds` と同じ）。
pub fn suggest_scene_ids(used: &BTreeSet<&str>, base: Option<&str>) -> Vec<String> {
    let next = |prefix: &str, from: u64, width: usize| -> String {
        let mut n = from;
        loop {
            let id = format!("{prefix}{n:0width$}");
            if !used.contains(id.as_str()) {
                return id;
            }
            // 数があふれるほど大きい番号は、そのまま返す（起こりにくい）。
            match n.checked_add(1) {
                Some(next) => n = next,
                None => return id,
            }
        }
    };
    let mut out = Vec::new();
    if let Some(base) = base {
        let digits = base.len() - base.trim_end_matches(|c: char| c.is_ascii_digit()).len();
        let cut = base.len() - digits;
        let (prefix, number) = (
            base.get(..cut).unwrap_or_default(),
            base.get(cut..).unwrap_or_default(),
        );
        if digits > 0
            && prefix.chars().all(|c| c.is_ascii_alphabetic() || c == '_')
            && let Ok(n) = number.parse::<u64>()
        {
            out.push(next(prefix, n.saturating_add(1), digits));
        }
    }
    let numbered: Vec<&str> = used
        .iter()
        .filter_map(|id| id.strip_prefix('s'))
        .filter(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
        .collect();
    let max = numbered
        .iter()
        .filter_map(|d| d.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    let width = numbered.iter().map(|d| d.len()).max().unwrap_or(3).max(3);
    let fallback = next("s", max.saturating_add(1), width);
    if !out.contains(&fallback) {
        out.push(fallback);
    }
    out
}

/// 変数の書き方：`flag.` は省き、キャラクターの名前空間は名前で書く（本文での書き方）。
fn variable_label(project: &Project, key: &str) -> String {
    if let Some(rest) = key.strip_prefix("flag.") {
        return rest.to_owned();
    }
    match key.split_once('.') {
        Some((ns, rest)) => match project.characters.iter().find(|c| c.id == ns) {
            Some(c) => format!("{}.{rest}", c.name),
            None => key.to_owned(),
        },
        None => key.to_owned(),
    }
}

/// キャラクターを ID・名前・呼び名の順で引く（話者の解決と同じ順）。
fn find_character<'p>(project: &'p Project, text: &str) -> Option<&'p crate::project::Character> {
    project
        .characters
        .iter()
        .find(|c| c.id == text)
        .or_else(|| project.characters.iter().find(|c| c.name == text))
        .or_else(|| {
            project
                .characters
                .iter()
                .find(|c| c.aliases.iter().any(|a| a == text))
        })
}

/// 補完の候補を作る。`text` は本文のファイルの今の中身、`offset` はカーソル（バイト位置）。
pub fn complete_at(project: &Project, ir: &Ir, text: &str, offset: usize) -> Vec<Completion> {
    let offset = offset.min(text.len());
    let Some(before_all) = text.get(..offset) else {
        return Vec::new();
    };
    let line_start = before_all.rfind('\n').map_or(0, |i| i + 1);
    let before = before_all.get(line_start..).unwrap_or_default();
    let before = before.strip_suffix('\r').unwrap_or(before);
    let after_all = text.get(offset..).unwrap_or_default();
    let after = after_all
        .split('\n')
        .next()
        .unwrap_or_default()
        .trim_end_matches('\r');
    let mut out = Vec::new();
    let mut push = |label: String,
                    detail: String,
                    kind,
                    from: usize,
                    insert: Option<(String, Option<usize>)>| {
        let (insert, cursor) = insert.unwrap_or_else(|| (label.clone(), None));
        out.push(Completion {
            label,
            detail,
            kind,
            span: Span::new(from, offset),
            insert,
            cursor,
        });
    };
    let trimmed = before.trim_start_matches([' ', '\t']);
    let indent = before.len() - trimmed.len();

    // シーン見出しの ` #` の後ろ：空いているシーンID（`## ` の空白は ` #` に数えない。アプリと同じ）。
    if trimmed.starts_with("## ")
        && let Some(hash) = trimmed.rfind(" #").filter(|h| *h >= 3)
    {
        let typed = trimmed.get(hash + 2..).unwrap_or_default();
        if typed.chars().all(is_id_char) {
            if !rest_empty(after) {
                return out;
            }
            let used: BTreeSet<&str> = ir.scenes.iter().map(|s| s.id.as_str()).collect();
            let base = (!typed.is_empty()).then_some(typed);
            for id in suggest_scene_ids(&used, base) {
                if matches(&id, typed) {
                    push(
                        id,
                        "空いているシーンID".to_owned(),
                        CompletionKind::Scene,
                        offset - typed.len(),
                        None,
                    );
                }
            }
            return out;
        }
    }

    // `->` の後ろ：シーンID。
    if let Some(arrow) = before.rfind("->") {
        let head = before.get(..arrow).unwrap_or_default();
        let tail = before.get(arrow + 2..).unwrap_or_default();
        let typed = tail.trim_start_matches([' ', '\t']);
        let head_ok =
            head.trim_start_matches([' ', '\t']).is_empty() || head.ends_with([' ', '\t']);
        if head_ok && typed.chars().all(is_id_char) {
            for scene in ir.scenes.iter().filter(|s| !s.id.starts_with("_anon_")) {
                if matches(&scene.id, typed) {
                    push(
                        scene.id.clone(),
                        scene.title.clone(),
                        CompletionKind::Scene,
                        offset - typed.len(),
                        None,
                    );
                }
            }
            return out;
        }
    }

    // 文中の `{se ` の後ろ：効果音。
    if let Some(brace) = before.rfind("{se") {
        let tail = before.get(brace + 3..).unwrap_or_default();
        let typed = tail.trim_start_matches([' ', '\t']);
        if typed.len() < tail.len() && !typed.chars().any(|c| c == '}' || c.is_whitespace()) {
            for name in &project.catalog.se {
                if matches(name, typed) {
                    push(
                        name.clone(),
                        "効果音".to_owned(),
                        CompletionKind::Asset,
                        offset - typed.len(),
                        None,
                    );
                }
            }
            return out;
        }
    }

    // 行頭の `@`：命令の名前。全角の `＠` も受け、選んだら `@` にする。
    let at = trimmed
        .strip_prefix('@')
        .or_else(|| trimmed.strip_prefix('＠'));
    if let Some(rest) = at
        && !rest.contains([' ', '\t', '　', '@', '＠'])
    {
        let typed = rest;
        for d in &DIRECTIVES {
            if matches(d.ja, typed) || matches(d.en, typed) {
                let detail = if d.ja == d.en {
                    d.args.to_owned()
                } else {
                    format!("{}　{}", d.en, d.args)
                };
                let insert = format!("@{}{}", d.ja, if d.args.is_empty() { "" } else { " " });
                push(
                    format!("@{}", d.ja),
                    detail,
                    CompletionKind::Directive,
                    line_start + indent,
                    Some((insert, None)),
                );
            }
        }
        return out;
    }

    // 命令の引数。
    if let Some(rest) = at {
        let name_end = rest.find([' ', '\t', '　']).unwrap_or(rest.len());
        let name = rest.get(..name_end).unwrap_or_default();
        let args_text = rest.get(name_end..).unwrap_or_default();
        let Some(info) = DIRECTIVES.iter().find(|d| d.ja == name || d.en == name) else {
            return out;
        };
        // 引数は空白（全角の空白も）の並びで区切る。いちばん後ろが今打っている語（空でもよい）。
        let mut parts: Vec<&str> = args_text.split([' ', '\t', '　']).collect();
        let typed = parts.pop().unwrap_or_default();
        let args: Vec<&str> = parts.into_iter().filter(|p| !p.is_empty()).collect();
        let from = offset - typed.len();
        let mut pick = |names: Vec<String>, detail: &str, kind: CompletionKind| {
            for n in names {
                if matches(&n, typed) {
                    push(n, detail.to_owned(), kind, from, None);
                }
            }
        };
        let characters = || {
            project
                .characters
                .iter()
                .map(|c| c.name.clone())
                .collect::<Vec<_>>()
        };
        let variables = || {
            project
                .variables
                .iter()
                .map(|v| variable_label(project, &v.key))
                .collect::<Vec<_>>()
        };
        match (info.en, args.len()) {
            ("bg", 0) => pick(
                project.catalog.backgrounds.iter().cloned().collect(),
                "背景",
                CompletionKind::Asset,
            ),
            ("show", 0) | ("hide", 0) => pick(characters(), "キャラ", CompletionKind::Character),
            ("show", 1) => {
                if let Some(c) = args.first().and_then(|a| find_character(project, a)) {
                    let names = project
                        .catalog
                        .characters
                        .get(&c.id)
                        .map(|s| s.iter().cloned().collect())
                        .unwrap_or_default();
                    pick(names, &format!("{}の差分", c.name), CompletionKind::Asset);
                }
            }
            ("show", 2) => pick(
                POSITIONS.iter().map(|p| (*p).to_owned()).collect(),
                "位置",
                CompletionKind::Keyword,
            ),
            ("bgm", 0) => {
                let mut names: Vec<String> = project.catalog.bgm.iter().cloned().collect();
                names.push(BGM_STOP.to_owned());
                pick(names, "曲名", CompletionKind::Asset);
            }
            ("se", 0) => pick(
                project.catalog.se.iter().cloned().collect(),
                "効果音",
                CompletionKind::Asset,
            ),
            ("call", 0) => pick(
                ir.scenes
                    .iter()
                    .filter(|s| !s.id.starts_with("_anon_"))
                    .map(|s| s.id.clone())
                    .collect(),
                "シーンID",
                CompletionKind::Scene,
            ),
            ("set", 0) => pick(variables(), "変数", CompletionKind::Variable),
            ("if" | "elif", _) => {
                // 式の中の、今打っている名前。
                let word_start = args_text
                    .rfind(|c: char| c.is_whitespace() || "!=<>()&|+-*/".contains(c))
                    .map_or(0, |i| {
                        i + args_text
                            .get(i..)
                            .and_then(|s| s.chars().next())
                            .map_or(1, char::len_utf8)
                    });
                let word = args_text.get(word_start..).unwrap_or_default();
                for n in variables() {
                    if matches(&n, word) {
                        push(
                            n,
                            "変数".to_owned(),
                            CompletionKind::Variable,
                            offset - word.len(),
                            None,
                        );
                    }
                }
            }
            _ => {}
        }
        return out;
    }

    // 行頭のキャラ名：`名前「」` にする。行の後ろに文字が無いときだけ。
    let typed = trimmed;
    let forbidden = |c: char| c.is_whitespace() || "@＠#/{}\\-「」『』（）()〈〉<>".contains(c);
    if !typed.is_empty() && !typed.contains(forbidden) && rest_empty(after) {
        let (open, close) = project
            .settings
            .brackets
            .first()
            .map_or(('「', '」'), |b| (b.open, b.close));
        // 1文字の前方一致では出さない。名前・呼び名・IDとちょうど同じなら1文字でも出す。
        let hits = |candidate: &str| {
            // 長さは UTF-16 で数える（アプリと同じ）。
            candidate == typed || (typed.encode_utf16().count() >= 2 && matches(candidate, typed))
        };
        for c in &project.characters {
            let alias = c.aliases.iter().find(|a| hits(a));
            if hits(&c.name) || hits(&c.id) || alias.is_some() {
                let insert = format!("{}{open}{close}", c.name);
                let cursor = c.name.len() + open.len_utf8();
                let detail = alias.map_or_else(|| c.id.clone(), |a| format!("{a}（{}）", c.id));
                push(
                    c.name.clone(),
                    detail,
                    CompletionKind::Character,
                    line_start + indent,
                    Some((insert, Some(cursor))),
                );
            }
        }
    }
    out
}
