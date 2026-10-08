//! 設定DBと変数の画面（アプリ、T1-10、D049）：項目の型、値の検査（W805・W806）、フロントマターの
//! 書き換え、エントリの逆引き、変数の一覧。
//!
//! フロントマターの書き換えは、直したキーの値だけをテキストで差し替え、ほかの行（コメント、キーの順、
//! 空行、引用の書き方、flow 形式）と本文は1バイトも変えない。書き換えた後に読み直し、直したキー以外の
//! 項目と本文が変わっていないことを確かめ、違っていれば書かない（ソース表示で直してもらう）。
//! YAMLの書き出し（`yaml_serde`）はコメントを消すので使わない。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::Serialize;
use serde_json::Value;

use crate::diag::Span;
use crate::index::{Index, Location};
use crate::ir::{Ir, IrInline, Op};
use crate::project::{Bible, Entry, FieldDef, Project};

/// 伏線の型のID（`bible/types/foreshadow.yaml`。D043）。
pub const FORESHADOW_TYPE: &str = "foreshadow";

/// 伏線の回収の項目。W703 で調べるので、W805・W806 では調べない（同じ誤りを2回出さない）。
const FORESHADOW_PAYOFF: &str = "payoff";

/// フォームで直せない共通の項目（`id` を変えるとファイル名や参照と食い違う。リネームはフェーズ2）。
pub const READ_ONLY_KEYS: [&str; 1] = ["id"];

// ---- 項目の型 ----

/// 項目の型（`docs/spec/project-files.md` の5節）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FieldType {
    Text,
    Longtext,
    Number,
    Bool,
    /// `YYYY-MM-DD`。
    Date,
    Select,
    Multiselect,
    /// 素材の意味名。
    Asset,
    /// 参照。`target` は型のID か `scene`。
    Ref {
        target: String,
    },
    List {
        item: Box<FieldType>,
    },
}

impl FieldType {
    /// 型の書き方（`text`、`list<ref<scene>>` など）を読む。読めなければ `None`。
    pub fn parse(text: &str) -> Option<FieldType> {
        let text = text.trim();
        if let Some(inner) = text.strip_prefix("list<").and_then(|r| r.strip_suffix('>')) {
            return Some(FieldType::List {
                item: Box::new(Self::parse(inner)?),
            });
        }
        if let Some(inner) = text.strip_prefix("ref<").and_then(|r| r.strip_suffix('>')) {
            let target = inner.trim();
            let bad = |c: char| c.is_whitespace() || matches!(c, '<' | '>' | ',' | '/' | '\\');
            if target.is_empty() || target.contains(bad) {
                return None;
            }
            return Some(FieldType::Ref {
                target: target.to_owned(),
            });
        }
        Some(match text {
            "text" => FieldType::Text,
            "longtext" => FieldType::Longtext,
            "number" => FieldType::Number,
            "bool" => FieldType::Bool,
            "date" => FieldType::Date,
            "select" => FieldType::Select,
            "multiselect" => FieldType::Multiselect,
            "asset" => FieldType::Asset,
            _ => return None,
        })
    }

    /// 参照の先（`ref<…>` と `list<ref<…>>`）。参照でなければ `None`。
    pub fn ref_target(&self) -> Option<&str> {
        match self {
            FieldType::Ref { target } => Some(target),
            FieldType::List { item } => item.ref_target(),
            _ => None,
        }
    }
}

impl fmt::Display for FieldType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FieldType::Text => f.write_str("text"),
            FieldType::Longtext => f.write_str("longtext"),
            FieldType::Number => f.write_str("number"),
            FieldType::Bool => f.write_str("bool"),
            FieldType::Date => f.write_str("date"),
            FieldType::Select => f.write_str("select"),
            FieldType::Multiselect => f.write_str("multiselect"),
            FieldType::Asset => f.write_str("asset"),
            FieldType::Ref { target } => write!(f, "ref<{target}>"),
            FieldType::List { item } => write!(f, "list<{item}>"),
        }
    }
}

// ---- 値の検査（W805・W806） ----

fn is_scalar(value: &Value) -> bool {
    matches!(value, Value::String(_) | Value::Number(_) | Value::Bool(_))
}

/// 値を文字列にする（文字列・数・真偽）。
pub fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// `YYYY-MM-DD` の日付か（月は1〜12、日はその月の日数まで）。
fn is_date(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    let [y, m, d] = parts.as_slice() else {
        return false;
    };
    if y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return false;
    }
    let (Ok(y), Ok(m), Ok(d)) = (y.parse::<u32>(), m.parse::<u32>(), d.parse::<u32>()) else {
        return false;
    };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&d)
}

