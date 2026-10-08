//! フロー図の配置（`layout.json`、T1-9、D048）。人が動かしたノードの位置を、粒度（シーン・章・ルート）
//! ごとに持つ。本文の差分を汚さないよう、本文とは別のファイルにする（docs/design.md）。
//!
//! 読み込みでは止まらない：無い・壊れている・形の合わない値があっても、読めた分だけを返し、読み飛ばした
//! ことを知らせる（表示を止めない）。無いシーンを指す位置も残す（そのシーンが戻ってきたときに使う）。
//! 書き込みでは、キーの順を固定し、座標を整数にそろえて、Gitの差分を小さくする。
//!
//! 壊れたファイル（JSONでない、表でない）は、ノードを動かしても上書きしない（Gitのマージの印などを
//! 消さないため）。「自動整列」のときだけ書き直す。読めないファイル（入出力の誤り）は書かない。

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::project::{ProjectError, write_source};

/// 作品フォルダの中の置き場所。
pub const LAYOUT_FILE: &str = "layout.json";

/// 形の版。
pub const LAYOUT_VERSION: u64 = 1;

/// 粒度の名前（`graph::Level` の JSON の名前と同じ）。
pub const LAYOUT_LEVELS: [&str; 3] = ["scene", "chapter", "route"];

/// ノードの位置。
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// 配置。粒度ごとに、ノードのID（シーンID、章のファイル、ルートの名前）と位置。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Layout {
    pub scene: BTreeMap<String, Point>,
    pub chapter: BTreeMap<String, Point>,
    pub route: BTreeMap<String, Point>,
}

impl Layout {
    /// 粒度の位置。知らない粒度なら `None`。
    pub fn level(&self, level: &str) -> Option<&BTreeMap<String, Point>> {
        match level {
            "scene" => Some(&self.scene),
            "chapter" => Some(&self.chapter),
            "route" => Some(&self.route),
            _ => None,
        }
    }

    pub fn level_mut(&mut self, level: &str) -> Option<&mut BTreeMap<String, Point>> {
        match level {
            "scene" => Some(&mut self.scene),
            "chapter" => Some(&mut self.chapter),
            "route" => Some(&mut self.route),
            _ => None,
        }
    }
}

/// ファイルの状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutState {
    /// 読めた（無いときも。形の合わない値を読み飛ばしたときも）。
    Ok,
    /// JSONでない、UTF-8でない、表でない。位置を保存しても上書きしない（自動整列のときだけ書き直す）。
    Invalid,
    /// 読めない（読む権限が無い、ほかのプログラムが使っているなど）。書かない。
    Unreadable,
}

/// 読み込みの結果。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LoadedLayout {
    pub layout: Layout,
    /// 読み飛ばしたもの（壊れたファイル、形の合わない値）。無ければ `None`。
    pub problem: Option<String>,
    pub state: LayoutState,
}

/// 配置を書けない。
#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    #[error("知らない粒度です：{0}")]
    UnknownLevel(String),
    #[error("{LAYOUT_FILE} が壊れているので、位置を保存しません（「自動整列」で書き直せます）")]
    Invalid,
    #[error("{LAYOUT_FILE} を読めないので、位置を保存しません")]
    Unreadable,
    #[error(transparent)]
    Write(#[from] ProjectError),
}

/// 位置を読む：`{"x": 数, "y": 数}` で、どちらも有限の数。
fn point(value: &Value) -> Option<Point> {
    let x = value.get("x")?.as_f64()?;
    let y = value.get("y")?.as_f64()?;
    (x.is_finite() && y.is_finite()).then_some(Point { x, y })
}

