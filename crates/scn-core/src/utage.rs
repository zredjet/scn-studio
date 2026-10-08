//! 宴（Unity のADVエンジン）への書き出し（T3-12、D073）：IRを、出力プロファイル（`export/utage.yaml`）に従って、
//! 宴の表形式のシナリオ（Excel 形式。シーンごとのシート）と設定のシート（Character・Texture・Sound・Param）に
//! する。書き出しは一方向で、生成物を手で直さない（`docs/design.md` の「エクスポート」）。素材の扱いは Ren'Py と
//! 同じ考え（D053）。
//!
//! 書き出し先（既定は `export/utage/`）は、宴のプロジェクトへ取り込む前の置き場で、まるごと作り直す（印のファイル
//! `.scn-studio` が無ければ消さずに止める）。
//!
//! - シナリオのシートの列は `Command`・`Arg1`〜`Arg6`・`WaitType`・`Text`・`PageCtrl`・`Voice`。宴は見出しの名前で
//!   列を読む。
//! - シーンはシナリオラベル（`*s012`）。流れはラベルとジャンプだけで書く（選択肢は `Selection` の行と選択肢ごとの
//!   ラベル、`@if` は条件つきの `Jump`）。
//! - 台詞の `Arg1` は表示の名前（キャラクターの名前）にし、立ち絵（Character のシートの `ch_…`）とは分ける
//!   （`.scn` では台詞と立ち絵は別の命令なので、話しても立ち絵を出さない）。
//! - 変数はパラメーター（`akari.好感度` → `akari_好感度`。プロファイルの `variables` で上書きできる）。
//! - 素材は `assets/` から数えたパスのまま写す。ファイルの無い画像は色の矩形の PNG（仮素材）。ファイルの無い音は
//!   鳴らさない。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use yaml_serde::Value as Yaml;

use crate::assets::assets_relative;
use crate::check::falls_through;
use crate::ir::{Ir, IrBranch, IrExpr, IrInline, IrOption, Op};
use crate::preview::{PreviewData, preview_data};
use crate::project::{Project, ProjectError, VarType, without_bom};
use crate::renpy::{
    Used, color_of, identifier, mark_of, position_key, relative_inside, yaml_map, yaml_string,
    yaml_text,
};
use crate::tyrano::{ascii_name, unique};
use crate::xlsx::{Sheet, crc32, xlsx_book};

/// 出力プロファイルの置き場所（作品フォルダから）。
pub const UTAGE_PROFILE: &str = "export/utage.yaml";

/// 書き出し先の既定（作品フォルダから）。
pub const DEFAULT_OUTPUT: &str = "export/utage";

/// 書き出したフォルダの印のファイル。
pub const MARK_FILE: &str = ".scn-studio";

/// 印のファイルの中身。
const MARK: &str =
    "scn-studio が .scn から書き出したフォルダ。手で直さない（書き出すたびに作り直す）。\n";

/// シナリオのシートの列（宴は見出しの名前で列を読む）。
pub const SCENARIO_COLUMNS: [&str; 11] = [
    "Command", "Arg1", "Arg2", "Arg3", "Arg4", "Arg5", "Arg6", "WaitType", "Text", "PageCtrl",
    "Voice",
];

const COMMAND: usize = 0;
const ARG1: usize = 1;
const ARG2: usize = 2;
const ARG3: usize = 3;
const ARG6: usize = 6;
const TEXT: usize = 8;
const PAGE_CTRL: usize = 9;
const VOICE: usize = 10;

/// 開始のシート（シナリオラベル）。宴の作品の `Start` のシートから `Jump *scn_start` で飛ぶ（新しい作品にある
/// `Start` と重ならないように）。
pub const START_LABEL: &str = "scn_start";

/// シナリオラベル（シートの名前）の長さの上限（Excel のシートの名前は31文字まで。重なりの `_2` などの余地を残す）。
const LABEL_LIMIT: usize = 27;

/// 立ち絵の位置ごとのレイヤー（Layer のシートに足す。初めが既定の Character のレイヤー）。
const LAYERS: [(&str, &str); 3] = [
    ("center", "scn_character_center"),
    ("left", "scn_character_left"),
    ("right", "scn_character_right"),
];

/// 出力プロファイル。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UtageProfile {
    /// 書き出し先（作品フォルダから）。
    pub output: String,
    /// ブックの名前（`<book>.xlsx` がシナリオ、`<book>_settings.xlsx` が設定）。
    pub book: String,
    /// 立ち絵の X 座標（IRの値 `left`・`center`・`right` から）。書かなければ画面の幅から決める。
    pub positions: BTreeMap<String, i64>,
    /// トランジションの名前から、フェードの秒。
    pub transitions: BTreeMap<String, f64>,
    /// `@効果` の名前から、宴のコマンドの行（`[Command, Arg1, …]`）。
    pub fx: BTreeMap<String, Vec<String>>,
    /// ボイスのファイル名（`{char_id}`・`{line_id}`。作品フォルダの `assets/` から数える）。
    pub voice: Option<String>,
    /// エンディングで書くコマンドの行。
    pub ending: Vec<String>,
    /// キャラクターIDから、Character のシートの CharacterName へ。
    pub characters: BTreeMap<String, String>,
    /// 変数名から、パラメーターの名前へ。
    pub variables: BTreeMap<String, String>,
}

impl Default for UtageProfile {
    fn default() -> Self {
        Self {
            output: DEFAULT_OUTPUT.to_owned(),
            book: "scn".to_owned(),
            positions: BTreeMap::new(),
            transitions: [("フェード".to_owned(), 1.0), ("即時".to_owned(), 0.0)]
                .into_iter()
                .collect(),
            fx: BTreeMap::new(),
            voice: None,
            ending: vec!["EndScenario".to_owned()],
            characters: BTreeMap::new(),
            variables: BTreeMap::new(),
        }
    }
}

/// コマンドの行（文字列か、文字列のリスト）を読む。
fn command_row(value: &Yaml) -> Option<Vec<String>> {
    match value {
        Yaml::Sequence(items) => {
            let row: Option<Vec<String>> = items.iter().map(yaml_text).collect();
            row.filter(|r| !r.is_empty() && r.len() <= SCENARIO_COLUMNS.len())
        }
        other => yaml_text(other).map(|t| vec![t]),
    }
}

