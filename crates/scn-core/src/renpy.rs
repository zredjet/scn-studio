//! Ren'Py への書き出し（T2-2、D053）：IRを、出力プロファイル（`export/renpy.yaml`）に従って Ren'Py の
//! スクリプトにする。書き出しは一方向で、生成物を手で直さない（`docs/design.md` の「エクスポート」）。
//!
//! 書き出し先（既定は `export/renpy/`）の `game/scn/` の下だけを、書き出すたびに作り直す。人が置いた
//! `game/` のほかのファイル（`options.rpy`、`gui.rpy`、フォントなど）には触れない。
//!
//! - シーンはラベル。`.scn` ではシーンの終わりで次のシーンへ進まない（仕様書13節）が、Ren'Py のラベルは
//!   次のラベルへ落ちるので、出口の無い経路が残るシーン（W503）の終わりには、行き止まりを知らせて止める文を置く。
//! - `@背景`・`@立ち絵`・`@退場` は `scene`・`show`・`hide`、`@BGM`・`@SE` は `play`、選択肢は `menu`、
//!   `@if` は `if`、`@設定` は `$`、ジャンプと呼び出しは `jump`・`call`・`return`、エンディングはプロファイルの文。
//! - 素材は意味名から Ren'Py の画像の名前を作る（プロファイルの `images`）。カタログのファイルがあれば
//!   `game/scn/` の下に写し、無ければ名前入りの色の矩形（仮素材）を定義する。ファイルの無い音は鳴らさない。
//! - 変数とキャラクターIDは、Python の名前として使える形にする（プロファイルの `variables`・`characters`
//!   で上書きできる）。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use yaml_serde::Value as Yaml;

use crate::check::falls_through;
use crate::ir::{Ir, IrExpr, IrInline, IrOption, Op};
use crate::preview::{PreviewData, preview_data};
use crate::project::{Project, ProjectError, VarType};

/// 出力プロファイルの置き場所（作品フォルダから）。
pub const RENPY_PROFILE: &str = "export/renpy.yaml";

/// 書き出し先の既定（作品フォルダから）。
pub const DEFAULT_OUTPUT: &str = "export/renpy";

/// 書き出し先の中で、書き出すたびに作り直すフォルダ。
pub const GENERATED_DIR: &str = "game/scn";

/// `game/scn/` が書き出したものであることの印（書き出したファイルの1行目）。
const HEADER: &str =
    "# scn-studio が .scn から書き出したファイル。手で直さない（書き出すたびに作り直す）。\n";

/// 定義のファイル（画面の大きさ、フォント、キャラクター、変数、画像）。
const DEFS_FILE: &str = "scn_defs.rpy";

/// 出力プロファイル。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RenpyProfile {
    /// 書き出し先（作品フォルダから）。
    pub output: String,
    /// 背景の画像の名前（`{file_stem}` を置き換える）。
    pub background_image: String,
    /// 立ち絵の画像の名前（`{char_id}`・`{expr_stem}` を置き換える）。
    pub sprite_image: String,
    /// 位置（IRの値 `left`・`center`・`right`）から Ren'Py の位置へ。
    pub positions: BTreeMap<String, String>,
    /// トランジションの名前から Ren'Py のトランジションへ（`None` は付けない）。
    pub transitions: BTreeMap<String, String>,
    /// `@効果` の名前から Ren'Py の文へ（例：`with vpunch`）。
    pub fx: BTreeMap<String, String>,
    /// ボイスのファイル名（`{char_id}`・`{line_id}`。作品フォルダの `assets/` から数える）。
    pub voice: Option<String>,
    /// エンディングで書く文。
    pub ending: String,
    /// 変数名（`akari.好感度`）から Ren'Py の名前へ（書かなければ自動で作る）。
    pub variables: BTreeMap<String, String>,
    /// キャラクターIDから Ren'Py の名前へ（書かなければ自動で作る）。
    pub characters: BTreeMap<String, String>,
    /// 台詞のフォント（作品フォルダから数えたパスか、絶対パス）。日本語を見せるのに要る。
    pub font: Option<String>,
    /// `label start` を書くか。
    pub start_label: bool,
}

impl Default for RenpyProfile {
    fn default() -> Self {
        let pairs = |items: &[(&str, &str)]| {
            items
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect::<BTreeMap<_, _>>()
        };
        Self {
            output: DEFAULT_OUTPUT.to_owned(),
            background_image: "bg {file_stem}".to_owned(),
            sprite_image: "{char_id} {expr_stem}".to_owned(),
            positions: pairs(&[("left", "left"), ("center", "center"), ("right", "right")]),
            transitions: pairs(&[("フェード", "fade"), ("即時", "None")]),
            fx: BTreeMap::new(),
            voice: None,
            ending: "$ renpy.full_restart()".to_owned(),
            variables: BTreeMap::new(),
            characters: BTreeMap::new(),
            font: None,
            start_label: true,
        }
    }
}

