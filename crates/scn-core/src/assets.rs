//! 素材カタログ（T2-3、D054）：素材の一覧（意味名ごとの状態、使用回数、初出シーン、使用シーン）、発注リスト
//! （CSV）、カタログ（`assets/catalog.yaml`）の書き換え（意味名を足す、ファイルを割り当てる）。
//!
//! - 状態は、ファイルあり（カタログの `file` が `assets/` の中の実在するファイルを指す）、仮素材（カタログにあるが
//!   ファイルが無い）、未登録（本文で使われているがカタログに無い）。ファイルの読み方はプレビュー（D050）と同じ。
//! - カタログの書き換えは、設定DB（D049）と同じく、YAMLを書き出し直さずにテキストの行を足すか1行を差し替え、
//!   書いた後に読み直して、意図した変更だけが起きたことを確かめる（コメントや書き方を残すため）。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use yaml_serde::{Mapping, Value as Yaml};

use crate::index::{Index, Location};
use crate::project::{Project, ProjectError, without_bom, write_source};
use crate::script::csv_table;

/// 素材カタログのファイル（作品フォルダから）。
pub const CATALOG_FILE: &str = "assets/catalog.yaml";

/// 発注リストの既定の置き場所（作品フォルダから）。
pub const ORDERS_FILE: &str = "export/orders.csv";

// ---- カタログに書かれたファイル ----

/// カタログに書かれたファイル（`assets/` から数えたパス。立ち絵の差分は `dir` とつないだもの）と `loop`。
/// 形の合わない値は読み飛ばす（読み込みの診断は E802 で出している）。
#[derive(Debug, Default)]
pub(crate) struct CatalogFiles {
    pub backgrounds: BTreeMap<String, String>,
    pub expressions: BTreeMap<String, BTreeMap<String, String>>,
    pub bgm: BTreeMap<String, (Option<String>, Option<bool>)>,
    pub se: BTreeMap<String, (Option<String>, Option<bool>)>,
}

fn yaml_str<'v>(value: &'v Yaml, key: &str) -> Option<&'v str> {
    value.get(key).and_then(Yaml::as_str)
}

