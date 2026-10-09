//! 作品全体の検索の材料（`search`、T4-22）：本文の範囲は、台詞の括弧の中と地の文だけ（地の文として読む台詞の形の
//! 行は、話者から行の終わりまで。仕様書6節）。話者の名前・行ID・演出・ジャンプ・選択肢の表示文と問いかけ文・
//! メタ情報・コメント・文中の命令を含まない。行IDの範囲も返す。

use std::path::PathBuf;

use scn_core::load_project;
use scn_core::project::Character;
use scn_core::search::{search_ranges, story_search_ranges};
use scn_core::syntax::{ParseOptions, parse};

fn characters() -> Vec<Character> {
    ["灯", "湊"]
        .iter()
        .map(|name| Character {
            id: (*name).to_owned(),
            name: (*name).to_owned(),
            aliases: Vec::new(),
        })
        .collect()
}

/// 本文の範囲の文字と、行IDの範囲の文字。
fn ranges(source: &str) -> (Vec<String>, Vec<String>) {
    let parsed = parse(source, &ParseOptions::default());
    let r = search_ranges(source, &parsed.chapter, &parsed.diagnostics, &characters());
    let pick = |list: &[(usize, usize)]| -> Vec<String> {
        list.iter()
            .map(|&(s, e)| source.get(s..e).unwrap_or_default().to_owned())
            .collect()
    };
    (pick(&r.text), pick(&r.line_ids))
}

#[test]
fn body_text_only() {
    let source = "\
# 第1章
## 屋上 #s1
---
summary: 要約は入らない
---
// コメントは入らない ^cccc
@背景 屋上/夕方 ^dddd
灯「あのね、{w 0.5}話があるんだ」 ^k3f9
　風が強くて、｜灯《あかり》の髪が乱れた。 ^k3fa
湊「行こう」と言った。 ^k3fb
店員「いらっしゃい」 ^k3fc
@選択 どうする？ ^k3fd
  - 打ち明ける ^k3fe
    灯「実はね」 ^k3ff
    -> s1
@end
@if 灯.好感度 >= 3
  　夕日が沈む。 ^k3fg
@else
  　雨が降る。 ^k3fh
@end
-> s1
";
    let (text, ids) = ranges(source);
    assert_eq!(
        text,
        [
            "あのね、",
            "話があるんだ",
            "　風が強くて、｜灯《あかり》の髪が乱れた。",
            // 閉じ括弧の後ろに文が続く行と、話者を解決できない行（W202）は、地の文として行ごと（仕様書6節）。
            "湊「行こう」と言った。",
            "店員「いらっしゃい」",
            "実はね",
            "　夕日が沈む。",
            "　雨が降る。",
        ]
    );
    // 行IDの範囲（演出の行に付いたもの（W303）も。コメントの行の ^cccc は行IDではない）。
    assert_eq!(
        ids,
        [
            "^dddd", "^k3f9", "^k3fa", "^k3fb", "^k3fc", "^k3fd", "^k3fe", "^k3ff", "^k3fg",
            "^k3fh",
        ]
    );
}

#[test]
fn narration_like_dialogue_drops_comments_commands_and_bad_ids() {
    let source = "\
# 第1章
## 駅 #s1
湊「行こう{w 0.5}」と言った。 ^ABC
店員「いらっしゃい
// 途中のコメント
ませ」 ^k3fa
店員「どうぞ ^k3fb
こちらへ ^XYZ
お席へ」 ^QQQ
-> s1
";
    let (text, ids) = ranges(source);
    // 文中の命令と形の合わない行IDの候補（E304）と、途中のコメントの行と、途中の行の行IDの候補は、地の文として
    // 読む行の本文に入れない。
    assert_eq!(
        text,
        [
            "湊「行こう",
            "」と言った。",
            "店員「いらっしゃい",
            "ませ」",
            "店員「どうぞ",
            "こちらへ",
            "お席へ」",
        ]
    );
    assert_eq!(ids, ["^k3fa", "^k3fb"]);
}

#[test]
fn ranges_of_every_chapter() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago");
    let project = load_project(&root).unwrap();
    let all = story_search_ranges(&project);
    assert_eq!(all.len(), project.files.len());
    // スタブだけの章（chapter03）には本文が無い。
    assert!(all.values().filter(|r| !r.text.is_empty()).count() >= 2);
    for file in &project.files {
        let r = &all[&file.path];
        // 重ならず、前から順。行IDを含まず、行をまたがない。
        assert!(r.text.windows(2).all(|w| w[0].1 < w[1].0));
        for &(s, e) in &r.text {
            let text = file.text.get(s..e).unwrap();
            assert!(!text.contains('^'), "{text}");
            assert!(!text.contains('\n'), "{text}");
        }
        for &(s, e) in &r.line_ids {
            let id = file.text.get(s..e).unwrap();
            assert!(id.starts_with('^') && id.len() >= 5, "{id}");
        }
    }
}
