//! 台本の出力（T1-12、D051）：収録と校正に使う表を、IRから作る。
//!
//! 1行が1つの台詞（`@選択` の中や `@if` の枝の中も、IRの順＝ファイルのパスの順と、ファイルの中の順）。
//! 地の文は読まないので行にせず、台詞の「直前の地の文」に入れる。列は、行ID、シーン、キャラ、台詞、
//! 直前の地の文、ボイスファイル名、備考。CSV は Excel でそのまま開けるよう、UTF-8（BOM付き）と CRLF で書く。

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::ir::{Ir, IrInline, IrScene, Op};
use crate::project::{Project, ProjectError, write_source};

/// 台本の列の見出し。
pub const SCRIPT_COLUMNS: [&str; 7] = [
    "行ID",
    "シーン",
    "キャラ",
    "台詞",
    "直前の地の文",
    "ボイスファイル名",
    "備考",
];

/// 台本の1行（1つの台詞）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScriptRow {
    /// 行ID。無ければ空。
    pub line_id: String,
    /// シーンIDと題名（`s012 放課後の屋上`）。
    pub scene: String,
    /// キャラクターの名前（表示名があれば表示名）。
    pub character: String,
    /// 台詞（括弧は付けない。ルビは `親文字（読み）`）。
    pub text: String,
    /// 同じシーンで、この台詞より前にある、いちばん近い地の文。
    pub narration: String,
    /// ボイスファイル名（`キャラID_行ID`。行IDが無ければ空）。
    pub voice: String,
    /// 備考（AI下書き、心の声、表示名、文中の効果音、行IDなし、行IDの重なり）。
    pub note: String,
}

/// 台本。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScriptSheet {
    pub rows: Vec<ScriptRow>,
    /// 行IDの無い台詞の数（ボイスファイル名を付けられない。形の合わない行ID（E304）の台詞も含む）。
    pub missing_line_ids: usize,
}

/// 文中の記法を、台本の文字にする。ルビは `親文字（読み）`、傍点は中身、`{w}`・`{p}`・`{speed}` は
/// 出さず、文中の効果音は備考に回し、変数の埋め込みは `{=式}` のまま、複数行の台詞の改行は改行にする。
fn plain(text: &[IrInline], effects: &mut Vec<String>) -> String {
    let mut out = String::new();
    for inline in text {
        match inline {
            IrInline::Text { text } | IrInline::Emphasis { text } => out.push_str(text),
            IrInline::Ruby { base, reading } => {
                out.push_str(base);
                out.push('（');
                out.push_str(reading);
                out.push('）');
            }
            IrInline::Se { name } => effects.push(name.clone()),
            IrInline::Interpolation { expr_text, .. } => {
                out.push_str("{=");
                out.push_str(expr_text);
                out.push('}');
            }
            IrInline::LineBreak => out.push('\n'),
            IrInline::Wait { .. } | IrInline::PageBreak | IrInline::Speed { .. } => {}
        }
    }
    out
}

struct Builder<'a> {
    project: &'a Project,
    rows: Vec<ScriptRow>,
    missing: usize,
    /// 2回以上使われている行ID（E301）。
    duplicated: BTreeSet<String>,
}

impl Builder<'_> {
    fn scene(&mut self, scene: &IrScene) {
        let mut narration = String::new();
        let label = if scene.title.is_empty() {
            scene.id.clone()
        } else {
            format!("{} {}", scene.id, scene.title)
        };
        let ai_draft = scene.meta.status.as_deref() == Some("ai-draft");
        self.ops(&scene.ops, &label, ai_draft, &mut narration);
    }

    fn ops(&mut self, ops: &[Op], scene: &str, ai_draft: bool, narration: &mut String) {
        for op in ops {
            match op {
                Op::Narrate { text, .. } => {
                    *narration = plain(text, &mut Vec::new());
                }
                Op::Say {
                    line_id,
                    speaker,
                    display_name,
                    style,
                    text,
                    ..
                } => {
                    let mut effects = Vec::new();
                    let line = plain(text, &mut effects);
                    let name = self
                        .project
                        .characters
                        .iter()
                        .find(|c| c.id == *speaker)
                        .map_or(speaker.as_str(), |c| c.name.as_str());
                    let mut notes = Vec::new();
                    if ai_draft {
                        notes.push("AI下書き".to_owned());
                    }
                    if style == "thought" {
                        notes.push("心の声".to_owned());
                    } else if style != "normal" {
                        notes.push(format!("表示スタイル：{style}"));
                    }
                    let character = match display_name.as_deref() {
                        Some(shown) if !shown.is_empty() && shown != name => {
                            notes.push(format!("表示名：{shown}"));
                            format!("{name}（{shown}）")
                        }
                        _ => name.to_owned(),
                    };
                    if !effects.is_empty() {
                        notes.push(format!("SE：{}", effects.join("・")));
                    }
                    let (id, voice) = match line_id {
                        Some(id) => {
                            if self.duplicated.contains(id) {
                                notes.push(
                                    "行IDが重なっている（E301。ボイスファイル名も重なる）"
                                        .to_owned(),
                                );
                            }
                            (id.clone(), format!("{speaker}_{id}"))
                        }
                        None => {
                            self.missing += 1;
                            notes.push("行IDなし".to_owned());
                            (String::new(), String::new())
                        }
                    };
                    self.rows.push(ScriptRow {
                        line_id: id,
                        scene: scene.to_owned(),
                        character,
                        text: line,
                        narration: narration.clone(),
                        voice,
                        note: notes.join("、"),
                    });
                }
                Op::Choice { options, .. } => {
                    for option in options {
                        // 選択肢の中身は、選んだ後に続く。直前の地の文は、選択肢の前のものを引き継ぐ。
                        // 中身の地の文は、ブロックの後ろへ持ち越さない（どの枝を通ったか分からないため）。
                        let mut inner = narration.clone();
                        self.ops(&option.body, scene, ai_draft, &mut inner);
                    }
                }
                Op::If {
                    branches,
                    else_body,
                    ..
                } => {
                    for branch in branches {
                        let mut inner = narration.clone();
                        self.ops(&branch.body, scene, ai_draft, &mut inner);
                    }
                    if let Some(body) = else_body {
                        let mut inner = narration.clone();
                        self.ops(body, scene, ai_draft, &mut inner);
                    }
                }
                _ => {}
            }
        }
    }
}