/// 値が型に合うかを調べる。合わなければ、どう書くかの文を返す。書いていない値（null と空の文字列）は
/// 合うとみなす（壊れたまま書ける。設計の原則4）。参照先があるかは調べない（W806）。
pub fn check_value(ty: &FieldType, options: &[String], value: &Value) -> Option<String> {
    if value.is_null() || value.as_str() == Some("") {
        return None;
    }
    let in_options = |v: &Value| {
        options.is_empty() || scalar_text(v).is_some_and(|text| options.contains(&text))
    };
    let choices = || options.join("・");
    match ty {
        FieldType::Text | FieldType::Longtext | FieldType::Asset => {
            (!is_scalar(value)).then(|| "文字列で書きます".to_owned())
        }
        FieldType::Number => (!value.is_number()).then(|| "数で書きます".to_owned()),
        FieldType::Bool => (!value.is_boolean()).then(|| "true か false で書きます".to_owned()),
        FieldType::Date => (!value.as_str().is_some_and(is_date))
            .then(|| "日付（YYYY-MM-DD）で書きます".to_owned()),
        FieldType::Select => (!is_scalar(value) || !in_options(value))
            .then(|| format!("選択肢（{}）のどれかで書きます", choices())),
        FieldType::Multiselect => match value {
            Value::Array(items) if items.iter().all(|i| is_scalar(i) && in_options(i)) => None,
            _ => Some(format!("選択肢（{}）のリストで書きます", choices())),
        },
        FieldType::Ref { .. } => (!is_scalar(value)).then(|| "参照先のIDで書きます".to_owned()),
        FieldType::List { item } => match value {
            Value::Array(items) => items.iter().enumerate().find_map(|(i, v)| {
                check_value(item, options, v).map(|m| format!("{}番目の値は、{m}", i + 1))
            }),
            _ => Some(format!(
                "リスト（[a, b] の形）で書きます（{item} のリスト）"
            )),
        },
    }
}

/// 検査の対象の項目か。伏線の回収は W703 で、キャラクターの `name`・`aliases` は読み込み（E802）で
/// 調べるので、同じ誤りを2回出さないよう除く。
fn checked_field(bible: &Bible, entry: &Entry, field: &FieldDef) -> bool {
    let foreshadow = entry.type_id == FORESHADOW_TYPE && field.key == FORESHADOW_PAYOFF;
    let character =
        entry.type_id == bible.character_type() && matches!(field.key.as_str(), "name" | "aliases");
    !foreshadow && !character
}

/// 型の分かるエントリの、値が型に合わない項目（W805）。ファイル、位置（キー）、文。
pub(crate) fn value_problems(bible: &Bible) -> Vec<(String, Span, String)> {
    let mut problems = Vec::new();
    for entry in &bible.entries {
        let Some(entry_type) = bible.types.iter().find(|t| t.id == entry.type_id) else {
            continue;
        };
        for field in &entry_type.fields {
            if !checked_field(bible, entry, field) {
                continue;
            }
            let Some(value) = entry.fields.get(&field.key) else {
                continue;
            };
            if let Some(message) = check_value(&field.kind, &field.options, value) {
                problems.push((
                    entry.path.clone(),
                    entry
                        .key_spans
                        .get(&field.key)
                        .copied()
                        .unwrap_or(Span::new(0, 0)),
                    format!(
                        "{} の{}（{}）は、{message}",
                        entry.id, field.label, field.key
                    ),
                ));
            }
        }
    }
    problems
}

/// 参照の値（`ref<…>` は1つ、`list<ref<…>>` はリストの中身）。文字列にできないものは除く（W805）。
fn ref_values(ty: &FieldType, value: &Value) -> Vec<String> {
    match (ty, value) {
        (FieldType::Ref { .. }, v) => scalar_text(v)
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect(),
        (FieldType::List { item }, Value::Array(items)) => {
            items.iter().flat_map(|v| ref_values(item, v)).collect()
        }
        _ => Vec::new(),
    }
}