/// 出力プロファイルを読む。無ければ既定。値の誤りは読み飛ばして既定を使い、何を読み飛ばしたかを返す。
pub fn read_profile(root: &Path) -> (UtageProfile, Vec<String>) {
    let mut profile = UtageProfile::default();
    let mut problems = Vec::new();
    let text = match fs::read_to_string(root.join(UTAGE_PROFILE)) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (profile, problems),
        Err(e) => {
            problems.push(format!("読めません：{e}。既定の設定で書き出します"));
            return (profile, problems);
        }
    };
    let yaml: Yaml = match yaml_serde::from_str(without_bom(&text)) {
        Ok(value) => value,
        Err(e) => {
            problems.push(format!(
                "YAMLとして読めません：{e}。既定の設定で書き出します"
            ));
            return (profile, problems);
        }
    };
    if !yaml.is_mapping() {
        if !yaml.is_null() {
            problems.push("対応表で書きます。既定の設定で書き出します".to_owned());
        }
        return (profile, problems);
    }
    match yaml_string(&yaml, "target", &mut problems) {
        Some(target) if target != "utage" => {
            problems.push(format!("target が utage ではありません（{target}）"));
        }
        _ => {}
    }
    if let Some(output) = yaml_string(&yaml, "output", &mut problems) {
        if relative_inside(&output) {
            profile.output = output;
        } else {
            problems.push(format!(
                "output は作品フォルダの中を指す相対パスで書きます（{output}）。{DEFAULT_OUTPUT} に書き出します"
            ));
        }
    }
    if let Some(book) = yaml_string(&yaml, "book", &mut problems) {
        if !book.is_empty()
            && book
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            profile.book = book;
        } else {
            problems.push(format!("book は英数字と _ と - で書きます（{book}）"));
        }
    }
    for (key, value) in yaml_map(&yaml, "positions", &mut problems) {
        let key = position_key(&key);
        if !matches!(key, "left" | "center" | "right") {
            continue;
        }
        match value.trim().parse::<i64>() {
            Ok(x) => {
                profile.positions.insert(key.to_owned(), x);
            }
            Err(_) => problems.push(format!(
                "positions の {key} は、立ち絵の X 座標（整数）で書きます"
            )),
        }
    }
    for (key, value) in yaml_map(&yaml, "transitions", &mut problems) {
        let seconds = if value == "None" {
            Some(0.0)
        } else {
            value
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|s| s.is_finite() && *s >= 0.0)
        };
        match seconds {
            Some(s) => {
                profile.transitions.insert(key, s);
            }
            None => problems.push(format!(
                "transitions の {key} は、フェードの秒（0 以上の数）で書きます"
            )),
        }
    }
    match yaml.get("fx") {
        None | Some(Yaml::Null) => {}
        Some(Yaml::Mapping(map)) => {
            for (k, v) in map {
                match (yaml_text(k), command_row(v)) {
                    (Some(k), Some(row)) => {
                        profile.fx.insert(k, row);
                    }
                    _ => problems.push(
                        "fx の値は、コマンドの行（[Command, Arg1, …] か Command の文字列）で書きます"
                            .to_owned(),
                    ),
                }
            }
        }
        Some(_) => problems.push("fx は対応表で書きます".to_owned()),
    }
    match yaml.get("ending") {
        None | Some(Yaml::Null) => {}
        Some(value) => match command_row(value) {
            Some(row) => profile.ending = row,
            None => problems.push(
                "ending は、コマンドの行（[Command, Arg1, …] か Command の文字列）で書きます"
                    .to_owned(),
            ),
        },
    }
    profile.characters = yaml_map(&yaml, "characters", &mut problems);
    profile.variables = yaml_map(&yaml, "variables", &mut problems);
    match yaml.get("voice") {
        None | Some(Yaml::Null) => {}
        Some(voice) => match voice.get("pattern").and_then(yaml_text) {
            Some(pattern) => profile.voice = Some(pattern),
            None => problems.push("voice は { pattern: \"…\" } で書きます".to_owned()),
        },
    }
    (profile, problems)
}

/// 書き出したシート（中身を読める形でも持つ。スナップショットと画面に使う）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UtageSheet {
    pub name: String,
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// 書き出すファイル。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UtageFile {
    /// 書き出し先の中の相対パスと中身（ブック、仮素材の PNG）。
    Bytes {
        path: String,
        #[serde(skip)]
        bytes: Vec<u8>,
    },
    /// 書き出し先の中の相対パスと、写す元（絶対パス）。
    Copy { path: String, from: String },
}

impl UtageFile {
    pub fn path(&self) -> &str {
        match self {
            UtageFile::Bytes { path, .. } | UtageFile::Copy { path, .. } => path,
        }
    }
}

/// 書き出しの結果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UtageExport {
    /// シナリオのシート（開始のシートと、シーンごと）。
    pub scenario: Vec<UtageSheet>,
    /// 設定のシート（Character・Texture・Sound・Param）。
    pub settings: Vec<UtageSheet>,
    pub files: Vec<UtageFile>,
    /// 知らせ。
    pub warnings: Vec<String>,
}

/// 1行（列の数だけの欄）。
fn blank_row() -> Vec<String> {
    vec![String::new(); SCENARIO_COLUMNS.len()]
}

/// 秒を書く（`1`、`0.5`）。
fn seconds(value: f64) -> String {
    format!("{value}")
}

/// C# の文字列（宴の式）。
fn cs_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// 仮素材の PNG（1色の矩形。1ビットのパレット、圧縮しない zlib）。
fn placeholder_png(width: u32, height: u32, color: &str) -> Vec<u8> {
    let hex = |i: usize| {
        color
            .get(i..i + 2)
            .and_then(|h| u8::from_str_radix(h, 16).ok())
            .unwrap_or(0x40)
    };
    let (r, g, b) = (hex(1), hex(3), hex(5));
    let row = 1 + (width as usize).div_ceil(8);
    let raw = vec![0u8; row * height as usize];
    // zlib：ヘッダ、無圧縮のブロック（65535バイトずつ）、Adler-32。
    let mut zlib = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65535).peekable();
    if chunks.peek().is_none() {
        zlib.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    while let Some(chunk) = chunks.next() {
        let last = chunks.peek().is_none();
        let len = u16::try_from(chunk.len()).unwrap_or(u16::MAX);
        zlib.push(u8::from(last));
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(chunk);
    }
    let (mut a, mut b2) = (1u32, 0u32);
    for byte in &raw {
        a = (a + u32::from(*byte)) % 65521;
        b2 = (b2 + a) % 65521;
    }
    zlib.extend_from_slice(&((b2 << 16) | a).to_be_bytes());
    let chunk = |out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(0).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    };
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[1, 3, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &header);
    chunk(&mut png, b"PLTE", &[r, g, b]);
    chunk(&mut png, b"IDAT", &zlib);
    chunk(&mut png, b"IEND", &[]);
    png
}