fn yaml_text(value: &Yaml) -> Option<String> {
    match value {
        Yaml::String(s) => Some(s.clone()),
        Yaml::Number(n) => Some(n.to_string()),
        Yaml::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// 文字列から文字列への対応表を読む。値の `null` は `None`（トランジションを付けない）。
fn yaml_map(root: &Yaml, key: &str, problems: &mut Vec<String>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    match root.get(key) {
        None | Some(Yaml::Null) => {}
        Some(Yaml::Mapping(map)) => {
            for (k, v) in map {
                let Some(k) = yaml_text(k) else {
                    problems.push(format!("{key} のキーは文字列で書きます"));
                    continue;
                };
                match v {
                    Yaml::Null => {
                        out.insert(k, "None".to_owned());
                    }
                    v => match yaml_text(v) {
                        Some(v) => {
                            out.insert(k, v);
                        }
                        None => problems.push(format!("{key} の {k} の値は文字列で書きます")),
                    },
                }
            }
        }
        Some(_) => problems.push(format!("{key} は対応表で書きます")),
    }
    out
}

fn yaml_string(root: &Yaml, key: &str, problems: &mut Vec<String>) -> Option<String> {
    match root.get(key) {
        None | Some(Yaml::Null) => None,
        Some(value) => {
            let text = yaml_text(value);
            if text.is_none() {
                problems.push(format!("{key} は文字列で書きます"));
            }
            text
        }
    }
}

/// 位置のキーワードを、IRの値（英語）にそろえる。
fn position_key(key: &str) -> &str {
    match key {
        "左" => "left",
        "中" => "center",
        "右" => "right",
        _ => key,
    }
}

/// 出力プロファイルを読む。無ければ既定。値の誤りは読み飛ばして既定を使い、何を読み飛ばしたかを返す
/// （書き出しは止めない。D053）。知らないキー（`command` など、後のフェーズで使うものを含む）は、作品フォルダの
/// ほかのファイルと同じく、黙って読み飛ばす（`docs/spec/project-files.md`）。
pub fn read_profile(root: &Path) -> (RenpyProfile, Vec<String>) {
    let mut profile = RenpyProfile::default();
    let mut problems = Vec::new();
    let text = match fs::read_to_string(root.join(RENPY_PROFILE)) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (profile, problems),
        Err(e) => {
            problems.push(format!("読めません：{e}。既定の設定で書き出します"));
            return (profile, problems);
        }
    };
    let yaml: Yaml = match yaml_serde::from_str(&text) {
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
        Some(target) if target != "renpy" => {
            problems.push(format!("target が renpy ではありません（{target}）"));
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
    for (key, value) in yaml_map(&yaml, "images", &mut problems) {
        match key.as_str() {
            "背景" | "background" => profile.background_image = value,
            "立ち絵" | "sprite" => profile.sprite_image = value,
            _ => {}
        }
    }
    for (key, value) in yaml_map(&yaml, "positions", &mut problems) {
        let key = position_key(&key);
        if matches!(key, "left" | "center" | "right") {
            profile.positions.insert(key.to_owned(), value);
        }
    }
    profile
        .transitions
        .extend(yaml_map(&yaml, "transitions", &mut problems));
    profile.fx = yaml_map(&yaml, "fx", &mut problems);
    profile.variables = yaml_map(&yaml, "variables", &mut problems);
    profile.characters = yaml_map(&yaml, "characters", &mut problems);
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
    profile.font = yaml_string(&yaml, "font", &mut problems);
    match yaml.get("start_label") {
        None | Some(Yaml::Null) => {}
        Some(Yaml::Bool(b)) => profile.start_label = *b,
        Some(_) => problems.push("start_label は true か false で書きます".to_owned()),
    }
    (profile, problems)
}

/// 作品フォルダの中を指す相対パスか（`..` や絶対パスでない）。
fn relative_inside(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// 書き出すファイル。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RenpyFile {
    /// 書き出し先の中の相対パスと中身。
    Text { path: String, text: String },
    /// 書き出し先の中の相対パスと、写す元（絶対パス）。
    Copy { path: String, from: String },
}

impl RenpyFile {
    pub fn path(&self) -> &str {
        match self {
            RenpyFile::Text { path, .. } | RenpyFile::Copy { path, .. } => path,
        }
    }
}

/// 書き出しの結果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RenpyExport {
    pub files: Vec<RenpyFile>,
    /// 知らせ（対応の無いトランジションや効果、仮素材、行き止まり、フォントなど）。
    pub warnings: Vec<String>,
}

/// Python の予約語と、Ren'Py の名前空間で使われている名前。変数やキャラクターの名前にしない。
const RESERVED: &[&str] = &[
    "False",
    "None",
    "True",
    "and",
    "as",
    "assert",
    "async",
    "await",
    "break",
    "class",
    "continue",
    "def",
    "del",
    "elif",
    "else",
    "except",
    "finally",
    "for",
    "from",
    "global",
    "if",
    "import",
    "in",
    "is",
    "lambda",
    "nonlocal",
    "not",
    "or",
    "pass",
    "raise",
    "return",
    "try",
    "while",
    "with",
    "yield",
    "match",
    "case",
    "type",
    "adv",
    "nvl",
    "narrator",
    "extend",
    "centered",
    "vcentered",
    "name_only",
    "config",
    "renpy",
    "store",
    "persistent",
    "style",
    "gui",
    "preferences",
    "build",
    "director",
    "achievement",
    "updater",
    "layeredimage",
    "im",
    "ui",
    "layout",
    "theme",
    "menu",
    "character",
    "Character",
    "voice",
    "sound",
    "music",
    "audio",
    "left",
    "right",
    "center",
    "truecenter",
    "top",
    "topleft",
    "topright",
    "fade",
    "dissolve",
    "pixellate",
    "move",
    "ease",
    "vpunch",
    "hpunch",
    "black",
    "text",
    "image",
    "label",
    "jump",
    "call",
    "show",
    "hide",
    "scene",
    "play",
    "stop",
    "queue",
    "pause",
    "define",
    "default",
    "init",
    "python",
    "screen",
    "transform",
    "window",
    "camera",
    "translate",
    "rpy",
    "testcase",
    "int",
    "str",
    "bool",
    "float",
    "len",
    "abs",
    "min",
    "max",
    "sum",
    "round",
    "range",
    "list",
    "dict",
    "set",
    "object",
    "print",
    "format",
    "open",
    "input",
    "id",
];

/// Python の名前に使える文字か。ASCII の英数字と `_` のほかは、Unicode の識別子の続きの文字（XID_Continue。
/// Python の規則）で、Ren'Py の名前の正規表現が受け付ける基本面の文字（U+00A0〜U+FFFD）だけ。
fn name_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || c == '_'
        || (('\u{a0}'..='\u{fffd}').contains(&c) && unicode_ident::is_xid_continue(c))
}

/// Python の名前として使える形にする（使えない文字は `_`。日本語の文字はそのまま）。先頭が名前の初めに
/// 使えない文字（数字、`_`（Ren'Py は `__` で始まる名前をファイルごとに変える）など）か、予約された名前なら、
/// `prefix` を付ける。
fn identifier(text: &str, prefix: &str) -> String {
    let out: String = text
        .chars()
        .map(|c| if name_char(c) { c } else { '_' })
        .collect();
    if out.is_empty()
        || out.starts_with(|c: char| !c.is_ascii_alphabetic() && !unicode_ident::is_xid_start(c))
        || out.starts_with(|c: char| c.is_ascii_digit() || c == '_')
        || RESERVED.contains(&out.as_str())
    {
        format!("{prefix}{out}")
    } else {
        out
    }
}

/// 重ならない名前にする（重なれば `_2`、`_3` …を付ける）。
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

/// 名前から決まる数（仮素材の画像の名前と色）。FNV-1a。
fn hash_of(name: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for b in name.bytes() {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// 仮素材の色（暗めの色）。
fn color_of(name: &str) -> String {
    let hash = hash_of(name);
    let r = 0x30 + (hash & 0x3f);
    let g = 0x30 + ((hash >> 8) & 0x3f);
    let b = 0x40 + ((hash >> 16) & 0x3f);
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Ren'Py の画像の名前の1語（英小文字と数字と `_`。数字で始まれば `i` を付ける）。英数字を含まなければ `None`。
fn image_word(text: &str) -> Option<String> {
    let word: String = text
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_owned();
    if !word.chars().any(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(if word.starts_with(|c: char| c.is_ascii_digit()) {
        format!("i{word}")
    } else {
        word
    })
}

/// 名前から決まる印（`ph_` と16進の6桁）。
fn mark_of(prefix: &str, name: &str) -> String {
    format!("{prefix}{:06x}", hash_of(name) & 0x00ff_ffff)
}

/// 素材の画像の名前に入れる語（カタログのファイル名。ファイルが無ければ、意味名から決まる印）。
fn stem_word(file: Option<&str>, name: &str) -> String {
    file.and_then(|f| Path::new(f).file_stem())
        .and_then(|s| image_word(&s.to_string_lossy()))
        .unwrap_or_else(|| mark_of("ph_", name))
}

/// プロファイルの画像の名前のひな形を埋めて、Ren'Py の画像の名前にする。
fn image_name(template: &str, pairs: &[(&str, &str)]) -> String {
    let mut text = template.to_owned();
    for (key, value) in pairs {
        text = text.replace(key, value);
    }
    let words: Vec<String> = text.split_whitespace().filter_map(image_word).collect();
    if words.is_empty() {
        "scn_image".to_owned()
    } else {
        words.join(" ")
    }
}

/// Ren'Py の文字列の中に、書かれた文字をそのまま見せるように書く。`\`・`"` と改行に加えて、Ren'Py が
/// 特別に読む `[`（変数の埋め込み）、`{`（テキストタグ）、`%`（古い埋め込み）、`【`（ルビ）を2つ重ねる。
fn literal(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '[' | '{' | '%' | '【' => {
                out.push(c);
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Python の文字列の式（JSON の文字列の書き方は Python でもそのまま読める）。
fn python_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned())
}

/// 画面に見せる名前（キャラクターの名前、仮素材の札）の Python の文字列の式。Ren'Py が特別に読む `[`・`{`・
/// `【` を重ねる。
fn shown_string(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if matches!(c, '[' | '{' | '【') {
            out.push(c);
        }
        out.push(c);
    }
    python_string(&out)
}

/// 秒を書く（`1`、`0.5`）。
fn seconds(value: f64) -> String {
    format!("{value}")
}

/// 文中の記法で、ひとまとまりの文字の後に来る区切り。
#[derive(Debug, Clone, PartialEq)]
enum After {
    /// 台詞の終わり。
    End,
    /// `{p}`：ページを送る（次の say 文にする）。
    Page,
    /// `{se}`：効果音を鳴らして続ける（`{nw}` と `extend`）。
    Se(String),
}

/// ひとまとまりの文字。
struct Segment {
    text: String,
    /// 見える文字があるか（待ちと速さのタグだけなら無い）。
    visible: bool,
    after: After,
}

/// 変数の名前の対応。
struct Names {
    variables: BTreeMap<String, String>,
    /// 真偽と宣言された変数。
    bools: BTreeSet<String>,
}

impl Names {
    fn var(&self, key: &str) -> String {
        self.variables
            .get(key)
            .cloned()
            .unwrap_or_else(|| identifier(&key.replace('.', "_"), "v_"))
    }
}

/// 式を Python にする。
fn expr(e: &IrExpr, names: &Names) -> String {
    match e {
        IrExpr::Int { value } => value.to_string(),
        IrExpr::String { value } => python_string(value),
        IrExpr::Bool { value } => if *value { "True" } else { "False" }.to_owned(),
        IrExpr::Var { name } => names.var(name),
        IrExpr::Unary { op, operand } => match *op {
            "not" => format!("(not {})", expr(operand, names)),
            _ => format!("(-{})", expr(operand, names)),
        },
        IrExpr::Binary { op, lhs, rhs } => {
            let (l, r) = (expr(lhs, names), expr(rhs, names));
            match *op {
                // 整数の割り算は0の向きへ切り捨てる（仕様書14節）。Python の // は負の向きへ切り捨てるので、
                // 定義のファイルの関数を使う。
                "/" => format!("_scn_div({l}, {r})"),
                _ => format!("({l} {op} {r})"),
            }
        }
    }
}

/// 真偽になる式か（埋め込みで `true`・`false` と見せるため）。
fn is_bool(e: Option<&IrExpr>, names: &Names) -> bool {
    match e {
        Some(IrExpr::Bool { .. }) => true,
        Some(IrExpr::Var { name }) => names.bools.contains(name),
        Some(IrExpr::Unary { op, .. }) => *op == "not",
        Some(IrExpr::Binary { op, .. }) => {
            matches!(*op, "and" | "or" | "==" | "!=" | "<" | "<=" | ">" | ">=")
        }
        _ => false,
    }
}

/// 画像の名前（意味名から）。
#[derive(Default)]
struct Images {
    backgrounds: BTreeMap<String, String>,
    sprites: BTreeMap<(String, String), String>,
    /// キャラクターID（解決できなければ書かれた名前）から、画像のタグ（`hide` に使う）。
    tags: BTreeMap<String, String>,
}

/// 音（意味名から、書き出し先の `game/` から数えたパス。BGM はくり返すかも）。
#[derive(Default)]
struct Sounds {
    bgm: BTreeMap<String, (String, bool)>,
    se: BTreeMap<String, String>,
}

/// 台詞の Character を分ける鍵（話者・表示名・表示スタイル）。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SpeakerKey {
    speaker: String,
    display: Option<String>,
    style: String,
}

/// 書き出しの途中の状態。
struct Exporter<'a> {
    data: &'a PreviewData<'a>,
    profile: &'a RenpyProfile,
    root: &'a Path,
    names: Names,
    images: Images,
    sounds: Sounds,
    /// 使っている Python の名前（キャラクターと変数）。
    taken: BTreeSet<String>,
    /// キャラクターIDから Character の名前。
    characters: BTreeMap<String, String>,
    /// 表示名や表示スタイルの違う台詞の Character。
    speakers: BTreeMap<SpeakerKey, String>,
    /// 写すファイル（書き出し先の中のパスから、写す元）。
    copies: BTreeMap<String, String>,
    scenes: BTreeSet<String>,
    warnings: Vec<String>,
    /// 1つのシーンの中の、変数の埋め込みの一時変数の数。
    temp: usize,
    out: String,
    /// `out` に書いた文の数（コメントと空行を除く。空のブロックに `pass` を置くため）。
    statements: usize,
}

impl Exporter<'_> {
    fn warn(&mut self, message: String) {
        if !self.warnings.contains(&message) {
            self.warnings.push(message);
        }
    }

    fn line(&mut self, depth: usize, text: &str) {
        if text.is_empty() {
            self.out.push('\n');
            return;
        }
        for _ in 0..depth {
            self.out.push_str("    ");
        }
        self.out.push_str(text);
        self.out.push('\n');
        if !text.starts_with('#') {
            self.statements += 1;
        }
    }

    fn comment(&mut self, depth: usize, text: &str) {
        self.line(depth, &format!("# {text}"));
    }

    fn transition(&mut self, name: Option<&str>) -> String {
        let Some(name) = name.filter(|n| !n.is_empty()) else {
            return String::new();
        };
        match self.profile.transitions.get(name).cloned() {
            Some(t) if t == "None" => String::new(),
            Some(t) => format!(" with {t}"),
            None => {
                self.warn(format!(
                    "トランジション「{name}」の対応がプロファイル（transitions）に無いので、dissolve にしました"
                ));
                " with dissolve".to_owned()
            }
        }
    }

    /// 台詞の Character の名前。表示名や表示スタイルが違えば、`kind` で元を引き継ぐ Character を作る。
    fn speaker(&mut self, speaker: &str, display: Option<&str>, style: &str) -> String {
        let base = match self.characters.get(speaker) {
            Some(base) => base.clone(),
            None => {
                let name = unique(&mut self.taken, &identifier(speaker, "c_"));
                self.characters.insert(speaker.to_owned(), name.clone());
                name
            }
        };
        if display.is_none() && style == "normal" {
            return base;
        }
        let key = SpeakerKey {
            speaker: speaker.to_owned(),
            display: display.map(str::to_owned),
            style: style.to_owned(),
        };
        if let Some(name) = self.speakers.get(&key) {
            return name.clone();
        }
        let wanted = match display {
            None => format!("{base}_{}", identifier(style, "s_")),
            Some(_) => format!("{base}_name"),
        };
        let name = unique(&mut self.taken, &wanted);
        self.speakers.insert(key, name.clone());
        name
    }

    /// 変数の埋め込みを、Ren'Py の `[名前!q]` にする（`!q`：値の中の `{` をテキストタグとして読まない）。
    /// 変数でない式と、真偽の値（プレビューと同じく `true`・`false` と見せる）は、一時変数に入れてから埋め込む。
    fn interpolation(&mut self, depth: usize, e: Option<&IrExpr>, text: &str) -> String {
        match e {
            Some(IrExpr::Var { name }) if !is_bool(e, &self.names) => {
                format!("[{}!q]", self.names.var(name))
            }
            Some(e) => {
                self.temp += 1;
                let temp = format!("_scn_value_{}", self.temp);
                let value = expr(e, &self.names);
                let value = if is_bool(Some(e), &self.names) {
                    format!("(\"true\" if {value} else \"false\")")
                } else {
                    value
                };
                self.line(depth, &format!("$ {temp} = {value}"));
                format!("[{temp}!q]")
            }
            // 構文の崩れた式（E601）は、プレビューと同じく `{=式}` のまま見せる。
            None => literal(&format!("{{={text}}}")),
        }
    }

    /// 文中の記法を、Ren'Py のテキストにして、`{p}` と `{se}` で区切る。`plain`（`menu` の文字）では、
    /// 待ちと速さのタグを書かず、区切らない。変数の埋め込みの一時変数は、`depth` の深さで先に代入する。
    fn segments(&mut self, depth: usize, text: &[IrInline], plain: bool) -> Vec<Segment> {
        let mut segments = Vec::new();
        let mut current = String::new();
        let mut visible = false;
        let mut speed: Option<&'static str> = None;
        for inline in text {
            match inline {
                IrInline::Text { text } => {
                    current.push_str(&literal(text));
                    visible |= !text.is_empty();
                }
                IrInline::Emphasis { text } => {
                    // 傍点：1文字ずつ、ルビの位置に「﹅」を付ける。
                    for c in text.chars() {
                        let _ = write!(
                            current,
                            "{{rb}}{}{{/rb}}{{rt}}﹅{{/rt}}",
                            literal(&c.to_string())
                        );
                    }
                    visible |= !text.is_empty();
                }
                IrInline::Ruby { base, reading } => {
                    let _ = write!(
                        current,
                        "{{rb}}{}{{/rb}}{{rt}}{}{{/rt}}",
                        literal(base),
                        literal(reading)
                    );
                    visible = true;
                }
                IrInline::LineBreak => {
                    current.push_str("\\n");
                    visible = true;
                }
                IrInline::Interpolation { expr: e, expr_text } => {
                    let piece = self.interpolation(depth, e.as_ref(), expr_text);
                    current.push_str(&piece);
                    visible = true;
                }
                IrInline::Wait { seconds: s } => {
                    if !plain {
                        let _ = write!(current, "{{w={}}}", seconds(*s));
                    }
                }
                IrInline::Speed { speed: s } => {
                    if plain {
                        continue;
                    }
                    if speed.is_some() {
                        current.push_str("{/cps}");
                    }
                    speed = match *s {
                        "slow" => Some("{cps=*0.5}"),
                        "fast" => Some("{cps=*2}"),
                        _ => None,
                    };
                    if let Some(tag) = speed {
                        current.push_str(tag);
                    }
                }
                IrInline::PageBreak | IrInline::Se { .. } => {
                    if plain {
                        if let IrInline::Se { name } = inline {
                            self.warn(format!(
                                "選択肢と問いかけ文の中の効果音（{{se {name}}}）は、書き出していません"
                            ));
                        }
                        continue;
                    }
                    // タグは文字列ごとに閉じ、次の文字列で開き直す。
                    if speed.is_some() {
                        current.push_str("{/cps}");
                    }
                    let after = match inline {
                        IrInline::Se { name } => After::Se(name.clone()),
                        _ => After::Page,
                    };
                    segments.push(Segment {
                        text: std::mem::take(&mut current),
                        visible,
                        after,
                    });
                    visible = false;
                    if let Some(tag) = speed {
                        current.push_str(tag);
                    }
                }
            }
        }
        if speed.is_some() {
            current.push_str("{/cps}");
        }
        segments.push(Segment {
            text: current,
            visible,
            after: After::End,
        });
        // 空のページ（`{p}` が続く、`{p}` で始まる・終わる）は出さない。ただし `{se}` の直後の `{p}` は、
        // クリックを待ってページを送るので残す。
        let mut out: Vec<Segment> = Vec::new();
        for segment in segments {
            let after_se = out
                .last()
                .is_some_and(|last| matches!(last.after, After::Se(_)));
            if !segment.visible && segment.after == After::Page && !after_se {
                continue;
            }
            if !segment.visible
                && segment.after == After::End
                && let Some(last) = out.last_mut()
                && last.after == After::Page
            {
                last.after = After::End;
                continue;
            }
            out.push(segment);
        }
        out
    }

    /// 文中の記法を1つの文字列にする（`menu` の問いかけ文と選択肢）。
    fn plain_text(&mut self, depth: usize, text: &[IrInline]) -> String {
        self.segments(depth, text, true)
            .into_iter()
            .map(|s| s.text)
            .collect()
    }

    /// 台詞・地の文を書く。`{p}` は次の say 文にし、括弧（`brackets`）は最初のページの初めと最後のページの
    /// 終わりにだけ付ける（プレビューと同じ）。`{se}` は `{nw}` で止めずに効果音を鳴らし、`extend` で続ける。
    /// `extend` は元の say 文の引数を引き継ぐので、括弧を両方とも書く。ボイスのある台詞（`voiced`）は、
    /// 続きの文でボイスを止めないよう `voice sustain` を書く。
    fn say(
        &mut self,
        depth: usize,
        who: Option<&str>,
        brackets: Option<(&str, &str)>,
        voiced: bool,
        text: &[IrInline],
    ) {
        let segments = self.segments(depth, text, false);
        let mut page = 0;
        let mut extend = false;
        for (i, segment) in segments.iter().enumerate() {
            let nw = if matches!(segment.after, After::Se(_)) {
                "{nw}"
            } else {
                ""
            };
            let mut args = Vec::new();
            if let Some((open, close)) = brackets {
                let open = if page > 0 { "" } else { open };
                let close = if segment.after == After::End {
                    close
                } else {
                    ""
                };
                if extend || open.is_empty() {
                    args.push(format!("what_prefix={}", python_string(open)));
                }
                if extend || close.is_empty() {
                    args.push(format!("what_suffix={}", python_string(close)));
                }
            }
            if voiced && i > 0 {
                self.line(depth, "voice sustain");
            }
            let args = if args.is_empty() {
                String::new()
            } else {
                format!(" ({})", args.join(", "))
            };
            let chunk = &segment.text;
            let statement = match (extend, who) {
                (true, _) => format!("extend \"{chunk}{nw}\"{args}"),
                (false, Some(who)) => format!("{who} \"{chunk}{nw}\"{args}"),
                (false, None) => format!("\"{chunk}{nw}\"{args}"),
            };
            self.line(depth, &statement);
            match &segment.after {
                After::Se(name) => {
                    self.sound(depth, name, false);
                    extend = true;
                }
                After::Page => {
                    page += 1;
                    extend = false;
                }
                After::End => {}
            }
        }
    }

    fn sound(&mut self, depth: usize, name: &str, music: bool) {
        if music {
            match self.sounds.bgm.get(name).cloned() {
                Some((path, repeat)) => {
                    let mode = if repeat { "" } else { " noloop" };
                    self.line(depth, &format!("play music {}{mode}", python_string(&path)));
                }
                None => {
                    self.comment(
                        depth,
                        &format!("BGM {name}（ファイルが無いので鳴らさない）"),
                    );
                    self.line(depth, "stop music");
                }
            }
        } else {
            match self.sounds.se.get(name).cloned() {
                Some(path) => self.line(depth, &format!("play sound {}", python_string(&path))),
                None => self.comment(depth, &format!("SE {name}（ファイルが無いので鳴らさない）")),
            }
        }
    }

    /// ボイス：プロファイルの `voice` のファイルが `assets/` にあれば、写して鳴らす。鳴らしたかを返す。
    fn voice(&mut self, depth: usize, speaker: &str, line_id: Option<&str>) -> bool {
        let (Some(pattern), Some(id)) = (self.profile.voice.as_deref(), line_id) else {
            return false;
        };
        let relative = pattern
            .replace("{char_id}", speaker)
            .replace("{line_id}", id);
        if !relative_inside(&relative) {
            return false;
        }
        // 素材と同じく、`assets/` の中の実在するファイルだけを使う（D050）。
        let source = fs::canonicalize(self.root.join("assets").join(&relative)).ok();
        let assets = fs::canonicalize(self.root.join("assets")).ok();
        if let (Some(source), Some(assets)) = (source, assets)
            && source.starts_with(&assets)
            && source.is_file()
        {
            let target = format!("scn/{relative}");
            self.copies
                .insert(format!("game/{target}"), source.display().to_string());
            self.line(depth, &format!("voice {}", python_string(&target)));
            return true;
        }
        false
    }

    fn jump_target(&mut self, depth: usize, keyword: &str, target: &str) {
        if self.scenes.contains(target) {
            self.line(depth, &format!("{keyword} {}", label_name(target)));
        } else {
            self.warn(format!(
                "存在しないシーン {target} へのジャンプ・呼び出しがあります（E501）。そこで止まるようにしました"
            ));
            self.dead_end(depth, &format!("存在しないシーン {target}"));
        }
    }

    /// 行き止まり：知らせてから、タイトルへ戻る。
    fn dead_end(&mut self, depth: usize, what: &str) {
        let text = literal(&format!("（行き止まり：{what}）"));
        self.line(depth, &format!("\"{text}\""));
        self.line(depth, "$ renpy.full_restart()");
    }

    fn ops(&mut self, depth: usize, ops: &[Op]) {
        for op in ops {
            self.op(depth, op);
        }
    }

    /// ブロックの中身。文が1つも無ければ `pass`。
    fn block(&mut self, depth: usize, ops: &[Op], jump: Option<&str>) {
        let before = self.statements;
        self.ops(depth, ops);
        if let Some(target) = jump {
            self.jump_target(depth, "jump", target);
        }
        if self.statements == before {
            self.line(depth, "pass");
        }
    }

    fn choice(&mut self, depth: usize, prompt: Option<&[IrInline]>, options: &[IrOption]) {
        // 選択肢の無い `@選択`（E106）は、Ren'Py の `menu` にできない（空のブロックは読めない）。プレビューと
        // 同じく、選べずに止まる。
        if options.is_empty() {
            self.comment(depth, "選択肢の無い @選択（E106）");
            self.dead_end(depth, "選択肢がありません");
            return;
        }
        // 選択肢がすべて条件付きなら、どれも選べないとき、Ren'Py の `menu` は次の文へ進んでしまう。
        // プレビューと同じく、そこで止まる。
        if options.iter().all(|o| o.cond_text.is_some()) {
            let any = options
                .iter()
                .map(|o| {
                    o.cond
                        .as_ref()
                        .map_or_else(|| "False".to_owned(), |c| expr(c, &self.names))
                })
                .collect::<Vec<_>>()
                .join(" or ");
            self.line(depth, &format!("if not ({any}):"));
            self.dead_end(depth + 1, "選べる選択肢がありません");
        }
        // 文字の中の一時変数は `menu` の前で代入する（`menu` の中には `$` を書けない）。
        let caption = prompt.map(|p| self.plain_text(depth, p));
        let labels: Vec<String> = options
            .iter()
            .map(|o| self.plain_text(depth, &o.text))
            .collect();
        self.line(depth, "menu:");
        if let Some(caption) = caption {
            self.line(depth + 1, &format!("\"{caption}\""));
        }
        for (option, label) in options.iter().zip(labels) {
            let cond = match (&option.cond, &option.cond_text) {
                (Some(c), _) => format!(" if {}", expr(c, &self.names)),
                // 構文の崩れた条件（E601）は、プレビューと同じく偽にする。
                (None, Some(_)) => " if False".to_owned(),
                (None, None) => String::new(),
            };
            self.line(depth + 1, &format!("\"{label}\"{cond}:"));
            self.block(depth + 2, &option.body, option.jump.as_deref());
        }
    }

    fn op(&mut self, depth: usize, op: &Op) {
        match op {
            Op::Say {
                line_id,
                speaker,
                display_name,
                style,
                text,
                ..
            } => {
                let voiced = self.voice(depth, speaker, line_id.as_deref());
                let who = self.speaker(speaker, display_name.as_deref(), style);
                let data = self.data;
                let brackets = data
                    .brackets
                    .iter()
                    .find(|b| b.style == *style)
                    .map(|b| (b.open.as_str(), b.close.as_str()));
                self.say(depth, Some(&who), brackets, voiced, text);
            }
            Op::Narrate { text, .. } => self.say(depth, None, None, false, text),
            Op::Break { .. } => {}
            Op::Bg {
                name, transition, ..
            } => {
                let image = self
                    .images
                    .backgrounds
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| "black".to_owned());
                let with = self.transition(transition.as_deref());
                self.line(depth, &format!("scene {image}{with}"));
            }
            Op::Show {
                character,
                expression,
                position,
                transition,
                ..
            } => {
                let image = self
                    .images
                    .sprites
                    .get(&(character.clone(), expression.clone()))
                    .cloned()
                    .unwrap_or_else(|| "black".to_owned());
                let at = position
                    .and_then(|p| self.profile.positions.get(p))
                    .map(|p| format!(" at {p}"))
                    .unwrap_or_default();
                let with = self.transition(transition.as_deref());
                self.line(depth, &format!("show {image}{at}{with}"));
            }
            Op::Hide {
                character,
                transition,
                ..
            } => {
                let with = self.transition(transition.as_deref());
                match self.images.tags.get(character).cloned() {
                    Some(tag) => self.line(depth, &format!("hide {tag}{with}")),
                    // 本文で一度も表示しないキャラクターは、隠すものが無い。
                    None => self.comment(depth, &format!("退場 {character}（表示していない）")),
                }
            }
            Op::Bgm { name, .. } => self.sound(depth, name, true),
            Op::BgmStop { .. } => self.line(depth, "stop music"),
            Op::Se { name, .. } => self.sound(depth, name, false),
            Op::Fx {
                name, seconds: s, ..
            } => {
                match self.profile.fx.get(name).cloned() {
                    Some(statement) => self.line(depth, &statement),
                    None => {
                        self.warn(format!(
                            "効果「{name}」の対応がプロファイル（fx）に無いので、書き出していません"
                        ));
                        self.comment(depth, &format!("効果 {name}（対応なし）"));
                    }
                }
                if let Some(s) = s {
                    self.line(depth, &format!("pause {}", seconds(*s)));
                }
            }
            Op::Wait { seconds: s, .. } => self.line(depth, &format!("pause {}", seconds(*s))),
            Op::Set { var, op, value, .. } => {
                let line = format!(
                    "$ {} {op} {}",
                    self.names.var(var),
                    expr(value, &self.names)
                );
                self.line(depth, &line);
            }
            Op::Choice {
                prompt, options, ..
            } => self.choice(depth, prompt.as_deref(), options),
            Op::If {
                branches,
                else_body,
                ..
            } => {
                for (i, branch) in branches.iter().enumerate() {
                    // 構文の崩れた条件（E601）は、プレビューと同じく偽にする。
                    let cond = branch
                        .cond
                        .as_ref()
                        .map_or_else(|| "False".to_owned(), |c| expr(c, &self.names));
                    let keyword = if i == 0 { "if" } else { "elif" };
                    self.line(depth, &format!("{keyword} {cond}:"));
                    self.block(depth + 1, &branch.body, None);
                }
                if let Some(body) = else_body {
                    self.line(depth, "else:");
                    self.block(depth + 1, body, None);
                }
            }
            Op::Jump { target, .. } => self.jump_target(depth, "jump", target),
            Op::Call { target, .. } => self.jump_target(depth, "call", target),
            Op::Return { .. } => self.line(depth, "return"),
            Op::Ending { id, .. } => {
                self.comment(
                    depth,
                    &format!(
                        "エンディング {}",
                        id.as_deref().unwrap_or("（IDの形が不正）")
                    ),
                );
                let ending = self.profile.ending.clone();
                self.line(depth, &ending);
            }
            Op::Raw { engine, lines, .. } => {
                if engine.eq_ignore_ascii_case("renpy") {
                    for line in lines {
                        self.line(depth, line);
                    }
                } else {
                    self.warn(format!(
                        "@raw {engine} は Ren'Py への書き出しでは使わないので、読み飛ばしました"
                    ));
                    self.comment(depth, &format!("@raw {engine}（読み飛ばし）"));
                }
            }
            Op::UnknownDirective { name, .. } => {
                self.warn(format!(
                    "知らないディレクティブ @{name}（W401）は、書き出していません"
                ));
                self.comment(depth, &format!("@{name}（知らないディレクティブ）"));
            }
        }
    }
}

