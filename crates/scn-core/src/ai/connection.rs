//! 接続先の解決（T3-5、D066）：作品フォルダの `project.toml` の `[ai.<名前>]` に、端末の設定（URL・モデルの
//! 上書きと、クラウドへの送信の許可）を重ね、呼び出せる形にする。

use std::collections::{BTreeMap, BTreeSet};
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::Path;

use reqwest::Url;
use serde::{Deserialize, Serialize};

use super::client::{missing_path_hint, request_preview, test_request};
use super::context::TokenBudget;
use super::keys::KeyStore;
use super::task::AiTask;
use crate::project::AiConnectionDecl;

/// OpenAI 互換の API（チャットの補完）。Ollama、LM Studio、llama.cpp のサーバー、互換のクラウドをまとめて扱う。
pub const OPENAI_COMPATIBLE: &str = "openai-compatible";
/// Anthropic の Messages API（ネイティブのアダプタ）。
pub const ANTHROPIC: &str = "anthropic";
/// `provider = "anthropic"` で `base_url` を書かないときの URL。
pub const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com/v1";
/// 文脈の長さ（`context_tokens`）を書かないときの既定（OpenAI 互換。ローカルの LLM の短い文脈でも動くように）。
pub const DEFAULT_CONTEXT_TOKENS: u64 = 8192;
/// 文脈の長さを書かないときの既定（Messages API）。
pub const ANTHROPIC_CONTEXT_TOKENS: u64 = 200_000;
/// 既定の接続先の名前（設計検討書「接続」：既定はローカル）。
pub const DEFAULT_CONNECTION: &str = "local";
/// 端末の設定のファイル名（アプリの設定のフォルダに置く。作品フォルダには置かない）。
pub const MACHINE_SETTINGS_FILE: &str = "ai.toml";

/// 接続の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    OpenaiCompatible,
    Anthropic,
}

impl Provider {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            OPENAI_COMPATIBLE => Some(Self::OpenaiCompatible),
            ANTHROPIC => Some(Self::Anthropic),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::OpenaiCompatible => OPENAI_COMPATIBLE,
            Self::Anthropic => ANTHROPIC,
        }
    }
}

/// この端末での呼び方（T4-26、N3）。Messages API の接続先だけが Claude Code を選べる。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Via {
    /// API（キーで呼ぶ）。
    #[default]
    Api,
    /// Claude Code の CLI（`claude -p`。CLI のログインを使い、キーをこのアプリに置かない）。
    ClaudeCode,
}

/// 端末の設定（アプリの設定のフォルダの `ai.toml`）。チームで共有しないものだけを持つ。キーは持たない。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineSettings {
    /// 接続先の名前ごとの上書き（すべての作品フォルダに効く）。
    #[serde(default)]
    pub connections: BTreeMap<String, Override>,
    #[serde(default)]
    pub cloud: CloudSettings,
    /// タスクの名前ごとの接続先の上書き（すべての作品フォルダに効く。T4-1）。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tasks: BTreeMap<String, TaskOverride>,
}

/// この端末での、接続先の URL・モデル・文脈の長さ・返す分の上限の上書き。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Override {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// 文脈の長さ（正の整数。T4-1）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_tokens: Option<u64>,
    /// 返す分の上限（正の整数。T4-10）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    /// 構造化出力を使うか（T4-2）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_output: Option<bool>,
    /// 呼び方（無ければ API。T4-26）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<Via>,
    /// Claude Code の CLI の場所（無ければ探す。T4-26）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli: Option<String>,
}

impl Override {
    fn is_empty(&self) -> bool {
        self.base_url.is_none()
            && self.model.is_none()
            && self.context_tokens.is_none()
            && self.output_tokens.is_none()
            && self.structured_output.is_none()
            && self.via.is_none()
            && self.cli.is_none()
    }
}

