//! 文中の記法（仕様書8節）。
//!
//! 台詞の本文、地の文、選択肢の表示文、`@選択` の問いかけ文で使う。
//! 記法として読めないものは、診断を出すか（W405）、そのまま文字として扱う。

use crate::diag::{Code, Diagnostic, Span};
use crate::expr::{SourceExpr, parse_condition};
use crate::line::{Piece, is_space};

/// 文中の記法を解析した文。
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    pub span: Span,
    pub parts: Vec<Inline>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    /// ただの文字。`{{`・`}}`・`|《` は、`{`・`}`・`《` にしてある。
    Text { text: String, span: Span },
    /// ルビ（`|親文字《るび》`、`｜親文字《るび》`、`漢字《かんじ》`）
    Ruby {
        base: String,
        reading: String,
        span: Span,
    },
    /// 傍点（`《《強調》》`）
    Emphasis { text: String, span: Span },
    /// ウェイト（`{w 0.5}`）。秒。
    Wait { seconds: f64, span: Span },
    /// 改ページ（`{p}`）
    PageBreak { span: Span },
    /// 文中の効果音（`{se ドア}`）
    Se { name: String, span: Span },
    /// 文字速度（`{speed 遅}`）
    Speed { speed: TextSpeed, span: Span },
    /// 変数の埋め込み（`{=湊.名前}`）
    Interpolation { expr: SourceExpr, span: Span },
}

/// 文字速度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextSpeed {
    /// `遅`
    Slow,
    /// `普通`
    Normal,
    /// `速`
    Fast,
}

impl TextSpeed {
    pub fn as_str(self) -> &'static str {
        match self {
            TextSpeed::Slow => "slow",
            TextSpeed::Normal => "normal",
            TextSpeed::Fast => "fast",
        }
    }
}

/// 自動ルビの親文字になる文字（漢字と `々〆ヵヶ`）。
fn is_kanji(c: char) -> bool {
    matches!(c,
        '\u{3400}'..='\u{4DBF}'
        | '\u{4E00}'..='\u{9FFF}'
        | '\u{F900}'..='\u{FAFF}'
        | '\u{20000}'..='\u{3FFFF}'
        | '々' | '〆' | 'ヵ' | 'ヶ')
}