/// 参照先の無い参照（W806、検証の段階1）。ファイル、位置（キー）、文。
pub(crate) fn broken_references(project: &Project, ir: &Ir) -> Vec<(String, Span, String)> {
    let scenes: BTreeSet<&str> = ir.scenes.iter().map(|s| s.id.as_str()).collect();
    let bible = &project.bible;
    let mut problems = Vec::new();
    for entry in &bible.entries {
        let Some(entry_type) = bible.types.iter().find(|t| t.id == entry.type_id) else {
            continue;
        };
        for field in &entry_type.fields {
            let Some(target) = field.kind.ref_target() else {
                continue;
            };
            if !checked_field(bible, entry, field) {
                continue;
            }
            let Some(value) = entry.fields.get(&field.key) else {
                continue;
            };
            for id in ref_values(&field.kind, value) {
                let exists = if target == "scene" {
                    scenes.contains(id.as_str())
                } else {
                    bible
                        .entries
                        .iter()
                        .any(|e| e.type_id == target && e.id == id)
                };
                if exists {
                    continue;
                }
                let what = if target == "scene" {
                    "シーン".to_owned()
                } else {
                    bible.types.iter().find(|t| t.id == target).map_or_else(
                        || format!("型 {target} の定義もありません"),
                        |t| t.name.clone(),
                    )
                };
                problems.push((
                    entry.path.clone(),
                    entry
                        .key_spans
                        .get(&field.key)
                        .copied()
                        .unwrap_or(Span::new(0, 0)),
                    format!(
                        "{} の{}（{}）の参照先 {id} がありません（{what}）",
                        entry.id, field.label, field.key
                    ),
                ));
            }
        }
    }
    problems
}

// ---- フロントマターの書き換え ----

/// フロントマターを書き換えられない。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    /// 今のフロントマターをYAMLとして読めない。
    #[error("フロントマターをYAMLとして読めません（{0}）。ソース表示で直してください")]
    Unreadable(String),
    /// 直したキー以外を変えずには書き換えられない（入れ子の書き方、同じキーが2つ、アンカーなど）。
    #[error(
        "この項目は、ほかの行を変えずには書き換えられない書き方になっています。ソース表示で直してください"
    )]
    Unsupported,
    /// フォームでは直せない項目（`id`）か、キーの形が不正。
    #[error("{0} はフォームでは直せません")]
    ReadOnly(String),
    /// 値が正しくない（型定義の項目の名前・型など。T2-10）。
    #[error("{0}")]
    Invalid(String),
}

/// フロントマターの位置：中身の始まり、閉じの `---` の行の始まり、閉じの行の後ろ。
struct Front {
    start: usize,
    close: usize,
    end: usize,
}

/// 先頭の `---` の行から、次の `---` の行までを探す（`project::front_matter` と同じ読み方）。
fn find_front(text: &str) -> Option<Front> {
    let bom = if text.starts_with('\u{feff}') { 3 } else { 0 };
    let body = text.get(bom..)?;
    let mut lines = body.split_inclusive('\n');
    let first = lines.next()?;
    if first.trim_end() != "---" {
        return None;
    }
    let start = bom + first.len();
    let mut at = start;
    for line in lines {
        if line.trim_end() == "---" {
            return Some(Front {
                start,
                close: at,
                end: at + line.len(),
            });
        }
        at += line.len();
    }
    None
}

/// フロントマターの後ろの本文（フロントマターが無ければ、BOMを除いた全体）。
pub fn body_of(text: &str) -> &str {
    match find_front(text) {
        Some(front) => text.get(front.end..).unwrap_or_default(),
        None => text.trim_start_matches('\u{feff}'),
    }
}

/// フロントマターの値（キーごと）。フロントマターが無ければ空。読めなければ誤りの文。
pub fn front_fields(text: &str) -> Result<BTreeMap<String, Value>, String> {
    let Some(front) = find_front(text) else {
        return Ok(BTreeMap::new());
    };
    let yaml = text.get(front.start..front.close).unwrap_or_default();
    let value: yaml_serde::Value = yaml_serde::from_str(yaml).map_err(|e| e.to_string())?;
    match value {
        yaml_serde::Value::Null => Ok(BTreeMap::new()),
        yaml_serde::Value::Mapping(map) => {
            let mut fields = BTreeMap::new();
            for (k, v) in map {
                let key = match k {
                    yaml_serde::Value::String(s) => s,
                    yaml_serde::Value::Number(n) => n.to_string(),
                    yaml_serde::Value::Bool(b) => b.to_string(),
                    _ => return Err("キーが文字列ではありません".to_owned()),
                };
                let value = serde_json::to_value(&v).map_err(|e| e.to_string())?;
                if fields.insert(key.clone(), value).is_some() {
                    return Err(format!("キー {key} が2つあります"));
                }
            }
            Ok(fields)
        }
        _ => Err("フロントマターは「キー: 値」の対応表で書きます".to_owned()),
    }
}

