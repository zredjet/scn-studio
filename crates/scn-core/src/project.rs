//! プロジェクトフォルダの読み込み。
//!
//! `project.toml`、設定DB（`bible/`）、`variables.yaml`、`assets/catalog.yaml`、
//! `story/*.scn` を読む。設定ファイルが読めなければ、パスと理由つきのエラーにして止める
//! （D036。診断コードはフェーズ1で足す）。無いファイルは空として扱う。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use yaml_serde::Value;

use crate::syntax::{DialogueBracket, ParseOptions};

/// 対応している形式のバージョン（`project.toml` の `format_version`）。
pub const FORMAT_VERSION: i64 = 0;

/// 読み込んだプロジェクト。
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    pub settings: Settings,
    pub characters: Vec<Character>,
    pub variables: Vec<VarDecl>,
    pub catalog: AssetCatalog,
    /// `story/*.scn`。パスの辞書順に並べてある。
    pub files: Vec<SourceFile>,
}

/// `project.toml` のうち、解析に使う設定。
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub format_version: i64,
    pub start_scene: Option<String>,
    pub brackets: Vec<DialogueBracket>,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VarType {
    Bool,
    Int,
    String,
    Enum(Vec<String>),
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

/// プロジェクトの読み込みと、本文の書き換え（[`write_source`]）のエラー。
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("{path} を読めません：{source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{path} の形が不正です：{message}")]
    Invalid { path: PathBuf, message: String },
    #[error("{path} に書けません：{source}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

impl ProjectError {
    fn invalid(path: &Path, message: impl Into<String>) -> Self {
        ProjectError::Invalid {
            path: path.to_owned(),
            message: message.into(),
        }
    }
}

impl Project {
    /// 構文解析の設定（台詞の括弧）。
    pub fn parse_options(&self) -> ParseOptions {
        ParseOptions {
            brackets: self.settings.brackets.clone(),
        }
    }
}

/// プロジェクトフォルダを読み込む。
pub fn load_project(root: &Path) -> Result<Project, ProjectError> {
    Ok(Project {
        settings: load_settings(&root.join("project.toml"))?,
        characters: load_bible(&root.join("bible"))?,
        variables: load_variables(&root.join("variables.yaml"))?,
        catalog: load_catalog(&root.join("assets").join("catalog.yaml"))?,
        files: load_story(root)?,
    })
}

/// 本文のファイルを書き換える（`scn fmt`、保存）。`file` はプロジェクトの根からの相対パス
/// （`SourceFile::path`）。中身は渡したバイトのまま書く。
///
/// 同じフォルダに一時ファイルを書き、ディスクへ書き出してから置き換えるので、途中でプロセスが
/// 止まっても、元のファイルは壊れない。読み取り専用のファイルは書き換えない（OSによらず）。
/// シンボリックリンクは、リンクを残したまま、リンク先を書き換える。
pub fn write_source(root: &Path, file: &str, text: &str) -> Result<(), ProjectError> {
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
    let result = write_synced(&temp, text).and_then(|()| {
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
fn write_synced(path: &Path, text: &str) -> io::Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()
}

fn read(path: &Path) -> Result<String, ProjectError> {
    fs::read_to_string(path).map_err(|source| ProjectError::Io {
        path: path.to_owned(),
        source,
    })
}

/// ファイルがあれば読む。無ければ `None`。
fn read_optional(path: &Path) -> Result<Option<String>, ProjectError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ProjectError::Io {
            path: path.to_owned(),
            source,
        }),
    }
}

