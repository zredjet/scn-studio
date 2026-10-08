//! 表記の検査（検証の段階2。T3-3、D064）：本文の文字を見て、表記ゆれと、登録した用語に近い未登録の書き方を
//! 知らせる（`docs/design.md` の「検証項目」の「表記ゆれ、未登録の用語」。情報）。
//!
//! - 調べるのは、台詞の本文（行ごと）・地の文・選択肢の問いかけ文と表示文（`@if` と選択肢の中も）の、見える
//!   文字。構文木の文中の記法の位置から、文字ごとのファイルの中の位置を求める（ルビの読み、インライン命令は
//!   見えないので除く。変数の埋め込みは語の区切りにする）。位置は、その書き方の文字の範囲。
//! - 一人称の表記ゆれ（I204）：キャラクターの台詞に、設定DBの一人称（`first_person`）と読みが同じで書き方の
//!   違う一人称（`わたし` に対する `私`・`ワタシ` など）がある。読みの組は決めたものだけ（`FIRST_PERSON`）。
//!   ルビで読みを示した書き方には出さない（書き手が読みを決めている）。
//! - 避ける書き方（I205）：設定DBのエントリの `variants`（避ける書き方の文字列か、そのリスト）が本文にある。
//!   登録した名前・呼び名の中の現れ（`北風高` に対する `北風高校`）には出さない。
//! - 未登録の用語（I206）：設定DBの名前・呼び名と、かなの違い（ひらがなとカタカナ、小さいかな、長音符・中黒）
//!   だけが違う、カタカナを含む書き方（`あかり` に対する `アカリ` など）。ひらがなだけの書き方は、よくある語と
//!   重なるので出さない。登録した書き方そのもの、語の端の長音符だけが違うもの（`あかりー`）、名前・呼び名の中の
//!   現れ、`variants` に書いた書き方（I205 で知らせる）には出さない。
//! - 語の区切り：漢字の書き方は前後が漢字でないとき、カタカナの書き方は前後がカタカナでないとき、ひらがなの
//!   書き方は前がひらがなでなく、後ろが助詞・記号・行末のとき（後ろの `ー`・`っ`・`ッ`・`〜` は飛ばして見る）。
//! - 比べるときだけかなをそろえ、本文の文字は変えない（コードの約束）。

use std::collections::{BTreeMap, BTreeSet};

use crate::ast::Stmt;
use crate::diag::{Code, Span};
use crate::inline::{Inline, Text};
use crate::ir::{Ir, IrDiagnostic, Op};
use crate::project::Project;
use crate::syntax::parse;

/// 一人称の、読みが同じ書き方の組。設定DBの一人称がどれかに当たれば、同じ組のほかの書き方を表記ゆれにする。
const FIRST_PERSON: [&[&str]; 7] = [
    &["わたし", "私", "ワタシ"],
    &["わたくし", "ワタクシ"],
    &["ぼく", "僕", "ボク"],
    &["おれ", "俺", "オレ"],
    &["あたし", "アタシ"],
    &["わし", "儂", "ワシ"],
    &["おいら", "オイラ"],
];

/// ひらがなの語の後ろに来てよい助詞（ほかには、記号・空白・行末）。
const PARTICLES: &str = "がはのもをにとへでやかよねなだ";

/// 語の区切りにする見えない文字（変数の埋め込みの代わり）。
const SEPARATOR: char = '\u{FFFC}';

fn is_kanji(c: char) -> bool {
    matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '々' | '〆')
}

fn is_hiragana(c: char) -> bool {
    matches!(c, '\u{3041}'..='\u{309F}')
}

fn is_katakana(c: char) -> bool {
    matches!(c, '\u{30A1}'..='\u{30FA}' | 'ー' | '\u{30FD}' | '\u{30FE}')
}

/// かなをそろえるときに除く文字（長音符・中黒）。
fn is_dropped(c: char) -> bool {
    matches!(c, 'ー' | '・' | '･')
}

/// 語の後ろで飛ばして見る文字（伸ばす・詰める）。
fn is_elongation(c: char) -> bool {
    matches!(c, 'ー' | 'っ' | 'ッ' | '〜' | '～')
}

