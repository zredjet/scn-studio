//! `.scn` の解析、意味解析、IR、診断を受け持つコア。
//!
//! モジュールの構成は `.claude/rules/rust.md` にある。T0-3では構文解析
//! （`line`、`syntax`、`inline`、`expr`）までを作る。意味解析とIRはT0-4で作る。

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
mod line;
pub mod syntax;

pub use diag::{Code, Diagnostic, Severity, Span, Spanned};
pub use syntax::{DialogueBracket, ParseOptions, Parsed, parse};
