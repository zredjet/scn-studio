//! 企画書の画面（`concept.md`、T2-6、D057）のデータ：フロントマターの項目、主要キャラの欲求と葛藤（設定DB
//! から）、企画書のエンディングと本文の `@エンディング` の対応（W701・W702 と同じ照らし方）。
//!
//! 企画書の書き換えは、設定DBのエントリと同じ（`workspace::set_entry_field` など）。

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::bible::{body_of, front_fields, scalar_text};
use crate::diag::Span;
use crate::ir::{Ir, Op};
use crate::project::Project;
use crate::route::collect_ending_ops;

/// 企画書のファイル。
pub const CONCEPT_FILE: &str = "concept.md";

/// 主要キャラの欲求と葛藤のキー（キャラクターの型の項目。project-files 5節）。
const TRAIT_KEYS: [(&str, &str); 2] = [("want", "欲求"), ("conflict", "葛藤")];

/// 本文のシーン（エンディングのある場所）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SceneRef {
    pub id: String,
    pub title: String,
    pub file: String,
    pub span: Span,
}

/// 主要キャラの1つの項目（欲求、葛藤）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CharacterTrait {
    pub label: String,
    pub value: String,
}

/// 主要キャラ（`main_characters` のID）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MainCharacter {
    pub id: String,
    /// 設定DBのエントリの名前。エントリが無ければ `None`。
    pub name: Option<String>,
    pub path: Option<String>,
    pub traits: Vec<CharacterTrait>,
}

/// 企画書のエンディングと、それを書いた本文のシーン（無ければ未作成）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlannedEnding {
    pub id: String,
    pub name: String,
    pub intent: String,
    pub scenes: Vec<SceneRef>,
}

/// 本文にだけあるエンディング（企画書の `endings` に無い）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnplannedEnding {
    pub id: String,
    pub scenes: Vec<SceneRef>,
}

/// 企画書の画面のデータ。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConceptData {
    /// `concept.md` の中身。無ければ `None`（画面は「企画書を作る」を出す）。
    pub text: Option<String>,
    /// 読み方の知らせ（読めないフロントマター、形の違う項目）。
    pub problems: Vec<String>,
    /// フロントマターの値（キーごと。フォームの入力欄の値）。
    pub fields: BTreeMap<String, Value>,
    pub logline: String,
    pub theme: String,
    pub tone: String,
    pub volume_chars: Option<i64>,
    pub volume_routes: Option<i64>,
    pub main_characters: Vec<MainCharacter>,
    pub endings: Vec<PlannedEnding>,
    /// 企画書に `endings` が書いてあるか（無ければ、本文のエンディングと照らさない。W701・W702 と同じ）。
    pub endings_listed: bool,
    pub unplanned: Vec<UnplannedEnding>,
    /// フロントマターの後ろの本文（Markdown）。
    pub body: String,
}

/// 本文の `@エンディング` のIDと、そのシーン（本文の順。同じIDが複数のシーンにあれば、それぞれ）。
fn body_endings(ir: &Ir) -> Vec<(String, SceneRef)> {
    let mut out = Vec::new();
    for scene in &ir.scenes {
        let mut ops = Vec::new();
        collect_ending_ops(&scene.ops, &mut ops);
        for op in ops {
            if let Op::Ending { id: Some(id), .. } = op {
                let at = SceneRef {
                    id: scene.id.clone(),
                    title: scene.title.clone(),
                    file: scene.file.clone(),
                    span: scene.span,
                };
                if !out
                    .iter()
                    .any(|(i, s): &(String, SceneRef)| i == id && s.id == at.id)
                {
                    out.push((id.clone(), at));
                }
            }
        }
    }
    out
}

/// 数（`300000` か `"300000"`）。
fn number(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// 企画書の画面のデータを作る。
pub fn concept_data(root: &Path, project: &Project, ir: &Ir) -> ConceptData {
    let text = fs::read(root.join(CONCEPT_FILE))
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok());
    let mut data = ConceptData {
        text: text.clone(),
        problems: Vec::new(),
        fields: BTreeMap::new(),
        logline: String::new(),
        theme: String::new(),
        tone: String::new(),
        volume_chars: None,
        volume_routes: None,
        main_characters: Vec::new(),
        endings: Vec::new(),
        endings_listed: false,
        unplanned: Vec::new(),
        body: String::new(),
    };
    let found = body_endings(ir);
    let fields = match &text {
        None => Default::default(),
        Some(text) => {
            body_of(text).clone_into(&mut data.body);
            match front_fields(text) {
                Ok(fields) => fields,
                Err(e) => {
                    data.problems.push(format!(
                        "フロントマターをYAMLとして読めません（{e}）。ソースで直してください"
                    ));
                    Default::default()
                }
            }
        }
    };
    let text_of = |key: &str| fields.get(key).and_then(scalar_text).unwrap_or_default();
    data.logline = text_of("logline");
    data.theme = text_of("theme");
    data.tone = text_of("tone");
    if let Some(volume) = fields.get("volume") {
        data.volume_chars = number(volume.get("chars"));
        data.volume_routes = number(volume.get("routes"));
    }
    // 主要キャラ（設定DBのキャラクターの型のエントリ）。
    let bible = &project.bible;
    let character_type = bible.character_type();
    let ty = bible.types.iter().find(|t| t.id == character_type);
    let ids: Vec<String> = match fields.get("main_characters") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items.iter().filter_map(scalar_text).collect(),
        Some(_) => {
            data.problems
                .push("main_characters はキャラクターのIDのリストで書きます".to_owned());
            Vec::new()
        }
    };
    for id in ids {
        let entry = bible
            .entries
            .iter()
            .find(|e| e.type_id == character_type && e.id == id);
        let traits = entry
            .map(|e| {
                TRAIT_KEYS
                    .iter()
                    .map(|(key, fallback)| CharacterTrait {
                        label: ty
                            .and_then(|t| t.fields.iter().find(|f| f.key == *key))
                            .map_or_else(|| (*fallback).to_owned(), |f| f.label.clone()),
                        value: e.fields.get(*key).and_then(scalar_text).unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        data.main_characters.push(MainCharacter {
            name: entry.map(|e| {
                e.fields
                    .get("name")
                    .and_then(scalar_text)
                    .unwrap_or_else(|| e.id.clone())
            }),
            path: entry.map(|e| e.path.clone()),
            id,
            traits,
        });
    }
    // エンディング。
    match fields.get("endings") {
        None | Some(Value::Null) => {}
        Some(Value::Array(items)) => {
            data.endings_listed = true;
            for item in items {
                let Some(id) = item.get("id").and_then(scalar_text) else {
                    data.problems
                        .push("endings の要素は、id を持つ対応表で書きます".to_owned());
                    continue;
                };
                let field = |key: &str| item.get(key).and_then(scalar_text).unwrap_or_default();
                data.endings.push(PlannedEnding {
                    scenes: found
                        .iter()
                        .filter(|(i, _)| *i == id)
                        .map(|(_, s)| s.clone())
                        .collect(),
                    name: field("name"),
                    intent: field("intent"),
                    id,
                });
            }
        }
        Some(_) => data
            .problems
            .push("endings はエンディングのリストで書きます".to_owned()),
    }
    data.fields = fields;
    for (id, scene) in found {
        if data.endings.iter().any(|e| e.id == id) {
            continue;
        }
        match data.unplanned.iter_mut().find(|u| u.id == id) {
            Some(u) => u.scenes.push(scene),
            None => data.unplanned.push(UnplannedEnding {
                id,
                scenes: vec![scene],
            }),
        }
    }
    data
}