/// `layout.json` を読む。無ければ空。壊れていれば空にして、`problem` に書く。
pub fn read_layout(root: &Path) -> LoadedLayout {
    let empty = |state: LayoutState, problem: String| LoadedLayout {
        layout: Layout::default(),
        problem: Some(problem),
        state,
    };
    let path = root.join(LAYOUT_FILE);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return LoadedLayout {
                layout: Layout::default(),
                problem: None,
                state: LayoutState::Ok,
            };
        }
        Err(e) => {
            return empty(
                LayoutState::Unreadable,
                format!("{LAYOUT_FILE} を読めません：{e}。自動の配置で表示し、位置は保存しません"),
            );
        }
    };
    let value: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(e) => {
            return empty(
                LayoutState::Invalid,
                format!(
                    "{LAYOUT_FILE} を読めません（{e}）。自動の配置で表示し、直すか「自動整列」を押すまで位置は保存しません"
                ),
            );
        }
    };
    let Some(object) = value.as_object() else {
        return empty(
            LayoutState::Invalid,
            format!(
                "{LAYOUT_FILE} は表（オブジェクト）で書きます。自動の配置で表示し、直すか「自動整列」を押すまで位置は保存しません"
            ),
        );
    };
    let mut layout = Layout::default();
    let mut skipped = 0usize;
    for level in LAYOUT_LEVELS {
        let Some(entries) = object.get(level) else {
            continue;
        };
        let Some(entries) = entries.as_object() else {
            skipped += 1;
            continue;
        };
        let Some(target) = layout.level_mut(level) else {
            continue;
        };
        for (id, value) in entries {
            match point(value) {
                Some(p) => {
                    target.insert(id.clone(), p);
                }
                None => skipped += 1,
            }
        }
    }
    LoadedLayout {
        layout,
        problem: (skipped > 0)
            .then(|| format!("{LAYOUT_FILE} の、形の合わない位置を {skipped} 件読み飛ばしました")),
        state: LayoutState::Ok,
    }
}

/// `layout.json` の中身（書く文字列）。キーの順を固定し、座標を整数にそろえる。
pub fn layout_text(layout: &Layout) -> String {
    let level = |points: &BTreeMap<String, Point>| -> Value {
        Value::Object(
            points
                .iter()
                .map(|(id, p)| {
                    let mut point = serde_json::Map::new();
                    point.insert("x".into(), Value::from(p.x.round() as i64));
                    point.insert("y".into(), Value::from(p.y.round() as i64));
                    (id.clone(), Value::Object(point))
                })
                .collect(),
        )
    };
    let mut object = serde_json::Map::new();
    object.insert("version".into(), Value::from(LAYOUT_VERSION));
    object.insert("scene".into(), level(&layout.scene));
    object.insert("chapter".into(), level(&layout.chapter));
    object.insert("route".into(), level(&layout.route));
    let mut text = serde_json::to_string_pretty(&Value::Object(object)).unwrap_or_default();
    text.push('\n');
    text
}

/// `layout.json` を書く。
pub fn write_layout(root: &Path, layout: &Layout) -> Result<(), ProjectError> {
    write_source(root, LAYOUT_FILE, &layout_text(layout))
}

/// 位置を保存する：今のファイルを読み、`positions` をその粒度の位置に重ねて書く。`replace` なら、その粒度の
/// 位置をすべて捨ててから書く（自動整列）。ほかの粒度と、無いシーンを指す位置は残す。有限でない座標は捨てる。
/// 壊れたファイルは `replace` のときだけ書き直し、読めないファイルには書かない。書いた中身を返す。
pub fn update_layout(
    root: &Path,
    level: &str,
    positions: &BTreeMap<String, Point>,
    replace: bool,
) -> Result<String, LayoutError> {
    if !LAYOUT_LEVELS.contains(&level) {
        return Err(LayoutError::UnknownLevel(level.to_owned()));
    }
    let loaded = read_layout(root);
    match loaded.state {
        LayoutState::Ok => {}
        LayoutState::Invalid if replace => {}
        LayoutState::Invalid => return Err(LayoutError::Invalid),
        LayoutState::Unreadable => return Err(LayoutError::Unreadable),
    }
    let mut layout = loaded.layout;
    let Some(points) = layout.level_mut(level) else {
        return Err(LayoutError::UnknownLevel(level.to_owned()));
    };
    if replace {
        points.clear();
    }
    for (id, p) in positions {
        if p.x.is_finite() && p.y.is_finite() {
            points.insert(id.clone(), *p);
        }
    }
    let text = layout_text(&layout);
    write_source(root, LAYOUT_FILE, &text)?;
    Ok(text)
}

/// 位置のキーを変える（シーンIDと章のファイルの名前の変更。T2-8、D059）。ファイルが読めない・壊れているときと、
/// その位置が無いときは書かずに `None`。書いた中身を返す。
pub fn rename_layout_key(
    root: &Path,
    level: &str,
    from: &str,
    to: &str,
) -> Result<Option<String>, ProjectError> {
    let loaded = read_layout(root);
    if loaded.state != LayoutState::Ok {
        return Ok(None);
    }
    let mut layout = loaded.layout;
    let Some(points) = layout.level_mut(level) else {
        return Ok(None);
    };
    let Some(point) = points.remove(from) else {
        return Ok(None);
    };
    points.insert(to.to_owned(), point);
    let text = layout_text(&layout);
    write_source(root, LAYOUT_FILE, &text)?;
    Ok(Some(text))
}
