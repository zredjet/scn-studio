//! 書いている最中の補助（アプリ、T1-8、D047）：文字数、今のシーンの情報（文脈パネル）、補完の材料。
//!
//! どれもIR・索引・プロジェクトの宣言から導く（テキストが正。設計の原則1）。

use std::collections::BTreeMap;

use serde::Serialize;

use crate::index::Index;
use crate::ir::{IrInline, Op};
use crate::project::{Entry, Project};
use crate::syntax::{BGM_STOP, DIRECTIVES, DirectiveInfo, POSITIONS};

/// 表示する文字の数（D047）。ルビは親文字、傍点は中身を数え、インライン命令と変数の埋め込み、
/// 空白（半角・全角・タブ）は数えない。全角も半角も1字に数える（はみ出しの数え方とは分ける）。
pub fn display_chars(text: &[IrInline]) -> usize {
    text.iter()
        .map(|inline| match inline {
            IrInline::Text { text } | IrInline::Emphasis { text } => count(text),
            IrInline::Ruby { base, .. } => count(base),
            _ => 0,
        })
        .sum()
}

fn count(text: &str) -> usize {
    text.chars()
        .filter(|c| !matches!(c, ' ' | '\t' | '\u{3000}'))
        .count()
}

/// シーンの文字数：台詞、地の文、選択肢の問いかけ文と表示文（選択肢と条件のブロックの中を含む）。
/// 話者名、括弧、ディレクティブ、コメント、行IDは数えない。
pub fn scene_chars(ops: &[Op]) -> usize {
    ops.iter()
        .map(|op| match op {
            Op::Say { text, .. } | Op::Narrate { text, .. } => display_chars(text),
            Op::Choice {
                prompt, options, ..
            } => {
                prompt.as_deref().map_or(0, display_chars)
                    + options
                        .iter()
                        .map(|o| display_chars(&o.text) + scene_chars(&o.body))
                        .sum::<usize>()
            }
            Op::If {
                branches,
                else_body,
                ..
            } => {
                branches.iter().map(|b| scene_chars(&b.body)).sum::<usize>()
                    + else_body.as_deref().map_or(0, scene_chars)
            }
            _ => 0,
        })
        .sum()
}

