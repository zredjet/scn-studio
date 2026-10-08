//! フロー図の配置（`layout.json`、T1-9、D048）：読み書きと、無いファイル・壊れたファイル・形の合わない値・
//! 無いシーンの扱い。

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use scn_core::layout::{
    Layout, LayoutError, LayoutState, Point, layout_text, read_layout, update_layout, write_layout,
};
use scn_core::workspace::{Change, Workspace};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scn-core-layout-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// 書いて読むと同じものになる。キーの順は固定で、座標は整数にそろえる。
#[test]
fn round_trip() {
    let dir = temp_dir("round-trip");
    let mut layout = Layout::default();
    layout
        .scene
        .insert("s012".into(), Point { x: 40.4, y: 118.6 });
    layout
        .scene
        .insert("s010".into(), Point { x: -8.0, y: 0.0 });
    layout
        .chapter
        .insert("story/chapter01.scn".into(), Point { x: 1.0, y: 2.0 });
    layout.route.insert("灯".into(), Point { x: 3.0, y: 4.0 });
    write_layout(&dir, &layout).unwrap();
    let text = fs::read_to_string(dir.join("layout.json")).unwrap();
    assert_eq!(text, layout_text(&layout));
    // 座標は整数、シーンIDは名前の順。
    assert!(text.find("\"s010\"").unwrap() < text.find("\"s012\"").unwrap());
    assert!(text.contains("\"x\": 40"));
    assert!(text.contains("\"y\": 119"));
    let loaded = read_layout(&dir);
    assert_eq!(loaded.problem, None);
    assert_eq!(loaded.layout.scene["s012"], Point { x: 40.0, y: 119.0 });
    assert_eq!(loaded.layout.chapter.len(), 1);
    assert_eq!(loaded.layout.route["灯"], Point { x: 3.0, y: 4.0 });
    let _ = fs::remove_dir_all(&dir);
}

/// 無ければ空で、知らせることも無い。
#[test]
fn missing_file_is_empty() {
    let dir = temp_dir("missing");
    let loaded = read_layout(&dir);
    assert_eq!(loaded.layout, Layout::default());
    assert_eq!(loaded.problem, None);
    assert_eq!(loaded.state, LayoutState::Ok);
    let _ = fs::remove_dir_all(&dir);
}

/// 壊れたファイル（JSONでない、表でない、UTF-8でない）は、空にして知らせる。表示は止めない。
#[test]
fn broken_file_is_empty_with_problem() {
    let dir = temp_dir("broken");
    for bytes in [&b"{ not json"[..], b"[1, 2]", b"\xff\xfe"] {
        fs::write(dir.join("layout.json"), bytes).unwrap();
        let loaded = read_layout(&dir);
        assert_eq!(loaded.layout, Layout::default());
        assert!(loaded.problem.is_some());
        assert_eq!(loaded.state, LayoutState::Invalid);
    }
    let _ = fs::remove_dir_all(&dir);
}

/// 形の合わない値（数でない座標、表でない粒度）は、その分だけ読み飛ばして知らせる。知らないキーは黙って
/// 読み飛ばす。無いシーンを指す位置は残す（シーンが戻ってきたときに使う）。
#[test]
fn bad_items_are_skipped() {
    let dir = temp_dir("items");
    fs::write(
        dir.join("layout.json"),
        r#"{
  "version": 1,
  "scene": {
    "s010": { "x": 1, "y": 2 },
    "s999": { "x": 5, "y": 6 },
    "bad": { "x": "1", "y": 2 },
    "short": { "x": 1 }
  },
  "chapter": [1, 2],
  "unknown": { "a": 1 }
}"#,
    )
    .unwrap();
    let loaded = read_layout(&dir);
    assert_eq!(
        loaded.layout.scene.keys().collect::<Vec<_>>(),
        ["s010", "s999"]
    );
    assert!(loaded.layout.chapter.is_empty());
    assert_eq!(
        loaded.problem.as_deref(),
        Some("layout.json の、形の合わない位置を 3 件読み飛ばしました")
    );
    assert_eq!(loaded.state, LayoutState::Ok);
    let _ = fs::remove_dir_all(&dir);
}

