//! AIの接続（T3-5、D066）：接続先の解決（`project.toml` の `[ai.*]` と端末の設定）、呼び出し（OpenAI 互換と
//! Anthropic の Messages API）、キーの保管（OS のキーチェーン）。
//! AIタスクの定義と文脈の組み立て（T3-6、D067）：`ai/*.yaml` の読み方、文脈の組み立てと削る順番、シーンの要約の
//! キャッシュ、設定DBのエントリが渡る内容。
//! AIタスクの実行（T3-7、D068）：指摘とシーンの要約を応答から読み、行IDで本文の位置を引く。
//! 提案（T3-8、D069）：行IDつきの差分（アプリの実行と `ai/proposals/*.md`）、行ごとの採用。
//! 生成と自律（T3-9、D070）：スタブへの下書き、自律の反復、Git のブランチ。
//! プロットとフローへの AI 支援（T3-10、D071）：提案ノード、ルートの骨格、構造の検証の結果、要約のずれ、メモの分類案。
//! 企画書と設定DBの下書き（T4-4、D080）：空いている項目にだけ書き、値のある項目は提案にする。
//! Claude Code（CLI）で呼ぶ（T4-26、N3）：Messages API の接続先を、この端末では `claude -p` で呼ぶ。

mod auto;
mod claude_code;
mod client;
mod connection;
mod context;
mod fielddraft;
mod generate;
mod keys;
mod plot;
mod proposal;
mod run;
mod task;

pub use auto::*;
pub use claude_code::{CLI_NAME, cli_args, cli_logged_in, cli_prompt, cli_version, find_cli};
pub use client::*;
pub use connection::*;
pub use context::*;
pub use fielddraft::*;
pub use generate::*;
pub use keys::*;
pub use plot::*;
pub use proposal::*;
pub use run::*;
pub use task::*;
