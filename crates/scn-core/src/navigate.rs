//! 定義へ移動・参照・ホバー（言語サーバー。T3-15）：本文の位置の名前（シーンID、キャラクター、変数）から、
//! 定義の場所、すべての現れ、説明の文を作る。名前の見つけ方は名前の変更（`rename::symbol_at`）と同じ。

use std::fmt::Write as _;

use serde::Serialize;

use crate::Span;
use crate::ir::Ir;
use crate::project::Project;
use crate::rename::{RenameTarget, occurrences, symbol_at};

/// 作品フォルダの中の場所（ファイルは作品フォルダから。スパンはそのファイルの UTF-8 のバイト位置）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Place {
    pub file: String,
    pub span: Span,
}

/// 位置の名前の定義：シーンは見出しの行、キャラクターは設定DBのエントリ（`name` の項目、無ければ先頭）、変数は
/// `variables.yaml` の宣言のキー。
pub fn definition(project: &Project, ir: &Ir, file: &str, offset: usize) -> Option<Place> {
    let symbol = symbol_at(project, file, offset)?;
    match &symbol.target {
        RenameTarget::Scene { id } => ir.scenes.iter().find(|s| &s.id == id).map(|s| Place {
            file: s.file.clone(),
            span: s.span,
        }),
        RenameTarget::CharacterName { id } => {
            entry_place(project, project.bible.character_type(), id)
        }
        RenameTarget::EntryId { type_id, id } => entry_place(project, type_id, id),
        RenameTarget::Variable { key } => {
            project
                .variables
                .iter()
                .find(|v| &v.key == key)
                .map(|v| Place {
                    file: "variables.yaml".to_owned(),
                    span: v.span,
                })
        }
    }
}

fn entry_place(project: &Project, type_id: &str, id: &str) -> Option<Place> {
    let entry = project
        .bible
        .entries
        .iter()
        .find(|e| e.type_id == type_id && e.id == id)?;
    Some(Place {
        file: entry.path.clone(),
        span: entry
            .key_spans
            .get("name")
            .copied()
            .unwrap_or(Span::new(0, 0)),
    })
}

/// 位置の名前のすべての現れ（本文の中。`include_definition` なら定義の場所も）。
pub fn references(
    project: &Project,
    ir: &Ir,
    file: &str,
    offset: usize,
    include_definition: bool,
) -> Vec<Place> {
    let Some(symbol) = symbol_at(project, file, offset) else {
        return Vec::new();
    };
    let mut out: Vec<Place> = occurrences(project, &symbol.target)
        .into_iter()
        .map(|(file, span)| Place { file, span })
        .collect();
    match &symbol.target {
        // シーンの定義（見出しのID）は現れに入っているので、含めないときは除く。
        RenameTarget::Scene { id } => {
            if !include_definition && let Some(scene) = ir.scenes.iter().find(|s| &s.id == id) {
                out.retain(|p| {
                    !(p.file == scene.file
                        && scene.span.start <= p.span.start
                        && p.span.end <= scene.span.end)
                });
            }
        }
        _ => {
            if include_definition && let Some(def) = definition(project, ir, file, offset) {
                out.insert(0, def);
            }
        }
    }
    out
}

/// ホバーの文（Markdown）と、名前の範囲。
pub fn hover(project: &Project, ir: &Ir, file: &str, offset: usize) -> Option<(String, Span)> {
    let symbol = symbol_at(project, file, offset)?;
    let mut text = String::new();
    match &symbol.target {
        RenameTarget::Scene { id } => {
            let scene = ir.scenes.iter().find(|s| &s.id == id)?;
            let _ = writeln!(text, "**{}** `#{id}`", scene.title);
            let mut facts = Vec::new();
            if let Some(route) = &scene.meta.route {
                facts.push(format!("ルート：{route}"));
            }
            if let Some(status) = &scene.meta.status {
                facts.push(format!("状態：{status}"));
            }
            if scene.is_stub {
                facts.push("スタブ".to_owned());
            }
            facts.push(scene.file.clone());
            let _ = writeln!(text, "\n{}", facts.join("　"));
            if let Some(summary) = scene.meta.summary.as_deref().filter(|s| !s.is_empty()) {
                let _ = writeln!(text, "\n{summary}");
            }
        }
        RenameTarget::CharacterName { id } | RenameTarget::EntryId { id, .. } => {
            let c = project.characters.iter().find(|c| &c.id == id)?;
            let _ = writeln!(text, "**{}** `{id}`", c.name);
            if !c.aliases.is_empty() {
                let _ = writeln!(text, "\n呼び名：{}", c.aliases.join("、"));
            }
        }
        RenameTarget::Variable { key } => {
            let _ = writeln!(text, "**{key}**");
            match project.variables.iter().find(|v| &v.key == key) {
                Some(v) => {
                    let mut facts = vec![format!("型：{}", v.ty.name())];
                    if let Some(default) = &v.default {
                        facts.push(format!("初めの値：{default}"));
                    }
                    if let Some((min, max)) = v.range {
                        facts.push(format!("範囲：{min}〜{max}"));
                    }
                    if let Some(scope) = &v.scope {
                        facts.push(format!("スコープ：{scope}"));
                    }
                    let _ = writeln!(text, "\n{}", facts.join("　"));
                    if let Some(desc) = v.desc.as_deref().filter(|d| !d.is_empty()) {
                        let _ = writeln!(text, "\n{desc}");
                    }
                }
                None => {
                    let _ = writeln!(text, "\n宣言がありません（variables.yaml）");
                }
            }
        }
    }
    Some((text.trim_end().to_owned(), symbol.span))
}