/// 書き出しの途中の状態。
struct Exporter<'a> {
    data: &'a PreviewData<'a>,
    profile: &'a UtageProfile,
    root: &'a Path,
    /// 変数名から、パラメーターの名前。
    params: BTreeMap<String, String>,
    bools: BTreeSet<String>,
    /// 埋め込みの一時のパラメーター（名前と型）。
    temps: Vec<(String, &'static str)>,
    /// シーンIDから、シナリオラベル。
    labels: BTreeMap<String, String>,
    /// 使っているラベル。
    taken: BTreeSet<String>,
    /// 今のシーンのラベルと、作ったラベルの数。
    label: String,
    next: usize,
    /// 背景の意味名から、テクスチャのラベル。
    backgrounds: BTreeMap<String, String>,
    /// キャラクターIDから、CharacterName。
    charas: BTreeMap<String, String>,
    /// （キャラクターID、差分）があるか。
    sprites: BTreeSet<(String, String)>,
    /// 音の意味名から、サウンドのラベル（BGM はくり返すかも）。
    bgm: BTreeMap<String, (String, bool)>,
    se: BTreeMap<String, String>,
    /// 設定のシートの行。
    character_rows: Vec<Vec<String>>,
    texture_rows: Vec<Vec<String>>,
    sound_rows: Vec<Vec<String>>,
    /// 写すファイルと、書く仮素材。
    copies: BTreeMap<String, String>,
    pngs: BTreeMap<String, Vec<u8>>,
    warnings: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl Exporter<'_> {
    fn warn(&mut self, message: String) {
        if !self.warnings.contains(&message) {
            self.warnings.push(message);
        }
    }

    fn push(&mut self, cells: &[(usize, String)]) {
        let mut row = blank_row();
        for (i, v) in cells {
            if let Some(cell) = row.get_mut(*i) {
                cell.clone_from(v);
            }
        }
        self.rows.push(row);
    }

    fn command(&mut self, name: &str, args: &[String]) {
        let mut cells = vec![(COMMAND, name.to_owned())];
        for (i, a) in args.iter().enumerate() {
            if !a.is_empty() {
                cells.push((ARG1 + i, a.clone()));
            }
        }
        self.push(&cells);
    }

    fn comment(&mut self, text: &str) {
        self.push(&[(COMMAND, format!("//{}", text.replace('\n', " ")))]);
    }

    fn label_row(&mut self, label: &str) {
        self.push(&[(COMMAND, format!("*{label}"))]);
    }

    fn new_label(&mut self) -> String {
        loop {
            self.next += 1;
            let name = format!("{}__{}", self.label, self.next);
            if self.taken.insert(name.clone()) {
                return name;
            }
        }
    }

    fn jump(&mut self, label: &str, cond: Option<&str>) {
        let mut args = vec![format!("*{label}")];
        if let Some(cond) = cond {
            args.push(cond.to_owned());
        }
        self.command("Jump", &args);
    }

    fn param(&self, key: &str) -> String {
        self.params
            .get(key)
            .cloned()
            .unwrap_or_else(|| identifier(&key.replace('.', "_"), "p_"))
    }

    /// 式を宴の式（C# に近い）にする。整数の割り算は C# と同じく0の向きへ切り捨てる。
    fn expr(&self, e: &IrExpr) -> String {
        match e {
            IrExpr::Int { value } => format!("({value})"),
            IrExpr::String { value } => cs_string(value),
            IrExpr::Bool { value } => if *value { "true" } else { "false" }.to_owned(),
            IrExpr::Var { name } => self.param(name),
            IrExpr::Unary { op, operand } => match *op {
                "not" => format!("(!{})", self.expr(operand)),
                _ => format!("(-{})", self.expr(operand)),
            },
            IrExpr::Binary { op, lhs, rhs } => {
                let (l, r) = (self.expr(lhs), self.expr(rhs));
                match *op {
                    "and" => format!("({l} && {r})"),
                    "or" => format!("({l} || {r})"),
                    _ => format!("({l} {op} {r})"),
                }
            }
        }
    }

    fn cond(&self, cond: Option<&IrExpr>) -> String {
        cond.map_or_else(|| "false".to_owned(), |c| self.expr(c))
    }

    /// 式の型（埋め込みの一時のパラメーターの型）。
    fn expr_type(&self, e: &IrExpr) -> &'static str {
        match e {
            IrExpr::String { .. } => "String",
            IrExpr::Bool { .. } => "Bool",
            IrExpr::Var { name } => {
                if self.bools.contains(name) {
                    "Bool"
                } else if self
                    .data
                    .variables
                    .iter()
                    .any(|v| v.key == *name && matches!(v.ty, "string" | "enum"))
                {
                    "String"
                } else {
                    "Int"
                }
            }
            IrExpr::Unary { op, .. } => {
                if *op == "not" {
                    "Bool"
                } else {
                    "Int"
                }
            }
            IrExpr::Binary { op, lhs, .. } => match *op {
                "and" | "or" | "==" | "!=" | "<" | "<=" | ">" | ">=" => "Bool",
                "+" if self.expr_type(lhs) == "String" => "String",
                _ => "Int",
            },
            IrExpr::Int { .. } => "Int",
        }
    }

    /// 文中の記法を宴のテキスト（テキストタグを含む）にし、`{p}` で区切る。変数でない式の埋め込みは、一時の
    /// パラメーターに入れる行を先に書く。文中の効果音は、台詞の前で鳴らす行を書く。
    fn pages(&mut self, text: &[IrInline], plain: bool) -> Vec<String> {
        let mut pages = Vec::new();
        let mut current = String::new();
        // 開いている速さのタグ（ページを替えたら閉じて開き直す）。
        let mut speed: Option<&'static str> = None;
        for inline in text {
            match inline {
                IrInline::Text { text } => current.push_str(text),
                IrInline::Emphasis { text } => {
                    let _ = write!(current, "<em=﹅>{text}</em>");
                }
                IrInline::Ruby { base, reading } => {
                    let _ = write!(current, "<ruby={reading}>{base}</ruby>");
                }
                IrInline::LineBreak => current.push('\n'),
                IrInline::Interpolation { expr, expr_text } => match expr {
                    Some(IrExpr::Var { name }) => {
                        let _ = write!(current, "<param={}>", self.param(name));
                    }
                    Some(e) => {
                        let ty = self.expr_type(e);
                        let name = format!("scn_value_{}", self.temps.len() + 1);
                        self.temps.push((name.clone(), ty));
                        let value = self.expr(e);
                        self.command("Param", &[format!("{name}={value}")]);
                        let _ = write!(current, "<param={name}>");
                    }
                    None => {
                        let _ = write!(current, "{{={expr_text}}}");
                    }
                },
                IrInline::Wait { seconds: s } => {
                    if !plain {
                        let _ = write!(current, "<interval={}>", seconds(*s));
                    }
                }
                IrInline::Speed { speed: s } => {
                    if plain {
                        continue;
                    }
                    if speed.is_some() {
                        current.push_str("</speed>");
                    }
                    speed = match *s {
                        "slow" => Some("<speed=0.1>"),
                        "fast" => Some("<speed=0.01>"),
                        _ => None,
                    };
                    if let Some(tag) = speed {
                        current.push_str(tag);
                    }
                }
                IrInline::PageBreak => {
                    if plain {
                        continue;
                    }
                    if speed.is_some() {
                        current.push_str("</speed>");
                    }
                    pages.push(std::mem::take(&mut current));
                    if let Some(tag) = speed {
                        current.push_str(tag);
                    }
                }
                IrInline::Se { name } => {
                    if plain {
                        self.warn(format!(
                            "選択肢と問いかけ文の中の効果音（{{se {name}}}）は、書き出していません"
                        ));
                        continue;
                    }
                    self.warn(
                        "文中の効果音（{se}）は、宴の台詞の前で鳴らすようにしました".to_owned(),
                    );
                    self.sound(name, false);
                }
            }
        }
        if speed.is_some() {
            current.push_str("</speed>");
        }
        pages.push(current);
        // 空のページ（`{p}` が続く、`{p}` で始まる・終わる）は出さない。
        let mut out: Vec<String> = pages.into_iter().filter(|p| !p.is_empty()).collect();
        if out.is_empty() {
            out.push(String::new());
        }
        out
    }

    /// 台詞・地の文：ページごとに1行（`Arg1` は表示の名前、`Text`）。括弧は初めのページの初めと最後のページの
    /// 終わりに付ける。ボイスは初めのページに付ける。
    fn say(
        &mut self,
        name: Option<&str>,
        brackets: Option<(&str, &str)>,
        voice: Option<String>,
        text: &[IrInline],
    ) {
        let pages = self.pages(text, false);
        let last = pages.len().saturating_sub(1);
        let (open, close) = brackets.unwrap_or(("", ""));
        for (i, page) in pages.iter().enumerate() {
            let mut body = page.clone();
            if i == 0 {
                body = format!("{open}{body}");
            }
            if i == last {
                body.push_str(close);
            }
            let mut cells = vec![(TEXT, body)];
            if let Some(name) = name {
                cells.push((ARG1, name.to_owned()));
            }
            if i == 0
                && let Some(voice) = &voice
            {
                cells.push((VOICE, voice.clone()));
            }
            self.push(&cells);
        }
    }

    /// 行き止まり：知らせてから、シナリオを終える。
    fn dead_end(&mut self, what: &str) {
        self.push(&[(TEXT, format!("（行き止まり：{what}）"))]);
        self.command("EndScenario", &[]);
    }

    fn jump_scene(&mut self, command: &str, target: &str) {
        match self.labels.get(target).cloned() {
            Some(label) => self.command(command, &[format!("*{label}")]),
            None => {
                self.warn(format!(
                    "存在しないシーン {target} へのジャンプ・呼び出しがあります（E501）。そこで止まるようにしました"
                ));
                self.dead_end(&format!("存在しないシーン {target}"));
            }
        }
    }

    fn fade(&mut self, name: Option<&str>) -> String {
        let Some(name) = name.filter(|n| !n.is_empty()) else {
            return "0".to_owned();
        };
        match self.profile.transitions.get(name) {
            Some(s) => seconds(*s),
            None => {
                self.warn(format!(
                    "トランジション「{name}」の対応がプロファイル（transitions）に無いので、0.5 秒にしました"
                ));
                "0.5".to_owned()
            }
        }
    }

    fn x_of(&self, position: &str) -> i64 {
        if let Some(x) = self.profile.positions.get(position) {
            return *x;
        }
        let width = i64::from(self.data.screen.width);
        match position {
            "left" => -width * 3 / 10,
            "right" => width * 3 / 10,
            _ => 0,
        }
    }

    /// ボイス：プロファイルの `voice` のファイルが `assets/` にあれば写し、`Voice` の列の値を返す。
    fn voice(&mut self, speaker: &str, line_id: Option<&str>) -> Option<String> {
        let (Some(pattern), Some(id)) = (self.profile.voice.as_deref(), line_id) else {
            return None;
        };
        let relative = pattern
            .replace("{char_id}", speaker)
            .replace("{line_id}", id);
        if !relative_inside(&relative) {
            return None;
        }
        let source = fs::canonicalize(self.root.join("assets").join(&relative)).ok()?;
        let assets = fs::canonicalize(self.root.join("assets")).ok()?;
        if !source.starts_with(&assets) || !source.is_file() {
            return None;
        }
        let name = format!("scn/{relative}");
        self.copies
            .insert(format!("Sound/Voice/{name}"), source.display().to_string());
        Some(name)
    }

    fn sound(&mut self, name: &str, music: bool) {
        if music {
            match self.bgm.get(name).cloned() {
                Some((label, repeat)) => {
                    let lp = if repeat { "" } else { "FALSE" };
                    self.command("Bgm", &[label, lp.to_owned()]);
                }
                None => {
                    self.comment(&format!("BGM {name}（ファイルが無いので鳴らさない）"));
                    self.command("StopBgm", &[]);
                }
            }
        } else {
            match self.se.get(name).cloned() {
                Some(label) => self.command("Se", &[label]),
                None => self.comment(&format!("SE {name}（ファイルが無いので鳴らさない）")),
            }
        }
    }

    fn ops(&mut self, ops: &[Op]) {
        for op in ops {
            self.op(op);
        }
    }

    /// 選択肢：問いかけ文の行、`Selection` の行（`Arg1` は選択肢ごとのラベル、`Arg2` は条件、`Text` は選択肢の
    /// 文）。選択肢のラベルでは、中身の後で行き先か続きのラベルへ飛ぶ。
    fn choice(&mut self, prompt: Option<&[IrInline]>, options: &[IrOption]) {
        if options.is_empty() {
            self.comment("選択肢の無い @選択（E106）");
            self.dead_end("選択肢がありません");
            return;
        }
        let end = self.new_label();
        let targets: Vec<String> = options.iter().map(|_| self.new_label()).collect();
        // 選択肢の文（埋め込みの一時のパラメーターの行）は先に作る（問いかけ文の行と `Selection` の行の間に入れない）。
        let texts: Vec<String> = options.iter().map(|o| self.option_text(&o.text)).collect();
        if let Some(prompt) = prompt {
            let text = self.pages(prompt, true).join("");
            // 問いかけ文を選択肢と一緒に見せる（`PageCtrl` に `Input`）。
            self.push(&[(TEXT, text), (PAGE_CTRL, "Input".to_owned())]);
        }
        let mut all_conditional = true;
        for ((option, target), text) in options.iter().zip(&targets).zip(texts) {
            let cond = match (&option.cond, &option.cond_text) {
                (Some(c), _) => self.expr(c),
                (None, Some(_)) => "false".to_owned(),
                (None, None) => {
                    all_conditional = false;
                    String::new()
                }
            };
            let mut cells = vec![
                (COMMAND, "Selection".to_owned()),
                (ARG1, format!("*{target}")),
                (TEXT, text),
            ];
            if !cond.is_empty() {
                cells.push((ARG2, cond));
            }
            self.push(&cells);
        }
        // どの選択肢も出ないときは、選択の後ろの行へ進むので、そこで止まる（プレビューと同じ）。
        if all_conditional {
            self.dead_end("選べる選択肢がありません");
        }
        for (option, target) in options.iter().zip(&targets) {
            self.label_row(target);
            self.ops(&option.body);
            match option.jump.as_deref() {
                Some(to) => self.jump_scene("Jump", to),
                None => self.jump(&end, None),
            }
        }
        self.label_row(&end);
    }

    /// 選択肢の文（テキストタグを含む。待ちと速さと効果音は書かない）。
    fn option_text(&mut self, text: &[IrInline]) -> String {
        self.pages(text, true).join("")
    }

    fn if_chain(&mut self, branches: &[IrBranch], else_body: Option<&[Op]>) {
        let end = self.new_label();
        let targets: Vec<String> = branches.iter().map(|_| self.new_label()).collect();
        let otherwise = else_body.map(|_| self.new_label());
        for (branch, target) in branches.iter().zip(&targets) {
            let cond = self.cond(branch.cond.as_ref());
            self.jump(target, Some(&cond));
        }
        self.jump(otherwise.as_deref().unwrap_or(end.as_str()), None);
        for (branch, target) in branches.iter().zip(&targets) {
            self.label_row(target);
            self.ops(&branch.body);
            self.jump(&end, None);
        }
        if let (Some(label), Some(body)) = (otherwise, else_body) {
            self.label_row(&label);
            self.ops(body);
            self.jump(&end, None);
        }
        self.label_row(&end);
    }

    fn op(&mut self, op: &Op) {
        match op {
            Op::Say {
                line_id,
                speaker,
                display_name,
                style,
                text,
                ..
            } => {
                let voice = self.voice(speaker, line_id.as_deref());
                let data = self.data;
                let shown = match display_name {
                    Some(name) => name.clone(),
                    None => data
                        .characters
                        .iter()
                        .find(|c| c.id == *speaker)
                        .map_or_else(|| speaker.clone(), |c| c.name.clone()),
                };
                let brackets = data
                    .brackets
                    .iter()
                    .find(|b| b.style == *style)
                    .map(|b| (b.open.as_str(), b.close.as_str()));
                // 空の表示名（名前を出さない）は、地の文と同じ。
                let name = (!shown.is_empty()).then_some(shown);
                self.say(name.as_deref(), brackets, voice, text);
            }
            Op::Narrate { text, .. } => self.say(None, None, None, text),
            Op::Break { .. } => {}
            Op::Bg {
                name, transition, ..
            } => {
                let fade = self.fade(transition.as_deref());
                match self.backgrounds.get(name).cloned() {
                    Some(label) => {
                        let mut args = vec![label];
                        args.resize(5, String::new());
                        args.push(fade);
                        self.command("Bg", &args);
                    }
                    None => self.comment(&format!("背景 {name}（定義が無い）")),
                }
            }
            Op::Show {
                character,
                expression,
                position,
                transition,
                ..
            } => {
                let fade = self.fade(transition.as_deref());
                let chara = self.charas.get(character).cloned();
                match chara.filter(|_| {
                    self.sprites
                        .contains(&(character.clone(), expression.clone()))
                }) {
                    Some(chara) => {
                        // 文の無い行は、ページを替えずに立ち絵を出す・表情と位置を変える。
                        // 位置は Character のレイヤー（1つのレイヤーに1人。プレビューの位置ごとに1人と同じ）。
                        let mut cells =
                            vec![(ARG1, chara), (ARG2, expression.clone()), (ARG6, fade)];
                        if let Some(layer) = position.and_then(|p| {
                            LAYERS
                                .iter()
                                .find(|(k, _)| *k == p)
                                .map(|(_, l)| (*l).to_owned())
                        }) {
                            cells.push((ARG3, layer));
                        }
                        self.push(&cells);
                    }
                    None => self.comment(&format!("立ち絵 {character} {expression}（定義が無い）")),
                }
            }
            Op::Hide {
                character,
                transition,
                ..
            } => {
                let fade = self.fade(transition.as_deref());
                match self.charas.get(character).cloned() {
                    Some(chara) => self.push(&[
                        (COMMAND, "CharacterOff".to_owned()),
                        (ARG1, chara),
                        (ARG6, fade),
                    ]),
                    None => self.comment(&format!("退場 {character}（表示していない）")),
                }
            }
            Op::Bgm { name, .. } => self.sound(name, true),
            Op::BgmStop { .. } => self.command("StopBgm", &[]),
            Op::Se { name, .. } => self.sound(name, false),
            Op::Fx {
                name, seconds: s, ..
            } => {
                match self.profile.fx.get(name).cloned() {
                    Some(row) => {
                        let mut cells = Vec::new();
                        for (i, v) in row.into_iter().enumerate() {
                            cells.push((COMMAND + i, v));
                        }
                        self.push(&cells);
                    }
                    None => {
                        self.warn(format!(
                            "効果「{name}」の対応がプロファイル（fx）に無いので、書き出していません"
                        ));
                        self.comment(&format!("効果 {name}（対応なし）"));
                    }
                }
                if let Some(s) = s {
                    self.push(&[(COMMAND, "Wait".to_owned()), (ARG6, seconds(*s))]);
                }
            }
            Op::Wait { seconds: s, .. } => {
                self.push(&[(COMMAND, "Wait".to_owned()), (ARG6, seconds(*s))]);
            }
            Op::Set { var, op, value, .. } => {
                let line = format!("{}{op}{}", self.param(var), self.expr(value));
                self.command("Param", &[line]);
            }
            Op::Choice {
                prompt, options, ..
            } => self.choice(prompt.as_deref(), options),
            Op::If {
                branches,
                else_body,
                ..
            } => self.if_chain(branches, else_body.as_deref()),
            Op::Jump { target, .. } => self.jump_scene("Jump", target),
            Op::Call { target, .. } => self.jump_scene("JumpSubroutine", target),
            Op::Return { .. } => self.command("EndSubroutine", &[]),
            Op::Ending { id, .. } => {
                self.comment(&format!(
                    "エンディング {}",
                    id.as_deref().unwrap_or("（IDの形が不正）")
                ));
                let row = self.profile.ending.clone();
                let cells: Vec<(usize, String)> = row.into_iter().enumerate().collect();
                self.push(&cells);
            }
            Op::Raw { engine, lines, .. } => {
                if engine.eq_ignore_ascii_case("utage") {
                    // 1行が1つの行。欄はタブで区切る（Command、Arg1〜Arg6、WaitType、Text、PageCtrl、Voice の順）。
                    for line in lines {
                        let cells: Vec<(usize, String)> =
                            line.split('\t').map(str::to_owned).enumerate().collect();
                        self.push(&cells);
                    }
                } else {
                    self.warn(format!(
                        "@raw {engine} は宴への書き出しでは使わないので、読み飛ばしました"
                    ));
                    self.comment(&format!("@raw {engine}（読み飛ばし）"));
                }
            }
            Op::UnknownDirective { name, .. } => {
                self.warn(format!(
                    "知らないディレクティブ @{name}（W401）は、書き出していません"
                ));
                self.comment(&format!("@{name}（知らないディレクティブ）"));
            }
        }
    }
}

