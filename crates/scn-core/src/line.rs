//! 1行の分類（仕様書4節）。
//!
//! 行末の空白の除去、コメントの判定、行IDの切り出し、行頭の空白の読み飛ばし、
//! エスケープを行ってから、行の種類を決める。章見出しやメタ情報の区切りのように
//! 前後の行で意味が変わるものは、ここでは形だけを見て、最終的な判断は `syntax` で行う。

use crate::ast::LineId;
use crate::diag::Span;
use crate::syntax::DialogueBracket;

/// 台詞候補の話者名の長さの上限（文字数）。
const MAX_SPEAKER_CHARS: usize = 20;

/// 行頭で読み飛ばし、行末で取り除く空白（半角空白とタブ）。
pub(crate) fn is_indent(c: char) -> bool {
    c == ' ' || c == '\t'
}

/// 区切りとしての空白（半角空白、タブ、全角空白）。
pub(crate) fn is_space(c: char) -> bool {
    c == ' ' || c == '\t' || c == '\u{3000}'
}

/// ソースの一部と、その先頭のバイト位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Piece<'a> {
    pub(crate) text: &'a str,
    pub(crate) start: usize,
}

impl<'a> Piece<'a> {
    pub(crate) fn new(text: &'a str, start: usize) -> Self {
        Self { text, start }
    }

    pub(crate) fn end(&self) -> usize {
        self.start + self.text.len()
    }

    pub(crate) fn span(&self) -> Span {
        Span::new(self.start, self.end())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// 相対位置 `at` で2つに分ける。文字の境界でなければ、全体と空を返す。
    pub(crate) fn split_at(&self, at: usize) -> (Piece<'a>, Piece<'a>) {
        match self.text.split_at_checked(at) {
            Some((head, tail)) => (
                Piece::new(head, self.start),
                Piece::new(tail, self.start + at),
            ),
            None => (*self, Piece::new("", self.end())),
        }
    }

    /// 相対位置 `from` から `to` まで。
    pub(crate) fn slice(&self, from: usize, to: usize) -> Piece<'a> {
        let (head, _) = self.split_at(to);
        head.split_at(from).1
    }

    pub(crate) fn skip(&self, n: usize) -> Piece<'a> {
        self.split_at(n).1
    }

    pub(crate) fn strip_prefix(&self, prefix: &str) -> Option<Piece<'a>> {
        self.text
            .strip_prefix(prefix)
            .map(|rest| Piece::new(rest, self.start + prefix.len()))
    }

    pub(crate) fn trim_start_by(&self, pred: fn(char) -> bool) -> Piece<'a> {
        let rest = self.text.trim_start_matches(pred);
        Piece::new(rest, self.start + (self.text.len() - rest.len()))
    }

    pub(crate) fn trim_end_by(&self, pred: fn(char) -> bool) -> Piece<'a> {
        Piece::new(self.text.trim_end_matches(pred), self.start)
    }

    pub(crate) fn trim_by(&self, pred: fn(char) -> bool) -> Piece<'a> {
        self.trim_start_by(pred).trim_end_by(pred)
    }

    pub(crate) fn first_char(&self) -> Option<char> {
        self.text.chars().next()
    }
}

/// 改行を除いた1行。
#[derive(Debug, Clone, Copy)]
pub(crate) struct SourceLine<'a> {
    pub(crate) piece: Piece<'a>,
}

impl SourceLine<'_> {
    pub(crate) fn span(&self) -> Span {
        self.piece.span()
    }
}

/// ソースを行に分ける。先頭のBOMは読み飛ばし、改行はLFとCRLFを受け付ける。
pub(crate) fn split_lines(source: &str) -> Vec<SourceLine<'_>> {
    let (body, offset) = match source.strip_prefix('\u{feff}') {
        Some(rest) => (rest, source.len() - rest.len()),
        None => (source, 0),
    };
    let mut lines = Vec::new();
    let mut start = offset;
    for segment in body.split_inclusive('\n') {
        let text = segment.strip_suffix('\n').unwrap_or(segment);
        let text = text.strip_suffix('\r').unwrap_or(text);
        lines.push(SourceLine {
            piece: Piece::new(text, start),
        });
        start += segment.len();
    }
    lines
}