/// Ren'Py が特別に扱うラベルの名前（シーンIDと重なれば `s_` を付ける）。
const SPECIAL_LABELS: [&str; 9] = [
    "start",
    "main_menu",
    "main_menu_screen",
    "before_main_menu",
    "splashscreen",
    "after_load",
    "after_warp",
    "quit",
    "hide_windows",
];

/// シーンIDから、ラベルの名前（先頭が数字か `_` のときと、予約された名前なら `s_` を付ける）。
fn label_name(id: &str) -> String {
    if SPECIAL_LABELS.contains(&id) {
        format!("s_{id}")
    } else {
        identifier(id, "s_")
    }
}

/// 作品フォルダの `assets/` の中のファイル（絶対パス）を、`assets/` から数えたパス（`/` 区切り）にする。
fn assets_relative(file: &str, root: &Path) -> Option<String> {
    let assets = fs::canonicalize(root.join("assets")).ok()?;
    let relative = Path::new(file).strip_prefix(&assets).ok()?;
    let parts: Vec<String> = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// 本文で使われている素材の意味名と、変数。
#[derive(Default)]
struct Used {
    backgrounds: BTreeSet<String>,
    sprites: BTreeSet<(String, String)>,
    bgm: BTreeSet<String>,
    se: BTreeSet<String>,
    variables: BTreeSet<String>,
}

impl Used {
    fn expr(&mut self, e: &IrExpr) {
        match e {
            IrExpr::Var { name } => {
                self.variables.insert(name.clone());
            }
            IrExpr::Unary { operand, .. } => self.expr(operand),
            IrExpr::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            IrExpr::Int { .. } | IrExpr::String { .. } | IrExpr::Bool { .. } => {}
        }
    }

    fn text(&mut self, text: &[IrInline]) {
        for inline in text {
            match inline {
                IrInline::Se { name } => {
                    self.se.insert(name.clone());
                }
                IrInline::Interpolation { expr: Some(e), .. } => self.expr(e),
                _ => {}
            }
        }
    }

    fn ops(&mut self, ops: &[Op]) {
        for op in ops {
            match op {
                Op::Bg { name, .. } => {
                    self.backgrounds.insert(name.clone());
                }
                Op::Show {
                    character,
                    expression,
                    ..
                } => {
                    self.sprites.insert((character.clone(), expression.clone()));
                }
                Op::Bgm { name, .. } => {
                    self.bgm.insert(name.clone());
                }
                Op::Se { name, .. } => {
                    self.se.insert(name.clone());
                }
                Op::Say { text, .. } | Op::Narrate { text, .. } => self.text(text),
                Op::Set { var, value, .. } => {
                    self.variables.insert(var.clone());
                    self.expr(value);
                }
                Op::Choice {
                    prompt, options, ..
                } => {
                    if let Some(prompt) = prompt {
                        self.text(prompt);
                    }
                    for option in options {
                        self.text(&option.text);
                        if let Some(cond) = &option.cond {
                            self.expr(cond);
                        }
                        self.ops(&option.body);
                    }
                }
                Op::If {
                    branches,
                    else_body,
                    ..
                } => {
                    for branch in branches {
                        if let Some(cond) = &branch.cond {
                            self.expr(cond);
                        }
                        self.ops(&branch.body);
                    }
                    if let Some(body) = else_body {
                        self.ops(body);
                    }
                }
                _ => {}
            }
        }
    }
}

/// IRとプロジェクトから、Ren'Py の書き出しを作る（ファイルは書かない）。
pub fn export_renpy(
    project: &Project,
    root: &Path,
    ir: &Ir,
    profile: &RenpyProfile,
) -> RenpyExport {
    let data = preview_data(project, root, ir);
    let mut used = Used::default();
    for scene in &ir.scenes {
        used.ops(&scene.ops);
    }

    // 名前：プロファイルで決めた名前を先に取っておき、キャラクター、変数の順に決める（重なれば後のものに
    // `_2` を付ける）。
    let mut taken: BTreeSet<String> = RESERVED
        .iter()
        .map(|s| (*s).to_owned())
        .chain(profile.characters.values().cloned())
        .chain(profile.variables.values().cloned())
        .collect();
    let mut characters = BTreeMap::new();
    for c in &project.characters {
        let name = match profile.characters.get(&c.id) {
            Some(name) => name.clone(),
            None => unique(&mut taken, &identifier(&c.id, "c_")),
        };
        characters.insert(c.id.clone(), name);
    }
    let declared: BTreeSet<&str> = project.variables.iter().map(|v| v.key.as_str()).collect();
    let keys = project
        .variables
        .iter()
        .map(|v| (v.key.clone(), v.scope.as_deref() == Some("persistent")))
        .chain(
            used.variables
                .iter()
                .filter(|k| !declared.contains(k.as_str()))
                .map(|k| (k.clone(), false)),
        );
    let mut variables = BTreeMap::new();
    for (key, persistent) in keys {
        let name = match profile.variables.get(&key) {
            Some(name) => name.clone(),
            None => unique(&mut taken, &identifier(&key.replace('.', "_"), "v_")),
        };
        let name = if persistent && !name.contains('.') {
            format!("persistent.{name}")
        } else {
            name
        };
        variables.insert(key, name);
    }

    let mut exporter = Exporter {
        data: &data,
        profile,
        root,
        names: Names {
            variables,
            bools: project
                .variables
                .iter()
                .filter(|v| v.ty == VarType::Bool)
                .map(|v| v.key.clone())
                .collect(),
        },
        images: Images::default(),
        sounds: Sounds::default(),
        taken,
        characters,
        speakers: BTreeMap::new(),
        copies: BTreeMap::new(),
        scenes: ir.scenes.iter().map(|s| s.id.clone()).collect(),
        warnings: Vec::new(),
        temp: 0,
        out: String::new(),
        statements: 0,
    };
    let image_defs = define_assets(&mut exporter, &used);

    // シーン（章のファイルごと。IRの順＝ファイルのパスの順）。
    let mut chapters: Vec<(String, String)> = Vec::new();
    let mut labels: BTreeSet<String> = BTreeSet::new();
    let mut dead_ends = 0;
    for scene in &ir.scenes {
        let label = label_name(&scene.id);
        if !labels.insert(label.clone()) {
            exporter.warn(format!(
                "シーン {} のラベルの名前が、ほかのシーンと重なるので、書き出していません",
                scene.id
            ));
            continue;
        }
        exporter.out.clear();
        exporter.temp = 0;
        let title = if scene.title.is_empty() {
            scene.id.clone()
        } else {
            format!("{} {}", scene.id, scene.title)
        };
        exporter.comment(0, &title);
        if scene.meta.status.as_deref() == Some("ai-draft") {
            exporter.comment(0, "AI下書き（status: ai-draft）");
        }
        exporter.line(0, &format!("label {label}:"));
        let before = exporter.statements;
        exporter.ops(1, &scene.ops);
        if falls_through(&scene.ops) {
            dead_ends += 1;
            let what = if scene.is_stub {
                format!("{title}（スタブ）")
            } else {
                title.clone()
            };
            exporter.dead_end(1, &what);
        }
        if exporter.statements == before {
            exporter.line(1, "pass");
        }
        exporter.out.push('\n');
        let stem = Path::new(&scene.file)
            .file_stem()
            .map_or_else(|| "story".to_owned(), |s| s.to_string_lossy().into_owned());
        let file = format!("story_{}", identifier(&stem, "f_"));
        let body = std::mem::take(&mut exporter.out);
        match chapters.iter_mut().find(|(name, _)| *name == file) {
            Some((_, text)) => text.push_str(&body),
            None => chapters.push((file, body)),
        }
    }
    if dead_ends > 0 {
        exporter.warn(format!(
            "出口の無い経路が残るシーンが {dead_ends} 個あります（W503）。終わりで知らせてタイトルへ戻るようにしました"
        ));
    }

    let mut files: Vec<RenpyFile> = chapters
        .into_iter()
        .map(|(name, body)| RenpyFile::Text {
            path: format!("{GENERATED_DIR}/{name}.rpy"),
            text: format!("{HEADER}\n{body}"),
        })
        .collect();
    let defs = definitions(&mut exporter, project, &image_defs);
    files.push(RenpyFile::Text {
        path: format!("{GENERATED_DIR}/{DEFS_FILE}"),
        text: defs,
    });
    if profile.start_label {
        let start = ir
            .start_scene
            .as_deref()
            .filter(|s| exporter.scenes.contains(*s))
            .or_else(|| ir.scenes.first().map(|s| s.id.as_str()));
        let body = match start {
            Some(start) => format!("label start:\n    jump {}\n", label_name(start)),
            None => "label start:\n    return\n".to_owned(),
        };
        files.push(RenpyFile::Text {
            path: format!("{GENERATED_DIR}/scn_start.rpy"),
            text: format!("{HEADER}\n{body}"),
        });
    }
    for (path, from) in std::mem::take(&mut exporter.copies) {
        files.push(RenpyFile::Copy { path, from });
    }
    files.sort_by(|a, b| a.path().cmp(b.path()));
    RenpyExport {
        files,
        warnings: exporter.warnings,
    }
}

/// 使われている素材の画像を定義し、音の対応を決め、写すファイルを集める。画像の定義の文を返す。
fn define_assets(exporter: &mut Exporter<'_>, used: &Used) -> String {
    let data = exporter.data;
    let root = exporter.root;
    let profile = exporter.profile;
    let (sw, sh) = (data.screen.width, data.screen.height);
    let mut taken: BTreeSet<String> = BTreeSet::new();
    let mut defs = String::new();
    let mut placeholders = 0;
    let copy = |exporter: &mut Exporter<'_>, file: &str| -> Option<String> {
        let relative = assets_relative(file, root)?;
        let target = format!("scn/{relative}");
        exporter
            .copies
            .insert(format!("game/{target}"), file.to_owned());
        Some(target)
    };
    for name in &used.backgrounds {
        let file = data.assets.backgrounds.get(name).cloned().flatten();
        let stem = stem_word(file.as_deref(), name);
        let image = unique(
            &mut taken,
            &image_name(&profile.background_image, &[("{file_stem}", &stem)]),
        );
        match file.as_deref().and_then(|f| copy(exporter, f)) {
            Some(path) => {
                let _ = writeln!(defs, "image {image} = {}", python_string(&path));
            }
            None => {
                placeholders += 1;
                let _ = writeln!(
                    defs,
                    "image {image} = Composite(({sw}, {sh}), (0, 0), Solid(\"{}\"), (40, 40), Text({}, size=36))",
                    color_of(name),
                    shown_string(&format!("背景：{name}（仮素材）"))
                );
            }
        }
        exporter.images.backgrounds.insert(name.clone(), image);
    }
    for (character, expression) in &used.sprites {
        let file = data
            .assets
            .expressions
            .get(character)
            .and_then(|e| e.get(expression))
            .cloned()
            .flatten();
        let id_word = image_word(character).unwrap_or_else(|| mark_of("c_", character));
        let stem = stem_word(file.as_deref(), &format!("{character}/{expression}"));
        let image = unique(
            &mut taken,
            &image_name(
                &profile.sprite_image,
                &[("{char_id}", &id_word), ("{expr_stem}", &stem)],
            ),
        );
        let tag = image.split(' ').next().unwrap_or_default().to_owned();
        exporter.images.tags.entry(character.clone()).or_insert(tag);
        match file.as_deref().and_then(|f| copy(exporter, f)) {
            Some(path) => {
                let _ = writeln!(defs, "image {image} = {}", python_string(&path));
            }
            None => {
                placeholders += 1;
                let shown = data
                    .characters
                    .iter()
                    .find(|c| c.id == *character)
                    .map_or(character.as_str(), |c| c.name.as_str());
                let _ = writeln!(
                    defs,
                    "image {image} = Composite(({w}, {h}), (0, 0), Solid(\"{}\"), (12, 12), Text({}, size=28))",
                    color_of(character),
                    shown_string(&format!("{shown}：{expression}（仮素材）")),
                    w = u64::from(sw) * 3 / 10,
                    h = u64::from(sh) * 17 / 20,
                );
            }
        }
        exporter
            .images
            .sprites
            .insert((character.clone(), expression.clone()), image);
    }
    if placeholders > 0 {
        exporter.warn(format!(
            "ファイルの無い画像が {placeholders} 個あります（名前入りの色の矩形にしました）"
        ));
    }
    let mut silent = 0;
    for name in &used.bgm {
        let sound = data.assets.bgm.get(name);
        match sound.and_then(|s| s.file.as_deref().and_then(|f| copy(exporter, f))) {
            Some(path) => {
                let repeat = sound.is_none_or(|s| s.repeat);
                exporter.sounds.bgm.insert(name.clone(), (path, repeat));
            }
            None => silent += 1,
        }
    }
    for name in &used.se {
        let sound = data.assets.se.get(name);
        match sound.and_then(|s| s.file.as_deref().and_then(|f| copy(exporter, f))) {
            Some(path) => {
                exporter.sounds.se.insert(name.clone(), path);
            }
            None => silent += 1,
        }
    }
    if silent > 0 {
        exporter.warn(format!(
            "ファイルの無い音が {silent} 個あります（鳴らさずに、コメントを書きました）"
        ));
    }
    defs
}