fn yaml_key(key: &Yaml) -> Option<String> {
    match key {
        Yaml::String(s) => Some(s.clone()),
        Yaml::Number(n) => Some(n.to_string()),
        Yaml::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

pub(crate) fn with_slash(dir: &str) -> String {
    if dir.is_empty() || dir.ends_with('/') {
        dir.to_owned()
    } else {
        format!("{dir}/")
    }
}

/// カタログに書かれたファイルを読む。読めなければ空。
pub(crate) fn catalog_files(root: &Path) -> CatalogFiles {
    let mut out = CatalogFiles::default();
    let Ok(text) = fs::read_to_string(root.join(CATALOG_FILE)) else {
        return out;
    };
    let Ok(yaml) = yaml_serde::from_str::<Yaml>(without_bom(&text)) else {
        return out;
    };
    let section = |name: &str| match yaml.get(name) {
        Some(Yaml::Mapping(map)) => map.iter().collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    for (key, entry) in section("backgrounds") {
        if let (Some(name), Some(file)) = (yaml_key(key), yaml_str(entry, "file")) {
            out.backgrounds.insert(name, file.to_owned());
        }
    }
    for (kind, target) in [("bgm", &mut out.bgm), ("se", &mut out.se)] {
        for (key, entry) in section(kind) {
            let Some(name) = yaml_key(key) else { continue };
            let file = yaml_str(entry, "file").map(str::to_owned);
            let repeat = entry.get("loop").and_then(Yaml::as_bool);
            target.insert(name, (file, repeat));
        }
    }
    for (key, entry) in section("characters") {
        let Some(id) = yaml_key(key) else { continue };
        let folder = yaml_str(entry, "dir").unwrap_or_default();
        let Some(Yaml::Mapping(map)) = entry.get("expressions") else {
            continue;
        };
        let slots = out.expressions.entry(id).or_default();
        for (name, value) in map {
            let Some(name) = yaml_key(name) else { continue };
            // 差分は `笑顔: smile.png` か `笑顔: { file: smile.png }`。`dir` とつなぐ。
            let file = value
                .as_str()
                .or_else(|| yaml_str(value, "file"))
                .map(|f| format!("{}{f}", with_slash(folder)));
            if let Some(file) = file {
                slots.insert(name, file);
            }
        }
    }
    out
}

/// `assets/` から数えたパスを、作品フォルダの中の実在するファイルの絶対パスにする。外を指すパスと
/// 無いファイルは `None`。
pub(crate) fn asset_file(assets: &Path, relative: &str) -> Option<String> {
    let rel = Path::new(relative);
    if relative.is_empty()
        || rel
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return None;
    }
    let path = assets.join(rel);
    let canonical = fs::canonicalize(&path).ok()?;
    let base = fs::canonicalize(assets).ok()?;
    (canonical.starts_with(&base) && canonical.is_file()).then(|| canonical.display().to_string())
}

/// 作品フォルダの `assets/` の中の実在するファイルを、`assets/` から数えたパス（`/` 区切り）にする。
/// 外を指すパスと、無いファイルは `None`。
pub fn assets_relative(file: &Path, root: &Path) -> Option<String> {
    let assets = fs::canonicalize(root.join("assets")).ok()?;
    let canonical = fs::canonicalize(file).ok()?;
    if !canonical.is_file() {
        return None;
    }
    let relative = canonical.strip_prefix(&assets).ok()?;
    let parts: Vec<String> = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

// ---- 素材の一覧 ----

/// 素材の状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetState {
    /// カタログにあり、ファイルが `assets/` の中にある。
    File,
    /// カタログにあるが、ファイルが無い（`{}`、または書いたファイルが見つからない）。
    Placeholder,
    /// 本文で使われているが、カタログに無い（I403）。
    Unregistered,
}

/// 素材の一覧の1行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AssetRow {
    /// `background`・`expression`・`bgm`・`se`。
    pub kind: &'static str,
    /// 立ち絵のキャラクターID（解決できなければ書かれた名前）。
    pub character: Option<String>,
    /// 立ち絵のキャラクターの名前（解決できなければ書かれた名前）。
    pub character_name: Option<String>,
    /// 意味名（立ち絵は差分の名前）。
    pub name: String,
    pub state: AssetState,
    /// カタログに書かれたファイル（`assets/` から。立ち絵は `dir` とつないだもの）。
    pub file: Option<String>,
    /// 実在するファイルの絶対パス（画面で見せる）。
    pub path: Option<String>,
    /// 本文で使われている位置（本文の順）。
    pub uses: Vec<Location>,
    /// 使用シーン（初めて使われた順、重なりなし）。最初が初出シーン。
    pub scenes: Vec<String>,
    /// カタログに足せるか（未登録で、立ち絵ならキャラクターが設定DBにある）。
    pub addable: bool,
}

/// 素材の一覧（種類・キャラクター・意味名の順）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AssetList {
    pub rows: Vec<AssetRow>,
    /// 立ち絵の表に出すキャラクター（設定DBの順。差分がカタログにあるか、本文で使われているもの）。
    pub characters: Vec<AssetCharacter>,
}

/// 立ち絵の表のキャラクター。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AssetCharacter {
    pub id: String,
    pub name: String,
    /// 設定DBにあるか（無ければ、本文に書かれた名前）。
    pub registered: bool,
}

