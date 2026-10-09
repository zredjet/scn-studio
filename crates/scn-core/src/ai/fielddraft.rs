//! 企画書と設定DBの下書き（T4-4・T4-5、D080）：応答（JSON の対応表）の読み方、空いている項目にだけ書く中身と、値のある
//! 項目への提案（項目の単位）。書くのはワークスペース（[`crate::workspace::Workspace::write_concept_draft`]）。
//!
//! - 空いている項目：キーが無い、`null`、空の文字列、空のリスト・対応表。本文は空か、ひな形のまま。
//! - 値のある項目（人が書いた項目も AI が書いた項目も）には書かず、今の値と案を並べた提案にする（原則6）。
//! - 書き換えは設定DBのフォームと同じ（直したキーの行だけ。`bible::set_field`）。読み直して確かめる。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::client::OutputSchema;
use super::run::parse_json_items;
use super::task::AiTask;
use crate::bible::{
    EditError, FORESHADOW_PAYOFF, FORESHADOW_TYPE, body_of, check_value, front_fields, set_body,
    set_field,
};
use crate::ir::Ir;
use crate::project::{FieldDef, Project};
use crate::provenance::BODY_KEY;
use crate::scaffold::CONCEPT_TEMPLATE;
use crate::sema::{Resolution, resolve_character_in};

/// 企画書の下書き（読めた項目と本文）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConceptDraft {
    /// フロントマターの項目（`logline`・`theme`・`tone`・`volume`・`main_characters`・`endings`）。
    pub values: BTreeMap<String, Value>,
    /// 本文（Markdown）。
    pub body: Option<String>,
}

/// 値のある項目への案（項目の単位。人が採用する。T4-4）。本文は `key` が [`BODY_KEY`]。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldProposal {
    pub file: String,
    pub key: String,
    pub current: Value,
    pub proposed: Value,
}

/// 企画書の短い項目（1行の文字列）。
const CONCEPT_TEXT_KEYS: [&str; 3] = ["logline", "theme", "tone"];

/// 企画書の下書きで読む項目の順。
pub const CONCEPT_KEYS: [&str; 6] = [
    "logline",
    "theme",
    "tone",
    "volume",
    "main_characters",
    "endings",
];

