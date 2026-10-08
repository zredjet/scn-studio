//! カーソル位置から再生（T2-4、D055）：開始シーンから本文のある行までの最短経路（選んだ選択肢の数が
//! いちばん少ない道。同じ数なら、前にある選択肢を選ぶ道）を探し、その道を実行した結果の変数を求める。
//!
//! - 式の評価と命令の実行は、プレビューのランタイム（`app/src/preview/runtime.ts`・`expr.ts`）と同じ規則
//!   （仕様書14節の「評価」、13節）。誤りのある条件は偽、誤りのある `@設定` は書かない、宣言の無い変数は書かれる
//!   まで値が無い、シーンの終わりは行き止まり（呼ばれた先でも）。
//! - プレビューは、この道の選択肢を順に選んでランタイムを速送りし、目的の行の手前で止めてから再生する
//!   （背景・立ち絵・BGM の状態もそろう）。ここで求めた変数は、その結果と同じになる（テストで確かめる）。
//! - 探すのは選択肢で分かれる状態（シーン、実行中のブロックと位置、呼び出し、流れに関わる変数）。同じ状態は
//!   2度調べない。流れに関わる変数は、条件（`@if`・`@elif`・選択肢の ` if `）で読む変数と、それを書く `@設定`
//!   で読む変数（たどって閉じたもの）。ほかの変数は、どの道を通れるかを変えないので、見分けに使わない（選んだ
//!   結果を別々のフラグに書く選択肢が続いても、状態が倍々に増えない）。調べる数には上限を置き、超えたら
//!   「探しきれない」と知らせる。
//! - 永続（`scope: persistent`）の変数は、プレビューの今の値から始められる（プレビューは「最初から」でも永続の
//!   変数を残すため）。

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::Span;
use crate::ir::{Ir, IrBranch, IrExpr, IrInline, IrOption, Op};
use crate::preview::initial_value;
use crate::project::{Project, VarType};

/// 選択肢で分かれた状態を調べる数の上限。
const MAX_STATES: usize = 20_000;
/// 選択肢から次の選択肢までに実行する命令の数の上限（プレビューのランタイムと同じ）。
const MAX_STEPS: usize = 100_000;
/// `@呼出` の積み重ねの深さの上限（再帰する呼び出しで、状態とメモリが際限なく増えないように。T3-2）。
pub(crate) const MAX_CALL_DEPTH: usize = 64;

/// 変数の値（真偽・整数・文字列。列挙の値は文字列）。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Str(String),
}

impl Value {
    fn from_json(value: &serde_json::Value) -> Option<Value> {
        match value {
            serde_json::Value::Bool(b) => Some(Value::Bool(*b)),
            serde_json::Value::Number(n) => n.as_i64().map(Value::Int),
            serde_json::Value::String(s) => Some(Value::Str(s.clone())),
            _ => None,
        }
    }
}

/// 変数の値（そろえた名前から）。
pub type Variables = BTreeMap<String, Value>;

/// 式を評価する。誤り（値の無い変数、型の合わない演算、0での割り算、あふれ）は `None`。
fn evaluate(expr: &IrExpr, vars: &Variables) -> Option<Value> {
    match expr {
        IrExpr::Int { value } => Some(Value::Int(*value)),
        IrExpr::String { value } => Some(Value::Str(value.clone())),
        IrExpr::Bool { value } => Some(Value::Bool(*value)),
        IrExpr::Var { name } => vars.get(name).cloned(),
        IrExpr::Unary { op, operand } => {
            let v = evaluate(operand, vars)?;
            match (*op, v) {
                ("not", Value::Bool(b)) => Some(Value::Bool(!b)),
                ("-", Value::Int(n)) => n.checked_neg().map(Value::Int),
                _ => None,
            }
        }
        IrExpr::Binary { op, lhs, rhs } => {
            if matches!(*op, "and" | "or") {
                // 左だけで決まれば右を評価しない。
                let Value::Bool(l) = evaluate(lhs, vars)? else {
                    return None;
                };
                if *op == "and" && !l {
                    return Some(Value::Bool(false));
                }
                if *op == "or" && l {
                    return Some(Value::Bool(true));
                }
                let Value::Bool(r) = evaluate(rhs, vars)? else {
                    return None;
                };
                return Some(Value::Bool(r));
            }
            let l = evaluate(lhs, vars)?;
            let r = evaluate(rhs, vars)?;
            match *op {
                "==" => return Some(Value::Bool(l == r)),
                "!=" => return Some(Value::Bool(l != r)),
                _ => {}
            }
            let (Value::Int(l), Value::Int(r)) = (l, r) else {
                return None;
            };
            match *op {
                "+" => l.checked_add(r).map(Value::Int),
                "-" => l.checked_sub(r).map(Value::Int),
                "*" => l.checked_mul(r).map(Value::Int),
                // 0の向きへ切り捨てる。0での割り算は誤り。
                "/" => l.checked_div(r).map(Value::Int),
                "<" => Some(Value::Bool(l < r)),
                "<=" => Some(Value::Bool(l <= r)),
                ">" => Some(Value::Bool(l > r)),
                ">=" => Some(Value::Bool(l >= r)),
                _ => None,
            }
        }
    }
}

/// 条件：構文の崩れた式（E601）、誤り、真偽でない値は偽。
pub(crate) fn condition(expr: Option<&IrExpr>, vars: &Variables) -> bool {
    matches!(
        expr.and_then(|e| evaluate(e, vars)),
        Some(Value::Bool(true))
    )
}

