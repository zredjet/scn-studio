//! 書き出しのプラグイン（T3-14、D075）：作品フォルダの出力プロファイル `export/<名前>.yaml` に `template:` か
//! `command:` を書くと、`scn export <名前>` とアプリの「出力」の画面から書き出せる（設計検討書「プラグイン」）。
//!
//! - テンプレート：IR・通読の本文・作品の設定を minijinja のテンプレートに渡し、1つのテキストのファイルを書く。
//! - 外部コマンド：IR の JSON を標準入力で渡し、作品フォルダを作業フォルダにして実行する。書き出し先などは
//!   環境変数と `{output}` などの置き換えで渡し、コマンドがファイルを書く。シェルは通さない。終了コードが0で
//!   なければ失敗、タイムアウトで止める。
//! - 縦書きPDFを組むコマンド（`export/book.yaml` の `pdf_command`。T3-13 の続き）も、同じ実行の仕方で呼ぶ
//!   （[`run_command`]）。

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use minijinja::{AutoEscape, Environment, ErrorKind, UndefinedBehavior, Value};
use serde::Serialize;
use thiserror::Error;
use yaml_serde::Value as Yaml;

use crate::ir::Ir;
use crate::plaintext::plain_text;
use crate::project::{Project, without_bom};
use crate::renpy::{relative_inside, yaml_text};
use crate::route::{ReadingItem, ReadingTarget, Variables, read, reading_choices, target_label};

/// 出力プロファイルのフォルダ（作品フォルダから）。
pub const PROFILE_DIR: &str = "export";
/// 組み込みの書き出しの名前（`scn export` の下のコマンドと、組み込みの書き出しのプロファイル）。同じ名前の
/// プロファイルはプラグインにしない。
pub const BUILTIN_NAMES: [&str; 9] = [
    "book", "epub", "orders", "pdf", "renpy", "script", "text", "tyrano", "utage",
];
/// 外部コマンドのタイムアウトの既定（秒）。
pub const DEFAULT_TIMEOUT: u64 = 300;
/// 取っておく標準出力と標準エラーの長さ（バイト。超えたら後ろを残す）。
const OUTPUT_LIMIT: usize = 64 * 1024;
/// コマンドが終わった後に、標準出力と標準エラーを読み終えるのを待つ長さ（コマンドが起こした別のプロセスが
/// 持ち続けていても止まらないように）。
const DRAIN_WAIT: Duration = Duration::from_millis(500);

/// プラグインの種類。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginKind {
    /// テンプレートのファイル（作品フォルダから数えたパス）。
    Template { template: String },
    /// コマンドと引数（`{project}`・`{output}` などは実行のときに置き換える）。
    Command { argv: Vec<String> },
}

/// 出力プロファイル `export/<名前>.yaml` から読んだプラグイン。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Plugin {
    pub name: String,
    /// プロファイルのファイル（作品フォルダから。`export/<名前>.yaml`）。
    pub profile: String,
    #[serde(flatten)]
    pub kind: PluginKind,
    /// 書き出し先（作品フォルダから数えたパス）。テンプレートは書くファイル、外部コマンドは渡すフォルダ。
    pub output: String,
    /// 外部コマンドのタイムアウト（秒）。
    pub timeout: u64,
    /// 読み飛ばして既定を使った値。
    pub problems: Vec<String>,
    /// プロファイルの中身（テンプレートに `profile` として渡す）。
    #[serde(skip)]
    pub settings: Yaml,
}

/// 外部コマンドの標準出力と標準エラー（UTF-8 として読めない分は置き換える。長ければ後ろだけ）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
}

