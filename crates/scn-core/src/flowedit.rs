//! フロー図からの編集（T2-9、D060）：辺を引いたときに、本文にジャンプか選択肢を書き足す中身を作る。スタブの
//! シーンを足すのは `ideas::stub_scene` と同じ（`workspace::Workspace::add_stub`）。
//!
//! - どれも書き足しだけで、人が書いた行は1バイトも変えない（設計の原則6の考え）。
//! - ジャンプ：シーンの最後の中身の行（空行とコメントの行を除く）の後ろに `-> 行き先` を足す。シーンに行き先の
//!   決まった文（ジャンプ、`@エンディング`、`@戻る`、どの枝もそうなる選択肢と条件分岐。`check` の流れと同じ）が
//!   あれば、その後ろは実行されないので断る（選択肢として足させる）。最後の文が閉じていないブロック（E103）でも、
//!   中に入ってしまうので断る。
//! - 選択肢：シーンの最後が選択ブロックなら、その `@end` の前に `- 表示文 -> 行き先` を足す（字下げは最初の選択肢
//!   にそろえる）。そうでなければ、最後の中身の行の後ろに `@選択`・選択肢・`@end` のブロックを足す（シーンが行き先
//!   の決まった終わり方なら断る）。
//! - 改行は、そのファイルの改行にそろえる。

use serde::Deserialize;

use crate::ast::{DirectiveKind, Scene, Stmt};
use crate::project::Project;
use crate::syntax::parse;

/// フロー図で引いた辺。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FlowEdge {
    /// ジャンプを足す。
    Jump { from: String, to: String },
    /// 選択肢を足す。
    Choice {
        from: String,
        to: String,
        label: String,
    },
}

/// 書き足した結果：ファイルと、書き足した後の中身。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowInsert {
    pub file: String,
    pub text: String,
    /// 書き足した範囲（バイト位置）。
    pub inserted: (usize, usize),
}

/// 文の後ろへは進めない（行き先が決まっている）か。枝の中のどこかに行き先の決まった文があれば、その枝は出ていく
/// （仕様書13節。`check` の `terminates` と同じ）。
fn terminal(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Jump(_) => true,
        Stmt::Directive(d) => {
            matches!(d.kind, DirectiveKind::Ending { .. } | DirectiveKind::Return)
        }
        Stmt::Choice(c) => {
            !c.options.is_empty()
                && c.options
                    .iter()
                    .all(|o| o.jump.is_some() || o.body.iter().any(terminal))
        }
        Stmt::If(block) => {
            block
                .else_branch
                .as_ref()
                .is_some_and(|e| e.body.iter().any(terminal))
                && block.branches.iter().all(|b| b.body.iter().any(terminal))
        }
        _ => false,
    }
}

/// 閉じていないブロック（E103）か。
fn unclosed(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Choice(c) => c.end.is_none(),
        Stmt::If(b) => b.end.is_none(),
        Stmt::Raw(r) => r.end.is_none(),
        _ => false,
    }
}

fn newline_of(text: &str) -> &'static str {
    match text.find('\n') {
        Some(at) if at > 0 && text.as_bytes().get(at - 1) == Some(&b'\r') => "\r\n",
        _ => "\n",
    }
}

/// シーンの最後の中身の行（空行を除く）の終わり（改行の後ろ）。`end` は次のシーンの見出しの始まり（無ければ
/// ファイルの終わり）。
fn after_last_line(text: &str, scene: &Scene, end: usize) -> usize {
    let region = text.get(scene.span.start..end).unwrap_or_default();
    let mut at = scene.span.start;
    let mut last = scene.span.end;
    for line in region.split_inclusive('\n') {
        let next = at + line.len();
        let body = line.trim_matches([' ', '\t', '\r', '\n']);
        if !body.is_empty() && !body.starts_with("//") {
            last = next;
        }
        at = next;
    }
    last
}

/// 選択肢の表示文を確かめる：1行で、`->` と ` if ` を含まない（選択肢の行の書き方と取り違えないため。仕様書
/// 11節）。
fn check_label(label: &str) -> Result<&str, String> {
    // 前後の半角空白とタブだけを除く（全角空白は本文の一部）。
    let label = label.trim_matches([' ', '\t']);
    if label.is_empty() || label.contains(['\n', '\r']) {
        return Err("選択肢の表示文を1行で書いてください".to_owned());
    }
    // `- if x` は条件として読まれる（` if ` の前の空白は、`-` の後の空白と兼ねる。仕様書11節）。
    if label.contains("->")
        || label.contains(" if ")
        || label.contains("\tif ")
        || label.starts_with("if ")
        || label.starts_with("if\t")
    {
        return Err(
            "選択肢の表示文には「->」と「if 」を書けません（条件と飛び先の書き方と取り違えるため）"
                .to_owned(),
        );
    }
    Ok(label)
}

/// 辺を引いたときに書き足す中身を作る。
pub fn insert_edge(project: &Project, edge: &FlowEdge) -> Result<FlowInsert, String> {
    insert_edge_with(project, edge, &[])
}