/// 宣言の型に合う値か（宣言の無い変数は何でもよい）。
fn fits(ty: Option<&VarType>, value: &Value) -> bool {
    match (ty, value) {
        (None, _) => true,
        (Some(VarType::Int), Value::Int(_)) | (Some(VarType::Bool), Value::Bool(_)) => true,
        (Some(VarType::String), Value::Str(_)) => true,
        (Some(VarType::Enum(values)), Value::Str(s)) => values.contains(s),
        _ => false,
    }
}

/// 道で選んだ選択肢。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteStep {
    /// 選択肢のあるシーン。
    pub scene: String,
    /// `@選択` の行。
    pub choice: Span,
    /// 選んだ選択肢（0から数える）。
    pub option: usize,
}

/// 再生を始める命令。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RouteTarget {
    pub scene: String,
    pub file: String,
    /// 命令の行（`@選択`・`@if` はその行）。
    pub span: Span,
    pub line_id: Option<String>,
}

/// 最短経路。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Route {
    pub target: RouteTarget,
    /// 開始シーンから着けるか。
    pub reachable: bool,
    /// 選ぶ選択肢（出会う順）。
    pub steps: Vec<RouteStep>,
    /// 目的の命令の手前での変数（書かれていない宣言の無い変数は無い）。
    pub variables: Variables,
    /// 目的の命令の手前での、`@呼出` の積み重ね（呼び出し元のシーン。古いものが前）。
    pub calls: Vec<String>,
    /// 着けないとき、その理由。
    pub message: Option<String>,
}

fn line_id_of(op: &Op) -> Option<String> {
    match op {
        Op::Say { line_id, .. } | Op::Narrate { line_id, .. } | Op::Choice { line_id, .. } => {
            line_id.clone()
        }
        _ => None,
    }
}