/// ファイルの改行（最初の行の終わり）。
pub(crate) fn newline_of(text: &str) -> &'static str {
    match text.find('\n') {
        Some(at) if at > 0 && text.as_bytes().get(at - 1) == Some(&b'\r') => "\r\n",
        _ => "\n",
    }
}

/// YAMLとして読んだ値（`k: …` の形で読む）。
fn parse_yaml_value(line: &str) -> Option<Value> {
    let value: yaml_serde::Value = yaml_serde::from_str(line).ok()?;
    let value = serde_json::to_value(&value).ok()?;
    value.get("k").cloned()
}

/// 素のまま書いても同じ文字列として読める（`yes` や `12`、`: ` を含むものなどは読み違える）。
fn plain_ok(text: &str, in_flow: bool) -> bool {
    if text.is_empty()
        || text != text.trim()
        || text.contains(['\n', '\r', '\t'])
        || text.starts_with(|c: char| "-?:,[]{}#&*!|>'\"%@`".contains(c))
        || (in_flow && text.contains([',', '[', ']', '{', '}']))
    {
        return false;
    }
    let line = if in_flow {
        format!("k: [{text}]")
    } else {
        format!("k: {text}")
    };
    let parsed = parse_yaml_value(&line);
    let expected = Value::String(text.to_owned());
    if in_flow {
        parsed == Some(Value::Array(vec![expected]))
    } else {
        parsed == Some(expected)
    }
}

/// 文字列の1行の書き方：素のままか、JSONの文字列（YAMLの二重引用符の文字列としても読める）。
pub(crate) fn scalar_inline(text: &str, in_flow: bool) -> String {
    if plain_ok(text, in_flow) {
        text.to_owned()
    } else {
        serde_json::to_string(text).unwrap_or_default()
    }
}

/// 1行の文字列のYAMLでの書き方（素のまま読めなければJSONの文字列）。
pub fn yaml_scalar(text: &str) -> String {
    scalar_inline(text, false)
}

/// 値の書き方の候補（先に試すもの順）。`Inline` はキーの後ろ（`key: …`）、`Block` は `|` のブロック。
enum Written {
    Inline(String),
    Block {
        header: &'static str,
        lines: Vec<String>,
    },
}

fn json_inline(value: &Value) -> Written {
    Written::Inline(serde_json::to_string(value).unwrap_or_default())
}

fn candidates(value: &Value) -> Vec<Written> {
    match value {
        Value::Null => vec![Written::Inline(String::new())],
        Value::Bool(_) | Value::Number(_) => vec![Written::Inline(value.to_string())],
        Value::String(s) if s.contains('\n') && !s.contains('\r') => {
            // 終わりの改行の数で、ブロックの終わりの扱い（chomping）を決める。
            let trailing = s.len() - s.trim_end_matches('\n').len();
            let header = match trailing {
                0 => "|-",
                1 => "|",
                _ => "|+",
            };
            let content = s.strip_suffix('\n').unwrap_or(s);
            let lines = content.split('\n').map(str::to_owned).collect();
            vec![Written::Block { header, lines }, json_inline(value)]
        }
        Value::String(s) => vec![Written::Inline(scalar_inline(s, false)), json_inline(value)],
        Value::Array(items) => {
            let parts: Vec<String> = items
                .iter()
                .map(|item| match item {
                    Value::String(s) => scalar_inline(s, true),
                    other => serde_json::to_string(other).unwrap_or_default(),
                })
                .collect();
            vec![
                Written::Inline(format!("[{}]", parts.join(", "))),
                json_inline(value),
            ]
        }
        // `{ chars: 300000, routes: 2 }` の形（値が文字列・数・真偽値だけのとき）。読み直して違えば JSON。
        Value::Object(map) => {
            let parts: Option<Vec<String>> = map
                .iter()
                .map(|(k, v)| {
                    let v = match v {
                        Value::String(s) => scalar_inline(s, true),
                        Value::Number(_) | Value::Bool(_) => v.to_string(),
                        _ => return None,
                    };
                    Some(format!("{}: {v}", key_text(k)))
                })
                .collect();
            match parts {
                Some(parts) if !parts.is_empty() => vec![
                    Written::Inline(format!("{{ {} }}", parts.join(", "))),
                    json_inline(value),
                ],
                _ => vec![json_inline(value)],
            }
        }
    }
}