/// 書き出せなかった。
#[derive(Debug, Error)]
pub enum PluginError {
    #[error("{0} がありません")]
    Missing(String),
    #[error("{profile}：{message}")]
    Profile { profile: String, message: String },
    #[error("テンプレート {template}：{message}")]
    Template { template: String, message: String },
    #[error("{program} を実行できません：{source}")]
    Spawn {
        program: String,
        #[source]
        source: io::Error,
    },
    #[error("{program} が失敗しました（{status}）")]
    Failed {
        program: String,
        status: String,
        output: CommandOutput,
    },
    #[error("{program} が {seconds} 秒で終わらないので止めました")]
    Timeout {
        program: String,
        seconds: u64,
        output: CommandOutput,
    },
    #[error("{} に書けません：{source}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

impl PluginError {
    /// 失敗したコマンドの標準出力と標準エラー。
    pub fn output(&self) -> Option<&CommandOutput> {
        match self {
            PluginError::Failed { output, .. } | PluginError::Timeout { output, .. } => {
                Some(output)
            }
            _ => None,
        }
    }
}

/// プロファイルの名前として使えるか（`export/` の直下のファイル名になる）。
fn usable_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && !name
            .chars()
            .any(|c| c == '/' || c == '\\' || c == ':' || c.is_control())
}

/// プロファイルのファイル（作品フォルダから）。
fn profile_path(name: &str) -> String {
    format!("{PROFILE_DIR}/{name}.yaml")
}

/// 作品フォルダのプラグインのプロファイル（`export/*.yaml` のうち `template` か `command` のあるものと、
/// YAML として読めないもの）を、名前の順に。組み込みの書き出しの名前のものは除く。
pub fn list_plugins(root: &Path) -> Vec<(String, Result<Plugin, PluginError>)> {
    let Ok(entries) = fs::read_dir(root.join(PROFILE_DIR)) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| {
            let file = e.file_name().into_string().ok()?;
            let name = file.strip_suffix(".yaml")?.to_owned();
            (usable_name(&name) && !BUILTIN_NAMES.contains(&name.as_str())).then_some(name)
        })
        .collect();
    names.sort();
    names
        .into_iter()
        .filter_map(|name| match load(root, &name) {
            Ok(None) => None,
            Ok(Some(plugin)) => Some((name, Ok(plugin))),
            Err(e) => Some((name, Err(e))),
        })
        .collect()
}

/// プラグインのプロファイル `export/<名前>.yaml` を読む。
pub fn read_plugin(root: &Path, name: &str) -> Result<Plugin, PluginError> {
    let profile = profile_path(name);
    if BUILTIN_NAMES.contains(&name) {
        return Err(PluginError::Profile {
            profile,
            message: "組み込みの書き出しの名前です（プラグインには別の名前を付けます）".to_owned(),
        });
    }
    if !usable_name(name) {
        return Err(PluginError::Missing(profile));
    }
    load(root, name)?.ok_or_else(|| PluginError::Profile {
        profile,
        message: "template（テンプレート）か command（外部コマンド）を書きます".to_owned(),
    })
}

/// 読む。`template` も `command` も無ければ `None`（プラグインのプロファイルではない）。
fn load(root: &Path, name: &str) -> Result<Option<Plugin>, PluginError> {
    let profile = profile_path(name);
    let text = match fs::read_to_string(root.join(&profile)) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(PluginError::Missing(profile)),
        Err(e) => {
            return Err(PluginError::Profile {
                profile,
                message: format!("読めません：{e}"),
            });
        }
    };
    parse(name, &text)
}

/// プロファイルの中身を読む。
fn parse(name: &str, text: &str) -> Result<Option<Plugin>, PluginError> {
    let profile = profile_path(name);
    let bad = |message: String| PluginError::Profile {
        profile: profile.clone(),
        message,
    };
    let settings: Yaml = yaml_serde::from_str(without_bom(text))
        .map_err(|e| bad(format!("YAML として読めません：{e}")))?;
    let present = |key: &str| settings.get(key).filter(|v| !v.is_null());
    let kind = match (present("template"), present("command")) {
        (None, None) => return Ok(None),
        (Some(_), Some(_)) => {
            return Err(bad(
                "template と command は、どちらか1つだけを書きます".to_owned()
            ));
        }
        (Some(template), None) => match yaml_text(template).filter(|t| relative_inside(t)) {
            Some(template) => PluginKind::Template { template },
            None => {
                return Err(bad(
                    "template は、作品フォルダの中のファイルを作品フォルダから数えたパスで書きます"
                        .to_owned(),
                ));
            }
        },
        (None, Some(command)) => PluginKind::Command {
            argv: command_argv(command, "command").map_err(bad)?,
        },
    };
    let mut problems = Vec::new();
    let default = default_output(name, &kind);
    let output = match settings.get("output") {
        None | Some(Yaml::Null) => default,
        Some(value) => match yaml_text(value) {
            Some(output) if allowed_output(&output, &kind) => output,
            _ => {
                problems.push(format!(
                    "output は、export/ の下を作品フォルダから数えたパスで書きます（出力プロファイルと\
                     テンプレートは指せません。既定の {default} にしました）"
                ));
                default
            }
        },
    };
    let timeout = read_timeout(&settings, "timeout", &mut problems);
    Ok(Some(Plugin {
        name: name.to_owned(),
        profile,
        kind,
        output,
        timeout,
        problems,
        settings,
    }))
}

