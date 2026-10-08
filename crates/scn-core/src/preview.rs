//! 簡易ADVプレビューのデータ（アプリ、T1-11、D050）：IR全体と、プレビューに要る作品の設定
//! （画面とメッセージウィンドウの大きさ、台詞の括弧と表示スタイル、キャラクターの名前、変数の型と初期値、
//! 素材のファイル）。ランタイムはフロント（TypeScript）にあり、これを読んで動く。
//!
//! 素材のファイルは、素材カタログ（`assets/catalog.yaml`）の `file`・`dir` を `assets/` から数えたパスで
//! 読む。`assets/` の外を指すパス（`..`、絶対パス）と、無いファイルは使わない（仮素材で見せる）。
//! カタログの誤りの診断は読み込み（E802）で出しているので、ここでは黙って読み飛ばす。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use serde_json::Value;
use yaml_serde::Value as Yaml;

use crate::ir::Ir;
use crate::project::{Project, VarType};

/// 画面の大きさ（`project.toml` の `[screen]`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ScreenSize {
    pub width: u32,
    pub height: u32,
}

/// メッセージウィンドウの大きさ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct WindowSize {
    pub lines: u32,
    pub chars_per_line: u32,
}

/// 台詞の括弧と表示スタイル。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BracketStyle {
    pub open: String,
    pub close: String,
    pub style: String,
}

/// キャラクター。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreviewCharacter {
    pub id: String,
    pub name: String,
}

/// 宣言された変数と、その初期値。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PreviewVariable {
    pub key: String,
    /// `bool`・`int`・`string`・`enum`。
    #[serde(rename = "type")]
    pub ty: &'static str,
    pub values: Vec<String>,
    /// 初期値：`default` を型どおりの値にしたもの。無いか型に合わなければ、型の既定（0・false・空の
    /// 文字列・列挙の最初の値）。
    pub initial: Value,
    /// `default` が書かれていたが、型に合わなかった。
    pub bad_default: bool,
    pub scope: Option<String>,
    pub range: Option<(i64, i64)>,
}

/// 素材のファイル（絶対パス）。登録されているがファイルが無い（仮素材）なら `None`。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PreviewAssets {
    pub backgrounds: BTreeMap<String, Option<String>>,
    /// キャラクターIDごとの立ち絵の差分。
    pub expressions: BTreeMap<String, BTreeMap<String, Option<String>>>,
    pub bgm: BTreeMap<String, PreviewSound>,
    pub se: BTreeMap<String, PreviewSound>,
}

/// 音の素材。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PreviewSound {
    pub file: Option<String>,
    /// くり返すか（BGM の既定は true、SE は false）。
    #[serde(rename = "loop")]
    pub repeat: bool,
}

/// プレビューのデータ。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PreviewData<'a> {
    pub ir: &'a Ir,
    pub screen: ScreenSize,
    /// メッセージウィンドウ（`[message_window]`）。無ければ 3行×24字で見せる。
    pub message_window: WindowSize,
    pub brackets: Vec<BracketStyle>,
    pub characters: Vec<PreviewCharacter>,
    pub variables: Vec<PreviewVariable>,
    pub assets: PreviewAssets,
}

/// 画面の大きさの既定（`[screen]` が無いとき）。
pub const DEFAULT_SCREEN: ScreenSize = ScreenSize {
    width: 1280,
    height: 720,
};

/// メッセージウィンドウの既定（`[message_window]` が無いとき）。
pub const DEFAULT_WINDOW: WindowSize = WindowSize {
    lines: 3,
    chars_per_line: 24,
};

/// 変数の初期値（`default` を型どおりに読む）。2つ目は、書かれていたが型に合わなかったか。
fn initial_value(ty: &VarType, default: Option<&str>) -> (Value, bool) {
    let fallback = match ty {
        VarType::Bool => Value::Bool(false),
        VarType::Int => Value::from(0),
        VarType::String => Value::String(String::new()),
        VarType::Enum(values) => Value::String(values.first().cloned().unwrap_or_default()),
    };
    let Some(text) = default else {
        return (fallback, false);
    };
    let parsed = match ty {
        VarType::Bool => match text {
            "true" => Some(Value::Bool(true)),
            "false" => Some(Value::Bool(false)),
            _ => None,
        },
        VarType::Int => text.trim().parse::<i64>().ok().map(Value::from),
        VarType::String => Some(Value::String(text.to_owned())),
        VarType::Enum(values) => values
            .iter()
            .any(|v| v == text)
            .then(|| Value::String(text.to_owned())),
    };
    match parsed {
        Some(value) => (value, false),
        None => (fallback, true),
    }
}

/// `assets/` から数えたパスを、作品フォルダの中の実在するファイルの絶対パスにする。外を指すパスと
/// 無いファイルは `None`。
fn asset_file(assets: &Path, relative: &str) -> Option<String> {
    let rel = Path::new(relative);
    if relative.is_empty()
        || rel
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return None;
    }
    let path = assets.join(rel);
    let canonical = fs::canonicalize(&path).ok()?;
    let base = fs::canonicalize(assets).ok()?;
    (canonical.starts_with(&base) && canonical.is_file()).then(|| canonical.display().to_string())
}