/// この端末での、タスクの接続先の上書き（T4-1）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskOverride {
    /// 接続先の名前（`[ai.<名前>]`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// タスクの接続先をどこで決めたか（T4-1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskConnectionSource {
    /// この端末の上書き。
    Machine,
    /// タスクの定義の `model`。
    Task,
    /// 既定の接続先。
    Default,
}

/// クラウドへの送信の許可。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudSettings {
    /// 送ってよい行き先（[`Connection::endpoint`]）。
    #[serde(default)]
    pub allowed: BTreeSet<String>,
}

impl MachineSettings {
    /// 接続先の上書きを置き換える。空の値と0の文脈の長さ・返す分の上限は上書きしない（すべて無ければ上書きを
    /// やめる）。
    pub fn set_override(&mut self, name: &str, value: Override) {
        // 呼び方と CLI の場所は `set_via` で変える（ここでは今のまま残す）。
        let kept = self.connections.get(name);
        let item = Override {
            base_url: non_empty(value.base_url.as_deref()),
            model: non_empty(value.model.as_deref()),
            context_tokens: value.context_tokens.filter(|&n| n > 0),
            output_tokens: value.output_tokens.filter(|&n| n > 0),
            structured_output: value.structured_output,
            via: kept.and_then(|o| o.via),
            cli: kept.and_then(|o| o.cli.clone()),
        };
        if item.is_empty() {
            self.connections.remove(name);
        } else {
            self.connections.insert(name.to_owned(), item);
        }
    }

    /// 呼び方と Claude Code の CLI の場所を変える（T4-26）。API で CLI の場所も無ければ書かない。ほかの上書きは残す。
    pub fn set_via(&mut self, name: &str, via: Via, cli: Option<&str>) {
        let mut item = self.connections.get(name).cloned().unwrap_or_default();
        item.via = (via != Via::Api).then_some(via);
        item.cli = non_empty(cli);
        if item.is_empty() {
            self.connections.remove(name);
        } else {
            self.connections.insert(name.to_owned(), item);
        }
    }

    /// タスクの接続先の上書きを置き換える。空なら上書きをやめる（T4-1）。
    pub fn set_task_connection(&mut self, task: &str, connection: Option<&str>) {
        match non_empty(connection) {
            Some(model) => {
                self.tasks
                    .insert(task.to_owned(), TaskOverride { model: Some(model) });
            }
            None => {
                self.tasks.remove(task);
            }
        }
    }

    /// クラウドへの送信を許可する・やめる。
    pub fn set_cloud_allowed(&mut self, endpoint: &str, allowed: bool) {
        if allowed {
            self.cloud.allowed.insert(endpoint.to_owned());
        } else {
            self.cloud.allowed.remove(endpoint);
        }
    }
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

/// 端末の設定を読む。無ければ既定。読めなければ既定と、知らせる文。
pub fn read_machine_settings(path: &Path) -> (MachineSettings, Option<String>) {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return (MachineSettings::default(), None);
        }
        Err(e) => {
            return (
                MachineSettings::default(),
                Some(format!(
                    "AIの端末の設定（{}）を読めません：{e}",
                    path.display()
                )),
            );
        }
    };
    match toml::from_str(text.trim_start_matches('\u{feff}')) {
        Ok(settings) => (settings, None),
        Err(e) => (
            MachineSettings::default(),
            Some(format!(
                "AIの端末の設定（{}）を読めません：{}",
                path.display(),
                e.message()
            )),
        ),
    }
}

/// 端末の設定を書く（アプリが書くファイルなので、丸ごと書き直す）。
pub fn write_machine_settings(path: &Path, settings: &MachineSettings) -> Result<(), String> {
    let body = toml::to_string(settings).map_err(|e| e.to_string())?;
    let text = format!(
        "# scn-studio の AI の接続の、この端末だけの設定（アプリが書きます。キーは OS のキーチェーンに置きます）\n{body}"
    );
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}：{e}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("{}：{e}", path.display()))
}