/// 書き出し先にできるか：`export/` の下（書き出しは一方向で、本文や設定を書き換えない）。`export/` の直下の
/// `.yaml`（出力プロファイル）とテンプレートは指せない。
fn allowed_output(output: &str, kind: &PluginKind) -> bool {
    if !relative_inside(output) {
        return false;
    }
    let parts: Vec<Component<'_>> = Path::new(output)
        .components()
        .filter(|c| !matches!(c, Component::CurDir))
        .collect();
    let in_export =
        parts.len() >= 2 && parts.first() == Some(&Component::Normal(PROFILE_DIR.as_ref()));
    let profile = parts.len() == 2
        && Path::new(output)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("yaml"));
    let same = |a: &str, b: &str| Path::new(a).components().eq(Path::new(b).components());
    let template = matches!(kind, PluginKind::Template { template } if same(output, template));
    in_export && !profile && !template
}

/// タイムアウト（秒。正の整数）を読む。無ければ既定。
pub(crate) fn read_timeout(settings: &Yaml, key: &str, problems: &mut Vec<String>) -> u64 {
    match settings.get(key) {
        None | Some(Yaml::Null) => DEFAULT_TIMEOUT,
        Some(Yaml::Number(n)) if n.as_u64().is_some_and(|n| n > 0) => {
            n.as_u64().unwrap_or(DEFAULT_TIMEOUT)
        }
        Some(_) => {
            problems.push(format!(
                "{key} は秒（正の整数）で書きます（既定の {DEFAULT_TIMEOUT} にしました）"
            ));
            DEFAULT_TIMEOUT
        }
    }
}

/// 書き出し先の既定：`export/<名前>/` の中。テンプレートは、テンプレートのファイル名から `.j2`・`.jinja`・
/// `.jinja2` を除いた名前（`cast.csv.j2` なら `export/<名前>/cast.csv`）。外部コマンドはフォルダそのもの。
fn default_output(name: &str, kind: &PluginKind) -> String {
    match kind {
        PluginKind::Command { .. } => format!("{PROFILE_DIR}/{name}"),
        PluginKind::Template { template } => {
            let file = Path::new(template)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(name);
            let file = [".j2", ".jinja2", ".jinja"]
                .iter()
                .find_map(|suffix| file.strip_suffix(suffix))
                .filter(|f| !f.is_empty())
                .unwrap_or(file);
            format!("{PROFILE_DIR}/{name}/{file}")
        }
    }
}

/// `command`（`pdf_command`）の値を、コマンドと引数にする。文字列は空白で区切り（`"…"`・`'…'` で囲めば
/// 空白を含められる。シェルは通さないので、`|`・`>`・`$変数` は使えない）、文字列の列はそのまま。
pub(crate) fn command_argv(value: &Yaml, key: &str) -> Result<Vec<String>, String> {
    let argv = match value {
        Yaml::Sequence(items) => items
            .iter()
            .map(|item| {
                yaml_text(item).ok_or_else(|| format!("{key} の列の要素は文字列で書きます"))
            })
            .collect::<Result<Vec<_>, _>>()?,
        value => {
            let text = yaml_text(value)
                .ok_or_else(|| format!("{key} は文字列か、文字列の列で書きます"))?;
            split_command(&text).map_err(|e| format!("{key}：{e}"))?
        }
    };
    match argv.first() {
        Some(program) if !program.is_empty() => Ok(argv),
        _ => Err(format!("{key} が空です")),
    }
}