/// かなをそろえた1文字（比べるためだけ）：ひらがなをカタカナに、小さいかなを大きいかなに。
fn kana_char(c: char) -> char {
    let c = if matches!(c, '\u{3041}'..='\u{3096}') {
        char::from_u32(u32::from(c) + 0x60).unwrap_or(c)
    } else {
        c
    };
    match c {
        'ァ' => 'ア',
        'ィ' => 'イ',
        'ゥ' => 'ウ',
        'ェ' => 'エ',
        'ォ' => 'オ',
        'ッ' => 'ツ',
        'ャ' => 'ヤ',
        'ュ' => 'ユ',
        'ョ' => 'ヨ',
        'ヮ' => 'ワ',
        'ヵ' => 'カ',
        'ヶ' => 'ケ',
        c => c,
    }
}

/// かなをそろえた形。
fn kana_key(text: &str) -> Vec<char> {
    text.chars()
        .filter(|c| !is_dropped(*c))
        .map(kana_char)
        .collect()
}

/// 見える文字1つ：文字と、ファイルの中のバイト位置の範囲と、ルビの親文字か。
#[derive(Debug, Clone, Copy)]
struct Glyph {
    c: char,
    start: usize,
    end: usize,
    ruby: bool,
}

/// 本文の1行（台詞の本文の1行分、地の文、選択肢の問いかけ文・表示文）。
struct Line<'a> {
    file: &'a str,
    /// 台詞なら話者のキャラクターID（話者を解決できた台詞だけ）。
    speaker: Option<&'a str>,
    glyphs: Vec<Glyph>,
}

impl Line<'_> {
    fn chars(&self) -> Vec<char> {
        self.glyphs.iter().map(|g| g.c).collect()
    }

    fn span(&self, from: usize, to: usize) -> Span {
        match (
            self.glyphs.get(from),
            to.checked_sub(1).and_then(|i| self.glyphs.get(i)),
        ) {
            (Some(a), Some(b)) => Span::new(a.start, b.end),
            _ => Span::new(0, 0),
        }
    }
}

/// `text`（中身）の文字を、ファイルの `raw`（`at` から始まる、その部分の書かれた文字）に順に当てる。エスケープ
/// （`{{`・`|《` など）で書かれた文字の数が違っても、同じ文字を前から探して当てる。
fn align(text: &str, raw: &str, at: usize, ruby: bool, out: &mut Vec<Glyph>) {
    let mut rest = raw.char_indices();
    for c in text.chars() {
        let found = rest.by_ref().find(|(_, r)| *r == c);
        let (start, end) = match found {
            Some((i, r)) => (at + i, at + i + r.len_utf8()),
            // 当たらなければ（書かれた文字に無い文字）、部分の終わりに置く。
            None => (at + raw.len(), at + raw.len()),
        };
        out.push(Glyph {
            c,
            start,
            end,
            ruby,
        });
    }
}

fn glyphs(text: &Text, source: &str) -> Vec<Glyph> {
    let mut out = Vec::new();
    for part in &text.parts {
        match part {
            Inline::Text { text, span } | Inline::Emphasis { text, span } => {
                let raw = source.get(span.start..span.end).unwrap_or_default();
                align(text, raw, span.start, false, &mut out);
            }
            Inline::Ruby { base, span, .. } => {
                let raw = source.get(span.start..span.end).unwrap_or_default();
                align(base, raw, span.start, true, &mut out);
            }
            Inline::Interpolation { span, .. } => out.push(Glyph {
                c: SEPARATOR,
                start: span.start,
                end: span.end,
                ruby: false,
            }),
            Inline::Wait { .. }
            | Inline::PageBreak { .. }
            | Inline::Se { .. }
            | Inline::Speed { .. } => {}
        }
    }
    out
}

