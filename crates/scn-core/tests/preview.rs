//! 簡易ADVプレビューのデータ（`preview`、T1-11、D050）：画面とウィンドウの大きさ、括弧と表示スタイル、
//! 変数の初期値、素材のファイル。サンプルのデータはスナップショットにし、フロントのランタイムのテスト
//! （`app/test/preview.test.ts`）がそれを読んで、各エンディングまで通す。

use std::fs;
use std::path::{Path, PathBuf};

use scn_core::preview::preview_data;
use scn_core::{analyze, load_project};

fn sample() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago")
}

/// サンプルのプレビューのデータ。素材のファイルはサンプルに無いので、どれも null（絶対パスが入らない）。
#[test]
fn sample_preview_data() {
    let project = load_project(&sample()).unwrap();
    let ir = analyze(&project);
    let data = preview_data(&project, &sample(), &ir);
    assert_eq!((data.screen.width, data.screen.height), (1280, 720));
    assert_eq!(
        (
            data.message_window.lines,
            data.message_window.chars_per_line
        ),
        (3, 24)
    );
    let json = serde_json::to_string_pretty(&data).unwrap();
    insta::assert_snapshot!("houkago", json);
}

/// フェーズ1のゲートの短編（`samples/amayadori`。T1-13）のプレビューのデータ。フロントのテストが、選択肢だけで
/// すべてのエンディングに着けることを確かめる。
#[test]
fn gate_story_preview_data() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/amayadori");
    let project = load_project(&root).unwrap();
    let ir = analyze(&project);
    let json = serde_json::to_string_pretty(&preview_data(&project, &root, &ir)).unwrap();
    insta::assert_snapshot!("amayadori", json);
}

/// 変数の初期値：`default` を型どおりに読み、無いか合わなければ型の既定。
#[test]
fn initial_values() {
    let dir = temp("initial");
    fs::write(dir.join("project.toml"), "format_version = 0\n").unwrap();
    fs::write(
        dir.join("variables.yaml"),
        "a: { type: int, default: 3 }\nb: { type: int, default: x }\nc: { type: bool, default: true }\nd: { type: enum, values: [甲, 乙], default: 乙 }\ne: { type: enum, values: [甲, 乙], default: 丙 }\nf: { type: string }\ng: { type: bool }\n",
    )
    .unwrap();
    let project = load_project(&dir).unwrap();
    let ir = analyze(&project);
    let data = preview_data(&project, &dir, &ir);
    let value = |key: &str| {
        let v = data.variables.iter().find(|v| v.key == key).unwrap();
        (v.initial.clone(), v.bad_default)
    };
    assert_eq!(value("flag.a"), (serde_json::json!(3), false));
    assert_eq!(value("flag.b"), (serde_json::json!(0), true));
    assert_eq!(value("flag.c"), (serde_json::json!(true), false));
    assert_eq!(value("flag.d"), (serde_json::json!("乙"), false));
    assert_eq!(value("flag.e"), (serde_json::json!("甲"), true));
    assert_eq!(value("flag.f"), (serde_json::json!(""), false));
    assert_eq!(value("flag.g"), (serde_json::json!(false), false));
    // 既定の画面とウィンドウ。
    assert_eq!((data.screen.width, data.screen.height), (1280, 720));
    assert_eq!(
        (
            data.message_window.lines,
            data.message_window.chars_per_line
        ),
        (3, 24)
    );
    let _ = fs::remove_dir_all(&dir);
}

/// 素材のファイル：`assets/` から数えたパスで、あるファイルだけを絶対パスにする。外を指すパスと無いファイルは
/// 使わない（仮素材）。差分は `dir` とつなぐ。BGM の loop の既定は true。
#[test]
fn asset_files() {
    let dir = temp("assets");
    fs::write(dir.join("project.toml"), "format_version = 0\n").unwrap();
    fs::create_dir_all(dir.join("assets/files/bg")).unwrap();
    fs::create_dir_all(dir.join("assets/files/chara/akari")).unwrap();
    fs::write(dir.join("assets/files/bg/roof.png"), b"png").unwrap();
    fs::write(dir.join("assets/files/chara/akari/smile.png"), b"png").unwrap();
    fs::write(dir.join("assets/files/day.ogg"), b"ogg").unwrap();
    fs::write(dir.join("secret.txt"), b"x").unwrap();
    fs::write(
        dir.join("assets/catalog.yaml"),
        r#"backgrounds:
  屋上: { file: files/bg/roof.png }
  教室: { file: files/bg/none.png }
  外: { file: ../secret.txt }
  絶対: { file: /etc/hosts }
  仮: {}
characters:
  akari:
    dir: files/chara/akari
    expressions:
      笑顔: smile.png
      真顔: { file: none.png }
bgm:
  日常: { file: files/day.ogg }
  切ない: { file: files/day.ogg, loop: false }
se:
  風: {}
"#,
    )
    .unwrap();
    let project = load_project(&dir).unwrap();
    let ir = analyze(&project);
    let assets = preview_data(&project, &dir, &ir).assets;
    let roof = assets.backgrounds["屋上"].clone().unwrap();
    assert!(roof.ends_with("roof.png") && Path::new(&roof).is_absolute());
    for name in ["教室", "外", "絶対", "仮"] {
        assert_eq!(assets.backgrounds[name], None, "{name}");
    }
    assert!(
        assets.expressions["akari"]["笑顔"]
            .as_deref()
            .is_some_and(|p| p.ends_with("smile.png"))
    );
    assert_eq!(assets.expressions["akari"]["真顔"], None);
    assert!(assets.bgm["日常"].file.is_some() && assets.bgm["日常"].repeat);
    assert!(!assets.bgm["切ない"].repeat);
    assert_eq!(assets.se["風"].file, None);
    assert!(!assets.se["風"].repeat);
    let _ = fs::remove_dir_all(&dir);
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scn-core-preview-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// `assets/` の中のシンボリックリンクが外を指していれば、使わない。
#[cfg(unix)]
#[test]
fn symlink_out_of_assets_is_refused() {
    let dir = temp("symlink");
    fs::write(dir.join("project.toml"), "format_version = 0\n").unwrap();
    fs::create_dir_all(dir.join("assets/files")).unwrap();
    fs::write(dir.join("secret.png"), b"x").unwrap();
    std::os::unix::fs::symlink(dir.join("secret.png"), dir.join("assets/files/link.png")).unwrap();
    fs::write(
        dir.join("assets/catalog.yaml"),
        "backgrounds:\n  外: { file: files/link.png }\n",
    )
    .unwrap();
    let project = load_project(&dir).unwrap();
    let ir = analyze(&project);
    assert_eq!(
        preview_data(&project, &dir, &ir).assets.backgrounds["外"],
        None
    );
    let _ = fs::remove_dir_all(&dir);
}