/// 呼び出せる形にした接続先。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Connection {
    pub name: String,
    pub provider: Provider,
    /// API の URL（末尾の `/` を除く）。
    pub base_url: String,
    pub model: String,
    /// クラウド（ローカルとLAN以外）の行き先。
    pub cloud: bool,
    /// 行き先（`<provider> <base_url>`）。キーチェーンのアカウントと、クラウドへの送信の許可の単位。
    pub endpoint: String,
    /// この端末で、クラウドへの送信を許可した（ローカルの行き先では常に真）。
    pub allowed: bool,
    /// 文脈の長さ（トークン数）。
    pub context_tokens: u64,
    /// 返す分の上限（端末の上書きか `project.toml` の値。無ければ文脈の長さから決める。T4-10）。
    pub output_tokens: Option<u64>,
    /// 構造化出力（`response_format`）を使う（OpenAI 互換だけ。T4-2）。
    pub structured_output: bool,
    /// この端末での呼び方（Messages API の接続先だけが Claude Code を選べる。T4-26）。
    pub via: Via,
    /// Claude Code の CLI の場所（この端末で書いたもの。無ければ探す。T4-26）。
    pub cli: Option<String>,
}

impl Connection {
    /// 文脈の長さと返す分の上限（文脈の組み立てに渡す。T4-10）。
    pub fn budget(&self) -> TokenBudget {
        TokenBudget {
            context_tokens: self.context_tokens,
            output_tokens: self.output_tokens,
        }
    }

    /// 送り先（行き先とモデル。Claude Code で呼ぶならそう添える）。人がクラウドへ送ることを確かめた単位（自律と
    /// 同期。T4-1）。
    pub fn destination(&self) -> String {
        match self.via {
            Via::Api => format!("{}\n{}", self.endpoint, self.model),
            Via::ClaudeCode => format!("{}\n{}\nclaude-code", self.endpoint, self.model),
        }
    }
}

/// 接続先の宣言に端末の上書きを重ね、呼び出せる形にする。足りない・誤った値は、知らせる文で返す。
pub fn resolve(decl: &AiConnectionDecl, machine: &MachineSettings) -> Result<Connection, String> {
    resolve_with(decl, machine, true)
}

/// モデルの一覧を読むための解決（T4-23、N4）：`resolve` と同じで、`model` が無くてもよい（無ければ空）。
pub fn resolve_for_models(
    decl: &AiConnectionDecl,
    machine: &MachineSettings,
) -> Result<Connection, String> {
    resolve_with(decl, machine, false)
}