/// 本文の行IDのうち、2回以上使われているもの。
fn duplicated_ids(ir: &Ir) -> BTreeSet<String> {
    fn note(id: Option<&String>, seen: &mut BTreeSet<String>, twice: &mut BTreeSet<String>) {
        if let Some(id) = id
            && !seen.insert(id.clone())
        {
            twice.insert(id.clone());
        }
    }
    fn walk(ops: &[Op], seen: &mut BTreeSet<String>, twice: &mut BTreeSet<String>) {
        for op in ops {
            match op {
                Op::Say { line_id, .. } | Op::Narrate { line_id, .. } => {
                    note(line_id.as_ref(), seen, twice);
                }
                Op::Choice {
                    line_id, options, ..
                } => {
                    note(line_id.as_ref(), seen, twice);
                    for option in options {
                        note(option.line_id.as_ref(), seen, twice);
                        walk(&option.body, seen, twice);
                    }
                }
                Op::If {
                    branches,
                    else_body,
                    ..
                } => {
                    for branch in branches {
                        walk(&branch.body, seen, twice);
                    }
                    if let Some(body) = else_body {
                        walk(body, seen, twice);
                    }
                }
                _ => {}
            }
        }
    }
    let mut seen = BTreeSet::new();
    let mut twice = BTreeSet::new();
    for scene in &ir.scenes {
        walk(&scene.ops, &mut seen, &mut twice);
    }
    twice
}

/// 台本を作る。
pub fn script(project: &Project, ir: &Ir) -> ScriptSheet {
    let mut builder = Builder {
        project,
        rows: Vec::new(),
        missing: 0,
        duplicated: duplicated_ids(ir),
    };
    for scene in &ir.scenes {
        builder.scene(scene);
    }
    ScriptSheet {
        rows: builder.rows,
        missing_line_ids: builder.missing,
    }
}

/// CSV の1つの欄（RFC 4180：`"`・`,`・改行を含めば `"` で囲み、`"` は2つにする）。改行は CRLF にそろえる。
fn field(value: &str) -> String {
    let value = value.replace("\r\n", "\n").replace('\n', "\r\n");
    if value.contains(['"', ',', '\r', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value
    }
}

/// 見出しと行から、CSV（UTF-8 の BOM 付き、CRLF）を作る（台本と発注リスト）。
pub(crate) fn csv_table(header: &[&str], rows: &[Vec<String>]) -> String {
    let mut out = String::from("\u{feff}");
    out.push_str(
        &header
            .iter()
            .map(|c| field(c))
            .collect::<Vec<_>>()
            .join(","),
    );
    out.push_str("\r\n");
    for row in rows {
        out.push_str(&row.iter().map(|f| field(f)).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    out
}

/// 台本の CSV（UTF-8 の BOM 付き、CRLF）。
pub fn script_csv(sheet: &ScriptSheet) -> String {
    let rows: Vec<Vec<String>> = sheet
        .rows
        .iter()
        .map(|row| {
            vec![
                row.line_id.clone(),
                row.scene.clone(),
                row.character.clone(),
                row.text.clone(),
                row.narration.clone(),
                row.voice.clone(),
                row.note.clone(),
            ]
        })
        .collect();
    csv_table(&SCRIPT_COLUMNS, &rows)
}

/// 作品フォルダの中の、台本の既定の置き場所。
pub const SCRIPT_FILE: &str = "export/script.csv";

/// 台本を作品フォルダの既定の置き場所（`export/script.csv`）に書く。フォルダが無ければ作る。書き込みは
/// `write_source` を通す（一時ファイルに書いてから置き換えるので、途中で止まっても壊れたファイルを残さない）。
pub fn write_script(root: &Path, sheet: &ScriptSheet) -> Result<PathBuf, ProjectError> {
    let path = root.join(SCRIPT_FILE);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|source| ProjectError::Write {
            path: dir.to_owned(),
            source,
        })?;
    }
    write_source(root, SCRIPT_FILE, &script_csv(sheet))?;
    Ok(path)
}
