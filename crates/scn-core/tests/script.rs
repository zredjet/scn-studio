//! 台本の出力（`script`、T1-12、D051）：サンプルの台本のスナップショット（完了条件）と、CSV の書き方。

use std::path::{Path, PathBuf};

use scn_core::script::{
    Retakes, SCRIPT_COLUMNS, ScriptRow, ScriptSheet, baseline_of, mark_retakes, read_baseline,
    script, script_csv, script_xlsx, write_baseline,
};
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
        retakes: None,
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

/// ZIP の中のファイル（stored だけ）を、ローカルヘッダから読む。
fn unzip(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]) as usize;
    let u32_at = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
    let mut files = Vec::new();
    let mut at = 0;
    while u32_at(at) == 0x0403_4b50 {
        assert_eq!(u16_at(at + 8), 0, "stored");
        let size = u32_at(at + 18);
        assert_eq!(u32_at(at + 22), size);
        let name_len = u16_at(at + 26);
        let extra = u16_at(at + 28);
        let name = String::from_utf8(bytes[at + 30..at + 30 + name_len].to_vec()).unwrap();
        let start = at + 30 + name_len + extra;
        files.push((name, bytes[start..start + size].to_vec()));
        at = start + size;
    }
    // 中央ディレクトリと終わりの記録。
    assert_eq!(u32_at(at), 0x0201_4b50);
    let end = bytes.len() - 22;
    assert_eq!(u32_at(end), 0x0605_4b50);
    assert_eq!(u16_at(end + 10), files.len());
    assert_eq!(u32_at(end + 16), at);
    files
}

/// Excel 形式（T2-10、D061）：最小のブックの部品がそろい、欄はすべてインライン文字列。同じ台本からは同じ
/// バイト列になる。サンプルのシートの行は、CSV と同じ数。
#[test]
fn sample_xlsx() {
    let project = load_project(&sample()).unwrap();
    let ir = analyze(&project);
    let sheet = script(&project, &ir);
    let bytes = script_xlsx(&sheet).unwrap();
    assert_eq!(bytes, script_xlsx(&sheet).unwrap());
    let files = unzip(&bytes);
    let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "[Content_Types].xml",
            "_rels/.rels",
            "xl/workbook.xml",
            "xl/_rels/workbook.xml.rels",
            "xl/styles.xml",
            "xl/worksheets/sheet1.xml"
        ]
    );
    let xml = String::from_utf8(files[5].1.clone()).unwrap();
    assert_eq!(xml.matches("<row ").count(), sheet.rows.len() + 1);
    assert!(!xml.contains("t=\"n\""));
    assert!(xml.contains(
        r#"<c r="A2" s="0" t="inlineStr"><is><t xml:space="preserve">a1b3</t></is></c>"#
    ));
    assert!(xml.contains(r#"<t xml:space="preserve">やっと来た。遅いよ</t>"#));
    let workbook = String::from_utf8(files[2].1.clone()).unwrap();
    assert!(workbook.contains(r#"<sheet name="台本""#));
    insta::assert_snapshot!("houkago_xlsx_sheet", xml.replace("<row ", "\n<row "));
}

fn row(id: &str, voice: &str, text: &str) -> ScriptRow {
    ScriptRow {
        line_id: id.into(),
        scene: "s001".into(),
        character: "灯".into(),
        text: text.into(),
        narration: String::new(),
        voice: voice.into(),
        note: String::new(),
    }
}

/// 再収録候補（T2-10、D061）：基準の後に台詞か話者が変わった行と、足した行の備考に書き足す。地の文や
/// 備考の違いは数えない。行IDの無い台詞は比べない。
#[test]
fn retakes_against_baseline() {
    let before = ScriptSheet {
        rows: vec![
            row("a001", "akari_a001", "同じ"),
            row("a002", "akari_a002", "直す前"),
            row("a003", "akari_a003", "話者が変わる"),
            row("a004", "akari_a004", "消す"),
        ],
        missing_line_ids: 0,
        retakes: None,
    };
    let base = baseline_of(&before);
    assert_eq!(base.lines.len(), 4);
    let mut after = ScriptSheet {
        rows: vec![
            ScriptRow {
                narration: "地の文が変わった".into(),
                note: "AI下書き".into(),
                ..row("a001", "akari_a001", "同じ")
            },
            ScriptRow {
                note: "AI下書き".into(),
                ..row("a002", "akari_a002", "直した後")
            },
            row("a003", "kaito_a003", "話者が変わる"),
            row("a005", "akari_a005", "足した"),
            row("", "", "行IDなし"),
        ],
        missing_line_ids: 1,
        retakes: None,
    };
    mark_retakes(&mut after, &base);
    assert_eq!(
        after.retakes,
        Some(Retakes {
            changed: 2,
            added: 1,
            removed: 1,
            removed_ids: vec!["a004".to_owned()],
        })
    );
    let notes: Vec<&str> = after.rows.iter().map(|r| r.note.as_str()).collect();
    assert_eq!(
        notes,
        [
            "AI下書き",
            "AI下書き、再収録候補（収録の基準の後に変わった）",
            "再収録候補（収録の基準の後に変わった）",
            "収録の基準の後に足した",
            ""
        ]
    );
}

/// 収録の基準の読み書き：無ければ None、書いたものは読み戻せる、壊れていればわけを返す。
#[test]
fn baseline_file() {
    let dir = std::env::temp_dir().join(format!("scn-core-baseline-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert_eq!(read_baseline(&dir), Ok(None));
    let sheet = ScriptSheet {
        rows: vec![row("a001", "akari_a001", "台詞")],
        missing_line_ids: 0,
        retakes: None,
    };
    let path = write_baseline(&dir, &sheet).unwrap();
    assert_eq!(path, dir.join("export/script-baseline.json"));
    assert_eq!(read_baseline(&dir), Ok(Some(baseline_of(&sheet))));
    std::fs::write(&path, "[").unwrap();
    assert!(read_baseline(&dir).unwrap_err().contains("読めません"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// 重なった行ID（E301）の台詞は、基準にも比較にも使わない（記録した直後に再収録候補が出ないように）。
#[test]
fn retakes_skip_repeated_ids() {
    let sheet = ScriptSheet {
        rows: vec![
            row("aaaa", "akari_aaaa", "一"),
            row("aaaa", "akari_aaaa", "二"),
            row("bbbb", "akari_bbbb", "三"),
        ],
        missing_line_ids: 0,
        retakes: None,
    };
    let base = baseline_of(&sheet);
    assert_eq!(base.lines.keys().collect::<Vec<_>>(), ["bbbb"]);
    let mut again = sheet.clone();
    mark_retakes(&mut again, &base);
    let r = again.retakes.unwrap();
    assert_eq!((r.changed, r.added, r.removed), (0, 0, 0));
    assert!(again.rows.iter().all(|r| r.note.is_empty()));
}