/// 選択肢を足す中身を作る。`effects` は選んだときの `@設定` の行（選択肢の中に字下げして、行き先の前に書く。
/// 提案ノードの採用。T3-10）。
pub fn insert_choice(
    project: &Project,
    from: &str,
    to: &str,
    label: &str,
    effects: &[String],
) -> Result<FlowInsert, String> {
    for effect in effects {
        let t = effect.trim();
        if !(t.starts_with("@設定 ") || t.starts_with("@set ")) || t.contains(['\n', '\r']) {
            return Err(format!("選択肢の中に書けるのは @設定 の行だけです：{t}"));
        }
    }
    insert_edge_with(
        project,
        &FlowEdge::Choice {
            from: from.to_owned(),
            to: to.to_owned(),
            label: label.to_owned(),
        },
        effects,
    )
}

/// 選択肢の行（と中身）。
fn option_lines(indent: &str, label: &str, to: &str, effects: &[String], nl: &str) -> String {
    if effects.is_empty() {
        return format!("{indent}- {label} -> {to}{nl}");
    }
    let mut out = format!("{indent}- {label}{nl}");
    for effect in effects {
        out.push_str(&format!("{indent}  {}{nl}", effect.trim()));
    }
    out.push_str(&format!("{indent}  -> {to}{nl}"));
    out
}

fn insert_edge_with(
    project: &Project,
    edge: &FlowEdge,
    effects: &[String],
) -> Result<FlowInsert, String> {
    let (from, to) = match edge {
        FlowEdge::Jump { from, to } | FlowEdge::Choice { from, to, .. } => (from, to),
    };
    let options = project.parse_options();
    let mut found = None;
    let mut target = false;
    for file in &project.files {
        let parsed = parse(&file.text, &options);
        let scenes = &parsed.chapter.scenes;
        for (i, scene) in scenes.iter().enumerate() {
            let id = scene.id.as_ref().map(|s| s.value.as_str());
            if id == Some(to.as_str()) {
                target = true;
            }
            if id == Some(from.as_str()) && found.is_none() {
                let end = scenes
                    .get(i + 1)
                    .map_or(file.text.len(), |next| next.span.start);
                found = Some((file, scene.clone(), end));
            }
        }
    }
    let Some((file, scene, end)) = found else {
        return Err(format!("シーン {from} がありません"));
    };
    if !target {
        return Err(format!("シーン {to} がありません"));
    }
    let text = &file.text;
    let nl = newline_of(text);
    let last = scene.body.last();
    if last.is_some_and(unclosed) {
        return Err(format!(
            "シーン {from} の最後のブロックが @end で閉じていません（E103）。閉じてから足してください"
        ));
    }
    let ends = scene.body.iter().any(terminal);
    let insert_at = |at: usize, add: &str| -> (String, (usize, usize)) {
        let head = text.get(..at).unwrap_or_default();
        let tail = text.get(at..).unwrap_or_default();
        let lead = if head.is_empty() || head.ends_with('\n') {
            ""
        } else {
            nl
        };
        let start = head.len() + lead.len();
        (
            format!("{head}{lead}{add}{tail}"),
            (start, start + add.len()),
        )
    };
    let (new_text, inserted) = match edge {
        FlowEdge::Jump { .. } => {
            if ends {
                return Err(format!(
                    "シーン {from} は行き先の決まった終わり方（ジャンプ、エンディングなど）なので、ジャンプを足しても届きません。選択肢として足してください"
                ));
            }
            insert_at(after_last_line(text, &scene, end), &format!("-> {to}{nl}"))
        }
        FlowEdge::Choice { label, .. } => {
            let label = check_label(label)?;
            match last {
                Some(Stmt::Choice(choice)) if choice.end.is_some() => {
                    // そのブロックより前に行き先の決まった文があれば、ブロックに届かない。
                    let before = scene.body.len().saturating_sub(1);
                    if scene.body.iter().take(before).any(terminal) {
                        return Err(format!(
                            "シーン {from} の最後の選択ブロックは、その前のジャンプなどで届きません"
                        ));
                    }
                    let end_line = choice.end.map_or(0, |s| s.start);
                    // 字下げは最初の選択肢の行にそろえる。
                    let indent = choice
                        .options
                        .first()
                        .and_then(|o| {
                            let line_start = text
                                .get(..o.span.start)
                                .and_then(|h| h.rfind('\n'))
                                .map_or(0, |n| n + 1);
                            let line = text.get(line_start..)?;
                            let body = line.trim_start_matches([' ', '\t']);
                            line.get(..line.len() - body.len())
                        })
                        .unwrap_or_default();
                    // `@end` の行の始まり（字下げを含む）。
                    let at = text
                        .get(..end_line)
                        .and_then(|h| h.rfind('\n'))
                        .map_or(0, |n| n + 1);
                    insert_at(at, &option_lines(indent, label, to, effects, nl))
                }
                _ => {
                    if ends {
                        return Err(format!(
                            "シーン {from} は行き先の決まった終わり方（ジャンプ、エンディングなど）なので、選択肢を足しても届きません"
                        ));
                    }
                    insert_at(
                        after_last_line(text, &scene, end),
                        &format!(
                            "@選択{nl}{}@end{nl}",
                            option_lines("", label, to, effects, nl)
                        ),
                    )
                }
            }
        }
    };
    Ok(FlowInsert {
        file: file.path.clone(),
        text: new_text,
        inserted,
    })
}
