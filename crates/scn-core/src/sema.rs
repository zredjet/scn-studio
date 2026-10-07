//! 意味解析：話者の解決、変数の名前空間と型、シーンの参照、行IDの重複、素材の意味名。
//!
//! 構文解析の結果（AST）と、プロジェクトの設定DB・変数・素材カタログを照らし合わせて
//! 診断を出す。IRを作るときにも、ここの照合（話者、変数名）をそのまま使う。

use std::collections::{HashMap, HashSet};

use crate::ast::{Choice, Dialogue, DirectiveKind, IfBlock, LineIdSlot, Scene, Stmt};
use crate::diag::{Code, Diagnostic, Span};
use crate::expr::{AssignOp, Assignment, BinaryOp, Expr, ExprKind, SourceExpr, UnaryOp, VarRef};
use crate::inline::{Inline, Text};
use crate::project::{Project, VarType};
use crate::syntax::Parsed;

/// 構文解析した1つのファイル。
pub(crate) struct ParsedFile<'a> {
    pub(crate) path: &'a str,
    pub(crate) parsed: Parsed,
}

/// どのファイルの診断か。`file` は `ParsedFile` の並びの番号。
pub(crate) struct FileDiagnostic {
    pub(crate) file: usize,
    pub(crate) diag: Diagnostic,
}

/// キャラクターの照合の結果。
pub(crate) enum Resolution<'p> {
    Found(&'p str),
    NotFound,
    /// 同じ段で2人以上に当てはまった（解決できない）。
    Ambiguous(Vec<&'p str>),
}

/// 台詞候補を、台詞と地の文のどちらにするか（仕様書6節）。
pub(crate) enum Speaker<'p> {
    Character(&'p str),
    /// 地の文。`warn` なら W202 を出す。
    Narration {
        warn: bool,
    },
}

/// 式の型。`Unknown` は宣言されていない変数を含む式で、型の検査をしない。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ty<'p> {
    Bool,
    Int,
    Str,
    Enum(&'p [String]),
    Unknown,
}

impl Ty<'_> {
    fn name(self) -> &'static str {
        match self {
            Ty::Bool => "bool",
            Ty::Int => "int",
            Ty::Str => "string",
            Ty::Enum(_) => "enum",
            Ty::Unknown => "不明",
        }
    }
}

pub(crate) struct Sema<'p> {
    project: &'p Project,
    vars: HashMap<&'p str, &'p VarType>,
    /// プロジェクトにあるシーンID。
    scenes: HashSet<String>,
}

impl<'p> Sema<'p> {
    /// キャラクターを、ID、名前、呼び名の順に照合する。同じ段で2人以上に当てはまれば解決できない。
    pub(crate) fn resolve_character(&self, name: &str) -> Resolution<'p> {
        let characters = &self.project.characters;
        let stages: [&dyn Fn(&crate::project::Character) -> bool; 3] =
            [&|c| c.id == name, &|c| c.name == name, &|c| {
                c.aliases.iter().any(|a| a == name)
            }];
        for matches in stages {
            let found: Vec<&'p str> = characters
                .iter()
                .filter(|c| matches(c))
                .map(|c| c.id.as_str())
                .collect();
            match found.as_slice() {
                [] => continue,
                [id] => return Resolution::Found(id),
                _ => return Resolution::Ambiguous(found),
            }
        }
        Resolution::NotFound
    }

    fn character_id(&self, name: &str) -> Option<&'p str> {
        match self.resolve_character(name) {
            Resolution::Found(id) => Some(id),
            _ => None,
        }
    }

    pub(crate) fn speaker(&self, dialogue: &Dialogue) -> Speaker<'p> {
        if dialogue.has_trailing_text() {
            return Speaker::Narration { warn: false };
        }
        match self.character_id(&dialogue.speaker.value) {
            Some(id) => Speaker::Character(id),
            None => Speaker::Narration {
                warn: dialogue.closed,
            },
        }
    }

    /// 変数名をそろえる（仕様書14節）。名前空間が無ければ `flag.` を補い、
    /// キャラクターのID・名前・呼び名ならキャラクターIDにする。
    pub(crate) fn var_key(&self, var: &VarRef) -> String {
        let namespace = match &var.namespace {
            None => "flag",
            Some(ns) => self.character_id(ns).unwrap_or(ns),
        };
        format!("{namespace}.{}", var.name)
    }

    pub(crate) fn scene_exists(&self, id: &str) -> bool {
        self.scenes.contains(id)
    }
}