/// 応答の文から、最初の `{` から最後の `}` までを JSON の対応表として読む（コードブロックの囲みと前置きの文を
/// 越える）。読めなければ `None`。
fn object_of(text: &str) -> Option<Map<String, Value>> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    let inner = text.get(start..=end)?;
    match serde_json::from_str::<Value>(inner).ok()? {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

/// 1行の空でない文字列（前後の空白を除く）。
fn one_line(value: &Value) -> Option<String> {
    let s = value.as_str()?.trim();
    (!s.is_empty() && !s.contains(['\n', '\r'])).then(|| s.to_owned())
}

/// 英数字と `_` の ID（エンディングのID、シーンIDと同じ形）。
fn is_id(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// 正の整数。
fn positive(value: &Value) -> Option<u64> {
    value.as_u64().filter(|&n| n > 0)
}

/// 応答の文から企画書の下書きを読む（T4-4）。形の合わない値と知らないキーは読まずに、知らせる文を返す。
/// 対応表として読めなければ `None` と、応答の文（読めなかった行として見せる）。
pub fn parse_concept_draft(text: &str) -> (Option<ConceptDraft>, Vec<String>) {
    let Some(map) = object_of(text) else {
        let trimmed = text.trim();
        let unread = if trimmed.is_empty() {
            Vec::new()
        } else {
            vec![trimmed.to_owned()]
        };
        return (None, unread);
    };
    let mut draft = ConceptDraft::default();
    let mut notes = Vec::new();
    for (key, value) in &map {
        let skip = |notes: &mut Vec<String>, why: &str| {
            notes.push(format!("{key} は{why}ので、読みませんでした"));
        };
        match key.as_str() {
            k if CONCEPT_TEXT_KEYS.contains(&k) => match one_line(value) {
                Some(s) => {
                    draft.values.insert(key.clone(), Value::String(s));
                }
                None => skip(&mut notes, "空か、1行の文字列でない"),
            },
            "volume" => {
                let Some(obj) = value.as_object() else {
                    skip(&mut notes, "対応表でない");
                    continue;
                };
                let mut volume = Map::new();
                for sub in obj
                    .keys()
                    .filter(|k| !matches!(k.as_str(), "chars" | "routes"))
                {
                    notes.push(format!(
                        "volume.{sub} は企画書の項目ではないので、読みませんでした"
                    ));
                }
                for sub in ["chars", "routes"] {
                    match obj.get(sub) {
                        None => {}
                        Some(v) => match positive(v) {
                            Some(n) => {
                                volume.insert(sub.to_owned(), json!(n));
                            }
                            None => notes.push(format!(
                                "volume.{sub} は正の整数でないので、読みませんでした"
                            )),
                        },
                    }
                }
                if volume.is_empty() {
                    skip(&mut notes, "読める値（chars・routes）が無い");
                } else {
                    draft.values.insert(key.clone(), Value::Object(volume));
                }
            }
            "main_characters" => {
                let Some(items) = value.as_array() else {
                    skip(&mut notes, "リストでない");
                    continue;
                };
                let mut ids: Vec<Value> = Vec::new();
                for item in items {
                    match one_line(item) {
                        Some(id) if !ids.contains(&Value::String(id.clone())) => {
                            ids.push(Value::String(id));
                        }
                        Some(_) => {}
                        None => notes.push(
                            "main_characters の空か改行を含むIDは、読みませんでした".to_owned(),
                        ),
                    }
                }
                if !ids.is_empty() {
                    draft.values.insert(key.clone(), Value::Array(ids));
                }
            }
            "endings" => {
                let Some(items) = value.as_array() else {
                    skip(&mut notes, "リストでない");
                    continue;
                };
                let mut endings: Vec<Value> = Vec::new();
                let mut seen: Vec<String> = Vec::new();
                for item in items {
                    let id = item.get("id").and_then(Value::as_str).map(str::trim);
                    let Some(id) = id.filter(|id| is_id(id)) else {
                        notes.push(
                            "endings の id が英数字と _ でない案は、読みませんでした".to_owned(),
                        );
                        continue;
                    };
                    if seen.iter().any(|s| s == id) {
                        notes.push(format!(
                            "endings の id {id} が重なるので、2つ目を読みませんでした"
                        ));
                        continue;
                    }
                    seen.push(id.to_owned());
                    let mut ending = Map::new();
                    ending.insert("id".to_owned(), json!(id));
                    for sub in ["name", "intent"] {
                        match item.get(sub) {
                            None => {}
                            Some(v) => match one_line(v) {
                                Some(text) => {
                                    ending.insert(sub.to_owned(), Value::String(text));
                                }
                                None => notes.push(format!(
                                    "endings の {id} の {sub} は、空か1行の文字列でないので、読みませんでした"
                                )),
                            },
                        }
                    }
                    endings.push(Value::Object(ending));
                }
                if !endings.is_empty() {
                    draft.values.insert(key.clone(), Value::Array(endings));
                }
            }
            "body" => match value.as_str().map(str::trim) {
                Some(body) if !body.is_empty() => {
                    draft.body = Some(format!("{body}\n"));
                }
                _ => skip(&mut notes, "空か、文字列でない"),
            },
            _ => notes.push(format!(
                "{key} は企画書の項目ではないので、読みませんでした"
            )),
        }
    }
    (Some(draft), notes)
}

/// 企画書の下書きの返す形（構造化出力。T4-4）。
pub fn concept_schema() -> OutputSchema {
    let text = || json!({"type": "string"});
    OutputSchema {
        name: "concept".to_owned(),
        schema: json!({
            "type": "object",
            "properties": {
                "logline": text(),
                "theme": text(),
                "tone": text(),
                "volume": {
                    "type": "object",
                    "properties": {"chars": {"type": "integer"}, "routes": {"type": "integer"}},
                },
                "main_characters": {"type": "array", "items": text()},
                "endings": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {"id": text(), "name": text(), "intent": text()},
                        "required": ["id"],
                    },
                },
                "body": text(),
            },
        }),
    }
}

