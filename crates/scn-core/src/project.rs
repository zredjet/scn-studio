//! プロジェクトフォルダの読み込み。
//!
//! `project.toml`、設定DB（`bible/`）、`variables.yaml`、`assets/catalog.yaml`、
//! `story/` の下（下のフォルダも）の `*.scn` を読む。設定ファイルの誤りは、読み込みのエラーで止めずに診断にし
//! （E801〜W807）、読めた部分で解析を続ける（D042）。無いファイルは空として扱う。`project.toml` の無いフォルダは
//! プロジェクトではないので、エラーにする。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use yaml_serde::Value;

use crate::ai::{AiTask, parse_task};
use crate::bible::{FieldType, value_problems};
use crate::diag::{Code, Diagnostic, Span};
use crate::syntax::{DialogueBracket, ParseOptions};

/// 対応している形式のバージョン（`project.toml` の `format_version`）。
pub const FORMAT_VERSION: i64 = 0;

/// 読み込んだプロジェクト。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Project {
    pub settings: Settings,
    /// 設定DBの型とエントリ。
    pub bible: Bible,
    /// 設定DBのキャラクター（型定義に `builtin: character` のある型のエントリ）。
    pub characters: Vec<Character>,
    pub variables: Vec<VarDecl>,
    /// `variables.yaml` を丸ごと読めなかった（E801）。宣言の無い変数（W602）を出さない（D042）。
    pub variables_unreadable: bool,
    pub catalog: AssetCatalog,
    /// 企画書（`concept.md`）。無ければ `None`。
    pub concept: Option<Concept>,
    /// `story/` の下（下のフォルダも）の `*.scn`。パスの辞書順に並べてある。読めなかったファイル（E801）は含まない。
    pub files: Vec<SourceFile>,
    /// AIタスクの定義（`ai/*.yaml`。T3-6）。ファイル名の順。読めなかったファイル（E801）は含まない。
    pub ai_tasks: Vec<AiTask>,
    /// 読み込みの診断（E801〜W805）。読んだ順に並べてある。
    pub load_diagnostics: Vec<LoadDiagnostic>,
}

/// `project.toml` のうち、解析に使う設定。
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub format_version: i64,
    /// 作品名（`title`）。アプリの表示に使う。
    pub title: Option<String>,
    pub start_scene: Option<String>,
    /// `start_scene` の値の位置（`project.toml` の中）。無いシーンを指すときの E501 に使う。
    pub start_scene_span: Span,
    pub brackets: Vec<DialogueBracket>,
    /// メッセージウィンドウの大きさ（`[message_window]`）。無ければ、はみ出しを調べない。
    pub message_window: Option<MessageWindow>,
    /// 画面の大きさ（`[screen]`。プレビュー。T1-11）。無ければ `None`（プレビューは 1280×720）。
    pub screen: Option<Screen>,
    /// 本文の読み方に関わる設定を、書かれたとおりに読めなかった：`project.toml` そのものが読めない（E801）、
    /// `format_version` の誤り（E802、E803）、`[dialogue]` と `[dialogue.brackets]` の誤り（E802）。
    /// 台詞の括弧が既定に戻って行の読み方が変わるので、行IDを付けない（D042、D045）。
    /// `title`・`start_scene`・`[message_window]`・`[screen]`・`[editor]`・`[ai.*]` の誤りは含めない。
    pub reading_errors: bool,
    /// エディタの入力補助（`[editor]`。T1-8、D047）。
    pub editor: EditorSettings,
    /// AIの接続先（`[ai.<名前>]`。T3-5）。名前の順。型の合わない接続先（E802）は含めない。
    pub ai: Vec<AiConnectionDecl>,
}

/// AIの接続先の宣言（`project.toml` の `[ai.<名前>]`。T3-5、D066）。値は書かれたとおりに持ち、
/// 知らない `provider` や足りない値は、使うときに知らせる（[`crate::ai::resolve`]）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AiConnectionDecl {
    /// 接続先の名前（`[ai.local]` なら `local`）。AIタスクの `model` が指す。
    pub name: String,
    /// 接続の種類（`openai-compatible` か `anthropic`）。
    pub provider: Option<String>,
    /// API の URL（`/v1` まで）。
    pub base_url: Option<String>,
    pub model: Option<String>,
    /// 文脈の長さ（トークン数。T3-6）。無ければ接続の種類の既定。
    pub context_tokens: Option<u64>,
}

/// エディタの入力補助の設定（`project.toml` の `[editor]`）。キーが無ければ切（D047）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct EditorSettings {
    /// 三点リーダとダッシュの正規化（`...` を `……`、`--` を `――` など）。
    pub normalize_ellipsis: bool,
    /// 地の文の段落頭に全角空白を入れる。
    pub indent_narration: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            format_version: FORMAT_VERSION,
            title: None,
            start_scene: None,
            start_scene_span: Span::new(0, 0),
            brackets: ParseOptions::default().brackets,
            message_window: None,
            screen: None,
            reading_errors: false,
            editor: EditorSettings::default(),
            ai: Vec::new(),
        }
    }
}

/// 画面の大きさ（`project.toml` の `[screen]`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Screen {
    pub width: u32,
    pub height: u32,
}

/// メッセージウィンドウの大きさ（`project.toml` の `[message_window]`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageWindow {
    /// 行数。
    pub lines: u32,
    /// 1行の字数（全角で数える）。
    pub chars_per_line: u32,
}

/// 企画書（`concept.md`）のうち、検証に使うもの。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Concept {
    /// フロントマターの `endings`。書かれていなければ `None`（W701・W702 を調べない）。
    pub endings: Option<Vec<ConceptEnding>>,
}

/// 企画書のエンディング。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConceptEnding {
    pub id: String,
    /// `concept.md` の中の、IDの位置。
    pub span: Span,
}

/// 設定DB（`bible/`）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Bible {
    /// 型（`types/*.yaml`）。ファイル名の順。
    pub types: Vec<EntryType>,
    /// エントリ（`entries/<型>/*.md`）。パスの順。
    pub entries: Vec<Entry>,
}

impl Bible {
    /// キャラクターの型のID：`builtin: character` のある最初の型。無ければ（型定義が読めないときも）
    /// `character`。
    pub fn character_type(&self) -> &str {
        self.types
            .iter()
            .find(|t| t.builtin.as_deref() == Some("character"))
            .map_or("character", |t| t.id.as_str())
    }
}

/// 設定DBの型（`bible/types/<型>.yaml`）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct EntryType {
    /// ファイル名から拡張子を除いたもの。エントリのフォルダ名と同じ。
    pub id: String,
    pub name: String,
    /// 組み込みの型（`character`）。
    pub builtin: Option<String>,
    pub fields: Vec<FieldDef>,
}

/// 型の項目の定義（`fields` の1つ）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FieldDef {
    pub key: String,
    pub label: String,
    /// 書かれたとおりの型（`text`、`list<ref<scene>>` など）。
    #[serde(rename = "type")]
    pub ty: String,
    /// 読んだ型。値の検査（W805・W806）とフォームに使う（T1-10）。
    pub kind: FieldType,
    /// `select`・`multiselect` の選択肢。
    pub options: Vec<String>,
}

