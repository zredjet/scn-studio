//! 検証の段階1（T1-4、仕様書16節、D043）：`scn check` がプロジェクト全体で行う検証。
//!
//! IRと索引とプロジェクトの宣言から、メッセージウィンドウからのはみ出し（W203）、行き止まり（W503）、
//! 開始シーンから着けないシーン（W504）、エンディングに着かないシーン（W505）、書き込むだけ・読まれるだけの
//! 変数（W605・W606）、企画書とのエンディングの食い違い（W701・W702）、回収の無い伏線（W703）を出す。
//! 条件は見ず、辺（ジャンプと呼び出し）だけをたどる（`docs/design.md` の「到達性解析の進め方」の段階1）。

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::diag::{Code, Span};
use crate::index::Index;
use crate::ir::{Ir, IrDiagnostic, IrInline, IrScene, Op};
use crate::project::{MessageWindow, Project};

/// 伏線の型のID（`bible/types/foreshadow.yaml`。D043）。
const FORESHADOW_TYPE: &str = "foreshadow";

/// 検証の段階1の診断を返す。ファイルと位置の順に並べる。
pub fn check(project: &Project, ir: &Ir, index: &Index) -> Vec<IrDiagnostic> {
    let mut checker = Checker { diags: Vec::new() };
    checker.graph(project, ir);
    if let Some(window) = project.settings.message_window {
        for scene in &ir.scenes {
            checker.overflow(window, &scene.file, &scene.ops);
        }
    }
    checker.variables(project, index);
    checker.endings(project, ir);
    checker.foreshadows(project, ir);
    let mut diags = checker.diags;
    diags.sort_by(|a, b| (&a.file, a.span.start).cmp(&(&b.file, b.span.start)));
    diags
}

struct Checker {
    diags: Vec<IrDiagnostic>,
}

impl Checker {
    fn diag(&mut self, code: Code, file: &str, span: Span, message: String) {
        self.diags.push(IrDiagnostic {
            code: code.as_str(),
            severity: code.severity().as_str(),
            file: file.to_owned(),
            span,
            message,
        });
    }

    // ---- シーンのつながり（W503・W504・W505） ----

    fn graph(&mut self, project: &Project, ir: &Ir) {
        let scenes: BTreeMap<&str, &IrScene> =
            ir.scenes.iter().map(|s| (s.id.as_str(), s)).collect();
        let mut next: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        let mut prev: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        // ジャンプの辺だけを逆にたどる表（呼ばれる側が `@戻る` に着くかを見る）。
        let mut prev_jump: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        let mut broken_edge = BTreeSet::new();
        for edge in &ir.edges {
            if scenes.contains_key(edge.to.as_str()) {
                next.entry(&edge.from).or_default().push(&edge.to);
                prev.entry(&edge.to).or_default().push(&edge.from);
                if edge.kind == "jump" {
                    prev_jump.entry(&edge.to).or_default().push(&edge.from);
                }
            } else {
                broken_edge.insert(edge.from.as_str());
            }
        }

        // W503：出口の無い経路があるシーン（仕様書13節）。
        let mut dead_ends = BTreeSet::new();
        for scene in &ir.scenes {
            if falls_through(&scene.ops) {
                dead_ends.insert(scene.id.as_str());
                self.diag(
                    Code::W503,
                    &scene.file,
                    scene.span,
                    format!(
                        "シーン {} の終わりに出口の無い経路があります（-> シーンID・@エンディング・@戻る で終えます）",
                        scene.id
                    ),
                );
            }
        }

        // W504：開始シーンから着けないシーン。開始シーンが無ければ、最初のシーンから数える。
        if let Some(id) = &project.settings.start_scene
            && !scenes.contains_key(id.as_str())
        {
            let fallback = ir
                .scenes
                .first()
                .map(|s| format!("（最初のシーン {} から数えます）", s.id))
                .unwrap_or_default();
            self.diag(
                Code::E501,
                "project.toml",
                project.settings.start_scene_span,
                format!("start_scene のシーン {id} がありません{fallback}"),
            );
        }
        let Some(first) = ir.scenes.first() else {
            return;
        };
        let start = match &project.settings.start_scene {
            Some(id) if scenes.contains_key(id.as_str()) => id.as_str(),
            _ => first.id.as_str(),
        };
        let reachable = walk([start], &next);
        for scene in &ir.scenes {
            if !reachable.contains(scene.id.as_str()) {
                self.diag(
                    Code::W504,
                    &scene.file,
                    scene.span,
                    format!(
                        "シーン {} には、開始シーン {start} から着けません",
                        scene.id
                    ),
                );
            }
        }

        // W505：どのエンディングにも着かないシーン。エンディング・行き止まり・無いシーンへの辺に
        // （ジャンプと呼び出しの辺をたどって）続くシーンには出さない（行き止まりなどは、それぞれの診断で分かる）。
        // `@戻る` で終える呼ばれる側のシーン（ジャンプをたどって `@戻る` に着くシーン）にも出さない
        // （呼び出しを挟んだ循環は、呼ぶ側に出る）。
        let sinks = ir.scenes.iter().map(|s| s.id.as_str()).filter(|id| {
            dead_ends.contains(id)
                || broken_edge.contains(id)
                || scenes.get(id).is_some_and(|s| has_op(&s.ops, &is_ending))
        });
        let good = walk(sinks, &prev);
        // 呼ばれる側：ジャンプの辺だけをたどって `@戻る` を含むシーンに着くシーン。
        let returners = walk(
            ir.scenes
                .iter()
                .filter(|s| has_op(&s.ops, &is_return))
                .map(|s| s.id.as_str()),
            &prev_jump,
        );
        for scene in &ir.scenes {
            if !good.contains(scene.id.as_str()) && !returners.contains(scene.id.as_str()) {
                self.diag(
                    Code::W505,
                    &scene.file,
                    scene.span,
                    format!(
                        "シーン {} からは、どのエンディングにも着きません（ジャンプが循環しています）",
                        scene.id
                    ),
                );
            }
        }
    }

