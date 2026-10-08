//! ティラノスクリプトへの書き出し（`tyrano`、T3-11、D072）：サンプルの書き出しのスナップショット（完了条件）と、
//! 記法・制御・プロファイル・素材・書き込みの扱い。

use std::fs;
use std::path::{Path, PathBuf};

use scn_core::tyrano::{
    GENERATED_DIRS, MARK_FILE, TyranoExport, TyranoFile, export_tyrano, read_profile, write_tyrano,
};
use scn_core::{analyze, load_project};

fn sample(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples")
        .join(name)
}

/// 書き出しを、人が読める1つの文字列にする（写すファイルは、作品フォルダから数えたパス）。
fn dump(root: &Path, export: &TyranoExport, problems: &[String]) -> String {
    let root = fs::canonicalize(root).unwrap();
    let mut out = String::new();
    for file in &export.files {
        match file {
            TyranoFile::Text { path, text } => {
                out.push_str(&format!("==== {path}\n{text}"));
            }
            TyranoFile::Copy { path, from } => {
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

fn export_of(root: &Path) -> (TyranoExport, Vec<String>) {
    let project = load_project(root).unwrap();
    let ir = analyze(&project);
    let (profile, problems) = read_profile(root);
    (export_tyrano(&project, root, &ir, &profile), problems)
}

fn text_of<'a>(export: &'a TyranoExport, path: &str) -> &'a str {
    export
        .files
        .iter()
        .find_map(|f| match f {
            TyranoFile::Text { path: p, text } if p == path => Some(text.as_str()),
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
            std::env::temp_dir().join(format!("scn-core-tyrano-{name}-{}", std::process::id()));
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

/// 文中の記法：括弧は本文の初めと終わり、`{p}` は `[p]`、`{se}` は `[playse]`（ファイルが無ければ鳴らさない）、
/// 変数の埋め込みは `[emb]`（真偽は `true`・`false`）、`[` と行の初めの特別な文字・空白はそのまま見せるタグ、
/// ルビと傍点、待ちと速さ。
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
         灯「{p}前だけ{p}」\n\
         灯「ドアが{se ドア}開いた」\n\
         {=flag.ok} {=flag.勇気 >= 1} {=flag.名前}\n\
         灯『心の声』\n\
         灯（）「名無し」\n\
         [x] #見出し @命令 \"引用\" 'it''s'\n\
         \u{3000}|乱《みだ》れて《《強調》》{w 0.5}{speed 遅}ゆっくり{speed 普通}。\n\
         @エンディング end\n",
    );
    let (export, _) = export_of(&t.0);
    let text = text_of(&export, "data/scenario/scn/story_a.ks");
    insta::assert_snapshot!("inline_markup", text);
    assert!(text.contains("#灯\n「前[p]後」[p]\n"), "{text}");
    assert!(text.contains("#灯\n「前だけ」[p]\n"), "{text}");
    assert!(text.contains("[emb exp=\"(f['flag.ok'] ? 'true' : 'false')\"]"));
    assert!(text.contains("[emb exp=\"f['flag.名前']\"]"));
    assert!(
        text.contains("#\n[emb exp=\"String.fromCharCode(91)\"]x]"),
        "{text}"
    );
    assert!(text.contains("[emb exp=\"String.fromCharCode(12288)\"][ruby text=\"みだ\"]乱"));
    assert!(text.contains("[ruby text=\"﹅\"]強[ruby text=\"﹅\"]調"));
    assert!(text.contains("[wait time=\"500\"][delay speed=\"60\"]ゆっくり[resetdelay]"));
    assert!(text.contains("#灯\n『心の声』[p]"));
    let start = text_of(&export, "data/scenario/scn/scn_start.ks");
    assert!(start.contains("[eval exp=\"f['flag.勇気'] = 1\"]"));
    assert!(start.contains("[eval exp=\"f['flag.名前'] = '湊'\"]"));
    assert!(start.contains("[jump storage=\"scn/story_a.ks\" target=\"*s1\"]"));
    assert!(
        export
            .warnings
            .iter()
            .any(|w| w.contains("ファイルの無い効果音"))
    );
    // 始めるときは舞台を空にして、メッセージ枠と名前の欄を用意する（`#名前` に要る）。
    assert!(start.contains("*start\n[clearstack]\n[cm]\n[chara_hide_all time=\"0\"]\n[stopbgm]\n"));
    assert!(
        start.contains("[chara_config ptext=\"chara_name_area\"]"),
        "{start}"
    );
}

/// 属性の値の初めの `&`・`%` は式・マクロの引数として読まれるので、文字列の式にする。選択肢の文の `{se}` は
/// 書かずに知らせる。ラベルの名前が重なれば `_2` を付ける。`setup: null` は準備を書かない。
#[test]
fn attribute_values_and_labels() {
    let t = Temp::new("attr");
    t.write("export/tyrano.yaml", "setup: null\n");
    t.write(
        "story/a.scn",
        "## 番号 #001\n\
         @選択\n\
         - \"引用\"で始まる -> s_001\n\
         - %50 -> s_001\n\
         - 鳴る{se ドア} -> s_001\n\
         - 普通 -> s_001\n\
         @end\n\
         ## もう一つ #s_001\n\
         |&《アンド》\n\
         @エンディング end\n",
    );
    let (export, _) = export_of(&t.0);
    let text = text_of(&export, "data/scenario/scn/story_a.ks");
    assert!(
        text.contains("text=\"&'&quot;引用&quot;で始まる'\""),
        "{text}"
    );
    assert!(text.contains("text=\"&'%50'\""), "{text}");
    assert!(text.contains("text=\"普通\""), "{text}");
    assert!(text.contains("[ruby text=\"アンド\"]&"), "{text}");
    assert!(
        text.contains("*s_001\n") && text.contains("*s_001_2\n"),
        "{text}"
    );
    assert!(text.contains("target=\"*s_001_2\"]"), "{text}");
    assert!(
        export
            .warnings
            .iter()
            .any(|w| w.contains("選択肢と問いかけ文の中の効果音")),
        "{:?}",
        export.warnings
    );
    let start = text_of(&export, "data/scenario/scn/scn_start.ks");
    assert!(!start.contains("chara_config"), "{start}");
}

/// 制御：選択肢（条件、問いかけ文、どれも選べないとき、中身と行き先、続き）、`@if`（枝のラベルと条件つきの
/// ジャンプ）、呼び出し、行き止まり、存在しないシーン、`@raw`、知らないディレクティブ、宣言の無い変数、永続の
/// 変数、割り算、ラベルの名前。
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
         @選択 どうする\n\
         - 行く {=flag.勇気}回目 if flag.勇気 >= 1\n\
         \u{20}\u{20}@設定 flag.勇気 += 1\n\
         \u{20}\u{20}-> 001\n\
         - 待つ\n\
         @end\n\
         @if flag.勇気 / 2 > 0\n\
         地の文。\n\
         @elif not 周回.解放\n\
         @else\n\
         @設定 周回.解放 = true\n\
         @end\n\
         @call sub\n\
         @設定 flag.未宣言 += 1\n\
         @選択\n\
         - 勇者なら if flag.勇気 >= 3 -> sub\n\
         - 崩れた if && -> sub\n\
         @end\n\
         -> nowhere\n\
         ## 番号 #001\n\
         @raw tyrano\n\
         [cm]\n\
         @end\n\
         @raw renpy\n\
         $ a = 1\n\
         @end\n\
         @ふしぎ 引数\n\
         ## 呼ばれる #sub\n\
         @return\n\
         ## 行き止まり #s9\n\
         地の文だけ。\n",
    );
    let (export, _) = export_of(&t.0);
    let text = text_of(&export, "data/scenario/scn/story_a.ks");
    insta::assert_snapshot!("control_flow", text);
    // 選択肢：条件つきの glink と [s]、選択肢ごとのラベル、続きのラベル。
    assert!(text.contains("target=\"*start__2\" text=\"&'行く ' + String(f['flag.勇気']) + '回目'\" cond=\"(f['flag.勇気'] >= (1))\"]"), "{text}");
    assert!(text.contains("*start__2\n[cm]\n[eval exp=\"f['flag.勇気'] += (1)\"]\n[jump storage=\"scn/story_a.ks\" target=\"*s_001\"]\n"), "{text}");
    assert!(
        text.contains("*start__3\n[cm]\n[jump target=\"*start__1\"]\n*start__1\n"),
        "{text}"
    );
    // @if：条件つきのジャンプ。割り算は0の向きへ切り捨て。永続の変数は sf。
    assert!(
        text.contains(
            "[jump target=\"*start__5\" cond=\"(Math.trunc(f['flag.勇気'] / (2)) > (0))\"]"
        ),
        "{text}"
    );
    assert!(
        text.contains("[jump target=\"*start__6\" cond=\"(!sf['周回.解放'])\"]"),
        "{text}"
    );
    assert!(text.contains("[eval exp=\"sf['周回.解放'] = true\"]"));
    // 選択肢がすべて条件付きなら、どれも選べないときに止まる。崩れた条件は偽。
    assert!(
        text.contains("[if exp=\"!((f['flag.勇気'] >= (3)) || false)\"]"),
        "{text}"
    );
    assert!(text.contains("cond=\"false\"]"));
    // 呼び出し、存在しないシーン、行き止まり。
    assert!(text.contains("[call storage=\"scn/story_a.ks\" target=\"*sub\"]"));
    assert!(
        text.contains("（行き止まり：存在しないシーン nowhere）[p]\n[jump storage=\"title.ks\"]")
    );
    assert!(
        text.contains("*s9\n#\n地の文だけ。[p]\n#\n（行き止まり：s9 行き止まり）[p]"),
        "{text}"
    );
    // @raw tyrano はそのまま、ほかは読み飛ばす。
    assert!(
        text.contains("*s_001\n[cm]\n; @raw renpy（読み飛ばし）"),
        "{text}"
    );
    let start = text_of(&export, "data/scenario/scn/scn_start.ks");
    assert!(
        start.contains("[eval exp=\"if (sf['周回.解放'] === undefined) sf['周回.解放'] = false\"]")
    );
    assert!(start.contains("[eval exp=\"f['flag.未宣言'] = 0\"]"));
    let warnings = export.warnings.join("\n");
    for expected in ["E501", "W503", "@raw renpy", "W401", "W602"] {
        assert!(warnings.contains(expected), "{expected}：{warnings}");
    }
}

/// プロファイル：位置（px）、トランジション（ミリ秒）、効果、ボイス、エンディング、キャラクターの名前、first、
/// 誤りの知らせ。素材：カタログのファイルを写し、無ければ仮素材の SVG。
#[test]
fn profile_and_assets() {
    let t = Temp::new("profile");
    t.write(
        "export/tyrano.yaml",
        "target: tyrano\noutput: build/tyrano\npositions: { 左: 10, 中: abc }\ntransitions: { フェード: 800, ゆっくり: 2000, 消える: null, 変: x }\nfx: { 揺れ: \"[quake count=3 time=300]\" }\nvoice: { pattern: \"voice/{char_id}_{line_id}.ogg\" }\nending: \"[jump storage=\\\"end.ks\\\"]\"\ncharacters: { akari: hero, x: \"1bad\" }\nfirst: true\n",
    );
    t.write(
        "assets/catalog.yaml",
        "backgrounds:\n  屋上: { file: files/bg/rooftop.png }\n  教室: {}\n\
         characters:\n  akari:\n    dir: files/chara/akari/\n    expressions:\n      笑顔: smile.png\n      泣き: {}\n\
         bgm:\n  日常: { file: files/bgm/daily.ogg }\n\
         se:\n  ドア: {}\n",
    );
    t.write("assets/files/bg/rooftop.png", "png");
    t.write("assets/files/chara/akari/smile.png", "png");
    t.write("assets/files/bgm/daily.ogg", "ogg");
    t.write("assets/voice/akari_a001.ogg", "ogg");
    t.write(
        "story/a.scn",
        "## 屋上 #s1\n\
         @背景 屋上 フェード\n\
         @立ち絵 灯 笑顔 左 ゆっくり\n\
         灯「やあ」 ^a001\n\
         @立ち絵 灯 泣き 中\n\
         @退場 灯 消える\n\
         @背景 教室 変な\n\
         @BGM 日常\n\
         @SE ドア\n\
         @効果 揺れ 0.5\n\
         @効果 光\n\
         @待機 1\n\
         @エンディング end\n",
    );
    let (profile, problems) = read_profile(&t.0);
    assert_eq!(profile.output, "build/tyrano");
    assert!(profile.first);
    let problems = problems.join("\n");
    for expected in [
        "positions の center",
        "transitions の 変",
        "characters の x",
    ] {
        assert!(problems.contains(expected), "{expected}：{problems}");
    }
    let (export, _) = export_of(&t.0);
    let text = text_of(&export, "data/scenario/scn/story_a.ks");
    assert!(
        text.contains("[bg storage=\"scn/files/bg/rooftop.png\" time=\"800\"]"),
        "{text}"
    );
    assert!(
        text.contains("[chara_show name=\"hero\" face=\"f_"),
        "{text}"
    );
    assert!(text.contains("time=\"2000\" left=\"10\"]"), "{text}");
    assert!(text.contains("left=\"448\"]"), "中は画面の幅から：{text}");
    // 表示していたキャラクターも位置へ動かす。
    assert!(
        text.contains("[chara_move name=\"hero\" left=\"448\" time=\"1\"]"),
        "{text}"
    );
    assert!(text.contains("[chara_hide name=\"hero\" time=\"0\"]"));
    assert!(text.contains("[playse storage=\"scn/voice/akari_a001.ogg\" buf=\"1\"]"));
    assert!(text.contains("[playbgm storage=\"scn/files/bgm/daily.ogg\" loop=\"true\"]"));
    assert!(text.contains("; SE ドア（ファイルが無いので鳴らさない）"));
    assert!(text.contains("[quake count=3 time=300]\n[wait time=\"500\"]"));
    assert!(text.contains("; 効果 光（対応なし）"));
    assert!(text.contains("[wait time=\"1000\"]"));
    assert!(text.contains("[jump storage=\"end.ks\"]"));
    let start = text_of(&export, "data/scenario/scn/scn_start.ks");
    assert!(
        start.contains(
            "[chara_new name=\"hero\" storage=\"scn/files/chara/akari/smile.png\" jname=\"灯\"]"
        ),
        "{start}"
    );
    assert!(
        text_of(&export, "data/scenario/first.ks").contains("[jump storage=\"scn/scn_start.ks\"]")
    );
    let paths: Vec<&str> = export.files.iter().map(TyranoFile::path).collect();
    for expected in [
        "data/bgimage/scn/files/bg/rooftop.png",
        "data/fgimage/scn/files/chara/akari/smile.png",
        "data/bgm/scn/files/bgm/daily.ogg",
        "data/sound/scn/voice/akari_a001.ogg",
        "data/bgimage/scn/.scn-studio",
        "data/scenario/scn/.scn-studio",
    ] {
        assert!(paths.contains(&expected), "{expected}：{paths:?}");
    }
    // ファイルの無い画像は SVG の仮素材。
    let svg = export
        .files
        .iter()
        .find_map(|f| match f {
            TyranoFile::Text { path, text } if path.starts_with("data/bgimage/scn/ph_") => {
                Some(text.as_str())
            }
            _ => None,
        })
        .unwrap();
    assert!(
        svg.starts_with("<svg") && svg.contains("背景：教室（仮素材）"),
        "{svg}"
    );
    let warnings = export.warnings.join("\n");
    assert!(warnings.contains("「変な」"), "{warnings}");
    assert!(warnings.contains("効果「光」"), "{warnings}");
}

/// 書き込み：作り直すフォルダを消して書き、印の無いフォルダ・書き出したものでない first.ks は消さずに止める。
#[test]
fn write_and_refuse() {
    let t = Temp::new("write");
    t.write("story/a.scn", "## a #s1\n地の文。\n@エンディング end\n");
    let out = t.0.join("out");
    let (mut export, _) = export_of(&t.0);
    let (mut profile, _) = read_profile(&t.0);
    // 人のファイルは残す。
    fs::create_dir_all(out.join("data/scenario")).unwrap();
    fs::write(out.join("data/scenario/title.ks"), "title").unwrap();
    write_tyrano(&t.0, &profile, &export, Some(&out)).unwrap();
    assert!(out.join("data/scenario/scn/story_a.ks").is_file());
    assert!(out.join("data/scenario/scn").join(MARK_FILE).is_file());
    // 前の書き出しの残りは消す。
    fs::write(out.join("data/scenario/scn/old.ks"), "old").unwrap();
    write_tyrano(&t.0, &profile, &export, Some(&out)).unwrap();
    assert!(!out.join("data/scenario/scn/old.ks").exists());
    assert_eq!(
        fs::read_to_string(out.join("data/scenario/title.ks")).unwrap(),
        "title"
    );
    // 印の無い作り直すフォルダがあれば、何も消さずに止める。
    fs::create_dir_all(out.join("data/bgimage/scn")).unwrap();
    fs::write(out.join("data/bgimage/scn/mine.png"), "png").unwrap();
    assert!(write_tyrano(&t.0, &profile, &export, Some(&out)).is_err());
    assert!(out.join("data/bgimage/scn/mine.png").is_file());
    assert!(out.join("data/scenario/scn/story_a.ks").is_file());
    fs::remove_dir_all(out.join("data/bgimage/scn")).unwrap();
    // first.ks：書き出したものでなければ上書きしない。
    t.write("export/tyrano.yaml", "first: true\n");
    (export, _) = export_of(&t.0);
    (profile, _) = read_profile(&t.0);
    fs::write(
        out.join("data/scenario/first.ks"),
        "[jump storage=\"title.ks\"]\n",
    )
    .unwrap();
    assert!(write_tyrano(&t.0, &profile, &export, Some(&out)).is_err());
    fs::remove_file(out.join("data/scenario/first.ks")).unwrap();
    write_tyrano(&t.0, &profile, &export, Some(&out)).unwrap();
    write_tyrano(&t.0, &profile, &export, Some(&out)).unwrap();
    assert!(
        fs::read_to_string(out.join("data/scenario/first.ks"))
            .unwrap()
            .contains("scn/scn_start.ks")
    );
    assert_eq!(GENERATED_DIRS.len(), 5);
}