/// 素材の一覧を作る。カタログの意味名と、本文で使われている意味名（索引）から。
pub fn asset_list(project: &Project, root: &Path, index: &Index) -> AssetList {
    let files = catalog_files(root);
    let dir = root.join("assets");
    let name_of = |id: &str| {
        project
            .characters
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.clone())
    };
    let mut rows = Vec::new();
    for asset in &index.assets {
        let file = match asset.kind {
            "background" => files.backgrounds.get(&asset.name).cloned(),
            "expression" => asset
                .character
                .as_ref()
                .and_then(|c| files.expressions.get(c))
                .and_then(|e| e.get(&asset.name))
                .cloned(),
            "bgm" => files.bgm.get(&asset.name).and_then(|(f, _)| f.clone()),
            "se" => files.se.get(&asset.name).and_then(|(f, _)| f.clone()),
            _ => None,
        };
        let path = file.as_deref().and_then(|f| asset_file(&dir, f));
        let state = match (asset.registered, &path) {
            (false, _) => AssetState::Unregistered,
            (true, Some(_)) => AssetState::File,
            (true, None) => AssetState::Placeholder,
        };
        let known = asset
            .character
            .as_deref()
            .is_some_and(|c| name_of(c).is_some());
        let mut scenes: Vec<String> = Vec::new();
        for location in &asset.uses {
            if !scenes.contains(&location.scene) {
                scenes.push(location.scene.clone());
            }
        }
        rows.push(AssetRow {
            kind: asset.kind,
            character_name: asset
                .character
                .as_deref()
                .map(|c| name_of(c).unwrap_or_else(|| c.to_owned())),
            character: asset.character.clone(),
            name: asset.name.clone(),
            state,
            file,
            path,
            uses: asset.uses.clone(),
            scenes,
            addable: state == AssetState::Unregistered && (asset.kind != "expression" || known),
        });
    }
    let used: BTreeSet<&str> = rows.iter().filter_map(|r| r.character.as_deref()).collect();
    let mut characters: Vec<AssetCharacter> = project
        .characters
        .iter()
        .filter(|c| used.contains(c.id.as_str()))
        .map(|c| AssetCharacter {
            id: c.id.clone(),
            name: c.name.clone(),
            registered: true,
        })
        .collect();
    for id in used {
        if !characters.iter().any(|c| c.id == id) {
            characters.push(AssetCharacter {
                id: id.to_owned(),
                name: id.to_owned(),
                registered: false,
            });
        }
    }
    AssetList { rows, characters }
}

/// 立ち絵の差分のうち、ファイルの無いもの（仮素材）。キャラクターIDごと（設定DBの画面の印）。
pub fn placeholder_expressions(project: &Project, root: &Path) -> BTreeMap<String, Vec<String>> {
    let files = catalog_files(root);
    let dir = root.join("assets");
    let mut out = BTreeMap::new();
    for (id, names) in &project.catalog.characters {
        let placeholders: Vec<String> = names
            .iter()
            .filter(|name| {
                files
                    .expressions
                    .get(id)
                    .and_then(|e| e.get(*name))
                    .and_then(|f| asset_file(&dir, f))
                    .is_none()
            })
            .cloned()
            .collect();
        if !placeholders.is_empty() {
            out.insert(id.clone(), placeholders);
        }
    }
    out
}

// ---- 発注リスト ----

/// 発注リストの列の見出し。
pub const ORDER_COLUMNS: [&str; 8] = [
    "種別",
    "名前",
    "キャラ",
    "差分",
    "使用回数",
    "初出",
    "使用シーン",
    "備考",
];

/// 発注リストの1行（まだファイルの無い素材）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OrderRow {
    /// 背景・立ち絵・BGM・SE。
    pub kind: &'static str,
    /// 意味名（立ち絵は `キャラの名前/差分`）。
    pub name: String,
    pub character: String,
    pub expression: String,
    pub uses: usize,
    pub first_scene: String,
    /// 使用シーン（空白区切り）。
    pub scenes: String,
    /// カタログに無い、本文で未使用、書いたファイルが見つからない。
    pub note: String,
}

fn kind_label(kind: &str) -> &'static str {
    match kind {
        "background" => "背景",
        "expression" => "立ち絵",
        "bgm" => "BGM",
        _ => "SE",
    }
}

/// 発注リスト：まだファイルの無い素材（仮素材と未登録）ごとの行。種類（背景・立ち絵・BGM・SE）、
/// キャラクター、意味名の順。
pub fn orders(list: &AssetList) -> Vec<OrderRow> {
    list.rows
        .iter()
        .filter(|r| r.state != AssetState::File)
        .map(|r| {
            let mut notes = Vec::new();
            if r.state == AssetState::Unregistered {
                notes.push("カタログに無い".to_owned());
            }
            let unknown = r
                .character
                .as_deref()
                .is_some_and(|id| !list.characters.iter().any(|c| c.id == id && c.registered));
            if unknown {
                notes.push("キャラクターが設定DBに無い".to_owned());
            }
            if let Some(file) = &r.file {
                notes.push(format!("ファイルが見つからない（{file}）"));
            }
            if r.uses.is_empty() {
                notes.push("本文で未使用".to_owned());
            }
            let character = r.character_name.clone().unwrap_or_default();
            let (name, expression) = if r.kind == "expression" {
                (format!("{character}/{}", r.name), r.name.clone())
            } else {
                (r.name.clone(), String::new())
            };
            OrderRow {
                kind: kind_label(r.kind),
                name,
                character,
                expression,
                uses: r.uses.len(),
                first_scene: r.scenes.first().cloned().unwrap_or_default(),
                scenes: r.scenes.join(" "),
                note: notes.join("、"),
            }
        })
        .collect()
}