/// 文書の順で、行の始まり `line_start` 以降にある最初の命令。選択肢の行なら、その `@選択`。
fn first_op_from(ops: &[Op], line_start: usize) -> Option<&Op> {
    for op in ops {
        if op.span().end > line_start {
            return Some(op);
        }
        match op {
            Op::Choice { options, .. } => {
                for option in options {
                    if option.span.end > line_start {
                        return Some(op);
                    }
                    if let Some(found) = first_op_from(&option.body, line_start) {
                        return Some(found);
                    }
                }
            }
            Op::If {
                branches,
                else_body,
                ..
            } => {
                for branch in branches {
                    if let Some(found) = first_op_from(&branch.body, line_start) {
                        return Some(found);
                    }
                }
                if let Some(found) = else_body
                    .as_deref()
                    .and_then(|body| first_op_from(body, line_start))
                {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

/// ファイルのある位置（バイト）から再生を始める命令。その行か、その後ろの最初の命令（同じシーンの中）。
/// シーンの外や、シーンの最後の命令より後ろなら `None`。
pub fn target_at(project: &Project, ir: &Ir, file: &str, offset: usize) -> Option<RouteTarget> {
    let text = &project.files.iter().find(|f| f.path == file)?.text;
    let head = text.get(..offset.min(text.len()))?;
    let line_start = head.rfind('\n').map_or(0, |i| i + 1);
    // 位置を含むシーン：そのファイルで、見出しが位置より前にある最後のシーン。
    let scene = ir
        .scenes
        .iter()
        .filter(|s| s.file == file && s.span.start <= offset.max(line_start))
        .max_by_key(|s| s.span.start)?;
    let op = first_op_from(&scene.ops, line_start)?;
    Some(RouteTarget {
        scene: scene.id.clone(),
        file: scene.file.clone(),
        span: op.span(),
        line_id: line_id_of(op),
    })
}

/// シーンの最初の命令（無ければ `None`）。
pub fn scene_target(ir: &Ir, scene: &str) -> Option<RouteTarget> {
    let s = ir.scenes.iter().find(|s| s.id == scene)?;
    let op = s.ops.first()?;
    Some(RouteTarget {
        scene: s.id.clone(),
        file: s.file.clone(),
        span: op.span(),
        line_id: line_id_of(op),
    })
}

/// 実行中のブロック。
#[derive(Debug, Clone)]
pub(crate) struct Frame<'a> {
    pub(crate) ops: &'a [Op],
    pub(crate) index: usize,
    /// 中身を実行し終えたら飛ぶ先（選択肢の行の `-> ID`）。
    pub(crate) after: Option<&'a str>,
}

/// 実行の状態。
#[derive(Debug, Clone)]
pub(crate) struct Machine<'a> {
    pub(crate) scene: &'a str,
    pub(crate) frames: Vec<Frame<'a>>,
    pub(crate) calls: Vec<(&'a str, Vec<Frame<'a>>)>,
    pub(crate) vars: Variables,
    pub(crate) steps: Vec<RouteStep>,
}

/// 同じ状態を見分ける鍵（ブロックは命令列の場所で見分ける）。
pub(crate) type Key = (
    String,
    Vec<(usize, usize)>,
    Vec<(String, Vec<(usize, usize)>)>,
    Vec<(String, Value)>,
);

fn frames_key(frames: &[Frame<'_>]) -> Vec<(usize, usize)> {
    frames
        .iter()
        .map(|f| (f.ops.as_ptr() as usize, f.index))
        .collect()
}

impl Machine<'_> {
    /// 状態の鍵。変数は、流れに関わるもの（`relevant`）だけ。
    pub(crate) fn key(&self, relevant: &BTreeSet<String>) -> Key {
        (
            self.scene.to_owned(),
            frames_key(&self.frames),
            self.calls
                .iter()
                .map(|(s, f)| ((*s).to_owned(), frames_key(f)))
                .collect(),
            self.vars
                .iter()
                .filter(|(k, _)| relevant.contains(*k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        )
    }
}

/// 式で読む変数。
fn read_vars(expr: &IrExpr, out: &mut BTreeSet<String>) {
    match expr {
        IrExpr::Var { name } => {
            out.insert(name.clone());
        }
        IrExpr::Unary { operand, .. } => read_vars(operand, out),
        IrExpr::Binary { lhs, rhs, .. } => {
            read_vars(lhs, out);
            read_vars(rhs, out);
        }
        IrExpr::Int { .. } | IrExpr::String { .. } | IrExpr::Bool { .. } => {}
    }
}

/// 流れに関わる変数：条件で読む変数と、それを書く `@設定` で読む変数（たどって閉じたもの）。
pub(crate) fn relevant_variables(ir: &Ir) -> BTreeSet<String> {
    fn walk(ops: &[Op], conds: &mut BTreeSet<String>, sets: &mut Vec<(String, BTreeSet<String>)>) {
        for op in ops {
            match op {
                Op::Set { var, op, value, .. } => {
                    let mut reads = BTreeSet::new();
                    read_vars(value, &mut reads);
                    // `+=`・`-=` は今の値も読む。
                    if *op != "=" {
                        reads.insert(var.clone());
                    }
                    sets.push((var.clone(), reads));
                }
                Op::Choice { options, .. } => {
                    for option in options {
                        if let Some(cond) = &option.cond {
                            read_vars(cond, conds);
                        }
                        walk(&option.body, conds, sets);
                    }
                }
                Op::If {
                    branches,
                    else_body,
                    ..
                } => {
                    for branch in branches {
                        if let Some(cond) = &branch.cond {
                            read_vars(cond, conds);
                        }
                        walk(&branch.body, conds, sets);
                    }
                    if let Some(body) = else_body {
                        walk(body, conds, sets);
                    }
                }
                _ => {}
            }
        }
    }
    let mut relevant = BTreeSet::new();
    let mut sets = Vec::new();
    for scene in &ir.scenes {
        walk(&scene.ops, &mut relevant, &mut sets);
    }
    loop {
        let before = relevant.len();
        for (var, reads) in &sets {
            if relevant.contains(var) {
                relevant.extend(reads.iter().cloned());
            }
        }
        if relevant.len() == before {
            return relevant;
        }
    }
}

/// 選択肢まで実行した結果。
pub(crate) enum Outcome<'a> {
    /// 目的の命令の手前に着いた。
    Reached(Machine<'a>),
    /// 選択肢で止まった（選べる選択肢がある）。
    Choice(Machine<'a>, Span, &'a [IrOption]),
    /// 進めない。
    End(EndReason),
}

/// 進めなくなったわけ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EndReason {
    /// `@エンディング`。
    Ending,
    /// 行き止まり（シーンの終わりに出口が無い）、呼び出しの外での `@戻る`。
    DeadEnd,
    /// 無いシーンへのジャンプ・呼び出し。
    MissingScene,
    /// 選べる選択肢が1つも無い（`@選択` の行）。
    NoOption(Span),
    /// 命令の数の上限。
    StepLimit,
    /// `@呼出` の積み重ねの深さの上限（[`MAX_CALL_DEPTH`]）。
    CallDepth,
}

/// 実行を見る（検証の段階2。T3-2）。最短経路と通読は見ない（[`Quiet`]）。
pub(crate) trait Observer<'a> {
    /// シーンに入った（ジャンプ、呼び出し、選択肢の行の `-> ID`）。
    fn entered(&mut self, _scene: &'a str) {}
    /// `@if`・`@elif` の条件を評価した（真になるまで、上から順に）。
    fn branch(&mut self, _scene: &'a str, _branch: &'a IrBranch, _value: bool) {}
    /// `@エンディング` に着いた（`vars` は着いたときの変数）。
    fn ending(&mut self, _scene: &'a str, _id: Option<&'a str>, _vars: &Variables) {}
    /// 選択肢に着いたが、選べる選択肢が1つも無かった。
    fn no_option(&mut self, _scene: &'a str, _span: Span, _options: &'a [IrOption]) {}
}

/// 何も見ない。
pub(crate) struct Quiet;

impl Observer<'_> for Quiet {}

pub(crate) struct Searcher<'a> {
    pub(crate) ir: &'a Ir,
    pub(crate) project: &'a Project,
    /// 止まる命令（無ければ、エンディングか行き止まりまで）。
    pub(crate) target: Option<&'a RouteTarget>,
}

impl<'a> Searcher<'a> {
    /// シーンに入る（無いシーンなら `false`）。
    pub(crate) fn enter(&self, m: &mut Machine<'a>, scene: &str) -> bool {
        match self.ir.scenes.iter().find(|s| s.id == scene) {
            Some(s) => {
                m.scene = &s.id;
                m.frames = vec![Frame {
                    ops: &s.ops,
                    index: 0,
                    after: None,
                }];
                true
            }
            None => false,
        }
    }

    fn declared(&self, name: &str) -> Option<&'a VarType> {
        self.project
            .variables
            .iter()
            .find(|v| v.key == name)
            .map(|v| &v.ty)
    }

    /// `@設定`：誤りなら書かない。宣言の型に合わない値も書かない。
    fn set(&self, vars: &mut Variables, var: &str, op: &str, value: &IrExpr) {
        let new = evaluate(value, vars).and_then(|v| match (op, v) {
            ("=", v) => Some(v),
            ("+=", Value::Int(d)) => match vars.get(var) {
                Some(Value::Int(c)) => c.checked_add(d).map(Value::Int),
                _ => None,
            },
            ("-=", Value::Int(d)) => match vars.get(var) {
                Some(Value::Int(c)) => c.checked_sub(d).map(Value::Int),
                _ => None,
            },
            _ => None,
        });
        if let Some(new) = new
            && fits(self.declared(var), &new)
        {
            vars.insert(var.to_owned(), new);
        }
    }

    /// 選択肢か目的の命令に着くまで実行する。`obs` は、入ったシーンと評価した条件を見る。
    pub(crate) fn run(&self, mut m: Machine<'a>, obs: &mut dyn Observer<'a>) -> Outcome<'a> {
        for _ in 0..MAX_STEPS {
            let Some(top) = m.frames.last_mut() else {
                return Outcome::End(EndReason::DeadEnd);
            };
            let Some(op) = top.ops.get(top.index) else {
                // ブロックの終わり。シーンの終わりなら行き止まり（呼ばれた先でも）。
                let after = top.after;
                m.frames.pop();
                if let Some(target) = after {
                    if !self.enter(&mut m, target) {
                        return Outcome::End(EndReason::MissingScene);
                    }
                    obs.entered(m.scene);
                    continue;
                }
                if m.frames.is_empty() {
                    return Outcome::End(EndReason::DeadEnd);
                }
                continue;
            };
            if let Some(target) = self.target
                && m.scene == target.scene
                && op.span() == target.span
            {
                return Outcome::Reached(m);
            }
            top.index += 1;
            match op {
                Op::Set { var, op, value, .. } => self.set(&mut m.vars, var, op, value),
                Op::Choice { span, options, .. } => {
                    let any = options
                        .iter()
                        .any(|o| o.cond_text.is_none() || condition(o.cond.as_ref(), &m.vars));
                    if !any {
                        obs.no_option(m.scene, *span, options);
                        return Outcome::End(EndReason::NoOption(*span));
                    }
                    return Outcome::Choice(m, *span, options);
                }
                Op::If {
                    branches,
                    else_body,
                    ..
                } => {
                    let mut taken = None;
                    for branch in branches {
                        let value = condition(branch.cond.as_ref(), &m.vars);
                        obs.branch(m.scene, branch, value);
                        if value {
                            taken = Some(branch.body.as_slice());
                            break;
                        }
                    }
                    let body = taken.or(else_body.as_deref());
                    if let Some(body) = body {
                        m.frames.push(Frame {
                            ops: body,
                            index: 0,
                            after: None,
                        });
                    }
                }
                Op::Jump { target, .. } => {
                    if !self.enter(&mut m, target) {
                        return Outcome::End(EndReason::MissingScene);
                    }
                    obs.entered(m.scene);
                }
                Op::Call { target, .. } => {
                    if m.calls.len() >= MAX_CALL_DEPTH {
                        return Outcome::End(EndReason::CallDepth);
                    }
                    let caller = (m.scene, m.frames.clone());
                    m.calls.push(caller);
                    if !self.enter(&mut m, target) {
                        return Outcome::End(EndReason::MissingScene);
                    }
                    obs.entered(m.scene);
                }
                Op::Return { .. } => match m.calls.pop() {
                    Some((scene, frames)) => {
                        m.scene = scene;
                        m.frames = frames;
                    }
                    None => return Outcome::End(EndReason::DeadEnd),
                },
                Op::Ending { id, .. } => {
                    obs.ending(m.scene, id.as_deref(), &m.vars);
                    return Outcome::End(EndReason::Ending);
                }
                _ => {}
            }
        }
        Outcome::End(EndReason::StepLimit)
    }

    fn found(&self, m: Machine<'a>, target: &RouteTarget) -> Route {
        Route {
            target: target.clone(),
            reachable: true,
            steps: m.steps,
            variables: m.vars,
            calls: m.calls.iter().map(|(s, _)| (*s).to_owned()).collect(),
            message: None,
        }
    }
}

/// 開始シーンの、初めの変数（宣言の初期値。宣言の無い変数は無い）。永続の変数は、`persistent` に宣言の型に
/// 合う値があれば、その値（プレビューの今の値）。
pub(crate) fn initial_variables(project: &Project, persistent: &Variables) -> Variables {
    project
        .variables
        .iter()
        .filter_map(|v| {
            if v.scope.as_deref() == Some("persistent")
                && let Some(kept) = persistent.get(&v.key)
                && fits(Some(&v.ty), kept)
            {
                return Some((v.key.clone(), kept.clone()));
            }
            let (value, _) = initial_value(&v.ty, v.default.as_deref());
            Value::from_json(&value).map(|value| (v.key.clone(), value))
        })
        .collect()
}

/// 開始シーンから `target` までの最短経路を探す。`persistent` は、永続の変数の今の値（無ければ初期値）。
pub fn route_to(project: &Project, ir: &Ir, target: &RouteTarget, persistent: &Variables) -> Route {
    let unreachable = |message: &str| Route {
        target: target.clone(),
        reachable: false,
        steps: Vec::new(),
        variables: BTreeMap::new(),
        calls: Vec::new(),
        message: Some(message.to_owned()),
    };
    let start = ir
        .start_scene
        .as_deref()
        .filter(|s| ir.scenes.iter().any(|x| x.id == *s))
        .or_else(|| ir.scenes.first().map(|s| s.id.as_str()));
    let Some(start) = start else {
        return unreachable("シーンがありません");
    };
    let searcher = Searcher {
        ir,
        project,
        target: Some(target),
    };
    let mut first = Machine {
        scene: "",
        frames: Vec::new(),
        calls: Vec::new(),
        vars: initial_variables(project, persistent),
        steps: Vec::new(),
    };
    if !searcher.enter(&mut first, start) {
        return unreachable("開始シーンがありません");
    }
    let relevant = relevant_variables(ir);
    let mut queue = VecDeque::from([first]);
    let mut seen: BTreeSet<Key> = BTreeSet::new();
    let mut states = 0;
    while let Some(machine) = queue.pop_front() {
        match searcher.run(machine, &mut Quiet) {
            Outcome::Reached(m) => return searcher.found(m, target),
            Outcome::End(_) => {}
            Outcome::Choice(m, span, options) => {
                if !seen.insert(m.key(&relevant)) {
                    continue;
                }
                states += 1;
                if states > MAX_STATES {
                    return unreachable(
                        "分かれ道が多すぎて、最短経路を探しきれませんでした。デバッグの欄で変数を直して始めてください",
                    );
                }
                for (index, option) in options.iter().enumerate() {
                    if option.cond_text.is_some() && !condition(option.cond.as_ref(), &m.vars) {
                        continue;
                    }
                    let mut next = m.clone();
                    next.steps.push(RouteStep {
                        scene: m.scene.to_owned(),
                        choice: span,
                        option: index,
                    });
                    next.frames.push(Frame {
                        ops: &option.body,
                        index: 0,
                        after: option.jump.as_deref(),
                    });
                    queue.push_back(next);
                }
            }
        }
    }
    unreachable("開始シーンから、選択肢と条件をどう選んでも着けません")
}

/// ファイルのある位置から再生するための最短経路。始められる命令が無ければ `None`。
pub fn route_at(
    project: &Project,
    ir: &Ir,
    file: &str,
    offset: usize,
    persistent: &Variables,
) -> Option<Route> {
    let target = target_at(project, ir, file, offset)?;
    Some(route_to(project, ir, &target, persistent))
}

// ---- 通読（T2-5、D056） ----

/// 通読する道。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReadingTarget {
    /// 開始シーンのルート（共通）だけ：ほかのルートのシーンに入る手前まで（選択肢は前のものを選ぶ）。
    /// 開始シーンにルートが無ければ、通し（終わりまで）。
    Common,
    /// ルート：そのルートの、着ける最初のエンディング（本文の順）までの最短経路で（どれにも着けなければ、
    /// そのルートの最初のシーンまで）。
    Route { name: String },
    /// エンディング：そのエンディングまでの最短経路で。
    Ending { id: String },
    /// 選んだ選択肢の順（プレビューで選んだ道）。合わない道は読み飛ばし、使い切ったら選べる最初の選択肢を選ぶ。
    Steps { steps: Vec<RouteStep> },
}