/// キーの行：行頭に字下げの無い `キー:`（引用符で囲んだキーも）。キーの部分（`:` の手前まで）の長さを返す。
pub(crate) fn key_prefix(line: &str, key: &str) -> Option<usize> {
    for written in [
        key.to_owned(),
        serde_json::to_string(key).unwrap_or_default(),
        format!("'{}'", key.replace('\'', "''")),
    ] {
        if let Some(rest) = line.strip_prefix(written.as_str())
            && let Some(after) = rest.strip_prefix(':')
            && (after.is_empty() || after.starts_with([' ', '\t', '\r', '\n']))
        {
            return Some(written.len());
        }
    }
    None
}

/// 行の中の、行末のコメントの始まり（前の空白を含む）。引用符は、値の始まり（直前の空白でない文字が無い、
/// または `[`・`{`・`,`・`-`・`:`・`?`）でだけ開いたとみなす（`It's`、`5"`、`灯 'x` のような素の文字列の
/// 途中の引用符で、コメントを見落とさないため）。
pub(crate) fn comment_start(value: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    let mut prev: Option<char> = None;
    let mut prev_solid: Option<char> = None;
    let mut chars = value.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let at_start = prev_solid.is_none_or(|p| matches!(p, '[' | '{' | ',' | '-' | ':' | '?'));
        match quote {
            Some('"') if c == '\\' => {
                chars.next();
            }
            Some('\'') if c == '\'' && chars.peek().is_some_and(|(_, n)| *n == '\'') => {
                // 一重引用符の中の `''` は、引用符そのもの。
                chars.next();
            }
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if (c == '"' || c == '\'') && at_start => quote = Some(c),
            None if c == '#' && prev.is_none_or(|p| p == ' ' || p == '\t') => {
                let before = value.get(..i).unwrap_or_default();
                return Some(before.trim_end_matches([' ', '\t']).len());
            }
            None => {}
        }
        prev = Some(c);
        if c != ' ' && c != '\t' {
            prev_solid = Some(c);
        }
    }
    None
}

/// キーの値の範囲（キーの行の始まりから、続きの行の終わりまで）と、行末のコメント（前の空白を含む）。
struct KeyLines {
    start: usize,
    end: usize,
    /// キーの部分（`:` の手前まで）。書いてあるとおりに残す。
    key_text: String,
    comment: String,
}

/// フロントマターの中で、`key` の行を探す。同じキーが2つあれば `Err`。
fn find_key(text: &str, front: &Front, key: &str) -> Result<Option<KeyLines>, EditError> {
    let region = text.get(front.start..front.close).unwrap_or_default();
    let lines: Vec<(usize, &str)> = region
        .split_inclusive('\n')
        .scan(front.start, |at, line| {
            let start = *at;
            *at += line.len();
            Some((start, line))
        })
        .collect();
    let mut found: Option<KeyLines> = None;
    for (i, (start, line)) in lines.iter().enumerate() {
        let Some(key_len) = key_prefix(line, key) else {
            continue;
        };
        if found.is_some() {
            return Err(EditError::Unsupported);
        }
        let content = line.trim_end_matches(['\n', '\r']);
        let after_colon = content.get(key_len + 1..).unwrap_or_default();
        let comment_at = comment_start(after_colon);
        let comment = comment_at
            .and_then(|at| after_colon.get(at..))
            .unwrap_or_default()
            .to_owned();
        let inline = after_colon
            .get(..comment_at.unwrap_or(after_colon.len()))
            .unwrap_or_default()
            .trim();
        // `|`・`>` のブロックなら、続きの行はすべて値（`#` で始まる行も中身）。
        let block = inline.starts_with(['|', '>']);
        // 続きの行：字下げした行と、`- ` で始まる行（キーと同じ深さのリスト）。間の空行とコメントの行も
        // 含め、終わりの空行とコメントの行は含めない（後ろのコメントは残す）。
        let mut end = start + line.len();
        let mut inner_comment = false;
        let mut pending_comment = false;
        for (next_start, next) in lines.iter().skip(i + 1) {
            let trimmed = next.trim();
            let blank = trimmed.is_empty();
            let indented = next.starts_with([' ', '\t']);
            let continues = indented || next.starts_with("- ") || next.trim_end() == "-";
            if !continues && !blank {
                break;
            }
            if blank {
                continue;
            }
            if !block && trimmed.starts_with('#') {
                // 字下げしたコメントの行。後ろに値の行が続けば、値の途中のコメント。
                pending_comment = true;
                continue;
            }
            if !block {
                let body = next.trim_end_matches(['\n', '\r']);
                inner_comment |= pending_comment || comment_start(body).is_some();
            }
            pending_comment = false;
            end = next_start + next.len();
        }
        // 値の途中のコメント（リストの項目の行末のコメントなど）は、差し替えると消えるので書かない。
        if inner_comment {
            return Err(EditError::Unsupported);
        }
        found = Some(KeyLines {
            start: *start,
            end,
            key_text: line.get(..key_len).unwrap_or_default().to_owned(),
            comment,
        });
    }
    Ok(found)
}

