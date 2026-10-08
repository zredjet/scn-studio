//! 台本の出力（`script`、T1-12、D051）：サンプルの台本のスナップショット（完了条件）と、CSV の書き方。

use std::path::{Path, PathBuf};

use scn_core::script::{SCRIPT_COLUMNS, ScriptRow, ScriptSheet, script, script_csv};
use scn_core::{analyze, load_project};

fn sample() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago")
}

/// サンプルの台本。CSV の BOM と CRLF は、読みやすさのためにスナップショットでは外す（別のテストで確かめる）。
#[test]
fn sample_script() {
    let project = load_project(&sample()).unwrap();
    let ir = analyze(&project);
    let sheet = script(&project, &ir);
    assert_eq!(sheet.missing_line_ids, 2);
    let csv = script_csv(&sheet);
    assert!(csv.starts_with("\u{feff}行ID,シーン,キャラ"));
    insta::assert_snapshot!(
        "houkago",
        csv.trim_start_matches('\u{feff}').replace("\r\n", "\n")
    );
}

/// CSV：`"`・`,`・改行を含む欄は `"` で囲み、`"` は2つにする。改行は CRLF。先頭に BOM。
#[test]
fn csv_quoting() {
    let row = ScriptRow {
        line_id: "k3f9".into(),
        scene: "s001 屋上".into(),
        character: "灯".into(),
        text: "一行目\n二行目".into(),
        narration: "「a, b」".into(),
        voice: "akari_k3f9".into(),
        note: "\"引用\"".into(),
    };
    let csv = script_csv(&ScriptSheet {
        rows: vec![row],
        missing_line_ids: 0,
    });
    assert_eq!(
        csv,
        format!(
            "\u{feff}{}\r\nk3f9,s001 屋上,灯,\"一行目\r\n二行目\",\"「a, b」\",akari_k3f9,\"\"\"引用\"\"\"\r\n",
            SCRIPT_COLUMNS.join(",")
        )
    );
}

/// 台詞の中の記法：ルビは `親文字（読み）`、文中の効果音は備考、表示名は名前に添えて備考にも書く。選択肢の
/// 中身と `@if` の枝の台詞も、直前の地の文を引き継いで出す。
#[test]
fn inline_markup_and_blocks() {
    let dir = std::env::temp_dir().join(format!("scn-core-script-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("story")).unwrap();
    std::fs::create_dir_all(dir.join("bible/entries/character")).unwrap();
    std::fs::write(dir.join("project.toml"), "format_version = 0\n").unwrap();
    std::fs::write(
        dir.join("bible/entries/character/akari.md"),
        "---\nid: akari\nname: 灯\n---\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("story/a.scn"),
        "## 屋上 #s1\n地の文。 ^aaaa\n灯（？？？）「|乱《みだ》れて{se 風}いる」 ^aaab\n@選択 ^aaac\n- 行く ^aaad\n  中の地の文。 ^aaaf\n  灯「来た」 ^aaae\n@end\n灯「後ろ」 ^aaag\n灯「重なり」 ^aaag\n",
    )
    .unwrap();
    let project = load_project(&dir).unwrap();
    let ir = analyze(&project);
    let sheet = script(&project, &ir);
    assert_eq!(sheet.rows.len(), 4);
    let first = &sheet.rows[0];
    assert_eq!(first.text, "乱（みだ）れている");
    assert_eq!(first.character, "灯（？？？）");
    assert_eq!(first.narration, "地の文。");
    assert_eq!(first.voice, "akari_aaab");
    assert_eq!(first.note, "表示名：？？？、SE：風");
    // 選択肢の中では、中の地の文がいちばん近い。
    assert_eq!(sheet.rows[1].narration, "中の地の文。");
    assert_eq!(sheet.rows[1].line_id, "aaae");
    // ブロックの後ろへは、中の地の文を持ち越さない（どの枝を通ったか分からない）。
    assert_eq!(sheet.rows[2].narration, "地の文。");
    // 行IDが重なっていれば、備考で知らせる。
    assert!(sheet.rows[2].note.contains("E301"));
    assert!(sheet.rows[3].note.contains("E301"));
    let _ = std::fs::remove_dir_all(&dir);
}