/// コマンドの文字列を、空白（半角の空白・タブ・改行）で区切る。`"…"`・`'…'` で囲んだ所は1つの引数にする
/// （囲みの中に `\` の特別な意味は無い。Windows のパスをそのまま書ける）。
pub fn split_command(text: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    for c in text.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => current.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                started = true;
            }
            None if matches!(c, ' ' | '\t' | '\n' | '\r') => {
                if started {
                    out.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            None => {
                current.push(c);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return Err("引用符が閉じていません".to_owned());
    }
    if started {
        out.push(current);
    }
    Ok(out)
}

/// コマンドと引数を、見せるための1行にする（空白や引用符を含む引数は囲む）。
pub fn command_line(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| {
            if arg.is_empty()
                || arg
                    .chars()
                    .any(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '"' | '\''))
            {
                if arg.contains('"') {
                    format!("'{arg}'")
                } else {
                    format!("\"{arg}\"")
                }
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ---- テンプレート ----

/// テンプレートに渡す作品の設定。
#[derive(Serialize)]
struct ProjectInfo<'a> {
    title: Option<&'a str>,
    start_scene: Option<&'a str>,
    /// 表示スタイルごとの括弧（同じ表示スタイルの括弧がいくつかあれば、初めのもの）。
    brackets: BTreeMap<&'a str, Bracket>,
}

#[derive(Serialize)]
struct Bracket {
    open: String,
    close: String,
}

#[derive(Serialize)]
struct CharacterInfo<'a> {
    name: &'a str,
    aliases: &'a [String],
}

/// 通読の道（`scn export text` で書く道）。
#[derive(Serialize)]
struct Road {
    name: String,
    target: ReadingTarget,
    stop: &'static str,
    items: Vec<ReadingItem>,
    /// プレーンテキスト（`scn export text` と同じ中身。注記つき）。
    text: String,
}

#[derive(Serialize)]
struct TemplateContext<'a> {
    ir: &'a Ir,
    project: ProjectInfo<'a>,
    characters: BTreeMap<&'a str, CharacterInfo<'a>>,
    roads: Vec<Road>,
    profile: &'a Yaml,
}

fn context<'a>(project: &'a Project, ir: &'a Ir, settings: &'a Yaml) -> TemplateContext<'a> {
    let mut brackets = BTreeMap::new();
    for b in &project.settings.brackets {
        brackets.entry(b.style.as_str()).or_insert_with(|| Bracket {
            open: b.open.to_string(),
            close: b.close.to_string(),
        });
    }
    let characters = project
        .characters
        .iter()
        .map(|c| {
            (
                c.id.as_str(),
                CharacterInfo {
                    name: &c.name,
                    aliases: &c.aliases,
                },
            )
        })
        .collect();
    let roads = reading_choices(ir)
        .into_iter()
        .map(|c| c.target)
        .filter(|t| !matches!(t, ReadingTarget::Ending { .. }))
        .filter_map(|target| {
            let reading = read(project, ir, &target, &Variables::new());
            if reading.message.is_some() {
                return None;
            }
            let name = target_label(ir, &target);
            let text = plain_text(project, &reading, &name, true);
            Some(Road {
                name,
                target,
                stop: reading.stop,
                items: reading.items,
                text,
            })
        })
        .collect();
    TemplateContext {
        ir,
        project: ProjectInfo {
            title: project.settings.title.as_deref(),
            start_scene: project.settings.start_scene.as_deref(),
            brackets,
        },
        characters,
        roads,
        profile: settings,
    }
}

/// テンプレートの名前から、自動のエスケープを決める：HTML と XML（`.j2` などを除いた拡張子が `html`・`htm`・
/// `xhtml`・`xml`）だけ HTML のエスケープ。ほかはしない。
fn auto_escape(name: &str) -> AutoEscape {
    let name = [".j2", ".jinja2", ".jinja"]
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))
        .unwrap_or(name);
    match Path::new(name).extension().and_then(|e| e.to_str()) {
        Some("html" | "htm" | "xhtml" | "xml") => AutoEscape::Html,
        _ => AutoEscape::None,
    }
}