fn parse_yaml(path: &Path, text: &str) -> Result<Value, ProjectError> {
    yaml_serde::from_str(text).map_err(|e| ProjectError::invalid(path, e.to_string()))
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

// ---- project.toml ----

#[derive(Deserialize)]
struct ProjectToml {
    format_version: i64,
    start_scene: Option<String>,
    #[serde(default)]
    dialogue: DialogueToml,
}

#[derive(Deserialize, Default)]
struct DialogueToml {
    brackets: Option<BTreeMap<String, String>>,
}

fn load_settings(path: &Path) -> Result<Settings, ProjectError> {
    let text = read(path)?;
    let parsed: ProjectToml =
        toml::from_str(&text).map_err(|e| ProjectError::invalid(path, e.to_string()))?;
    if parsed.format_version != FORMAT_VERSION {
        return Err(ProjectError::invalid(
            path,
            format!(
                "format_version {} には対応していません（対応しているのは {FORMAT_VERSION}）",
                parsed.format_version
            ),
        ));
    }
    let brackets = match parsed.dialogue.brackets {
        None => ParseOptions::default().brackets,
        Some(table) => parse_brackets(path, &table)?,
    };
    Ok(Settings {
        format_version: parsed.format_version,
        start_scene: parsed.start_scene,
        brackets,
    })
}

/// `[dialogue.brackets]` を読む（仕様書6節）。キーは互いに違う開き括弧と閉じ括弧の2文字、
/// 値は英字で始まる表示スタイルの名前。
fn parse_brackets(
    path: &Path,
    table: &BTreeMap<String, String>,
) -> Result<Vec<DialogueBracket>, ProjectError> {
    let mut brackets: Vec<DialogueBracket> = Vec::new();
    for (key, style) in table {
        let chars: Vec<char> = key.chars().collect();
        let (open, close) = match chars.as_slice() {
            [open, close] if open != close => (*open, *close),
            _ => {
                return Err(ProjectError::invalid(
                    path,
                    format!(
                        "[dialogue.brackets] のキー \"{key}\" は、互いに違う開き括弧と閉じ括弧の2文字で書きます"
                    ),
                ));
            }
        };
        let valid_style = style
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
            && style
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if !valid_style {
            return Err(ProjectError::invalid(
                path,
                format!("[dialogue.brackets] の表示スタイル \"{style}\" は英字の名前で書きます"),
            ));
        }
        if brackets
            .iter()
            .any(|b| [b.open, b.close].contains(&open) || [b.open, b.close].contains(&close))
        {
            return Err(ProjectError::invalid(
                path,
                format!("[dialogue.brackets] の \"{key}\" の括弧が、ほかのキーと重なっています"),
            ));
        }
        brackets.push(DialogueBracket {
            open,
            close,
            style: style.clone(),
        });
    }
    Ok(brackets)
}

// ---- 設定DB ----

/// 設定DBを読む。型（`types/*.yaml`）とエントリ（`entries/<型>/*.md`）がすべて読めることを確かめ、
/// キャラクター（`entries/character/`）を返す。
fn load_bible(dir: &Path) -> Result<Vec<Character>, ProjectError> {
    for path in read_dir_sorted(&dir.join("types"))? {
        if has_extension(&path, "yaml") {
            parse_yaml(&path, &read(&path)?)?;
        }
    }
    let mut characters = Vec::new();
    for type_dir in read_dir_sorted(&dir.join("entries"))? {
        if !type_dir.is_dir() {
            continue;
        }
        let is_character = type_dir.file_name().is_some_and(|n| n == "character");
        for path in read_dir_sorted(&type_dir)? {
            if !has_extension(&path, "md") {
                continue;
            }
            let text = read(&path)?;
            let front = match front_matter(&text) {
                Some(yaml) => parse_yaml(&path, yaml)?,
                None if is_character => {
                    return Err(ProjectError::invalid(
                        &path,
                        "キャラクターのエントリには、--- で囲んだフロントマターが要ります",
                    ));
                }
                None => continue,
            };
            if is_character {
                characters.push(character(&path, &front)?);
            }
        }
    }
    Ok(characters)
}

/// 先頭の `---` の行から、次の `---` の行までを取り出す。
fn front_matter(text: &str) -> Option<&str> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.split_inclusive('\n');
    let first = lines.next()?;
    if first.trim_end() != "---" {
        return None;
    }
    let start = first.len();
    let mut end = start;
    for line in lines {
        if line.trim_end() == "---" {
            return text.get(start..end);
        }
        end += line.len();
    }
    None
}

fn character(path: &Path, front: &Value) -> Result<Character, ProjectError> {
    let id = match front.get("id") {
        None => file_stem(path),
        Some(value) => {
            scalar(value).ok_or_else(|| ProjectError::invalid(path, "id は文字列で書きます"))?
        }
    };
    let name = front
        .get("name")
        .and_then(scalar)
        .ok_or_else(|| ProjectError::invalid(path, "キャラクターに name がありません"))?;
    let aliases = match front.get("aliases") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Sequence(items)) => items
            .iter()
            .map(|item| {
                scalar(item).ok_or_else(|| {
                    ProjectError::invalid(path, "aliases は文字列のリストで書きます")
                })
            })
            .collect::<Result<_, _>>()?,
        Some(_) => {
            return Err(ProjectError::invalid(
                path,
                "aliases は文字列のリストで書きます",
            ));
        }
    };
    Ok(Character { id, name, aliases })
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

// ---- variables.yaml ----