/// 意味解析を行い、照合に使う `Sema` と診断を返す。
pub(crate) fn check<'p>(
    project: &'p Project,
    files: &[ParsedFile<'_>],
) -> (Sema<'p>, Vec<FileDiagnostic>) {
    let mut diags = Vec::new();
    let mut push = |file: usize, code: Code, span: Span, message: String| {
        diags.push(FileDiagnostic {
            file,
            diag: Diagnostic::new(code, span, message),
        });
    };

    // シーンIDの重複（E108）。ファイルはパスの辞書順、ファイルの中は行の順。
    let mut first_scene: HashMap<&str, &str> = HashMap::new();
    for (index, file) in files.iter().enumerate() {
        for scene in &file.parsed.chapter.scenes {
            let Some(id) = &scene.id else { continue };
            match first_scene.get(id.value.as_str()) {
                Some(first) => push(
                    index,
                    Code::E108,
                    id.span,
                    format!(
                        "シーンID {} が重複しています（先に {first} にあります）",
                        id.value
                    ),
                ),
                None => {
                    first_scene.insert(&id.value, file.path);
                }
            }
        }
    }

    // 行IDの重複（E301）。
    let mut first_line_id: HashMap<String, &str> = HashMap::new();
    for (index, file) in files.iter().enumerate() {
        let mut ids = Vec::new();
        for scene in &file.parsed.chapter.scenes {
            collect_line_ids(&scene.body, &mut ids);
        }
        for (value, span) in ids {
            match first_line_id.get(&value) {
                Some(first) => push(
                    index,
                    Code::E301,
                    span,
                    format!("行ID ^{value} が重複しています（先に {first} にあります）"),
                ),
                None => {
                    first_line_id.insert(value, file.path);
                }
            }
        }
    }

    let sema = Sema {
        project,
        vars: project
            .variables
            .iter()
            .map(|v| (v.key.as_str(), &v.ty))
            .collect(),
        scenes: first_scene.keys().map(|id| (*id).to_owned()).collect(),
    };
    let mut checker = Checker {
        sema: &sema,
        file: 0,
        diags: Vec::new(),
        in_unreachable: false,
    };
    for (index, file) in files.iter().enumerate() {
        checker.file = index;
        for scene in &file.parsed.chapter.scenes {
            checker.scene(scene);
        }
    }
    diags.append(&mut checker.diags);
    (sema, diags)
}

/// 行IDを、書かれた順に集める。
fn collect_line_ids(stmts: &[Stmt], out: &mut Vec<(String, Span)>) {
    fn push(slot: &LineIdSlot, out: &mut Vec<(String, Span)>) {
        if let LineIdSlot::Valid(id) = slot {
            out.push((id.value.clone(), id.span));
        }
    }
    for stmt in stmts {
        match stmt {
            Stmt::Dialogue(dialogue) => push(&dialogue.line_id, out),
            Stmt::Narration(narration) => push(&narration.line_id, out),
            Stmt::Choice(choice) => {
                push(&choice.line_id, out);
                for option in &choice.options {
                    push(&option.line_id, out);
                    collect_line_ids(&option.body, out);
                }
            }
            Stmt::If(block) => {
                for branch in &block.branches {
                    collect_line_ids(&branch.body, out);
                }
                if let Some(branch) = &block.else_branch {
                    collect_line_ids(&branch.body, out);
                }
            }
            Stmt::Break(_) | Stmt::Directive(_) | Stmt::Jump(_) | Stmt::Raw(_) => {}
        }
    }
}

