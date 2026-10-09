//! 企画書と設定DBの下書き（T4-4、D080）：応答（JSON の対応表）の読み方、空いている項目にだけ書く中身と、値のある
//! 項目への提案（項目の単位）。書くのはワークスペース（[`crate::workspace::Workspace::write_concept_draft`]）。
//!
//! - 空いている項目：キーが無い、`null`、空の文字列、空のリスト・対応表。本文は空か、ひな形のまま。
//! - 値のある項目（人が書いた項目も AI が書いた項目も）には書かず、今の値と案を並べた提案にする（原則6）。
//! - 書き換えは設定DBのフォームと同じ（直したキーの行だけ。`bible::set_field`）。読み直して確かめる。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::client::OutputSchema;
use crate::bible::{EditError, body_of, front_fields, set_body, set_field};
use crate::provenance::BODY_KEY;
use crate::scaffold::CONCEPT_TEMPLATE;

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
    let mut text = current.unwrap_or(CONCEPT_TEMPLATE).to_owned();
    let fields =
        front_fields(&text).map_err(|e| format!("{file} のフロントマターを読めません：{e}"))?;
    let mut plan = FieldsPlan {
        text: String::new(),
        written: Vec::new(),
        proposals: Vec::new(),
        notes: Vec::new(),
    };
    for key in CONCEPT_KEYS {
        let Some(value) = draft.values.get(key) else {
            continue;
        };
        let now = fields.get(key);
        if is_empty_value(now) {
            match set_field(&text, key, value) {
                Ok(next) => {
                    text = next;
                    plan.written.push(key.to_owned());
                }
                Err(e) => plan.notes.push(edit_note(file, key, &e)),
            }
        } else if now != Some(value) {
            plan.proposals.push(FieldProposal {
                file: file.to_owned(),
                key: key.to_owned(),
                current: now.cloned().unwrap_or(Value::Null),
                proposed: value.clone(),
            });
        }
    }
    if let Some(body) = &draft.body {
        let now = body_of(&text);
        if template_body(now) {
            text = set_body(&text, body);
            plan.written.push(BODY_KEY.to_owned());
        } else if now.trim() != body.trim() {
            plan.proposals.push(FieldProposal {
                file: file.to_owned(),
                key: BODY_KEY.to_owned(),
                current: Value::String(now.to_owned()),
                proposed: Value::String(body.clone()),
            });
        }
    }
    plan.text = text;
    Ok(plan)
}

fn edit_note(file: &str, key: &str, e: &EditError) -> String {
    format!("{file} の {key} に書けませんでした（{e}）。ソース表示で直します")
}