/// キーの書き方（素のままで読めなければJSONの文字列）。
fn key_text(key: &str) -> String {
    if plain_ok(key, true) && !key.contains(':') {
        key.to_owned()
    } else {
        serde_json::to_string(key).unwrap_or_default()
    }
}

/// キーと値の行（改行まで）。
fn key_value_lines(key_text: &str, written: &Written, comment: &str, nl: &str) -> String {
    match written {
        Written::Inline(v) if v.is_empty() => format!("{key_text}:{comment}{nl}"),
        Written::Inline(v) => format!("{key_text}: {v}{comment}{nl}"),
        Written::Block { header, lines } => {
            let mut out = format!("{key_text}: {header}{comment}{nl}");
            for line in lines {
                if line.is_empty() {
                    out.push_str(nl);
                } else {
                    out.push_str("  ");
                    out.push_str(line);
                    out.push_str(nl);
                }
            }
            out
        }
    }
}

/// フロントマターの `key` の値を `value` にする。ほかの行と本文は変えない。キーが無ければ、閉じの `---` の
/// 直前に足す（フロントマターが無ければ作る）。`null` は値の無いキー（`key:`）にする。
///
/// 書き換えた後に読み直し、`key` 以外の項目と本文が同じで、`key` の値が `value` になっていることを確かめる。
pub fn set_field(text: &str, key: &str, value: &Value) -> Result<String, EditError> {
    if key.is_empty() || key.contains(['\n', '\r']) || READ_ONLY_KEYS.contains(&key) {
        return Err(EditError::ReadOnly(key.to_owned()));
    }
    let before = front_fields(text).map_err(EditError::Unreadable)?;
    let mut expected = before;
    expected.insert(key.to_owned(), value.clone());
    let body = body_of(text);
    let nl = newline_of(text);
    for written in candidates(value) {
        let next = match find_front(text) {
            None => {
                let bom = if text.starts_with('\u{feff}') {
                    "\u{feff}"
                } else {
                    ""
                };
                format!(
                    "{bom}---{nl}{}---{nl}{}",
                    key_value_lines(&key_text(key), &written, "", nl),
                    text.get(bom.len()..).unwrap_or_default()
                )
            }
            Some(front) => match find_key(text, &front, key)? {
                Some(lines) => format!(
                    "{}{}{}",
                    text.get(..lines.start).unwrap_or_default(),
                    key_value_lines(&lines.key_text, &written, &lines.comment, nl),
                    text.get(lines.end..).unwrap_or_default()
                ),
                None => format!(
                    "{}{}{}",
                    text.get(..front.close).unwrap_or_default(),
                    key_value_lines(&key_text(key), &written, "", nl),
                    text.get(front.close..).unwrap_or_default()
                ),
            },
        };
        if body_of(&next) == body && front_fields(&next).ok().as_ref() == Some(&expected) {
            return Ok(next);
        }
    }
    Err(EditError::Unsupported)
}

/// 本文（フロントマターの後ろ）を `body` にする。フロントマターは1バイトも変えない。`body` の改行は、
/// ファイルの改行（`\r\n` か `\n`）にそろえる（画面の入力欄は `\n` で返すため）。
pub fn set_body(text: &str, body: &str) -> String {
    let nl = newline_of(text);
    let body = if nl == "\r\n" {
        body.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        body.to_owned()
    };
    match find_front(text) {
        Some(front) => format!("{}{body}", text.get(..front.end).unwrap_or_default()),
        None => {
            let bom = if text.starts_with('\u{feff}') {
                "\u{feff}"
            } else {
                ""
            };
            format!("{bom}{body}")
        }
    }
}

// ---- 逆引き ----

/// エントリが現れるシーン。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SceneUse {
    pub scene: String,
    pub title: String,
    pub file: String,
    /// そのシーンで最初に現れる行。
    pub span: Span,
    /// 行の数（キャラクターは台詞の数。台詞が無ければ立ち絵と退場の数）。
    pub count: usize,
}