    // ---- メッセージウィンドウ（W203） ----

    fn overflow(&mut self, window: MessageWindow, file: &str, ops: &[Op]) {
        for op in ops {
            match op {
                Op::Say { span, text, .. } | Op::Narrate { span, text, .. } => {
                    let lines = max_page_lines(text, window.chars_per_line);
                    if lines > window.lines {
                        self.diag(
                            Code::W203,
                            file,
                            *span,
                            format!(
                                "メッセージウィンドウ（{}行×{}字）に収まりません（{lines}行になります）",
                                window.lines, window.chars_per_line
                            ),
                        );
                    }
                }
                Op::Choice { options, .. } => {
                    for option in options {
                        self.overflow(window, file, &option.body);
                    }
                }
                Op::If {
                    branches,
                    else_body,
                    ..
                } => {
                    for branch in branches {
                        self.overflow(window, file, &branch.body);
                    }
                    if let Some(body) = else_body {
                        self.overflow(window, file, body);
                    }
                }
                _ => {}
            }
        }
    }

    // ---- 変数（W605・W606） ----

    fn variables(&mut self, project: &Project, index: &Index) {
        for decl in &project.variables {
            let Some(var) = index.variables.iter().find(|v| v.key == decl.key) else {
                continue;
            };
            if !var.writes.is_empty() && var.reads.is_empty() {
                self.diag(
                    Code::W605,
                    "variables.yaml",
                    decl.span,
                    format!("変数 {} は書き込まれますが、どこでも読まれません", decl.key),
                );
            }
            // `persistent`（周回など）は、ゲームの側で書く想定なので出さない（D043）。
            if var.writes.is_empty()
                && !var.reads.is_empty()
                && decl.scope.as_deref() != Some("persistent")
            {
                self.diag(
                    Code::W606,
                    "variables.yaml",
                    decl.span,
                    format!(
                        "変数 {} は読まれますが、どこでも書き込まれません（既定値のまま使われます）",
                        decl.key
                    ),
                );
            }
        }
    }

    // ---- 企画書のエンディング（W701・W702） ----

    fn endings(&mut self, project: &Project, ir: &Ir) {
        let Some(concept_endings) = project.concept.as_ref().and_then(|c| c.endings.as_ref())
        else {
            return;
        };
        let mut body: Vec<(&str, &str, Span)> = Vec::new();
        for scene in &ir.scenes {
            collect_endings(&scene.file, &scene.ops, &mut body);
        }
        let planned: BTreeSet<&str> = concept_endings.iter().map(|e| e.id.as_str()).collect();
        let written: BTreeSet<&str> = body.iter().map(|(id, _, _)| *id).collect();
        for ending in concept_endings {
            if !written.contains(ending.id.as_str()) {
                self.diag(
                    Code::W701,
                    "concept.md",
                    ending.span,
                    format!(
                        "企画書のエンディング {} が、本文の @エンディング にありません",
                        ending.id
                    ),
                );
            }
        }
        for (id, file, span) in body {
            if !planned.contains(id) {
                self.diag(
                    Code::W702,
                    file,
                    span,
                    format!("エンディング {id} が、企画書（concept.md）の endings にありません"),
                );
            }
        }
    }

