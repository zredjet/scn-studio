//! 宴への書き出し（`utage`、T3-12、D073）：サンプルの書き出しのスナップショット（完了条件）と、記法・制御・
//! プロファイル・素材・書き込みの扱い。

use std::fs;
use std::path::{Path, PathBuf};

use scn_core::utage::{
    MARK_FILE, SCENARIO_COLUMNS, UtageExport, UtageFile, UtageSheet, export_utage, read_profile,
    write_utage,
};
use scn_core::{analyze, load_project};

fn sample(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples")
        .join(name)
}

/// シートを読める形にする（1行を、欄をタブで区切った1行に。欄の中の改行は ⏎）。
fn sheet_text(sheet: &UtageSheet) -> String {
    let mut out = format!("---- {}\n{}\n", sheet.name, sheet.header.join("\t"));
    for row in &sheet.rows {
        let cells: Vec<String> = row.iter().map(|c| c.replace('\n', "⏎")).collect();
        out.push_str(cells.join("\t").trim_end_matches('\t'));
        out.push('\n');
    }
    out
}

/// 書き出しを、人が読める1つの文字列にする（写すファイルは、作品フォルダから数えたパス）。
fn dump(root: &Path, export: &UtageExport, problems: &[String]) -> String {
    let root = fs::canonicalize(root).unwrap();
    let mut out = String::from("==== scenario\n");
    for sheet in &export.scenario {
        out.push_str(&sheet_text(sheet));
    }
    out.push_str("==== settings\n");
    for sheet in &export.settings {
        out.push_str(&sheet_text(sheet));
    }
    out.push_str("==== files\n");
    for file in &export.files {
        match file {
            UtageFile::Bytes { path, bytes } => {
                out.push_str(&format!("{path}（{}バイト）\n", bytes.len()));
            }
            UtageFile::Copy { path, from } => {
                let from = Path::new(from)
                    .strip_prefix(&root)
                    .map_or_else(|_| from.clone(), |p| p.display().to_string());
                out.push_str(&format!("{path} ← {from}\n"));
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

fn export_of(root: &Path) -> (UtageExport, Vec<String>) {
    let project = load_project(root).unwrap();
    let ir = analyze(&project);
    let (profile, problems) = read_profile(root);
    (export_utage(&project, root, &ir, &profile), problems)
}

fn sheet<'a>(sheets: &'a [UtageSheet], name: &str) -> &'a UtageSheet {
    sheets
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("シート {name} がありません"))
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
            std::env::temp_dir().join(format!("scn-core-utage-{name}-{}", std::process::id()));
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

/// 文中の記法：ページごとの行（`Arg1` は表示の名前、括弧は初めと終わり）、ルビ・傍点・待ち・速さのテキストタグ、
/// 変数は `<param>`（式は一時のパラメーターに入れる行を先に）、文中の効果音は台詞の前、空の表示名は地の文。
#[test]
fn inline_markup() {
    let t = Temp::new("inline");
    t.write(
        "variables.yaml",
        "flag.勇気: { type: int, default: 1 }\nflag.名前: { type: string, default: \"湊\" }\nflag.ok: { type: bool, default: true }\n",
    );
    t.write(
        "assets/catalog.yaml",
        "se:\n  ドア: { file: files/se/door.ogg }\n",
    );
    t.write("assets/files/se/door.ogg", "ogg");
    t.write(
        "story/a.scn",
        "## 屋上 #s1\n\
         灯「前{p}後」\n\
         灯「ドアが{se ドア}開いた」\n\
         {=flag.名前} {=flag.勇気 + 1} {=flag.ok}\n\
         灯『心の声』\n\
         灯（）「名無し」\n\
         \u{3000}|乱《みだ》れて《《強調》》{w 0.5}{speed 遅}ゆっくり{p}続き{speed 普通}。\n\
         @エンディング end\n",
    );
    let (export, _) = export_of(&t.0);
    let s1 = sheet(&export.scenario, "s1");
    let text = sheet_text(s1);
    insta::assert_snapshot!("inline_markup", text);
    assert!(
        text.contains("\t灯\t\t\t\t\t\t\t「前\n\t灯\t\t\t\t\t\t\t後」\n"),
        "{text}"
    );
    assert!(
        text.contains("Se\tドア\n\t灯\t\t\t\t\t\t\t「ドアが開いた」\n"),
        "{text}"
    );
    assert!(
        text.contains("Param\tscn_value_1=(flag_勇気 + (1))\n"),
        "{text}"
    );
    assert!(
        text.contains("<param=flag_名前> <param=scn_value_1> <param=flag_ok>"),
        "{text}"
    );
    assert!(text.contains("\t灯\t\t\t\t\t\t\t『心の声』\n"), "{text}");
    assert!(text.contains("\n\t\t\t\t\t\t\t\t「名無し」\n"), "{text}");
    assert!(text.contains("\u{3000}<ruby=みだ>乱</ruby>れて<em=﹅>強調</em><interval=0.5><speed=0.1>ゆっくり</speed>\n"), "{text}");
    assert!(text.contains("<speed=0.1>続き</speed>。\n"), "{text}");
    let params = sheet_text(sheet(&export.settings, "Param"));
    assert!(params.contains("flag_勇気\tInt\t1\n"), "{params}");
    assert!(params.contains("flag_名前\tString\t湊\n"), "{params}");
    assert!(params.contains("scn_value_1\tInt\t0\n"), "{params}");
}

/// 制御：選択肢（`Selection` の行、条件、選択肢ごとのラベル、続き、どれも選べないとき）、`@if`（条件つきの
/// `Jump`）、呼び出し、行き止まり、存在しないシーン、`@raw utage`、知らないディレクティブ、永続の変数、開始の
/// シート。
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
         - 行く if flag.勇気 >= 1\n\
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
         @選択\n\
         - 勇者なら if flag.勇気 >= 3 -> sub\n\
         - 崩れた if && -> sub\n\
         @end\n\
         -> nowhere\n\
         ## 番号 #001\n\
         @raw utage\n\
         Wait\t1\n\
         @end\n\
         @raw renpy\n\
         $ a = 1\n\
         @end\n\
         @ふしぎ 引数\n\
         ## 呼ばれる #sub\n\
         @return\n",
    );
    let (export, _) = export_of(&t.0);
    let text = sheet_text(sheet(&export.scenario, "start"));
    insta::assert_snapshot!("control_flow", text);
    assert!(
        text.contains("Selection\t*start__2\t(flag_勇気 >= (1))\t\t\t\t\t\t行く\n"),
        "{text}"
    );
    assert!(
        text.contains("Selection\t*start__3\t\t\t\t\t\t\t待つ\n"),
        "{text}"
    );
    assert!(
        text.contains("*start__2\nParam\tflag_勇気+=(1)\nJump\t*s_001\n"),
        "{text}"
    );
    assert!(
        text.contains("Jump\t*start__5\t((flag_勇気 / (2)) > (0))\n"),
        "{text}"
    );
    assert!(text.contains("Jump\t*start__6\t(!周回_解放)\n"), "{text}");
    assert!(text.contains("JumpSubroutine\t*sub\n"), "{text}");
    // どれも選べないときは、選択の後ろで止まる。
    assert!(text.contains("Selection\t*start__10\tfalse\t\t\t\t\t\t崩れた\n\t\t\t\t\t\t\t\t（行き止まり：選べる選択肢がありません）\nEndScenario\n"), "{text}");
    assert!(
        text.contains("（行き止まり：存在しないシーン nowhere）"),
        "{text}"
    );
    let number = sheet_text(sheet(&export.scenario, "s_001"));
    assert!(number.contains("Wait\t1\n"), "{number}");
    assert!(number.contains("//@raw renpy（読み飛ばし）"), "{number}");
    assert!(sheet_text(sheet(&export.scenario, "sub")).contains("EndSubroutine\n"));
    // 開始のシートは scn_start（シートの名前がラベル。同じ名前のラベルの行は書かない）。
    let start = sheet_text(sheet(&export.scenario, "scn_start"));
    assert!(start.contains("Jump\t*start\n"), "{start}");
    assert!(!text.contains("*start\n"), "{text}");
    // 問いかけ文は選択肢と一緒に見せる。
    assert!(text.contains("どうする\tInput\n"), "{text}");
    let params = sheet_text(sheet(&export.settings, "Param"));
    assert!(
        params.contains("周回_解放\tBool\tfalse\tSystem\n"),
        "{params}"
    );
    let warnings = export.warnings.join("\n");
    for expected in ["E501", "@raw renpy", "W401"] {
        assert!(warnings.contains(expected), "{expected}：{warnings}");
    }
}