/// 通読の道の候補（画面の選択肢）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReadingChoice {
    pub label: String,
    pub target: ReadingTarget,
}

/// 通読の選択肢の、ひとつの項目。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReadingOption {
    pub text: Vec<IrInline>,
    pub enabled: bool,
}

/// 通読の本文の並び。文中の記法は IR のまま（変数の埋め込みは、値が決まればその値の文字にする）。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReadingItem {
    /// シーンに入った（`@戻る` で呼び出し元へ戻ったときは出さない）。
    Scene {
        id: String,
        title: String,
        route: Option<String>,
        file: String,
        span: Span,
    },
    Say {
        file: String,
        span: Span,
        line_id: Option<String>,
        speaker: String,
        style: String,
        text: Vec<IrInline>,
    },
    Narrate {
        file: String,
        span: Span,
        line_id: Option<String>,
        text: Vec<IrInline>,
    },
    /// 段落の区切り。
    Break,
    /// 選んだ選択肢。
    Choice {
        file: String,
        span: Span,
        prompt: Option<Vec<IrInline>>,
        options: Vec<ReadingOption>,
        chosen: usize,
    },
    Ending {
        id: Option<String>,
    },
}

/// 通読の結果。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Reading {
    pub target: ReadingTarget,
    pub items: Vec<ReadingItem>,
    /// 止まった理由：`ending`・`dead_end`（行き止まり、無いシーン、選べる選択肢が無い）・`route_changed`
    /// （共通だけで、ほかのルートに入る）・`limit`（長すぎる、循環）・`unreachable`（道が無い）。
    pub stop: &'static str,
    /// 知らせ（道が無い理由など）。
    pub message: Option<String>,
}

