//! `.scn` の解析、意味解析、IR、診断を受け持つコア。
//!
//! モジュールの構成は `.claude/rules/rust.md` にある。中身はT0-3とT0-4で作る。

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
