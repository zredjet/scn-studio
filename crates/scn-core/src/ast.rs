//! 構文木（AST）。`syntax` が組み立てる。
//!
//! 設定DBを見ない段階の結果なので、台詞は「台詞候補」のまま持つ。台詞にするか
//! 地の文にするかは `sema` で決める（仕様書6節「台詞の判定」）。
//! どのノードも、元の位置（UTF-8のバイト位置の範囲）を持つ。

use crate::diag::{Span, Spanned};
use crate::expr::{Assignment, SourceExpr};
use crate::inline::Text;

/// 1つの `.scn` ファイル（1章）。
#[derive(Debug, Clone, PartialEq)]
pub struct Chapter {
    pub heading: Option<ChapterHeading>,
    pub scenes: Vec<Scene>,
}

/// 章見出し（ファイルの先頭の `# ` の行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChapterHeading {
    pub title: String,
    pub span: Span,
}

/// シーン。
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    /// 見出しの行。
    pub span: Span,
    pub title: String,
    /// シーンID。` #` が無いか（W107）、形が合わなければ（E111）`None`。
    pub id: Option<Spanned<String>>,
    pub meta: Option<SceneMeta>,
    pub body: Vec<Stmt>,
}

/// メタ情報（見出しの直後の `---` から `---` まで）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneMeta {
    /// 開きの `---` の行。
    pub span: Span,
    /// キーの重複は、後の値で前の値を置き換えてある（W113）。
    pub entries: Vec<MetaEntry>,
    /// 閉じの `---` があったか（無ければ E102）。
    pub closed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaEntry {
    pub key: Spanned<String>,
    pub value: Spanned<MetaValue>,
    /// 書かれたとおりの値（`[a, b]` の括弧も含む）。
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaValue {
    Text(String),
    /// `[a, b]` 形式のリスト。
    List(Vec<String>),
}

/// 行ID（`^k3f9`）。`span` は `^` から。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineId {
    pub value: String,
    pub span: Span,
}

/// 行IDを持てる行での、行IDの有無。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineIdSlot {
    /// 行IDが無い（`sema` で I302）。
    Absent,
    Valid(LineId),
    /// 形の合わない行IDがあった（E304）。I302 は出さない。
    Malformed,
}

/// シーンやブロックの中の1つの要素。
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Dialogue(Dialogue),
    Narration(Narration),
    /// 段落の区切り（本文の行に挟まれた空行）。
    Break(Span),
    Directive(Directive),
    Jump(Jump),
    Choice(Choice),
    If(IfBlock),
    Raw(Raw),
}

/// 台詞候補（`話者「本文」`、`話者（表示名）「本文」`）。
#[derive(Debug, Clone, PartialEq)]
pub struct Dialogue {
    /// 1行目から最後の行まで。
    pub span: Span,
    pub speaker: Spanned<String>,
    pub display_name: Option<Spanned<String>>,
    pub open: char,
    pub close: char,
    /// 表示スタイル（`project.toml` の `[dialogue.brackets]` の値）。
    pub style: String,
    /// 括弧の中の本文。複数行にまたがるときは、行ごとに分けて持つ。
    pub lines: Vec<Text>,
    /// 閉じ括弧があったか（無ければ E201）。
    pub closed: bool,
    /// 閉じ括弧の後ろに続く文（`彼は「行こう」と言った。` の `と言った。`）。
    pub trailing: Option<Text>,
    pub line_id: LineIdSlot,
}

/// 地の文。
#[derive(Debug, Clone, PartialEq)]
pub struct Narration {
    pub span: Span,
    pub text: Text,
    pub line_id: LineIdSlot,
}

/// ジャンプ（`-> シーンID`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Jump {
    pub span: Span,
    pub target: Spanned<String>,
}

/// ブロックではないディレクティブ。
#[derive(Debug, Clone, PartialEq)]
pub struct Directive {
    pub span: Span,
    /// 書かれたとおりの命令名（`背景`、`bg` など）。
    pub name: String,
    pub kind: DirectiveKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DirectiveKind {
    Bg {
        name: Spanned<String>,
        transition: Option<Spanned<String>>,
    },
    Show {
        character: Spanned<String>,
        expression: Spanned<String>,
        position: Option<Position>,
        transition: Option<Spanned<String>>,
    },
    Hide {
        character: Spanned<String>,
        transition: Option<Spanned<String>>,
    },
    Bgm {
        name: Spanned<String>,
    },
    BgmStop,
    Se {
        name: Spanned<String>,
    },
    Fx {
        name: Spanned<String>,
        seconds: Option<f64>,
    },
    Wait {
        seconds: f64,
    },
    Set {
        /// `@設定` の後ろに書かれた文字列。
        source: Spanned<String>,
        /// 構文が崩れていれば `None`（E601）。
        assignment: Option<Assignment>,
    },
    Call {
        target: Spanned<String>,
    },
    Return,
    Ending {
        /// 形が合わなければ `None`（E111）。
        id: Option<Spanned<String>>,
    },
    /// 知らないディレクティブ（W401）。フェーズ1でマクロとして解決する可能性がある。
    Unknown {
        args: Vec<Spanned<String>>,
    },
}

/// 立ち絵の位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    Left,
    Center,
    Right,
}

impl Position {
    pub fn as_str(self) -> &'static str {
        match self {
            Position::Left => "left",
            Position::Center => "center",
            Position::Right => "right",
        }
    }
}

/// 選択ブロック（`@選択` から `@end` まで）。
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    /// `@選択` の行。
    pub span: Span,
    pub prompt: Option<Text>,
    pub line_id: LineIdSlot,
    pub options: Vec<ChoiceOption>,
    /// `@end` の行。閉じていなければ `None`（E103）。
    pub end: Option<Span>,
}

/// 選択肢（`- 表示文 [if 式] [-> シーンID]`）と、その中身。
#[derive(Debug, Clone, PartialEq)]
pub struct ChoiceOption {
    /// 選択肢の行。
    pub span: Span,
    pub text: Text,
    pub cond: Option<SourceExpr>,
    /// 中身を実行してから飛ぶ先。
    pub jump: Option<Spanned<String>>,
    pub line_id: LineIdSlot,
    pub body: Vec<Stmt>,
}

/// 条件分岐（`@if` から `@end` まで）。
#[derive(Debug, Clone, PartialEq)]
pub struct IfBlock {
    /// `@if` の行。
    pub span: Span,
    /// `@if` と `@elif` の枝。最初の要素が `@if`。
    pub branches: Vec<CondBranch>,
    pub else_branch: Option<ElseBranch>,
    /// `@end` の行。閉じていなければ `None`（E103）。
    pub end: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CondBranch {
    /// `@if` または `@elif` の行。
    pub span: Span,
    pub cond: SourceExpr,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ElseBranch {
    /// `@else` の行。
    pub span: Span,
    pub body: Vec<Stmt>,
}

/// 生コード（`@raw` から `@end` まで）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raw {
    /// `@raw` の行。
    pub span: Span,
    pub engine: String,
    /// 共通の字下げを取り除いた行。空白だけの行は空文字列。
    pub lines: Vec<String>,
    /// `@end` の行。閉じていなければ `None`（E103）。
    pub end: Option<Span>,
}
