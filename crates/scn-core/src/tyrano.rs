//! ティラノスクリプトへの書き出し（T3-11、D072）：IRを、出力プロファイル（`export/tyrano.yaml`）に従って
//! ティラノスクリプト（`.ks`）にする。書き出しは一方向で、生成物を手で直さない（`docs/design.md` の「エクスポート」）。
//! 素材の扱いは Ren'Py と同じ考え（D053）。
//!
//! 書き出し先（既定は `export/tyrano/`。ティラノスクリプトのプロジェクトのフォルダ）の `data/scenario/scn/` と、
//! 素材の `data/{bgimage,fgimage,bgm,sound}/scn/` だけを、書き出すたびに作り直す。人が置いたほかのファイル
//! （`first.ks`・`title.ks`、`system/` など）には触れない（`first: true` のときの `first.ks` を除く）。
//!
//! - シーンはラベル（`*s012`）。ティラノスクリプトのラベルは次のラベルへ落ちるので、出口の無い経路が残るシーン
//!   （W503）の終わりには、行き止まりを知らせて止める文を置く（Ren'Py と同じ）。
//! - 流れはラベルとジャンプだけで書く（`[if]` の中にラベルを置かない）：選択肢は `[glink]` と `[s]` で選択肢ごとの
//!   ラベルへ飛び、中身の後で続きのラベルへ飛ぶ。`@if` は条件つきの `[jump cond=…]` で枝のラベルへ飛ぶ。
//! - 変数はゲーム変数 `f['akari.好感度']`（`scope: persistent` はシステム変数 `sf[…]`）。名前を変えない。
//! - 素材は `assets/` から数えたパスのまま写す。ファイルの無い画像は、名前入りの色の矩形の SVG（仮素材）。
//!   ファイルの無い音は鳴らさない。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use yaml_serde::Value as Yaml;

use crate::assets::assets_relative;
use crate::check::falls_through;
use crate::ir::{Ir, IrExpr, IrInline, IrOption, Op};
use crate::preview::{PreviewData, preview_data};
use crate::project::{Project, ProjectError, VarType, without_bom};
use crate::renpy::{
    Used, color_of, hash_of, image_word, mark_of, position_key, relative_inside, yaml_map,
    yaml_string, yaml_text,
};

/// 出力プロファイルの置き場所（作品フォルダから）。
pub const TYRANO_PROFILE: &str = "export/tyrano.yaml";

/// 書き出し先の既定（作品フォルダから）。
pub const DEFAULT_OUTPUT: &str = "export/tyrano";

/// 書き出すたびに作り直すフォルダ（書き出し先から）。シナリオと、素材の種類ごと。
pub const GENERATED_DIRS: [&str; 5] = [
    "data/scenario/scn",
    "data/bgimage/scn",
    "data/fgimage/scn",
    "data/bgm/scn",
    "data/sound/scn",
];

/// 書き出したフォルダの印のファイル（中身は [`HEADER`]）。
pub const MARK_FILE: &str = ".scn-studio";

/// 書き出したファイルの1行目（印）。
const HEADER: &str =
    "; scn-studio が .scn から書き出したファイル。手で直さない（書き出すたびに作り直す）。\n";

/// 定義と開始のファイル（`data/scenario/` から）。
pub const START_STORAGE: &str = "scn/scn_start.ks";

/// `first: true` のとき書く、ティラノスクリプトの最初のシナリオ（書き出し先から）。
const FIRST_FILE: &str = "data/scenario/first.ks";

/// 出力プロファイル。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TyranoProfile {
    /// 書き出し先（作品フォルダから）。
    pub output: String,
    /// 立ち絵の左端（px。IRの値 `left`・`center`・`right` から）。書かなければ画面の幅から決める。
    pub positions: BTreeMap<String, i64>,
    /// トランジションの名前から、かける時間（ミリ秒）。
    pub transitions: BTreeMap<String, u64>,
    /// `@効果` の名前から、ティラノスクリプトのタグ（例：`[quake count=3 time=300]`）。
    pub fx: BTreeMap<String, String>,
    /// ボイスのファイル名（`{char_id}`・`{line_id}`。作品フォルダの `assets/` から数える）。
    pub voice: Option<String>,
    /// エンディングで書くタグ。
    pub ending: String,
    /// キャラクターIDから、ティラノスクリプトのキャラクターの名前（`chara_new` の `name`）へ。
    pub characters: BTreeMap<String, String>,
    /// `data/scenario/first.ks`（開始のファイルへ飛ぶだけ）を書くか。
    pub first: bool,
    /// 開始のファイルで書く準備のタグ（メッセージ枠と名前の欄）。`None` は既定（画面の大きさから決める）。
    pub setup: Option<String>,
}

impl Default for TyranoProfile {
    fn default() -> Self {
        Self {
            output: DEFAULT_OUTPUT.to_owned(),
            positions: BTreeMap::new(),
            transitions: [("フェード".to_owned(), 1000), ("即時".to_owned(), 0)]
                .into_iter()
                .collect(),
            fx: BTreeMap::new(),
            voice: None,
            ending: "[jump storage=\"title.ks\"]".to_owned(),
            characters: BTreeMap::new(),
            first: false,
            setup: None,
        }
    }
}