fn resolve_with(
    decl: &AiConnectionDecl,
    machine: &MachineSettings,
    needs_model: bool,
) -> Result<Connection, String> {
    let name = &decl.name;
    let over = machine.connections.get(name);
    let provider_name = decl.provider.as_deref().unwrap_or(OPENAI_COMPATIBLE);
    let provider = Provider::parse(provider_name).ok_or_else(|| {
        format!(
            "[ai.{name}] の provider「{provider_name}」には対応していません（{OPENAI_COMPATIBLE} か {ANTHROPIC}）"
        )
    })?;
    let base_url = over
        .and_then(|o| o.base_url.clone())
        .or_else(|| decl.base_url.clone())
        .or_else(|| (provider == Provider::Anthropic).then(|| ANTHROPIC_BASE_URL.to_owned()))
        .ok_or_else(|| format!("[ai.{name}] に base_url がありません"))?;
    let written = base_url.trim();
    let url = Url::parse(written)
        .map_err(|e| format!("[ai.{name}] の base_url「{written}」を URL として読めません：{e}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!(
            "[ai.{name}] の base_url は http か https の URL で書きます"
        ));
    }
    // 読んだ URL の書き方にそろえる（`HTTP:/host` のような書き方でも、行き先と http の判断を誤らないため）。
    let base_url = url.as_str().trim_end_matches('/').to_owned();
    let model = over
        .and_then(|o| o.model.clone())
        .or_else(|| decl.model.clone())
        .map(|m| m.trim().to_owned())
        .filter(|m| !m.is_empty());
    let model = match model {
        Some(model) => model,
        None if needs_model => return Err(format!("[ai.{name}] に model がありません")),
        None => String::new(),
    };
    let cloud = provider == Provider::Anthropic || !url.host().is_some_and(|h| is_local_host(&h));
    let endpoint = format!("{} {base_url}", provider.name());
    let allowed = !cloud || machine.cloud.allowed.contains(&endpoint);
    let context_tokens = over
        .and_then(|o| o.context_tokens)
        .filter(|&n| n > 0)
        .or(decl.context_tokens)
        .unwrap_or(match provider {
            Provider::OpenaiCompatible => DEFAULT_CONTEXT_TOKENS,
            Provider::Anthropic => ANTHROPIC_CONTEXT_TOKENS,
        });
    let output_tokens = over
        .and_then(|o| o.output_tokens)
        .filter(|&n| n > 0)
        .or(decl.output_tokens);
    let structured_output = provider == Provider::OpenaiCompatible
        && over
            .and_then(|o| o.structured_output)
            .or(decl.structured_output)
            .unwrap_or(false);
    // Claude Code は、既定の URL の Messages API の接続先だけ（CLI は自分に設定された行き先へ送るので、送ることの
    // 許可の行き先と合わせるため。ほかでは書いてあっても API で呼ぶ）。
    let via = over
        .and_then(|o| o.via)
        .filter(|_| claude_code_capable(provider, &base_url))
        .unwrap_or_default();
    Ok(Connection {
        name: name.clone(),
        provider,
        base_url,
        model,
        cloud,
        endpoint,
        allowed,
        context_tokens,
        output_tokens,
        structured_output,
        via,
        cli: over.and_then(|o| o.cli.clone()),
    })
}

/// Claude Code（CLI）で呼べる接続先か：Messages API で、URL が既定（`https://api.anthropic.com/v1`）のもの（T4-26）。
pub fn claude_code_capable(provider: Provider, base_url: &str) -> bool {
    provider == Provider::Anthropic && base_url.trim_end_matches('/') == ANTHROPIC_BASE_URL
}

/// 端末の外へ出ない行き先か（この端末と、LAN の中）。localhost、`*.localhost`、`*.local`、ループバック、
/// プライベートとリンクローカルのアドレス。
pub fn is_local_host(host: &url::Host<&str>) -> bool {
    match host {
        url::Host::Domain(name) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            name == "localhost" || name.ends_with(".localhost") || name.ends_with(".local")
        }
        url::Host::Ipv4(ip) => is_local_v4(*ip),
        url::Host::Ipv6(ip) => is_local_v6(*ip),
    }
}

fn is_local_v4(ip: Ipv4Addr) -> bool {
    ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
}

fn is_local_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_local_v4(v4);
    }
    let first = ip.segments().first().copied().unwrap_or(0);
    ip.is_loopback()
        || ip.is_unspecified()
        // 一意のローカル（fc00::/7）とリンクローカル（fe80::/10）
        || (first & 0xfe00) == 0xfc00
        || (first & 0xffc0) == 0xfe80
}

/// タスクに使う接続先の名前と、どこで決めたか（T4-1）：この端末の上書き、タスクの定義の `model`、既定の接続先の順。
pub fn task_connection_name(
    decls: &[AiConnectionDecl],
    machine: &MachineSettings,
    task: &AiTask,
) -> Option<(String, TaskConnectionSource)> {
    if let Some(name) = machine.tasks.get(&task.id).and_then(|t| t.model.clone()) {
        return Some((name, TaskConnectionSource::Machine));
    }
    if let Some(name) = &task.model {
        return Some((name.clone(), TaskConnectionSource::Task));
    }
    default_connection(decls, machine).map(|name| (name.to_owned(), TaskConnectionSource::Default))
}

