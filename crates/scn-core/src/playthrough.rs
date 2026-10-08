//! 検証の段階2（T3-2、D063）：自動巡回。開始シーンから、選択肢の組み合わせを実際に実行して調べる
//! （`docs/design.md` の「到達性解析の進め方」の2、「検証項目」の段階2）。
//!
//! - 実行の規則は、カーソル位置から再生の最短経路（`route`）と同じ実行器を使う（式の評価と命令の実行は
//!   プレビューのランタイムと同じ。D055）。選択肢で分かれる状態（シーン、実行中のブロックと位置、呼び出し、
//!   流れに関わる変数）ごとにメモ化し、同じ状態は2度調べない。状態の数に上限を置き、超えたら調べきれなかった
//!   ことを結果に書く（「無い」と言う診断（W506・W507・W509）は出さない。見つけた事実（W508・W704）は出す）。
//! - 永続（`scope: persistent`）の変数は、周回を考える：宣言の初期値から始め、エンディングに着いたときの永続の
//!   変数の値から、もう一度始める（新しい値の組が出なくなるまで。始め方の数にも上限を置く）。周回の始め方は、
//!   それを生んだエンディングの区間を覚え、道の例と伏線の道は前の周からつなぐ（前の周で通った仕込みも数える）。
//! - 重くならないように：道の例はエンディングに初めて着いたときだけ作り、伏線の道は前の区間への印でたどる。
//!   `@呼出` の積み重ねは64段まで（`route::MAX_CALL_DEPTH`）。
//! - 診断：決して選べない選択肢（W506）、決して真にならない条件（W507）、選べる選択肢が1つも無くなる選択
//!   （W508）、条件と選択肢をどう選んでも着かないシーン（W509。着いたシーンからの辺が実行されないものだけ。
//!   その先のシーンには出さない）、仕込みを通らずに回収へ着ける伏線（W704）。
//! - 診断ではない結果：着いたシーンと選べた選択肢の数（カバレッジ）、エンディングごとの到達条件（着いたか、
//!   選択肢のいちばん少ない道の例、着いたときの流れに関わる変数の値）。

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::Serialize;

use crate::bible::{FORESHADOW_TYPE, plain_text};
use crate::diag::{Code, Span};
use crate::ir::{Ir, IrBranch, IrDiagnostic, IrOption, Op};
use crate::project::Project;
use crate::route::{
    EndReason, Frame, Key, MAX_CALL_DEPTH, Machine, Observer, Outcome, Searcher, Value, Variables,
    condition, initial_variables, relevant_variables,
};

/// 調べる状態（選択肢で分かれた状態）の数の上限。
const MAX_STATES: usize = 50_000;
/// 周回の始め方（永続の変数の値の組）の数の上限。
const MAX_STARTS: usize = 16;
/// 道の例に書く選択肢の数の上限。
const MAX_EXAMPLE: usize = 12;
/// エンディングごとに書く、変数の値の数の上限。
const MAX_VALUES: usize = 20;
/// 道の例で、前の周回のエンディングからもう一度始めたところの印。
pub const REPLAY_MARK: &str = "〔周回〕";

/// 数（全体のうち、着いた・選べた数）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Coverage {
    pub reached: usize,
    pub total: usize,
}

/// エンディングの到達条件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EndingReach {
    /// エンディングのID。
    pub id: String,
    /// `@エンディング` のあるシーン（最初のもの）。
    pub scene: String,
    pub reached: bool,
    /// 着いた状態の数（流れに関わる変数で見分けたもの）。
    pub states: usize,
    /// 選択肢のいちばん少ない道で選ぶ選択肢の表示文（多ければ先頭だけ）。
    pub example: Vec<String>,
    /// 着いたときの、流れに関わる変数の値（変数ごとに、出た値。多ければ先頭だけ）。
    pub values: BTreeMap<String, Vec<Value>>,
    /// 値が多くて、`values` に書ききれなかった変数。
    pub more: Vec<String>,
}