/// 空いている値か（無い、`null`、空の文字列、空のリスト・対応表）。
pub fn is_empty_value(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => true,
        Some(Value::String(s)) => s.trim().is_empty(),
        Some(Value::Array(items)) => items.is_empty(),
        Some(Value::Object(map)) => map.is_empty(),
        Some(_) => false,
    }
}

/// 本文が空か、ひな形（`scn new` の `concept.md`）のままか（改行の種類は比べない）。
fn template_body(body: &str) -> bool {
    let body = body.replace("\r\n", "\n");
    let body = body.trim();
    body.is_empty() || body == body_of(CONCEPT_TEMPLATE).trim()
}

/// 下書きを書いた中身（書いた項目と、値のある項目への提案）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldsPlan {
    pub text: String,
    /// 書いた項目（キー。本文は [`BODY_KEY`]）。
    pub written: Vec<String>,
    pub proposals: Vec<FieldProposal>,
    /// 書けなかった項目のわけ。
    pub notes: Vec<String>,
}

/// 企画書（今の中身。無ければ `None`）に下書きを書いた中身（T4-4）。読めない企画書なら `Err`。
pub fn plan_concept_write(
    file: &str,
    current: Option<&str>,
    draft: &ConceptDraft,
) -> Result<FieldsPlan, String> {
    plan_fields(
        file,
        current.unwrap_or(CONCEPT_TEMPLATE),
        &CONCEPT_KEYS.map(str::to_owned),
        &draft.values,
        draft.body.as_deref(),
        template_body,
    )
}

/// 文書（企画書かエントリ）に、項目（`keys` の順）と本文の下書きを書いた中身：空いている項目と、書いてよい本文
/// （`body_writable`）にだけ書き、値のある項目は値が違えば案にする。読めない文書なら `Err`。
fn plan_fields(
    file: &str,
    text: &str,
    keys: &[String],
    values: &BTreeMap<String, Value>,
    body: Option<&str>,
    body_writable: fn(&str) -> bool,
) -> Result<FieldsPlan, String> {
    let mut text = text.to_owned();
    let fields =
        front_fields(&text).map_err(|e| format!("{file} のフロントマターを読めません：{e}"))?;
    let mut plan = FieldsPlan {
        text: String::new(),
        written: Vec::new(),
        proposals: Vec::new(),
        notes: Vec::new(),
    };
    for key in keys {
        let Some(value) = values.get(key) else {
            continue;
        };
        let now = fields.get(key);
        if is_empty_value(now) {
            match set_field(&text, key, value) {
                Ok(next) => {
                    text = next;
                    plan.written.push(key.clone());
                }
                Err(e) => plan.notes.push(edit_note(file, key, &e)),
            }
        } else if now != Some(value) {
            plan.proposals.push(FieldProposal {
                file: file.to_owned(),
                key: key.clone(),
                current: now.cloned().unwrap_or(Value::Null),
                proposed: value.clone(),
            });
        }
    }
    if let Some(body) = body {
        let now = body_of(&text);
        if body_writable(now) {
            text = set_body(&text, body);
            plan.written.push(BODY_KEY.to_owned());
        } else if now.trim() != body.trim() {
            plan.proposals.push(FieldProposal {
                file: file.to_owned(),
                key: BODY_KEY.to_owned(),
                current: Value::String(now.to_owned()),
                proposed: Value::String(body.to_owned()),
            });
        }
    }
    plan.text = text;
    Ok(plan)
}

fn edit_note(file: &str, key: &str, e: &EditError) -> String {
    format!("{file} の {key} に書けませんでした（{e}）。ソース表示で直します")
}

// ---- 設定の下書き（T4-5） ----

/// 足りない設定の1つ（文脈の `missing_entries`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MissingEntry {
    pub type_id: String,
    /// 決まっている ID（主要キャラ、参照先）。未登録の話者では `None`（AI が付ける）。
    pub id: Option<String>,
    /// 決まっている名前（未登録の話者の書き方）。
    pub name: Option<String>,
    /// わけ（「本文の未登録の話者（2回）」など）。
    pub reason: String,
}