/// フィルタ `plain`：文中の記法の並び（IR の `text`）を文字にする。`style` は `base`（既定。ルビは親文字だけ）・
/// `paren`（`親文字（読み）`）・`narou`（`｜親文字《読み》`、傍点は `《《傍点》》`。`scn export text` と同じ）。
/// 変数の埋め込みは `{=式}`、複数行の台詞の境目は改行。`{w}`・`{p}`・`{speed}`・文中の効果音は書かない。
/// 文字列はそのまま返す。
fn plain_filter(value: Value, style: Option<String>) -> Result<String, minijinja::Error> {
    let style = style.unwrap_or_else(|| "base".to_owned());
    if !matches!(style.as_str(), "base" | "paren" | "narou") {
        return Err(minijinja::Error::new(
            ErrorKind::InvalidOperation,
            format!("plain の書き方 {style} はありません（base・paren・narou）"),
        ));
    }
    if let Some(text) = value.as_str() {
        return Ok(text.to_owned());
    }
    let narou = style == "narou";
    let mut out = String::new();
    for item in value.try_iter()? {
        let attr = |key: &str| {
            item.get_attr(key)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default()
        };
        match attr("kind").as_str() {
            "text" if narou => out.push_str(&attr("text").replace('《', "｜《")),
            "text" => out.push_str(&attr("text")),
            "ruby" => match style.as_str() {
                "paren" => out.push_str(&format!("{}（{}）", attr("base"), attr("reading"))),
                "narou" => out.push_str(&format!("｜{}《{}》", attr("base"), attr("reading"))),
                _ => out.push_str(&attr("base")),
            },
            "emphasis" if narou => out.push_str(&format!("《《{}》》", attr("text"))),
            "emphasis" => out.push_str(&attr("text")),
            "interpolation" => out.push_str(&format!("{{={}}}", attr("expr_text"))),
            "line_break" => out.push('\n'),
            _ => {}
        }
    }
    Ok(out)
}

/// テンプレートを読む（テンプレートのフォルダの中だけ。`..` でもシンボリックリンクでも外へ出ない。先頭の BOM は
/// 読み飛ばす）。
fn loader(
    dir: PathBuf,
) -> impl Fn(&str) -> Result<Option<String>, minijinja::Error> + Send + Sync + 'static {
    let base = fs::canonicalize(&dir).unwrap_or(dir);
    move |name| {
        if !relative_inside(name) {
            return Ok(None);
        }
        let path = base.join(name);
        // シンボリックリンクをたどってフォルダの外へ出ない。
        match fs::canonicalize(&path) {
            Ok(real) if real.starts_with(&base) => {}
            Ok(_) => return Ok(None),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(minijinja::Error::new(
                    ErrorKind::InvalidOperation,
                    format!("{name} を読めません：{e}"),
                ));
            }
        }
        match fs::read_to_string(&path) {
            Ok(text) => Ok(Some(without_bom(&text).to_owned())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(minijinja::Error::new(
                ErrorKind::InvalidOperation,
                format!("{name} を読めません：{e}"),
            )),
        }
    }
}

/// テンプレートの誤りを、場所つきの1行にする。
fn template_message(error: &minijinja::Error) -> String {
    let mut message = match (error.name(), error.line()) {
        (Some(name), Some(line)) => format!("{name} の {line} 行目：{error}"),
        _ => error.to_string(),
    };
    let mut source = std::error::Error::source(error);
    while let Some(inner) = source {
        message.push_str(&format!("（{inner}）"));
        source = inner.source();
    }
    message
}