/// 発注リストの CSV（台本と同じく、UTF-8 の BOM 付き、CRLF、RFC 4180）。
pub fn orders_csv(rows: &[OrderRow]) -> String {
    let rows: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            vec![
                r.kind.to_owned(),
                r.name.clone(),
                r.character.clone(),
                r.expression.clone(),
                r.uses.to_string(),
                r.first_scene.clone(),
                r.scenes.clone(),
                r.note.clone(),
            ]
        })
        .collect();
    csv_table(&ORDER_COLUMNS, &rows)
}

/// 発注リストを作品フォルダの既定の置き場所（`export/orders.csv`）に書く。
pub fn write_orders(root: &Path, rows: &[OrderRow]) -> Result<PathBuf, ProjectError> {
    let path = root.join(ORDERS_FILE);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|source| ProjectError::Write {
            path: dir.to_owned(),
            source,
        })?;
    }
    write_source(root, ORDERS_FILE, &orders_csv(rows))?;
    Ok(path)
}

// ---- カタログの書き換え ----

/// カタログを書き換えられなかった理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogEditError {
    /// カタログを YAML として読めない。
    Unreadable(String),
    /// この書き方は書き換えられない（テキストで直す）。
    Unsupported(String),
}

impl CatalogEditError {
    pub fn message(&self) -> String {
        match self {
            CatalogEditError::Unreadable(e) => {
                format!("{CATALOG_FILE} を YAML として読めません（{e}）。テキストで直してください")
            }
            CatalogEditError::Unsupported(m) => m.clone(),
        }
    }
}

/// 素材の種類から、カタログの中の場所（節、キャラクター、`expressions`）。
fn section_path<'a>(
    kind: &str,
    character: Option<&'a str>,
) -> Result<Vec<&'a str>, CatalogEditError> {
    match (kind, character) {
        ("background", _) => Ok(vec!["backgrounds"]),
        ("bgm", _) => Ok(vec!["bgm"]),
        ("se", _) => Ok(vec!["se"]),
        ("expression", Some(id)) => Ok(vec!["characters", id, "expressions"]),
        _ => Err(CatalogEditError::Unsupported(format!(
            "知らない素材の種類です：{kind}"
        ))),
    }
}

/// テキストの1行（改行を含まない範囲）。
#[derive(Debug, Clone, Copy)]
struct Line<'a> {
    start: usize,
    end: usize,
    text: &'a str,
}

impl Line<'_> {
    fn indent(&self) -> usize {
        self.text.len() - self.text.trim_start_matches(' ').len()
    }

    /// 中身のある行か（空行とコメントの行でない）。
    fn content(&self) -> bool {
        let t = self.text.trim();
        !t.is_empty() && !t.starts_with('#')
    }
}

fn split_lines(text: &str) -> Vec<Line<'_>> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if c == '\n' {
            let raw = text.get(start..i).unwrap_or_default();
            let line = raw.strip_suffix('\r').unwrap_or(raw);
            lines.push(Line {
                start,
                end: start + line.len(),
                text: line,
            });
            start = i + 1;
        }
    }
    if start < text.len() {
        let line = text.get(start..).unwrap_or_default();
        lines.push(Line {
            start,
            end: text.len(),
            text: line,
        });
    }
    lines
}

/// `キー: 値` の1行を読む（キーと値）。読めなければ `None`。
fn key_value(line: &str) -> Option<(String, Yaml)> {
    match yaml_serde::from_str::<Yaml>(line.trim()) {
        Ok(Yaml::Mapping(map)) if map.len() == 1 => {
            let (k, v) = map.into_iter().next()?;
            Some((yaml_key(&k)?, v))
        }
        _ => None,
    }
}

/// 字下げで区切った、ある行の下のまとまり（`from..to` の行）。
#[derive(Debug, Clone, Copy)]
struct Region {
    /// 親の行（無ければファイル全体）。
    parent: Option<usize>,
    from: usize,
    to: usize,
}