/// 型の名前（型の定義が無ければ型ID）。
fn type_name<'a>(project: &'a Project, type_id: &'a str) -> &'a str {
    project
        .bible
        .types
        .iter()
        .find(|t| t.id == type_id)
        .map_or(type_id, |t| t.name.as_str())
}

/// 足りない設定（T4-5）：本文の未登録の話者（W202。出た順、回数）、企画書の `main_characters` にあってエントリの
/// 無いキャラクター、エントリの参照先の無い参照（W806。シーンへの参照は除く）。重なるものは1つにする。
pub fn missing_entries(project: &Project, ir: &Ir, concept: Option<&str>) -> Vec<MissingEntry> {
    let character = project.bible.character_type().to_owned();
    let mut out: Vec<MissingEntry> = Vec::new();
    let mut speakers: Vec<(String, usize)> = Vec::new();
    for d in ir.diagnostics.iter().filter(|d| d.code == "W202") {
        let Some(text) = project
            .files
            .iter()
            .find(|f| f.path == d.file)
            .and_then(|f| f.text.get(d.span.start..d.span.end))
        else {
            continue;
        };
        let name = text.trim().to_owned();
        // 2人以上に当てはまる話者（W202 は、それでも出る）は、未登録ではない（作っても解決しない）。
        if name.is_empty()
            || !matches!(
                resolve_character_in(&project.characters, &name),
                Resolution::NotFound
            )
        {
            continue;
        }
        match speakers.iter_mut().find(|(n, _)| *n == name) {
            Some((_, count)) => *count += 1,
            None => speakers.push((name, 1)),
        }
    }
    for (name, count) in speakers {
        out.push(MissingEntry {
            type_id: character.clone(),
            id: None,
            name: Some(name),
            reason: format!("本文の未登録の話者（{count}回）"),
        });
    }
    let main: Vec<String> = concept
        .and_then(|text| front_fields(text).ok())
        .and_then(|f| f.get("main_characters").cloned())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v.as_str().map(str::trim).map(str::to_owned))
        .filter(|id| !id.is_empty())
        .collect();
    for id in main {
        let known = project.character_entry(&id).is_some()
            || out
                .iter()
                .any(|m| m.type_id == character && m.id.as_deref() == Some(&id));
        if !known {
            out.push(MissingEntry {
                type_id: character.clone(),
                id: Some(id),
                name: None,
                reason: "企画書の主要キャラ（main_characters）".to_owned(),
            });
        }
    }
    for m in crate::bible::missing_refs(project, ir) {
        // シーンへの参照と、型の定義の無い型への参照（作れない）は除く。
        let creatable =
            m.target == character || project.bible.types.iter().any(|t| t.id == m.target);
        if m.target == "scene"
            || !creatable
            || out
                .iter()
                .any(|o| o.type_id == m.target && o.id.as_deref() == Some(m.id.as_str()))
        {
            continue;
        }
        out.push(MissingEntry {
            type_id: m.target.to_owned(),
            id: Some(m.id.clone()),
            name: None,
            reason: format!(
                "{} の{}（{}）の参照先",
                m.entry.id, m.field.label, m.field.key
            ),
        });
    }
    out
}

/// 型の項目（キャラクターの `name`・`aliases` も、定義に書いてあれば）。`id`・`name` は除く。
fn type_fields<'a>(project: &'a Project, type_id: &str) -> Vec<&'a FieldDef> {
    project
        .bible
        .types
        .iter()
        .find(|t| t.id == type_id)
        .map(|t| {
            t.fields
                .iter()
                .filter(|f| !matches!(f.key.as_str(), "id" | "name" | "status"))
                .collect()
        })
        .unwrap_or_default()
}

