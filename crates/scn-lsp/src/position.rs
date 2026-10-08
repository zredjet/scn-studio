//! 位置の変換（LSP との境界の1か所）：コアの UTF-8 のバイト位置と、LSP の位置（行と、行の中の UTF-16 の
//! 単位の数）。行の区切りは `\n`。行の中の位置は改行（`\r\n` の `\r` を含む）を数えない（行の外は行末に寄せる）。
//! 本文の先頭の BOM（U+FEFF）は、コアはバイト位置に数えるが、エディタは文書の中身から外すので、幅0として扱う。
//! ファイルごとに行頭の表（[`Lines`]）を一度作り、位置ごとに二分探索で引く。

use tower_lsp_server::ls_types::{Position, Range};

use scn_core::Span;

const BOM: char = '\u{feff}';

/// 本文と、行頭のバイト位置の表。
pub struct Lines<'a> {
    text: &'a str,
    starts: Vec<usize>,
}

impl<'a> Lines<'a> {
    pub fn new(text: &'a str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        Self { text, starts }
    }

    /// 行の中身の範囲（改行と `\r` を除く。0行目は BOM を除く）。
    fn line(&self, line: usize) -> Option<(usize, usize)> {
        let start = *self.starts.get(line)?;
        let next = self.starts.get(line + 1).map_or(self.text.len(), |n| n - 1);
        let body = self.text.get(start..next).unwrap_or_default();
        let end = start + body.strip_suffix('\r').unwrap_or(body).len();
        let start = if line == 0 && self.text.starts_with(BOM) {
            BOM.len_utf8().min(end)
        } else {
            start
        };
        Some((start, end))
    }

    /// バイト位置を LSP の位置にする。文字の途中や本文の外は、手前の文字の境目に寄せる。
    pub fn position(&self, offset: usize) -> Position {
        let offset = floor_char_boundary(self.text, offset.min(self.text.len()));
        let line = self
            .starts
            .partition_point(|s| *s <= offset)
            .saturating_sub(1);
        let (start, end) = self.line(line).unwrap_or((offset, offset));
        let column: usize = self
            .text
            .get(start.min(offset)..offset.min(end).max(start.min(offset)))
            .unwrap_or_default()
            .chars()
            .map(char::len_utf16)
            .sum();
        Position::new(to_u32(line), to_u32(column))
    }

    /// LSP の位置をバイト位置にする。行の外は行末（改行の前）に、本文の外は本文の終わりに寄せる。
    /// サロゲートペアの途中は、その文字の前に寄せる。
    pub fn offset(&self, position: Position) -> usize {
        let Some((start, end)) = self.line(position.line as usize) else {
            return self.text.len();
        };
        let mut units = 0usize;
        for (i, c) in self.text.get(start..end).unwrap_or_default().char_indices() {
            let next = units + c.len_utf16();
            if next > position.character as usize {
                return start + i;
            }
            units = next;
        }
        end
    }

    /// スパンを LSP の範囲にする。
    pub fn range(&self, span: Span) -> Range {
        Range::new(self.position(span.start), self.position(span.end))
    }
}

/// バイト位置を LSP の位置にする（1回だけ使うとき）。
pub fn position(text: &str, offset: usize) -> Position {
    Lines::new(text).position(offset)
}

/// LSP の位置をバイト位置にする（1回だけ使うとき）。
pub fn offset(text: &str, position: Position) -> usize {
    Lines::new(text).offset(position)
}

/// スパンを LSP の範囲にする（1回だけ使うとき）。
pub fn range(text: &str, span: Span) -> Range {
    Lines::new(text).range(span)
}

fn floor_char_boundary(text: &str, mut offset: usize) -> usize {
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn to_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_full_width_and_surrogate_pairs() {
        // 「あ」は UTF-8 で3バイト・UTF-16 で1単位、「😀」は4バイト・2単位。
        let text = "ab\nあ😀c\n";
        let cases = [
            (0, (0, 0)),
            (2, (0, 2)),
            (3, (1, 0)),
            (6, (1, 1)),  // 「あ」の後ろ
            (10, (1, 3)), // 「😀」の後ろ
            (11, (1, 4)), // 「c」の後ろ
            (12, (2, 0)),
        ];
        for (offset, (line, character)) in cases {
            assert_eq!(
                position(text, offset),
                Position::new(line, character),
                "{offset}"
            );
            assert_eq!(
                offset_of(text, line, character),
                offset,
                "{line}:{character}"
            );
        }
    }

    fn offset_of(text: &str, line: u32, character: u32) -> usize {
        offset(text, Position::new(line, character))
    }

    #[test]
    fn out_of_range_and_inside_characters() {
        let text = "あ😀\r\nx";
        // 文字の途中のバイト位置は、手前の境目に寄せる。
        assert_eq!(position(text, 1), Position::new(0, 0));
        assert_eq!(position(text, 5), Position::new(0, 1));
        // サロゲートペアの途中の位置は、その文字の前に寄せる。
        assert_eq!(offset_of(text, 0, 2), 3);
        // 行の外は行末（`\r` の前）、本文の外は本文の終わり。
        assert_eq!(offset_of(text, 0, 99), 7);
        assert_eq!(offset_of(text, 9, 0), text.len());
        assert_eq!(position(text, 999), Position::new(1, 1));
        assert_eq!(
            range(text, Span::new(3, 7)),
            Range::new(Position::new(0, 1), Position::new(0, 3))
        );
    }

    #[test]
    fn bom_has_no_width() {
        // コアは BOM（3バイト）を位置に数えるが、エディタの文書には無い。
        let text = "\u{feff}## 前日 #s001\nx";
        let id = text.find("s001").unwrap();
        assert_eq!(position(text, id), Position::new(0, 7));
        assert_eq!(offset_of(text, 0, 7), id);
        assert_eq!(position(text, 0), Position::new(0, 0));
        assert_eq!(position(text, 3), Position::new(0, 0));
        assert_eq!(offset_of(text, 0, 0), 3);
        assert_eq!(position(text, text.len()), Position::new(1, 1));
    }

    #[test]
    fn many_positions_use_one_table() {
        let text = "あ\n😀\r\nb";
        let lines = Lines::new(text);
        assert_eq!(lines.position(4), Position::new(1, 0));
        assert_eq!(lines.position(8), Position::new(1, 2));
        // `\r` の位置は行の中身の終わり。
        assert_eq!(lines.position(9), Position::new(1, 2));
        assert_eq!(lines.position(10), Position::new(2, 0));
        assert_eq!(lines.offset(Position::new(1, 2)), 8);
    }
}