fn collect<'a>(
    stmts: &[Stmt],
    file: &'a str,
    source: &str,
    speakers: &BTreeMap<usize, &'a str>,
    out: &mut Vec<Line<'a>>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Dialogue(d) => {
                let speaker = speakers.get(&d.span.start).copied();
                // 台詞にならなかった行（話者を解決できない、閉じ括弧の後ろに文が続く。仕様書6節）は地の文なので、
                // 話者名・表示名・閉じ括弧の後ろの文も調べる。
                let narration = speaker.is_none();
                if narration {
                    for name in std::iter::once(&d.speaker).chain(d.display_name.as_ref()) {
                        let raw = source
                            .get(name.span.start..name.span.end)
                            .unwrap_or_default();
                        let mut glyphs = Vec::new();
                        align(&name.value, raw, name.span.start, false, &mut glyphs);
                        out.push(Line {
                            file,
                            speaker: None,
                            glyphs,
                        });
                    }
                }
                for text in &d.lines {
                    out.push(Line {
                        file,
                        speaker,
                        glyphs: glyphs(text, source),
                    });
                }
                if narration && let Some(trailing) = &d.trailing {
                    out.push(Line {
                        file,
                        speaker: None,
                        glyphs: glyphs(trailing, source),
                    });
                }
            }
            Stmt::Narration(n) => out.push(Line {
                file,
                speaker: None,
                glyphs: glyphs(&n.text, source),
            }),
            Stmt::Choice(c) => {
                if let Some(prompt) = &c.prompt {
                    out.push(Line {
                        file,
                        speaker: None,
                        glyphs: glyphs(prompt, source),
                    });
                }
                for option in &c.options {
                    out.push(Line {
                        file,
                        speaker: None,
                        glyphs: glyphs(&option.text, source),
                    });
                    collect(&option.body, file, source, speakers, out);
                }
            }
            Stmt::If(block) => {
                for branch in &block.branches {
                    collect(&branch.body, file, source, speakers, out);
                }
                if let Some(e) = &block.else_branch {
                    collect(&e.body, file, source, speakers, out);
                }
            }
            Stmt::Break(_) | Stmt::Directive(_) | Stmt::Jump(_) | Stmt::Raw(_) => {}
        }
    }
}

/// IR の台詞の話者（台詞の行の始まりの位置から、キャラクターID）。
fn say_speakers<'a>(ops: &'a [Op], out: &mut BTreeMap<usize, &'a str>) {
    for op in ops {
        match op {
            Op::Say { span, speaker, .. } => {
                out.insert(span.start, speaker.as_str());
            }
            Op::Choice { options, .. } => {
                for option in options {
                    say_speakers(&option.body, out);
                }
            }
            Op::If {
                branches,
                else_body,
                ..
            } => {
                for branch in branches {
                    say_speakers(&branch.body, out);
                }
                if let Some(body) = else_body {
                    say_speakers(body, out);
                }
            }
            _ => {}
        }
    }
}

/// 語の区切り：`chars[from..to]` の書き方が、語の一部でないか。
fn bounded(chars: &[char], from: usize, to: usize) -> bool {
    let (Some(&first), Some(&last)) = (
        chars.get(from),
        to.checked_sub(1).and_then(|i| chars.get(i)),
    ) else {
        return false;
    };
    let prev = from.checked_sub(1).and_then(|i| chars.get(i)).copied();
    let direct = chars.get(to).copied();
    // 後ろの、伸ばす・詰める文字は飛ばして見る。
    let next = chars
        .get(to..)
        .and_then(|rest| rest.iter().copied().find(|c| !is_elongation(*c)));
    if is_kanji(first) || is_kanji(last) {
        return !prev.is_some_and(is_kanji) && !direct.is_some_and(is_kanji);
    }
    if is_katakana(first) || is_katakana(last) {
        // 長音符はカタカナの語の続き（`アカリー` は `アカリ` の語の一部）。
        return !prev.is_some_and(is_katakana) && !direct.is_some_and(is_katakana);
    }
    if is_hiragana(first) || is_hiragana(last) {
        return !prev.is_some_and(is_hiragana)
            && next.is_none_or(|c| PARTICLES.contains(c) || !c.is_alphanumeric());
    }
    true
}

/// `chars` の中の `needle` の現れ（文字の位置）。
fn find_all(chars: &[char], needle: &[char]) -> Vec<usize> {
    if needle.is_empty() || needle.len() > chars.len() {
        return Vec::new();
    }
    (0..=chars.len() - needle.len())
        .filter(|&i| chars.get(i..i + needle.len()) == Some(needle))
        .collect()
}