/// 今あるエントリのうち、型の項目か本文が空いているもの（パス、見せ方、空いている項目のキー。本文は
/// [`BODY_KEY`]）。型の定義の無いエントリは除く。
pub fn empty_fields(project: &Project) -> Vec<(String, String, Vec<String>)> {
    let mut out = Vec::new();
    for entry in &project.bible.entries {
        if !project.bible.types.iter().any(|t| t.id == entry.type_id) {
            continue;
        }
        let mut keys: Vec<String> = type_fields(project, &entry.type_id)
            .into_iter()
            .filter(|f| is_empty_value(entry.fields.get(&f.key)))
            .map(|f| f.key.clone())
            .collect();
        if entry.body.trim().is_empty() {
            keys.push(BODY_KEY.to_owned());
        }
        if keys.is_empty() {
            continue;
        }
        let name = entry
            .fields
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&entry.id);
        out.push((entry.path.clone(), format!("{}（{name}）", entry.id), keys));
    }
    out
}

/// 文脈の節「設定DBの型」の文（T4-5）。型が無ければ `None`。
pub(crate) fn types_text(project: &Project) -> Option<String> {
    let character = project.bible.character_type();
    let mut blocks: Vec<String> = project
        .bible
        .types
        .iter()
        .map(|t| {
            let mut lines = vec![format!("### {}（{}）", t.name, t.id)];
            lines.push("- name（名前、text。必須）".to_owned());
            for f in t
                .fields
                .iter()
                .filter(|f| !matches!(f.key.as_str(), "id" | "name"))
            {
                let options = if f.options.is_empty() {
                    String::new()
                } else {
                    format!("。選択肢：{}", f.options.join("・"))
                };
                lines.push(format!("- {}（{}、{}{options}）", f.key, f.label, f.ty));
            }
            lines.join("\n")
        })
        .collect();
    if !project.bible.types.iter().any(|t| t.id == character) {
        blocks.insert(
            0,
            format!("### キャラクター（{character}）\n- name（名前、text。必須）"),
        );
    }
    if blocks.is_empty() {
        return None;
    }
    Some(format!("## 設定DBの型\n\n{}", blocks.join("\n\n")))
}

/// 文脈の節「足りない設定」の文（T4-5）。足りない設定も空いている項目も無ければ `None`。
pub(crate) fn missing_text(project: &Project, ir: &Ir, concept: Option<&str>) -> Option<String> {
    let missing = missing_entries(project, ir, concept);
    let empty = empty_fields(project);
    if missing.is_empty() && empty.is_empty() {
        return None;
    }
    let mut out = String::from("## 足りない設定\n");
    if missing.is_empty() {
        out.push_str("\n（ありません）\n");
    } else {
        out.push('\n');
        for m in &missing {
            let what = match (&m.id, &m.name) {
                (Some(id), Some(name)) => format!("{id}「{name}」"),
                (Some(id), None) => id.clone(),
                (None, Some(name)) => format!("「{name}」（IDは英数字と _ で付けます）"),
                (None, None) => String::new(),
            };
            out.push_str(&format!(
                "- {}（{}） {what}：{}\n",
                type_name(project, &m.type_id),
                m.type_id,
                m.reason
            ));
        }
    }
    if !empty.is_empty() {
        out.push_str("\n### 空いている項目のあるエントリ\n\n");
        for (path, label, keys) in &empty {
            let entry = project.bible.entries.iter().find(|e| &e.path == path);
            let fields = entry
                .map(|e| type_fields(project, &e.type_id))
                .unwrap_or_default();
            let names: Vec<String> = keys
                .iter()
                .map(|k| {
                    if k == BODY_KEY {
                        "本文".to_owned()
                    } else {
                        fields
                            .iter()
                            .find(|f| &f.key == k)
                            .map_or_else(|| k.clone(), |f| f.label.clone())
                    }
                })
                .collect();
            out.push_str(&format!("- {label}：{}\n", names.join("、")));
        }
    }
    Some(out.trim_end().to_owned())
}

/// 設定の下書きの1つのエントリ（読めたもの）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryDraft {
    #[serde(rename = "type")]
    pub type_id: String,
    pub id: String,
    pub name: String,
    /// 型の項目（型に合う値だけ）。
    pub fields: BTreeMap<String, Value>,
    pub body: Option<String>,
}

/// 設定の下書き（T4-5）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntriesDraft {
    pub entries: Vec<EntryDraft>,
}