/// 大文字と小文字を区別せずに重ならない名前（Excel のシートの名前は大文字と小文字を区別しない）。
fn unique_ascii_ci(taken: &mut BTreeSet<String>, wanted: &str) -> String {
    let clash =
        |taken: &BTreeSet<String>, name: &str| taken.iter().any(|t| t.eq_ignore_ascii_case(name));
    let mut name = wanted.to_owned();
    let mut n = 2;
    while clash(taken, &name) {
        name = format!("{wanted}_{n}");
        n += 1;
    }
    taken.insert(name.clone());
    name
}

/// シート・ラベルに使う名前（宴の素材のラベル。`/` などは `_` にする）。
fn asset_label(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_whitespace() || "/\\,[]*:?".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// IRとプロジェクトから、宴への書き出しを作る（ファイルは書かない）。
pub fn export_utage(
    project: &Project,
    root: &Path,
    ir: &Ir,
    profile: &UtageProfile,
) -> UtageExport {
    let data = preview_data(project, root, ir);
    let mut used = Used::default();
    for scene in &ir.scenes {
        used.ops(&scene.ops);
    }
    // パラメーターの名前（プロファイルで決めた名前を先に取っておく）。
    let mut param_names: BTreeSet<String> = profile.variables.values().cloned().collect();
    let mut params = BTreeMap::new();
    let declared: BTreeSet<&str> = project.variables.iter().map(|v| v.key.as_str()).collect();
    for key in project.variables.iter().map(|v| v.key.clone()).chain(
        used.variables
            .iter()
            .filter(|k| !declared.contains(k.as_str()))
            .cloned(),
    ) {
        let name = match profile.variables.get(&key) {
            Some(name) => name.clone(),
            None => unique(&mut param_names, &identifier(&key.replace('.', "_"), "p_")),
        };
        params.insert(key, name);
    }
    let mut exporter = Exporter {
        data: &data,
        profile,
        root,
        params,
        bools: project
            .variables
            .iter()
            .filter(|v| v.ty == VarType::Bool)
            .map(|v| v.key.clone())
            .collect(),
        temps: Vec::new(),
        labels: BTreeMap::new(),
        taken: [START_LABEL.to_owned()].into_iter().collect(),
        label: String::new(),
        next: 0,
        backgrounds: BTreeMap::new(),
        charas: BTreeMap::new(),
        sprites: BTreeSet::new(),
        bgm: BTreeMap::new(),
        se: BTreeMap::new(),
        character_rows: Vec::new(),
        texture_rows: Vec::new(),
        sound_rows: Vec::new(),
        copies: BTreeMap::new(),
        pngs: BTreeMap::new(),
        warnings: Vec::new(),
        rows: Vec::new(),
    };
    define_assets(&mut exporter, &used);

    // シーンのラベル（重なれば `_2` …。シーンIDが重なるシーン（E108）は後のものを書かない）。
    let mut skipped: BTreeSet<&str> = BTreeSet::new();
    for scene in &ir.scenes {
        if exporter.labels.contains_key(&scene.id) {
            exporter.warn(format!(
                "シーンID {} が重なっているので（E108）、後のシーンを書き出していません",
                scene.id
            ));
            skipped.insert(scene.id.as_str());
            continue;
        }
        let wanted: String = ascii_name(&scene.id, "s_")
            .chars()
            .take(LABEL_LIMIT)
            .collect();
        let label = unique_ascii_ci(&mut exporter.taken, &wanted);
        exporter.labels.insert(scene.id.clone(), label);
    }

    let mut scenario: Vec<UtageSheet> = Vec::new();
    let mut dead_ends = 0;
    for scene in &ir.scenes {
        if skipped.contains(scene.id.as_str()) {
            continue;
        }
        let Some(label) = exporter.labels.get(&scene.id).cloned() else {
            continue;
        };
        exporter.rows.clear();
        exporter.label.clone_from(&label);
        exporter.next = 0;
        let title = if scene.title.is_empty() {
            scene.id.clone()
        } else {
            format!("{} {}", scene.id, scene.title)
        };
        exporter.comment(&title);
        if scene.meta.status.as_deref() == Some("ai-draft") {
            exporter.comment("AI下書き（status: ai-draft）");
        }
        // シートの名前がシナリオラベルになるので、同じ名前の `*ラベル` は書かない（宴は重なったラベルを誤りにする）。
        exporter.ops(&scene.ops);
        if falls_through(&scene.ops) {
            dead_ends += 1;
            let what = if scene.is_stub {
                format!("{title}（スタブ）")
            } else {
                title.clone()
            };
            exporter.dead_end(&what);
        }
        scenario.push(UtageSheet {
            // シートの名前はシナリオラベル（31文字まで。ラベルを決めるときに切ってある）。
            name: label.clone(),
            header: SCENARIO_COLUMNS.iter().map(|s| (*s).to_owned()).collect(),
            rows: std::mem::take(&mut exporter.rows),
        });
    }
    if dead_ends > 0 {
        exporter.warn(format!(
            "出口の無い経路が残るシーンが {dead_ends} 個あります（W503）。終わりで知らせてシナリオを終えるようにしました"
        ));
    }
    // 開始のシート：開始シーン（`start_scene`、無ければ最初のシーン）へ飛ぶ。
    exporter.rows.clear();
    exporter.comment(
        "scn-studio が .scn から書き出したシナリオ。手で直さない（書き出すたびに作り直す）。",
    );
    let start = ir
        .start_scene
        .as_deref()
        .filter(|s| exporter.labels.contains_key(*s))
        .or_else(|| {
            ir.scenes
                .iter()
                .map(|s| s.id.as_str())
                .find(|id| exporter.labels.contains_key(*id))
        });
    match start {
        Some(start) => exporter.jump_scene("Jump", start),
        None => exporter.command("EndScenario", &[]),
    }
    scenario.insert(
        0,
        UtageSheet {
            name: START_LABEL.to_owned(),
            header: SCENARIO_COLUMNS.iter().map(|s| (*s).to_owned()).collect(),
            rows: std::mem::take(&mut exporter.rows),
        },
    );

    let settings = settings_sheets(&mut exporter, &used);
    let mut files = Vec::new();
    let book = |sheets: &[UtageSheet], widths: &[u32]| -> Option<Vec<u8>> {
        let headers: Vec<Vec<&str>> = sheets
            .iter()
            .map(|s| s.header.iter().map(String::as_str).collect())
            .collect();
        let list: Vec<Sheet<'_>> = sheets
            .iter()
            .zip(&headers)
            .map(|(s, h)| Sheet {
                name: &s.name,
                header: h,
                rows: &s.rows,
                widths,
            })
            .collect();
        xlsx_book(&list).ok()
    };
    match book(&scenario, &[14, 14, 12, 10, 8, 8, 8, 10, 48, 10, 24]) {
        Some(bytes) => files.push(UtageFile::Bytes {
            path: format!("{}.xlsx", profile.book),
            bytes,
        }),
        None => exporter.warn("シナリオが大きすぎて Excel 形式にできません".to_owned()),
    }
    match book(&settings, &[16, 16, 16, 24]) {
        Some(bytes) => files.push(UtageFile::Bytes {
            path: format!("{}_settings.xlsx", profile.book),
            bytes,
        }),
        None => exporter.warn("設定が大きすぎて Excel 形式にできません".to_owned()),
    }
    for (path, bytes) in std::mem::take(&mut exporter.pngs) {
        files.push(UtageFile::Bytes { path, bytes });
    }
    for (path, from) in std::mem::take(&mut exporter.copies) {
        files.push(UtageFile::Copy { path, from });
    }
    files.push(UtageFile::Bytes {
        path: MARK_FILE.to_owned(),
        bytes: MARK.as_bytes().to_vec(),
    });
    files.sort_by(|a, b| a.path().cmp(b.path()));
    UtageExport {
        scenario,
        settings,
        files,
        warnings: exporter.warnings,
    }
}

/// 使われている素材のラベルを決め、設定のシートの行と、写すファイルと仮素材を集める。
fn define_assets(exporter: &mut Exporter<'_>, used: &Used) {
    let data = exporter.data;
    let root = exporter.root;
    // 仮素材の大きさ（1色なので、大きな画面でも 4096 までにする）。
    let clamp = |v: u64| u32::try_from(v.min(4096)).unwrap_or(4096);
    let (sw, sh) = (u64::from(data.screen.width), u64::from(data.screen.height));
    let mut placeholders = 0;
    let copy = |exporter: &mut Exporter<'_>, folder: &str, file: &str| -> Option<String> {
        let relative = assets_relative(Path::new(file), root)?;
        let name = format!("scn/{relative}");
        exporter
            .copies
            .insert(format!("{folder}/{name}"), file.to_owned());
        Some(name)
    };
    let mut labels: BTreeSet<String> = BTreeSet::new();
    for name in &used.backgrounds {
        let file = data.assets.backgrounds.get(name).cloned().flatten();
        let file_name = match file
            .as_deref()
            .and_then(|f| copy(exporter, "Texture/Bg", f))
        {
            Some(name) => name,
            None => {
                placeholders += 1;
                let png = format!("scn/{}.png", mark_of("ph_", name));
                exporter.pngs.insert(
                    format!("Texture/Bg/{png}"),
                    placeholder_png(clamp(sw), clamp(sh), &color_of(name)),
                );
                png
            }
        };
        let label = unique(&mut labels, &asset_label(name));
        exporter
            .texture_rows
            .push(vec![label.clone(), "Bg".to_owned(), file_name]);
        exporter.backgrounds.insert(name.clone(), label);
    }
    // キャラクター：CharacterName（台詞の名前と分ける）と、差分ごとのパターン。
    let mut taken: BTreeSet<String> = exporter.profile.characters.values().cloned().collect();
    let mut per_chara: BTreeMap<String, Vec<Vec<String>>> = BTreeMap::new();
    for (character, expression) in &used.sprites {
        let chara = match exporter.charas.get(character) {
            Some(chara) => chara.clone(),
            None => {
                let chara = match exporter.profile.characters.get(character) {
                    Some(name) => name.clone(),
                    None => unique(&mut taken, &format!("ch_{}", ascii_name(character, "c_"))),
                };
                exporter.charas.insert(character.clone(), chara.clone());
                chara
            }
        };
        let file = data
            .assets
            .expressions
            .get(character)
            .and_then(|e| e.get(expression))
            .cloned()
            .flatten();
        let copied = file
            .as_deref()
            .and_then(|f| copy(exporter, "Texture/Character", f));
        let real = copied.is_some();
        let file_name = match copied {
            Some(name) => name,
            None => {
                placeholders += 1;
                let png = format!(
                    "scn/{}.png",
                    mark_of("ph_", &format!("{character}/{expression}"))
                );
                exporter.pngs.insert(
                    format!("Texture/Character/{png}"),
                    placeholder_png(
                        clamp(sw * 3 / 10),
                        clamp(sh * 17 / 20),
                        &color_of(character),
                    ),
                );
                png
            }
        };
        let shown = data
            .characters
            .iter()
            .find(|c| c.id == *character)
            .map_or(character.as_str(), |c| c.name.as_str());
        let rows = per_chara.entry(chara.clone()).or_default();
        let row = vec![
            chara.clone(),
            shown.to_owned(),
            expression.clone(),
            file_name,
        ];
        // 初めの行が既定のパターン：ファイルのある差分を先にする。
        if real
            && rows
                .first()
                .and_then(|r| r.get(3))
                .is_some_and(|f| f.contains("/ph_"))
        {
            rows.insert(0, row);
        } else {
            rows.push(row);
        }
        exporter
            .sprites
            .insert((character.clone(), expression.clone()));
    }
    for rows in per_chara.into_values() {
        exporter.character_rows.extend(rows);
    }
    // 台詞の名前（表示の名前）が CharacterName と同じだと、宴は話すたびに立ち絵を出す。
    let clashes: Vec<String> = exporter
        .charas
        .values()
        .filter(|name| data.characters.iter().any(|c| c.name == **name))
        .cloned()
        .collect();
    for name in clashes {
        exporter.warn(format!(
            "CharacterName「{name}」がキャラクターの表示の名前と同じなので、宴では話すたびに立ち絵が出ます（プロファイルの characters で変えます）"
        ));
    }
    if placeholders > 0 {
        exporter.warn(format!(
            "ファイルの無い画像が {placeholders} 個あります（色の矩形の PNG にしました）"
        ));
    }
    let mut silent = 0;
    let mut sound_labels: BTreeSet<String> = BTreeSet::new();
    for name in &used.bgm {
        let sound = data.assets.bgm.get(name);
        match sound.and_then(|s| {
            s.file
                .as_deref()
                .and_then(|f| copy(exporter, "Sound/Bgm", f))
        }) {
            Some(file_name) => {
                let label = unique(&mut sound_labels, &asset_label(name));
                exporter
                    .sound_rows
                    .push(vec![label.clone(), "Bgm".to_owned(), file_name]);
                let repeat = sound.is_none_or(|s| s.repeat);
                exporter.bgm.insert(name.clone(), (label, repeat));
            }
            None => silent += 1,
        }
    }
    for name in &used.se {
        let sound = data.assets.se.get(name);
        match sound.and_then(|s| {
            s.file
                .as_deref()
                .and_then(|f| copy(exporter, "Sound/Se", f))
        }) {
            Some(file_name) => {
                let label = unique(&mut sound_labels, &asset_label(name));
                exporter
                    .sound_rows
                    .push(vec![label.clone(), "Se".to_owned(), file_name]);
                exporter.se.insert(name.clone(), label);
            }
            None => silent += 1,
        }
    }
    if silent > 0 {
        exporter.warn(format!(
            "ファイルの無い音が {silent} 個あります（鳴らさずに、コメントを書きました）"
        ));
    }
}