/// 行末から切り出した行ID。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RawLineId {
    None,
    Valid(LineId),
    /// 形の合わない候補（E304）。候補は本文から取り除いてある。
    Malformed(LineId),
}

/// 台詞候補の1行目。
#[derive(Debug, Clone, Copy)]
pub(crate) struct DialogueStart<'a> {
    pub(crate) speaker: Piece<'a>,
    pub(crate) display_name: Option<Piece<'a>>,
    /// `ParseOptions::brackets` の何番目の括弧か。
    pub(crate) bracket: usize,
    /// 開き括弧の後ろから行の終わりまで。
    pub(crate) after_open: Piece<'a>,
}

/// シーン見出しの行の中身。
#[derive(Debug, Clone, Copy)]
pub(crate) struct HeadingLine<'a> {
    pub(crate) title: Piece<'a>,
    /// ` #` 以降。` #` が無ければ `None`。
    pub(crate) id: Option<Piece<'a>>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum LineKind<'a> {
    Blank,
    Comment,
    /// `# ` で始まる行。章見出しになるかは位置で決まる。
    Hash {
        title: Piece<'a>,
    },
    SceneHeading(HeadingLine<'a>),
    /// `---` だけの行。メタ情報の区切りになるかは位置で決まる。
    TripleDash,
    Directive {
        name: Piece<'a>,
        rest: Piece<'a>,
    },
    /// 選択肢の形の行。`rest` は `-` の後ろ。
    Option {
        rest: Piece<'a>,
    },
    Jump {
        target: Piece<'a>,
    },
    Dialogue(DialogueStart<'a>),
    Narration,
}

/// 分類した1行。
#[derive(Debug, Clone)]
pub(crate) struct Line<'a> {
    /// 改行を除いた行全体。
    pub(crate) span: Span,
    pub(crate) line_id: RawLineId,
    /// 行IDと行頭の空白を除いた残り（エスケープの `\` は含む）。
    pub(crate) body: Piece<'a>,
    /// `body` からエスケープの `\` を除いたもの。本文として使う。
    pub(crate) content: Piece<'a>,
    pub(crate) kind: LineKind<'a>,
}

/// 1行を分類する。
pub(crate) fn classify<'a>(line: SourceLine<'a>, brackets: &[DialogueBracket]) -> Line<'a> {
    let span = line.span();
    let trimmed = line.piece.trim_end_by(is_indent);

    // コメントの行からは行IDを切り出さない（D035）。
    let indented = trimmed.trim_start_by(is_indent);
    if indented.text.starts_with("//") {
        return Line {
            span,
            line_id: RawLineId::None,
            body: indented,
            content: indented,
            kind: LineKind::Comment,
        };
    }

    let (rest, line_id) = split_line_id(trimmed);
    let body = rest.trim_start_by(is_indent);

    if let Some(escaped) = body.strip_prefix("\\") {
        return Line {
            span,
            line_id,
            body,
            content: escaped,
            kind: LineKind::Narration,
        };
    }

    let kind = classify_body(body, brackets);
    Line {
        span,
        line_id,
        body,
        content: body,
        kind,
    }
}

fn classify_body<'a>(body: Piece<'a>, brackets: &[DialogueBracket]) -> LineKind<'a> {
    let text = body.text;
    if text.is_empty() {
        return LineKind::Blank;
    }
    if text.starts_with("//") {
        return LineKind::Comment;
    }
    if let Some(title) = body.strip_prefix("# ") {
        return LineKind::Hash { title };
    }
    if let Some(after) = body.strip_prefix("##")
        && after.text.starts_with(' ')
    {
        return LineKind::SceneHeading(split_heading(after));
    }
    if text == "---" {
        return LineKind::TripleDash;
    }
    if let Some(after_at) = body.strip_prefix("@") {
        let name_len = after_at.text.find(is_space).unwrap_or(after_at.text.len());
        let (name, rest) = after_at.split_at(name_len);
        return LineKind::Directive {
            name,
            rest: rest.trim_start_by(is_space),
        };
    }
    if text == "-" || text.starts_with("- ") {
        return LineKind::Option { rest: body.skip(1) };
    }
    if let Some(after) = body.strip_prefix("->") {
        return LineKind::Jump {
            target: after.trim_by(is_space),
        };
    }
    if let Some(start) = dialogue_start(body, brackets) {
        return LineKind::Dialogue(start);
    }
    LineKind::Narration
}