/// 応答の文から、エントリのリスト（`{"entries": [...]}`、リスト、1行に1つの JSON）を読む（指摘と同じ読み方）。
fn entry_items(text: &str) -> Option<Vec<Value>> {
    let (items, _) = parse_json_items(text, "entries", &|v: &Value| {
        v.is_object().then(|| v.clone())
    });
    (!items.is_empty()).then_some(items)
}

/// 応答の文から設定の下書きを読む（T4-5）。読まないもの（型の無い型、IDの形、名前、同じ型とIDの2つ目、型に無い
/// 項目、`id`・`name`・`status` の項目、型に合わない値、空の値）は知らせる。
pub fn parse_entries_draft(text: &str, project: &Project) -> (Option<EntriesDraft>, Vec<String>) {
    let Some(items) = entry_items(text) else {
        let trimmed = text.trim();
        let unread = if trimmed.is_empty() {
            Vec::new()
        } else {
            vec![trimmed.to_owned()]
        };
        return (None, unread);
    };
    let character = project.bible.character_type();
    let mut draft = EntriesDraft::default();
    let mut notes = Vec::new();
    for item in &items {
        let type_id = item
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let id = item.get("id").and_then(Value::as_str).unwrap_or("").trim();
        let label = format!("{id}（{type_id}）");
        if type_id != character && !project.bible.types.iter().any(|t| t.id == type_id) {
            notes.push(format!(
                "{label} は型の定義の無い型なので、読みませんでした"
            ));
            continue;
        }
        if !is_id(id) {
            notes.push(format!(
                "{label} の ID が英数字と _ でないので、読みませんでした"
            ));
            continue;
        }
        let Some(name) = item.get("name").and_then(one_line) else {
            notes.push(format!(
                "{label} の名前が空か1行でないので、読みませんでした"
            ));
            continue;
        };
        if draft
            .entries
            .iter()
            .any(|e| e.type_id == type_id && e.id == id)
        {
            notes.push(format!(
                "{label}「{name}」は同じ型とIDの2つ目なので、読みませんでした"
            ));
            continue;
        }
        let defs = type_fields(project, type_id);
        let mut fields = BTreeMap::new();
        if let Some(map) = item.get("fields").and_then(Value::as_object) {
            for (key, value) in map {
                let Some(def) = defs.iter().find(|f| &f.key == key) else {
                    notes.push(format!(
                        "{label} の {key} は型の項目ではないので、読みませんでした"
                    ));
                    continue;
                };
                if is_empty_value(Some(value)) {
                    continue;
                }
                // 伏線の回収は W703 で調べる（シーンIDのリストも受け付ける。W805 では調べない）。
                let payoff = type_id == FORESHADOW_TYPE && key == FORESHADOW_PAYOFF;
                if payoff {
                    let ids = match value {
                        Value::String(s) => vec![s.as_str()],
                        Value::Array(items) => items.iter().filter_map(Value::as_str).collect(),
                        _ => Vec::new(),
                    };
                    if ids.is_empty() || ids.iter().any(|id| !is_id(id)) {
                        notes.push(format!(
                            "{label} の {key} はシーンIDかそのリストでないので、読みませんでした"
                        ));
                    } else {
                        fields.insert(key.clone(), value.clone());
                    }
                    continue;
                }
                if let Some(problem) = check_value(&def.kind, &def.options, value) {
                    notes.push(format!(
                        "{label} の {key} は{problem}ので、読みませんでした"
                    ));
                    continue;
                }
                fields.insert(key.clone(), value.clone());
            }
        }
        let body = item
            .get("body")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|b| !b.is_empty())
            .map(|b| format!("{b}\n"));
        draft.entries.push(EntryDraft {
            type_id: type_id.to_owned(),
            id: id.to_owned(),
            name,
            fields,
            body,
        });
    }
    (Some(draft), notes)
}