/// プロファイル：ブックの名前、位置、トランジション（秒）、効果（コマンドの行）、エンディング、キャラクターと
/// 変数の名前、ボイス。素材：写すファイルと設定のシートの行、ファイルの無い画像は PNG の仮素材。
#[test]
fn profile_and_assets() {
    let t = Temp::new("profile");
    t.write(
        "export/utage.yaml",
        "target: utage\nbook: story_a\npositions: { 左: -400, 中: x }\ntransitions: { フェード: 0.8, 消える: null, 変: -1 }\nfx: { 揺れ: [Shake, Camera] }\nending: [JumpScenario, \"*Title\"]\ncharacters: { akari: akari_chara }\nvariables: { flag.勇気: yuuki }\nvoice: { pattern: \"voice/{char_id}_{line_id}.ogg\" }\n",
    );
    t.write("variables.yaml", "flag.勇気: { type: int, default: 2 }\n");
    t.write(
        "assets/catalog.yaml",
        "backgrounds:\n  屋上/夕方: { file: files/bg/rooftop.png }\n  教室: {}\n\
         characters:\n  akari:\n    dir: files/chara/akari/\n    expressions:\n      笑顔: smile.png\n      泣き: {}\n\
         bgm:\n  日常: { file: files/bgm/daily.ogg, loop: false }\n\
         se:\n  ドア: {}\n",
    );
    t.write("assets/files/bg/rooftop.png", "png");
    t.write("assets/files/chara/akari/smile.png", "png");
    t.write("assets/files/bgm/daily.ogg", "ogg");
    t.write("assets/voice/akari_a001.ogg", "ogg");
    t.write(
        "story/a.scn",
        "## 屋上 #s1\n\
         @背景 屋上/夕方 フェード\n\
         @立ち絵 灯 泣き 左 消える\n\
         @立ち絵 灯 笑顔\n\
         灯「やあ」 ^a001\n\
         @退場 灯\n\
         @背景 教室 変な\n\
         @BGM 日常\n\
         @SE ドア\n\
         @効果 揺れ 0.5\n\
         @待機 1\n\
         @設定 flag.勇気 += 1\n\
         @エンディング end\n",
    );
    let (profile, problems) = read_profile(&t.0);
    assert_eq!(profile.book, "story_a");
    let problems = problems.join("\n");
    for expected in ["positions の center", "transitions の 変"] {
        assert!(problems.contains(expected), "{expected}：{problems}");
    }
    let (export, _) = export_of(&t.0);
    let text = sheet_text(sheet(&export.scenario, "s1"));
    assert!(text.contains("Bg\t屋上_夕方\t\t\t\t\t0.8\n"), "{text}");
    assert!(
        text.contains("\takari_chara\t泣き\tscn_character_left\t\t\t0\n"),
        "{text}"
    );
    assert!(text.contains("\takari_chara\t笑顔\t\t\t\t0\n"), "{text}");
    assert!(
        text.contains("\t灯\t\t\t\t\t\t\t「やあ」\t\tscn/voice/akari_a001.ogg\n"),
        "{text}"
    );
    assert!(
        text.contains("CharacterOff\takari_chara\t\t\t\t\t0\n"),
        "{text}"
    );
    // 位置ごとの Character のレイヤー（X はプロファイルの positions、無ければ画面の幅から）。
    let layers = sheet_text(sheet(&export.settings, "Layer"));
    assert!(
        layers.contains("scn_character_center\tCharacter\t0\t0\t100\n"),
        "{layers}"
    );
    assert!(
        layers.contains("scn_character_left\tCharacter\t-400\t0\t101\n"),
        "{layers}"
    );
    assert!(text.contains("Bg\t教室\t\t\t\t\t0.5\n"), "{text}");
    assert!(text.contains("Bgm\t日常\tFALSE\n"), "{text}");
    assert!(
        text.contains("//SE ドア（ファイルが無いので鳴らさない）"),
        "{text}"
    );
    assert!(
        text.contains("Shake\tCamera\nWait\t\t\t\t\t\t0.5\nWait\t\t\t\t\t\t1\nParam\tyuuki+=(1)\n"),
        "{text}"
    );
    assert!(text.contains("JumpScenario\t*Title\n"), "{text}");
    let characters = sheet_text(sheet(&export.settings, "Character"));
    // ファイルのある差分が既定のパターン（初めの行）。
    assert!(characters.contains("CharacterName\tNameText\tPattern\tFileName\nakari_chara\t灯\t笑顔\tscn/files/chara/akari/smile.png\nakari_chara\t灯\t泣き\tscn/ph_"), "{characters}");
    let textures = sheet_text(sheet(&export.settings, "Texture"));
    assert!(
        textures.contains("屋上_夕方\tBg\tscn/files/bg/rooftop.png\n"),
        "{textures}"
    );
    assert!(textures.contains("教室\tBg\tscn/ph_"), "{textures}");
    let sounds = sheet_text(sheet(&export.settings, "Sound"));
    assert!(
        sounds.contains("日常\tBgm\tscn/files/bgm/daily.ogg\n"),
        "{sounds}"
    );
    let paths: Vec<&str> = export.files.iter().map(UtageFile::path).collect();
    for expected in [
        "story_a.xlsx",
        "story_a_settings.xlsx",
        "Texture/Bg/scn/files/bg/rooftop.png",
        "Texture/Character/scn/files/chara/akari/smile.png",
        "Sound/Bgm/scn/files/bgm/daily.ogg",
        "Sound/Voice/scn/voice/akari_a001.ogg",
        MARK_FILE,
    ] {
        assert!(paths.contains(&expected), "{expected}：{paths:?}");
    }
    // 仮素材は PNG。ブックはシートの名前を持つ。
    let bytes_of = |prefix: &str| {
        export
            .files
            .iter()
            .find_map(|f| match f {
                UtageFile::Bytes { path, bytes } if path.starts_with(prefix) => Some(bytes.clone()),
                _ => None,
            })
            .unwrap()
    };
    assert!(bytes_of("Texture/Bg/scn/ph_").starts_with(b"\x89PNG\r\n\x1a\n"));
    let book = String::from_utf8_lossy(&bytes_of("story_a.xlsx")).into_owned();
    assert!(
        book.contains("<sheet name=\"scn_start\" sheetId=\"1\""),
        "シートの一覧"
    );
    assert!(
        book.contains("<sheet name=\"s1\" sheetId=\"2\""),
        "シートの一覧"
    );
    let settings = String::from_utf8_lossy(&bytes_of("story_a_settings.xlsx")).into_owned();
    for name in ["Character", "Texture", "Sound", "Param", "Layer"] {
        assert!(
            settings.contains(&format!("<sheet name=\"{name}\"")),
            "{name}"
        );
    }
    assert_eq!(SCENARIO_COLUMNS.first(), Some(&"Command"));
    let warnings = export.warnings.join("\n");
    assert!(warnings.contains("「変な」"), "{warnings}");
}

