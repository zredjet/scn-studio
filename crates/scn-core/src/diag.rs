//! 診断（コード、重大度、位置、メッセージ）。
//!
//! コードは仕様書（`docs/spec/scn-format-v0.md`）16節の診断表と一致させる。
//! ここには、実装済みの段階（段階0）で出すコードだけを置く。

use std::fmt;

use serde::{Serialize, Serializer};

/// ソース中の範囲。UTF-8のバイト位置で、`start` を含み `end` を含まない。
///
/// 位置は、`parse` に渡した文字列の先頭から数える（先頭のBOMも数に入れる）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// 2つの範囲を覆う範囲。
    pub fn cover(self, other: Span) -> Span {
        Span::new(self.start.min(other.start), self.end.max(other.end))
    }
}

/// JSONでは `[start, end]` の形にする。
impl Serialize for Span {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        [self.start, self.end].serialize(serializer)
    }
}

/// バイト位置から、行と列を求める。行と列は1から数え、列は文字（Unicodeのスカラー値）の数。
/// 先頭のBOMは列に数えない。
#[derive(Debug, Clone)]
pub struct LineIndex<'a> {
    source: &'a str,
    /// 各行の先頭のバイト位置。
    starts: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    pub fn new(source: &'a str) -> Self {
        let mut starts = vec![0];
        starts.extend(
            source
                .bytes()
                .enumerate()
                .filter(|&(_, b)| b == b'\n')
                .map(|(at, _)| at + 1),
        );
        Self { source, starts }
    }

    /// `offset` の行と列。範囲の外の位置は、ファイルの終わりとして扱う。
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.source.len());
        let line = self.starts.partition_point(|&start| start <= offset).max(1);
        let start = self.starts.get(line - 1).copied().unwrap_or(0);
        let column = match self.source.get(start..offset) {
            Some(text) => {
                let chars = text.chars().count();
                if start == 0 && text.starts_with('\u{feff}') {
                    chars - 1
                } else {
                    chars
                }
            }
            None => offset - start,
        };
        (line, column + 1)
    }
}

/// 位置つきの値。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spanned<T> {
    pub value: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(value: T, span: Span) -> Self {
        Self { value, span }
    }
}

/// 重大度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
}

/// 診断コード。先頭の文字が重大度を表す（E：エラー、W：警告、I：情報）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Code {
    /// 最初のシーンより前に本文がある
    W101,
    /// メタ情報の `---` が閉じていない
    E102,
    /// ブロックが `@end` で閉じていない
    E103,
    /// 対応するブロックの無い `@elif`・`@else`・`@end`
    E104,
    /// 選択ブロックの外にある選択肢の行
    E105,
    /// 選択肢が1つも無い `@選択`
    E106,
    /// シーン見出しにIDが無い
    W107,
    /// シーンIDの重複
    E108,
    /// 知らないメタ情報のキー
    I109,
    /// `status` の値が不正
    E110,
    /// シーンID・エンディングIDの形が不正
    E111,
    /// ファイルの先頭以外にある `# ` の行
    W112,
    /// メタ情報のキーの重複
    W113,
    /// メタ情報の行の形が不正
    E114,
    /// 台詞の閉じ括弧が無い
    E201,
    /// 台詞の形だが、話者を解決できない
    W202,
    /// 行IDの重複
    E301,
    /// 行IDが無い
    I302,
    /// 行IDを持てない行に行IDがある
    W303,
    /// 行IDの形が不正
    E304,
    /// 知らないディレクティブ
    W401,
    /// ディレクティブの引数の数、または選択肢の形が合わない
    E402,
    /// 素材カタログに無い意味名
    I403,
    /// `@立ち絵`・`@退場` のキャラを解決できない
    W404,
    /// 知らないインライン命令、または値の合わないインライン命令
    W405,
    /// 存在しないシーンへのジャンプ・呼び出し
    E501,
    /// ジャンプ・`@エンディング`・`@戻る` の後にある、実行されない行
    W502,
    /// 式の構文エラー
    E601,
    /// 宣言されていない変数
    W602,
    /// 型の不一致
    E603,
    /// `+=`・`-=` を整数以外に使った
    E604,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::W101 => "W101",
            Code::E102 => "E102",
            Code::E103 => "E103",
            Code::E104 => "E104",
            Code::E105 => "E105",
            Code::E106 => "E106",
            Code::W107 => "W107",
            Code::E108 => "E108",
            Code::I109 => "I109",
            Code::E110 => "E110",
            Code::E111 => "E111",
            Code::W112 => "W112",
            Code::W113 => "W113",
            Code::E114 => "E114",
            Code::E201 => "E201",
            Code::W202 => "W202",
            Code::E301 => "E301",
            Code::I302 => "I302",
            Code::W303 => "W303",
            Code::E304 => "E304",
            Code::W401 => "W401",
            Code::E402 => "E402",
            Code::I403 => "I403",
            Code::W404 => "W404",
            Code::W405 => "W405",
            Code::E501 => "E501",
            Code::W502 => "W502",
            Code::E601 => "E601",
            Code::W602 => "W602",
            Code::E603 => "E603",
            Code::E604 => "E604",
        }
    }

    pub fn severity(self) -> Severity {
        match self.as_str().as_bytes().first() {
            Some(b'E') => Severity::Error,
            Some(b'W') => Severity::Warning,
            _ => Severity::Info,
        }
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 1つの診断。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: Code,
    pub span: Span,
    pub message: String,
}

impl Diagnostic {
    pub fn new(code: Code, span: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            span,
            message: message.into(),
        }
    }

    pub fn severity(&self) -> Severity {
        self.code.severity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col() {
        let index = LineIndex::new("ab\n灯「や」\r\nc");
        assert_eq!(index.line_col(0), (1, 1));
        assert_eq!(index.line_col(2), (1, 3));
        assert_eq!(index.line_col(3), (2, 1));
        assert_eq!(index.line_col(6), (2, 2));
        assert_eq!(index.line_col("ab\n灯「や」\r\n".len()), (3, 1));
        assert_eq!(index.line_col(1000), (3, 2));
        // 文字の途中の位置でもパニックしない。
        assert_eq!(index.line_col(4).0, 2);

        let bom = LineIndex::new("\u{feff}ab");
        assert_eq!(bom.line_col(3), (1, 1));
        assert_eq!(bom.line_col(4), (1, 2));
    }
}