/// テンプレートのプラグインで、書き出すテキストを作る。
pub fn render_template(
    root: &Path,
    plugin: &Plugin,
    project: &Project,
    ir: &Ir,
) -> Result<String, PluginError> {
    let PluginKind::Template { template } = &plugin.kind else {
        return Err(PluginError::Profile {
            profile: plugin.profile.clone(),
            message: "テンプレートのプラグインではありません".to_owned(),
        });
    };
    let error = |message: String| PluginError::Template {
        template: template.clone(),
        message,
    };
    let path = root.join(template);
    let (Some(dir), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) else {
        return Err(error("ファイルの名前が分かりません".to_owned()));
    };
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::SemiStrict);
    env.set_trim_blocks(true);
    env.set_lstrip_blocks(true);
    env.set_keep_trailing_newline(true);
    env.set_auto_escape_callback(auto_escape);
    env.set_loader(loader(dir.to_owned()));
    env.add_filter("plain", plain_filter);
    let tmpl = env.get_template(name).map_err(|e| {
        if e.kind() == ErrorKind::TemplateNotFound {
            error("ありません".to_owned())
        } else {
            error(template_message(&e))
        }
    })?;
    tmpl.render(context(project, ir, &plugin.settings))
        .map_err(|e| error(template_message(&e)))
}

/// テキストを書く（フォルダが無ければ作る）。
pub fn write_output(path: &Path, text: &str) -> Result<(), PluginError> {
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        fs::create_dir_all(dir).map_err(|source| PluginError::Write {
            path: dir.to_owned(),
            source,
        })?;
    }
    fs::write(path, text).map_err(|source| PluginError::Write {
        path: path.to_owned(),
        source,
    })
}

// ---- 外部コマンド ----

/// 外部コマンドの実行の仕方。
pub struct Invocation<'a> {
    /// 作品フォルダ（作業フォルダ。区切りを含むコマンドの名前は、ここから数える）。
    pub root: &'a Path,
    /// コマンドと引数（置き換える前）。
    pub argv: &'a [String],
    /// 引数の `{名前}` を置き換える値。環境変数 `SCN_<名前を大文字に>` にも渡す。
    pub vars: &'a [(&'a str, String)],
    /// 標準入力に渡すもの。
    pub stdin: &'a [u8],
    /// タイムアウト（秒）。
    pub timeout: u64,
    /// 標準出力と標準エラーを取っておく（アプリ）か、そのまま流す（CLI）か。
    pub capture: bool,
}

/// 引数の `{名前}` を置き換える（知らない名前はそのまま）。
fn substitute(arg: &str, vars: &[(&str, String)]) -> String {
    // 左から1回だけ見る（入れた値の中の `{…}` は、もう置き換えない）。
    let mut out = String::new();
    let mut rest = arg;
    while let Some(open) = rest.find('{') {
        out.push_str(rest.get(..open).unwrap_or_default());
        let tail = rest.get(open + 1..).unwrap_or_default();
        let found = vars.iter().find(|(key, _)| {
            tail.strip_prefix(key)
                .is_some_and(|after| after.starts_with('}'))
        });
        match found {
            Some((key, value)) => {
                out.push_str(value);
                rest = tail.get(key.len() + 1..).unwrap_or_default();
            }
            None => {
                out.push('{');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// コマンドの名前：区切り（`/`、Windows では `\` も）を含めば作品フォルダから数え、含まなければ `PATH` から探す
/// （OS の決まりのとおり。Windows では `.exe` 以外は拡張子まで書く）。
fn program_path(root: &Path, program: &str) -> PathBuf {
    let path = Path::new(program);
    if path.is_absolute() || path.components().count() <= 1 {
        path.to_owned()
    } else {
        root.join(path)
    }
}

/// 絶対のパスにする（シンボリックリンクは解かない。Windows の `\\?\` も付けない）。
pub fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_owned())
}

/// 読み取りのスレッド。読んだものの後ろを `OUTPUT_LIMIT` まで取っておく。
struct Drain {
    buffer: Arc<Mutex<Vec<u8>>>,
    done: mpsc::Receiver<()>,
}

fn drain(mut reader: impl Read + Send + 'static) -> io::Result<Drain> {
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let (sender, done) = mpsc::channel();
    let shared = Arc::clone(&buffer);
    thread::Builder::new()
        .name("scn-plugin-output".into())
        .spawn(move || {
            let mut chunk = [0u8; 8192];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let mut buffer = shared.lock().unwrap_or_else(PoisonError::into_inner);
                        buffer.extend_from_slice(chunk.get(..n).unwrap_or_default());
                        if buffer.len() > OUTPUT_LIMIT * 2 {
                            let cut = buffer.len() - OUTPUT_LIMIT;
                            buffer.drain(..cut);
                        }
                    }
                }
            }
            let _ = sender.send(());
        })?;
    Ok(Drain { buffer, done })
}