/// 要素の位置。ブロックは開きの行。
pub(crate) fn stmt_span(stmt: &Stmt) -> Span {
    match stmt {
        Stmt::Dialogue(d) => d.span,
        Stmt::Narration(n) => n.span,
        Stmt::Break(span) => *span,
        Stmt::Directive(d) => d.span,
        Stmt::Jump(j) => j.span,
        Stmt::Choice(c) => c.span,
        Stmt::If(b) => b.span,
        Stmt::Raw(r) => r.span,
    }
}

/// ジャンプ、`@エンディング`、`@戻る`。この後ろの同じ枝の要素は実行されない（仕様書13節）。
fn is_terminator(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Jump(_) => true,
        Stmt::Directive(d) => {
            matches!(d.kind, DirectiveKind::Ending { .. } | DirectiveKind::Return)
        }
        _ => false,
    }
}

struct Checker<'s, 'p> {
    sema: &'s Sema<'p>,
    file: usize,
    diags: Vec<FileDiagnostic>,
    /// W502 を出したブロックの中を調べているか。
    in_unreachable: bool,
}

impl<'p> Checker<'_, 'p> {
    fn diag(&mut self, code: Code, span: Span, message: String) {
        self.diags.push(FileDiagnostic {
            file: self.file,
            diag: Diagnostic::new(code, span, message),
        });
    }

    fn scene(&mut self, scene: &Scene) {
        self.stmts(&scene.body);
    }

    /// 1つの枝。ジャンプなどの後ろの要素に W502 を出す（空行の区切りは除く）。
    /// ブロックが実行されないと分かっているときは、中の行に W502 を重ねない（D036）。
    fn stmts(&mut self, stmts: &[Stmt]) {
        let mut terminated = false;
        for stmt in stmts {
            let unreachable = terminated && !matches!(stmt, Stmt::Break(_));
            if unreachable && !self.in_unreachable {
                self.diag(
                    Code::W502,
                    stmt_span(stmt),
                    "ジャンプ・@エンディング・@戻る の後ろにあるので、実行されません".into(),
                );
            }
            let outer = self.in_unreachable;
            self.in_unreachable |= unreachable;
            self.stmt(stmt);
            self.in_unreachable = outer;
            terminated |= is_terminator(stmt);
        }
    }

    fn missing_line_id(&mut self, slot: &LineIdSlot, span: Span) {
        if matches!(slot, LineIdSlot::Absent) {
            self.diag(Code::I302, span, "行IDがありません".into());
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Dialogue(dialogue) => self.dialogue(dialogue),
            Stmt::Narration(narration) => {
                self.missing_line_id(&narration.line_id, narration.span);
                self.text(&narration.text);
            }
            Stmt::Break(_) | Stmt::Raw(_) => {}
            Stmt::Directive(directive) => self.directive(&directive.kind),
            Stmt::Jump(jump) => self.target(&jump.target.value, jump.target.span),
            Stmt::Choice(choice) => self.choice(choice),
            Stmt::If(block) => self.if_block(block),
        }
    }

    fn dialogue(&mut self, dialogue: &Dialogue) {
        if let Speaker::Narration { warn: true } = self.sema.speaker(dialogue) {
            let name = &dialogue.speaker.value;
            let message = match self.sema.resolve_character(name) {
                Resolution::Ambiguous(ids) => format!(
                    "話者 {name} は2人以上のキャラクター（{}）に当てはまります。地の文として扱います",
                    ids.join("、")
                ),
                _ => format!(
                    "話者 {name} を設定DBのキャラクターに解決できません。地の文として扱います"
                ),
            };
            self.diag(Code::W202, dialogue.speaker.span, message);
        }
        self.missing_line_id(&dialogue.line_id, dialogue.span);
        for line in &dialogue.lines {
            self.text(line);
        }
        if let Some(trailing) = &dialogue.trailing {
            self.text(trailing);
        }
    }

    fn text(&mut self, text: &Text) {
        for part in &text.parts {
            match part {
                Inline::Se { name, span } => {
                    if !self.sema.project.catalog.se.contains(name) {
                        self.diag(
                            Code::I403,
                            *span,
                            format!("素材カタログに無い効果音です：{name}（仮素材として扱います）"),
                        );
                    }
                }
                Inline::Interpolation { expr, .. } => {
                    if let Some(expr) = &expr.expr {
                        self.expr_type(expr);
                    }
                }
                _ => {}
            }
        }
    }

    fn target(&mut self, target: &str, span: Span) {
        if !self.sema.scene_exists(target) {
            self.diag(Code::E501, span, format!("シーン {target} がありません"));
        }
    }

    fn unresolved_character(&mut self, name: &str, span: Span) -> Option<&'p str> {
        match self.sema.resolve_character(name) {
            Resolution::Found(id) => Some(id),
            Resolution::NotFound => {
                self.diag(
                    Code::W404,
                    span,
                    format!("キャラ {name} を設定DBのキャラクターに解決できません"),
                );
                None
            }
            Resolution::Ambiguous(ids) => {
                self.diag(
                    Code::W404,
                    span,
                    format!(
                        "キャラ {name} は2人以上のキャラクター（{}）に当てはまります",
                        ids.join("、")
                    ),
                );
                None
            }
        }
    }

    fn directive(&mut self, kind: &DirectiveKind) {
        let catalog = &self.sema.project.catalog;
        match kind {
            DirectiveKind::Bg { name, .. } => {
                if !catalog.backgrounds.contains(&name.value) {
                    self.diag(
                        Code::I403,
                        name.span,
                        format!(
                            "素材カタログに無い背景です：{}（仮素材として扱います）",
                            name.value
                        ),
                    );
                }
            }
            DirectiveKind::Show {
                character,
                expression,
                ..
            } => {
                if let Some(id) = self.unresolved_character(&character.value, character.span) {
                    let known = catalog
                        .characters
                        .get(id)
                        .is_some_and(|e| e.contains(&expression.value));
                    if !known {
                        self.diag(
                            Code::I403,
                            expression.span,
                            format!(
                                "素材カタログに無い立ち絵の差分です：{} {}（仮素材として扱います）",
                                character.value, expression.value
                            ),
                        );
                    }
                }
            }
            DirectiveKind::Hide { character, .. } => {
                self.unresolved_character(&character.value, character.span);
            }
            DirectiveKind::Bgm { name } => {
                if !catalog.bgm.contains(&name.value) {
                    self.diag(
                        Code::I403,
                        name.span,
                        format!(
                            "素材カタログに無いBGMです：{}（仮素材として扱います）",
                            name.value
                        ),
                    );
                }
            }
            DirectiveKind::Se { name } => {
                if !catalog.se.contains(&name.value) {
                    self.diag(
                        Code::I403,
                        name.span,
                        format!(
                            "素材カタログに無い効果音です：{}（仮素材として扱います）",
                            name.value
                        ),
                    );
                }
            }
            DirectiveKind::Set {
                assignment: Some(assignment),
                ..
            } => self.assignment(assignment),
            DirectiveKind::Call { target } => self.target(&target.value, target.span),
            DirectiveKind::Set {
                assignment: None, ..
            }
            | DirectiveKind::BgmStop
            | DirectiveKind::Fx { .. }
            | DirectiveKind::Wait { .. }
            | DirectiveKind::Return
            | DirectiveKind::Ending { .. }
            | DirectiveKind::Unknown { .. } => {}
        }
    }

    fn choice(&mut self, choice: &Choice) {
        self.missing_line_id(&choice.line_id, choice.span);
        if let Some(prompt) = &choice.prompt {
            self.text(prompt);
        }
        for option in &choice.options {
            self.missing_line_id(&option.line_id, option.span);
            self.text(&option.text);
            if let Some(cond) = &option.cond {
                self.condition(cond);
            }
            if let Some(jump) = &option.jump {
                self.target(&jump.value, jump.span);
            }
            self.stmts(&option.body);
        }
    }

    fn if_block(&mut self, block: &IfBlock) {
        for branch in &block.branches {
            self.condition(&branch.cond);
            self.stmts(&branch.body);
        }
        if let Some(branch) = &block.else_branch {
            self.stmts(&branch.body);
        }
    }

    // ---- 変数と型（仕様書14節） ----

    fn condition(&mut self, cond: &SourceExpr) {
        let Some(expr) = &cond.expr else { return };
        let ty = self.expr_type(expr);
        if !matches!(ty, Ty::Bool | Ty::Unknown) {
            self.diag(
                Code::E603,
                expr.span,
                format!(
                    "条件は真偽（true・false）になる式で書きます（この式は {}）",
                    ty.name()
                ),
            );
        }
    }

    fn var_type(&mut self, var: &VarRef) -> Ty<'p> {
        let key = self.sema.var_key(var);
        match self.sema.vars.get(key.as_str()).copied() {
            Some(VarType::Bool) => Ty::Bool,
            Some(VarType::Int) => Ty::Int,
            Some(VarType::String) => Ty::Str,
            Some(VarType::Enum(values)) => Ty::Enum(values.as_slice()),
            None => {
                // 宣言を丸ごと読めないときは、宣言の無い変数を警告しない（E801 を埋もれさせない。D042）。
                if !self.sema.project.variables_unreadable {
                    self.diag(
                        Code::W602,
                        var.span,
                        format!("宣言されていない変数です：{key}（variables.yaml に宣言します）"),
                    );
                }
                Ty::Unknown
            }
        }
    }

    fn require(&mut self, ty: Ty<'p>, want: Ty<'p>, span: Span, what: &str) {
        if ty != want && ty != Ty::Unknown {
            self.diag(
                Code::E603,
                span,
                format!(
                    "{what}は {} の値にだけ使えます（この値は {}）",
                    want.name(),
                    ty.name()
                ),
            );
        }
    }

    /// 式の型を求め、途中の誤り（W602、E603）を出す。演算の結果の型は、演算子で決まる。
    fn expr_type(&mut self, expr: &Expr) -> Ty<'p> {
        match &expr.kind {
            ExprKind::Int(_) => Ty::Int,
            ExprKind::Str(_) => Ty::Str,
            ExprKind::Bool(_) => Ty::Bool,
            ExprKind::Var(var) => self.var_type(var),
            ExprKind::Unary { op, operand } => {
                let ty = self.expr_type(operand);
                match op {
                    UnaryOp::Neg => {
                        self.require(ty, Ty::Int, operand.span, "単項の - ");
                        Ty::Int
                    }
                    UnaryOp::Not => {
                        self.require(ty, Ty::Bool, operand.span, "not ");
                        Ty::Bool
                    }
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let lt = self.expr_type(lhs);
                let rt = self.expr_type(rhs);
                match op {
                    BinaryOp::And | BinaryOp::Or => {
                        let what = format!("{} ", op.as_str());
                        self.require(lt, Ty::Bool, lhs.span, &what);
                        self.require(rt, Ty::Bool, rhs.span, &what);
                        Ty::Bool
                    }
                    BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
                        let what = format!("{} ", op.as_str());
                        self.require(lt, Ty::Int, lhs.span, &what);
                        self.require(rt, Ty::Int, rhs.span, &what);
                        Ty::Int
                    }
                    BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                        let what = format!("{} ", op.as_str());
                        self.require(lt, Ty::Int, lhs.span, &what);
                        self.require(rt, Ty::Int, rhs.span, &what);
                        Ty::Bool
                    }
                    BinaryOp::Eq | BinaryOp::Ne => {
                        if let Err(message) = compatible(lt, lhs, rt, rhs) {
                            self.diag(Code::E603, expr.span, message);
                        }
                        Ty::Bool
                    }
                }
            }
        }
    }

    fn assignment(&mut self, assignment: &Assignment) {
        let key = self.sema.var_key(&assignment.target);
        let Some(declared) = self.sema.vars.get(key.as_str()).copied() else {
            if !self.sema.project.variables_unreadable {
                self.diag(
                    Code::W602,
                    assignment.target.span,
                    format!("宣言されていない変数です：{key}（variables.yaml に宣言します）"),
                );
            }
            // 代入先の型は分からないので、式の中だけを確かめる。
            self.expr_type(&assignment.value);
            return;
        };
        let target = match declared {
            VarType::Bool => Ty::Bool,
            VarType::Int => Ty::Int,
            VarType::String => Ty::Str,
            VarType::Enum(values) => Ty::Enum(values.as_slice()),
        };
        let value = self.expr_type(&assignment.value);
        match assignment.op {
            AssignOp::Add | AssignOp::Sub => {
                if target != Ty::Int {
                    self.diag(
                        Code::E604,
                        assignment.target.span,
                        format!(
                            "{} は整数の変数にだけ使えます（{key} は {}）",
                            assignment.op.as_str(),
                            target.name()
                        ),
                    );
                } else {
                    self.require(value, Ty::Int, assignment.value.span, "+= と -= ");
                }
            }
            AssignOp::Set => {
                if let Err(message) = assignable(target, value, &assignment.value) {
                    self.diag(
                        Code::E603,
                        assignment.value.span,
                        format!("{key} に代入できません：{message}"),
                    );
                }
            }
        }
    }
}