/// 外で `layout.json` が変わったら、アプリに読み直させる。自分で書いた変更の知らせは無視する
/// （読み直すと、動かしている途中のノードが戻るため）。
#[test]
fn external_layout_change() {
    let dir = temp_dir("external");
    fs::write(dir.join("project.toml"), "format_version = 0\n").unwrap();
    let mut ws = Workspace::open(&dir).unwrap();
    fs::write(dir.join("layout.json"), "{}").unwrap();
    assert_eq!(ws.external_change(&dir.join("layout.json")), Change::Layout);
    ws.update_layout("scene", &points(&[("s010", 1.0, 2.0)]), false)
        .unwrap();
    assert_eq!(ws.external_change(&dir.join("layout.json")), Change::None);
    fs::write(dir.join("layout.json"), "{}").unwrap();
    assert_eq!(ws.external_change(&dir.join("layout.json")), Change::Layout);
    let _ = fs::remove_dir_all(&dir);
}

fn points(items: &[(&str, f64, f64)]) -> BTreeMap<String, Point> {
    items
        .iter()
        .map(|(id, x, y)| ((*id).to_owned(), Point { x: *x, y: *y }))
        .collect()
}

/// 位置の保存は、その粒度の位置に重ねる。ほかの粒度と、無いシーンを指す位置（s999）は残す。
/// 有限でない座標は捨てる。
#[test]
fn update_keeps_other_levels_and_missing_scenes() {
    let dir = temp_dir("update");
    fs::write(
        dir.join("layout.json"),
        r#"{"scene": {"s010": {"x": 1, "y": 2}, "s999": {"x": 5, "y": 6}},
            "route": {"灯": {"x": 7, "y": 8}}}"#,
    )
    .unwrap();
    let mut moved = points(&[("s010", 10.0, 20.0), ("s011", 30.0, 40.0)]);
    moved.insert(
        "bad".into(),
        Point {
            x: f64::NAN,
            y: 0.0,
        },
    );
    update_layout(&dir, "scene", &moved, false).unwrap();
    let loaded = read_layout(&dir).layout;
    assert_eq!(
        loaded.scene.keys().collect::<Vec<_>>(),
        ["s010", "s011", "s999"]
    );
    assert_eq!(loaded.scene["s010"], Point { x: 10.0, y: 20.0 });
    assert_eq!(loaded.route["灯"], Point { x: 7.0, y: 8.0 });
    let _ = fs::remove_dir_all(&dir);
}

/// 自動整列（replace）は、その粒度の位置だけを捨てる。
#[test]
fn replace_clears_only_that_level() {
    let dir = temp_dir("replace");
    fs::write(
        dir.join("layout.json"),
        r#"{"scene": {"s010": {"x": 1, "y": 2}}, "chapter": {"story/a.scn": {"x": 3, "y": 4}}}"#,
    )
    .unwrap();
    update_layout(&dir, "scene", &BTreeMap::new(), true).unwrap();
    let loaded = read_layout(&dir).layout;
    assert!(loaded.scene.is_empty());
    assert_eq!(loaded.chapter.len(), 1);
    let _ = fs::remove_dir_all(&dir);
}

/// 壊れたファイルは、ノードを動かしても上書きしない（マージの印などを消さない）。自動整列のときだけ
/// 書き直し、その後は正しいJSONになる。
#[test]
fn broken_file_is_overwritten_only_by_replace() {
    let dir = temp_dir("broken-update");
    let broken = "<<<<<<< HEAD\n{}\n=======\n{}\n>>>>>>> other\n";
    fs::write(dir.join("layout.json"), broken).unwrap();
    let moved = points(&[("s010", 1.0, 2.0)]);
    assert!(matches!(
        update_layout(&dir, "scene", &moved, false),
        Err(LayoutError::Invalid)
    ));
    assert_eq!(fs::read_to_string(dir.join("layout.json")).unwrap(), broken);
    update_layout(&dir, "scene", &moved, true).unwrap();
    let loaded = read_layout(&dir);
    assert_eq!(loaded.state, LayoutState::Ok);
    assert_eq!(loaded.problem, None);
    assert_eq!(loaded.layout.scene["s010"], Point { x: 1.0, y: 2.0 });
    let _ = fs::remove_dir_all(&dir);
}

/// 知らない粒度には書かない。
#[test]
fn unknown_level_is_an_error() {
    let dir = temp_dir("unknown-level");
    assert!(matches!(
        update_layout(&dir, "page", &BTreeMap::new(), false),
        Err(LayoutError::UnknownLevel(_))
    ));
    assert!(!dir.join("layout.json").exists());
    let _ = fs::remove_dir_all(&dir);
}