    // ---- 伏線（W703） ----

    fn foreshadows(&mut self, project: &Project, ir: &Ir) {
        let scenes: BTreeSet<&str> = ir.scenes.iter().map(|s| s.id.as_str()).collect();
        for entry in project
            .bible
            .entries
            .iter()
            .filter(|e| e.type_id == FORESHADOW_TYPE)
        {
            let span = entry
                .key_spans
                .get("payoff")
                .copied()
                .unwrap_or(Span::new(0, 0));
            let targets: Option<Vec<&str>> = match entry.fields.get("payoff") {
                None | Some(serde_json::Value::Null) => Some(Vec::new()),
                Some(serde_json::Value::String(id)) if id.is_empty() => Some(Vec::new()),
                Some(serde_json::Value::String(id)) => Some(vec![id.as_str()]),
                Some(serde_json::Value::Array(items)) => items.iter().map(|i| i.as_str()).collect(),
                Some(_) => None,
            };
            let message = match targets {
                None => format!("伏線 {} の回収（payoff）は、シーンIDで書きます", entry.id),
                Some(ids) if ids.is_empty() => {
                    format!("伏線 {} に回収（payoff）がありません", entry.id)
                }
                Some(ids) => match ids.iter().find(|id| !scenes.contains(**id)) {
                    Some(missing) => {
                        format!("伏線 {} の回収のシーン {missing} がありません", entry.id)
                    }
                    None => continue,
                },
            };
            self.diag(Code::W703, &entry.path, span, message);
        }
    }
}

/// 出口（ジャンプ・`@エンディング`・`@戻る`）に当たらずに、終わりまで進む経路があるか。
fn falls_through(ops: &[Op]) -> bool {
    !ops.iter().any(terminates)
}

/// この命令を通る経路が、どれも出口に当たるか。
fn terminates(op: &Op) -> bool {
    match op {
        Op::Jump { .. } | Op::Ending { .. } | Op::Return { .. } => true,
        Op::Choice { options, .. } => {
            !options.is_empty()
                && options
                    .iter()
                    .all(|o| o.jump.is_some() || !falls_through(&o.body))
        }
        Op::If {
            branches,
            else_body,
            ..
        } => {
            else_body.as_ref().is_some_and(|body| !falls_through(body))
                && branches.iter().all(|b| !falls_through(&b.body))
        }
        _ => false,
    }
}

fn is_ending(op: &Op) -> bool {
    matches!(op, Op::Ending { .. })
}

fn is_return(op: &Op) -> bool {
    matches!(op, Op::Return { .. })
}

/// 条件に合う命令を（どこかの枝に）含むか。
fn has_op(ops: &[Op], pred: &dyn Fn(&Op) -> bool) -> bool {
    ops.iter().any(|op| {
        pred(op)
            || match op {
                Op::Choice { options, .. } => options.iter().any(|o| has_op(&o.body, pred)),
                Op::If {
                    branches,
                    else_body,
                    ..
                } => {
                    branches.iter().any(|b| has_op(&b.body, pred))
                        || else_body.as_ref().is_some_and(|b| has_op(b, pred))
                }
                _ => false,
            }
    })
}

/// `@エンディング` のID（形の合うもの）と位置を集める。
fn collect_endings<'a>(file: &'a str, ops: &'a [Op], out: &mut Vec<(&'a str, &'a str, Span)>) {
    for op in ops {
        match op {
            Op::Ending { span, id: Some(id) } => out.push((id, file, *span)),
            Op::Choice { options, .. } => {
                for option in options {
                    collect_endings(file, &option.body, out);
                }
            }
            Op::If {
                branches,
                else_body,
                ..
            } => {
                for branch in branches {
                    collect_endings(file, &branch.body, out);
                }
                if let Some(body) = else_body {
                    collect_endings(file, body, out);
                }
            }
            _ => {}
        }
    }
}

/// 辺をたどって着けるシーン。
fn walk<'a>(
    from: impl IntoIterator<Item = &'a str>,
    edges: &BTreeMap<&'a str, Vec<&'a str>>,
) -> BTreeSet<&'a str> {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut queue: VecDeque<&str> = VecDeque::new();
    for id in from {
        if seen.insert(id) {
            queue.push_back(id);
        }
    }
    while let Some(id) = queue.pop_front() {
        for to in edges.get(id).into_iter().flatten() {
            if seen.insert(to) {
                queue.push_back(to);
            }
        }
    }
    seen
}