/// 項目の参照（エントリからエントリかシーンへ）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntryRef {
    /// 参照を書いた項目のキーと名前。
    pub field: String,
    pub label: String,
    /// 相手：`entry` か `scene`。
    pub kind: &'static str,
    /// 相手のエントリの型（シーンなら `None`）。
    pub type_id: Option<String>,
    pub id: String,
    /// 相手の名前（エントリの `name`、シーンの題名）。無ければ `None`。
    pub name: Option<String>,
    /// 相手のエントリのパス（シーンならファイル）。無ければ `None`。
    pub path: Option<String>,
    pub exists: bool,
}

/// 変数の一覧の1行（変数の画面と、キャラクターの「紐づく変数」）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VariableRow {
    /// そろえた変数名（`akari.好感度`）。
    pub key: String,
    pub declared: bool,
    #[serde(rename = "type")]
    pub ty: Option<&'static str>,
    pub values: Vec<String>,
    pub default: Option<String>,
    pub range: Option<(i64, i64)>,
    pub scope: Option<String>,
    pub desc: Option<String>,
    /// `variables.yaml` の中の宣言の位置。
    pub span: Option<Span>,
    pub reads: Vec<Location>,
    pub writes: Vec<Location>,
}

/// エントリの逆引き（設定DBの画面の右の欄）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EntryUsage {
    /// キャラクターのエントリか（登場は話者・立ち絵・退場で数える）。
    pub character: bool,
    /// 現れるシーン（キャラクターは登場、そのほかは名前か呼び名が台詞・地の文・選択肢に出る行）。IRの順。
    pub scenes: Vec<SceneUse>,
    /// 行の数（キャラクターは台詞の数、そのほかは名前の出る行の数）。
    pub lines: usize,
    /// 初出のシーン（IRの順。ファイルのパスの順と、ファイルの中の順）。
    pub first: Option<String>,
    /// このエントリが参照している先。
    pub refs_out: Vec<EntryRef>,
    /// このエントリを参照している元。
    pub refs_in: Vec<EntryRef>,
    /// 紐づく変数（キャラクターIDを名前空間にした変数）。
    pub variables: Vec<VariableRow>,
}

pub(crate) fn plain_text(text: &[IrInline]) -> String {
    text.iter()
        .filter_map(|inline| match inline {
            IrInline::Text { text } | IrInline::Emphasis { text } => Some(text.as_str()),
            IrInline::Ruby { base, .. } => Some(base.as_str()),
            _ => None,
        })
        .collect()
}

/// 本文の行（台詞、地の文、選択肢の問いかけ文と表示文）の位置と文字。選択肢と条件のブロックの中も含む。
fn text_lines(ops: &[Op], out: &mut Vec<(Span, String)>) {
    for op in ops {
        match op {
            Op::Say { span, text, .. } | Op::Narrate { span, text, .. } => {
                out.push((*span, plain_text(text)));
            }
            Op::Choice {
                span,
                prompt,
                options,
                ..
            } => {
                if let Some(prompt) = prompt {
                    out.push((*span, plain_text(prompt)));
                }
                for option in options {
                    out.push((option.span, plain_text(&option.text)));
                    text_lines(&option.body, out);
                }
            }
            Op::If {
                branches,
                else_body,
                ..
            } => {
                for branch in branches {
                    text_lines(&branch.body, out);
                }
                if let Some(body) = else_body {
                    text_lines(body, out);
                }
            }
            _ => {}
        }
    }
}

/// エントリの名前（`name`）。
fn entry_name(entry: &Entry) -> Option<String> {
    entry.fields.get("name").and_then(scalar_text)
}

/// 変数の一覧：宣言された変数と、宣言されずに使われた変数（索引の順＝名前の順）。
pub fn variable_rows(project: &Project, index: &Index) -> Vec<VariableRow> {
    index
        .variables
        .iter()
        .map(|v| {
            let decl = project.variables.iter().find(|d| d.key == v.key);
            VariableRow {
                key: v.key.clone(),
                declared: v.declared,
                ty: v.ty,
                values: v.values.clone(),
                default: decl.and_then(|d| d.default.clone()),
                range: decl.and_then(|d| d.range),
                scope: decl.and_then(|d| d.scope.clone()),
                desc: decl.and_then(|d| d.desc.clone()),
                span: decl.map(|d| d.span),
                reads: v.reads.clone(),
                writes: v.writes.clone(),
            }
        })
        .collect()
}