/// 設定DBのエントリ（`bible/entries/<型>/<ID>.md`）。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Entry {
    /// 型（エントリのフォルダ名）。
    pub type_id: String,
    /// フロントマターの `id`。無ければファイル名から拡張子を除いたもの。
    pub id: String,
    /// プロジェクトの根からの相対パス。
    pub path: String,
    /// フロントマターの値。
    pub fields: BTreeMap<String, serde_json::Value>,
    /// フロントマターのキーの位置（エントリのファイルの中）。診断の位置に使う。
    pub key_spans: BTreeMap<String, Span>,
    /// フロントマターの後ろの本文（Markdown）。
    pub body: String,
}

/// 設定DBのキャラクター。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Character {
    pub id: String,
    pub name: String,
    pub aliases: Vec<String>,
}

/// `variables.yaml` の1つの変数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarDecl {
    /// `名前空間.名前`。名前空間を省いた宣言には `flag.` を補ってある。
    pub key: String,
    pub ty: VarType,
    /// `scope`（`save`・`persistent`）。書かれていなければ `None`。
    pub scope: Option<String>,
    /// `default`（文字列・数・真偽を文字列にしたもの）。表示に使う（T1-10）。
    pub default: Option<String>,
    /// `range`（`[最小, 最大]` の整数）。形が合わなければ `None`。表示に使う（T1-10）。
    pub range: Option<(i64, i64)>,
    /// `desc`（用途の説明）。
    pub desc: Option<String>,
    /// `variables.yaml` の中の、宣言のキーの位置。
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VarType {
    Bool,
    Int,
    String,
    Enum(Vec<String>),
}

impl VarType {
    pub fn name(&self) -> &'static str {
        match self {
            VarType::Bool => "bool",
            VarType::Int => "int",
            VarType::String => "string",
            VarType::Enum(_) => "enum",
        }
    }
}

/// 素材カタログに登録された意味名。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AssetCatalog {
    pub backgrounds: BTreeSet<String>,
    /// キャラクターIDごとの立ち絵の差分。
    pub characters: BTreeMap<String, BTreeSet<String>>,
    pub bgm: BTreeSet<String>,
    pub se: BTreeSet<String>,
}

/// 1つの `.scn` ファイル。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    /// プロジェクトの根からの相対パス（区切りは `/`）。
    pub path: String,
    pub text: String,
}

/// 読み込みの診断。`file` はプロジェクトの根からの相対パス、`diag.span` はそのファイルの中の位置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadDiagnostic {
    pub file: String,
    pub diag: Diagnostic,
}

/// プロジェクトを読み込めない（`project.toml` が無いなど）ときと、本文の書き換え（[`write_source`]）の
/// エラー。設定ファイルの中身の誤りはエラーにせず、診断にする（[`Project::load_diagnostics`]）。
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("{path} を読めません：{source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{path} に書けません：{source}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// 新しい作品フォルダを作るフォルダが、空でない（[`crate::scaffold::create_project`]）。
    #[error(
        "{path} は空のフォルダではありません。新しい作品フォルダは、無いフォルダか空のフォルダに作ります"
    )]
    NotEmpty { path: PathBuf },
    /// 頼まれた変更をできない（名前が正しくない、本文の書き方の都合など）。何も書いていない。
    #[error("{0}")]
    Refused(String),
}

impl Project {
    /// キャラクターの設定DBのエントリ（口調メモや一人称を読む）。
    pub fn character_entry(&self, id: &str) -> Option<&Entry> {
        let type_id = self.bible.character_type();
        self.bible
            .entries
            .iter()
            .find(|e| e.type_id == type_id && e.id == id)
    }

    /// 構文解析の設定（台詞の括弧）。
    pub fn parse_options(&self) -> ParseOptions {
        ParseOptions {
            brackets: self.settings.brackets.clone(),
        }
    }

    /// 本文の読み方に関わる設定（`project.toml` の `format_version`、`[dialogue]`）を書かれたとおりに
    /// 読めなかったか（[`Settings::reading_errors`]）。あれば、台詞の括弧が既定に戻っているかもしれないので、
    /// 本文を書き換える処理（行IDの付与）はしない（D042、D045）。
    pub fn has_reading_errors(&self) -> bool {
        self.settings.reading_errors
    }
}

/// プロジェクトフォルダを読み込む。`project.toml` が無ければ（読めなければ）エラーにする。
/// 設定ファイルの中身の誤りは、診断にして読み込みを続ける。
pub fn load_project(root: &Path) -> Result<Project, ProjectError> {
    let toml_path = root.join("project.toml");
    let toml_text = fs::read(&toml_path).map_err(|source| ProjectError::Io {
        path: toml_path,
        source,
    })?;
    let mut loader = Loader {
        root,
        diags: Vec::new(),
        entry_texts: BTreeMap::new(),
    };
    let settings = loader.settings(&toml_text);
    let bible = loader.bible()?;
    let characters = loader.characters(&bible);
    // 値が項目の型に合うか（W805。T1-10、D049）。
    for (file, span, message) in value_problems(&bible) {
        loader.diag(&file, Code::W805, span, message);
    }
    let (variables, variables_unreadable) = loader.variables();
    let catalog = loader.catalog();
    let concept = loader.concept();
    let ai_tasks = loader.ai_tasks();
    let files = loader.story()?;
    Ok(Project {
        settings,
        bible,
        characters,
        variables,
        variables_unreadable,
        catalog,
        concept,
        files,
        ai_tasks,
        load_diagnostics: loader.diags,
    })
}

/// 先頭の BOM（U+FEFF）を除いた中身。Windows のエディタが設定ファイルに付けることがあり、YAML の読み手は
/// BOM の後ろを2つ目の文書として読んでしまうので、読む前に除く（T2-10、D061）。
pub(crate) fn without_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

/// 本文のファイルを書き換える（`scn fmt`、保存）。`file` はプロジェクトの根からの相対パス
/// （`SourceFile::path`）。中身は渡したバイトのまま書く。
///
/// 同じフォルダに一時ファイルを書き、ディスクへ書き出してから置き換えるので、途中でプロセスが
/// 止まっても、元のファイルは壊れない。読み取り専用のファイルは書き換えない（OSによらず）。
/// シンボリックリンクは、リンクを残したまま、リンク先を書き換える。
pub fn write_source(root: &Path, file: &str, text: &str) -> Result<(), ProjectError> {
    write_bytes(root, file, text.as_bytes())
}