/// 今のシーンの情報（文脈パネル。docs/ui/mockups/02-editor.html）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SceneInfo {
    pub scene: String,
    /// 登場キャラ（話者、`@立ち絵`、`@退場` に出たキャラクター）。最初に出た順。
    pub characters: Vec<SceneCharacter>,
    /// 読み書きした変数。名前の順。
    pub variables: Vec<SceneVariable>,
    /// 使った素材。種類・キャラクター・名前の順。
    pub assets: Vec<SceneAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SceneCharacter {
    pub id: String,
    pub name: String,
    /// 一人称（設定DBのエントリの `first_person`）。
    pub first_person: Option<String>,
    /// 口調メモ（設定DBのエントリの `speech`）。
    pub speech: Option<String>,
    /// このシーンで話者になった台詞の数。
    pub lines: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SceneVariable {
    pub key: String,
    /// 宣言の型。宣言が無ければ `None`。
    #[serde(rename = "type")]
    pub ty: Option<&'static str>,
    pub reads: usize,
    pub writes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SceneAsset {
    /// `background`・`expression`・`bgm`・`se`。
    pub kind: &'static str,
    /// 差分のキャラクター（`expression` のとき。名前に直してある）。
    pub character: Option<String>,
    pub name: String,
    /// 素材カタログにあるか（無ければ仮素材）。
    pub registered: bool,
}

/// エントリの項目を文字列で読む（数や真偽も文字列にする）。
fn field(entry: Option<&Entry>, key: &str) -> Option<String> {
    match entry?.fields.get(key)? {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// シーンの情報を、索引から集める。
pub fn scene_info(project: &Project, index: &Index, scene: &str) -> SceneInfo {
    // 登場キャラは、本文で最初に出た位置の順に並べる。
    let mut characters: Vec<(usize, SceneCharacter)> = index
        .characters
        .iter()
        .filter_map(|c| {
            let uses: Vec<_> = c
                .uses
                .iter()
                .filter(|u| u.location.scene == scene)
                .collect();
            let first = uses.iter().map(|u| u.location.span.start).min()?;
            let entry = project.character_entry(&c.id);
            Some((
                first,
                SceneCharacter {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    first_person: field(entry, "first_person"),
                    speech: field(entry, "speech"),
                    lines: uses.iter().filter(|u| u.kind == "speaker").count(),
                },
            ))
        })
        .collect();
    characters.sort_by_key(|(first, _)| *first);
    let variables = index
        .variables
        .iter()
        .filter_map(|v| {
            let reads = v.reads.iter().filter(|l| l.scene == scene).count();
            let writes = v.writes.iter().filter(|l| l.scene == scene).count();
            (reads + writes > 0).then(|| SceneVariable {
                key: v.key.clone(),
                ty: v.ty,
                reads,
                writes,
            })
        })
        .collect();
    let names: BTreeMap<&str, &str> = project
        .characters
        .iter()
        .map(|c| (c.id.as_str(), c.name.as_str()))
        .collect();
    let assets = index
        .assets
        .iter()
        .filter(|a| a.uses.iter().any(|l| l.scene == scene))
        .map(|a| SceneAsset {
            kind: a.kind,
            character: a
                .character
                .as_deref()
                .map(|c| names.get(c).copied().unwrap_or(c).to_owned()),
            name: a.name.clone(),
            registered: a.registered,
        })
        .collect();
    SceneInfo {
        scene: scene.to_owned(),
        characters: characters.into_iter().map(|(_, c)| c).collect(),
        variables,
        assets,
    }
}

/// 補完の材料（アプリのエディタ。開いたときと、設定ファイルが変わったときに渡す）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompletionData {
    pub characters: Vec<CompletionCharacter>,
    pub backgrounds: Vec<String>,
    /// キャラクターIDごとの立ち絵の差分。
    pub expressions: BTreeMap<String, Vec<String>>,
    pub bgm: Vec<String>,
    pub se: Vec<String>,
    pub variables: Vec<CompletionVariable>,
    pub directives: Vec<DirectiveInfo>,
    /// `@立ち絵` の位置のキーワード。
    pub positions: Vec<&'static str>,
    /// `@BGM` の停止のキーワード。
    pub bgm_stop: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompletionCharacter {
    pub id: String,
    pub name: String,
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompletionVariable {
    pub key: String,
    #[serde(rename = "type")]
    pub ty: &'static str,
    /// 列挙の値。
    pub values: Vec<String>,
}

/// 補完の材料を、プロジェクトの宣言（設定DB、素材カタログ、変数）から集める。
pub fn completion_data(project: &Project) -> CompletionData {
    CompletionData {
        characters: project
            .characters
            .iter()
            .map(|c| CompletionCharacter {
                id: c.id.clone(),
                name: c.name.clone(),
                aliases: c.aliases.clone(),
            })
            .collect(),
        backgrounds: project.catalog.backgrounds.iter().cloned().collect(),
        expressions: project
            .catalog
            .characters
            .iter()
            .map(|(id, set)| (id.clone(), set.iter().cloned().collect()))
            .collect(),
        bgm: project.catalog.bgm.iter().cloned().collect(),
        se: project.catalog.se.iter().cloned().collect(),
        variables: project
            .variables
            .iter()
            .map(|v| CompletionVariable {
                key: v.key.clone(),
                ty: v.ty.name(),
                values: match &v.ty {
                    crate::project::VarType::Enum(values) => values.clone(),
                    _ => Vec::new(),
                },
            })
            .collect(),
        directives: DIRECTIVES.to_vec(),
        positions: POSITIONS.to_vec(),
        bgm_stop: BGM_STOP,
    }
}
