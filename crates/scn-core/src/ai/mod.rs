//! AIの接続（T3-5、D066）：接続先の解決（`project.toml` の `[ai.*]` と端末の設定）、呼び出し（OpenAI 互換と
//! Anthropic の Messages API）、キーの保管（OS のキーチェーン）。

mod client;
mod connection;
mod keys;

pub use client::*;
pub use connection::*;
pub use keys::*;