fn load_variables(path: &Path) -> Result<Vec<VarDecl>, ProjectError> {
    let Some(text) = read_optional(path)? else {
        return Ok(Vec::new());
    };
    let root = parse_yaml(path, &text)?;
    let map = match &root {
        Value::Null => return Ok(Vec::new()),
        Value::Mapping(map) => map,
        _ => {
            return Err(ProjectError::invalid(
                path,
                "変数名をキーにした対応表で書きます",
            ));
        }
    };
    let mut decls = Vec::new();
    for (key, def) in map {
        let Some(key) = scalar(key) else {
            return Err(ProjectError::invalid(path, "変数名は文字列で書きます"));
        };
        let key = match key.split_once('.') {
            Some((namespace, name))
                if !namespace.is_empty() && !name.is_empty() && !name.contains('.') =>
            {
                key.clone()
            }
            None if !key.is_empty() => format!("flag.{key}"),
            _ => {
                return Err(ProjectError::invalid(
                    path,
                    format!("変数名 {key} は「名前空間.名前」か「名前」で書きます"),
                ));
            }
        };
        let ty = match def.get("type").and_then(scalar).as_deref() {
            Some("bool") => VarType::Bool,
            Some("int") => VarType::Int,
            Some("string") => VarType::String,
            Some("enum") => {
                let values = match def.get("values") {
                    Some(Value::Sequence(items)) if !items.is_empty() => items
                        .iter()
                        .map(|v| {
                            scalar(v).ok_or_else(|| {
                                ProjectError::invalid(
                                    path,
                                    format!("{key} の values は文字列で書きます"),
                                )
                            })
                        })
                        .collect::<Result<_, _>>()?,
                    _ => {
                        return Err(ProjectError::invalid(
                            path,
                            format!("列挙の変数 {key} には values（値のリスト）が要ります"),
                        ));
                    }
                };
                VarType::Enum(values)
            }
            other => {
                return Err(ProjectError::invalid(
                    path,
                    format!(
                        "{key} の type が不正です：{}（bool・int・string・enum のどれか）",
                        other.unwrap_or("なし")
                    ),
                ));
            }
        };
        decls.push(VarDecl { key, ty });
    }
    Ok(decls)
}

// ---- assets/catalog.yaml ----

fn load_catalog(path: &Path) -> Result<AssetCatalog, ProjectError> {
    let Some(text) = read_optional(path)? else {
        return Ok(AssetCatalog::default());
    };
    let root = parse_yaml(path, &text)?;
    if root.is_null() {
        return Ok(AssetCatalog::default());
    }
    let names = |section: &str| -> Result<BTreeSet<String>, ProjectError> {
        match root.get(section) {
            None | Some(Value::Null) => Ok(BTreeSet::new()),
            Some(Value::Mapping(map)) => map
                .keys()
                .map(|k| {
                    scalar(k).ok_or_else(|| {
                        ProjectError::invalid(path, format!("{section} の意味名は文字列で書きます"))
                    })
                })
                .collect(),
            Some(_) => Err(ProjectError::invalid(
                path,
                format!("{section} は意味名をキーにした対応表で書きます"),
            )),
        }
    };
    let mut characters = BTreeMap::new();
    match root.get("characters") {
        None | Some(Value::Null) => {}
        Some(Value::Mapping(map)) => {
            for (id, entry) in map {
                let id = scalar(id).ok_or_else(|| {
                    ProjectError::invalid(path, "characters のキーはキャラクターIDで書きます")
                })?;
                let expressions = match entry.get("expressions") {
                    None | Some(Value::Null) => BTreeSet::new(),
                    Some(Value::Mapping(map)) => map
                        .keys()
                        .map(|k| {
                            scalar(k).ok_or_else(|| {
                                ProjectError::invalid(path, "立ち絵の差分の名前は文字列で書きます")
                            })
                        })
                        .collect::<Result<_, _>>()?,
                    Some(_) => {
                        return Err(ProjectError::invalid(
                            path,
                            format!(
                                "characters.{id}.expressions は差分の名前をキーにした対応表で書きます"
                            ),
                        ));
                    }
                };
                characters.insert(id, expressions);
            }
        }
        Some(_) => {
            return Err(ProjectError::invalid(
                path,
                "characters はキャラクターIDをキーにした対応表で書きます",
            ));
        }
    }
    Ok(AssetCatalog {
        backgrounds: names("backgrounds")?,
        characters,
        bgm: names("bgm")?,
        se: names("se")?,
    })
}

// ---- story/ ----

/// `story/*.scn` を読み、パスの辞書順に並べる。サブディレクトリは読まない（D036）。
fn load_story(root: &Path) -> Result<Vec<SourceFile>, ProjectError> {
    let mut files = Vec::new();
    for path in read_dir_sorted(&root.join("story"))? {
        if path.is_file() && has_extension(&path, "scn") {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            files.push(SourceFile {
                path: format!("story/{name}"),
                text: read(&path)?,
            });
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_lines() {
        assert_eq!(front_matter("---\nid: a\n---\n本文"), Some("id: a\n"));
        assert_eq!(front_matter("---\r\nid: a\r\n---\r\n"), Some("id: a\r\n"));
        assert_eq!(front_matter("本文"), None);
        assert_eq!(front_matter("---\nid: a\n"), None);
    }

    #[test]
    fn brackets_are_validated() {
        let path = Path::new("project.toml");
        let ok = BTreeMap::from([("「」".to_owned(), "normal".to_owned())]);
        assert_eq!(parse_brackets(path, &ok).unwrap().len(), 1);
        for (key, style) in [
            ("「", "normal"),
            ("「「", "normal"),
            ("「」", "心の声"),
            ("「」", ""),
        ] {
            let table = BTreeMap::from([(key.to_owned(), style.to_owned())]);
            assert!(parse_brackets(path, &table).is_err(), "{key} = {style}");
        }
        let overlap = BTreeMap::from([
            ("「」".to_owned(), "normal".to_owned()),
            ("「』".to_owned(), "thought".to_owned()),
        ]);
        assert!(parse_brackets(path, &overlap).is_err());
    }
}