/// 通読の項目の数の上限。
const MAX_ITEMS: usize = 100_000;

/// 変数の埋め込みを、値が決まればその値の文字にする（プレビューと同じ見せ方）。
fn resolved(text: &[IrInline], vars: &Variables) -> Vec<IrInline> {
    text.iter()
        .map(|inline| match inline {
            IrInline::Interpolation {
                expr: Some(expr), ..
            } => match evaluate(expr, vars) {
                Some(Value::Bool(b)) => IrInline::Text {
                    text: b.to_string(),
                },
                Some(Value::Int(n)) => IrInline::Text {
                    text: n.to_string(),
                },
                Some(Value::Str(s)) => IrInline::Text { text: s },
                None => inline.clone(),
            },
            _ => inline.clone(),
        })
        .collect()
}

/// 通読の道の名前（画面の選択肢、プレーンテキストの見出しとファイル名）。
pub fn target_label(ir: &Ir, target: &ReadingTarget) -> String {
    match target {
        ReadingTarget::Common => match start_scene(ir).and_then(|s| s.meta.route.as_deref()) {
            Some(route) => format!("{route}のみ"),
            // ルートの無い作品は、選べる最初の選択肢で通しで読む。
            None => "通し".to_owned(),
        },
        ReadingTarget::Route { name } => format!("{name}ルート"),
        ReadingTarget::Ending { id } => format!("エンディング {id} まで"),
        ReadingTarget::Steps { .. } => "選んだ道".to_owned(),
    }
}