/// [`write_source`] と同じ書き方で、バイト列を書く（台本の Excel 形式など。T2-10）。
pub fn write_bytes(root: &Path, file: &str, bytes: &[u8]) -> Result<(), ProjectError> {
    let path = root.join(file);
    let error = |path: &Path, source| ProjectError::Write {
        path: path.to_owned(),
        source,
    };
    let target = match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            fs::canonicalize(&path).map_err(|e| error(&path, e))?
        }
        _ => path.clone(),
    };
    let permissions = match fs::metadata(&target) {
        Ok(meta) if meta.permissions().readonly() => {
            let denied = io::Error::new(
                io::ErrorKind::PermissionDenied,
                "読み取り専用のファイルです",
            );
            return Err(error(&path, denied));
        }
        Ok(meta) => Some(meta.permissions()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(error(&path, e)),
    };
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = target.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let result = write_synced(&temp, bytes).and_then(|()| {
        if let Some(permissions) = permissions {
            fs::set_permissions(&temp, permissions)?;
        }
        fs::rename(&temp, &target)
    });
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map_err(|e| error(&path, e))
}

/// ファイルを作って書き、ディスクへ書き出す。
fn write_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// ディレクトリの中身を、名前の順に返す。ディレクトリが無ければ空。
fn read_dir_sorted(dir: &Path) -> Result<Vec<PathBuf>, ProjectError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(ProjectError::Io {
                path: dir.to_owned(),
                source,
            });
        }
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| ProjectError::Io {
            path: dir.to_owned(),
            source,
        })?;
        paths.push(entry.path());
    }
    paths.sort();
    Ok(paths)
}

fn has_extension(path: &Path, ext: &str) -> bool {
    path.extension().is_some_and(|e| e == ext)
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// 文字列・数・真偽の値を、文字列にする。
fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// フロントマターの値（文字列・数・真偽）を、文字列にする。
fn json_scalar(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// `from` から後ろで、`id:` の値として書かれた `id` の位置（引用符は含まない）。
fn id_value_span(text: &str, from: usize, id: &str) -> Option<Span> {
    let rest = text.get(from..)?;
    for (at, _) in rest.match_indices("id:") {
        let before = rest.get(..at)?.chars().next_back();
        if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let after = rest.get(at + "id:".len()..)?;
        let value = after.trim_start_matches([' ', '\t', '"', '\'']);
        let value_at = from + at + "id:".len() + (after.len() - value.len());
        let ends_here = value
            .get(id.len()..)
            .and_then(|tail| tail.chars().next())
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
        if value.starts_with(id) && ends_here {
            return Some(Span::new(value_at, value_at + id.len()));
        }
    }
    None
}

/// 行（1から数える）と列（1から数える、文字の数）を、バイト位置にする。範囲の外なら終わりにする。
fn offset_of(text: &str, line: usize, column: usize) -> usize {
    let mut offset = 0;
    for (i, l) in text.split_inclusive('\n').enumerate() {
        if i + 1 == line {
            let in_line: usize = l
                .chars()
                .take(column.saturating_sub(1))
                .map(char::len_utf8)
                .sum();
            return offset + in_line.min(l.len());
        }
        offset += l.len();
    }
    text.len()
}

/// `キー:` で始まる最初の行（行頭の空白と `- ` を除く）の、キーの位置。見つからなければ先頭。
/// 設定ファイルの項目の誤り（E802）の位置に使う。
fn key_span(text: &str, from: usize, key: &str) -> Span {
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        if start < from {
            continue;
        }
        let trimmed = line.trim_start_matches([' ', '\t', '\u{feff}']);
        let trimmed = trimmed.strip_prefix("- ").unwrap_or(trimmed);
        let at = start + (line.len() - trimmed.len());
        for quoted in [key.to_owned(), format!("\"{key}\""), format!("'{key}'")] {
            if let Some(after) = trimmed.strip_prefix(quoted.as_str())
                && after
                    .trim_start_matches([' ', '\t'])
                    .starts_with([':', '='])
            {
                return Span::new(at, at + quoted.len());
            }
        }
    }
    Span::new(0, 0)
}

/// 型定義の中の、項目の定義（`key: <key>`）の位置。見つからなければ `fields` のキーの位置。
fn field_span(text: &str, key: &str) -> Span {
    let quoted = serde_json::to_string(key).unwrap_or_default();
    for written in [
        format!("key: {key}"),
        format!("key: {quoted}"),
        format!("key: '{key}'"),
    ] {
        let mut from = 0;
        while let Some(at) = text.get(from..).and_then(|rest| rest.find(&written)) {
            let start = from + at;
            let end = start + written.len();
            let next = text.get(end..).and_then(|rest| rest.chars().next());
            let before = text.get(..start).and_then(|head| head.chars().next_back());
            if next.is_none_or(|c| !(c.is_alphanumeric() || c == '_'))
                && before.is_none_or(|c| !(c.is_alphanumeric() || c == '_'))
            {
                return Span::new(start, end);
            }
            from = end;
        }
    }
    key_span(text, 0, "fields")
}

/// 先頭の `---` の行から、次の `---` の行までを取り出す。中身と、中身の始まりの位置と、
/// 閉じの `---` の行の後ろの位置を返す。
fn front_matter(text: &str) -> Option<(&str, usize, usize)> {
    let bom = if text.starts_with('\u{feff}') { 3 } else { 0 };
    let body = text.get(bom..)?;
    let mut lines = body.split_inclusive('\n');
    let first = lines.next()?;
    if first.trim_end() != "---" {
        return None;
    }
    let start = bom + first.len();
    let mut end = start;
    for line in lines {
        if line.trim_end() == "---" {
            return Some((text.get(start..end)?, start, end + line.len()));
        }
        end += line.len();
    }
    None
}

/// 読み込みの途中の状態。診断を集める。
struct Loader<'a> {
    root: &'a Path,
    diags: Vec<LoadDiagnostic>,
    /// 読んだエントリの本文（パスごと）。キャラクターの項目の誤りの位置を求めるのに使う。
    entry_texts: BTreeMap<String, String>,
}