/// 設定のシート：Character・Texture・Sound・Param。
fn settings_sheets(exporter: &mut Exporter<'_>, used: &Used) -> Vec<UtageSheet> {
    let sheet = |name: &str, header: &[&str], rows: Vec<Vec<String>>| UtageSheet {
        name: name.to_owned(),
        header: header.iter().map(|s| (*s).to_owned()).collect(),
        rows,
    };
    // パラメーター（宣言の初期値。宣言の無い変数は Int の 0。永続の変数は FileType を System）。
    let data = exporter.data;
    let mut params = Vec::new();
    let mut seen = BTreeSet::new();
    for v in &data.variables {
        seen.insert(v.key.clone());
        let ty = match v.ty {
            "bool" => "Bool",
            "int" => "Int",
            _ => "String",
        };
        let value = match &v.initial {
            serde_json::Value::Bool(b) => if *b { "true" } else { "false" }.to_owned(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => s.clone(),
            _ => String::new(),
        };
        let file_type = if v.scope.as_deref() == Some("persistent") {
            "System"
        } else {
            ""
        };
        params.push(vec![
            exporter.param(&v.key),
            ty.to_owned(),
            value,
            file_type.to_owned(),
        ]);
    }
    let undeclared: Vec<String> = used
        .variables
        .iter()
        .filter(|k| !seen.contains(*k))
        .cloned()
        .collect();
    for key in &undeclared {
        params.push(vec![
            exporter.param(key),
            "Int".to_owned(),
            "0".to_owned(),
            String::new(),
        ]);
    }
    if !undeclared.is_empty() {
        exporter.warn(format!(
            "宣言の無い変数が {} 個あります（W602）。初期値を 0 にしました",
            undeclared.len()
        ));
    }
    for (name, ty) in &exporter.temps {
        let value = match *ty {
            "Bool" => "false",
            "Int" => "0",
            _ => "",
        };
        params.push(vec![
            name.clone(),
            (*ty).to_owned(),
            value.to_owned(),
            String::new(),
        ]);
    }
    vec![
        sheet(
            "Character",
            &["CharacterName", "NameText", "Pattern", "FileName"],
            std::mem::take(&mut exporter.character_rows),
        ),
        sheet(
            "Texture",
            &["Label", "Type", "FileName"],
            std::mem::take(&mut exporter.texture_rows),
        ),
        sheet(
            "Sound",
            &["Label", "Type", "FileName"],
            std::mem::take(&mut exporter.sound_rows),
        ),
        sheet("Param", &["Label", "Type", "Value", "FileType"], params),
        sheet(
            "Layer",
            &["LayerName", "Type", "X", "Y", "Order"],
            LAYERS
                .iter()
                .zip(0..)
                .map(|((position, name), i)| {
                    vec![
                        (*name).to_owned(),
                        "Character".to_owned(),
                        exporter.x_of(position).to_string(),
                        "0".to_owned(),
                        (100 + i).to_string(),
                    ]
                })
                .collect(),
        ),
    ]
}

/// 書き出す。書き出し先（`out`、無ければプロファイルの `output`）を消して作り直す。書き出し先があって、書き出した
/// ものでなければ（印のファイルが無ければ）、消さずに誤りを返す。書き出し先を返す。
pub fn write_utage(
    root: &Path,
    profile: &UtageProfile,
    export: &UtageExport,
    out: Option<&Path>,
) -> Result<PathBuf, ProjectError> {
    let base = out.map_or_else(|| root.join(&profile.output), Path::to_path_buf);
    let error = |path: &Path, source: std::io::Error| ProjectError::Write {
        path: path.to_owned(),
        source,
    };
    if base.exists() {
        let empty = fs::read_dir(&base)
            .map_err(|e| error(&base, e))?
            .next()
            .is_none();
        let ours = fs::read_to_string(base.join(MARK_FILE)).is_ok_and(|text| text == MARK);
        if !empty && !ours {
            return Err(error(
                &base,
                std::io::Error::other(
                    "scn-studio が書き出したフォルダではないので、消さずに止めました",
                ),
            ));
        }
        let entries = fs::read_dir(&base).map_err(|e| error(&base, e))?;
        for entry in entries {
            let path = entry.map_err(|e| error(&base, e))?.path();
            if path.file_name() == Some(MARK_FILE.as_ref()) {
                continue;
            }
            let removed = if path.is_dir() && !path.is_symlink() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
            removed.map_err(|e| error(&path, e))?;
        }
    }
    for file in &export.files {
        let path = base.join(file.path());
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| error(dir, e))?;
        }
        match file {
            UtageFile::Bytes { bytes, .. } => {
                fs::write(&path, bytes).map_err(|e| error(&path, e))?
            }
            UtageFile::Copy { from, .. } => {
                fs::copy(from, &path).map_err(|e| error(&path, e))?;
            }
        }
    }
    Ok(base)
}