/// 設定の下書きの返す形（構造化出力。T4-5）。
pub fn entries_schema() -> OutputSchema {
    let text = || json!({"type": "string"});
    OutputSchema {
        name: "entries".to_owned(),
        schema: json!({
            "type": "object",
            "properties": {
                "entries": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "type": text(),
                            "id": text(),
                            "name": text(),
                            "fields": {"type": "object"},
                            "body": text(),
                        },
                        "required": ["type", "id", "name"],
                    },
                },
            },
            "required": ["entries"],
        }),
    }
}

/// 新しいキャラクターの ID・名前・呼び名が、今あるキャラクターの ID・名前・呼び名と重なるか（重なれば、人が書いた
/// 台詞の話者が2人以上に当てはまり、地の文になってしまう）。重なる書き方を返す。
pub fn character_overlap(project: &Project, draft: &EntryDraft) -> Option<String> {
    if draft.type_id != project.bible.character_type() {
        return None;
    }
    let mut names = vec![draft.id.clone(), draft.name.clone()];
    if let Some(Value::Array(aliases)) = draft.fields.get("aliases") {
        names.extend(aliases.iter().filter_map(Value::as_str).map(str::to_owned));
    }
    names.into_iter().find(|name| {
        !matches!(
            resolve_character_in(&project.characters, name),
            Resolution::NotFound
        )
    })
}

/// 今あるエントリに書く中身（T4-5）：型の項目（型の定義の順）と `name`、空の本文。
pub fn plan_entry_write(
    project: &Project,
    file: &str,
    text: &str,
    draft: &EntryDraft,
) -> Result<FieldsPlan, String> {
    let mut keys = vec!["name".to_owned()];
    keys.extend(
        type_fields(project, &draft.type_id)
            .into_iter()
            .map(|f| f.key.clone()),
    );
    let mut values = draft.fields.clone();
    values.insert("name".to_owned(), Value::String(draft.name.clone()));
    plan_fields(file, text, &keys, &values, draft.body.as_deref(), |body| {
        body.trim().is_empty()
    })
}

/// 新しいエントリの中身（T4-5）：`id`・`name`・`status: ai-draft`・項目（型の定義の順）・本文。書いた項目
/// （`name`・項目・本文）も返す。
pub fn new_entry_text(
    project: &Project,
    draft: &EntryDraft,
) -> Result<(String, Vec<String>), String> {
    let mut text = format!("---\nid: {}\n---\n", draft.id);
    let mut written = Vec::new();
    let put = |text: &mut String, key: &str, value: &Value| -> Result<(), String> {
        *text = set_field(text, key, value).map_err(|e| format!("{key}：{e}"))?;
        Ok(())
    };
    put(&mut text, "name", &Value::String(draft.name.clone()))?;
    written.push("name".to_owned());
    put(&mut text, "status", &json!("ai-draft"))?;
    for def in type_fields(project, &draft.type_id) {
        if let Some(value) = draft.fields.get(&def.key) {
            put(&mut text, &def.key, value)?;
            written.push(def.key.clone());
        }
    }
    if let Some(body) = &draft.body {
        text = set_body(&text, body);
        written.push(BODY_KEY.to_owned());
    }
    Ok((text, written))
}

/// 対象のエントリを選んで頼むタスク（T4-5）：依頼の後ろに、対象のエントリと空いている項目を書き足す。対象が
/// 設定DBのエントリでなければ `Err`。
pub fn with_entry_target(task: &AiTask, project: &Project, path: &str) -> Result<AiTask, String> {
    let entry = project
        .bible
        .entries
        .iter()
        .find(|e| e.path == path)
        .ok_or_else(|| format!("設定DBのエントリ {path} がありません"))?;
    let empty = empty_fields(project)
        .into_iter()
        .find(|(p, _, _)| p == path)
        .map(|(_, _, keys)| keys)
        .unwrap_or_default();
    let mut t = task.clone();
    let prompt = t.prompt.unwrap_or_default();
    t.prompt = Some(format!(
        "{}\n\n対象のエントリ：{path}（{}、型 {}）。このエントリの空いている項目（{}）だけを書きます。新しいエントリは作りません。",
        prompt.trim_end(),
        entry.id,
        entry.type_id,
        if empty.is_empty() {
            "なし".to_owned()
        } else {
            empty.join("、")
        }
    ));
    Ok(t)
}