/// まとまりの中の子（いちばん浅い字下げの、中身のある行）。キーと行の番号。
fn children(lines: &[Line<'_>], region: Region) -> (Option<usize>, Vec<(String, usize)>) {
    let range = lines.get(region.from..region.to).unwrap_or_default();
    let indent = range.iter().filter(|l| l.content()).map(Line::indent).min();
    let mut out = Vec::new();
    if let Some(indent) = indent {
        for (offset, line) in range.iter().enumerate() {
            if line.content()
                && line.indent() == indent
                && let Some((key, _)) = key_value(line.text)
            {
                out.push((key, region.from + offset));
            }
        }
    }
    (indent, out)
}

/// ある行の下のまとまり。
fn body(lines: &[Line<'_>], at: usize) -> Region {
    let indent = lines.get(at).map_or(0, Line::indent);
    let mut to = at + 1;
    while let Some(line) = lines.get(to) {
        if line.content() && line.indent() <= indent {
            break;
        }
        to += 1;
    }
    Region {
        parent: Some(at),
        from: at + 1,
        to,
    }
}

/// `"…"` で囲んだ YAML の文字列。JSON の書き方を元に、YAML が改行と読む文字（U+0085・U+2028・U+2029）と、
/// 印字できない文字（U+007F〜U+009F、U+FFFE・U+FFFF）を `\uXXXX` にする（JSON はそのまま書くため）。
fn yaml_quoted(text: &str) -> String {
    let json = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned());
    let mut out = String::with_capacity(json.len());
    for c in json.chars() {
        if matches!(
            c,
            '\u{7f}'..='\u{9f}' | '\u{2028}' | '\u{2029}' | '\u{fffe}' | '\u{ffff}'
        ) {
            out.push_str(&format!("\\u{:04x}", u32::from(c)));
        } else {
            out.push(c);
        }
    }
    out
}

/// キーの書き方（そのままで読めなければ、`"…"` で囲む）。
fn yaml_key_text(key: &str) -> String {
    let plain = !key.is_empty()
        && !key.contains(['#', '\n', '\r', '\t'])
        && key.trim() == key
        && matches!(
            yaml_serde::from_str::<Yaml>(&format!("{key}: {{}}")),
            Ok(Yaml::Mapping(ref map)) if map.len() == 1
                && map.keys().next() == Some(&Yaml::String(key.to_owned()))
        );
    if plain {
        key.to_owned()
    } else {
        yaml_quoted(key)
    }
}

/// 改行の書き方（ファイルの最初の改行にそろえる）。
fn newline_of(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

/// 字下げの1段（ファイルのいちばん浅い字下げ。無ければ2）。
fn indent_unit(lines: &[Line<'_>]) -> usize {
    lines
        .iter()
        .filter(|l| l.content())
        .map(Line::indent)
        .filter(|&i| i > 0)
        .min()
        .unwrap_or(2)
}

/// YAML の値の中の、`path` の対応表（無ければ作る）。対応表でない値があれば `None`。
fn mapping_at<'v>(root: &'v mut Yaml, path: &[&str]) -> Option<&'v mut Mapping> {
    if root.is_null() {
        *root = Yaml::Mapping(Mapping::new());
    }
    let mut current = root;
    for key in path {
        let map = current.as_mapping_mut()?;
        let entry = map
            .entry(Yaml::String((*key).to_owned()))
            .or_insert(Yaml::Null);
        if entry.is_null() {
            *entry = Yaml::Mapping(Mapping::new());
        }
        current = entry;
    }
    current.as_mapping_mut()
}

fn parse(text: &str) -> Result<Yaml, CatalogEditError> {
    if text.trim().is_empty() {
        return Ok(Yaml::Null);
    }
    yaml_serde::from_str::<Yaml>(text).map_err(|e| CatalogEditError::Unreadable(e.to_string()))
}

/// 書き換えた後のテキストを読み直し、意図した値と同じか確かめる。
fn verify(after: String, expected: &Yaml) -> Result<String, CatalogEditError> {
    match parse(&after) {
        Ok(value) if value == *expected => Ok(after),
        _ => Err(CatalogEditError::Unsupported(format!(
            "{CATALOG_FILE} のこの書き方には、画面から書き足せません。テキストで直してください"
        ))),
    }
}

/// カタログに意味名を足す（`意味名: {}`。仮素材）。ほかの行は変えない：節（とキャラクター、`expressions`）の
/// 最後の項目の次の行に1行を足す。節が無ければ、ファイル（または親）の終わりに作る。すでにあれば、そのまま返す。
pub fn add_entry(
    text: &str,
    kind: &str,
    character: Option<&str>,
    name: &str,
) -> Result<String, CatalogEditError> {
    // 先頭の BOM は除いて書き換え、戻す（T2-10）。
    if let Some(rest) = text.strip_prefix('\u{feff}') {
        return add_entry(rest, kind, character, name).map(|next| format!("\u{feff}{next}"));
    }
    let path = section_path(kind, character)?;
    let before = parse(text)?;
    if !before.is_null() && !before.is_mapping() {
        return Err(CatalogEditError::Unsupported(format!(
            "{CATALOG_FILE} が対応表ではありません。テキストで直してください"
        )));
    }
    let mut expected = before.clone();
    let Some(map) = mapping_at(&mut expected, &path) else {
        return Err(CatalogEditError::Unsupported(format!(
            "{CATALOG_FILE} の {} が対応表ではありません。テキストで直してください",
            path.join(".")
        )));
    };
    let key = Yaml::String(name.to_owned());
    if map.contains_key(&key) {
        return Ok(text.to_owned());
    }
    map.insert(key, Yaml::Mapping(Mapping::new()));

    let lines = split_lines(text);
    let unit = indent_unit(&lines);
    let mut region = Region {
        parent: None,
        from: 0,
        to: lines.len(),
    };
    // 1行の `{}` で書かれた節（`backgrounds: {}`）は、`{}` を消して、下に項目を書く。
    let mut emptied: Option<usize> = None;
    let mut depth = 0;
    for key in &path {
        let (_, kids) = children(&lines, region);
        let Some(&(_, at)) = kids.iter().find(|(k, _)| k == key) else {
            break;
        };
        let value = lines
            .get(at)
            .and_then(|l| key_value(l.text))
            .map(|(_, v)| v);
        match value {
            Some(Yaml::Null) => {}
            Some(Yaml::Mapping(m)) if m.is_empty() && emptied.is_none() => emptied = Some(at),
            _ => {
                return Err(CatalogEditError::Unsupported(format!(
                    "{CATALOG_FILE} の {key} が1行の書き方なので、画面から書き足せません。テキストで直してください"
                )));
            }
        }
        region = body(&lines, at);
        depth += 1;
    }
    let (child_indent, _) = children(&lines, region);
    let parent_indent = region.parent.and_then(|p| lines.get(p)).map(Line::indent);
    let mut indent = child_indent.unwrap_or_else(|| parent_indent.map_or(0, |i| i + unit));
    let newline = newline_of(text);
    let mut added = String::new();
    for key in path.iter().skip(depth) {
        added.push_str(&" ".repeat(indent));
        added.push_str(&yaml_key_text(key));
        added.push(':');
        added.push_str(newline);
        indent += unit;
    }
    added.push_str(&" ".repeat(indent));
    added.push_str(&yaml_key_text(name));
    added.push_str(": {}");
    added.push_str(newline);

    // 足す位置：まとまりの最後の中身のある行の次（無ければ親の行の次、ファイルの終わり）。
    let last = (region.from..region.to)
        .rev()
        .find(|&i| lines.get(i).is_some_and(Line::content))
        .or(region.parent);
    let mut out = String::new();
    match last.and_then(|i| lines.get(i)) {
        Some(line) => {
            let after_line = text.get(line.end..).map_or(0, |rest| {
                if rest.starts_with("\r\n") {
                    2
                } else {
                    usize::from(rest.starts_with('\n'))
                }
            });
            let cut = line.end + after_line;
            let head = text.get(..cut).unwrap_or_default();
            out.push_str(head);
            if after_line == 0 {
                out.push_str(newline);
            }
            out.push_str(&added);
            out.push_str(text.get(cut..).unwrap_or_default());
        }
        None => {
            out.push_str(text);
            if !text.is_empty() && !text.ends_with('\n') {
                out.push_str(newline);
            }
            out.push_str(&added);
        }
    }
    if let Some(at) = emptied {
        out = remove_empty_flow(&out, at)?;
    }
    verify(out, &expected)
}

/// `at` 行目の `キー: {}` の `{}` を消す（行末のコメントは残す）。
fn remove_empty_flow(text: &str, at: usize) -> Result<String, CatalogEditError> {
    let lines = split_lines(text);
    let Some(line) = lines.get(at) else {
        return Ok(text.to_owned());
    };
    let Some((colon, value_end)) = value_range(line.text) else {
        return Err(CatalogEditError::Unsupported(format!(
            "{CATALOG_FILE} の書き方を読めません。テキストで直してください"
        )));
    };
    let head = line.text.get(..colon + 1).unwrap_or_default();
    let tail = line.text.get(value_end..).unwrap_or_default();
    let mut out = String::new();
    out.push_str(text.get(..line.start).unwrap_or_default());
    out.push_str(head);
    out.push_str(tail);
    out.push_str(text.get(line.end..).unwrap_or_default());
    Ok(out)
}

/// `キー: 値  # コメント` の、キーの終わりの `:` の位置と、値の終わり（コメントの前の空白の始まり）。
/// 引用符と括弧の中の `:`・`#` は数えない。
fn value_range(line: &str) -> Option<(usize, usize)> {
    let mut quote: Option<char> = None;
    let mut depth = 0usize;
    let mut colon = None;
    let mut prev = ' ';
    // 直前の空白でない文字。引用符が値やキーの始まりになるのは、行頭（字下げだけの後）か、`:`・`{`・`,`・`[`
    // の後だけ（`a: x 'y.png` の `'` は文字）。
    let mut solid: Option<char> = None;
    let mut chars = line.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match quote {
            Some('"') if c == '\\' => {
                chars.next();
            }
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                '"' | '\'' if solid.is_none_or(|s| matches!(s, ':' | '{' | ',' | '[')) => {
                    quote = Some(c)
                }
                '{' | '[' => depth += 1,
                '}' | ']' => depth = depth.saturating_sub(1),
                ':' if colon.is_none()
                    && depth == 0
                    && chars.peek().is_none_or(|(_, n)| *n == ' ' || *n == '\t') =>
                {
                    colon = Some(i);
                }
                '#' if colon.is_some() && depth == 0 && (prev == ' ' || prev == '\t') => {
                    let before = line.get(..i).unwrap_or_default();
                    let end = before.trim_end_matches([' ', '\t']).len();
                    return colon.map(|c| (c, end));
                }
                _ => {}
            },
        }
        prev = c;
        if c != ' ' && c != '\t' {
            solid = Some(c);
        }
    }
    colon.map(|c| (c, line.trim_end_matches([' ', '\t']).len()))
}

