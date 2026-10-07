//! IR（中間表現）：シーンごとの命令列と、シーン間の辺（仕様書17節）。
//!
//! プレビュー、検証、エクスポート、AIの文脈の共通の入力にする。serdeでJSONにし、
//! フィールド名は `snake_case`、命令・文中の記法・式は `kind` で種類を示す（D036）。

use std::collections::BTreeMap;

use serde::Serialize;

use crate::ast::{
    Choice, ChoiceOption, Dialogue, DirectiveKind, IfBlock, LineIdSlot, MetaValue, Scene, Stmt,
};
use crate::diag::Span;
use crate::expr::{Expr, ExprKind, SourceExpr, UnaryOp};
use crate::inline::{Inline, Text};
use crate::project::Project;
use crate::sema::{self, FileDiagnostic, ParsedFile, Sema, Speaker};
use crate::syntax::parse;

/// IRの形のバージョン。形を変えたら上げ、仕様書17節も更新する。
pub const IR_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Ir {
    pub ir_version: u32,
    pub start_scene: Option<String>,
    pub scenes: Vec<IrScene>,
    pub edges: Vec<Edge>,
    pub diagnostics: Vec<IrDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IrScene {
    /// シーンID。IDの無いシーンは `_anon_<ファイル名>_<番号>`。
    pub id: String,
    pub title: String,
    pub file: String,
    /// 見出しの行。
    pub span: Span,
    pub meta: IrMeta,
    /// 本文に台詞・地の文が1つも無いシーン。
    pub is_stub: bool,
    pub ops: Vec<Op>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct IrMeta {
    pub summary: Option<String>,
    pub status: Option<String>,
    pub route: Option<String>,
    pub tags: Vec<String>,
    /// 知らないキー（I109）。値は書かれた文字列のまま。
    pub extra: BTreeMap<String, String>,
}

/// 命令。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Op {
    Say {
        span: Span,
        line_id: Option<String>,
        /// キャラクターID。
        speaker: String,
        display_name: Option<String>,
        style: String,
        text: Vec<IrInline>,
    },
    Narrate {
        span: Span,
        line_id: Option<String>,
        text: Vec<IrInline>,
    },
    /// 段落の区切り。
    Break {
        span: Span,
    },
    Bg {
        span: Span,
        name: String,
        transition: Option<String>,
    },
    Show {
        span: Span,
        /// キャラクターID。解決できなければ書かれた名前（W404）。
        character: String,
        expression: String,
        position: Option<&'static str>,
        transition: Option<String>,
    },
    Hide {
        span: Span,
        character: String,
        transition: Option<String>,
    },
    Bgm {
        span: Span,
        name: String,
    },
    BgmStop {
        span: Span,
    },
    Se {
        span: Span,
        name: String,
    },
    Fx {
        span: Span,
        name: String,
        seconds: Option<f64>,
    },
    Wait {
        span: Span,
        seconds: f64,
    },
    Set {
        span: Span,
        /// そろえた変数名（`akari.好感度`）。
        var: String,
        op: &'static str,
        value: IrExpr,
    },
    Choice {
        span: Span,
        line_id: Option<String>,
        prompt: Option<Vec<IrInline>>,
        options: Vec<IrOption>,
    },
    If {
        span: Span,
        branches: Vec<IrBranch>,
        #[serde(rename = "else")]
        else_body: Option<Vec<Op>>,
    },
    Jump {
        span: Span,
        target: String,
    },
    Call {
        span: Span,
        target: String,
    },
    Return {
        span: Span,
    },
    Ending {
        span: Span,
        /// 形が合わなければ `None`（E111）。
        id: Option<String>,
    },
    Raw {
        span: Span,
        engine: String,
        lines: Vec<String>,
    },
    /// 知らないディレクティブ（W401）。
    UnknownDirective {
        span: Span,
        name: String,
        args: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IrOption {
    pub span: Span,
    pub line_id: Option<String>,
    pub text: Vec<IrInline>,
    pub cond_text: Option<String>,
    /// 構文が崩れていれば `None`（E601）。
    pub cond: Option<IrExpr>,
    /// 中身を実行してから飛ぶ先。
    pub jump: Option<String>,
    pub body: Vec<Op>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IrBranch {
    pub span: Span,
    pub cond_text: String,
    pub cond: Option<IrExpr>,
    pub body: Vec<Op>,
}

/// 文中の記法。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IrInline {
    Text {
        text: String,
    },
    Ruby {
        base: String,
        reading: String,
    },
    Emphasis {
        text: String,
    },
    Wait {
        seconds: f64,
    },
    PageBreak,
    Se {
        name: String,
    },
    Speed {
        speed: &'static str,
    },
    Interpolation {
        expr_text: String,
        expr: Option<IrExpr>,
    },
    /// 複数行の台詞の、行の境目（D036）。
    LineBreak,
}

/// 式。変数名はそろえてある。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IrExpr {
    Int {
        value: i64,
    },
    String {
        value: String,
    },
    Bool {
        value: bool,
    },
    Var {
        name: String,
    },
    Unary {
        op: &'static str,
        operand: Box<IrExpr>,
    },
    Binary {
        op: &'static str,
        lhs: Box<IrExpr>,
        rhs: Box<IrExpr>,
    },
}

/// シーン間の辺。ジャンプと呼び出しごとに1本。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Edge {
    pub from: String,
    /// 存在しないシーン（E501）でも、書かれたとおりに出す。
    pub to: String,
    /// `jump` か `call`。
    pub kind: &'static str,
    /// いちばん内側の文脈：`direct`・`choice`・`if`・`else`。
    pub via: &'static str,
    /// 選択肢の表示文（`via` が `choice` のとき）。
    pub label: Option<String>,
    /// 条件の文字列（選択肢の条件、`@if`・`@elif` の式）。
    pub cond_text: Option<String>,
    /// ジャンプや呼び出しを書いた位置（`from` のシーンのファイル）。
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrDiagnostic {
    pub code: &'static str,
    pub severity: &'static str,
    pub file: String,
    pub span: Span,
    pub message: String,
}

/// プロジェクトを解析して、IRを作る。診断はIRの `diagnostics` に入る。
pub fn analyze(project: &Project) -> Ir {
    let options = project.parse_options();
    let files: Vec<ParsedFile<'_>> = project
        .files
        .iter()
        .map(|file| ParsedFile {
            path: &file.path,
            parsed: parse(&file.text, &options),
        })
        .collect();

    let mut diags: Vec<FileDiagnostic> = Vec::new();
    for (index, file) in files.iter().enumerate() {
        for diag in &file.parsed.diagnostics {
            diags.push(FileDiagnostic {
                file: index,
                diag: diag.clone(),
            });
        }
    }
    let (sema, sema_diags) = sema::check(project, &files);
    diags.extend(sema_diags);
    diags.sort_by_key(|d| (d.file, d.diag.span.start));

    let mut builder = Builder {
        sema: &sema,
        from: String::new(),
        edges: Vec::new(),
    };
    let mut scenes = Vec::new();
    for file in &files {
        let stem = file
            .path
            .rsplit('/')
            .next()
            .unwrap_or(file.path)
            .trim_end_matches(".scn");
        let mut anonymous = 0;
        for scene in &file.parsed.chapter.scenes {
            let id = match &scene.id {
                Some(id) => id.value.clone(),
                None => {
                    anonymous += 1;
                    format!("_anon_{stem}_{anonymous}")
                }
            };
            scenes.push(builder.scene(scene, id, file.path));
        }
    }

    Ir {
        ir_version: IR_VERSION,
        start_scene: project.settings.start_scene.clone(),
        scenes,
        edges: builder.edges,
        // 読み込みの診断（設定ファイル。E801〜W804）を先に、本文の診断をファイルと位置の順に並べる。
        diagnostics: project
            .load_diagnostics
            .iter()
            .map(|d| IrDiagnostic {
                code: d.diag.code.as_str(),
                severity: d.diag.severity().as_str(),
                file: d.file.clone(),
                span: d.diag.span,
                message: d.diag.message.clone(),
            })
            .chain(diags.into_iter().map(|d| {
                IrDiagnostic {
                    code: d.diag.code.as_str(),
                    severity: d.diag.severity().as_str(),
                    file: files
                        .get(d.file)
                        .map(|f| f.path.to_owned())
                        .unwrap_or_default(),
                    span: d.diag.span,
                    message: d.diag.message,
                }
            }))
            .collect(),
    }
}

fn line_id(slot: &LineIdSlot) -> Option<String> {
    match slot {
        LineIdSlot::Valid(id) => Some(id.value.clone()),
        LineIdSlot::Absent | LineIdSlot::Malformed => None,
    }
}

/// 辺の文脈。
#[derive(Clone)]
struct Context {
    via: &'static str,
    label: Option<String>,
    cond_text: Option<String>,
}

impl Context {
    fn direct() -> Self {
        Self {
            via: "direct",
            label: None,
            cond_text: None,
        }
    }
}

struct Builder<'s, 'p> {
    sema: &'s Sema<'p>,
    /// 今のシーンのID（辺の `from`）。
    from: String,
    edges: Vec<Edge>,
}

impl Builder<'_, '_> {
    fn scene(&mut self, scene: &Scene, id: String, file: &str) -> IrScene {
        self.from.clone_from(&id);
        let ops = self.ops(&scene.body, &Context::direct());
        IrScene {
            id,
            title: scene.title.clone(),
            file: file.to_owned(),
            span: scene.span,
            meta: meta(scene),
            is_stub: !has_text(&ops),
            ops,
        }
    }

    fn edge(&mut self, to: &str, kind: &'static str, ctx: &Context, span: Span) {
        self.edges.push(Edge {
            from: self.from.clone(),
            to: to.to_owned(),
            kind,
            via: ctx.via,
            label: ctx.label.clone(),
            cond_text: ctx.cond_text.clone(),
            span,
        });
    }

    fn ops(&mut self, stmts: &[Stmt], ctx: &Context) -> Vec<Op> {
        stmts.iter().filter_map(|stmt| self.op(stmt, ctx)).collect()
    }

    fn op(&mut self, stmt: &Stmt, ctx: &Context) -> Option<Op> {
        Some(match stmt {
            Stmt::Dialogue(dialogue) => self.dialogue(dialogue),
            Stmt::Narration(narration) => Op::Narrate {
                span: narration.span,
                line_id: line_id(&narration.line_id),
                text: self.text(&narration.text),
            },
            Stmt::Break(span) => Op::Break { span: *span },
            Stmt::Jump(jump) => {
                self.edge(&jump.target.value, "jump", ctx, jump.span);
                Op::Jump {
                    span: jump.span,
                    target: jump.target.value.clone(),
                }
            }
            Stmt::Directive(directive) => {
                let span = directive.span;
                match &directive.kind {
                    DirectiveKind::Bg { name, transition } => Op::Bg {
                        span,
                        name: name.value.clone(),
                        transition: transition.as_ref().map(|t| t.value.clone()),
                    },
                    DirectiveKind::Show {
                        character,
                        expression,
                        position,
                        transition,
                    } => Op::Show {
                        span,
                        character: self.character(&character.value),
                        expression: expression.value.clone(),
                        position: position.map(|p| p.as_str()),
                        transition: transition.as_ref().map(|t| t.value.clone()),
                    },
                    DirectiveKind::Hide {
                        character,
                        transition,
                    } => Op::Hide {
                        span,
                        character: self.character(&character.value),
                        transition: transition.as_ref().map(|t| t.value.clone()),
                    },
                    DirectiveKind::Bgm { name } => Op::Bgm {
                        span,
                        name: name.value.clone(),
                    },
                    DirectiveKind::BgmStop => Op::BgmStop { span },
                    DirectiveKind::Se { name } => Op::Se {
                        span,
                        name: name.value.clone(),
                    },
                    DirectiveKind::Fx { name, seconds } => Op::Fx {
                        span,
                        name: name.value.clone(),
                        seconds: *seconds,
                    },
                    DirectiveKind::Wait { seconds } => Op::Wait {
                        span,
                        seconds: *seconds,
                    },
                    DirectiveKind::Set { assignment, .. } => {
                        let assignment = assignment.as_ref()?;
                        Op::Set {
                            span,
                            var: self.sema.var_key(&assignment.target),
                            op: assignment.op.as_str(),
                            value: self.expr(&assignment.value),
                        }
                    }
                    DirectiveKind::Call { target } => {
                        self.edge(&target.value, "call", ctx, span);
                        Op::Call {
                            span,
                            target: target.value.clone(),
                        }
                    }
                    DirectiveKind::Return => Op::Return { span },
                    DirectiveKind::Ending { id } => Op::Ending {
                        span,
                        id: id.as_ref().map(|i| i.value.clone()),
                    },
                    DirectiveKind::Unknown { args } => Op::UnknownDirective {
                        span,
                        name: directive.name.clone(),
                        args: args.iter().map(|a| a.value.clone()).collect(),
                    },
                }
            }
            Stmt::Choice(choice) => self.choice(choice),
            Stmt::If(block) => self.if_block(block),
            Stmt::Raw(raw) => Op::Raw {
                span: raw.span,
                engine: raw.engine.clone(),
                lines: raw.lines.clone(),
            },
        })
    }

    fn character(&self, name: &str) -> String {
        match self.sema.resolve_character(name) {
            sema::Resolution::Found(id) => id.to_owned(),
            sema::Resolution::NotFound | sema::Resolution::Ambiguous(_) => name.to_owned(),
        }
    }

    fn dialogue(&mut self, dialogue: &Dialogue) -> Op {
        let mut body = Vec::new();
        for (i, line) in dialogue.lines.iter().enumerate() {
            if i > 0 {
                body.push(IrInline::LineBreak);
            }
            body.extend(self.text(line));
        }
        match self.sema.speaker(dialogue) {
            Speaker::Character(id) => {
                // 閉じ括弧の後ろのインライン命令は、本文の末尾に付ける。間の空白は捨てる（D041）。
                if let Some(trailing) = &dialogue.trailing {
                    body.extend(
                        self.text(trailing)
                            .into_iter()
                            .filter(|inline| !matches!(inline, IrInline::Text { .. })),
                    );
                }
                Op::Say {
                    span: dialogue.span,
                    line_id: line_id(&dialogue.line_id),
                    speaker: id.to_owned(),
                    display_name: dialogue.display_name.as_ref().map(|n| n.value.clone()),
                    style: dialogue.style.clone(),
                    text: body,
                }
            }
            Speaker::Narration { .. } => {
                // 書かれたとおりに並べ直して、地の文にする（仕様書6節）。
                let mut head = dialogue.speaker.value.clone();
                if let Some(name) = &dialogue.display_name {
                    head.push('（');
                    head.push_str(&name.value);
                    head.push('）');
                }
                head.push(dialogue.open);
                let mut text = vec![IrInline::Text { text: head }];
                text.extend(body);
                if dialogue.closed {
                    text.push(IrInline::Text {
                        text: dialogue.close.to_string(),
                    });
                }
                if let Some(trailing) = &dialogue.trailing {
                    text.extend(self.text(trailing));
                }
                Op::Narrate {
                    span: dialogue.span,
                    line_id: line_id(&dialogue.line_id),
                    text: merge_text(text),
                }
            }
        }
    }

    fn choice(&mut self, choice: &Choice) -> Op {
        let options = choice
            .options
            .iter()
            .map(|option| self.option(option))
            .collect();
        Op::Choice {
            span: choice.span,
            line_id: line_id(&choice.line_id),
            prompt: choice.prompt.as_ref().map(|p| self.text(p)),
            options,
        }
    }

    fn option(&mut self, option: &ChoiceOption) -> IrOption {
        let ctx = Context {
            via: "choice",
            label: Some(plain_text(&option.text)),
            cond_text: option.cond.as_ref().map(|c| c.text.clone()),
        };
        if let Some(jump) = &option.jump {
            self.edge(&jump.value, "jump", &ctx, option.span);
        }
        IrOption {
            span: option.span,
            line_id: line_id(&option.line_id),
            text: self.text(&option.text),
            cond_text: option.cond.as_ref().map(|c| c.text.clone()),
            cond: option.cond.as_ref().and_then(|c| self.source_expr(c)),
            jump: option.jump.as_ref().map(|j| j.value.clone()),
            body: self.ops(&option.body, &ctx),
        }
    }

    fn if_block(&mut self, block: &IfBlock) -> Op {
        let branches = block
            .branches
            .iter()
            .map(|branch| {
                let ctx = Context {
                    via: "if",
                    label: None,
                    cond_text: Some(branch.cond.text.clone()),
                };
                IrBranch {
                    span: branch.span,
                    cond_text: branch.cond.text.clone(),
                    cond: self.source_expr(&branch.cond),
                    body: self.ops(&branch.body, &ctx),
                }
            })
            .collect();
        let else_ctx = Context {
            via: "else",
            label: None,
            cond_text: None,
        };
        let else_body = block
            .else_branch
            .as_ref()
            .map(|branch| self.ops(&branch.body, &else_ctx));
        Op::If {
            span: block.span,
            branches,
            else_body,
        }
    }

    fn text(&self, text: &Text) -> Vec<IrInline> {
        text.parts
            .iter()
            .map(|part| match part {
                Inline::Text { text, .. } => IrInline::Text { text: text.clone() },
                Inline::Ruby { base, reading, .. } => IrInline::Ruby {
                    base: base.clone(),
                    reading: reading.clone(),
                },
                Inline::Emphasis { text, .. } => IrInline::Emphasis { text: text.clone() },
                Inline::Wait { seconds, .. } => IrInline::Wait { seconds: *seconds },
                Inline::PageBreak { .. } => IrInline::PageBreak,
                Inline::Se { name, .. } => IrInline::Se { name: name.clone() },
                Inline::Speed { speed, .. } => IrInline::Speed {
                    speed: speed.as_str(),
                },
                Inline::Interpolation { expr, .. } => IrInline::Interpolation {
                    expr_text: expr.text.clone(),
                    expr: self.source_expr(expr),
                },
            })
            .collect()
    }

    fn source_expr(&self, source: &SourceExpr) -> Option<IrExpr> {
        source.expr.as_ref().map(|e| self.expr(e))
    }

    fn expr(&self, expr: &Expr) -> IrExpr {
        match &expr.kind {
            ExprKind::Int(value) => IrExpr::Int { value: *value },
            ExprKind::Str(value) => IrExpr::String {
                value: value.clone(),
            },
            ExprKind::Bool(value) => IrExpr::Bool { value: *value },
            ExprKind::Var(var) => IrExpr::Var {
                name: self.sema.var_key(var),
            },
            ExprKind::Unary { op, operand } => IrExpr::Unary {
                op: match op {
                    UnaryOp::Neg => "-",
                    UnaryOp::Not => "not",
                },
                operand: Box::new(self.expr(operand)),
            },
            ExprKind::Binary { op, lhs, rhs } => IrExpr::Binary {
                op: op.as_str(),
                lhs: Box::new(self.expr(lhs)),
                rhs: Box::new(self.expr(rhs)),
            },
        }
    }
}

/// 続いた文をまとめる。
fn merge_text(parts: Vec<IrInline>) -> Vec<IrInline> {
    let mut merged: Vec<IrInline> = Vec::with_capacity(parts.len());
    for part in parts {
        match (merged.last_mut(), part) {
            (Some(IrInline::Text { text: prev }), IrInline::Text { text }) => prev.push_str(&text),
            (_, part) => merged.push(part),
        }
    }
    merged
}

/// 記法を除いた、表示される文字だけ（辺のラベル）。ルビは親文字を使う。
fn plain_text(text: &Text) -> String {
    let mut out = String::new();
    for part in &text.parts {
        match part {
            Inline::Text { text, .. } | Inline::Emphasis { text, .. } => out.push_str(text),
            Inline::Ruby { base, .. } => out.push_str(base),
            Inline::Interpolation { expr, .. } => {
                out.push_str("{=");
                out.push_str(&expr.text);
                out.push('}');
            }
            Inline::Wait { .. }
            | Inline::PageBreak { .. }
            | Inline::Se { .. }
            | Inline::Speed { .. } => {}
        }
    }
    out
}

/// IRのメタ情報（D036）。
fn meta(scene: &Scene) -> IrMeta {
    let mut meta = IrMeta::default();
    let Some(scene_meta) = &scene.meta else {
        return meta;
    };
    for entry in &scene_meta.entries {
        let raw = entry.raw.clone();
        match (entry.key.value.as_str(), &entry.value.value) {
            ("summary", _) => meta.summary = Some(raw),
            ("route", _) => meta.route = Some(raw),
            ("status", MetaValue::Text(text))
                if matches!(text.as_str(), "draft" | "writing" | "done" | "ai-draft") =>
            {
                meta.status = Some(text.clone());
            }
            ("status", _) => meta.status = None,
            ("tags", MetaValue::List(items)) => meta.tags.clone_from(items),
            ("tags", MetaValue::Text(text)) => {
                meta.tags = if text.is_empty() {
                    Vec::new()
                } else {
                    vec![text.clone()]
                };
            }
            (key, _) => {
                meta.extra.insert(key.to_owned(), raw);
            }
        }
    }
    meta
}

/// 台詞か地の文が、どこかにあるか。
fn has_text(ops: &[Op]) -> bool {
    ops.iter().any(|op| match op {
        Op::Say { .. } | Op::Narrate { .. } => true,
        Op::Choice { options, .. } => options.iter().any(|o| has_text(&o.body)),
        Op::If {
            branches,
            else_body,
            ..
        } => {
            branches.iter().any(|b| has_text(&b.body))
                || else_body.as_ref().is_some_and(|body| has_text(body))
        }
        _ => false,
    })
}