/// 書き込み：書き出し先をまるごと作り直し、印の無いフォルダ（空でない）は消さずに止める。
#[test]
fn write_and_refuse() {
    let t = Temp::new("write");
    t.write("story/a.scn", "## a #s1\n地の文。\n@エンディング end\n");
    let out = t.0.join("out");
    let (export, _) = export_of(&t.0);
    let (profile, _) = read_profile(&t.0);
    // 空のフォルダには書ける。
    fs::create_dir_all(&out).unwrap();
    write_utage(&t.0, &profile, &export, Some(&out)).unwrap();
    assert!(out.join("scn.xlsx").is_file());
    assert!(out.join(MARK_FILE).is_file());
    // 前の書き出しの残りは消す。
    fs::write(out.join("old.txt"), "old").unwrap();
    write_utage(&t.0, &profile, &export, Some(&out)).unwrap();
    assert!(!out.join("old.txt").exists());
    // 印の無い、空でないフォルダは消さずに止める。
    let mine = t.0.join("mine");
    fs::create_dir_all(&mine).unwrap();
    fs::write(mine.join("a.xlsx"), "mine").unwrap();
    assert!(write_utage(&t.0, &profile, &export, Some(&mine)).is_err());
    assert!(mine.join("a.xlsx").is_file());
}