/// 文字の幅（半角を1、全角を2として数える）。半角はASCIIの印字できる文字と、半角カタカナ。
fn width(c: char) -> u32 {
    match c {
        ' '..='~' | '\u{FF61}'..='\u{FF9F}' => 1,
        _ => 2,
    }
}

/// 台詞・地の文を、1行の字数で折り返したときの、いちばん多いページの行数（D043）。
/// 表示する文字だけを数える（ルビは親文字、傍点は中身、インライン命令と変数の埋め込みは0）。
/// 複数行の台詞の改行で行を改め、`{p}` でページを改める。各ページの最初と最後の空の行は数えない
/// （台詞の括弧だけの行や、`{p}` の隣の改行は、何も表示しないため）。途中の空の行は1行に数える。
/// 禁則処理は考えない。
fn max_page_lines(text: &[IrInline], chars_per_line: u32) -> u32 {
    let per_line = chars_per_line.saturating_mul(2).max(1);
    // ページごとの、行ごとの幅。
    let mut pages: Vec<Vec<u32>> = vec![vec![0]];
    for inline in text {
        let added = match inline {
            IrInline::Text { text } | IrInline::Emphasis { text } => text.chars().map(width).sum(),
            IrInline::Ruby { base, .. } => base.chars().map(width).sum(),
            IrInline::LineBreak => {
                if let Some(page) = pages.last_mut() {
                    page.push(0);
                }
                continue;
            }
            IrInline::PageBreak => {
                pages.push(vec![0]);
                continue;
            }
            IrInline::Wait { .. }
            | IrInline::Se { .. }
            | IrInline::Speed { .. }
            | IrInline::Interpolation { .. } => 0,
        };
        if let Some(line) = pages.last_mut().and_then(|page| page.last_mut()) {
            *line += added;
        }
    }
    pages
        .iter()
        .map(|page| {
            let mut lines = page.as_slice();
            if let [0, rest @ ..] = lines
                && !rest.is_empty()
            {
                lines = rest;
            }
            if let [rest @ .., 0] = lines
                && !rest.is_empty()
            {
                lines = rest;
            }
            lines
                .iter()
                .map(|w| w.div_ceil(per_line).max(1))
                .sum::<u32>()
        })
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> IrInline {
        IrInline::Text { text: s.into() }
    }

    #[test]
    fn page_lines() {
        let lb = || IrInline::LineBreak;
        let pb = || IrInline::PageBreak;
        assert_eq!(max_page_lines(&[text("一二三")], 3), 1);
        assert_eq!(max_page_lines(&[text("一二三四")], 3), 2);
        assert_eq!(max_page_lines(&[text("abcdef")], 3), 1);
        assert_eq!(max_page_lines(&[text("abcdefg")], 3), 2);
        assert_eq!(max_page_lines(&[], 3), 1);
        // 途中の空の行は数える。
        assert_eq!(max_page_lines(&[text("一"), lb(), lb(), text("二")], 3), 3);
        assert_eq!(
            max_page_lines(&[text("一二三四"), pb(), text("一二三四五六七")], 3),
            3
        );
        // `{p}` の直前と直後の改行は、空の行を作らない（各ページ3行）。
        let three = |a, b, c| vec![text(a), lb(), text(b), lb(), text(c)];
        let mut after = three("一", "二", "三");
        after.extend([pb(), lb()]);
        after.extend(three("四", "五", "六"));
        assert_eq!(max_page_lines(&after, 3), 3);
        let mut before = three("一", "二", "三");
        before.extend([lb(), pb()]);
        before.extend(three("四", "五", "六"));
        assert_eq!(max_page_lines(&before, 3), 3);
        assert_eq!(
            max_page_lines(&[text("一"), lb(), lb(), pb(), text("二")], 3),
            2
        );
        // 台詞の括弧だけの最初と最後の行は数えない。
        assert_eq!(
            max_page_lines(&[lb(), text("一"), lb(), text("二"), lb()], 3),
            2
        );
        assert_eq!(
            max_page_lines(&[lb(), IrInline::Wait { seconds: 1.0 }, text("一")], 3),
            1
        );
        let ruby = IrInline::Ruby {
            base: "漢字".into(),
            reading: "かんじかんじかんじ".into(),
        };
        assert_eq!(max_page_lines(&[ruby, text("一")], 3), 1);
        assert_eq!(max_page_lines(&[text("一")], 0), 2);
    }
}