/// 秒の書き方（半角の数。`1`、`0.5`）。
pub(crate) fn parse_seconds(text: &str) -> Option<f64> {
    let (int, frac) = match text.split_once('.') {
        Some((int, frac)) => (int, Some(frac)),
        None => (text, None),
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(int) || frac.is_some_and(|f| !digits(f)) {
        return None;
    }
    text.parse().ok()
}

/// 文中の記法を解析する。
pub(crate) fn parse_text(source: Piece<'_>, diags: &mut Vec<Diagnostic>) -> Text {
    let mut builder = Builder {
        parts: Vec::new(),
        buf: String::new(),
        buf_start: source.start,
    };
    let text = source.text;
    let mut at = 0;
    while let Some(c) = text.get(at..).and_then(|rest| rest.chars().next()) {
        let rest = source.skip(at);
        let next = rest.text.chars().nth(1);
        let pos = source.start + at;
        match c {
            '{' if next == Some('{') => {
                builder.push_char('{', pos);
                at += 2;
            }
            '}' if next == Some('}') => {
                builder.push_char('}', pos);
                at += 2;
            }
            '{' => match rest.text.find('}') {
                Some(close) => {
                    let whole = rest.split_at(close + 1).0;
                    let inner = rest.slice(1, close);
                    match command(inner, whole.span(), diags) {
                        Some(inline) => {
                            builder.flush(pos);
                            builder.parts.push(inline);
                            builder.buf_start = whole.end();
                        }
                        None => builder.push_str(whole.text, pos),
                    }
                    at += close + 1;
                }
                None => {
                    builder.push_char('{', pos);
                    at += 1;
                }
            },
            '|' | '｜' => {
                let after_bar = rest.skip(c.len_utf8());
                if after_bar.text.starts_with('《') {
                    builder.push_char('《', pos);
                    at += c.len_utf8() + '《'.len_utf8();
                } else if let Some((base, reading, end)) = explicit_ruby(after_bar) {
                    builder.flush(pos);
                    builder.parts.push(Inline::Ruby {
                        base: base.to_owned(),
                        reading: reading.to_owned(),
                        span: Span::new(pos, end),
                    });
                    builder.buf_start = end;
                    at = end - source.start;
                } else {
                    builder.push_char(c, pos);
                    at += c.len_utf8();
                }
            }
            '《' if next == Some('《') => {
                let inner = rest.skip('《'.len_utf8() * 2);
                match inner.text.find("》》") {
                    Some(close) if close > 0 => {
                        let end = inner.start + close + "》》".len();
                        builder.flush(pos);
                        builder.parts.push(Inline::Emphasis {
                            text: inner.split_at(close).0.text.to_owned(),
                            span: Span::new(pos, end),
                        });
                        builder.buf_start = end;
                        at = end - source.start;
                    }
                    _ => {
                        builder.push_char('《', pos);
                        at += '《'.len_utf8();
                    }
                }
            }
            '《' => {
                let base_len: usize = builder
                    .buf
                    .chars()
                    .rev()
                    .take_while(|&k| is_kanji(k))
                    .map(char::len_utf8)
                    .sum();
                match reading(rest.skip('《'.len_utf8())) {
                    Some((reading, end)) if base_len > 0 => {
                        let base_start = pos - base_len;
                        let base = builder.take_tail(base_len, base_start);
                        builder.parts.push(Inline::Ruby {
                            base,
                            reading: reading.to_owned(),
                            span: Span::new(base_start, end),
                        });
                        builder.buf_start = end;
                        at = end - source.start;
                    }
                    _ => {
                        builder.push_char('《', pos);
                        at += '《'.len_utf8();
                    }
                }
            }
            _ => {
                builder.push_char(c, pos);
                at += c.len_utf8();
            }
        }
    }
    builder.flush(source.end());
    Text {
        span: source.span(),
        parts: builder.parts,
    }
}

/// `|` の後ろから、`親文字《るび》` を読む。終わりの位置（絶対位置）も返す。
fn explicit_ruby(after_bar: Piece<'_>) -> Option<(&str, &str, usize)> {
    let open = after_bar.text.find('《')?;
    let base = after_bar.split_at(open).0.text;
    if base.is_empty() || base.contains(['|', '｜', '》']) {
        return None;
    }
    let (reading, end) = reading(after_bar.skip(open + '《'.len_utf8()))?;
    Some((base, reading, end))
}

/// `《` の後ろから、`るび》` を読む。終わりの位置（絶対位置）も返す。
fn reading(after_open: Piece<'_>) -> Option<(&str, usize)> {
    let close = after_open.text.find('》')?;
    let reading = after_open.split_at(close).0.text;
    if reading.is_empty() || reading.contains('《') {
        return None;
    }
    Some((reading, after_open.start + close + '》'.len_utf8()))
}

/// `{…}` の中身を、インライン命令として読む。読めなければ W405 を出して `None`。
fn command(inner: Piece<'_>, whole: Span, diags: &mut Vec<Diagnostic>) -> Option<Inline> {
    if let Some(expr) = inner.strip_prefix("=") {
        return Some(Inline::Interpolation {
            expr: parse_condition(expr.trim_by(is_space), diags),
            span: whole,
        });
    }
    let mut words = inner.text.split(is_space).filter(|w| !w.is_empty());
    let name = words.next().unwrap_or_default();
    let args: Vec<&str> = words.collect();
    let known = matches!(name, "p" | "w" | "se" | "speed");
    let inline = match (name, args.as_slice()) {
        ("p", []) => Some(Inline::PageBreak { span: whole }),
        ("w", [seconds]) => parse_seconds(seconds).map(|seconds| Inline::Wait {
            seconds,
            span: whole,
        }),
        ("se", [name]) => Some(Inline::Se {
            name: (*name).to_owned(),
            span: whole,
        }),
        ("speed", [speed]) => {
            let speed = match *speed {
                "遅" => Some(TextSpeed::Slow),
                "普通" => Some(TextSpeed::Normal),
                "速" => Some(TextSpeed::Fast),
                _ => None,
            };
            speed.map(|speed| Inline::Speed { speed, span: whole })
        }
        _ => None,
    };
    if inline.is_none() {
        let message = if known {
            format!("インライン命令の値が合いません：{{{}}}", inner.text)
        } else {
            format!("知らないインライン命令です：{{{}}}", inner.text)
        };
        diags.push(Diagnostic::new(Code::W405, whole, message));
    }
    inline
}

struct Builder {
    parts: Vec<Inline>,
    buf: String,
    /// `buf` の先頭の位置。
    buf_start: usize,
}

impl Builder {
    fn push_char(&mut self, c: char, pos: usize) {
        if self.buf.is_empty() {
            self.buf_start = pos;
        }
        self.buf.push(c);
    }

    fn push_str(&mut self, s: &str, pos: usize) {
        if self.buf.is_empty() {
            self.buf_start = pos;
        }
        self.buf.push_str(s);
    }

    /// たまった文字を、`end` までの文として出す。
    fn flush(&mut self, end: usize) {
        if self.buf.is_empty() {
            return;
        }
        self.parts.push(Inline::Text {
            text: std::mem::take(&mut self.buf),
            span: Span::new(self.buf_start, end),
        });
    }

    /// たまった文字の末尾 `len` バイトを取り出し、残りを `tail_start` までの文として出す。
    fn take_tail(&mut self, len: usize, tail_start: usize) -> String {
        let keep = self.buf.len().saturating_sub(len);
        let tail = self.buf.split_off(keep);
        self.flush(tail_start);
        tail
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> (Vec<Inline>, Vec<Diagnostic>) {
        let mut diags = Vec::new();
        let parsed = parse_text(Piece::new(text, 0), &mut diags);
        (parsed.parts, diags)
    }

    fn texts(parts: &[Inline]) -> Vec<String> {
        parts
            .iter()
            .map(|p| match p {
                Inline::Text { text, .. } => format!("text:{text}"),
                Inline::Ruby { base, reading, .. } => format!("ruby:{base}/{reading}"),
                Inline::Emphasis { text, .. } => format!("emph:{text}"),
                Inline::Wait { seconds, .. } => format!("wait:{seconds}"),
                Inline::PageBreak { .. } => "page".into(),
                Inline::Se { name, .. } => format!("se:{name}"),
                Inline::Speed { speed, .. } => format!("speed:{}", speed.as_str()),
                Inline::Interpolation { expr, .. } => format!("interp:{}", expr.text),
            })
            .collect()
    }

    #[test]
    fn ruby_forms() {
        let (parts, diags) = parse("髪が|乱《みだ》れ、漢字《かんじ》と佐々木《ささき》");
        assert!(diags.is_empty());
        assert_eq!(
            texts(&parts),
            [
                "text:髪が",
                "ruby:乱/みだ",
                "text:れ、",
                "ruby:漢字/かんじ",
                "text:と",
                "ruby:佐々木/ささき"
            ]
        );
        let (parts, _) = parse("ひらがな《るび》|《》|x");
        assert_eq!(texts(&parts), ["text:ひらがな《るび》《》|x"]);
    }

    #[test]
    fn spans_point_to_source() {
        let src = "あ|乱《みだ》い{w 0.5}";
        let (parts, _) = parse(src);
        let spans: Vec<&str> = parts
            .iter()
            .map(|p| match p {
                Inline::Text { span, .. }
                | Inline::Ruby { span, .. }
                | Inline::Wait { span, .. } => &src[span.start..span.end],
                _ => "",
            })
            .collect();
        assert_eq!(spans, ["あ", "|乱《みだ》", "い", "{w 0.5}"]);

        let src = "あ漢字《かんじ》";
        let (parts, _) = parse(src);
        let Inline::Ruby { span, .. } = &parts[1] else {
            panic!()
        };
        assert_eq!(&src[span.start..span.end], "漢字《かんじ》");
    }

    #[test]
    fn commands_and_escapes() {
        let (parts, diags) = parse("{{a}}{p}{se ドア}{speed 速}{=x}{shake}{w 長め}{ 閉じない");
        assert_eq!(
            texts(&parts),
            [
                "text:{a}",
                "page",
                "se:ドア",
                "speed:fast",
                "interp:x",
                "text:{shake}{w 長め}{ 閉じない"
            ]
        );
        let codes: Vec<_> = diags.iter().map(|d| d.code).collect();
        assert_eq!(codes, [Code::W405, Code::W405]);
    }

    #[test]
    fn seconds() {
        assert_eq!(parse_seconds("1"), Some(1.0));
        assert_eq!(parse_seconds("0.5"), Some(0.5));
        for bad in ["", ".5", "1.", "-1", "１", "1e3", "長め"] {
            assert_eq!(parse_seconds(bad), None, "{bad}");
        }
    }
}