/// 文字列のリテラルなら、その値。
fn string_literal(expr: &Expr) -> Option<&str> {
    match &expr.kind {
        ExprKind::Str(value) => Some(value),
        _ => None,
    }
}

/// 列挙と、文字列のリテラルを照らす。
fn enum_value(values: &[String], expr: &Expr) -> Result<(), String> {
    match string_literal(expr) {
        Some(value) if values.iter().any(|v| v == value) => Ok(()),
        Some(value) => Err(format!(
            "\"{value}\" は列挙の値にありません（値は {}）",
            values.join("・")
        )),
        None => Err("列挙には、values にある値を文字列のリテラルで書きます".into()),
    }
}

/// `==`・`!=` で比べられるか。
fn compatible(lt: Ty<'_>, lhs: &Expr, rt: Ty<'_>, rhs: &Expr) -> Result<(), String> {
    match (lt, rt) {
        (Ty::Unknown, _) | (_, Ty::Unknown) => Ok(()),
        (Ty::Enum(a), Ty::Enum(b)) if a == b => Ok(()),
        (Ty::Enum(values), Ty::Str) => enum_value(values, rhs),
        (Ty::Str, Ty::Enum(values)) => enum_value(values, lhs),
        (a, b) if a == b => Ok(()),
        (a, b) => Err(format!(
            "型が合いません：{} と {} は比べられません",
            a.name(),
            b.name()
        )),
    }
}

/// 型 `target` の変数に、型 `value` の式を代入できるか。
fn assignable(target: Ty<'_>, value: Ty<'_>, expr: &Expr) -> Result<(), String> {
    match (target, value) {
        (_, Ty::Unknown) => Ok(()),
        (Ty::Enum(a), Ty::Enum(b)) if a == b => Ok(()),
        (Ty::Enum(values), Ty::Str) => enum_value(values, expr),
        (a, b) if a == b => Ok(()),
        (a, b) => Err(format!(
            "型が合いません（変数は {}、値は {}）",
            a.name(),
            b.name()
        )),
    }
}