fn strings(value: Option<&serde_json::Value>) -> Vec<String> {
    match value {
        Some(serde_json::Value::String(s)) if !s.is_empty() => vec![s.clone()],
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

/// 名前・呼び名の書き方。
struct Term {
    surface: Vec<char>,
    /// 語の端の長音符・中黒を除いた形。
    core: String,
    /// かなをそろえた形。
    key: Vec<char>,
    name: String,
}

fn diag(out: &mut Vec<IrDiagnostic>, code: Code, file: &str, span: Span, message: String) {
    out.push(IrDiagnostic {
        code: code.as_str(),
        severity: code.severity().as_str(),
        file: file.to_owned(),
        span,
        message,
    });
}

/// 表記の検査の診断を返す。ファイルと位置の順に並べる。
pub fn wording(project: &Project, ir: &Ir) -> Vec<IrDiagnostic> {
    // 本文の行。
    let options = project.parse_options();
    let mut lines = Vec::new();
    for file in &project.files {
        let mut speakers = BTreeMap::new();
        for scene in ir.scenes.iter().filter(|s| s.file == file.path) {
            say_speakers(&scene.ops, &mut speakers);
        }
        let parsed = parse(&file.text, &options);
        for scene in &parsed.chapter.scenes {
            collect(&scene.body, &file.path, &file.text, &speakers, &mut lines);
        }
    }

    // 登録した名前と呼び名（すべてのエントリ）と、避ける書き方。
    let mut registered: BTreeSet<String> = BTreeSet::new();
    let mut terms: Vec<Term> = Vec::new();
    let mut variants: Vec<(Vec<char>, String, String)> = Vec::new(); // (文字, 書き方, エントリの名前)
    for entry in &project.bible.entries {
        let name = entry
            .fields
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&entry.id)
            .to_owned();
        for term in strings(entry.fields.get("name"))
            .into_iter()
            .chain(strings(entry.fields.get("aliases")))
        {
            registered.insert(term.clone());
            terms.push(Term {
                surface: term.chars().collect(),
                core: term.trim_matches(is_dropped).to_owned(),
                key: kana_key(&term),
                name: name.clone(),
            });
        }
        for variant in strings(entry.fields.get("variants")) {
            variants.push((variant.chars().collect(), variant, name.clone()));
        }
    }
    let variant_set: BTreeSet<&str> = variants.iter().map(|(_, v, _)| v.as_str()).collect();
    // 未登録の用語に使う書き方：かなを含み、そろえた形が2字以上のもの。そろえた形の最初の文字で引く。
    let mut by_first: BTreeMap<char, Vec<&Term>> = BTreeMap::new();
    for term in &terms {
        if term.key.len() >= 2
            && term
                .surface
                .iter()
                .any(|c| is_hiragana(*c) || is_katakana(*c))
            && let Some(&first) = term.key.first()
        {
            by_first.entry(first).or_default().push(term);
        }
    }
    let character_type = project.bible.character_type();

    let mut diags = Vec::new();
    for line in &lines {
        let chars = line.chars();
        // 登録した名前・呼び名の現れ（I205・I206 は、この中には出さない）。
        let mut inside: Vec<(usize, usize)> = Vec::new();
        for term in &terms {
            for at in find_all(&chars, &term.surface) {
                inside.push((at, at + term.surface.len()));
            }
        }
        let within = |from: usize, to: usize| inside.iter().any(|&(a, b)| from < b && a < to);

        // 一人称（I204）。
        if let Some(speaker) = line.speaker
            && let Some(first) = project
                .bible
                .entries
                .iter()
                .find(|e| e.type_id == character_type && e.id == speaker)
                .and_then(|e| e.fields.get("first_person"))
                .and_then(serde_json::Value::as_str)
            && let Some(group) = FIRST_PERSON.iter().find(|g| g.contains(&first))
        {
            for other in group.iter().filter(|o| **o != first) {
                let needle: Vec<char> = other.chars().collect();
                for at in find_all(&chars, &needle) {
                    let to = at + needle.len();
                    let ruby = line
                        .glyphs
                        .get(at..to)
                        .is_some_and(|g| g.iter().any(|g| g.ruby));
                    if ruby || !bounded(&chars, at, to) {
                        continue;
                    }
                    let name = project
                        .characters
                        .iter()
                        .find(|c| c.id == speaker)
                        .map_or(speaker, |c| c.name.as_str());
                    diag(
                        &mut diags,
                        Code::I204,
                        line.file,
                        line.span(at, to),
                        format!(
                            "表記ゆれ：{name} の一人称は「{first}」（設定DB）ですが、ここは「{other}」です"
                        ),
                    );
                }
            }
        }

        // 避ける書き方（I205）。
        for (needle, variant, name) in &variants {
            if registered.contains(variant) {
                continue;
            }
            for at in find_all(&chars, needle) {
                let to = at + needle.len();
                if !bounded(&chars, at, to) || within(at, to) {
                    continue;
                }
                diag(
                    &mut diags,
                    Code::I205,
                    line.file,
                    line.span(at, to),
                    format!("表記ゆれ：「{variant}」は「{name}」と書きます（{name} の variants）"),
                );
            }
        }

        // 未登録の用語（I206）：かなをそろえた形で、登録した書き方と同じになる書き方。
        let mut key = Vec::with_capacity(chars.len());
        let mut back = Vec::with_capacity(chars.len()); // そろえた形の文字 → 見える文字の位置
        for (i, c) in chars.iter().enumerate() {
            if !is_dropped(*c) {
                key.push(kana_char(*c));
                back.push(i);
            }
        }
        let mut reported: BTreeSet<String> = BTreeSet::new();
        for (j, c) in key.iter().enumerate() {
            for term in by_first.get(c).into_iter().flatten() {
                let len = term.key.len();
                if key.get(j..j + len) != Some(term.key.as_slice()) {
                    continue;
                }
                let (Some(&from), Some(&last)) = (back.get(j), back.get(j + len - 1)) else {
                    continue;
                };
                let to = last + 1;
                let surface: String = chars.get(from..to).unwrap_or_default().iter().collect();
                if !surface.chars().any(|c| is_katakana(c) && c != 'ー')
                    || surface == term.core
                    || registered.contains(&surface)
                    || variant_set.contains(surface.as_str())
                    || within(from, to)
                    || !bounded(&chars, from, to)
                    || !reported.insert(surface.clone())
                {
                    continue;
                }
                let written: String = term.surface.iter().collect();
                diag(
                    &mut diags,
                    Code::I206,
                    line.file,
                    line.span(from, to),
                    format!(
                        "未登録の用語：「{surface}」は、設定DBの「{written}」（{}）と、かなの書き方だけが違います",
                        term.name
                    ),
                );
            }
        }
    }
    diags.sort_by(|a, b| (&a.file, a.span.start, a.code).cmp(&(&b.file, b.span.start, b.code)));
    diags.dedup();
    diags
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    fn found(text: &str, needle: &str) -> Vec<usize> {
        let c = chars(text);
        let n = chars(needle);
        find_all(&c, &n)
            .into_iter()
            .filter(|&at| bounded(&c, at, at + n.len()))
            .collect()
    }

    #[test]
    fn boundaries() {
        assert_eq!(found("私が行く", "私"), [0]);
        assert!(found("私立の高校", "私").is_empty());
        assert!(found("下僕だ", "僕").is_empty());
        assert_eq!(found("ボクは", "ボク"), [0]);
        assert!(found("ボクシング", "ボク").is_empty());
        assert_eq!(found("ねえ、ぼくが", "ぼく"), [3]);
        assert!(found("どぼくの", "ぼく").is_empty());
        assert!(found("おれいを", "おれ").is_empty());
        // 後ろの伸ばす文字は飛ばして見る。記号は区切り。
        assert_eq!(found("ぼくーっ！", "ぼく"), [0]);
        assert_eq!(found("ぼく—", "ぼく"), [0]);
        // 長音符で続くカタカナは語の続き。
        assert!(found("アカリー", "アカリ").is_empty());
    }

    #[test]
    fn kana_forms() {
        assert_eq!(kana_key("あかり"), chars("アカリ"));
        assert_eq!(kana_key("ルーシィ"), chars("ルシイ"));
    }
}