fn yaml_str<'v>(value: &'v Yaml, key: &str) -> Option<&'v str> {
    value.get(key).and_then(Yaml::as_str)
}

fn yaml_key(key: &Yaml) -> Option<String> {
    match key {
        Yaml::String(s) => Some(s.clone()),
        Yaml::Number(n) => Some(n.to_string()),
        Yaml::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// 素材カタログを、ファイルのパスつきで読む（読めなければ、登録されている意味名だけ）。
fn assets(project: &Project, root: &Path) -> PreviewAssets {
    let catalog = &project.catalog;
    let mut out = PreviewAssets {
        backgrounds: catalog
            .backgrounds
            .iter()
            .map(|n| (n.clone(), None))
            .collect(),
        expressions: catalog
            .characters
            .iter()
            .map(|(id, names)| {
                (
                    id.clone(),
                    names.iter().map(|n| (n.clone(), None)).collect(),
                )
            })
            .collect(),
        bgm: catalog
            .bgm
            .iter()
            .map(|n| {
                (
                    n.clone(),
                    PreviewSound {
                        file: None,
                        repeat: true,
                    },
                )
            })
            .collect(),
        se: catalog
            .se
            .iter()
            .map(|n| {
                (
                    n.clone(),
                    PreviewSound {
                        file: None,
                        repeat: false,
                    },
                )
            })
            .collect(),
    };
    let dir = root.join("assets");
    let Ok(text) = fs::read_to_string(dir.join("catalog.yaml")) else {
        return out;
    };
    let Ok(yaml) = yaml_serde::from_str::<Yaml>(&text) else {
        return out;
    };
    let section = |name: &str| match yaml.get(name) {
        Some(Yaml::Mapping(map)) => map.iter().collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    for (key, entry) in section("backgrounds") {
        if let (Some(name), Some(file)) = (yaml_key(key), yaml_str(entry, "file"))
            && let Some(slot) = out.backgrounds.get_mut(&name)
        {
            *slot = asset_file(&dir, file);
        }
    }
    for (kind, target) in [("bgm", &mut out.bgm), ("se", &mut out.se)] {
        for (key, entry) in section(kind) {
            let Some(sound) = yaml_key(key).and_then(|name| target.get_mut(&name)) else {
                continue;
            };
            if let Some(file) = yaml_str(entry, "file") {
                sound.file = asset_file(&dir, file);
            }
            if let Some(repeat) = entry.get("loop").and_then(Yaml::as_bool) {
                sound.repeat = repeat;
            }
        }
    }
    for (key, entry) in section("characters") {
        let Some(id) = yaml_key(key) else { continue };
        let Some(slots) = out.expressions.get_mut(&id) else {
            continue;
        };
        let folder = yaml_str(entry, "dir").unwrap_or_default();
        let Some(Yaml::Mapping(map)) = entry.get("expressions") else {
            continue;
        };
        for (name, value) in map {
            let Some(name) = yaml_key(name) else { continue };
            // 差分は `笑顔: smile.png` か `笑顔: { file: smile.png }`。`dir` とつなぐ。
            let file = value
                .as_str()
                .or_else(|| yaml_str(value, "file"))
                .map(|f| format!("{}{f}", with_slash(folder)));
            if let (Some(file), Some(slot)) = (file, slots.get_mut(&name)) {
                *slot = asset_file(&dir, &file);
            }
        }
    }
    out
}

fn with_slash(dir: &str) -> String {
    if dir.is_empty() || dir.ends_with('/') {
        dir.to_owned()
    } else {
        format!("{dir}/")
    }
}

/// プレビューのデータを作る。`root` は作品フォルダ（素材のファイルを探す）。
pub fn preview_data<'a>(project: &Project, root: &Path, ir: &'a Ir) -> PreviewData<'a> {
    let settings = &project.settings;
    PreviewData {
        ir,
        screen: settings.screen.map_or(DEFAULT_SCREEN, |s| ScreenSize {
            width: s.width,
            height: s.height,
        }),
        message_window: settings
            .message_window
            .map_or(DEFAULT_WINDOW, |w| WindowSize {
                lines: w.lines,
                chars_per_line: w.chars_per_line,
            }),
        brackets: settings
            .brackets
            .iter()
            .map(|b| BracketStyle {
                open: b.open.to_string(),
                close: b.close.to_string(),
                style: b.style.clone(),
            })
            .collect(),
        characters: project
            .characters
            .iter()
            .map(|c| PreviewCharacter {
                id: c.id.clone(),
                name: c.name.clone(),
            })
            .collect(),
        variables: project
            .variables
            .iter()
            .map(|v| {
                let (initial, bad_default) = initial_value(&v.ty, v.default.as_deref());
                PreviewVariable {
                    key: v.key.clone(),
                    ty: v.ty.name(),
                    values: match &v.ty {
                        VarType::Enum(values) => values.clone(),
                        _ => Vec::new(),
                    },
                    initial,
                    bad_default,
                    scope: v.scope.clone(),
                    range: v.range,
                }
            })
            .collect(),
        assets: assets(project, root),
    }
}

/// 素材のファイルを置く場所（アセットプロトコルで見せてよいフォルダ）。
pub fn assets_dir(root: &Path) -> PathBuf {
    root.join("assets")
}