impl Drain {
    fn text(self) -> String {
        let _ = self.done.recv_timeout(DRAIN_WAIT);
        let buffer = self.buffer.lock().unwrap_or_else(PoisonError::into_inner);
        let start = buffer.len().saturating_sub(OUTPUT_LIMIT);
        String::from_utf8_lossy(buffer.get(start..).unwrap_or_default()).into_owned()
    }
}

fn gather(stdout: Option<Drain>, stderr: Option<Drain>) -> CommandOutput {
    CommandOutput {
        stdout: stdout.map(Drain::text).unwrap_or_default(),
        stderr: stderr.map(Drain::text).unwrap_or_default(),
    }
}

fn status_text(status: ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("終了コード {code}"),
        None => "シグナルで止まりました".to_owned(),
    }
}

/// 外部コマンドを実行する。作業フォルダは作品フォルダ、シェルは通さない。標準入力に `stdin` を渡す（読まずに
/// 終わってもよい）。終了コードが0なら標準出力と標準エラーを返し、0でなければ [`PluginError::Failed`]、
/// タイムアウトなら止めて [`PluginError::Timeout`]。
pub fn run_command(invocation: &Invocation<'_>) -> Result<CommandOutput, PluginError> {
    let argv: Vec<String> = invocation
        .argv
        .iter()
        .map(|arg| substitute(arg, invocation.vars))
        .collect();
    let Some((program, args)) = argv.split_first() else {
        return Err(PluginError::Spawn {
            program: String::new(),
            source: io::Error::new(io::ErrorKind::InvalidInput, "コマンドが空です"),
        });
    };
    // 作品フォルダは絶対のパスにする（相対のパスのまま作業フォルダにすると、区切りを含むコマンドの名前を
    // Unix では作業フォルダから、Windows では親の作業フォルダから数えて、OS によって違ってしまう）。
    let root = absolute(invocation.root);
    let mut command = Command::new(program_path(&root, program));
    command.args(args).current_dir(&root).stdin(Stdio::piped());
    for (key, value) in invocation.vars {
        command.env(format!("SCN_{}", key.to_ascii_uppercase()), value);
    }
    if invocation.capture {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
    }
    let mut child = command.spawn().map_err(|source| PluginError::Spawn {
        program: program.clone(),
        source,
    })?;
    // 標準入力は別のスレッドで書く（読まないコマンドや、コマンドの出力が詰まって止まらないように）。読まずに
    // 終わったコマンドへの書き込みの誤りは見ない。スレッドを作れなければ、コマンドを止めて誤りにする。
    let threads = (|| -> io::Result<(Option<Drain>, Option<Drain>)> {
        if let Some(mut stdin) = child.stdin.take() {
            let data = invocation.stdin.to_vec();
            thread::Builder::new()
                .name("scn-plugin-input".into())
                .spawn(move || {
                    let _ = stdin.write_all(&data);
                })?;
        }
        let stdout = child.stdout.take().map(drain).transpose()?;
        let stderr = child.stderr.take().map(drain).transpose()?;
        Ok((stdout, stderr))
    })();
    let (stdout, stderr) = match threads {
        Ok(drains) => drains,
        Err(source) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(PluginError::Spawn {
                program: program.clone(),
                source,
            });
        }
    };
    let started = Instant::now();
    let limit = Duration::from_secs(invocation.timeout);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {}
            Err(e) => break Err(e),
        }
        if started.elapsed() >= limit {
            let _ = child.kill();
            let _ = child.wait();
            return Err(PluginError::Timeout {
                program: program.clone(),
                seconds: invocation.timeout,
                output: gather(stdout, stderr),
            });
        }
        thread::sleep(Duration::from_millis(10));
    };
    let output = gather(stdout, stderr);
    match status {
        Ok(status) if status.success() => Ok(output),
        Ok(status) => Err(PluginError::Failed {
            program: program.clone(),
            status: status_text(status),
            output,
        }),
        Err(e) => {
            let _ = child.kill();
            Err(PluginError::Failed {
                program: program.clone(),
                status: format!("終わりを確かめられません：{e}"),
                output,
            })
        }
    }
}