/// 出力プロファイルを読む。無ければ既定。値の誤りは読み飛ばして既定を使い、何を読み飛ばしたかを返す
/// （書き出しは止めない。D053 と同じ）。知らないキーは黙って読み飛ばす。
pub fn read_profile(root: &Path) -> (TyranoProfile, Vec<String>) {
    let mut profile = TyranoProfile::default();
    let mut problems = Vec::new();
    let text = match fs::read_to_string(root.join(TYRANO_PROFILE)) {
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
        Some(target) if target != "tyrano" => {
            problems.push(format!("target が tyrano ではありません（{target}）"));
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
    for (key, value) in yaml_map(&yaml, "positions", &mut problems) {
        let key = position_key(&key);
        if !matches!(key, "left" | "center" | "right") {
            continue;
        }
        match value.trim().parse::<i64>() {
            Ok(px) => {
                profile.positions.insert(key.to_owned(), px);
            }
            Err(_) => problems.push(format!(
                "positions の {key} は、立ち絵の左端の位置（px の整数）で書きます"
            )),
        }
    }
    for (key, value) in yaml_map(&yaml, "transitions", &mut problems) {
        // `null`（`yaml_map` は "None" にする）は、時間をかけない。
        let ms = if value == "None" {
            Some(0)
        } else {
            value.trim().parse::<u64>().ok()
        };
        match ms {
            Some(ms) => {
                profile.transitions.insert(key, ms);
            }
            None => problems.push(format!(
                "transitions の {key} は、かける時間（ミリ秒の整数）で書きます"
            )),
        }
    }
    profile.fx = yaml_map(&yaml, "fx", &mut problems);
    for (key, value) in yaml_map(&yaml, "characters", &mut problems) {
        if css_name(&value) {
            profile.characters.insert(key, value);
        } else {
            problems.push(format!(
                "characters の {key} は、英字で始まる英数字と _ の名前で書きます（{value}）"
            ));
        }
    }
    match yaml.get("voice") {
        None | Some(Yaml::Null) => {}
        Some(voice) => match voice.get("pattern").and_then(yaml_text) {
            Some(pattern) => profile.voice = Some(pattern),
            None => problems.push("voice は { pattern: \"…\" } で書きます".to_owned()),
        },
    }
    if let Some(ending) = yaml_string(&yaml, "ending", &mut problems) {
        profile.ending = ending;
    }
    match yaml.get("setup") {
        None => {}
        // `null` は何も書かない（準備は人のシナリオで行う）。
        Some(Yaml::Null) => profile.setup = Some(String::new()),
        Some(value) => match yaml_text(value) {
            Some(text) => profile.setup = Some(text),
            None => problems.push("setup は文字列（タグの行）で書きます".to_owned()),
        },
    }
    match yaml.get("first") {
        None | Some(Yaml::Null) => {}
        Some(Yaml::Bool(b)) => profile.first = *b,
        Some(_) => problems.push("first は true か false で書きます".to_owned()),
    }
    (profile, problems)
}

/// 書き出すファイル。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TyranoFile {
    /// 書き出し先の中の相対パスと中身。
    Text { path: String, text: String },
    /// 書き出し先の中の相対パスと、写す元（絶対パス）。
    Copy { path: String, from: String },
}

impl TyranoFile {
    pub fn path(&self) -> &str {
        match self {
            TyranoFile::Text { path, .. } | TyranoFile::Copy { path, .. } => path,
        }
    }
}

/// 書き出しの結果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TyranoExport {
    pub files: Vec<TyranoFile>,
    /// 知らせ（対応の無いトランジションや効果、仮素材、行き止まりなど）。
    pub warnings: Vec<String>,
}

/// CSS のクラスの名前として使える名前か（ティラノスクリプトはキャラクターの名前をクラスの名前に使う）。
fn css_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// ラベル・キャラクター・表情の名前（英数字と `_`。英字で始まらなければ `prefix` を付ける。英数字が無ければ
/// 名前から決まる印）。
fn ascii_name(text: &str, prefix: &str) -> String {
    let word: String = text
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if !word.chars().any(|c| c.is_ascii_alphanumeric()) || word.chars().all(|c| c == '_') {
        return mark_of(prefix, text);
    }
    // 英数字でない文字を含む名前は、ほかの名前と重ならないよう印を足す。
    let word = if word == text {
        word
    } else {
        format!("{word}_{:04x}", hash_of(text) & 0xffff)
    };
    if word.starts_with(|c: char| c.is_ascii_alphabetic()) {
        word
    } else {
        format!("{prefix}{word}")
    }
}

/// 重ならない名前（重なれば `_2`、`_3` …を付ける）。
fn unique(taken: &mut BTreeSet<String>, wanted: &str) -> String {
    let mut name = wanted.to_owned();
    let mut n = 2;
    while taken.contains(&name) {
        name = format!("{wanted}_{n}");
        n += 1;
    }
    taken.insert(name.clone());
    name
}

/// JavaScript の文字列（`'…'`）。タグの属性（`"…"`）の中に書くので、`"` と、タグの区切りの `[`・`]` も
/// エスケープする。
fn js_string(text: &str) -> String {
    let mut out = String::from("'");
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '"' => out.push_str("\\x22"),
            '[' => out.push_str("\\x5b"),
            ']' => out.push_str("\\x5d"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// 画面に見せる属性の値（`text`・`jname`、ルビの読み）。HTML として入るので、`&`・`<`・`>`・`"` と、タグの区切りの
/// `[`・`]` を文字参照にする。
fn shown_attr(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '[' => out.push_str("&#91;"),
            ']' => out.push_str("&#93;"),
            '\n' | '\r' => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

/// 画面に見せる属性の値（[`shown_attr`]）。ティラノスクリプトは、値の初めの `&` を式、`%` をマクロの引数として
/// 読むので、その文字で始まる値は文字列の式（`&'…'`）にする。
fn shown_value(text: &str) -> String {
    let shown = shown_attr(text);
    if shown.starts_with(['&', '%']) {
        format!("&{}", js_string(&shown))
    } else {
        shown
    }
}

/// 1文字を、そのまま見せるタグ（`[emb]`）にする。
fn emb_char(c: char) -> String {
    format!("[emb exp=\"String.fromCharCode({})\"]", u32::from(c))
}

/// 本文の文字。`[` はタグとして読まれるので、そのまま見せるタグにする。
fn body_text(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c == '[' {
            out.push_str(&emb_char(c));
        } else {
            out.push(c);
        }
    }
    out
}

/// 秒をミリ秒にする。
fn millis(seconds: f64) -> u64 {
    let ms = (seconds * 1000.0).round();
    if ms.is_finite() && ms > 0.0 {
        // f64 から整数へは、範囲の中に丸めてから。
        ms.min(u64::MAX as f64) as u64
    } else {
        0
    }
}

/// XML の文字（仮素材の SVG）。
fn xml_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// 仮素材の SVG（名前入りの色の矩形）。
fn placeholder_svg(width: u64, height: u64, color: &str, label: &str, size: u64) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\"><rect width=\"100%\" height=\"100%\" fill=\"{color}\"/><text x=\"{pad}\" y=\"{y}\" font-size=\"{size}\" fill=\"#ffffff\" font-family=\"sans-serif\">{}</text></svg>\n",
        xml_text(label),
        pad = size,
        y = size * 2,
    )
}

/// シーンの書き先（章のファイルと、ラベル）。
#[derive(Debug, Clone)]
struct Place {
    storage: String,
    label: String,
}

