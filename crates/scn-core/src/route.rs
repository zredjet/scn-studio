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
use crate::ir::{Ir, IrExpr, IrOption, Op};
use crate::preview::initial_value;
use crate::project::{Project, VarType};

/// 選択肢で分かれた状態を調べる数の上限。
const MAX_STATES: usize = 20_000;
/// 選択肢から次の選択肢までに実行する命令の数の上限（プレビューのランタイムと同じ）。
const MAX_STEPS: usize = 100_000;

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
fn condition(expr: Option<&IrExpr>, vars: &Variables) -> bool {
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
struct Frame<'a> {
    ops: &'a [Op],
    index: usize,
    /// 中身を実行し終えたら飛ぶ先（選択肢の行の `-> ID`）。
    after: Option<&'a str>,
}

/// 実行の状態。
#[derive(Debug, Clone)]
struct Machine<'a> {
    scene: &'a str,
    frames: Vec<Frame<'a>>,
    calls: Vec<(&'a str, Vec<Frame<'a>>)>,
    vars: Variables,
    steps: Vec<RouteStep>,
}

/// 同じ状態を見分ける鍵（ブロックは命令列の場所で見分ける）。
type Key = (
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
    fn key(&self, relevant: &BTreeSet<String>) -> Key {
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
fn relevant_variables(ir: &Ir) -> BTreeSet<String> {
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
enum Outcome<'a> {
    /// 目的の命令の手前に着いた。
    Reached(Machine<'a>),
    /// 選択肢で止まった（選べる選択肢がある）。
    Choice(Machine<'a>, Span, &'a [IrOption]),
    /// 進めない（エンディング、行き止まり、無いシーン、選べる選択肢が無い、命令の数の上限）。
    End,
}

struct Searcher<'a> {
    ir: &'a Ir,
    project: &'a Project,
    target: &'a RouteTarget,
}

impl<'a> Searcher<'a> {
    /// シーンに入る（無いシーンなら `false`）。
    fn enter(&self, m: &mut Machine<'a>, scene: &str) -> bool {
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

    /// 選択肢か目的の命令に着くまで実行する。
    fn run(&self, mut m: Machine<'a>) -> Outcome<'a> {
        for _ in 0..MAX_STEPS {
            let Some(top) = m.frames.last_mut() else {
                return Outcome::End;
            };
            let Some(op) = top.ops.get(top.index) else {
                // ブロックの終わり。シーンの終わりなら行き止まり（呼ばれた先でも）。
                let after = top.after;
                m.frames.pop();
                if let Some(target) = after {
                    if !self.enter(&mut m, target) {
                        return Outcome::End;
                    }
                    continue;
                }
                if m.frames.is_empty() {
                    return Outcome::End;
                }
                continue;
            };
            if m.scene == self.target.scene && op.span() == self.target.span {
                return Outcome::Reached(m);
            }
            top.index += 1;
            match op {
                Op::Set { var, op, value, .. } => {
                    let new = evaluate(value, &m.vars).and_then(|v| match (*op, v) {
                        ("=", v) => Some(v),
                        ("+=", Value::Int(d)) => match m.vars.get(var) {
                            Some(Value::Int(c)) => c.checked_add(d).map(Value::Int),
                            _ => None,
                        },
                        ("-=", Value::Int(d)) => match m.vars.get(var) {
                            Some(Value::Int(c)) => c.checked_sub(d).map(Value::Int),
                            _ => None,
                        },
                        _ => None,
                    });
                    if let Some(new) = new
                        && fits(self.declared(var), &new)
                    {
                        m.vars.insert(var.clone(), new);
                    }
                }
                Op::Choice { span, options, .. } => {
                    let any = options
                        .iter()
                        .any(|o| o.cond_text.is_none() || condition(o.cond.as_ref(), &m.vars));
                    if !any {
                        return Outcome::End;
                    }
                    return Outcome::Choice(m, *span, options);
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
                    if !self.enter(&mut m, target) {
                        return Outcome::End;
                    }
                }
                Op::Call { target, .. } => {
                    let caller = (m.scene, m.frames.clone());
                    m.calls.push(caller);
                    if !self.enter(&mut m, target) {
                        return Outcome::End;
                    }
                }
                Op::Return { .. } => match m.calls.pop() {
                    Some((scene, frames)) => {
                        m.scene = scene;
                        m.frames = frames;
                    }
                    None => return Outcome::End,
                },
                Op::Ending { .. } => return Outcome::End,
                _ => {}
            }
        }
        Outcome::End
    }

    fn found(&self, m: Machine<'a>) -> Route {
        Route {
            target: self.target.clone(),
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
fn initial_variables(project: &Project, persistent: &Variables) -> Variables {
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
        target,
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
        match searcher.run(machine) {
            Outcome::Reached(m) => return searcher.found(m),
            Outcome::End => {}
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
