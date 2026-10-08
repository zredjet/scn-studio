//! `.scn` の解析、意味解析、IR、診断を受け持つコア。
//!
//! モジュールの構成は `.claude/rules/rust.md` にある。
//!
//! - 1つのファイルの構文解析：[`parse`]（設定DBは見ない）
//! - プロジェクトの読み込み：[`load_project`]
//! - プロジェクト全体の意味解析とIR：[`analyze`]
//! - 索引（補完、逆引き、検証が使う一覧）：[`index::build_index`]
//! - 検証の段階1（`scn check`）：[`check::check`]
//! - 構造の一覧（`scn graph`）：[`graph::build_graph`]
//! - 作品フォルダのひな形（`scn new`、`scn format-doc`）：[`scaffold`]
//! - 開いている作品フォルダ（アプリの保存と、外部の変更の取り込み）：[`workspace::Workspace`]
//! - 設定DBと変数の画面（項目の型と値の検査、フロントマターの書き換え、逆引き）：[`bible`]
//! - 簡易ADVプレビューのデータ（IR全体と、画面・変数・素材）：[`preview::preview_data`]
//! - 台本の出力（`scn export script`）：[`script::script`]
//! - 行IDの付与（`scn fmt`）：[`line_id::assign_line_ids`]

// ライブラリのコードでは、パニックしうる書き方を禁じる（`.claude/rules/rust.md`）。
// テスト（`cfg(test)`）では使ってよい。
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::string_slice
    )
)]

pub mod assets;
pub mod assist;
pub mod ast;
pub mod bible;
pub mod check;
pub mod concept;
pub mod diag;
pub mod expr;
pub mod flowedit;
pub mod graph;
pub mod ideas;
pub mod index;
pub mod inline;
pub mod ir;
pub mod layout;
mod line;
pub mod line_id;
pub mod merge;
pub mod plaintext;
pub mod preview;
pub mod project;
pub mod rename;
pub mod renpy;
pub mod route;
pub mod scaffold;
pub mod script;
mod sema;
pub mod syntax;
pub mod typedef;
pub mod vardecl;
pub mod workspace;
mod xlsx;

pub use diag::{Code, Diagnostic, LineIndex, Severity, Span, Spanned};
pub use ir::{Ir, analyze};
pub use project::{Project, ProjectError, load_project, write_source};
pub use syntax::{DialogueBracket, ParseOptions, Parsed, parse};