/// 自動巡回の結果。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Playthrough {
    /// 上限に当たらずに調べきったか。
    pub complete: bool,
    /// 調べきれなかったわけ。
    pub limit: Option<String>,
    /// 調べた状態の数。
    pub states: usize,
    /// 周回の始め方の数（永続の変数の値の組）。
    pub starts: usize,
    pub scenes: Coverage,
    pub options: Coverage,
    pub endings: Vec<EndingReach>,
    /// 段階2の診断（ファイルと位置の順）。
    pub diagnostics: Vec<IrDiagnostic>,
}

/// 調べた道の1区間：状態（無ければ始まり）から、選んだ選択肢の後、次の状態（選択肢）か終わりまで。
struct Segment<'a> {
    from: Option<usize>,
    /// 周回の始まりの区間なら、その周回を生んだ前の周のエンディングの区間。
    prev: Option<usize>,
    /// 選んだ選択肢の表示文（始まりの区間は無い）。
    label: Option<String>,
    /// 入ったシーン（入った順。始まりの区間は開始シーンから）。
    scenes: Vec<&'a str>,
    to: Option<usize>,
}

/// 1つの区間の実行を見る。
struct Recorder<'a, 'r> {
    scenes: Vec<&'a str>,
    branches: &'r mut BTreeMap<usize, (&'a str, &'a IrBranch, bool)>,
    endings: Vec<(&'a str, String, Variables)>,
    stuck: Vec<(&'a str, Span, &'a [IrOption])>,
}

impl<'a> Observer<'a> for Recorder<'a, '_> {
    fn entered(&mut self, scene: &'a str) {
        self.scenes.push(scene);
    }

    fn branch(&mut self, scene: &'a str, branch: &'a IrBranch, value: bool) {
        let entry = self
            .branches
            .entry(std::ptr::from_ref(branch) as usize)
            .or_insert((scene, branch, false));
        entry.2 |= value;
    }

    fn ending(&mut self, scene: &'a str, id: Option<&'a str>, vars: &Variables) {
        if let Some(id) = id {
            self.endings.push((scene, id.to_owned(), vars.clone()));
        }
    }

    fn no_option(&mut self, scene: &'a str, span: Span, options: &'a [IrOption]) {
        self.stuck.push((scene, span, options));
    }
}

/// 選択肢の行ごとの記録。
struct ChoiceStat<'a> {
    scene: &'a str,
    span: Span,
    options: &'a [IrOption],
    /// 選択肢ごとに、選べたことがあるか。
    selectable: Vec<bool>,
    /// 選べる選択肢が1つも無かったことがあるか。
    stuck: bool,
}

/// エンディングごとの記録。
#[derive(Default)]
struct EndingStat {
    states: BTreeSet<Vec<(String, Value)>>,
    example: Option<Vec<String>>,
    values: BTreeMap<String, BTreeSet<Value>>,
    more: BTreeSet<String>,
}

/// 周回の始め方。
struct Start {
    vars: Variables,
    /// この始め方を生んだエンディングの区間（最初の始め方は無い）。
    origin: Option<usize>,
    /// この始め方の始まりの区間。
    root: Option<usize>,
}

struct Explorer<'a> {
    ir: &'a Ir,
    searcher: Searcher<'a>,
    relevant: BTreeSet<String>,
    persistent: BTreeSet<String>,
    states: BTreeMap<Key, usize>,
    /// 状態を最初に見つけた区間（道の例を作る）。
    found_by: Vec<usize>,
    segments: Vec<Segment<'a>>,
    /// 状態から出る区間。
    outgoing: BTreeMap<Option<usize>, Vec<usize>>,
    branches: BTreeMap<usize, (&'a str, &'a IrBranch, bool)>,
    choices: BTreeMap<(String, usize), ChoiceStat<'a>>,
    endings: BTreeMap<String, EndingStat>,
    entered: BTreeSet<&'a str>,
    limit: Option<String>,
    starts: Vec<Start>,
    /// 永続の変数の値の組から、周回の始め方へ。
    start_of: BTreeMap<Vec<(String, Value)>, usize>,
    /// エンディングの区間から、そこから始める周回へ。
    replay: BTreeMap<usize, usize>,
}