/// 読み終えるまでの目安に使う、1分に読む字数（ステータスバー。T2-8、D059）。
pub const CHARS_PER_MINUTE: usize = 400;

/// 道の文字数と、読み終えるまでの目安（ステータスバー。T2-8、D059）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RouteStat {
    pub label: String,
    /// ルートの名前（共通なら開始シーンのルート。無ければ `None`）。
    pub route: Option<String>,
    /// 道に沿って読む台詞と地の文の、表示する文字の数（`assist::display_chars`）。
    pub chars: usize,
    /// 読み終えるまでの目安（分。`CHARS_PER_MINUTE` で割って切り上げ）。
    pub minutes: usize,
    /// 道の止まり方（`route::read` の `stop`）。
    pub stop: String,
}

/// 共通とルートごとの、道に沿った文字数と読み終えるまでの目安。永続の変数は初めの値。
pub fn route_stats(project: &Project, ir: &Ir) -> Vec<RouteStat> {
    let start_route = start_scene(ir).and_then(|s| s.meta.route.clone());
    reading_choices(ir)
        .into_iter()
        .filter_map(|choice| {
            let route = match &choice.target {
                ReadingTarget::Common => start_route.clone(),
                ReadingTarget::Route { name } => Some(name.clone()),
                ReadingTarget::Ending { .. } | ReadingTarget::Steps { .. } => return None,
            };
            let reading = read(project, ir, &choice.target, &Variables::new());
            let chars: usize = reading
                .items
                .iter()
                .map(|item| match item {
                    ReadingItem::Say { text, .. } | ReadingItem::Narrate { text, .. } => {
                        crate::assist::display_chars(text)
                    }
                    _ => 0,
                })
                .sum();
            Some(RouteStat {
                label: choice.label,
                route,
                chars,
                minutes: chars.div_ceil(CHARS_PER_MINUTE),
                stop: reading.stop.to_owned(),
            })
        })
        .collect()
}

/// 通読の道の候補：共通（開始シーンのルート）、ほかのルート、エンディング（本文の順）。
pub fn reading_choices(ir: &Ir) -> Vec<ReadingChoice> {
    let mut out = Vec::new();
    let start_route = start_scene(ir).and_then(|s| s.meta.route.clone());
    let mut push = |target: ReadingTarget| {
        out.push(ReadingChoice {
            label: target_label(ir, &target),
            target,
        });
    };
    if !ir.scenes.is_empty() {
        push(ReadingTarget::Common);
    }
    let mut routes: Vec<&str> = Vec::new();
    for scene in &ir.scenes {
        if let Some(route) = scene.meta.route.as_deref()
            && Some(route) != start_route.as_deref()
            && !routes.contains(&route)
        {
            routes.push(route);
        }
    }
    for route in routes {
        push(ReadingTarget::Route {
            name: route.to_owned(),
        });
    }
    let mut endings: Vec<String> = Vec::new();
    for scene in &ir.scenes {
        collect_endings(&scene.ops, &mut endings);
    }
    for id in endings {
        push(ReadingTarget::Ending { id });
    }
    out
}

fn collect_endings(ops: &[Op], out: &mut Vec<String>) {
    for op in ops {
        match op {
            Op::Ending { id: Some(id), .. } if !out.contains(id) => out.push(id.clone()),
            Op::Choice { options, .. } => {
                for option in options {
                    collect_endings(&option.body, out);
                }
            }
            Op::If {
                branches,
                else_body,
                ..
            } => {
                for branch in branches {
                    collect_endings(&branch.body, out);
                }
                if let Some(body) = else_body {
                    collect_endings(body, out);
                }
            }
            _ => {}
        }
    }
}

fn start_scene(ir: &Ir) -> Option<&crate::ir::IrScene> {
    ir.start_scene
        .as_deref()
        .and_then(|s| ir.scenes.iter().find(|x| x.id == s))
        .or_else(|| ir.scenes.first())
}