/// 書き出しの途中の状態。
struct Exporter<'a> {
    data: &'a PreviewData<'a>,
    profile: &'a TyranoProfile,
    root: &'a Path,
    /// 真偽と宣言された変数。
    bools: BTreeSet<String>,
    /// `scope: persistent` の変数。
    persistent: BTreeSet<String>,
    /// シーンIDから、書き先。
    places: BTreeMap<String, Place>,
    /// 今のシーンのラベル（作るラベルの名前の元）と、作ったラベルの数。
    label: String,
    next: usize,
    /// 使っているラベル（シーンと、作ったもの）。
    labels: BTreeSet<String>,
    /// 背景の意味名から、`data/bgimage/` から数えたパス。
    backgrounds: BTreeMap<String, String>,
    /// （キャラクターID、差分）から、（キャラクターの名前、表情の名前）。
    sprites: BTreeMap<(String, String), (String, String)>,
    /// キャラクターIDから、キャラクターの名前。
    charas: BTreeMap<String, String>,
    /// 定義のタグ（`chara_new`・`chara_face`）。
    chara_defs: Vec<String>,
    /// 音（意味名から、`data/bgm/`・`data/sound/` から数えたパス。BGM はくり返すかも）。
    bgm: BTreeMap<String, (String, bool)>,
    se: BTreeMap<String, String>,
    /// 写すファイル（書き出し先の中のパスから、写す元）と、書く SVG。
    copies: BTreeMap<String, String>,
    svgs: BTreeMap<String, String>,
    warnings: Vec<String>,
    out: String,
}

