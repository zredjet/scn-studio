//! 環境設定の「この作品」の頁（T4-25、N6）：`project.toml` のキーを、コメントと並びを残して直す。正しくない値と、
//! 読み直して新しい診断が出る中身は書かない。

use std::path::PathBuf;

use scn_core::project::read_settings;
use scn_core::projectedit::{SettingEdit, edit_project_toml};
use serde_json::{Value, json};

fn sample() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago/project.toml");
    std::fs::read_to_string(path).unwrap()
}

fn edit(path: &[&str], value: Option<Value>) -> SettingEdit {
    SettingEdit {
        path: path.iter().map(|s| (*s).to_owned()).collect(),
        value,
    }
}

#[test]
fn keeps_comments_and_order() {
    let text = sample();
    let after = edit_project_toml(
        &text,
        &[
            edit(&["title"], Some(json!("放課後の灯 改"))),
            edit(&["dialogue", "brackets", "『』"], Some(json!("whisper"))),
            edit(&["editor", "indent_narration"], Some(json!(false))),
            edit(&["message_window", "chars_per_line"], Some(json!(30))),
        ],
    )
    .unwrap();
    // 直した行のほかは変えない（コメントも、行末のコメントも残す）。
    let changed: Vec<(&str, &str)> = text
        .lines()
        .zip(after.lines())
        .filter(|(a, b)| a != b)
        .collect();
    assert_eq!(
        changed,
        [
            ("title = \"放課後の灯\"", "title = \"放課後の灯 改\""),
            ("chars_per_line = 24", "chars_per_line = 30"),
            (
                "\"『』\" = \"thought\"   # 心の声",
                "\"『』\" = \"whisper\"   # 心の声"
            ),
            (
                "indent_narration = true        # 地の文の段落頭に全角空白を入れる",
                "indent_narration = false        # 地の文の段落頭に全角空白を入れる"
            ),
        ]
    );
    assert_eq!(text.lines().count(), after.lines().count());
    let (settings, diagnostics) = read_settings(&after);
    assert_eq!(settings.title.as_deref(), Some("放課後の灯 改"));
    assert!(!settings.editor.indent_narration);
    assert_eq!(settings.message_window.unwrap().chars_per_line, 30);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn adds_and_removes_tables() {
    let text = "# 作品\nformat_version = 0\ntitle = \"a\"\n";
    // 無い表は、両方のキーをそろえて足す。
    let after = edit_project_toml(
        text,
        &[
            edit(&["screen", "width"], Some(json!(1920))),
            edit(&["screen", "height"], Some(json!(1080))),
            edit(&["start_scene"], Some(json!("s1"))),
            edit(&["dialogue", "brackets", "「」"], Some(json!("normal"))),
            edit(&["dialogue", "brackets", "（）"], Some(json!("thought"))),
        ],
    )
    .unwrap();
    assert!(after.starts_with("# 作品\nformat_version = 0\ntitle = \"a\"\n"));
    let (settings, diagnostics) = read_settings(&after);
    assert!(diagnostics.is_empty(), "{diagnostics:?}\n{after}");
    let screen = settings.screen.unwrap();
    assert_eq!((screen.width, screen.height), (1920, 1080));
    assert_eq!(settings.start_scene.as_deref(), Some("s1"));
    assert_eq!(settings.brackets.len(), 2);
    // `[dialogue]` の見出しは書かない。
    assert!(after.contains("[dialogue.brackets]") && !after.contains("[dialogue]\n"));

    // 表ごと消す、キーを消す、括弧の組を消す。
    let after = edit_project_toml(
        &sample(),
        &[
            edit(&["message_window"], None),
            edit(&["start_scene"], None),
            edit(&["dialogue", "brackets", "『』"], None),
        ],
    )
    .unwrap();
    let (settings, diagnostics) = read_settings(&after);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(settings.message_window.is_none() && settings.start_scene.is_none());
    assert_eq!(settings.brackets.len(), 1);
    assert!(!after.contains("[message_window]") && !after.contains("『』"));
    // ほかのコメントは残る。
    assert!(after.contains("# 台詞に使える括弧と、表示スタイル"));
}

#[test]
fn refuses_bad_values() {
    let text = sample();
    let refused = |edits: &[SettingEdit]| edit_project_toml(&text, edits).unwrap_err();
    // 形の合わない値。
    assert!(refused(&[edit(&["screen", "width"], Some(json!(0)))]).contains("正の整数"));
    assert!(refused(&[edit(&["screen", "width"], Some(json!("1280")))]).contains("正の整数"));
    assert!(refused(&[edit(&["title"], Some(json!("  ")))]).contains("空でない"));
    assert!(
        refused(&[edit(&["editor", "indent_narration"], Some(json!(1)))]).contains("true か false")
    );
    assert!(
        refused(&[edit(
            &["dialogue", "brackets", "「」」"],
            Some(json!("normal"))
        )])
        .contains("2文字")
    );
    // 直せないキー。
    assert!(refused(&[edit(&["format_version"], Some(json!(1)))]).contains("直せません"));
    assert!(refused(&[edit(&["ai", "local", "model"], Some(json!("x")))]).contains("直せません"));
    // 最後の括弧の組は消さない。
    let one = "format_version = 0\n[dialogue.brackets]\n\"「」\" = \"normal\"\n";
    assert!(
        edit_project_toml(one, &[edit(&["dialogue", "brackets", "「」"], None)])
            .unwrap_err()
            .contains("1組は残します")
    );
    // 読み直して新しい診断が出るもの：片方だけの大きさ、ほかの組と重なる括弧、英字でない表示スタイル。
    let bare = "format_version = 0\n";
    assert!(
        edit_project_toml(bare, &[edit(&["screen", "width"], Some(json!(800)))])
            .unwrap_err()
            .contains("E802")
    );
    assert!(
        refused(&[edit(
            &["dialogue", "brackets", "「』"],
            Some(json!("normal"))
        )])
        .contains("E802")
    );
    assert!(
        refused(&[edit(
            &["dialogue", "brackets", "『』"],
            Some(json!("心の声"))
        )])
        .contains("E802")
    );
    // 表でないところへは書かない（パニックしない）。
    let flat = "format_version = 0\nscreen = 5\n";
    assert!(
        edit_project_toml(flat, &[edit(&["screen", "width"], Some(json!(800)))])
            .unwrap_err()
            .contains("表ではない")
    );
    // TOML として読めない。
    assert!(
        edit_project_toml("title = \n", &[edit(&["title"], Some(json!("a")))])
            .unwrap_err()
            .contains("TOML として読めない")
    );
}

#[test]
fn keeps_earlier_problems() {
    // 前からある誤り（ここで直さないキーの E802）があっても、ほかのキーは直せる。
    let text = "format_version = 0\ntitle = 3\n[editor]\nindent_narration = true\n";
    let after = edit_project_toml(
        text,
        &[edit(&["editor", "indent_narration"], Some(json!(false)))],
    )
    .unwrap();
    assert!(after.contains("title = 3") && after.contains("indent_narration = false"));
}

#[test]
fn keeps_bom_and_crlf() {
    // 先頭の BOM と、改行の \r\n は残す（Windows のエディタで書いたファイル）。
    let text = "\u{feff}format_version = 0\r\ntitle = \"a\" # 名前\r\n";
    let after = edit_project_toml(text, &[edit(&["title"], Some(json!("b")))]).unwrap();
    assert_eq!(
        after,
        "\u{feff}format_version = 0\r\ntitle = \"b\" # 名前\r\n"
    );
}

#[test]
fn adding_a_bracket_keeps_the_default() {
    // 表が無ければ既定の「」が効いているので、ほかの組を足しても「」は残す。
    let after = edit_project_toml(
        "format_version = 0\n",
        &[edit(
            &["dialogue", "brackets", "『』"],
            Some(json!("thought")),
        )],
    )
    .unwrap();
    let (settings, diagnostics) = read_settings(&after);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let pairs: Vec<String> = settings
        .brackets
        .iter()
        .map(|b| format!("{}{}={}", b.open, b.close, b.style))
        .collect();
    assert_eq!(pairs, ["「」=normal", "『』=thought"]);
    // 空の表（既定の「」が効いている）に足しても同じ。
    let after = edit_project_toml(
        "format_version = 0\n[dialogue.brackets]\n",
        &[edit(
            &["dialogue", "brackets", "『』"],
            Some(json!("thought")),
        )],
    )
    .unwrap();
    assert_eq!(read_settings(&after).0.brackets.len(), 2, "{after}");
    // 表があっても読める組が無い（既定の「」が効いている）なら、足すと「」の読み方が変わるので書かない。
    let broken = "format_version = 0\n[dialogue.brackets]\n\"「」」\" = \"normal\"\n";
    assert!(
        edit_project_toml(
            broken,
            &[edit(
                &["dialogue", "brackets", "『』"],
                Some(json!("thought"))
            )]
        )
        .unwrap_err()
        .contains("dialogue.brackets")
    );
}

#[test]
fn removing_drops_the_comment_above() {
    // 消したキーや表の直上のコメントは、一緒に消える（その項目を説明するコメントなので。R223）。ほかは残す。
    let text = "format_version = 0\n# 開始シーン\nstart_scene = \"s1\"\n\n# 画面\n[screen]\nwidth = 800\nheight = 600\n\n# 入力補助\n[editor]\nindent_narration = true\n";
    let after = edit_project_toml(
        text,
        &[edit(&["screen"], None), edit(&["start_scene"], None)],
    )
    .unwrap();
    assert_eq!(
        after,
        "format_version = 0\n\n# 入力補助\n[editor]\nindent_narration = true\n"
    );
}