/// 外部コマンドのプラグインで書き出す：書き出し先のフォルダを作り（中は消さない）、IR の JSON を標準入力で
/// 渡して実行する。環境変数は `SCN_PROJECT`（作品フォルダ）・`SCN_OUTPUT`（書き出し先）・`SCN_PROFILE`
/// （プロファイル）・`SCN_NAME`（プロファイルの名前）・`SCN_VERSION`（ツールの版）。どれも引数の
/// `{project}` などでも渡せる。
pub fn export_command(
    root: &Path,
    plugin: &Plugin,
    ir: &Ir,
    output: &Path,
    capture: bool,
) -> Result<CommandOutput, PluginError> {
    let PluginKind::Command { argv } = &plugin.kind else {
        return Err(PluginError::Profile {
            profile: plugin.profile.clone(),
            message: "外部コマンドのプラグインではありません".to_owned(),
        });
    };
    fs::create_dir_all(output).map_err(|source| PluginError::Write {
        path: output.to_owned(),
        source,
    })?;
    let stdin = serde_json::to_vec(ir).map_err(|e| PluginError::Write {
        path: PathBuf::from("<stdin>"),
        source: io::Error::other(e),
    })?;
    let project = absolute(root);
    let vars = [
        ("project", project.display().to_string()),
        ("output", absolute(output).display().to_string()),
        (
            "profile",
            project.join(&plugin.profile).display().to_string(),
        ),
        ("name", plugin.name.clone()),
        ("version", env!("CARGO_PKG_VERSION").to_owned()),
    ];
    run_command(&Invocation {
        root,
        argv,
        vars: &vars,
        stdin: &stdin,
        timeout: plugin.timeout,
        capture,
    })
}

/// 印刷用の HTML から PDF を組むコマンド（`export/book.yaml` の `pdf_command`。T3-13 の続き）を実行する。
/// 引数の `{input}`（印刷用の HTML）・`{output}`（PDF）・`{project}`・`{profile}` を置き換え、環境変数
/// `SCN_INPUT`・`SCN_OUTPUT`・`SCN_PROJECT`・`SCN_PROFILE`・`SCN_VERSION` でも渡す。標準入力は空。
/// コマンドが成功しても PDF ができていない（前からあった PDF が書き換わっていない）ときは失敗にする。
pub fn typeset(
    root: &Path,
    argv: &[String],
    timeout: u64,
    input: &Path,
    output: &Path,
    capture: bool,
) -> Result<CommandOutput, PluginError> {
    let project = absolute(root);
    let output = absolute(output);
    let vars = [
        ("project", project.display().to_string()),
        ("input", absolute(input).display().to_string()),
        ("output", output.display().to_string()),
        (
            "profile",
            project
                .join(crate::book::BOOK_PROFILE)
                .display()
                .to_string(),
        ),
        ("version", env!("CARGO_PKG_VERSION").to_owned()),
    ];
    if let Some(dir) = output.parent() {
        fs::create_dir_all(dir).map_err(|source| PluginError::Write {
            path: dir.to_owned(),
            source,
        })?;
    }
    let modified = |path: &Path| fs::metadata(path).and_then(|m| m.modified()).ok();
    let before = modified(&output);
    let result = run_command(&Invocation {
        root,
        argv,
        vars: &vars,
        stdin: &[],
        timeout,
        capture,
    })?;
    let after = modified(&output);
    if after.is_some() && after != before {
        Ok(result)
    } else {
        Err(PluginError::Failed {
            program: argv.first().cloned().unwrap_or_default(),
            status: format!("{} ができていません", output.display()),
            output: result,
        })
    }
}
