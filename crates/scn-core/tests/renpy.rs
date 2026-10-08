//! Ren'Py への書き出し（`renpy`、T2-2、D053）：サンプルの書き出しのスナップショット（完了条件）と、
//! 記法・制御・プロファイル・素材・書き込みの扱い。

use std::fs;
use std::path::{Path, PathBuf};

use scn_core::renpy::{RenpyExport, RenpyFile, export_renpy, read_profile, write_renpy};
use scn_core::{analyze, load_project};

fn sample(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples")
        .join(name)
}

/// 書き出しを、人が読める1つの文字列にする（写すファイルは、作品フォルダから数えたパス）。
fn dump(root: &Path, export: &RenpyExport, problems: &[String]) -> String {
    let root = fs::canonicalize(root).unwrap();
    let mut out = String::new();
    for file in &export.files {
        match file {
            RenpyFile::Text { path, text } => {
                out.push_str(&format!("==== {path}\n{text}"));
            }
            RenpyFile::Copy { path, from } => {
                let from = Path::new(from)
                    .strip_prefix(&root)
                    .map_or_else(|_| from.clone(), |p| p.display().to_string());
                out.push_str(&format!("==== {path} ← {from}\n"));
            }
        }
    }
    out.push_str("==== warnings\n");
    for w in &export.warnings {
        out.push_str(&format!("{w}\n"));
    }
    out.push_str("==== profile\n");
    for p in problems {
        out.push_str(&format!("{p}\n"));
    }
    out
}

fn export_of(root: &Path) -> (RenpyExport, Vec<String>) {
    let project = load_project(root).unwrap();
    let ir = analyze(&project);
    let (profile, problems) = read_profile(root);
    (export_renpy(&project, root, &ir, &profile), problems)
}