/// シナリオラベル（シートの名前）は31文字に入るように切り、大文字と小文字を区別せずに重ならないようにする。
/// 選択肢の文の式の埋め込みの行は、`Selection` の行の並びの前に書く。
#[test]
fn long_labels_and_option_values() {
    let t = Temp::new("labels");
    t.write("variables.yaml", "flag.勇気: { type: int, default: 1 }\n");
    t.write(
        "story/a.scn",
        "## 一 #chapter_one_rooftop_confession_good_1\n\
         @選択\n\
         - 一 -> chapter_one_rooftop_confession_good_2\n\
         - 二 {=flag.勇気 + 1} -> chapter_one_rooftop_confession_good_2\n\
         @end\n\
         ## 二 #chapter_one_rooftop_confession_good_2\n\
         @エンディング end\n",
    );
    let (export, _) = export_of(&t.0);
    let names: Vec<&str> = export.scenario.iter().map(|s| s.name.as_str()).collect();
    assert!(names.iter().all(|n| n.chars().count() <= 31), "{names:?}");
    let lower: std::collections::BTreeSet<String> =
        names.iter().map(|n| n.to_ascii_lowercase()).collect();
    assert_eq!(lower.len(), names.len(), "{names:?}");
    let first = export.scenario.get(1).unwrap();
    let text = sheet_text(first);
    let second = names.get(2).unwrap();
    assert!(
        text.contains(&format!(
            "Param\tscn_value_1=(flag_勇気 + (1))\nSelection\t*{}__2",
            first.name
        )),
        "{text}"
    );
    assert!(text.contains(&format!("Jump\t*{second}\n")), "{text}");
}