impl Loader<'_> {
    /// プロジェクトの根からの相対パス（区切りは `/`）。
    fn relative(&self, path: &Path) -> String {
        path.strip_prefix(self.root)
            .unwrap_or(path)
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    }

    fn entry_text(&self, path: &str) -> &str {
        self.entry_texts.get(path).map_or("", String::as_str)
    }

    fn diag(&mut self, file: &str, code: Code, span: Span, message: impl Into<String>) {
        self.diags.push(LoadDiagnostic {
            file: file.to_owned(),
            diag: Diagnostic::new(code, span, message),
        });
    }

    /// ファイルを読む。無ければ `None`。読めなければ（UTF-8でないなど）E801 を出して `None`。
    fn read(&mut self, path: &Path) -> Option<String> {
        match fs::read(path) {
            Ok(bytes) => self.decode(path, bytes),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => {
                let file = self.relative(path);
                self.diag(
                    &file,
                    Code::E801,
                    Span::new(0, 0),
                    format!("{file} を読めません：{e}"),
                );
                None
            }
        }
    }

    fn decode(&mut self, path: &Path, bytes: Vec<u8>) -> Option<String> {
        match String::from_utf8(bytes) {
            Ok(text) => Some(text),
            Err(e) => {
                let file = self.relative(path);
                let at = e.utf8_error().valid_up_to();
                self.diag(
                    &file,
                    Code::E801,
                    Span::new(at, at),
                    format!("{file} はUTF-8で書かれていません。このファイルを読み飛ばします"),
                );
                None
            }
        }
    }

    /// YAMLを読む。構文が崩れていれば E801 を出して `None`。`text` は、ファイル（`whole`）の行と
    /// 行の番号がそろうようにしたもの（フロントマターは、前の行を空行で埋める）。先頭の BOM は読み飛ばす。
    fn yaml(&mut self, file: &str, whole: &str, text: &str) -> Option<Value> {
        let bom = text.starts_with('\u{feff}');
        match yaml_serde::from_str(without_bom(text)) {
            Ok(value) => Some(value),
            Err(e) => {
                let at = e.location().map_or(0, |l| {
                    let column = l.column() + usize::from(bom && l.line() == 1);
                    offset_of(whole, l.line(), column)
                });
                self.diag(
                    file,
                    Code::E801,
                    Span::new(at, at),
                    format!("{file} のYAMLを読めません：{e}。このファイルを読み飛ばします"),
                );
                None
            }
        }
    }

    // ---- AIタスクの定義（T3-6） ----

    /// `ai/*.yaml` を読む（下のフォルダは読まない）。読めないファイルは E801、項目の誤りは E802。
    fn ai_tasks(&mut self) -> Vec<AiTask> {
        let dir = self.root.join("ai");
        let Ok(entries) = fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "yaml"))
            .collect();
        paths.sort();
        let mut tasks = Vec::new();
        for path in paths {
            let file = self.relative(&path);
            let Some(id) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
                continue;
            };
            let Some(text) = self.read(&path) else {
                continue;
            };
            let Some(root) = self.yaml(&file, &text, &text) else {
                continue;
            };
            if !root.is_mapping() {
                self.diag(
                    &file,
                    Code::E802,
                    Span::new(0, 0),
                    format!("{file} は、name・level・context・prompt などのキーで書きます。このタスクを読み飛ばします"),
                );
                continue;
            }
            let (task, problems) = parse_task(&id, &file, &root);
            for (key, message) in problems {
                self.diag(&file, Code::E802, key_span(&text, 0, key), message);
            }
            tasks.push(task);
        }
        tasks
    }

    // ---- project.toml ----

    fn settings(&mut self, bytes: &[u8]) -> Settings {
        let path = self.root.join("project.toml");
        let file = "project.toml";
        let mut settings = Settings::default();
        let Some(text) = self.decode(&path, bytes.to_vec()) else {
            settings.reading_errors = true;
            return settings;
        };
        let table: toml::Table = match toml::from_str(&text) {
            Ok(table) => table,
            Err(e) => {
                let span = e
                    .span()
                    .map_or(Span::new(0, 0), |r| Span::new(r.start, r.end));
                self.diag(
                    file,
                    Code::E801,
                    span,
                    format!(
                        "project.toml を読めません：{}。既定の設定で読みます",
                        e.message()
                    ),
                );
                settings.reading_errors = true;
                return settings;
            }
        };
        let diags_before_version = self.diags.len();
        match table.get("format_version") {
            Some(toml::Value::Integer(v)) if *v == FORMAT_VERSION => {}
            Some(toml::Value::Integer(v)) => self.diag(
                file,
                Code::E803,
                key_span(&text, 0, "format_version"),
                format!(
                    "format_version {v} には対応していません（対応しているのは {FORMAT_VERSION}）。v0として読みます"
                ),
            ),
            Some(_) => self.diag(
                file,
                Code::E802,
                key_span(&text, 0, "format_version"),
                "format_version は整数で書きます。v0として読みます",
            ),
            None => self.diag(
                file,
                Code::E802,
                Span::new(0, 0),
                "format_version がありません。v0として読みます",
            ),
        }
        settings.reading_errors |= self.diags.len() > diags_before_version;
        match table.get("title") {
            None => {}
            Some(toml::Value::String(title)) => settings.title = Some(title.clone()),
            Some(_) => self.diag(
                file,
                Code::E802,
                key_span(&text, 0, "title"),
                "title は文字列で書きます",
            ),
        }
        match table.get("start_scene") {
            None => {}
            Some(toml::Value::String(id)) => {
                settings.start_scene = Some(id.clone());
                settings.start_scene_span = key_span(&text, 0, "start_scene");
            }
            Some(_) => self.diag(
                file,
                Code::E802,
                key_span(&text, 0, "start_scene"),
                "start_scene はシーンIDの文字列で書きます",
            ),
        }
        match table.get("message_window") {
            None => {}
            Some(toml::Value::Table(window)) => {
                let size = |key: &str| match window.get(key) {
                    Some(toml::Value::Integer(n)) if *n > 0 => u32::try_from(*n).ok(),
                    _ => None,
                };
                match (size("lines"), size("chars_per_line")) {
                    (Some(lines), Some(chars_per_line)) => {
                        settings.message_window = Some(MessageWindow {
                            lines,
                            chars_per_line,
                        });
                    }
                    _ => self.diag(
                        file,
                        Code::E802,
                        text.find("[message_window]")
                            .map_or(Span::new(0, 0), |at| Span::new(at, at + "[message_window]".len())),
                        "[message_window] の lines と chars_per_line は、正の整数で書きます。はみ出しを調べません",
                    ),
                }
            }
            Some(_) => self.diag(
                file,
                Code::E802,
                key_span(&text, 0, "message_window"),
                "message_window は [message_window] の表で書きます。はみ出しを調べません",
            ),
        }
        match table.get("screen") {
            None => {}
            Some(toml::Value::Table(screen)) => {
                let size = |key: &str| match screen.get(key) {
                    Some(toml::Value::Integer(n)) if *n > 0 => u32::try_from(*n).ok(),
                    _ => None,
                };
                match (size("width"), size("height")) {
                    (Some(width), Some(height)) => {
                        settings.screen = Some(Screen { width, height });
                    }
                    _ => self.diag(
                        file,
                        Code::E802,
                        text.find("[screen]")
                            .map_or(Span::new(0, 0), |at| Span::new(at, at + "[screen]".len())),
                        "[screen] の width と height は、正の整数で書きます。プレビューは 1280×720 で見せます",
                    ),
                }
            }
            Some(_) => self.diag(
                file,
                Code::E802,
                key_span(&text, 0, "screen"),
                "screen は [screen] の表で書きます。プレビューは 1280×720 で見せます",
            ),
        }
        match table.get("editor") {
            None => {}
            Some(toml::Value::Table(editor)) => {
                for (key, slot) in [
                    (
                        "normalize_ellipsis",
                        &mut settings.editor.normalize_ellipsis,
                    ),
                    ("indent_narration", &mut settings.editor.indent_narration),
                ] {
                    match editor.get(key) {
                        None => {}
                        Some(toml::Value::Boolean(value)) => *slot = *value,
                        Some(_) => self.diag(
                            file,
                            Code::E802,
                            key_span(&text, 0, key),
                            format!(
                                "[editor] の {key} は true か false で書きます。切として扱います"
                            ),
                        ),
                    }
                }
            }
            Some(_) => self.diag(
                file,
                Code::E802,
                key_span(&text, 0, "editor"),
                "editor は [editor] の表で書きます。入力補助を切として扱います",
            ),
        }
        settings.ai = self.ai_connections(&text, table.get("ai"));
        // ここから下（`[dialogue]`）の誤りは、本文の読み方に関わる。
        let diags_before_dialogue = self.diags.len();
        let brackets = match table.get("dialogue") {
            None => None,
            Some(toml::Value::Table(dialogue)) => dialogue.get("brackets"),
            Some(_) => {
                self.diag(
                    file,
                    Code::E802,
                    key_span(&text, 0, "dialogue"),
                    "dialogue は [dialogue] の表で書きます。既定の「」を使います",
                );
                None
            }
        };
        match brackets {
            None => {}
            Some(toml::Value::Table(brackets)) => {
                let parsed = self.brackets(&text, brackets);
                if !parsed.is_empty() {
                    settings.brackets = parsed;
                }
            }
            Some(_) => self.diag(
                file,
                Code::E802,
                key_span(&text, 0, "brackets"),
                "[dialogue.brackets] は、括弧をキーにした表で書きます。既定の「」を使います",
            ),
        }
        settings.reading_errors |= self.diags.len() > diags_before_dialogue;
        settings
    }

    /// `[ai.<名前>]` を読む（T3-5、D066）。表でない接続先と、`provider`・`base_url`・`model` が文字列でない
    /// 接続先は、E802 を出して読み飛ばす。ほかのキーは読まない。
    fn ai_connections(&mut self, text: &str, value: Option<&toml::Value>) -> Vec<AiConnectionDecl> {
        let file = "project.toml";
        let table = match value {
            None => return Vec::new(),
            Some(toml::Value::Table(table)) => table,
            Some(_) => {
                self.diag(
                    file,
                    Code::E802,
                    key_span(text, 0, "ai"),
                    "ai は [ai.<名前>] の表で書きます。AIの接続先を読みません",
                );
                return Vec::new();
            }
        };
        let mut out = Vec::new();
        for (name, value) in table {
            let header = text.find(&format!("[ai.{name}]")).unwrap_or(0);
            let toml::Value::Table(conn) = value else {
                self.diag(
                    file,
                    Code::E802,
                    key_span(text, 0, name),
                    format!("[ai.{name}] は表で書きます。この接続先を読み飛ばします"),
                );
                continue;
            };
            let mut decl = AiConnectionDecl {
                name: name.clone(),
                ..AiConnectionDecl::default()
            };
            let mut ok = true;
            for (key, slot) in [
                ("provider", &mut decl.provider),
                ("base_url", &mut decl.base_url),
                ("model", &mut decl.model),
            ] {
                match conn.get(key) {
                    None => {}
                    Some(toml::Value::String(value)) => *slot = Some(value.clone()),
                    Some(_) => {
                        self.diag(
                            file,
                            Code::E802,
                            key_span(text, header, key),
                            format!(
                                "[ai.{name}] の {key} は文字列で書きます。この接続先を読み飛ばします"
                            ),
                        );
                        ok = false;
                    }
                }
            }
            match conn.get("context_tokens") {
                None => {}
                Some(toml::Value::Integer(n)) if *n > 0 => {
                    decl.context_tokens = u64::try_from(*n).ok();
                }
                Some(_) => {
                    self.diag(
                        file,
                        Code::E802,
                        key_span(text, header, "context_tokens"),
                        format!(
                            "[ai.{name}] の context_tokens は正の整数で書きます。この接続先を読み飛ばします"
                        ),
                    );
                    ok = false;
                }
            }
            if ok {
                out.push(decl);
            }
        }
        out
    }

    /// `[dialogue.brackets]` を読む（仕様書6節）。キーは互いに違う開き括弧と閉じ括弧の2文字、
    /// 値は英字で始まる表示スタイルの名前。合わない組は E802 を出して読み飛ばす。
    fn brackets(&mut self, text: &str, table: &toml::Table) -> Vec<DialogueBracket> {
        let file = "project.toml";
        let mut brackets: Vec<DialogueBracket> = Vec::new();
        for (key, style) in table {
            let span = key_span(text, 0, key);
            let chars: Vec<char> = key.chars().collect();
            let (open, close) = match chars.as_slice() {
                [open, close] if open != close => (*open, *close),
                _ => {
                    self.diag(
                        file,
                        Code::E802,
                        span,
                        format!(
                            "[dialogue.brackets] のキー \"{key}\" は、互いに違う開き括弧と閉じ括弧の2文字で書きます"
                        ),
                    );
                    continue;
                }
            };
            let style = style.as_str().unwrap_or_default();
            let valid_style = style
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
                && style
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
            if !valid_style {
                self.diag(
                    file,
                    Code::E802,
                    span,
                    format!(
                        "[dialogue.brackets] の \"{key}\" の表示スタイルは英字の名前で書きます"
                    ),
                );
                continue;
            }
            if brackets
                .iter()
                .any(|b| [b.open, b.close].contains(&open) || [b.open, b.close].contains(&close))
            {
                self.diag(
                    file,
                    Code::E802,
                    span,
                    format!(
                        "[dialogue.brackets] の \"{key}\" の括弧が、ほかのキーと重なっています"
                    ),
                );
                continue;
            }
            brackets.push(DialogueBracket {
                open,
                close,
                style: style.to_owned(),
            });
        }
        brackets
    }

    // ---- 設定DB ----

    /// 設定DBを読む。型（`types/*.yaml`）とエントリ（`entries/<型>/*.md`）。
    fn bible(&mut self) -> Result<Bible, ProjectError> {
        let dir = self.root.join("bible");
        let mut bible = Bible::default();
        let mut type_files = BTreeSet::new();
        for path in read_dir_sorted(&dir.join("types"))? {
            if !has_extension(&path, "yaml") {
                continue;
            }
            type_files.insert(file_stem(&path));
            if let Some(entry_type) = self.entry_type(&path) {
                bible.types.push(entry_type);
            }
        }
        for type_dir in read_dir_sorted(&dir.join("entries"))? {
            if !type_dir.is_dir() {
                continue;
            }
            let type_id = file_stem(&type_dir);
            if !type_files.contains(&type_id) {
                let folder = self.relative(&type_dir);
                self.diag(
                    &folder,
                    Code::W804,
                    Span::new(0, 0),
                    format!("{folder} の型の定義（bible/types/{type_id}.yaml）がありません"),
                );
            }
            for path in read_dir_sorted(&type_dir)? {
                if has_extension(&path, "md")
                    && let Some(entry) = self.entry(&type_id, &path)
                {
                    bible.entries.push(entry);
                }
            }
        }
        Ok(bible)
    }

    fn entry_type(&mut self, path: &Path) -> Option<EntryType> {
        let text = self.read(path)?;
        let file = self.relative(path);
        let root = self.yaml(&file, &text, &text)?;
        let id = file_stem(path);
        let name = root
            .get("name")
            .and_then(scalar)
            .unwrap_or_else(|| id.clone());
        let builtin = match root.get("builtin") {
            None | Some(Value::Null) => None,
            Some(value) => match scalar(value).as_deref() {
                Some("character") => Some("character".to_owned()),
                _ => {
                    self.diag(
                        &file,
                        Code::E802,
                        key_span(&text, 0, "builtin"),
                        "builtin に書けるのは character だけです。組み込みの型として扱いません",
                    );
                    None
                }
            },
        };
        let mut fields = Vec::new();
        match root.get("fields") {
            None | Some(Value::Null) => {}
            Some(Value::Sequence(items)) => {
                for item in items {
                    let key = item.get("key").and_then(scalar);
                    let ty = item.get("type").and_then(scalar);
                    let (Some(key), Some(ty)) = (key, ty) else {
                        self.diag(
                            &file,
                            Code::E802,
                            key_span(&text, 0, "fields"),
                            "fields の項目には key と type が要ります。その項目を読み飛ばします",
                        );
                        continue;
                    };
                    let Some(kind) = FieldType::parse(&ty) else {
                        self.diag(
                            &file,
                            Code::E802,
                            field_span(&text, &key),
                            format!(
                                "項目 {key} の型 {ty} を読めません（text・longtext・number・bool・date・select・multiselect・asset・ref<型>・list<型> のどれか）。その項目を読み飛ばします"
                            ),
                        );
                        continue;
                    };
                    let label = item
                        .get("label")
                        .and_then(scalar)
                        .unwrap_or_else(|| key.clone());
                    let options = match item.get("options") {
                        Some(Value::Sequence(options)) => {
                            options.iter().filter_map(scalar).collect()
                        }
                        _ => Vec::new(),
                    };
                    fields.push(FieldDef {
                        key,
                        label,
                        ty,
                        kind,
                        options,
                    });
                }
            }
            Some(_) => self.diag(
                &file,
                Code::E802,
                key_span(&text, 0, "fields"),
                "fields は項目の定義のリストで書きます",
            ),
        }
        Some(EntryType {
            id,
            name,
            builtin,
            fields,
        })
    }

    fn entry(&mut self, type_id: &str, path: &Path) -> Option<Entry> {
        let text = self.read(path)?;
        let file = self.relative(path);
        let (fields, body) = match front_matter(&text) {
            None => (
                BTreeMap::new(),
                text.trim_start_matches('\u{feff}').to_owned(),
            ),
            Some((yaml, start, end)) => {
                // 前の行（開きの `---`）を空行で埋め、誤りの行の番号をファイルとそろえる。
                let before = text.get(..start).unwrap_or_default().matches('\n').count();
                let padded = format!("{}{yaml}", "\n".repeat(before));
                let front = self.yaml(&file, &text, &padded)?;
                let fields = match front {
                    Value::Null => BTreeMap::new(),
                    Value::Mapping(map) => map
                        .into_iter()
                        .filter_map(|(k, v)| {
                            Some((scalar(&k)?, serde_json::to_value(&v).unwrap_or_default()))
                        })
                        .collect(),
                    _ => {
                        self.diag(
                            &file,
                            Code::E802,
                            Span::new(start, start),
                            "フロントマターは「キー: 値」の対応表で書きます。このエントリを読み飛ばします",
                        );
                        return None;
                    }
                };
                (fields, text.get(end..).unwrap_or_default().to_owned())
            }
        };
        let id = match fields.get("id") {
            None => file_stem(path),
            Some(value) if json_scalar(value).is_some() => json_scalar(value).unwrap_or_default(),
            Some(_) => {
                self.diag(
                    &file,
                    Code::E802,
                    key_span(&text, 0, "id"),
                    "id は文字列で書きます。このエントリを読み飛ばします",
                );
                return None;
            }
        };
        let key_spans = fields
            .keys()
            .map(|k| (k.clone(), key_span(&text, 0, k)))
            .collect();
        self.entry_texts.insert(file.clone(), text);
        Some(Entry {
            type_id: type_id.to_owned(),
            id,
            path: file,
            fields,
            key_spans,
            body,
        })
    }

    /// キャラクターの型（`builtin: character`）のエントリを、キャラクターにする。
    /// そういう型が無ければ（型定義が読めないときも）、フォルダ名 `character` の型を使う。
    fn characters(&mut self, bible: &Bible) -> Vec<Character> {
        let builtin: Vec<&EntryType> = bible
            .types
            .iter()
            .filter(|t| t.builtin.as_deref() == Some("character"))
            .collect();
        let type_id = match builtin.as_slice() {
            [] => bible.character_type().to_owned(),
            [first, rest @ ..] => {
                for extra in rest {
                    let file = format!("bible/types/{}.yaml", extra.id);
                    self.diag(
                        &file,
                        Code::E802,
                        Span::new(0, 0),
                        format!(
                            "builtin: character は1つの型にだけ書きます（{} の型を使います）",
                            first.id
                        ),
                    );
                }
                first.id.clone()
            }
        };
        let mut characters = Vec::new();
        for entry in bible.entries.iter().filter(|e| e.type_id == type_id) {
            let Some(name) = entry.fields.get("name").and_then(json_scalar) else {
                let span = key_span(self.entry_text(&entry.path), 0, "name");
                self.diag(
                    &entry.path,
                    Code::E802,
                    span,
                    "キャラクターには、フロントマターに文字列の name が要ります。このキャラクターを読み飛ばします",
                );
                continue;
            };
            let aliases = match entry.fields.get("aliases") {
                None | Some(serde_json::Value::Null) => Vec::new(),
                Some(serde_json::Value::Array(items))
                    if items.iter().all(|i| json_scalar(i).is_some()) =>
                {
                    items.iter().filter_map(json_scalar).collect()
                }
                Some(_) => {
                    let span = key_span(self.entry_text(&entry.path), 0, "aliases");
                    self.diag(
                        &entry.path,
                        Code::E802,
                        span,
                        "aliases は文字列のリストで書きます。呼び名を読み飛ばします",
                    );
                    Vec::new()
                }
            };
            characters.push(Character {
                id: entry.id.clone(),
                name,
                aliases,
            });
        }
        characters
    }

    // ---- variables.yaml ----

    /// 変数の宣言を読む。丸ごと読めなければ、2つ目を `true` にする（W602 を出さないため）。
    fn variables(&mut self) -> (Vec<VarDecl>, bool) {
        let path = self.root.join("variables.yaml");
        let file = "variables.yaml";
        if !path.exists() {
            return (Vec::new(), false);
        }
        let Some(text) = self.read(&path) else {
            return (Vec::new(), true);
        };
        let Some(root) = self.yaml(file, &text, &text) else {
            return (Vec::new(), true);
        };
        let map = match &root {
            Value::Null => return (Vec::new(), false),
            Value::Mapping(map) => map,
            _ => {
                self.diag(
                    file,
                    Code::E802,
                    Span::new(0, 0),
                    "variables.yaml は、変数名をキーにした対応表で書きます。宣言を読み飛ばします",
                );
                return (Vec::new(), true);
            }
        };
        let mut decls = Vec::new();
        for (key, def) in map {
            let Some(written) = scalar(key) else {
                self.diag(
                    file,
                    Code::E802,
                    Span::new(0, 0),
                    "変数名は文字列で書きます",
                );
                continue;
            };
            let span = key_span(&text, 0, &written);
            let key = match written.split_once('.') {
                Some((namespace, name))
                    if !namespace.is_empty() && !name.is_empty() && !name.contains('.') =>
                {
                    written.clone()
                }
                None if !written.is_empty() => format!("flag.{written}"),
                _ => {
                    self.diag(
                        file,
                        Code::E802,
                        span,
                        format!("変数名 {written} は「名前空間.名前」か「名前」で書きます。この変数を読み飛ばします"),
                    );
                    continue;
                }
            };
            let ty = match def.get("type").and_then(scalar).as_deref() {
                Some("bool") => VarType::Bool,
                Some("int") => VarType::Int,
                Some("string") => VarType::String,
                Some("enum") => {
                    let values: Option<Vec<String>> = match def.get("values") {
                        Some(Value::Sequence(items)) if !items.is_empty() => {
                            items.iter().map(scalar).collect()
                        }
                        _ => None,
                    };
                    match values {
                        Some(values) => VarType::Enum(values),
                        None => {
                            self.diag(
                                file,
                                Code::E802,
                                span,
                                format!(
                                    "列挙の変数 {written} には values（文字列の値のリスト）が要ります。この変数を読み飛ばします"
                                ),
                            );
                            continue;
                        }
                    }
                }
                other => {
                    self.diag(
                        file,
                        Code::E802,
                        span,
                        format!(
                            "{written} の type が不正です：{}（bool・int・string・enum のどれか）。この変数を読み飛ばします",
                            other.unwrap_or("なし")
                        ),
                    );
                    continue;
                }
            };
            let scope = def.get("scope").and_then(scalar);
            let default = def.get("default").and_then(scalar);
            let range = match def.get("range") {
                Some(Value::Sequence(items)) => match items.as_slice() {
                    [min, max] => min.as_i64().zip(max.as_i64()),
                    _ => None,
                },
                _ => None,
            };
            // 値の検査（W807。T2-10、D061）。宣言は読み飛ばさない。
            for problem in declaration_problems(&ty, def, range, scope.as_deref()) {
                self.diag(file, Code::W807, span, format!("変数 {written}：{problem}"));
            }
            let desc = def.get("desc").and_then(scalar);
            decls.push(VarDecl {
                key,
                ty,
                scope,
                default,
                range,
                desc,
                span,
            });
        }
        (decls, false)
    }

    // ---- assets/catalog.yaml ----

    fn catalog(&mut self) -> AssetCatalog {
        let path = self.root.join("assets").join("catalog.yaml");
        let file = "assets/catalog.yaml";
        let Some(text) = self.read(&path) else {
            return AssetCatalog::default();
        };
        let Some(root) = self.yaml(file, &text, &text) else {
            return AssetCatalog::default();
        };
        if root.is_null() {
            return AssetCatalog::default();
        }
        if !root.is_mapping() {
            self.diag(
                file,
                Code::E802,
                Span::new(0, 0),
                "素材カタログは、backgrounds・characters・bgm・se の節で書きます。カタログを読み飛ばします",
            );
            return AssetCatalog::default();
        }
        let mut catalog = AssetCatalog {
            backgrounds: self.names(&text, &root, "backgrounds"),
            characters: BTreeMap::new(),
            bgm: self.names(&text, &root, "bgm"),
            se: self.names(&text, &root, "se"),
        };
        match root.get("characters") {
            None | Some(Value::Null) => {}
            Some(Value::Mapping(map)) => {
                for (id, entry) in map {
                    let Some(id) = scalar(id) else {
                        self.diag(
                            file,
                            Code::E802,
                            key_span(&text, 0, "characters"),
                            "characters のキーはキャラクターIDで書きます",
                        );
                        continue;
                    };
                    let expressions = match entry.get("expressions") {
                        None | Some(Value::Null) => BTreeSet::new(),
                        Some(Value::Mapping(map)) => {
                            self.keys(&text, map, &format!("characters.{id}.expressions"))
                        }
                        Some(_) => {
                            self.diag(
                                file,
                                Code::E802,
                                key_span(&text, 0, &id),
                                format!(
                                    "characters.{id}.expressions は差分の名前をキーにした対応表で書きます。差分を読み飛ばします"
                                ),
                            );
                            BTreeSet::new()
                        }
                    };
                    catalog.characters.insert(id, expressions);
                }
            }
            Some(_) => self.diag(
                file,
                Code::E802,
                key_span(&text, 0, "characters"),
                "characters はキャラクターIDをキーにした対応表で書きます。この節を読み飛ばします",
            ),
        }
        catalog
    }

    /// 対応表のキー（意味名）。文字列にできないキーは E802 を出して読み飛ばす。
    fn keys(&mut self, text: &str, map: &yaml_serde::Mapping, section: &str) -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        for key in map.keys() {
            match scalar(key) {
                Some(name) => {
                    names.insert(name);
                }
                None => self.diag(
                    "assets/catalog.yaml",
                    Code::E802,
                    key_span(text, 0, section.rsplit('.').next().unwrap_or(section)),
                    format!("{section} の意味名は文字列で書きます。その意味名を読み飛ばします"),
                ),
            }
        }
        names
    }

    /// カタログの節（意味名をキーにした対応表）の意味名。合わなければ E802 を出して空にする。
    fn names(&mut self, text: &str, root: &Value, section: &str) -> BTreeSet<String> {
        match root.get(section) {
            None | Some(Value::Null) => BTreeSet::new(),
            Some(Value::Mapping(map)) => self.keys(text, map, section),
            Some(_) => {
                self.diag(
                    "assets/catalog.yaml",
                    Code::E802,
                    key_span(text, 0, section),
                    format!(
                        "{section} は意味名をキーにした対応表で書きます。この節を読み飛ばします"
                    ),
                );
                BTreeSet::new()
            }
        }
    }

    // ---- concept.md ----

    /// 企画書のフロントマターから、エンディングの一覧を読む。無ければ `None`。
    fn concept(&mut self) -> Option<Concept> {
        let path = self.root.join("concept.md");
        let file = "concept.md";
        let text = self.read(&path)?;
        let Some((yaml, start, _)) = front_matter(&text) else {
            return Some(Concept::default());
        };
        let before = text.get(..start).unwrap_or_default().matches('\n').count();
        let padded = format!("{}{yaml}", "\n".repeat(before));
        let front = self.yaml(file, &text, &padded)?;
        let endings = match front.get("endings") {
            None | Some(Value::Null) => None,
            Some(Value::Sequence(items)) => {
                let mut endings = Vec::new();
                let fallback = key_span(&text, start, "endings");
                let mut from = fallback.end.max(start);
                for item in items {
                    match item.get("id").and_then(scalar) {
                        Some(id) => {
                            let span = id_value_span(&text, from, &id).unwrap_or(fallback);
                            from = span.end.max(from);
                            endings.push(ConceptEnding { id, span });
                        }
                        None => self.diag(
                            file,
                            Code::E802,
                            key_span(&text, start, "endings"),
                            "endings の要素は、id を持つ対応表（{ id: …, name: … }）で書きます。その要素を読み飛ばします",
                        ),
                    }
                }
                Some(endings)
            }
            Some(_) => {
                self.diag(
                    file,
                    Code::E802,
                    key_span(&text, start, "endings"),
                    "endings はエンディングのリストで書きます",
                );
                None
            }
        };
        Some(Concept { endings })
    }

    // ---- story/ ----

    /// `story/` の下（下のフォルダも。T2-10、D061）の `.scn` を読み、パスの辞書順に並べる。隠しフォルダと隠し
    /// ファイル（`.` で始まるもの）は読まない。読めないファイル（UTF-8でないなど）は E801 を出して読み飛ばす。
    /// 下のフォルダを読めなければ、そのフォルダを読み飛ばす。シンボリックリンクで同じフォルダを2回読まない。
    fn story(&mut self) -> Result<Vec<SourceFile>, ProjectError> {
        let root = self.root.join("story");
        // `story/` そのものを読めなければ、読み込みのエラー。
        read_dir_sorted(&root)?;
        let (paths, unreadable) = scn_paths(&root);
        for dir in unreadable {
            let file = self.relative(&dir);
            self.diag(
                &file,
                Code::E801,
                Span::new(0, 0),
                format!("フォルダ {file} を読めません。中の章を読み飛ばします"),
            );
        }
        let mut files = Vec::new();
        for path in paths {
            if let Some(text) = self.read(&path) {
                files.push(SourceFile {
                    path: self.relative(&path),
                    text,
                });
            }
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(files)
    }
}

