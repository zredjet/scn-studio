//! 台本の出力（T1-12、D051）：収録と校正に使う表を、IRから作る。
//!
//! 1行が1つの台詞（`@選択` の中や `@if` の枝の中も、IRの順＝ファイルのパスの順と、ファイルの中の順）。
//! 地の文は読まないので行にせず、台詞の「直前の地の文」に入れる。列は、行ID、シーン、キャラ、台詞、
//! 直前の地の文、ボイスファイル名、備考。CSV は Excel でそのまま開けるよう、UTF-8（BOM付き）と CRLF で書く。
//! Excel 形式（.xlsx）でも書ける（T2-10、D061。欄はすべて文字列）。
//!
//! 再収録候補（T2-10、D061）：書き出した台本を「収録の基準」として記録しておくと（`export/script-baseline.json`。
//! 行IDごとの、話者と台詞のハッシュ）、次からの台本で、基準の後に変わった台詞の備考に「再収録候補」と書く。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ir::{Ir, IrInline, IrScene, Op};
use crate::project::{Project, ProjectError, write_bytes};
use crate::xlsx::xlsx_table;

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
    /// 収録の基準と比べた結果（[`mark_retakes`]。基準が無ければ `None`）。
    pub retakes: Option<Retakes>,
}

/// 収録の基準と比べた結果。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct Retakes {
    /// 基準の後に、話者か台詞が変わった台詞（再収録候補）。
    pub changed: usize,
    /// 基準に無い行IDの台詞（基準の後に足した）。
    pub added: usize,
    /// 基準にあって、今の台本に無い行IDの数。
    pub removed: usize,
    /// 基準にあって、今の台本に無い行ID（要らなくなった収録。行IDの順）。
    pub removed_ids: Vec<String>,
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
        retakes: None,
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

fn table(sheet: &ScriptSheet) -> Vec<Vec<String>> {
    sheet
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
        .collect()
}

/// 台本の CSV（UTF-8 の BOM 付き、CRLF）。
pub fn script_csv(sheet: &ScriptSheet) -> String {
    csv_table(&SCRIPT_COLUMNS, &table(sheet))
}

/// Excel 形式の列の幅（文字数の目安）。
const XLSX_WIDTHS: [u32; 7] = [8, 22, 14, 50, 40, 18, 28];

/// 台本の Excel 形式（.xlsx）。欄はすべて文字列にする（行IDが数や日付に変わらない）。見出しの行は固定。
pub fn script_xlsx(sheet: &ScriptSheet) -> Result<Vec<u8>, ProjectError> {
    xlsx_table("台本", &SCRIPT_COLUMNS, &table(sheet), &XLSX_WIDTHS)
        .map_err(|e| ProjectError::Refused(e.to_string()))
}

/// 台本の形式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptFormat {
    /// CSV（UTF-8 の BOM 付き、CRLF）。
    #[default]
    Csv,
    /// Excel 形式（.xlsx）。
    Xlsx,
}

impl ScriptFormat {
    /// 作品フォルダの中の、既定の置き場所。
    pub fn default_file(self) -> &'static str {
        match self {
            ScriptFormat::Csv => SCRIPT_FILE,
            ScriptFormat::Xlsx => SCRIPT_XLSX_FILE,
        }
    }

    /// その形式のバイト列。
    pub fn bytes(self, sheet: &ScriptSheet) -> Result<Vec<u8>, ProjectError> {
        match self {
            ScriptFormat::Csv => Ok(script_csv(sheet).into_bytes()),
            ScriptFormat::Xlsx => script_xlsx(sheet),
        }
    }
}

/// 作品フォルダの中の、台本の既定の置き場所。
pub const SCRIPT_FILE: &str = "export/script.csv";
/// Excel 形式の台本の既定の置き場所。
pub const SCRIPT_XLSX_FILE: &str = "export/script.xlsx";

/// 作品フォルダの中のファイルを書く（フォルダが無ければ作る）。書き込みは `write_bytes` を通す（一時ファイルに
/// 書いてから置き換えるので、途中で止まっても壊れたファイルを残さない）。
fn write_export(root: &Path, file: &str, bytes: &[u8]) -> Result<PathBuf, ProjectError> {
    let path = root.join(file);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|source| ProjectError::Write {
            path: dir.to_owned(),
            source,
        })?;
    }
    write_bytes(root, file, bytes)?;
    Ok(path)
}