/// フォントのファイルか（拡張子で見る）。
fn is_font(path: &str) -> bool {
    Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .is_some_and(|e| matches!(e.as_str(), "ttf" | "otf" | "ttc"))
}

/// 定義のファイル（画面の大きさ、フォント、キャラクター、変数、画像）。
fn definitions(exporter: &mut Exporter<'_>, project: &Project, image_defs: &str) -> String {
    let data = exporter.data;
    let mut defs = String::from(HEADER);
    defs.push('\n');
    // Ren'Py のランチャーで作ったプロジェクトは、gui.rpy の gui.init で画面の大きさと既定の画面を決める。
    // gui.rpy の無いプロジェクト（書き出しだけで動かすとき）では、ここで画面の大きさを決め、確認のダイアログ
    // などの既定の画面を用意する（gui.init はスタイルを消すので呼ばず、Ren'Py の既定の見た目を使う）。
    // gui.rpy は gui.text_font を定義するので、それで見分ける。
    defs.push_str("init -1 python:\n");
    // 整数の割り算は0の向きへ切り捨てる（仕様書14節）。0で割ると Ren'Py の例外になる。
    defs.push_str(
        "    def _scn_div(a, b):\n        q = abs(a) // abs(b)\n        return q if (a < 0) == (b < 0) else -q\n\n",
    );
    defs.push_str("    if not hasattr(gui, \"text_font\"):\n");
    let _ = writeln!(
        defs,
        "        config.screen_width = {}\n        config.screen_height = {}\n        layout.defaults()",
        data.screen.width, data.screen.height
    );
    // 選択肢の窓は、問いかけ文（台詞の窓）と重ならないよう、画面の中ほどに出す。
    defs.push_str("        style.menu_window.yalign = 0.45\n");
    match exporter.profile.font.clone() {
        Some(font) => {
            let absolute = Path::new(&font).is_absolute();
            let source = if absolute {
                PathBuf::from(&font)
            } else {
                exporter.root.join(&font)
            };
            let name = Path::new(&font)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let found = (is_font(&font) && (absolute || relative_inside(&font)))
                .then(|| fs::canonicalize(&source).ok())
                .flatten()
                .filter(|source| source.is_file());
            let path = if let Some(source) = found {
                let target = format!("scn/fonts/{name}");
                exporter
                    .copies
                    .insert(format!("game/{target}"), source.display().to_string());
                target
            } else {
                exporter.warn(format!(
                    "フォント {font} が見つからないので、写していません。書き出し先の game/ に {name} を置くと使います"
                ));
                name
            };
            // フォントが無くても止まらないよう、読めるときだけ使う（無ければ Ren'Py の既定のフォント）。
            let path = python_string(&path);
            let _ = write!(
                defs,
                "\ninit python:\n    if renpy.loadable({path}):\n        style.default.font = {path}\n"
            );
        }
        None => exporter.warn(
            "フォントがプロファイル（font）にありません。Ren'Py の既定のフォントには日本語が無いので、日本語のフォントを指定してください（ランチャーで日本語のフォントを設定したプロジェクトに書き出すなら不要）".to_owned(),
        ),
    }

    // キャラクター。括弧は what_prefix・what_suffix で付ける（表示スタイルの括弧のうち、最初に書かれたもの）。
    let affixes = |style: &str| {
        data.brackets
            .iter()
            .find(|b| b.style == style)
            .map_or_else(String::new, |b| {
                format!(
                    ", what_prefix={}, what_suffix={}",
                    python_string(&b.open),
                    python_string(&b.close)
                )
            })
    };
    let name_of = |id: &str| {
        project
            .characters
            .iter()
            .find(|c| c.id == id)
            .map_or_else(|| id.to_owned(), |c| c.name.clone())
    };
    defs.push('\n');
    for (id, var) in &exporter.characters {
        let _ = writeln!(
            defs,
            "define {var} = Character({}{})",
            shown_string(&name_of(id)),
            affixes("normal")
        );
    }
    for (key, var) in &exporter.speakers {
        let base = exporter
            .characters
            .get(&key.speaker)
            .cloned()
            .unwrap_or_default();
        let shown = match key.display.as_deref() {
            Some("") => "None".to_owned(),
            Some(shown) => shown_string(shown),
            None => shown_string(&name_of(&key.speaker)),
        };
        let _ = writeln!(
            defs,
            "define {var} = Character({shown}, kind={base}{})",
            affixes(&key.style)
        );
    }

    // 変数（宣言の初期値。宣言の無い変数は 0）。
    if !exporter.names.variables.is_empty() {
        defs.push('\n');
    }
    let mut seen = BTreeSet::new();
    for v in &data.variables {
        seen.insert(v.key.as_str());
        let value = match &v.initial {
            serde_json::Value::Bool(b) => if *b { "True" } else { "False" }.to_owned(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => python_string(s),
            _ => "None".to_owned(),
        };
        let _ = writeln!(defs, "default {} = {value}", exporter.names.var(&v.key));
    }
    let undeclared: Vec<String> = exporter
        .names
        .variables
        .keys()
        .filter(|k| !seen.contains(k.as_str()))
        .cloned()
        .collect();
    for key in &undeclared {
        let _ = writeln!(
            defs,
            "default {} = 0  # 宣言の無い変数（W602）",
            exporter.names.var(key)
        );
    }
    if !undeclared.is_empty() {
        exporter.warn(format!(
            "宣言の無い変数が {} 個あります（W602）。初期値を 0 にしました",
            undeclared.len()
        ));
    }
    if !image_defs.is_empty() {
        defs.push('\n');
        defs.push_str(image_defs);
    }
    defs
}

/// 書き出す。書き出し先（`out`、無ければプロファイルの `output`）の `game/scn/` を消してから作り直す。
/// `game/scn/` があって、書き出したものでなければ（定義のファイルの印が無ければ）消さずに誤りを返す。
/// 書き出し先を返す。
pub fn write_renpy(
    root: &Path,
    profile: &RenpyProfile,
    export: &RenpyExport,
    out: Option<&Path>,
) -> Result<PathBuf, ProjectError> {
    let base = out.map_or_else(|| root.join(&profile.output), Path::to_path_buf);
    let generated = base.join(GENERATED_DIR);
    let error = |path: &Path, source: std::io::Error| ProjectError::Write {
        path: path.to_owned(),
        source,
    };
    if generated.exists() {
        let ours = fs::read_to_string(generated.join(DEFS_FILE))
            .is_ok_and(|text| text.starts_with(HEADER));
        if !ours {
            return Err(error(
                &generated,
                std::io::Error::other(
                    "scn-studio が書き出したフォルダではないので、消さずに止めました",
                ),
            ));
        }
        // 印のファイルは最後に消す（途中で消せないファイルがあっても、次の書き出しで消せるように）。
        let entries = fs::read_dir(&generated).map_err(|e| error(&generated, e))?;
        for entry in entries {
            let path = entry.map_err(|e| error(&generated, e))?.path();
            if path.file_name() == Some(DEFS_FILE.as_ref()) {
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
            RenpyFile::Text { text, .. } => fs::write(&path, text).map_err(|e| error(&path, e))?,
            RenpyFile::Copy { from, .. } => {
                fs::copy(from, &path).map_err(|e| error(&path, e))?;
            }
        }
    }
    Ok(base)
}