/// `dir` の下（下のフォルダも）の `*.scn` のパスと、読めなかった下のフォルダ（T2-10、D061）。`.` で始まる
/// フォルダとファイルは読まない。シンボリックリンクで同じフォルダに戻っても、一度だけ読む。
pub(crate) fn scn_paths(dir: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut found = Vec::new();
    let mut unreadable = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut dirs = vec![dir.to_owned()];
    while let Some(dir) = dirs.pop() {
        if !seen.insert(fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone())) {
            continue;
        }
        let Ok(paths) = read_dir_sorted(&dir) else {
            unreadable.push(dir);
            continue;
        };
        for path in paths {
            let hidden = path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'));
            if hidden {
                continue;
            }
            if path.is_dir() {
                dirs.push(path);
            } else if path.is_file() && has_extension(&path, "scn") {
                found.push(path);
            }
        }
    }
    found.sort();
    unreadable.sort();
    (found, unreadable)
}

/// 変数の宣言の値の問題（W807。T2-10、D061）：`default` が型に合わない（`bool` は真偽、`int` は整数、`enum` は
/// `values` のどれか。`string` は文字列・数・真偽のどれでもよい）、`range` が `[最小, 最大]` の整数でない・最小が
/// 最大より大きい・`int` でない変数にある、`default` が `range` の外、`scope` が `save`・`persistent` のどちらでもない。
fn declaration_problems(
    ty: &VarType,
    def: &Value,
    range: Option<(i64, i64)>,
    scope: Option<&str>,
) -> Vec<String> {
    let mut out = Vec::new();
    let default = def.get("default").filter(|v| !v.is_null());
    if let Some(value) = default {
        let fits = match ty {
            VarType::Bool => value.is_bool(),
            VarType::Int => value.as_i64().is_some(),
            VarType::String => scalar(value).is_some(),
            VarType::Enum(values) => scalar(value).is_some_and(|v| values.contains(&v)),
        };
        if !fits {
            out.push(match ty {
                VarType::Enum(_) => "default が values のどれでもありません".to_owned(),
                _ => format!("default が型（{}）に合いません", ty.name()),
            });
        }
    }
    if let Some(written) = def.get("range").filter(|v| !v.is_null()) {
        if !matches!(ty, VarType::Int) {
            out.push("range は int の変数にだけ書けます".to_owned());
        } else {
            match range {
                None => out.push(format!(
                    "range は [最小, 最大] の整数で書きます：{}",
                    serde_json::to_string(written).unwrap_or_default()
                )),
                Some((min, max)) if min > max => out.push(format!(
                    "range の最小（{min}）が最大（{max}）より大きくなっています"
                )),
                Some((min, max)) => {
                    if let Some(v) = default.and_then(Value::as_i64)
                        && (v < min || v > max)
                    {
                        out.push(format!("default（{v}）が range [{min}, {max}] の外です"));
                    }
                }
            }
        }
    }
    if let Some(scope) = scope
        && scope != "save"
        && scope != "persistent"
    {
        out.push(format!("scope「{scope}」は save か persistent で書きます"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_lines() {
        assert_eq!(
            front_matter("---\nid: a\n---\n本文"),
            Some(("id: a\n", 4, 14))
        );
        assert_eq!(
            front_matter("---\r\nid: a\r\n---\r\n"),
            Some(("id: a\r\n", 5, 17))
        );
        assert_eq!(
            front_matter("\u{feff}---\nid: a\n---\n"),
            Some(("id: a\n", 7, 17))
        );
        assert_eq!(front_matter("本文"), None);
        assert_eq!(front_matter("---\nid: a\n"), None);
    }

    #[test]
    fn id_values() {
        let text = "logline: a good story about the end\nendings:\n  - { id: good, name: 良い }\n  - { id: \"end\", name: 終 }\n";
        let from = key_span(text, 0, "endings").end;
        let span = id_value_span(text, from, "good").unwrap();
        assert_eq!(text.get(span.start..span.end), Some("good"));
        let span = id_value_span(text, span.end, "end").unwrap();
        assert_eq!(text.get(span.start..span.end), Some("end"));
        assert!(text[..span.start].ends_with("id: \""));
        assert_eq!(id_value_span(text, from, "goo"), None);
        assert_eq!(id_value_span("valid: x\n", 0, "x"), None);
    }

    #[test]
    fn offsets_and_key_spans() {
        let text = "あ: 1\nいう: [\n  - x\n";
        assert_eq!(offset_of(text, 1, 1), 0);
        assert_eq!(offset_of(text, 2, 2), "あ: 1\n".len() + "い".len());
        assert_eq!(offset_of(text, 9, 1), text.len());
        let span = key_span(text, 0, "いう");
        assert_eq!(text.get(span.start..span.end), Some("いう"));
        assert_eq!(key_span(text, 0, "x"), Span::new(0, 0));
        let toml = "[dialogue.brackets]\n\"「」\" = \"normal\"\n";
        let span = key_span(toml, 0, "「」");
        assert_eq!(toml.get(span.start..span.end), Some("\"「」\""));
    }
}