/// 台本を作品フォルダの既定の置き場所（`export/script.csv` か `export/script.xlsx`）に書く。
pub fn write_script(
    root: &Path,
    sheet: &ScriptSheet,
    format: ScriptFormat,
) -> Result<PathBuf, ProjectError> {
    write_export(root, format.default_file(), &format.bytes(sheet)?)
}

// ---- 再収録候補（T2-10、D061） ----

/// 収録の基準の置き場所。
pub const BASELINE_FILE: &str = "export/script-baseline.json";

/// 収録の基準：行IDごとの、話者と台詞のハッシュ（FNV-1a 64 ビットの16進）。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Baseline {
    pub lines: BTreeMap<String, String>,
}

/// 台詞のハッシュ（ボイスファイル名＝話者と行ID、台詞の文字）。
fn line_hash(row: &ScriptRow) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in row
        .voice
        .bytes()
        .chain(std::iter::once(0x1f))
        .chain(row.text.bytes())
    {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// 台本で2回以上使われている行ID（基準にも比較にも使わない。どちらの台詞か決められないため）。
fn repeated_ids(sheet: &ScriptSheet) -> BTreeSet<&str> {
    let mut seen = BTreeSet::new();
    let mut twice = BTreeSet::new();
    for row in &sheet.rows {
        if !row.line_id.is_empty() && !seen.insert(row.line_id.as_str()) {
            twice.insert(row.line_id.as_str());
        }
    }
    twice
}

/// 台本から、収録の基準を作る（行IDのある台詞だけ。重なった行IDは除く）。
pub fn baseline_of(sheet: &ScriptSheet) -> Baseline {
    let repeated = repeated_ids(sheet);
    Baseline {
        lines: sheet
            .rows
            .iter()
            .filter(|r| !r.line_id.is_empty() && !repeated.contains(r.line_id.as_str()))
            .map(|r| (r.line_id.clone(), line_hash(r)))
            .collect(),
    }
}

/// 収録の基準を読む。無ければ `None`。読めなければ、そのわけ。
pub fn read_baseline(root: &Path) -> Result<Option<Baseline>, String> {
    let path = root.join(BASELINE_FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{BASELINE_FILE} を読めません：{e}")),
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| format!("{BASELINE_FILE} を読めません：{e}"))
}

/// 収録の基準を書く（台本の今の台詞で置き換える）。
pub fn write_baseline(root: &Path, sheet: &ScriptSheet) -> Result<PathBuf, ProjectError> {
    let mut text = serde_json::to_string_pretty(&baseline_of(sheet))
        .map_err(|e| ProjectError::Refused(e.to_string()))?;
    text.push('\n');
    write_export(root, BASELINE_FILE, text.as_bytes())
}

/// 台本を収録の基準と比べ、変わった台詞と足した台詞の備考に書き足す。重なった行ID（E301）の台詞は比べない
/// （備考の「行IDが重なっている」で分かる）。
pub fn mark_retakes(sheet: &mut ScriptSheet, baseline: &Baseline) {
    let mut retakes = Retakes::default();
    let repeated: BTreeSet<String> = repeated_ids(sheet).into_iter().map(str::to_owned).collect();
    let mut seen = BTreeSet::new();
    for row in &mut sheet.rows {
        if row.line_id.is_empty() {
            continue;
        }
        seen.insert(row.line_id.clone());
        if repeated.contains(&row.line_id) {
            continue;
        }
        let note = match baseline.lines.get(&row.line_id) {
            Some(hash) if *hash == line_hash(row) => continue,
            Some(_) => {
                retakes.changed += 1;
                "再収録候補（収録の基準の後に変わった）"
            }
            None => {
                retakes.added += 1;
                "収録の基準の後に足した"
            }
        };
        if !row.note.is_empty() {
            row.note.push('、');
        }
        row.note.push_str(note);
    }
    retakes.removed_ids = baseline
        .lines
        .keys()
        .filter(|id| !seen.contains(*id))
        .cloned()
        .collect();
    retakes.removed = retakes.removed_ids.len();
    sheet.retakes = Some(retakes);
}