/// 実行を待つ区間。
struct Pending<'a> {
    machine: Machine<'a>,
    from: Option<usize>,
    prev: Option<usize>,
    label: Option<String>,
}

impl<'a> Explorer<'a> {
    /// 区間の道の例（最初の始まりから、その区間で選んだ選択肢まで。周回をまたぐところに印）。
    fn path(&self, segment: usize) -> Vec<String> {
        let mut labels = Vec::new();
        let mut at = Some(segment);
        let mut steps = 0;
        while let Some(index) = at {
            // 区間の数より多くたどらない（念のため）。
            steps += 1;
            if steps > self.segments.len() {
                break;
            }
            let Some(seg) = self.segments.get(index) else {
                break;
            };
            if let Some(label) = &seg.label {
                labels.push(label.clone());
            }
            at = match (seg.from, seg.prev) {
                (Some(state), _) => self.found_by.get(state).copied(),
                (None, Some(prev)) => {
                    labels.push(REPLAY_MARK.to_owned());
                    Some(prev)
                }
                (None, None) => None,
            };
        }
        labels.reverse();
        labels
    }

    fn persistent_values(&self, vars: &Variables) -> Vec<(String, Value)> {
        vars.iter()
            .filter(|(k, _)| self.persistent.contains(*k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// 1つの始め方（`starts` の `index` 番目）から調べる。
    fn explore(&mut self, start: &'a str, index: usize) {
        let Some(Start { vars, origin, .. }) = self.starts.get(index) else {
            return;
        };
        let mut first = Machine {
            scene: "",
            frames: Vec::new(),
            calls: Vec::new(),
            vars: vars.clone(),
            steps: Vec::new(),
        };
        let prev = *origin;
        if !self.searcher.enter(&mut first, start) {
            return;
        }
        if let Some(s) = self.starts.get_mut(index) {
            s.root = Some(self.segments.len());
        }
        let mut queue = VecDeque::from([Pending {
            machine: first,
            from: None,
            prev,
            label: None,
        }]);
        while let Some(pending) = queue.pop_front() {
            if self.limit.is_some() {
                return;
            }
            let mut recorder = Recorder {
                scenes: if pending.from.is_none() {
                    vec![pending.machine.scene]
                } else {
                    Vec::new()
                },
                branches: &mut self.branches,
                endings: Vec::new(),
                stuck: Vec::new(),
            };
            let outcome = self.searcher.run(pending.machine, &mut recorder);
            let Recorder {
                scenes,
                endings,
                stuck,
                ..
            } = recorder;
            self.entered.extend(scenes.iter().copied());
            let segment = self.segments.len();
            self.segments.push(Segment {
                from: pending.from,
                prev: pending.prev,
                label: pending.label,
                scenes,
                to: None,
            });
            self.outgoing.entry(pending.from).or_default().push(segment);
            for (scene, span, options) in stuck {
                self.choice(scene, span, options).stuck = true;
            }
            for (_, id, vars) in endings {
                self.ending(segment, id, &vars);
            }
            match outcome {
                Outcome::Choice(m, span, options) => {
                    let key = m.key(&self.relevant);
                    let (state, new) = match self.states.get(&key) {
                        Some(&state) => (state, false),
                        None => {
                            if self.states.len() >= MAX_STATES {
                                self.limit = Some(format!(
                                    "分かれ道が多すぎるので、{MAX_STATES} の状態で調べるのをやめました"
                                ));
                                return;
                            }
                            let state = self.states.len();
                            self.states.insert(key, state);
                            self.found_by.push(segment);
                            (state, true)
                        }
                    };
                    if let Some(seg) = self.segments.get_mut(segment) {
                        seg.to = Some(state);
                    }
                    let selectable: Vec<bool> = options
                        .iter()
                        .map(|o| o.cond_text.is_none() || condition(o.cond.as_ref(), &m.vars))
                        .collect();
                    let stat = self.choice(m.scene, span, options);
                    for (seen, now) in stat.selectable.iter_mut().zip(&selectable) {
                        *seen |= *now;
                    }
                    if !new {
                        continue;
                    }
                    for (index, option) in options.iter().enumerate() {
                        if !selectable.get(index).copied().unwrap_or(false) {
                            continue;
                        }
                        let mut next = m.clone();
                        next.frames.push(Frame {
                            ops: &option.body,
                            index: 0,
                            after: option.jump.as_deref(),
                        });
                        queue.push_back(Pending {
                            machine: next,
                            from: Some(state),
                            prev: None,
                            label: Some(plain_text(&option.text)),
                        });
                    }
                }
                Outcome::End(EndReason::StepLimit) => {
                    self.limit = Some(
                        "選択肢から次の選択肢までの命令が多すぎる（ループの）道があるので、調べるのをやめました"
                            .to_owned(),
                    );
                    return;
                }
                Outcome::End(EndReason::CallDepth) => {
                    self.limit = Some(format!(
                        "呼び出し（@呼出）が {MAX_CALL_DEPTH} 段より深くなる（再帰の）道があるので、調べるのをやめました"
                    ));
                    return;
                }
                Outcome::End(_) | Outcome::Reached(_) => {}
            }
        }
    }

    fn choice(
        &mut self,
        scene: &'a str,
        span: Span,
        options: &'a [IrOption],
    ) -> &mut ChoiceStat<'a> {
        self.choices
            .entry((scene.to_owned(), span.start))
            .or_insert_with(|| ChoiceStat {
                scene,
                span,
                options,
                selectable: vec![false; options.len()],
                stuck: false,
            })
    }

    fn ending(&mut self, segment: usize, id: String, vars: &Variables) {
        let relevant: Vec<(String, Value)> = vars
            .iter()
            .filter(|(k, _)| self.relevant.contains(*k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        // 道の例は、最初に着いたときだけ作る（着くたびに作ると、深い道で重くなる）。
        let first = self.endings.get(&id).is_none_or(|s| s.example.is_none());
        let example = first.then(|| self.path(segment));
        let stat = self.endings.entry(id).or_default();
        stat.states.insert(relevant.clone());
        if let Some(example) = example {
            stat.example = Some(example);
        }
        for (k, v) in relevant {
            let values = stat.values.entry(k.clone()).or_default();
            if values.len() < MAX_VALUES {
                values.insert(v);
            } else if !values.contains(&v) {
                stat.more.insert(k);
            }
        }
        // 周回：着いたときの永続の変数の値から、もう一度始める（同じ値の組なら、その始め方へつなぐ）。
        let kept = self.persistent_values(vars);
        if kept.is_empty() {
            return;
        }
        if let Some(&index) = self.start_of.get(&kept) {
            self.replay.insert(segment, index);
            return;
        }
        if self.starts.len() >= MAX_STARTS {
            self.limit.get_or_insert_with(|| {
                format!("永続の変数の値の組が多すぎるので、{MAX_STARTS} 通りの周回で調べるのをやめました")
            });
            return;
        }
        let Some(base) = self.starts.first() else {
            return;
        };
        let mut vars = base.vars.clone();
        vars.extend(kept.iter().cloned());
        let index = self.starts.len();
        self.starts.push(Start {
            vars,
            origin: Some(segment),
            root: None,
        });
        self.start_of.insert(kept, index);
        self.replay.insert(segment, index);
    }

    /// 回収（`payoff`）へ、仕込み（`setup`）のどれも通らずに着ける道の例。最初の始め方から、周回も
    /// たどる（前の周で通った仕込みも、通ったに数える）。
    fn bypass(&self, setups: &BTreeSet<&str>, payoff: &BTreeSet<&str>) -> Option<Vec<String>> {
        let root = self.starts.first().and_then(|s| s.root)?;
        let mut visited_states = BTreeSet::new();
        let mut visited_starts = BTreeSet::from([0]);
        // 区間から、たどってきた前の区間（と、周回をまたいだか）。
        let mut parent: BTreeMap<usize, (usize, bool)> = BTreeMap::new();
        let mut queue = VecDeque::from([root]);
        while let Some(index) = queue.pop_front() {
            let Some(seg) = self.segments.get(index) else {
                continue;
            };
            let mut covered = false;
            for scene in &seg.scenes {
                if setups.contains(scene) {
                    covered = true;
                    break;
                }
                if payoff.contains(scene) {
                    // 道を組み立てる（たどった区間を戻る）。
                    let mut labels = Vec::new();
                    let mut at = Some(index);
                    while let Some(i) = at {
                        if let Some(label) = self.segments.get(i).and_then(|s| s.label.as_ref()) {
                            labels.push(label.clone());
                        }
                        at = parent.get(&i).map(|&(p, replay)| {
                            if replay {
                                labels.push(REPLAY_MARK.to_owned());
                            }
                            p
                        });
                    }
                    labels.reverse();
                    return Some(labels);
                }
            }
            if covered {
                continue;
            }
            if let Some(to) = seg.to
                && visited_states.insert(to)
            {
                for &next in self.outgoing.get(&Some(to)).into_iter().flatten() {
                    parent.entry(next).or_insert((index, false));
                    queue.push_back(next);
                }
            }
            if let Some(&start) = self.replay.get(&index)
                && visited_starts.insert(start)
                && let Some(next) = self.starts.get(start).and_then(|s| s.root)
            {
                parent.entry(next).or_insert((index, true));
                queue.push_back(next);
            }
        }
        None
    }
}

fn diag(out: &mut Vec<IrDiagnostic>, code: Code, file: &str, span: Span, message: String) {
    out.push(IrDiagnostic {
        code: code.as_str(),
        severity: code.severity().as_str(),
        file: file.to_owned(),
        span,
        message,
    });
}

/// 選択肢の表示文を「」で囲む（もう「」で囲まれた台詞の形と、周回の印なら、そのまま）。
pub fn quote(label: &str) -> String {
    if label == REPLAY_MARK || (label.starts_with('「') && label.ends_with('」')) {
        label.to_owned()
    } else {
        format!("「{label}」")
    }
}

/// 道の例の文（選択肢の表示文を → でつなぐ）。
pub fn example_text(labels: &[String]) -> String {
    if labels.is_empty() {
        return "選択肢を選ばずに".to_owned();
    }
    let mut shown: Vec<String> = labels.iter().take(MAX_EXAMPLE).map(|l| quote(l)).collect();
    if labels.len() > MAX_EXAMPLE {
        shown.push("…".to_owned());
    }
    format!("選択肢 {}", shown.join(" → "))
}

/// 本文の `@エンディング`（ID と、最初に書かれたシーン）と、選択肢の数。
fn endings_and_options(ir: &Ir) -> (BTreeMap<String, String>, usize) {
    fn walk(ops: &[Op], scene: &str, endings: &mut BTreeMap<String, String>, options: &mut usize) {
        for op in ops {
            match op {
                Op::Ending { id: Some(id), .. } => {
                    endings
                        .entry(id.clone())
                        .or_insert_with(|| scene.to_owned());
                }
                Op::Choice { options: opts, .. } => {
                    *options += opts.len();
                    for option in opts {
                        walk(&option.body, scene, endings, options);
                    }
                }
                Op::If {
                    branches,
                    else_body,
                    ..
                } => {
                    for branch in branches {
                        walk(&branch.body, scene, endings, options);
                    }
                    if let Some(body) = else_body {
                        walk(body, scene, endings, options);
                    }
                }
                _ => {}
            }
        }
    }
    let mut endings = BTreeMap::new();
    let mut options = 0;
    for scene in &ir.scenes {
        walk(&scene.ops, &scene.id, &mut endings, &mut options);
    }
    (endings, options)
}

/// 伏線の仕込みと回収のシーンID（文字列か、文字列のリスト）。
fn scene_ids(value: Option<&serde_json::Value>) -> BTreeSet<&str> {
    match value {
        Some(serde_json::Value::String(id)) if !id.is_empty() => BTreeSet::from([id.as_str()]),
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .collect(),
        _ => BTreeSet::new(),
    }
}

/// 自動巡回を行う。
pub fn playthrough(project: &Project, ir: &Ir) -> Playthrough {
    let (ending_scenes, total_options) = endings_and_options(ir);
    let scene_ids_all: BTreeSet<&str> = ir.scenes.iter().map(|s| s.id.as_str()).collect();
    let start = ir
        .start_scene
        .as_deref()
        .filter(|s| scene_ids_all.contains(s))
        .or_else(|| ir.scenes.first().map(|s| s.id.as_str()));
    let relevant = relevant_variables(ir);
    let persistent: BTreeSet<String> = project
        .variables
        .iter()
        .filter(|v| v.scope.as_deref() == Some("persistent") && relevant.contains(&v.key))
        .map(|v| v.key.clone())
        .collect();
    let mut explorer = Explorer {
        ir,
        searcher: Searcher {
            ir,
            project,
            target: None,
        },
        relevant,
        persistent,
        states: BTreeMap::new(),
        found_by: Vec::new(),
        segments: Vec::new(),
        outgoing: BTreeMap::new(),
        branches: BTreeMap::new(),
        choices: BTreeMap::new(),
        endings: BTreeMap::new(),
        entered: BTreeSet::new(),
        limit: None,
        starts: vec![Start {
            vars: initial_variables(project, &BTreeMap::new()),
            origin: None,
            root: None,
        }],
        start_of: BTreeMap::new(),
        replay: BTreeMap::new(),
    };
    if let Some(start) = start {
        let kept = explorer
            .starts
            .first()
            .map(|s| explorer.persistent_values(&s.vars))
            .unwrap_or_default();
        explorer.start_of.insert(kept, 0);
        let mut next = 0;
        while next < explorer.starts.len() {
            explorer.explore(start, next);
            next += 1;
            if explorer.limit.is_some() {
                break;
            }
        }
    }
    let complete = explorer.limit.is_none();
    let mut diagnostics = Vec::new();
    let file_of = |scene: &str| {
        explorer
            .ir
            .scenes
            .iter()
            .find(|s| s.id == scene)
            .map(|s| s.file.clone())
            .unwrap_or_default()
    };
    // 選択肢（W506・W508）。
    for stat in explorer.choices.values() {
        let file = file_of(stat.scene);
        if stat.stuck {
            diag(
                &mut diagnostics,
                Code::W508,
                &file,
                stat.span,
                "条件のせいで、選べる選択肢が1つも無くなる道があります（そこで止まります）"
                    .to_owned(),
            );
        }
        if !complete {
            continue;
        }
        for (option, seen) in stat.options.iter().zip(&stat.selectable) {
            // 構文の崩れた条件（E601）には出さない。
            if option.cond_text.is_some() && option.cond.is_some() && !seen {
                diag(
                    &mut diagnostics,
                    Code::W506,
                    &file,
                    option.span,
                    format!(
                        "この選択肢は、条件（{}）が真にならないので選べません",
                        option.cond_text.as_deref().unwrap_or_default()
                    ),
                );
            }
        }
    }
    if complete {
        // 条件（W507）。
        for (scene, branch, ever) in explorer.branches.values() {
            // 構文の崩れた条件（E601）には出さない。
            if !ever && branch.cond.is_some() {
                diag(
                    &mut diagnostics,
                    Code::W507,
                    &file_of(scene),
                    branch.span,
                    format!(
                        "条件（{}）は、ここに着いても真になりません",
                        branch.cond_text
                    ),
                );
            }
        }
        // シーン（W509）：段階1の辺で着けるが着かないシーンのうち、着いたシーンからの辺があるもの。
        let mut graph_reached: BTreeSet<&str> = BTreeSet::new();
        if let Some(start) = start {
            let mut queue = VecDeque::from([start]);
            graph_reached.insert(start);
            while let Some(scene) = queue.pop_front() {
                for edge in ir.edges.iter().filter(|e| e.from == scene) {
                    if scene_ids_all.contains(edge.to.as_str()) && graph_reached.insert(&edge.to) {
                        queue.push_back(&edge.to);
                    }
                }
            }
        }
        let mut reported = BTreeSet::new();
        for edge in &ir.edges {
            let to = edge.to.as_str();
            if explorer.entered.contains(edge.from.as_str())
                && graph_reached.contains(to)
                && !explorer.entered.contains(to)
                && reported.insert(to)
                && let Some(scene) = ir.scenes.iter().find(|s| s.id == to)
            {
                diag(
                    &mut diagnostics,
                    Code::W509,
                    &scene.file,
                    scene.span,
                    format!(
                        "シーン {to} には、条件と選択肢をどう選んでも着きません（{} からの辺が実行されません）",
                        edge.from
                    ),
                );
            }
        }
    }
    // 伏線（W704）。
    for entry in project
        .bible
        .entries
        .iter()
        .filter(|e| e.type_id == FORESHADOW_TYPE)
    {
        // 無いシーン（W806・W703）は除いて調べる。
        let exists = |id: &&str| scene_ids_all.contains(id);
        let setups: BTreeSet<&str> = scene_ids(entry.fields.get("setup"))
            .into_iter()
            .filter(exists)
            .collect();
        let payoff: BTreeSet<&str> = scene_ids(entry.fields.get("payoff"))
            .into_iter()
            .filter(exists)
            .collect();
        if setups.is_empty() || payoff.is_empty() {
            continue;
        }
        if let Some(labels) = explorer.bypass(&setups, &payoff) {
            let span = entry
                .key_spans
                .get("payoff")
                .copied()
                .unwrap_or(Span::new(0, 0));
            diag(
                &mut diagnostics,
                Code::W704,
                &entry.path,
                span,
                format!(
                    "伏線 {} の回収（{}）に、仕込み（{}）を通らずに着く道があります（{}）",
                    entry.id,
                    payoff.iter().copied().collect::<Vec<_>>().join("・"),
                    setups.iter().copied().collect::<Vec<_>>().join("・"),
                    example_text(&labels)
                ),
            );
        }
    }
    diagnostics
        .sort_by(|a, b| (&a.file, a.span.start, a.code).cmp(&(&b.file, b.span.start, b.code)));
    let endings = ending_scenes
        .into_iter()
        .map(|(id, scene)| {
            let stat = explorer.endings.get(&id);
            EndingReach {
                reached: stat.is_some(),
                states: stat.map_or(0, |s| s.states.len()),
                example: stat.and_then(|s| s.example.clone()).unwrap_or_default(),
                values: stat
                    .map(|s| {
                        s.values
                            .iter()
                            .map(|(k, v)| (k.clone(), v.iter().cloned().collect()))
                            .collect()
                    })
                    .unwrap_or_default(),
                more: stat
                    .map(|s| s.more.iter().cloned().collect())
                    .unwrap_or_default(),
                id,
                scene,
            }
        })
        .collect();
    let selectable = explorer
        .choices
        .values()
        .map(|c| c.selectable.iter().filter(|s| **s).count())
        .sum();
    Playthrough {
        complete,
        limit: explorer.limit.clone(),
        states: explorer.states.len(),
        starts: explorer.starts.len(),
        scenes: Coverage {
            reached: explorer.entered.len(),
            total: scene_ids_all.len(),
        },
        options: Coverage {
            reached: selectable,
            total: total_options,
        },
        endings,
        diagnostics,
    }
}

/// 検証の段階2のすべて：自動巡回と、表記の検査（[`crate::wording`]。T3-3）。表記の診断は自動巡回の結果の
/// 診断に足す（`scn check --stage 2` とアプリの検証の画面）。
pub fn stage2(project: &Project, ir: &Ir) -> Playthrough {
    let mut report = playthrough(project, ir);
    report
        .diagnostics
        .extend(crate::wording::wording(project, ir));
    report
        .diagnostics
        .sort_by(|a, b| (&a.file, a.span.start, a.code).cmp(&(&b.file, b.span.start, b.code)));
    report
}
