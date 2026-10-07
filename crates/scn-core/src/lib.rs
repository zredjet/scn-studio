//! `.scn` の解析、意味解析、IR、診断を受け持つコア。
//!
//! モジュールの構成は `.claude/rules/rust.md` にある。
//!
//! - 1つのファイルの構文解析：[`parse`]（設定DBは見ない）
//! - プロジェクトの読み込み：[`load_project`]
//! - プロジェクト全体の意味解析とIR：[`analyze`]

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

pub mod ast;
pub mod diag;
pub mod expr;
pub mod inline;
pub mod ir;
mod line;
pub mod project;
mod sema;
pub mod syntax;

pub use diag::{Code, Diagnostic, Severity, Span, Spanned};
pub use ir::{Ir, analyze};
pub use project::{Project, ProjectError, load_project};
pub use syntax::{DialogueBracket, ParseOptions, Parsed, parse};
