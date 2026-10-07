//! 構造の一覧（`scn graph`、T1-5）：シーン、辺、条件、変数の読み書きを、粒度（シーン・章・ルート）ごとに
//! まとめる。IRと索引から導く。
//!
//! 章とルートの粒度では、まとまりの外へ出る辺だけを並べる（中の辺は、シーンの粒度で見る）。

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::index::Index;
use crate::ir::{Ir, IrScene, Op};

/// まとめる粒度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// シーンごと。
    Scene,
    /// 章（`.scn` のファイル）ごと。
    Chapter,
    /// ルート（メタ情報の `route`）ごと。
    Route,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Graph {
    pub level: Level,
    /// まとまり。シーンはIRの順、章とルートは最初のシーンが現れる順。
    pub groups: Vec<Group>,
}

/// 1つのまとまり（シーン、章、ルート）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Group {
    /// シーンID、章のファイル（`story/chapter01.scn`）、ルートの名前。ルートの無いシーンのまとまりは `None`。
    pub id: Option<String>,
    /// シーンの見出し（シーンの粒度だけ）。
    pub title: Option<String>,
    /// シーンのファイル（シーンの粒度だけ）。
    pub file: Option<String>,
    /// シーンのルート（シーンの粒度だけ）。
    pub route: Option<String>,
    /// シーンの状態（シーンの粒度だけ）。
    pub status: Option<String>,
    /// スタブか（シーンの粒度だけ）。
    pub is_stub: Option<bool>,
    /// まとまりの中のシーンのID。IRの順。
    pub scenes: Vec<String>,
    /// まとまりの外へ出る辺（シーンの粒度では、シーンから出る辺すべて）。IRの順。
    pub exits: Vec<GraphEdge>,
    /// まとまりの中の `@エンディング` のID。現れる順。
    pub endings: Vec<String>,
    /// 読む変数（そろえた名前の順）。
    pub reads: Vec<String>,
    /// 書く変数（そろえた名前の順）。
    pub writes: Vec<String>,
}

/// 辺（IRの辺に、行き先のまとまりを足したもの）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    /// `jump`・`call`。
    pub kind: &'static str,
    /// `direct`・`choice`・`if`・`else`。
    pub via: &'static str,
    /// 選択肢の表示文。
    pub label: Option<String>,
    /// 条件の文字列。
    pub cond_text: Option<String>,
    /// 行き先のシーンがあるか（無ければ E501）。
    pub to_exists: bool,
    /// 行き先のまとまり（行き先が無ければ `None`。ルートの粒度で、ルートの無いシーンも `None`）。
    pub to_group: Option<String>,
}

/// IRと索引から、構造の一覧を作る。
pub fn build_graph(ir: &Ir, index: &Index, level: Level) -> Graph {
    let scenes: BTreeMap<&str, &IrScene> = ir.scenes.iter().map(|s| (s.id.as_str(), s)).collect();
    let group_of = |scene: &IrScene| -> Option<String> {
        match level {
            Level::Scene => Some(scene.id.clone()),
            Level::Chapter => Some(scene.file.clone()),
            Level::Route => scene.meta.route.clone(),
        }
    };

    // まとまりの並び（最初のシーンが現れる順）と、中のシーン。
    let mut order: Vec<Option<String>> = Vec::new();
    let mut members: BTreeMap<Option<String>, Vec<&IrScene>> = BTreeMap::new();
    for scene in &ir.scenes {
        let key = group_of(scene);
        if !members.contains_key(&key) {
            order.push(key.clone());
        }
        members.entry(key).or_default().push(scene);
    }

    // 変数の読み書きを、シーンごとに集める。
    let mut reads: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    let mut writes: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for var in &index.variables {
        for location in &var.reads {
            reads.entry(&location.scene).or_default().insert(&var.key);
        }
        for location in &var.writes {
            writes.entry(&location.scene).or_default().insert(&var.key);
        }
    }

    let groups = order
        .into_iter()
        .map(|key| {
            let list = members.get(&key).map(Vec::as_slice).unwrap_or_default();
            let ids: BTreeSet<&str> = list.iter().map(|s| s.id.as_str()).collect();
            let exits = ir
                .edges
                .iter()
                .filter(|e| ids.contains(e.from.as_str()))
                .filter_map(|e| {
                    let target = scenes.get(e.to.as_str());
                    let to_group = target.and_then(|t| group_of(t));
                    // 章とルートの粒度では、まとまりの中の辺は並べない。
                    if level != Level::Scene && target.is_some() && to_group == key {
                        return None;
                    }
                    Some(GraphEdge {
                        from: e.from.clone(),
                        to: e.to.clone(),
                        kind: e.kind,
                        via: e.via,
                        label: e.label.clone(),
                        cond_text: e.cond_text.clone(),
                        to_exists: target.is_some(),
                        to_group,
                    })
                })
                .collect();
            let mut endings = Vec::new();
            for scene in list {
                collect_endings(&scene.ops, &mut endings);
            }
            let union = |table: &BTreeMap<&str, BTreeSet<&str>>| -> Vec<String> {
                let set: BTreeSet<&str> = ids
                    .iter()
                    .filter_map(|id| table.get(id))
                    .flatten()
                    .copied()
                    .collect();
                set.into_iter().map(str::to_owned).collect()
            };
            let single = match (level, list) {
                (Level::Scene, [scene]) => Some(*scene),
                _ => None,
            };
            Group {
                id: key,
                title: single.map(|s| s.title.clone()),
                file: single.map(|s| s.file.clone()),
                route: single.and_then(|s| s.meta.route.clone()),
                status: single.and_then(|s| s.meta.status.clone()),
                is_stub: single.map(|s| s.is_stub),
                scenes: list.iter().map(|s| s.id.clone()).collect(),
                exits,
                endings,
                reads: union(&reads),
                writes: union(&writes),
            }
        })
        .collect();
    Graph { level, groups }
}

/// `@エンディング` のID（形の合うもの）を、重ねずに集める。
fn collect_endings(ops: &[Op], out: &mut Vec<String>) {
    for op in ops {
        match op {
            Op::Ending { id: Some(id), .. } => {
                if !out.contains(id) {
                    out.push(id.clone());
                }
            }
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