impl Exporter<'_> {
    fn warn(&mut self, message: String) {
        if !self.warnings.contains(&message) {
            self.warnings.push(message);
        }
    }

    fn line(&mut self, text: &str) {
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn comment(&mut self, text: &str) {
        self.line(&format!("; {}", text.replace('\n', " ")));
    }

    /// シーンの中で使う、新しいラベル。
    fn new_label(&mut self) -> String {
        loop {
            self.next += 1;
            let name = format!("{}__{}", self.label, self.next);
            if self.labels.insert(name.clone()) {
                return name;
            }
        }
    }

    fn label_line(&mut self, name: &str) {
        self.line(&format!("*{name}"));
    }

    /// 同じファイルの中のラベルへ飛ぶ（`cond` があれば、そのときだけ）。
    fn local_jump(&mut self, label: &str, cond: Option<&str>) {
        match cond {
            Some(cond) => self.line(&format!("[jump target=\"*{label}\" cond=\"{cond}\"]")),
            None => self.line(&format!("[jump target=\"*{label}\"]")),
        }
    }

    fn var(&self, key: &str) -> String {
        let base = if self.persistent.contains(key) {
            "sf"
        } else {
            "f"
        };
        format!("{base}[{}]", js_string(key))
    }

    /// 式を JavaScript にする。
    fn expr(&self, e: &IrExpr) -> String {
        match e {
            IrExpr::Int { value } => format!("({value})"),
            IrExpr::String { value } => js_string(value),
            IrExpr::Bool { value } => if *value { "true" } else { "false" }.to_owned(),
            IrExpr::Var { name } => self.var(name),
            IrExpr::Unary { op, operand } => match *op {
                "not" => format!("(!{})", self.expr(operand)),
                _ => format!("(-{})", self.expr(operand)),
            },
            IrExpr::Binary { op, lhs, rhs } => {
                let (l, r) = (self.expr(lhs), self.expr(rhs));
                match *op {
                    // 整数の割り算は0の向きへ切り捨てる（仕様書14節）。
                    "/" => format!("Math.trunc({l} / {r})"),
                    "and" => format!("({l} && {r})"),
                    "or" => format!("({l} || {r})"),
                    // 型も比べる（仕様書14節）。
                    "==" => format!("({l} === {r})"),
                    "!=" => format!("({l} !== {r})"),
                    _ => format!("({l} {op} {r})"),
                }
            }
        }
    }

    /// 条件の式。構文の崩れた条件（E601）は、プレビューと同じく偽にする。
    fn cond(&self, cond: Option<&IrExpr>) -> String {
        cond.map_or_else(|| "false".to_owned(), |c| self.expr(c))
    }

    /// 真偽になる式か（埋め込みで `true`・`false` と見せる。JavaScript でも同じ文字になるが、型を確かめる）。
    fn is_bool(&self, e: &IrExpr) -> bool {
        match e {
            IrExpr::Bool { .. } => true,
            IrExpr::Var { name } => self.bools.contains(name),
            IrExpr::Unary { op, .. } => *op == "not",
            IrExpr::Binary { op, .. } => {
                matches!(*op, "and" | "or" | "==" | "!=" | "<" | "<=" | ">" | ">=")
            }
            _ => false,
        }
    }

    /// 文中の記法を、メッセージの文字（タグを含む）にする。`plain`（選択肢と問いかけ文）では待ちと速さと効果音を
    /// 書かない。
    fn inline_text(&mut self, text: &[IrInline], plain: bool) -> String {
        let mut out = String::new();
        let mut speed = false;
        for inline in text {
            match inline {
                IrInline::Text { text } => out.push_str(&body_text(text)),
                IrInline::Emphasis { text } => {
                    // 傍点：1文字ずつ、ルビに「﹅」。
                    for c in text.chars() {
                        let _ = write!(out, "[ruby text=\"﹅\"]{}", body_text(&c.to_string()));
                    }
                }
                IrInline::Ruby { base, reading } => {
                    // ティラノスクリプトのルビは次の1文字に付くので、読みを親文字の初めの文字に付ける。
                    let _ = write!(
                        out,
                        "[ruby text=\"{}\"]{}",
                        shown_value(reading),
                        body_text(base)
                    );
                }
                IrInline::LineBreak => out.push_str("[r]"),
                IrInline::Interpolation { expr, expr_text } => match expr {
                    Some(e) => {
                        let js = self.expr(e);
                        let js = if self.is_bool(e) {
                            format!("({js} ? 'true' : 'false')")
                        } else {
                            js
                        };
                        let _ = write!(out, "[emb exp=\"{js}\"]");
                    }
                    // 構文の崩れた式（E601）は、プレビューと同じく `{=式}` のまま見せる。
                    None => out.push_str(&body_text(&format!("{{={expr_text}}}"))),
                },
                IrInline::Wait { seconds } => {
                    if !plain {
                        let _ = write!(out, "[wait time=\"{}\"]", millis(*seconds));
                    }
                }
                IrInline::Speed { speed: s } => {
                    if plain {
                        continue;
                    }
                    match *s {
                        "slow" => {
                            out.push_str("[delay speed=\"60\"]");
                            speed = true;
                        }
                        "fast" => {
                            out.push_str("[delay speed=\"10\"]");
                            speed = true;
                        }
                        _ => {
                            out.push_str("[resetdelay]");
                            speed = false;
                        }
                    }
                }
                IrInline::PageBreak => {
                    if !plain {
                        out.push_str("[p]");
                    }
                }
                IrInline::Se { name } => {
                    if plain {
                        self.warn(format!(
                            "選択肢と問いかけ文の中の効果音（{{se {name}}}）は、書き出していません"
                        ));
                        continue;
                    }
                    match self.se.get(name).cloned() {
                        Some(path) => {
                            let _ = write!(out, "[playse storage=\"{path}\"]");
                        }
                        None => self.warn(format!(
                            "ファイルの無い効果音（{{se {name}}}）は、鳴らさずに書き出しました"
                        )),
                    }
                }
            }
        }
        if speed {
            out.push_str("[resetdelay]");
        }
        out
    }

    /// メッセージの行を書く。行の初めの空白（段落頭の全角空白を含む）と、ティラノスクリプトが行の初めで特別に
    /// 読む文字（`@`・`*`・`;`・`#`）は、そのまま見せるタグにする（ティラノスクリプトは行の前後の空白を除く）。
    fn message_line(&mut self, text: &str) {
        let mut chars = text.chars();
        let line = match chars.next() {
            Some(c) if c.is_whitespace() || matches!(c, '@' | '*' | ';' | '#') => {
                format!("{}{}", emb_char(c), chars.as_str())
            }
            _ => text.to_owned(),
        };
        self.line(&line);
    }

    /// 台詞・地の文：名前の行（`#名前`。地の文は `#` で消す）と、本文と `[p]`。括弧は本文の初めと終わりに付ける
    /// （`{p}` の後のページには付けない。プレビューと同じ）。
    fn say(&mut self, name: Option<&str>, brackets: Option<(&str, &str)>, text: &[IrInline]) {
        self.line(&format!("#{}", name.unwrap_or_default()));
        let body = self.inline_text(text, false);
        // 初めと終わりの `{p}` は、空のページになるので書かない（プレビューと同じ）。
        let mut body = body.as_str();
        while let Some(rest) = body.strip_prefix("[p]") {
            body = rest;
        }
        while let Some(rest) = body.strip_suffix("[p]") {
            body = rest;
        }
        let (open, close) = brackets.unwrap_or(("", ""));
        let body = format!("{}{body}{}[p]", body_text(open), body_text(close));
        self.message_line(&body);
    }

    /// 行き止まり：知らせてから、エンディングと同じくタイトルへ戻る。
    fn dead_end(&mut self, what: &str) {
        self.line("#");
        self.message_line(&format!(
            "{}[p]",
            body_text(&format!("（行き止まり：{what}）"))
        ));
        let ending = self.profile.ending.clone();
        self.line(&ending);
    }

    /// シーンへ飛ぶ・呼ぶ。無いシーン（E501）は行き止まり。
    fn jump_scene(&mut self, keyword: &str, target: &str) {
        match self.places.get(target).cloned() {
            Some(place) => self.line(&format!(
                "[{keyword} storage=\"{}\" target=\"*{}\"]",
                place.storage, place.label
            )),
            None => {
                self.warn(format!(
                    "存在しないシーン {target} へのジャンプ・呼び出しがあります（E501）。そこで止まるようにしました"
                ));
                self.dead_end(&format!("存在しないシーン {target}"));
            }
        }
    }

    fn transition(&mut self, name: Option<&str>) -> u64 {
        let Some(name) = name.filter(|n| !n.is_empty()) else {
            return 0;
        };
        match self.profile.transitions.get(name) {
            Some(ms) => *ms,
            None => {
                self.warn(format!(
                    "トランジション「{name}」の対応がプロファイル（transitions）に無いので、500 ミリ秒にしました"
                ));
                500
            }
        }
    }

    /// 立ち絵の左端（px）。
    fn left_of(&self, position: &str) -> i64 {
        if let Some(px) = self.profile.positions.get(position) {
            return *px;
        }
        let width = i64::from(self.data.screen.width);
        match position {
            "left" => width * 5 / 100,
            "right" => width * 65 / 100,
            _ => width * 35 / 100,
        }
    }

    /// ボイス：プロファイルの `voice` のファイルが `assets/` にあれば、写して鳴らす。
    fn voice(&mut self, speaker: &str, line_id: Option<&str>) {
        let (Some(pattern), Some(id)) = (self.profile.voice.as_deref(), line_id) else {
            return;
        };
        let relative = pattern
            .replace("{char_id}", speaker)
            .replace("{line_id}", id);
        if !relative_inside(&relative) {
            return;
        }
        // 素材と同じく、`assets/` の中の実在するファイルだけを使う（D050）。
        let source = fs::canonicalize(self.root.join("assets").join(&relative)).ok();
        let assets = fs::canonicalize(self.root.join("assets")).ok();
        if let (Some(source), Some(assets)) = (source, assets)
            && source.starts_with(&assets)
            && source.is_file()
        {
            let storage = format!("scn/{relative}");
            self.copies.insert(
                format!("data/sound/{storage}"),
                source.display().to_string(),
            );
            self.line(&format!("[playse storage=\"{storage}\" buf=\"1\"]"));
        }
    }

    fn ops(&mut self, ops: &[Op]) {
        for op in ops {
            self.op(op);
        }
    }

    /// 選択肢：問いかけ文を見せ、`[glink]` で選択肢ごとのラベルへ飛ぶ（`[s]` で待つ）。選択肢のラベルでは、中身の
    /// 後で行き先か続きのラベルへ飛ぶ。
    fn choice(&mut self, prompt: Option<&[IrInline]>, options: &[IrOption]) {
        if options.is_empty() {
            self.comment("選択肢の無い @選択（E106）");
            self.dead_end("選択肢がありません");
            return;
        }
        let conds: Vec<Option<String>> = options
            .iter()
            .map(|o| match (&o.cond, &o.cond_text) {
                (Some(c), _) => Some(self.expr(c)),
                (None, Some(_)) => Some("false".to_owned()),
                (None, None) => None,
            })
            .collect();
        // 選択肢がすべて条件付きで、どれも選べないときは、プレビューと同じく、そこで止まる。
        if conds.iter().all(Option::is_some) {
            let any = conds
                .iter()
                .flatten()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(" || ");
            self.line(&format!("[if exp=\"!({any})\"]"));
            self.dead_end("選べる選択肢がありません");
            self.line("[endif]");
        }
        let end = self.new_label();
        let targets: Vec<String> = options.iter().map(|_| self.new_label()).collect();
        if let Some(prompt) = prompt {
            self.line("#");
            let text = self.inline_text(prompt, true);
            self.message_line(&text);
        }
        let width = u64::from(self.data.screen.width);
        let height = u64::from(self.data.screen.height);
        let button = width * 6 / 10;
        let gap = (height / 9).max(48);
        let count = u64::try_from(options.len()).unwrap_or(1);
        let top = (height / 2).saturating_sub(gap * count / 2);
        for (i, ((option, target), cond)) in options.iter().zip(&targets).zip(&conds).enumerate() {
            let text = self.option_text(&option.text);
            let index = u64::try_from(i).unwrap_or(0);
            let mut tag = format!(
                "[glink color=\"black\" size=\"24\" width=\"{button}\" x=\"{}\" y=\"{}\" target=\"*{target}\" text=\"{text}\"",
                (width - button) / 2,
                top + gap * index
            );
            if let Some(cond) = cond {
                let _ = write!(tag, " cond=\"{cond}\"");
            }
            tag.push(']');
            self.line(&tag);
        }
        self.line("[s]");
        for (option, target) in options.iter().zip(&targets) {
            self.label_line(target);
            self.line("[cm]");
            self.ops(&option.body);
            match option.jump.as_deref() {
                Some(to) => self.jump_scene("jump", to),
                None => self.local_jump(&end, None),
            }
        }
        self.label_line(&end);
    }

    /// 選択肢の文（`[glink]` の `text`）。変数の埋め込みがあれば、属性の値を式（`&…`）にする。ルビと傍点は
    /// 付けられないので、親文字だけにする。
    fn option_text(&mut self, text: &[IrInline]) -> String {
        let mut pieces: Vec<String> = Vec::new();
        let mut plain = String::new();
        let mut dynamic = false;
        for inline in text {
            match inline {
                IrInline::Text { text } | IrInline::Emphasis { text } => {
                    if matches!(inline, IrInline::Emphasis { .. }) {
                        self.warn(
                            "選択肢の文の傍点は、ティラノスクリプトの選択肢には付けられないので、付けずに書き出しました"
                                .to_owned(),
                        );
                    }
                    plain.push_str(text);
                }
                IrInline::Ruby { base, .. } => {
                    self.warn(
                        "選択肢の文のルビは、ティラノスクリプトの選択肢には付けられないので、親文字だけにしました"
                            .to_owned(),
                    );
                    plain.push_str(base);
                }
                IrInline::Interpolation { expr, expr_text } => match expr {
                    Some(e) => {
                        if !plain.is_empty() {
                            pieces.push(js_string(&shown_attr(&std::mem::take(&mut plain))));
                        }
                        let js = self.expr(e);
                        pieces.push(if self.is_bool(e) {
                            format!("({js} ? 'true' : 'false')")
                        } else {
                            format!("String({js})")
                        });
                        dynamic = true;
                    }
                    None => plain.push_str(&format!("{{={expr_text}}}")),
                },
                IrInline::LineBreak => plain.push(' '),
                IrInline::Se { name } => self.warn(format!(
                    "選択肢と問いかけ文の中の効果音（{{se {name}}}）は、書き出していません"
                )),
                IrInline::Wait { .. } | IrInline::Speed { .. } | IrInline::PageBreak => {}
            }
        }
        if !dynamic {
            return shown_value(&plain);
        }
        if !plain.is_empty() {
            pieces.push(js_string(&shown_attr(&plain)));
        }
        format!("&{}", pieces.join(" + "))
    }

    /// `@if`：条件つきのジャンプで枝のラベルへ飛び、枝の後で続きのラベルへ飛ぶ。
    fn if_chain(&mut self, branches: &[crate::ir::IrBranch], else_body: Option<&[Op]>) {
        let end = self.new_label();
        let targets: Vec<String> = branches.iter().map(|_| self.new_label()).collect();
        let otherwise = else_body.map(|_| self.new_label());
        for (branch, target) in branches.iter().zip(&targets) {
            let cond = self.cond(branch.cond.as_ref());
            self.local_jump(target, Some(&cond));
        }
        self.local_jump(otherwise.as_deref().unwrap_or(end.as_str()), None);
        for (branch, target) in branches.iter().zip(&targets) {
            self.label_line(target);
            self.ops(&branch.body);
            self.local_jump(&end, None);
        }
        if let (Some(label), Some(body)) = (otherwise, else_body) {
            self.label_line(&label);
            self.ops(body);
            self.local_jump(&end, None);
        }
        self.label_line(&end);
    }

    fn sound(&mut self, name: &str, music: bool) {
        if music {
            match self.bgm.get(name).cloned() {
                Some((path, repeat)) => {
                    self.line(&format!("[playbgm storage=\"{path}\" loop=\"{repeat}\"]"))
                }
                None => {
                    self.comment(&format!("BGM {name}（ファイルが無いので鳴らさない）"));
                    self.line("[stopbgm]");
                }
            }
        } else {
            match self.se.get(name).cloned() {
                Some(path) => self.line(&format!("[playse storage=\"{path}\"]")),
                None => self.comment(&format!("SE {name}（ファイルが無いので鳴らさない）")),
            }
        }
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
                self.voice(speaker, line_id.as_deref());
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
                self.say(Some(&shown), brackets, text);
            }
            Op::Narrate { text, .. } => self.say(None, None, text),
            Op::Break { .. } => {}
            Op::Bg {
                name, transition, ..
            } => {
                let time = self.transition(transition.as_deref());
                match self.backgrounds.get(name).cloned() {
                    Some(path) => {
                        self.line(&format!("[bg storage=\"{path}\" time=\"{time}\"]"));
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
                let time = self.transition(transition.as_deref());
                match self
                    .sprites
                    .get(&(character.clone(), expression.clone()))
                    .cloned()
                {
                    Some((chara, face)) => {
                        // 表示していれば表情を変え、していなければ次に見せる表情にする。
                        self.line(&format!(
                            "[chara_mod name=\"{chara}\" face=\"{face}\" time=\"0\"]"
                        ));
                        let left = position.map(|p| self.left_of(p));
                        let at = left.map(|x| format!(" left=\"{x}\"")).unwrap_or_default();
                        self.line(&format!(
                            "[chara_show name=\"{chara}\" face=\"{face}\" time=\"{time}\"{at}]"
                        ));
                        // 表示していたキャラクターは `chara_show` では動かないので、位置へ動かす（すぐに。待ちを増やさない）。
                        if let Some(x) = left {
                            self.line(&format!(
                                "[chara_move name=\"{chara}\" left=\"{x}\" time=\"1\"]"
                            ));
                        }
                    }
                    None => self.comment(&format!("立ち絵 {character} {expression}（定義が無い）")),
                }
            }
            Op::Hide {
                character,
                transition,
                ..
            } => {
                let time = self.transition(transition.as_deref());
                match self.charas.get(character).cloned() {
                    Some(chara) => {
                        self.line(&format!("[chara_hide name=\"{chara}\" time=\"{time}\"]"));
                    }
                    // 本文で一度も表示しないキャラクターは、隠すものが無い。
                    None => self.comment(&format!("退場 {character}（表示していない）")),
                }
            }
            Op::Bgm { name, .. } => self.sound(name, true),
            Op::BgmStop { .. } => self.line("[stopbgm]"),
            Op::Se { name, .. } => self.sound(name, false),
            Op::Fx {
                name, seconds: s, ..
            } => {
                match self.profile.fx.get(name).cloned() {
                    Some(tag) => self.line(&tag),
                    None => {
                        self.warn(format!(
                            "効果「{name}」の対応がプロファイル（fx）に無いので、書き出していません"
                        ));
                        self.comment(&format!("効果 {name}（対応なし）"));
                    }
                }
                if let Some(s) = s {
                    self.line(&format!("[wait time=\"{}\"]", millis(*s)));
                }
            }
            Op::Wait { seconds: s, .. } => self.line(&format!("[wait time=\"{}\"]", millis(*s))),
            Op::Set { var, op, value, .. } => {
                let target = self.var(var);
                let value = self.expr(value);
                self.line(&format!("[eval exp=\"{target} {op} {value}\"]"));
            }
            Op::Choice {
                prompt, options, ..
            } => self.choice(prompt.as_deref(), options),
            Op::If {
                branches,
                else_body,
                ..
            } => self.if_chain(branches, else_body.as_deref()),
            Op::Jump { target, .. } => self.jump_scene("jump", target),
            Op::Call { target, .. } => self.jump_scene("call", target),
            Op::Return { .. } => self.line("[return]"),
            Op::Ending { id, .. } => {
                self.comment(&format!(
                    "エンディング {}",
                    id.as_deref().unwrap_or("（IDの形が不正）")
                ));
                let ending = self.profile.ending.clone();
                self.line(&ending);
            }
            Op::Raw { engine, lines, .. } => {
                if engine.eq_ignore_ascii_case("tyrano")
                    || engine.eq_ignore_ascii_case("tyranoscript")
                {
                    for line in lines {
                        self.line(line);
                    }
                } else {
                    self.warn(format!(
                        "@raw {engine} はティラノスクリプトへの書き出しでは使わないので、読み飛ばしました"
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

/// シーンIDから、ラベルの名前。
fn scene_label(id: &str) -> String {
    ascii_name(id, "s_")
}

/// IRとプロジェクトから、ティラノスクリプトの書き出しを作る（ファイルは書かない）。
pub fn export_tyrano(
    project: &Project,
    root: &Path,
    ir: &Ir,
    profile: &TyranoProfile,
) -> TyranoExport {
    let data = preview_data(project, root, ir);
    let mut used = Used::default();
    for scene in &ir.scenes {
        used.ops(&scene.ops);
    }
    let mut exporter = Exporter {
        data: &data,
        profile,
        root,
        bools: project
            .variables
            .iter()
            .filter(|v| v.ty == VarType::Bool)
            .map(|v| v.key.clone())
            .collect(),
        persistent: project
            .variables
            .iter()
            .filter(|v| v.scope.as_deref() == Some("persistent"))
            .map(|v| v.key.clone())
            .collect(),
        places: BTreeMap::new(),
        label: String::new(),
        next: 0,
        labels: BTreeSet::new(),
        backgrounds: BTreeMap::new(),
        sprites: BTreeMap::new(),
        charas: BTreeMap::new(),
        chara_defs: Vec::new(),
        bgm: BTreeMap::new(),
        se: BTreeMap::new(),
        copies: BTreeMap::new(),
        svgs: BTreeMap::new(),
        warnings: Vec::new(),
        out: String::new(),
    };
    define_assets(&mut exporter, &used);

    // シーンの書き先（章のファイルごと）。ラベルの名前が重なれば `_2` …を付ける。シーンIDが重なるシーン（E108）は
    // 後のものを書かない。
    let mut skipped: BTreeSet<&str> = BTreeSet::new();
    for scene in &ir.scenes {
        if exporter.places.contains_key(&scene.id) {
            exporter.warn(format!(
                "シーンID {} が重なっているので（E108）、後のシーンを書き出していません",
                scene.id
            ));
            skipped.insert(scene.id.as_str());
            continue;
        }
        let label = unique(&mut exporter.labels, &scene_label(&scene.id));
        let stem = crate::ir::anonymous_stem(&scene.file);
        let stem = if stem.is_empty() {
            "story".to_owned()
        } else {
            stem
        };
        exporter.places.insert(
            scene.id.clone(),
            Place {
                storage: format!("scn/story_{}.ks", ascii_name(&stem, "f_")),
                label,
            },
        );
    }

    let mut chapters: Vec<(String, String)> = Vec::new();
    let mut dead_ends = 0;
    for scene in &ir.scenes {
        if skipped.contains(scene.id.as_str()) {
            continue;
        }
        let Some(place) = exporter.places.get(&scene.id).cloned() else {
            continue;
        };
        exporter.out.clear();
        exporter.label = place.label.clone();
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
        exporter.label_line(&place.label);
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
        exporter.out.push('\n');
        let body = std::mem::take(&mut exporter.out);
        match chapters.iter_mut().find(|(name, _)| *name == place.storage) {
            Some((_, text)) => text.push_str(&body),
            None => chapters.push((place.storage.clone(), body)),
        }
    }
    if dead_ends > 0 {
        exporter.warn(format!(
            "出口の無い経路が残るシーンが {dead_ends} 個あります（W503）。終わりで知らせてタイトルへ戻るようにしました"
        ));
    }

    let mut files: Vec<TyranoFile> = chapters
        .into_iter()
        .map(|(storage, body)| TyranoFile::Text {
            path: format!("data/scenario/{storage}"),
            text: format!("{HEADER}\n{body}"),
        })
        .collect();
    let start = start_file(&mut exporter, &used, ir);
    files.push(TyranoFile::Text {
        path: format!("data/scenario/{START_STORAGE}"),
        text: start,
    });
    if profile.first {
        files.push(TyranoFile::Text {
            path: FIRST_FILE.to_owned(),
            text: format!(
                "{HEADER}; 開始のファイル（定義と、開始シーンへのジャンプ）へ飛ぶ。\n[jump storage=\"{START_STORAGE}\"]\n"
            ),
        });
    }
    for (path, text) in std::mem::take(&mut exporter.svgs) {
        files.push(TyranoFile::Text { path, text });
    }
    for (path, from) in std::mem::take(&mut exporter.copies) {
        files.push(TyranoFile::Copy { path, from });
    }
    // 書き出したフォルダの印（作り直すフォルダごと。中身のあるフォルダとシナリオのフォルダ）。
    let mut marked: BTreeSet<&str> = BTreeSet::new();
    for dir in GENERATED_DIRS {
        if dir == "data/scenario/scn"
            || files
                .iter()
                .any(|f| f.path().starts_with(&format!("{dir}/")))
        {
            marked.insert(dir);
        }
    }
    for dir in marked {
        files.push(TyranoFile::Text {
            path: format!("{dir}/{MARK_FILE}"),
            text: HEADER.to_owned(),
        });
    }
    files.sort_by(|a, b| a.path().cmp(b.path()));
    TyranoExport {
        files,
        warnings: exporter.warnings,
    }
}

/// キャラクターの定義：名前、表示の名前、表情（表情の名前、`data/fgimage/` から数えたパス、ファイルがあるか）。
struct CharaDef {
    name: String,
    jname: String,
    faces: Vec<(String, String, bool)>,
}

/// 使われている素材の書き先を決め、写すファイルと仮素材の SVG を集め、キャラクターの定義のタグを作る。
fn define_assets(exporter: &mut Exporter<'_>, used: &Used) {
    let data = exporter.data;
    let root = exporter.root;
    let (sw, sh) = (u64::from(data.screen.width), u64::from(data.screen.height));
    let mut placeholders = 0;
    let copy = |exporter: &mut Exporter<'_>, kind: &str, file: &str| -> Option<String> {
        let relative = assets_relative(Path::new(file), root)?;
        let storage = format!("scn/{relative}");
        exporter
            .copies
            .insert(format!("data/{kind}/{storage}"), file.to_owned());
        Some(storage)
    };
    for name in &used.backgrounds {
        let file = data.assets.backgrounds.get(name).cloned().flatten();
        let storage = match file.as_deref().and_then(|f| copy(exporter, "bgimage", f)) {
            Some(storage) => storage,
            None => {
                placeholders += 1;
                let storage = format!("scn/{}.svg", mark_of("ph_", name));
                exporter.svgs.insert(
                    format!("data/bgimage/{storage}"),
                    placeholder_svg(
                        sw,
                        sh,
                        &color_of(name),
                        &format!("背景：{name}（仮素材）"),
                        36,
                    ),
                );
                storage
            }
        };
        exporter.backgrounds.insert(name.clone(), storage);
    }
    // キャラクターの名前（プロファイルで決めた名前を先に取っておく）と表情。
    let mut taken: BTreeSet<String> = exporter.profile.characters.values().cloned().collect();
    let mut faces: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    // キャラクターごとの、表示の名前と表情（表情の名前、`data/fgimage/` から数えたパス、ファイルがあるか）。
    let mut charas: Vec<CharaDef> = Vec::new();
    for (character, expression) in &used.sprites {
        let chara = match exporter.charas.get(character) {
            Some(chara) => chara.clone(),
            None => {
                let chara = match exporter.profile.characters.get(character) {
                    Some(name) => name.clone(),
                    None => unique(&mut taken, &ascii_name(character, "c_")),
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
        let copied = file.as_deref().and_then(|f| copy(exporter, "fgimage", f));
        let real = copied.is_some();
        let storage = match copied {
            Some(storage) => storage,
            None => {
                placeholders += 1;
                let shown = data
                    .characters
                    .iter()
                    .find(|c| c.id == *character)
                    .map_or(character.as_str(), |c| c.name.as_str());
                let storage = format!(
                    "scn/{}.svg",
                    mark_of("ph_", &format!("{character}/{expression}"))
                );
                exporter.svgs.insert(
                    format!("data/fgimage/{storage}"),
                    placeholder_svg(
                        sw * 3 / 10,
                        sh * 17 / 20,
                        &color_of(character),
                        &format!("{shown}：{expression}（仮素材）"),
                        28,
                    ),
                );
                storage
            }
        };
        let taken_faces = faces.entry(chara.clone()).or_default();
        let face = image_word(expression)
            .filter(|w| css_name(w))
            .unwrap_or_else(|| mark_of("f_", expression));
        let face = unique(taken_faces, &face);
        match charas.iter_mut().find(|c| c.name == chara) {
            Some(def) => def.faces.push((face.clone(), storage, real)),
            None => {
                let jname = data
                    .characters
                    .iter()
                    .find(|c| c.id == *character)
                    .map_or(character.as_str(), |c| c.name.as_str());
                charas.push(CharaDef {
                    name: chara.clone(),
                    jname: jname.to_owned(),
                    faces: vec![(face.clone(), storage, real)],
                });
            }
        }
        exporter
            .sprites
            .insert((character.clone(), expression.clone()), (chara, face));
    }
    // 定義：`chara_new` の初めの画像は、ファイルのある表情（無ければ初めの表情）。
    for CharaDef {
        name: chara,
        jname,
        faces: list,
    } in &charas
    {
        let first = list
            .iter()
            .find(|(_, _, real)| *real)
            .or_else(|| list.first())
            .map(|(_, storage, _)| storage.clone())
            .unwrap_or_default();
        exporter.chara_defs.push(format!(
            "[chara_new name=\"{chara}\" storage=\"{first}\" jname=\"{}\"]",
            shown_value(jname)
        ));
        for (face, storage, _) in list {
            exporter.chara_defs.push(format!(
                "[chara_face name=\"{chara}\" face=\"{face}\" storage=\"{storage}\"]"
            ));
        }
    }
    if placeholders > 0 {
        exporter.warn(format!(
            "ファイルの無い画像が {placeholders} 個あります（名前入りの色の矩形の SVG にしました）"
        ));
    }
    let mut silent = 0;
    for name in &used.bgm {
        let sound = data.assets.bgm.get(name);
        match sound.and_then(|s| s.file.as_deref().and_then(|f| copy(exporter, "bgm", f))) {
            Some(path) => {
                let repeat = sound.is_none_or(|s| s.repeat);
                exporter.bgm.insert(name.clone(), (path, repeat));
            }
            None => silent += 1,
        }
    }
    for name in &used.se {
        let sound = data.assets.se.get(name);
        match sound.and_then(|s| s.file.as_deref().and_then(|f| copy(exporter, "sound", f))) {
            Some(path) => {
                exporter.se.insert(name.clone(), path);
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

/// 開始のファイル：キャラクターの定義、変数の初期値、開始シーンへのジャンプ。
fn start_file(exporter: &mut Exporter<'_>, used: &Used, ir: &Ir) -> String {
    let mut out = String::from(HEADER);
    out.push_str(
        "; 準備（メッセージ枠と名前の欄）、定義（キャラクターと変数）と、開始シーンへのジャンプ。first.ks か title.ks から [jump storage=\"scn/scn_start.ks\"] で始める。\n\n*start\n",
    );
    // 舞台を空にする（エンディングからタイトルへ戻って始め直したとき、前の立ち絵や音を残さない）。
    out.push_str("[clearstack]\n[cm]\n[chara_hide_all time=\"0\"]\n[stopbgm]\n");
    out.push_str(&setup_tags(exporter.profile, exporter.data));
    for def in &exporter.chara_defs {
        out.push_str(def);
        out.push('\n');
    }
    // 変数（宣言の初期値。宣言の無い変数は 0）。システム変数（永続）は、まだ値が無いときだけ。
    let data = exporter.data;
    let mut seen = BTreeSet::new();
    for v in &data.variables {
        seen.insert(v.key.clone());
        let value = match &v.initial {
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => js_string(s),
            _ => "null".to_owned(),
        };
        let target = exporter.var(&v.key);
        if exporter.persistent.contains(&v.key) {
            let _ = writeln!(
                out,
                "[eval exp=\"if ({target} === undefined) {target} = {value}\"]"
            );
        } else {
            let _ = writeln!(out, "[eval exp=\"{target} = {value}\"]");
        }
    }
    let undeclared: Vec<&String> = used
        .variables
        .iter()
        .filter(|k| !seen.contains(*k))
        .collect();
    for key in &undeclared {
        let target = exporter.var(key);
        let _ = writeln!(out, "[eval exp=\"{target} = 0\"]");
    }
    if !undeclared.is_empty() {
        exporter.warn(format!(
            "宣言の無い変数が {} 個あります（W602）。初期値を 0 にしました",
            undeclared.len()
        ));
    }
    let start = ir
        .start_scene
        .as_deref()
        .filter(|s| exporter.places.contains_key(*s))
        .or_else(|| {
            ir.scenes
                .iter()
                .map(|s| s.id.as_str())
                .find(|id| exporter.places.contains_key(*id))
        });
    match start.and_then(|s| exporter.places.get(s).cloned()) {
        Some(place) => {
            let _ = writeln!(
                out,
                "[jump storage=\"{}\" target=\"*{}\"]",
                place.storage, place.label
            );
        }
        None => out.push_str("; シーンがありません\n[s]\n"),
    }
    out
}

/// 準備のタグ：プロファイルの `setup`、無ければ既定（画面の大きさから決めたメッセージ枠と名前の欄。`#名前` には
/// `chara_config` の `ptext` が要る）。
fn setup_tags(profile: &TyranoProfile, data: &PreviewData<'_>) -> String {
    if let Some(setup) = &profile.setup {
        let mut text = setup.trim_end().to_owned();
        if !text.is_empty() {
            text.push('\n');
        }
        return text;
    }
    let (w, h) = (u64::from(data.screen.width), u64::from(data.screen.height));
    let (left, top, width, height) = (w / 8, h * 69 / 100, w * 3 / 4, h * 27 / 100);
    format!(
        "; メッセージ枠と名前の欄（プロファイルの setup で変える）\n\
         [layopt layer=\"message0\" visible=\"true\"]\n\
         [position layer=\"message0\" left=\"{left}\" top=\"{top}\" width=\"{width}\" height=\"{height}\" page=\"fore\" visible=\"true\"]\n\
         [position layer=\"message0\" page=\"fore\" margint=\"45\" marginl=\"50\" marginr=\"70\" marginb=\"60\"]\n\
         [free name=\"chara_name_area\" layer=\"message0\"]\n\
         [ptext name=\"chara_name_area\" layer=\"message0\" color=\"white\" size=\"28\" bold=\"true\" x=\"{}\" y=\"{}\"]\n\
         [chara_config ptext=\"chara_name_area\"]\n",
        left + 20,
        top + 10
    )
}

/// 書き出したフォルダか（印のファイルがある）。
fn ours(dir: &Path) -> bool {
    fs::read_to_string(dir.join(MARK_FILE)).is_ok_and(|text| text.starts_with(HEADER))
}

/// 書き出す。書き出し先（`out`、無ければプロファイルの `output`）の作り直すフォルダ（[`GENERATED_DIRS`]）を消して
/// から作り直す。作り直すフォルダがあって書き出したものでなければ（印が無ければ）、`first.ks` を書くときにそれが
/// 書き出したものでなければ、何も消さずに誤りを返す。書き出し先を返す。
pub fn write_tyrano(
    root: &Path,
    profile: &TyranoProfile,
    export: &TyranoExport,
    out: Option<&Path>,
) -> Result<PathBuf, ProjectError> {
    let base = out.map_or_else(|| root.join(&profile.output), Path::to_path_buf);
    let error = |path: &Path, source: std::io::Error| ProjectError::Write {
        path: path.to_owned(),
        source,
    };
    // 先にすべて確かめる（途中で止めて、一部だけ消さないため）。
    for dir in GENERATED_DIRS {
        let generated = base.join(dir);
        if generated.exists() && !ours(&generated) {
            return Err(error(
                &generated,
                std::io::Error::other(
                    "scn-studio が書き出したフォルダではないので、消さずに止めました",
                ),
            ));
        }
    }
    let first = base.join(FIRST_FILE);
    if export.files.iter().any(|f| f.path() == FIRST_FILE)
        && first.exists()
        && !fs::read_to_string(&first).is_ok_and(|text| text.starts_with(HEADER))
    {
        return Err(error(
            &first,
            std::io::Error::other(
                "first.ks が scn-studio の書き出したものではないので、上書きせずに止めました（プロファイルの first を外すか、first.ks から scn/scn_start.ks へ飛ぶ）",
            ),
        ));
    }
    for dir in GENERATED_DIRS {
        let generated = base.join(dir);
        if !generated.exists() {
            continue;
        }
        // 印のファイルは最後に消す（途中で消せないファイルがあっても、次の書き出しで消せるように）。
        let entries = fs::read_dir(&generated).map_err(|e| error(&generated, e))?;
        for entry in entries {
            let path = entry.map_err(|e| error(&generated, e))?.path();
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
        fs::remove_dir_all(&generated).map_err(|e| error(&generated, e))?;
    }
    for file in &export.files {
        let path = base.join(file.path());
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| error(dir, e))?;
        }
        match file {
            TyranoFile::Text { text, .. } => fs::write(&path, text).map_err(|e| error(&path, e))?,
            TyranoFile::Copy { from, .. } => {
                fs::copy(from, &path).map_err(|e| error(&path, e))?;
            }
        }
    }
    Ok(base)
}