/// 命令の木の中の、条件に合う最初の命令（文書の順）。
fn find_op<'a>(ops: &'a [Op], pred: &dyn Fn(&Op) -> bool) -> Option<&'a Op> {
    for op in ops {
        if pred(op) {
            return Some(op);
        }
        let found = match op {
            Op::Choice { options, .. } => options.iter().find_map(|o| find_op(&o.body, pred)),
            Op::If {
                branches,
                else_body,
                ..
            } => branches
                .iter()
                .find_map(|b| find_op(&b.body, pred))
                .or_else(|| else_body.as_deref().and_then(|b| find_op(b, pred))),
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

/// 道の行き先の候補（前から順に試す）。ルートなら、そのルートのエンディング（本文の順）と、そのルートの最初の
/// シーンの最初の命令。エンディングなら、そのエンディング。
fn reading_goals(ir: &Ir, target: &ReadingTarget) -> Vec<RouteTarget> {
    let to = |scene: &crate::ir::IrScene, op: &Op| RouteTarget {
        scene: scene.id.clone(),
        file: scene.file.clone(),
        span: op.span(),
        line_id: line_id_of(op),
    };
    match target {
        ReadingTarget::Route { name } => {
            let mine: Vec<&crate::ir::IrScene> = ir
                .scenes
                .iter()
                .filter(|s| s.meta.route.as_deref() == Some(name.as_str()))
                .collect();
            let mut goals = Vec::new();
            for scene in &mine {
                let mut endings = Vec::new();
                collect_ending_ops(&scene.ops, &mut endings);
                goals.extend(endings.into_iter().map(|op| to(scene, op)));
            }
            if let Some((scene, op)) = mine.first().and_then(|s| s.ops.first().map(|op| (s, op))) {
                goals.push(to(scene, op));
            }
            goals
        }
        ReadingTarget::Ending { id } => ir
            .scenes
            .iter()
            .find_map(|s| {
                find_op(
                    &s.ops,
                    &|op| matches!(op, Op::Ending { id: Some(x), .. } if x == id),
                )
                .map(|op| to(s, op))
            })
            .into_iter()
            .collect(),
        ReadingTarget::Common | ReadingTarget::Steps { .. } => Vec::new(),
    }
}

/// 命令の木の中のエンディング（本文の順）。
pub(crate) fn collect_ending_ops<'a>(ops: &'a [Op], out: &mut Vec<&'a Op>) {
    for op in ops {
        match op {
            Op::Ending { .. } => out.push(op),
            Op::Choice { options, .. } => {
                for option in options {
                    collect_ending_ops(&option.body, out);
                }
            }
            Op::If {
                branches,
                else_body,
                ..
            } => {
                for branch in branches {
                    collect_ending_ops(&branch.body, out);
                }
                if let Some(body) = else_body {
                    collect_ending_ops(body, out);
                }
            }
            _ => {}
        }
    }
}

/// 通読する：道（選択肢の順）に沿って本文を実行し、台詞・地の文・選んだ選択肢・シーンの見出しを並べる。
/// 道を使い切ったら、選べる最初の選択肢を選ぶ。エンディング・行き止まりで止まる。
pub fn read(project: &Project, ir: &Ir, target: &ReadingTarget, persistent: &Variables) -> Reading {
    let unreachable = |message: String| Reading {
        target: target.clone(),
        items: Vec::new(),
        stop: "unreachable",
        message: Some(message),
    };
    let steps = match target {
        ReadingTarget::Steps { steps } => steps.clone(),
        ReadingTarget::Common => Vec::new(),
        ReadingTarget::Route { .. } | ReadingTarget::Ending { .. } => {
            let goals = reading_goals(ir, target);
            if goals.is_empty() {
                return unreachable(match target {
                    ReadingTarget::Ending { id } => format!("エンディング {id} がありません"),
                    _ => format!("{}のシーンがありません", target_label(ir, target)),
                });
            }
            // 着ける最初の行き先（ルートの最初のエンディングに着けなければ、次のエンディング）。
            let mut last = None;
            let mut found = None;
            for goal in &goals {
                let route = route_to(project, ir, goal, persistent);
                if route.reachable {
                    found = Some(route.steps);
                    break;
                }
                last = route.message;
            }
            match found {
                Some(steps) => steps,
                None => return unreachable(last.unwrap_or_default()),
            }
        }
    };
    let Some(start) = start_scene(ir) else {
        return unreachable("シーンがありません".to_owned());
    };
    let common = matches!(target, ReadingTarget::Common).then(|| start.meta.route.clone());
    // 目的の命令では止まらない（通読は、エンディングか行き止まりまで）。
    let searcher = Searcher {
        ir,
        project,
        target: None,
    };
    let mut reader = Reader {
        searcher: &searcher,
        items: Vec::new(),
        file: String::new(),
        steps: steps.into_iter().collect(),
        common: common.flatten(),
    };
    let stop = reader.run(start, persistent);
    Reading {
        target: target.clone(),
        items: reader.items,
        stop,
        message: None,
    }
}

struct Reader<'s, 'a> {
    searcher: &'s Searcher<'a>,
    items: Vec<ReadingItem>,
    /// 今のシーンのファイル。
    file: String,
    steps: VecDeque<RouteStep>,
    /// 共通だけを読むとき、開始シーンのルート。
    common: Option<String>,
}

impl<'a> Reader<'_, 'a> {
    /// シーンに入る。入れなければ（無いシーン、共通だけでほかのルート）止まる理由。
    /// シーンに入る。`heading` なら見出しを出す（`@呼出` で呼ばれたシーンは、呼び出し元の本文に挟まるので
    /// 出さない）。入れなければ（無いシーン、共通だけでほかのルート）止まる理由。
    fn enter(
        &mut self,
        m: &mut Machine<'a>,
        scene: &str,
        heading: bool,
    ) -> Result<(), &'static str> {
        let Some(s) = self.searcher.ir.scenes.iter().find(|s| s.id == scene) else {
            return Err("dead_end");
        };
        if let Some(common) = &self.common
            && s.meta.route.as_ref().is_some_and(|r| r != common)
        {
            return Err("route_changed");
        }
        self.searcher.enter(m, scene);
        self.file.clone_from(&s.file);
        if heading {
            self.items.push(ReadingItem::Scene {
                id: s.id.clone(),
                title: s.title.clone(),
                route: s.meta.route.clone(),
                file: s.file.clone(),
                span: s.span,
            });
        }
        Ok(())
    }

    fn run(&mut self, start: &'a crate::ir::IrScene, persistent: &Variables) -> &'static str {
        let mut m = Machine {
            scene: "",
            frames: Vec::new(),
            calls: Vec::new(),
            vars: initial_variables(self.searcher.project, persistent),
            steps: Vec::new(),
        };
        if let Err(stop) = self.enter(&mut m, &start.id, true) {
            return stop;
        }
        for _ in 0..MAX_STEPS * 10 {
            if self.items.len() > MAX_ITEMS {
                return "limit";
            }
            let file = self.file.clone();
            let Some(top) = m.frames.last_mut() else {
                return "dead_end";
            };
            let Some(op) = top.ops.get(top.index) else {
                let after = top.after;
                m.frames.pop();
                if let Some(target) = after {
                    if let Err(stop) = self.enter(&mut m, target, true) {
                        return stop;
                    }
                    continue;
                }
                if m.frames.is_empty() {
                    return "dead_end";
                }
                continue;
            };
            top.index += 1;
            match op {
                Op::Say {
                    span,
                    line_id,
                    speaker,
                    style,
                    text,
                    ..
                } => self.items.push(ReadingItem::Say {
                    file,
                    span: *span,
                    line_id: line_id.clone(),
                    speaker: speaker.clone(),
                    style: style.clone(),
                    text: resolved(text, &m.vars),
                }),
                Op::Narrate {
                    span,
                    line_id,
                    text,
                } => self.items.push(ReadingItem::Narrate {
                    file,
                    span: *span,
                    line_id: line_id.clone(),
                    text: resolved(text, &m.vars),
                }),
                Op::Break { .. } => self.items.push(ReadingItem::Break),
                Op::Set { var, op, value, .. } => self.searcher.set(&mut m.vars, var, op, value),
                Op::Choice {
                    span,
                    prompt,
                    options,
                    ..
                } => {
                    let enabled: Vec<bool> = options
                        .iter()
                        .map(|o| o.cond_text.is_none() || condition(o.cond.as_ref(), &m.vars))
                        .collect();
                    // 道の選択肢：残りの道のうち、この選択肢のものを探し、その前の合わない道は捨てる（ADV の
                    // デバッグで飛んだ、本文が変わったなどで、道がずれたとき）。無ければ、選べる最初の選択肢。
                    let at = self
                        .steps
                        .iter()
                        .position(|s| s.scene == m.scene && s.choice == *span);
                    let planned = at
                        .and_then(|at| self.steps.get(at))
                        .map(|s| s.option)
                        .filter(|i| enabled.get(*i).copied().unwrap_or(false));
                    if let (Some(at), Some(_)) = (at, planned) {
                        self.steps.drain(..=at);
                    }
                    let Some(chosen) = planned.or_else(|| enabled.iter().position(|e| *e)) else {
                        return "dead_end";
                    };
                    let Some(option) = options.get(chosen) else {
                        return "dead_end";
                    };
                    self.items.push(ReadingItem::Choice {
                        file,
                        span: *span,
                        prompt: prompt.as_ref().map(|p| resolved(p, &m.vars)),
                        options: options
                            .iter()
                            .zip(&enabled)
                            .map(|(o, e)| ReadingOption {
                                text: resolved(&o.text, &m.vars),
                                enabled: *e,
                            })
                            .collect(),
                        chosen,
                    });
                    m.frames.push(Frame {
                        ops: &option.body,
                        index: 0,
                        after: option.jump.as_deref(),
                    });
                }
                Op::If {
                    branches,
                    else_body,
                    ..
                } => {
                    let body = branches
                        .iter()
                        .find(|b| condition(b.cond.as_ref(), &m.vars))
                        .map(|b| b.body.as_slice())
                        .or(else_body.as_deref());
                    if let Some(body) = body {
                        m.frames.push(Frame {
                            ops: body,
                            index: 0,
                            after: None,
                        });
                    }
                }
                Op::Jump { target, .. } => {
                    if let Err(stop) = self.enter(&mut m, target, true) {
                        return stop;
                    }
                }
                Op::Call { target, .. } => {
                    let caller = (m.scene, m.frames.clone());
                    m.calls.push(caller);
                    if let Err(stop) = self.enter(&mut m, target, false) {
                        return stop;
                    }
                }
                Op::Return { .. } => match m.calls.pop() {
                    Some((scene, frames)) => {
                        m.scene = scene;
                        m.frames = frames;
                        if let Some(s) = self.searcher.ir.scenes.iter().find(|s| s.id == scene) {
                            self.file.clone_from(&s.file);
                        }
                    }
                    None => return "dead_end",
                },
                Op::Ending { id, .. } => {
                    self.items.push(ReadingItem::Ending { id: id.clone() });
                    return "ending";
                }
                _ => {}
            }
        }
        "limit"
    }
}