/// エントリ（パスで指す）の逆引き。無いエントリなら `None`。
pub fn entry_usage(project: &Project, ir: &Ir, index: &Index, path: &str) -> Option<EntryUsage> {
    let bible = &project.bible;
    let entry = bible.entries.iter().find(|e| e.path == path)?;
    let character = entry.type_id == bible.character_type()
        && project.characters.iter().any(|c| c.id == entry.id);
    let titles: BTreeMap<&str, (&str, &str)> = ir
        .scenes
        .iter()
        .map(|s| (s.id.as_str(), (s.title.as_str(), s.file.as_str())))
        .collect();
    let mut scenes: Vec<SceneUse> = Vec::new();
    let mut add = |scene: &str, span: Span, weight: usize| {
        if let Some(last) = scenes.iter_mut().find(|s| s.scene == scene) {
            last.count += weight;
            return;
        }
        let (title, file) = titles.get(scene).copied().unwrap_or(("", ""));
        scenes.push(SceneUse {
            scene: scene.to_owned(),
            title: title.to_owned(),
            file: file.to_owned(),
            span,
            count: weight,
        });
    };
    let mut lines = 0;
    if character {
        if let Some(c) = index.characters.iter().find(|c| c.id == entry.id) {
            for u in &c.uses {
                let speaker = u.kind == "speaker";
                lines += usize::from(speaker);
                add(&u.location.scene, u.location.span, usize::from(speaker));
            }
        }
    } else {
        let mut needles: Vec<String> = entry_name(entry).into_iter().collect();
        if let Some(Value::Array(aliases)) = entry.fields.get("aliases") {
            needles.extend(aliases.iter().filter_map(scalar_text));
        }
        needles.retain(|n| !n.trim().is_empty());
        if !needles.is_empty() {
            for scene in &ir.scenes {
                let mut found = Vec::new();
                text_lines(&scene.ops, &mut found);
                for (span, text) in found {
                    if needles.iter().any(|n| text.contains(n.as_str())) {
                        lines += 1;
                        add(&scene.id, span, 1);
                    }
                }
            }
        }
    }
    let first = scenes.first().map(|s| s.scene.clone());

    // 参照（項目の型が ref のもの）。
    let scene_ref = |field: &FieldDef, id: &str| {
        let scene = ir.scenes.iter().find(|s| s.id == id);
        EntryRef {
            field: field.key.clone(),
            label: field.label.clone(),
            kind: "scene",
            type_id: None,
            id: id.to_owned(),
            name: scene.map(|s| s.title.clone()),
            path: scene.map(|s| s.file.clone()),
            exists: scene.is_some(),
        }
    };
    let entry_ref = |field: &FieldDef, target: &str, id: &str| {
        let found = bible
            .entries
            .iter()
            .find(|e| e.type_id == target && e.id == id);
        EntryRef {
            field: field.key.clone(),
            label: field.label.clone(),
            kind: "entry",
            type_id: Some(target.to_owned()),
            id: id.to_owned(),
            name: found.and_then(entry_name),
            path: found.map(|e| e.path.clone()),
            exists: found.is_some(),
        }
    };
    let fields_of = |e: &Entry| -> Vec<FieldDef> {
        bible
            .types
            .iter()
            .find(|t| t.id == e.type_id)
            .map(|t| t.fields.clone())
            .unwrap_or_default()
    };
    let mut refs_out = Vec::new();
    for field in fields_of(entry) {
        let (Some(target), Some(value)) = (field.kind.ref_target(), entry.fields.get(&field.key))
        else {
            continue;
        };
        for id in ref_values(&field.kind, value) {
            refs_out.push(if target == "scene" {
                scene_ref(&field, &id)
            } else {
                entry_ref(&field, target, &id)
            });
        }
    }
    let mut refs_in = Vec::new();
    for other in &bible.entries {
        for field in fields_of(other) {
            let (Some(target), Some(value)) =
                (field.kind.ref_target(), other.fields.get(&field.key))
            else {
                continue;
            };
            if target != entry.type_id || !ref_values(&field.kind, value).contains(&entry.id) {
                continue;
            }
            refs_in.push(EntryRef {
                field: field.key.clone(),
                label: field.label.clone(),
                kind: "entry",
                type_id: Some(other.type_id.clone()),
                id: other.id.clone(),
                name: entry_name(other),
                path: Some(other.path.clone()),
                exists: true,
            });
        }
    }
    let variables = if character {
        let prefix = format!("{}.", entry.id);
        variable_rows(project, index)
            .into_iter()
            .filter(|v| v.key.starts_with(&prefix))
            .collect()
    } else {
        Vec::new()
    };
    Some(EntryUsage {
        character,
        scenes,
        lines,
        first,
        refs_out,
        refs_in,
        variables,
    })
}