/// タスクに使う接続先（T4-1）。上書きの接続先が作品に無ければ、定義の接続先へ戻さずに知らせる。
pub fn task_connection(
    decls: &[AiConnectionDecl],
    machine: &MachineSettings,
    task: &AiTask,
) -> Result<Connection, String> {
    let (name, source) = task_connection_name(decls, machine, task)
        .ok_or_else(|| "project.toml に AI の接続先（[ai.local] など）がありません".to_owned())?;
    let decl = decls.iter().find(|d| d.name == name).ok_or_else(|| match source {
        TaskConnectionSource::Machine => format!(
            "この端末でタスク「{}」に選んだ接続先「{name}」が project.toml にありません（AI実行の画面で選び直します）",
            task.name
        ),
        _ => format!("{} の接続先「{name}」が project.toml にありません", task.path),
    })?;
    resolve(decl, machine)
}

/// 既定の接続先：`[ai.local]` があればそれ、無ければ最初のローカルの接続先、無ければ最初の接続先。
pub fn default_connection<'a>(
    decls: &'a [AiConnectionDecl],
    machine: &MachineSettings,
) -> Option<&'a str> {
    if let Some(local) = decls.iter().find(|d| d.name == DEFAULT_CONNECTION) {
        return Some(&local.name);
    }
    decls
        .iter()
        .find(|d| resolve(d, machine).is_ok_and(|c| !c.cloud))
        .or_else(|| decls.first())
        .map(|d| d.name.as_str())
}

/// 環境設定の画面に出す接続先の様子。キーそのものは持たない（フロントに渡さない）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectionStatus {
    pub name: String,
    /// 使う接続の種類（書かれていなければ `openai-compatible`）。
    pub provider: String,
    /// 使う URL とモデル（上書きを重ねたもの）。解決できなければ、URL は model を除いて解決した整えたもの（それも
    /// できなければ書かれた値）、モデルは書かれた値。
    pub base_url: Option<String>,
    pub model: Option<String>,
    /// `project.toml` に書かれた値。
    pub project_base_url: Option<String>,
    pub project_model: Option<String>,
    /// この端末の上書き。
    pub override_base_url: Option<String>,
    pub override_model: Option<String>,
    pub cloud: bool,
    /// 行き先（解決できたときだけ）。
    pub endpoint: Option<String>,
    pub allowed: bool,
    /// キーチェーンにキーがある。
    pub has_key: bool,
    /// キーが要る（Messages API。OpenAI 互換は、キーがあれば送る）。
    pub needs_key: bool,
    /// 文脈の長さ（解決できたときだけ）、`project.toml` の値、この端末の上書き（T4-1）。
    pub context_tokens: Option<u64>,
    pub project_context_tokens: Option<u64>,
    pub override_context_tokens: Option<u64>,
    /// 返す分の上限（解決できたときだけ。書いていなければ文脈の長さから決めた値）、`project.toml` の値、この端末の
    /// 上書き（T4-10）。
    pub output_tokens: Option<u64>,
    pub project_output_tokens: Option<u64>,
    pub override_output_tokens: Option<u64>,
    /// 構造化出力を使うか（解決できたときだけ）、`project.toml` の値、この端末の上書き（T4-2）。
    pub structured_output: Option<bool>,
    pub project_structured_output: Option<bool>,
    pub override_structured_output: Option<bool>,
    /// この端末での呼び方と、書いた CLI の場所（T4-26）。
    pub via: Via,
    pub cli: Option<String>,
    /// Claude Code で呼べる接続先か（既定の URL の Messages API。[`claude_code_capable`]）。
    pub claude_code: bool,
    /// 解決できない理由。
    pub problem: Option<String>,
    /// URL の書き方の知らせ（パスが無い。[`missing_path_hint`]）。
    pub url_hint: Option<String>,
    pub is_default: bool,
    /// 「接続を試す」で送る中身（解決できたときだけ）。
    pub test_preview: Option<String>,
}

