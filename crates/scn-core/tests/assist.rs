//! 書いている最中の補助（T1-8、D047）：文字数、今のシーンの情報、補完の材料、はみ出しだけの検査、
//! `[editor]` の設定。

use std::path::{Path, PathBuf};

use scn_core::assist::{completion_data, scene_chars, scene_info};
use scn_core::check::{check, overflow};
use scn_core::index::build_index;
use scn_core::project::{Character, Project, SourceFile};
use scn_core::{analyze, load_project};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn sample() -> Project {
    load_project(&repo().join("samples/houkago")).unwrap()
}

/// 文字数は、表示する文字を1字ずつ数える。ルビは親文字、命令と空白は数えない。
/// 話者名と括弧は数えず、選択肢の問いかけ文と表示文、ブロックの中の台詞は数える。
#[test]
fn scene_chars_counts_displayed_characters() {
    let text = "## a #s1\n灯「やあ、｜湊《みなと》{w 0.5}元気 」\n　地の文 abc\n@選択 問い\n- 行く\n  紬「うん」\n@end\n";
    // 話者が設定DBに無いと台詞にならない（地の文として、話者名と括弧まで数える）ので、登録しておく。
    let character = |id: &str, name: &str| Character {
        id: id.into(),
        name: name.into(),
        aliases: Vec::new(),
    };
    let project = Project {
        characters: vec![character("akari", "灯"), character("tsumugi", "紬")],
        files: vec![SourceFile {
            path: "story/a.scn".into(),
            text: text.into(),
        }],
        ..Project::default()
    };
    let ir = analyze(&project);
    let [scene] = ir.scenes.as_slice() else {
        panic!("シーンは1つ");
    };
    // やあ、湊元気（6）＋地の文abc（6）＋問い（2）＋行く（2）＋うん（2）
    assert_eq!(scene_chars(&scene.ops), 18);
}

/// 今のシーンの情報：登場キャラ（口調メモと一人称、台詞の数）、読み書きした変数、使った素材。
#[test]
fn scene_info_for_sample_scene() {
    let project = sample();
    let ir = analyze(&project);
    let index = build_index(&project, &ir);
    let info = scene_info(&project, &index, "s012");
    let [akari] = info.characters.as_slice() else {
        panic!("登場キャラは灯だけ：{:?}", info.characters);
    };
    assert_eq!(akari.id, "akari");
    assert_eq!(akari.name, "灯");
    assert_eq!(akari.first_person.as_deref(), Some("わたし"));
    assert!(akari.speech.as_deref().unwrap().starts_with("明るく軽口"));
    // 「」の台詞3つと『』の心の声1つ、選択肢と分岐の中の台詞2つ。
    assert_eq!(akari.lines, 5);
    let vars: Vec<(&str, usize, usize)> = info
        .variables
        .iter()
        .map(|v| (v.key.as_str(), v.reads, v.writes))
        .collect();
    assert_eq!(vars, [("akari.好感度", 1, 1), ("flag.勇気", 1, 0)]);
    let assets: Vec<(&str, Option<&str>, &str)> = info
        .assets
        .iter()
        .map(|a| (a.kind, a.character.as_deref(), a.name.as_str()))
        .collect();
    assert!(assets.contains(&("background", None, "屋上/夕方")));
    assert!(assets.contains(&("expression", Some("灯"), "笑顔")));
    assert!(assets.contains(&("expression", Some("灯"), "真顔")));
    assert!(assets.contains(&("bgm", None, "切ない")));
    assert!(assets.contains(&("se", None, "風")));
    // 無いシーンは空。
    let none = scene_info(&project, &index, "s999");
    assert!(none.characters.is_empty() && none.variables.is_empty() && none.assets.is_empty());
}

/// 補完の材料：キャラクター（呼び名を含む）、素材の意味名、変数、命令の一覧。
#[test]
fn completion_data_for_sample() {
    let project = sample();
    let data = completion_data(&project);
    let akari = data.characters.iter().find(|c| c.id == "akari").unwrap();
    assert_eq!(akari.name, "灯");
    assert!(akari.aliases.contains(&"あかり".to_owned()));
    assert!(data.backgrounds.contains(&"屋上/夕方".to_owned()));
    assert!(data.expressions["akari"].contains(&"笑顔".to_owned()));
    assert!(data.bgm.contains(&"切ない".to_owned()));
    assert!(data.variables.iter().any(|v| v.key == "akari.好感度"));
    assert_eq!(data.directives.len(), 17);
    assert_eq!(data.positions, ["左", "中", "右"]);
    assert_eq!(data.bgm_stop, "停止");
}

/// はみ出しだけの検査は、検証の段階1の W203 と同じものを返す。
#[test]
fn overflow_matches_check() {
    let dir = repo().join("crates/scn-core/tests/fixtures/check/overflow");
    let project = load_project(&dir).unwrap();
    let ir = analyze(&project);
    let index = build_index(&project, &ir);
    let from_check: Vec<_> = check(&project, &ir, &index)
        .into_iter()
        .filter(|d| d.code == "W203")
        .collect();
    let only = overflow(&project, &ir);
    assert!(!only.is_empty());
    assert_eq!(only, from_check);
}

/// `[editor]`：書いてあれば読む。無ければ切。
#[test]
fn editor_settings() {
    let project = sample();
    assert!(project.settings.editor.normalize_ellipsis);
    assert!(project.settings.editor.indent_narration);
    let dir = repo().join("crates/scn-core/tests/fixtures/check/ok_flow");
    let plain = load_project(&dir).unwrap();
    assert!(!plain.settings.editor.normalize_ellipsis);
    assert!(!plain.settings.editor.indent_narration);
}

/// アプリは、設定DBに無い話者の W202 を、この文で見分ける（`app/src/check/group.ts` の
/// `unregisteredSpeaker`。登録のボタンを出す）。文を変えるときは、あちらも直す。
#[test]
fn unregistered_speaker_message() {
    let project = Project {
        files: vec![SourceFile {
            path: "story/a.scn".into(),
            text: "## a #s1\n店員「いらっしゃい」\n".into(),
        }],
        ..Project::default()
    };
    let ir = analyze(&project);
    let w202: Vec<_> = ir.diagnostics.iter().filter(|d| d.code == "W202").collect();
    let [d] = w202.as_slice() else {
        panic!("W202 は1つ：{w202:?}");
    };
    assert!(
        d.message
            .starts_with("話者 店員 を設定DBのキャラクターに解決できません"),
        "{}",
        d.message
    );
}
