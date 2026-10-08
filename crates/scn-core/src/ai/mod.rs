//! AIの接続（T3-5、D066）：接続先の解決（`project.toml` の `[ai.*]` と端末の設定）、呼び出し（OpenAI 互換と
//! Anthropic の Messages API）、キーの保管（OS のキーチェーン）。
//! AIタスクの定義と文脈の組み立て（T3-6、D067）：`ai/*.yaml` の読み方、文脈の組み立てと削る順番、シーンの要約の
//! キャッシュ、設定DBのエントリが渡る内容。

mod client;
mod connection;
mod context;
mod keys;
mod task;

pub use client::*;
pub use connection::*;
pub use context::*;
pub use keys::*;
pub use task::*;