/// 接続先の様子の一覧と、キーチェーンを読めなかったときの知らせ。
pub fn connection_statuses(
    decls: &[AiConnectionDecl],
    machine: &MachineSettings,
    keys: &dyn KeyStore,
) -> (Vec<ConnectionStatus>, Option<String>) {
    let default = default_connection(decls, machine);
    let mut key_problem = None;
    let statuses = decls
        .iter()
        .map(|decl| {
            let over = machine.connections.get(&decl.name);
            let resolved = resolve(decl, machine);
            let (base_url, model, cloud, endpoint, allowed, provider) = match &resolved {
                Ok(c) => (
                    Some(c.base_url.clone()),
                    Some(c.model.clone()),
                    c.cloud,
                    Some(c.endpoint.clone()),
                    c.allowed,
                    c.provider.name().to_owned(),
                ),
                Err(_) => {
                    // model だけが無いときも、URL とローカルかどうかは求める（モデルの一覧を読むため。T4-23）。
                    let place = resolve_for_models(decl, machine).ok();
                    (
                        place.as_ref().map(|c| c.base_url.clone()).or_else(|| {
                            over.and_then(|o| o.base_url.clone())
                                .or_else(|| decl.base_url.clone())
                        }),
                        over.and_then(|o| o.model.clone())
                            .or_else(|| decl.model.clone()),
                        place.as_ref().is_some_and(|c| c.cloud),
                        None,
                        false,
                        decl.provider
                            .clone()
                            .unwrap_or_else(|| OPENAI_COMPATIBLE.to_owned()),
                    )
                }
            };
            let has_key = match &endpoint {
                Some(endpoint) => match keys.get(endpoint) {
                    Ok(key) => key.is_some(),
                    Err(e) => {
                        key_problem.get_or_insert_with(|| e.to_string());
                        false
                    }
                },
                None => false,
            };
            // Claude Code で呼ぶなら、キーは要らない（CLI のログインを使う。T4-26）。
            let needs_key = resolved
                .as_ref()
                .is_ok_and(|c| c.provider == Provider::Anthropic && c.via == Via::Api);
            let via = resolved.as_ref().map_or(Via::Api, |c| c.via);
            ConnectionStatus {
                name: decl.name.clone(),
                provider,
                base_url,
                model,
                project_base_url: decl.base_url.clone(),
                project_model: decl.model.clone(),
                override_base_url: over.and_then(|o| o.base_url.clone()),
                override_model: over.and_then(|o| o.model.clone()),
                cloud,
                endpoint,
                allowed,
                has_key,
                needs_key,
                context_tokens: resolved.as_ref().ok().map(|c| c.context_tokens),
                project_context_tokens: decl.context_tokens,
                override_context_tokens: over.and_then(|o| o.context_tokens),
                output_tokens: resolved.as_ref().ok().map(|c| c.budget().output()),
                project_output_tokens: decl.output_tokens,
                override_output_tokens: over.and_then(|o| o.output_tokens),
                structured_output: resolved.as_ref().ok().map(|c| c.structured_output),
                project_structured_output: decl.structured_output,
                override_structured_output: over.and_then(|o| o.structured_output),
                via,
                cli: over.and_then(|o| o.cli.clone()),
                claude_code: resolved
                    .as_ref()
                    .is_ok_and(|c| claude_code_capable(c.provider, &c.base_url)),
                test_preview: resolved
                    .as_ref()
                    .ok()
                    .map(|c| request_preview(c, &test_request())),
                url_hint: resolved
                    .as_ref()
                    .ok()
                    .and_then(missing_path_hint)
                    .map(str::to_owned),
                problem: resolved.err(),
                is_default: default == Some(decl.name.as_str()),
            }
        })
        .collect();
    (statuses, key_problem)
}
