//! AIの接続（T3-5、D066）：接続先の解決（`project.toml` の `[ai.*]` と端末の設定）、呼び出し（OpenAI 互換と
//! Anthropic の Messages API）、キーの保管（OS のキーチェーン）。
//! AIタスクの定義と文脈の組み立て（T3-6、D067）：`ai/*.yaml` の読み方、文脈の組み立てと削る順番、シーンの要約の
//! キャッシュ、設定DBのエントリが渡る内容。
//! AIタスクの実行（T3-7、D068）：指摘とシーンの要約を応答から読み、行IDで本文の位置を引く。
//! 提案（T3-8、D069）：行IDつきの差分（アプリの実行と `ai/proposals/*.md`）、行ごとの採用。

mod client;
mod connection;
mod context;
mod keys;
mod proposal;
mod run;
mod task;

pub use client::*;
pub use connection::*;
pub use context::*;
pub use keys::*;
pub use proposal::*;
pub use run::*;
pub use task::*;