/// 値の書き方（そのままで読めなければ、`"…"` で囲む）。
fn yaml_value_text(value: &Yaml) -> Option<String> {
    match value {
        Yaml::String(s) => {
            let plain = !s.is_empty()
                && s.trim() == s
                && !s.contains(['#', '\n', '\r', '\t', '{', '}', '[', ']', ','])
                && matches!(
                    yaml_serde::from_str::<Yaml>(&format!("x: {s}")),
                    Ok(ref v) if v.get("x") == Some(&Yaml::String(s.clone()))
                );
            Some(if plain { s.clone() } else { yaml_quoted(s) })
        }
        Yaml::Bool(b) => Some(b.to_string()),
        Yaml::Number(n) => Some(n.to_string()),
        Yaml::Null => Some("null".to_owned()),
        _ => None,
    }
}

/// カタログの意味名に、ファイル（`assets/` から数えたパス）を割り当てる。背景・BGM・SE は
/// `{ file: … }`（`loop` などほかのキーは残す）。立ち絵の差分は、キャラクターに `dir` があれば `dir` から
/// 数えたファイル名（`dir` の外のファイルは割り当てられない）、無ければ `assets/` から数えたパス。
/// 意味名の行の値だけを差し替え、ほかの行は変えない。
pub fn set_file(
    text: &str,
    kind: &str,
    character: Option<&str>,
    name: &str,
    file: &str,
) -> Result<String, CatalogEditError> {
    // 先頭の BOM は除いて書き換え、戻す（T2-10）。
    if let Some(rest) = text.strip_prefix('\u{feff}') {
        return set_file(rest, kind, character, name, file).map(|next| format!("\u{feff}{next}"));
    }
    let path = section_path(kind, character)?;
    let before = parse(text)?;
    let not_found = || {
        CatalogEditError::Unsupported(format!(
            "{CATALOG_FILE} に {name} がありません。先にカタログに足してください"
        ))
    };
    // 立ち絵の差分は `dir` から数える。
    let written = if kind == "expression" {
        let dir = character
            .and_then(|id| before.get("characters").and_then(|c| c.get(id)))
            .and_then(|c| yaml_str(c, "dir"))
            .unwrap_or_default();
        let prefix = with_slash(dir);
        match file.strip_prefix(prefix.as_str()) {
            Some(rest) if !rest.is_empty() => rest.to_owned(),
            _ => {
                return Err(CatalogEditError::Unsupported(format!(
                    "この差分のファイルは、キャラクターの dir（{dir}）の中から選んでください"
                )));
            }
        }
    } else {
        file.to_owned()
    };

    let lines = split_lines(text);
    let mut region = Region {
        parent: None,
        from: 0,
        to: lines.len(),
    };
    for key in &path {
        let (_, kids) = children(&lines, region);
        let &(_, at) = kids.iter().find(|(k, _)| k == key).ok_or_else(not_found)?;
        region = body(&lines, at);
    }
    let (_, kids) = children(&lines, region);
    let &(_, at) = kids.iter().find(|(k, _)| k == name).ok_or_else(not_found)?;
    let line = lines.get(at).ok_or_else(not_found)?;
    let (_, current) = key_value(line.text).ok_or_else(not_found)?;

    // 新しい値。
    let new_value = match (&current, kind) {
        (Yaml::Null | Yaml::String(_), "expression") => Yaml::String(written.clone()),
        (Yaml::Mapping(m), "expression") if m.is_empty() => Yaml::String(written.clone()),
        (Yaml::Null, _) => {
            let mut m = Mapping::new();
            m.insert(Yaml::String("file".into()), Yaml::String(written.clone()));
            Yaml::Mapping(m)
        }
        (Yaml::Mapping(m), _) => {
            let mut out = Mapping::new();
            let file_key = Yaml::String("file".into());
            if !m.contains_key(&file_key) {
                out.insert(file_key.clone(), Yaml::String(written.clone()));
            }
            for (k, v) in m {
                if *k == file_key {
                    out.insert(k.clone(), Yaml::String(written.clone()));
                } else {
                    out.insert(k.clone(), v.clone());
                }
            }
            Yaml::Mapping(out)
        }
        _ => {
            return Err(CatalogEditError::Unsupported(format!(
                "{CATALOG_FILE} の {name} の書き方には、画面からファイルを割り当てられません。テキストで直してください"
            )));
        }
    };
    let value_text = match &new_value {
        Yaml::Mapping(m) => {
            let mut parts = Vec::new();
            for (k, v) in m {
                let k = yaml_key(k).ok_or_else(not_found)?;
                let v = yaml_value_text(v).ok_or_else(|| {
                    CatalogEditError::Unsupported(format!(
                        "{CATALOG_FILE} の {name} の書き方には、画面からファイルを割り当てられません。テキストで直してください"
                    ))
                })?;
                parts.push(format!("{}: {v}", yaml_key_text(&k)));
            }
            format!("{{ {} }}", parts.join(", "))
        }
        v => yaml_value_text(v).ok_or_else(not_found)?,
    };

    // 値の部分だけを差し替える（キーの書き方と行末のコメントは残す）。
    let (colon, value_end) = value_range(line.text).ok_or_else(not_found)?;
    let mut out = String::new();
    out.push_str(text.get(..line.start).unwrap_or_default());
    out.push_str(line.text.get(..colon + 1).unwrap_or_default());
    out.push(' ');
    out.push_str(&value_text);
    out.push_str(line.text.get(value_end..).unwrap_or_default());
    out.push_str(text.get(line.end..).unwrap_or_default());

    let mut expected = before.clone();
    let parent = mapping_at(&mut expected, &path).ok_or_else(not_found)?;
    parent.insert(Yaml::String(name.to_owned()), new_value);
    verify(out, &expected)
}