fn text_of<'a>(export: &'a RenpyExport, path: &str) -> &'a str {
    export
        .files
        .iter()
        .find_map(|f| match f {
            RenpyFile::Text { path: p, text } if p == path => Some(text.as_str()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{path} がありません"))
}

#[test]
fn houkago() {
    let root = sample("houkago");
    let (export, problems) = export_of(&root);
    assert!(problems.is_empty(), "{problems:?}");
    insta::assert_snapshot!("houkago", dump(&root, &export, &problems));
}

#[test]
fn amayadori() {
    let root = sample("amayadori");
    let (export, problems) = export_of(&root);
    assert!(problems.is_empty(), "{problems:?}");
    insta::assert_snapshot!("amayadori", dump(&root, &export, &problems));
}

/// 作業用の作品フォルダ。
struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("scn-core-renpy-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("story")).unwrap();
        fs::create_dir_all(dir.join("bible/entries/character")).unwrap();
        fs::write(
            dir.join("project.toml"),
            "format_version = 0\n[dialogue.brackets]\n\"「」\" = \"normal\"\n\"『』\" = \"thought\"\n",
        )
        .unwrap();
        fs::write(
            dir.join("bible/entries/character/akari.md"),
            "---\nid: akari\nname: 灯\n---\n",
        )
        .unwrap();
        Self(dir)
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// 文中の記法：`{p}` は次の say 文にして括弧を最初と最後にだけ付け、`{se}` は `{nw}` と `extend`、
/// 変数の埋め込みは `[名前]`（式は一時変数）、Ren'Py が特別に読む文字は重ねる。
#[test]
fn inline_markup() {
    let t = Temp::new("inline");
    t.write(
        "variables.yaml",
        "flag.勇気: { type: int, default: 1 }\nflag.名前: { type: string, default: \"湊\" }\nflag.ok: { type: bool, default: true }\n",
    );
    t.write(
        "story/a.scn",
        "## 屋上 #s1\n\
         灯「前{p}後」\n\
         灯「ドアが{se ドア}開いた」\n\
         灯「待って！{se ドア}{p}……行っちゃった」\n\
         灯「前{p}{se ドア}後」\n\
         {=flag.ok} {=flag.勇気 >= 1} {=not flag.ok}\n\
         灯『心の声』\n\
         灯（）「名無し」\n\
         [x] {{y}} 100% 【見出し】 \"引用\" \\ {=flag.名前} {=flag.勇気 + 1}\n\
         |乱《みだ》れて《《強調》》{w 0.5}{speed 遅}ゆっくり{p}続き{speed 普通}。\n\
         @エンディング end\n",
    );
    let (export, _) = export_of(&t.0);
    let text = text_of(&export, "game/scn/story_a.rpy");
    insta::assert_snapshot!("inline_markup", text);
    let defs = text_of(&export, "game/scn/scn_defs.rpy");
    assert!(
        defs.contains("define akari = Character(\"灯\", what_prefix=\"「\", what_suffix=\"」\")")
    );
    assert!(defs.contains(
        "define akari_thought = Character(\"灯\", kind=akari, what_prefix=\"『\", what_suffix=\"』\")"
    ));
    assert!(defs.contains(
        "define akari_name = Character(None, kind=akari, what_prefix=\"「\", what_suffix=\"」\")"
    ));
    assert!(defs.contains("default flag_勇気 = 1\n"));
    assert!(defs.contains("default flag_名前 = \"湊\"\n"));
}

/// 制御：選択肢（条件、問いかけ文の一時変数は `menu` の前、空の中身は `pass`）、`@if`、呼び出し、
/// 行き止まり、存在しないシーン、`@raw`、知らないディレクティブ、宣言の無い変数、永続の変数、ラベルの名前。
#[test]
fn control_flow() {
    let t = Temp::new("control");
    t.write(
        "variables.yaml",
        "flag.勇気: { type: int }\n周回.解放: { type: bool, scope: persistent }\n",
    );
    t.write(
        "story/a.scn",
        "## 始まり #start\n\
         @選択 {=flag.勇気 * 2}回目\n\
         - 行く if flag.勇気 >= 1 && -> 001\n\
         - 待つ if flag.勇気 >= 2 -> 001\n\
         - 黙る\n\
         @end\n\
         @if flag.勇気 / 2 > 0\n\
         @elif not 周回.解放\n\
         \u{3000}地の文。\n\
         @else\n\
         @設定 周回.解放 = true\n\
         @end\n\
         @call sub\n\
         @設定 flag.未宣言 += 1\n\
         @設定 flag.選択① = 1\n\
         @選択 空\n\
         @end\n\
         @選択\n\
         - 勇者なら if flag.勇気 >= 3 -> sub\n\
         - 崩れた if && -> sub\n\
         @end\n\
         -> nowhere\n\
         ## 番号 #001\n\
         @raw renpy\n\
         \u{20}\u{20}$ a = 1\n\
         \u{20}\u{20}if a:\n\
         \u{20}\u{20}    $ a = 2\n\
         @end\n\
         @raw tyrano\n\
         [cm]\n\
         @end\n\
         @ふしぎ 引数\n\
         ## 呼ばれる #sub\n\
         @return\n\
         ## 行き止まり #s9\n\
         地の文だけ。\n",
    );
    let (export, _) = export_of(&t.0);
    insta::assert_snapshot!("control_flow", text_of(&export, "game/scn/story_a.rpy"));
    let defs = text_of(&export, "game/scn/scn_defs.rpy");
    assert!(defs.contains("default persistent.周回_解放 = False\n"));
    assert!(defs.contains("default flag_未宣言 = 0  # 宣言の無い変数（W602）\n"));
    assert!(defs.contains("default flag_選択_ = 0  # 宣言の無い変数（W602）\n"));
    assert_eq!(
        text_of(&export, "game/scn/scn_start.rpy").lines().last(),
        Some("    jump s_start")
    );
    insta::assert_debug_snapshot!("control_flow_warnings", export.warnings);
}

/// プロファイルの値の誤りは読み飛ばし、何を読み飛ばしたかを返す（書き出しは止めない）。知らないキーは黙って
/// 読み飛ばす（作品フォルダの規約）。
#[test]
fn profile_problems() {
    let t = Temp::new("profile");
    t.write("story/a.scn", "## a #s1\n地の文。\n@エンディング e\n");
    t.write("export/renpy.yaml", "target: tyrano\n: [\n");
    let (_, problems) = read_profile(&t.0);
    assert_eq!(problems.len(), 1);
    assert!(
        problems[0].starts_with("YAMLとして読めません"),
        "{problems:?}"
    );

    t.write(
        "export/renpy.yaml",
        "target: tyrano\noutput: ../outside\nimages: { 背景: \"bg {file_stem}\", 顔: x }\n\
         positions: { 左: truecenter, 上: top }\ntransitions: { フェード: dissolve, 即時: null }\n\
         fx: [a]\nvoice: voice.ogg\nstart_label: maybe\ncommand: python x.py\nfonts: x\n",
    );
    let (profile, problems) = read_profile(&t.0);
    insta::assert_debug_snapshot!("profile_problems", problems);
    assert_eq!(profile.output, "export/renpy");
    assert_eq!(
        profile.positions.get("left").map(String::as_str),
        Some("truecenter")
    );
    assert_eq!(
        profile.transitions.get("即時").map(String::as_str),
        Some("None")
    );
    assert!(profile.start_label);

    t.write(
        "export/renpy.yaml",
        "start_label: false\nending: return\nvariables: { flag.x: score }\ncharacters: { akari: a }\n",
    );
    let (profile, problems) = read_profile(&t.0);
    assert!(problems.is_empty(), "{problems:?}");
    t.write(
        "story/a.scn",
        "## a #s1\n@設定 flag.x = 1\n灯「やあ」\n@エンディング e\n",
    );
    let project = load_project(&t.0).unwrap();
    let ir = analyze(&project);
    let export = export_renpy(&project, &t.0, &ir, &profile);
    assert!(
        export
            .files
            .iter()
            .all(|f| f.path() != "game/scn/scn_start.rpy"),
        "start_label: false でも label start を書いた"
    );
    let story = text_of(&export, "game/scn/story_a.rpy");
    assert!(
        story.contains("    $ score = 1\n    a \"やあ\"\n    # エンディング e\n    return\n"),
        "{story}"
    );
}

/// 素材：カタログのファイルを `game/scn/` に写して使う。ファイルの無い素材は仮素材（画像）か鳴らさない（音）。
/// ボイスとフォントも、ファイルがあれば写す。
#[test]
fn assets_and_write() {
    let t = Temp::new("assets");
    t.write("assets/files/bg/room.png", "png");
    t.write("assets/files/chara/akari/smile.png", "png");
    t.write("assets/files/bgm/theme.ogg", "ogg");
    t.write("assets/voice/akari_k3f9.ogg", "ogg");
    t.write("fonts/my.ttf", "ttf");
    t.write(
        "assets/catalog.yaml",
        "backgrounds:\n  部屋: { file: files/bg/room.png }\n  外: {}\n\
         characters:\n  akari:\n    dir: files/chara/akari/\n    expressions:\n      笑顔: smile.png\n      泣き: {}\n\
         bgm:\n  テーマ: { file: files/bgm/theme.ogg, loop: false }\n\
         se:\n  ドア: {}\n",
    );
    t.write(
        "export/renpy.yaml",
        "voice: { pattern: \"voice/{char_id}_{line_id}.ogg\" }\nfont: fonts/my.ttf\n",
    );
    t.write(
        "story/a.scn",
        "## a #s1\n@背景 部屋 フェード\n@背景 外 ワイプ\n@立ち絵 灯 笑顔 左\n@立ち絵 灯 泣き\n\
         @BGM テーマ\n@SE ドア\n灯「声{p}続き{se ドア}おわり」 ^k3f9\n灯「無声」 ^k3fa\n@退場 灯\n@BGM 停止\n@エンディング e\n",
    );
    let (export, problems) = export_of(&t.0);
    assert!(problems.is_empty(), "{problems:?}");
    insta::assert_snapshot!("assets", dump(&t.0, &export, &problems));

    let out = t.0.join("export/renpy");
    let profile = read_profile(&t.0).0;
    // 人が置いたファイルは残し、`game/scn/` だけを作り直す。
    t.write("export/renpy/game/options.rpy", "# 人のファイル\n");
    t.write("export/renpy/game/scn/stale.rpy", "# 古い\n");
    let err = write_renpy(&t.0, &profile, &export, None).unwrap_err();
    assert!(err.to_string().contains("消さずに止めました"), "{err}");
    assert!(out.join("game/scn/stale.rpy").exists());
    fs::remove_dir_all(out.join("game/scn")).unwrap();

    assert_eq!(write_renpy(&t.0, &profile, &export, None).unwrap(), out);
    t.write("export/renpy/game/scn/stale.rpy", "# 古い\n");
    write_renpy(&t.0, &profile, &export, None).unwrap();
    assert!(!out.join("game/scn/stale.rpy").exists());
    assert!(out.join("game/options.rpy").exists());
    assert_eq!(
        fs::read_to_string(out.join("game/scn/files/bg/room.png")).unwrap(),
        "png"
    );
    assert_eq!(
        fs::read_to_string(out.join("game/scn/voice/akari_k3f9.ogg")).unwrap(),
        "ogg"
    );
    assert_eq!(
        fs::read_to_string(out.join("game/scn/fonts/my.ttf")).unwrap(),
        "ttf"
    );

    // 書き出し先を指定する。
    let other = t.0.join("elsewhere");
    assert_eq!(
        write_renpy(&t.0, &profile, &export, Some(&other)).unwrap(),
        other
    );
    assert!(other.join("game/scn/scn_defs.rpy").exists());
}