/// 行末の行IDを切り出す（仕様書9節）。`line` は行末の空白を取り除いたもの。
fn split_line_id(line: Piece<'_>) -> (Piece<'_>, RawLineId) {
    let without_id = line
        .text
        .trim_end_matches(|c: char| c.is_ascii_alphanumeric());
    let id_len = line.text.len() - without_id.len();
    if id_len == 0 {
        return (line, RawLineId::None);
    }
    let Some(before_caret) = without_id.strip_suffix('^') else {
        return (line, RawLineId::None);
    };
    if !before_caret.ends_with(' ') {
        return (line, RawLineId::None);
    }
    let caret_at = before_caret.len();
    let id = line.skip(caret_at + 1);
    let id_span = Span::new(line.start + caret_at, line.end());
    let rest = line.split_at(caret_at).0.trim_end_by(is_indent);
    let valid = (4..=12).contains(&id.text.len())
        && id
            .text
            .chars()
            .all(|c| c.is_ascii_digit() || c.is_ascii_lowercase());
    let id = LineId {
        value: id.text.to_owned(),
        span: id_span,
    };
    let line_id = if valid {
        RawLineId::Valid(id)
    } else {
        RawLineId::Malformed(id)
    };
    (rest, line_id)
}

/// シーン見出しの `##` の後ろを、タイトルとIDに分ける（仕様書5節）。
fn split_heading(after_hashes: Piece<'_>) -> HeadingLine<'_> {
    match after_hashes.text.rfind(" #") {
        Some(at) => HeadingLine {
            title: after_hashes.split_at(at).0.trim_by(is_indent),
            id: Some(after_hashes.skip(at + 2)),
        },
        None => HeadingLine {
            title: after_hashes.trim_by(is_indent),
            id: None,
        },
    }
}

fn is_bracket(c: char, brackets: &[DialogueBracket]) -> bool {
    c == '（' || c == '）' || brackets.iter().any(|b| b.open == c || b.close == c)
}

fn open_bracket_index(c: char, brackets: &[DialogueBracket]) -> Option<usize> {
    brackets.iter().position(|b| b.open == c)
}

/// 台詞候補の形に当てはまるかを見る（仕様書6節「台詞の判定」の1）。
fn dialogue_start<'a>(body: Piece<'a>, brackets: &[DialogueBracket]) -> Option<DialogueStart<'a>> {
    let mut count = 0;
    let mut stop = None;
    for (at, c) in body.text.char_indices() {
        if is_space(c) || is_bracket(c, brackets) {
            stop = Some((at, c));
            break;
        }
        count += 1;
        if count > MAX_SPEAKER_CHARS {
            return None;
        }
    }
    let (speaker_len, stop_char) = stop?;
    if count == 0 {
        return None;
    }
    let speaker = body.split_at(speaker_len).0;
    let after_speaker = body.skip(speaker_len);

    let (display_name, at_open) = if stop_char == '（' {
        let inner = after_speaker.skip('（'.len_utf8());
        let close_at = inner.text.find('）')?;
        let (name, after) = inner.split_at(close_at);
        (Some(name), after.skip('）'.len_utf8()))
    } else {
        (None, after_speaker)
    };

    let open = at_open.first_char()?;
    let bracket = open_bracket_index(open, brackets)?;
    Some(DialogueStart {
        speaker,
        display_name,
        bracket,
        after_open: at_open.skip(open.len_utf8()),
    })
}

