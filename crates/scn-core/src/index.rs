//! 索引：エディタの補完、設定DBの逆引き、検証が共通に使う一覧（T1-3、仕様書17節）。
//!
//! IRとプロジェクトの宣言（設定DB、`variables.yaml`、素材カタログ）から導く。テキストが正なので、
//! 索引は保存せず、解析のたびに作り直す。位置は行の単位（命令や選択肢のスパン）で持つ。

use std::collections::BTreeMap;

use serde::Serialize;

use crate::diag::Span;
use crate::ir::{Ir, IrExpr, IrInline, Op};
use crate::project::{Project, VarType};

/// 索引の形のバージョン。形を変えたら上げ、仕様書17節も更新する。
pub const INDEX_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Index {
    pub index_version: u32,
    /// シーン。IRの順。
    pub scenes: Vec<IndexScene>,
    /// 設定DBのキャラクター。IDの順。
    pub characters: Vec<IndexCharacter>,
    /// 宣言された変数と、宣言されずに使われた変数。名前の順。
    pub variables: Vec<IndexVariable>,
    /// カタログに登録された意味名と、登録されずに使われた意味名。種類・キャラクター・名前の順。
    pub assets: Vec<IndexAsset>,
    /// 行ID。本文に現れる順（重複していれば、それぞれを並べる）。
    pub line_ids: Vec<IndexLineId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IndexScene {
    pub id: String,
    pub title: String,
    pub file: String,
    /// 見出しの行。
    pub span: Span,
    pub route: Option<String>,
    pub status: Option<String>,
    pub is_stub: bool,
}

/// 本文の中の位置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Location {
    /// シーンID。
    pub scene: String,
    pub file: String,
    /// その行（命令、台詞、選択肢など）のスパン。
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IndexCharacter {
    pub id: String,
    pub name: String,
    pub aliases: Vec<String>,
    /// 話者になった台詞（`speaker`）と、`@立ち絵`（`show`）・`@退場`（`hide`）。本文の順。
    pub uses: Vec<CharacterUse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CharacterUse {
    /// `speaker`・`show`・`hide`。
    pub kind: &'static str,
    #[serde(flatten)]
    pub location: Location,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IndexVariable {
    /// そろえた変数名（`akari.好感度`）。
    pub key: String,
    pub declared: bool,
    /// 宣言の型（`bool`・`int`・`string`・`enum`）。宣言が無ければ `None`。
    #[serde(rename = "type")]
    pub ty: Option<&'static str>,
    /// 列挙の値。
    pub values: Vec<String>,
    /// 読む位置：条件（`@if`・`@elif`・選択肢の条件）、変数の埋め込み、`@設定` の右辺。
    pub reads: Vec<Location>,
    /// 書く位置：`@設定`（`=`・`+=`・`-=`）。
    pub writes: Vec<Location>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IndexAsset {
    /// `background`・`expression`（立ち絵の差分）・`bgm`・`se`。
    pub kind: &'static str,
    /// 差分のキャラクターID（解決できなければ書かれた名前）。`expression` のときだけ。
    pub character: Option<String>,
    pub name: String,
    /// 素材カタログにあるか。
    pub registered: bool,
    pub uses: Vec<Location>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IndexLineId {
    pub id: String,
    #[serde(flatten)]
    pub location: Location,
}

/// IRとプロジェクトから、索引を作る。
pub fn build_index(project: &Project, ir: &Ir) -> Index {
    let mut builder = Builder::new(project);
    let mut scenes = Vec::new();
    for scene in &ir.scenes {
        scenes.push(IndexScene {
            id: scene.id.clone(),
            title: scene.title.clone(),
            file: scene.file.clone(),
            span: scene.span,
            route: scene.meta.route.clone(),
            status: scene.meta.status.clone(),
            is_stub: scene.is_stub,
        });
        builder.scene = scene.id.clone();
        builder.file = scene.file.clone();
        builder.ops(&scene.ops);
    }
    Index {
        index_version: INDEX_VERSION,
        scenes,
        characters: builder.characters.into_values().collect(),
        variables: builder.variables.into_values().collect(),
        assets: builder.assets.into_values().collect(),
        line_ids: builder.line_ids,
    }
}

/// 素材の並び：種類・キャラクター・名前。
type AssetKey = (u8, String, String);

struct Builder<'p> {
    project: &'p Project,
    scene: String,
    file: String,
    characters: BTreeMap<String, IndexCharacter>,
    variables: BTreeMap<String, IndexVariable>,
    assets: BTreeMap<AssetKey, IndexAsset>,
    line_ids: Vec<IndexLineId>,
}

/// 素材の種類と、並びの順。
const BACKGROUND: (&str, u8) = ("background", 0);
const EXPRESSION: (&str, u8) = ("expression", 1);
const BGM: (&str, u8) = ("bgm", 2);
const SE: (&str, u8) = ("se", 3);

impl<'p> Builder<'p> {
    fn new(project: &'p Project) -> Self {
        let mut builder = Self {
            project,
            scene: String::new(),
            file: String::new(),
            characters: BTreeMap::new(),
            variables: BTreeMap::new(),
            assets: BTreeMap::new(),
            line_ids: Vec::new(),
        };
        for c in &project.characters {
            builder.characters.insert(
                c.id.clone(),
                IndexCharacter {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    aliases: c.aliases.clone(),
                    uses: Vec::new(),
                },
            );
        }
        for v in &project.variables {
            let values = match &v.ty {
                VarType::Enum(values) => values.clone(),
                _ => Vec::new(),
            };
            builder.variables.insert(
                v.key.clone(),
                IndexVariable {
                    key: v.key.clone(),
                    declared: true,
                    ty: Some(v.ty.name()),
                    values,
                    reads: Vec::new(),
                    writes: Vec::new(),
                },
            );
        }
        let catalog = &project.catalog;
        for name in &catalog.backgrounds {
            builder.asset(BACKGROUND, None, name);
        }
        for (character, expressions) in &catalog.characters {
            for name in expressions {
                builder.asset(EXPRESSION, Some(character), name);
            }
        }
        for name in &catalog.bgm {
            builder.asset(BGM, None, name);
        }
        for name in &catalog.se {
            builder.asset(SE, None, name);
        }
        builder
    }

    fn location(&self, span: Span) -> Location {
        Location {
            scene: self.scene.clone(),
            file: self.file.clone(),
            span,
        }
    }

    /// 素材の項目。無ければ作る（カタログにあるかも決める）。
    fn asset(
        &mut self,
        (kind, order): (&'static str, u8),
        character: Option<&str>,
        name: &str,
    ) -> &mut IndexAsset {
        let catalog = &self.project.catalog;
        let registered = match kind {
            "background" => catalog.backgrounds.contains(name),
            "expression" => character
                .and_then(|c| catalog.characters.get(c))
                .is_some_and(|e| e.contains(name)),
            "bgm" => catalog.bgm.contains(name),
            _ => catalog.se.contains(name),
        };
        let key = (
            order,
            character.unwrap_or_default().to_owned(),
            name.to_owned(),
        );
        self.assets.entry(key).or_insert_with(|| IndexAsset {
            kind,
            character: character.map(str::to_owned),
            name: name.to_owned(),
            registered,
            uses: Vec::new(),
        })
    }

    fn use_asset(
        &mut self,
        kind: (&'static str, u8),
        character: Option<&str>,
        name: &str,
        span: Span,
    ) {
        let location = self.location(span);
        self.asset(kind, character, name).uses.push(location);
    }

    fn use_character(&mut self, kind: &'static str, id: &str, span: Span) {
        let location = self.location(span);
        if let Some(c) = self.characters.get_mut(id) {
            c.uses.push(CharacterUse { kind, location });
        }
    }

    fn variable(&mut self, key: &str) -> &mut IndexVariable {
        self.variables
            .entry(key.to_owned())
            .or_insert_with(|| IndexVariable {
                key: key.to_owned(),
                declared: false,
                ty: None,
                values: Vec::new(),
                reads: Vec::new(),
                writes: Vec::new(),
            })
    }

    fn read_expr(&mut self, expr: &IrExpr, span: Span) {
        match expr {
            IrExpr::Var { name } => {
                let location = self.location(span);
                let reads = &mut self.variable(name).reads;
                // 同じ行で同じ変数を何度読んでも、位置は1つにする。
                if reads.last() != Some(&location) {
                    reads.push(location);
                }
            }
            IrExpr::Unary { operand, .. } => self.read_expr(operand, span),
            IrExpr::Binary { lhs, rhs, .. } => {
                self.read_expr(lhs, span);
                self.read_expr(rhs, span);
            }
            IrExpr::Int { .. } | IrExpr::String { .. } | IrExpr::Bool { .. } => {}
        }
    }

    fn text(&mut self, text: &[IrInline], span: Span) {
        for inline in text {
            match inline {
                IrInline::Se { name } => self.use_asset(SE, None, name, span),
                IrInline::Interpolation {
                    expr: Some(expr), ..
                } => self.read_expr(expr, span),
                _ => {}
            }
        }
    }

    fn line_id(&mut self, line_id: Option<&String>, span: Span) {
        if let Some(id) = line_id {
            let location = self.location(span);
            self.line_ids.push(IndexLineId {
                id: id.clone(),
                location,
            });
        }
    }

    fn ops(&mut self, ops: &[Op]) {
        for op in ops {
            self.op(op);
        }
    }

    fn op(&mut self, op: &Op) {
        match op {
            Op::Say {
                span,
                line_id,
                speaker,
                text,
                ..
            } => {
                self.line_id(line_id.as_ref(), *span);
                self.use_character("speaker", speaker, *span);
                self.text(text, *span);
            }
            Op::Narrate {
                span,
                line_id,
                text,
            } => {
                self.line_id(line_id.as_ref(), *span);
                self.text(text, *span);
            }
            Op::Bg { span, name, .. } => self.use_asset(BACKGROUND, None, name, *span),
            Op::Show {
                span,
                character,
                expression,
                ..
            } => {
                self.use_character("show", character, *span);
                self.use_asset(EXPRESSION, Some(character), expression, *span);
            }
            Op::Hide {
                span, character, ..
            } => self.use_character("hide", character, *span),
            Op::Bgm { span, name } => self.use_asset(BGM, None, name, *span),
            Op::Se { span, name } => self.use_asset(SE, None, name, *span),
            Op::Set {
                span, var, value, ..
            } => {
                let location = self.location(*span);
                self.variable(var).writes.push(location);
                self.read_expr(value, *span);
            }
            Op::Choice {
                span,
                line_id,
                prompt,
                options,
            } => {
                self.line_id(line_id.as_ref(), *span);
                if let Some(prompt) = prompt {
                    self.text(prompt, *span);
                }
                for option in options {
                    self.line_id(option.line_id.as_ref(), option.span);
                    self.text(&option.text, option.span);
                    if let Some(cond) = &option.cond {
                        self.read_expr(cond, option.span);
                    }
                    self.ops(&option.body);
                }
            }
            Op::If {
                branches,
                else_body,
                ..
            } => {
                for branch in branches {
                    if let Some(cond) = &branch.cond {
                        self.read_expr(cond, branch.span);
                    }
                    self.ops(&branch.body);
                }
                if let Some(body) = else_body {
                    self.ops(body);
                }
            }
            Op::Break { .. }
            | Op::BgmStop { .. }
            | Op::Fx { .. }
            | Op::Wait { .. }
            | Op::Jump { .. }
            | Op::Call { .. }
            | Op::Return { .. }
            | Op::Ending { .. }
            | Op::Raw { .. }
            | Op::UnknownDirective { .. } => {}
        }
    }
}
