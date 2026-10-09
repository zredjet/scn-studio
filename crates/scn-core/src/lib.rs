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
//! - AIの接続（接続先の解決、呼び出し、キーの保管）：[`ai`]

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

pub mod ai;
pub mod assets;
pub mod assist;
pub mod ast;
pub mod bible;
pub mod book;
pub mod check;
pub mod complete;
pub mod concept;
pub mod diag;
pub mod expr;
pub mod flowedit;
pub mod gitops;
pub mod graph;
pub mod ideas;
pub mod index;
pub mod inline;
pub mod ir;
pub mod layout;
mod line;
pub mod line_id;
pub mod merge;
pub mod navigate;
pub mod plaintext;
pub mod playthrough;
pub mod plugin;
pub mod preview;
pub mod project;
pub mod provenance;
pub mod rename;
pub mod renpy;
pub mod route;
pub mod scaffold;
pub mod script;
mod sema;
pub mod syntax;
pub mod typedef;
pub mod tyrano;
pub mod utage;
pub mod vardecl;
pub mod wording;
pub mod workspace;
mod xlsx;

pub use diag::{Code, Diagnostic, LineIndex, Severity, Span, Spanned};
pub use ir::{ChapterTitle, Ir, analyze, analyze_with_chapters};
pub use project::{Project, ProjectError, load_project, write_source};
pub use syntax::{DialogueBracket, ParseOptions, Parsed, parse};