/// 台詞の本文から閉じ括弧を探す。同じ種類の括弧は入れ子として数える。
///
/// `depth` は呼び出し前の深さ（開き括弧の直後なら1）。閉じ括弧が見つかれば、
/// その相対位置を返す。見つからなければ `depth` を更新して `None` を返す。
pub(crate) fn find_close(
    text: &str,
    bracket: &DialogueBracket,
    depth: &mut usize,
) -> Option<usize> {
    for (at, c) in text.char_indices() {
        if c == bracket.close {
            *depth = depth.saturating_sub(1);
            if *depth == 0 {
                return Some(at);
            }
        } else if c == bracket.open {
            *depth += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_brackets() -> Vec<DialogueBracket> {
        crate::syntax::ParseOptions::default().brackets
    }

    fn line_of(text: &str) -> Line<'_> {
        let lines = split_lines(text);
        classify(lines[0], &default_brackets())
    }

    #[test]
    fn splits_lf_crlf_and_bom() {
        let lines = split_lines("\u{feff}a\r\nb\nc");
        let texts: Vec<_> = lines.iter().map(|l| l.piece.text).collect();
        assert_eq!(texts, ["a", "b", "c"]);
        assert_eq!(lines[0].piece.start, 3);
        assert_eq!(lines[1].piece.start, 6);
        assert!(split_lines("").is_empty());
        assert_eq!(split_lines("a\n").len(), 1);
    }

    #[test]
    fn extracts_line_id() {
        let line = line_of("灯「やあ」 ^k3f9");
        assert!(matches!(&line.line_id, RawLineId::Valid(id) if id.value == "k3f9"));
        assert_eq!(line.body.text, "灯「やあ」");

        let line = line_of("灯「やあ」 ^k3f9 \t");
        assert!(matches!(&line.line_id, RawLineId::Valid(id) if id.value == "k3f9"));

        let line = line_of("顔 ^_^");
        assert_eq!(line.line_id, RawLineId::None);
        assert_eq!(line.body.text, "顔 ^_^");

        let line = line_of("空白なし^abcd");
        assert_eq!(line.line_id, RawLineId::None);

        let line = line_of("大文字 ^K3F9");
        assert!(matches!(line.line_id, RawLineId::Malformed(_)));
        assert_eq!(line.body.text, "大文字");
    }

    #[test]
    fn comment_keeps_line_id() {
        let line = line_of("  // 灯「やあ」 ^k3f9");
        assert!(matches!(line.kind, LineKind::Comment));
        assert_eq!(line.line_id, RawLineId::None);
    }

    #[test]
    fn detects_dialogue_candidates() {
        let line = line_of("灯（？？？）「誰？」");
        let LineKind::Dialogue(start) = line.kind else {
            panic!("台詞候補にならない");
        };
        assert_eq!(start.speaker.text, "灯");
        assert_eq!(start.display_name.map(|p| p.text), Some("？？？"));
        assert_eq!(start.after_open.text, "誰？」");

        assert!(matches!(line_of("　灯「やあ」").kind, LineKind::Narration));
        assert!(matches!(line_of("灯 「やあ」").kind, LineKind::Narration));
        assert!(matches!(line_of("灯『やあ』").kind, LineKind::Narration));
        let long = format!("{}「やあ」", "あ".repeat(21));
        assert!(matches!(line_of(&long).kind, LineKind::Narration));
        let max = format!("{}「やあ」", "あ".repeat(20));
        assert!(matches!(line_of(&max).kind, LineKind::Dialogue(_)));
    }

    #[test]
    fn classifies_symbols() {
        assert!(matches!(line_of("-").kind, LineKind::Option { .. }));
        assert!(matches!(line_of("- a").kind, LineKind::Option { .. }));
        assert!(matches!(line_of("-a").kind, LineKind::Narration));
        assert!(matches!(line_of("-> s1").kind, LineKind::Jump { .. }));
        assert!(matches!(line_of("---").kind, LineKind::TripleDash));
        assert!(matches!(line_of("----").kind, LineKind::Narration));
        assert!(matches!(
            line_of("## a #s1").kind,
            LineKind::SceneHeading(_)
        ));
        assert!(matches!(line_of("##").kind, LineKind::Narration));
        assert!(matches!(line_of("\\@背景").kind, LineKind::Narration));
    }

    #[test]
    fn finds_nested_close() {
        let bracket = &default_brackets()[0];
        let mut depth = 1;
        assert_eq!(
            find_close("a「b」c」d", bracket, &mut depth),
            Some("a「b」c".len())
        );
        let mut depth = 1;
        assert_eq!(find_close("a「b", bracket, &mut depth), None);
        assert_eq!(depth, 2);
    }
}
