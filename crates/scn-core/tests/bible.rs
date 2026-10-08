//! 設定DBの画面のための処理（`bible`、T1-10、D049）：項目の型、値の検査、フロントマターの書き換え、
//! 逆引き、変数の一覧。
//!
//! 完了条件「フォームで値を直しても、直した項目以外（本文と、ほかの項目）が変わらない」は、
//! `editing_changes_only_that_key` と `random_values_round_trip` で確かめる。

use std::fs;
use std::path::{Path, PathBuf};

use proptest::prelude::*;
use scn_core::bible::{
    EditError, FieldType, body_of, check_value, entry_usage, front_fields, set_body, set_field,
    variable_rows,
};
use scn_core::index::build_index;
use scn_core::{analyze, load_project};
use serde_json::{Value, json};

fn sample() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago")
}

/// サンプルのエントリ（パスと中身）。
fn sample_entries() -> Vec<(String, String)> {
    let root = sample().join("bible/entries");
    let mut out = Vec::new();
    for dir in fs::read_dir(&root).unwrap() {
        let dir = dir.unwrap().path();
        for file in fs::read_dir(&dir).unwrap() {
            let file = file.unwrap().path();
            let rel = file.strip_prefix(sample()).unwrap().display().to_string();
            out.push((rel, fs::read_to_string(&file).unwrap()));
        }
    }
    out.sort();
    out
}

#[test]
fn field_types() {
    for text in [
        "text",
        "longtext",
        "number",
        "bool",
        "date",
        "select",
        "multiselect",
        "asset",
        "ref<organization>",
        "ref<scene>",
        "list<text>",
        "list<ref<scene>>",
    ] {
        let ty = FieldType::parse(text).unwrap_or_else(|| panic!("{text}"));
        assert_eq!(ty.to_string(), text);
    }
    assert_eq!(
        FieldType::parse(" list< ref<character> > ").map(|t| t.to_string()),
        Some("list<ref<character>>".to_owned())
    );
    for bad in ["txt", "ref<>", "list<>", "ref<a b>", "list<text", ""] {
        assert_eq!(FieldType::parse(bad), None, "{bad}");
    }
    assert_eq!(
        FieldType::parse("list<ref<scene>>")
            .as_ref()
            .and_then(FieldType::ref_target),
        Some("scene")
    );
}

#[test]
fn value_checks() {
    let ty = |t: &str| FieldType::parse(t).unwrap();
    let options = vec!["学校".to_owned(), "部活".to_owned()];
    // 書いていない値は合う。
    for t in ["number", "bool", "date", "list<text>", "ref<scene>"] {
        assert_eq!(check_value(&ty(t), &[], &Value::Null), None, "{t}");
        assert_eq!(check_value(&ty(t), &[], &json!("")), None, "{t}");
    }
    assert_eq!(check_value(&ty("text"), &[], &json!(12)), None);
    assert!(check_value(&ty("text"), &[], &json!([1])).is_some());
    assert_eq!(check_value(&ty("number"), &[], &json!(1.5)), None);
    assert!(check_value(&ty("number"), &[], &json!("1")).is_some());
    assert!(check_value(&ty("bool"), &[], &json!("true")).is_some());
    assert_eq!(check_value(&ty("date"), &[], &json!("2024-02-29")), None);
    assert!(check_value(&ty("date"), &[], &json!("2023-02-29")).is_some());
    assert!(check_value(&ty("date"), &[], &json!("2024-1-01")).is_some());
    assert_eq!(check_value(&ty("select"), &options, &json!("学校")), None);
    assert!(check_value(&ty("select"), &options, &json!("店")).is_some());
    assert_eq!(
        check_value(&ty("multiselect"), &options, &json!(["学校", "部活"])),
        None
    );
    assert!(check_value(&ty("multiselect"), &options, &json!("学校")).is_some());
    assert_eq!(check_value(&ty("ref<scene>"), &[], &json!("s001")), None);
    assert!(check_value(&ty("ref<scene>"), &[], &json!(["s001"])).is_some());
    assert_eq!(
        check_value(&ty("list<ref<scene>>"), &[], &json!(["s001", "s002"])),
        None
    );
    assert_eq!(
        check_value(&ty("list<number>"), &[], &json!([1, "x"])).as_deref(),
        Some("2番目の値は、数で書きます")
    );
    assert!(check_value(&ty("list<text>"), &[], &json!("あかり")).is_some());
}

/// テキストの差分が、`key` の行（とその続きの行）の中に収まっている。
fn assert_only_key_lines_changed(before: &str, after: &str, key: &str) {
    if before == after {
        return;
    }
    let prefix = before
        .bytes()
        .zip(after.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    let suffix = before
        .bytes()
        .rev()
        .zip(after.bytes().rev())
        .take_while(|(a, b)| a == b)
        .count()
        .min(before.len() - prefix)
        .min(after.len() - prefix);
    // 前の変わらない部分は、キーの行の始まりまで（足したときは閉じの `---` まで）を含む。
    let key_line = before
        .find(&format!("\n{key}:"))
        .map(|at| at + 1)
        .or_else(|| before.rfind("\n---").map(|at| at + 1))
        .unwrap();
    assert!(
        prefix >= key_line,
        "{key}: キーの行より前が変わった\n{after}"
    );
    // 後ろの変わらない部分は、キーの行の続きの後ろからを含む。
    let rest = &before[key_line..];
    let mut region_end = key_line + rest.find('\n').map_or(rest.len(), |at| at + 1);
    for line in before[region_end..].split_inclusive('\n') {
        if line.starts_with([' ', '\t']) || line.starts_with("- ") {
            region_end += line.len();
        } else {
            break;
        }
    }
    assert!(
        before.len() - suffix <= region_end.max(key_line),
        "{key}: キーの行より後ろが変わった\n{after}"
    );
}

/// サンプルのどのエントリのどのキーを直しても、そのキーの値だけが変わり、ほかの項目と本文と、
/// ほかの行のバイトは変わらない。無いキーは閉じの `---` の直前に足す。
#[test]
fn editing_changes_only_that_key() {
    let values = [
        json!("ぼく"),
        json!("yes"),
        json!("12"),
        json!("a: b # c"),
        json!(" 前に空白"),
        json!("一行目\n二行目"),
        json!("一行目\n\n三行目\n"),
        json!(["あかり", "a, b", "true"]),
        json!([]),
        json!(3),
        json!(false),
        Value::Null,
        json!(""),
    ];
    for (path, text) in sample_entries() {
        let before = front_fields(&text).unwrap();
        let mut keys: Vec<String> = before.keys().filter(|k| *k != "id").cloned().collect();
        keys.push("new_key".to_owned());
        for key in &keys {
            for value in &values {
                let after = set_field(&text, key, value)
                    .unwrap_or_else(|e| panic!("{path} {key} {value}: {e}"));
                assert_eq!(body_of(&after), body_of(&text), "{path} {key}");
                let mut expected = before.clone();
                expected.insert(key.clone(), value.clone());
                assert_eq!(front_fields(&after).unwrap(), expected, "{path} {key}");
                assert_only_key_lines_changed(&text, &after, key);
            }
        }
    }
}

#[test]
fn comments_order_and_style_are_kept() {
    let text = "---\n# 人物の設定\nid: akari\nname: 灯   # 表示名\naliases: [あかり, 灯ちゃん]\n\nspeech: |\n  一行目\n  二行目\n\nvoice: \"\"\n---\n本文。\n";
    let after = set_field(text, "name", &json!("あかり")).unwrap();
    assert_eq!(
        after,
        "---\n# 人物の設定\nid: akari\nname: あかり   # 表示名\naliases: [あかり, 灯ちゃん]\n\nspeech: |\n  一行目\n  二行目\n\nvoice: \"\"\n---\n本文。\n"
    );
    // 続きの行のある値（ブロック）を1行にしても、後ろの空行と項目は残る。
    let after = set_field(text, "speech", &json!("短く")).unwrap();
    assert!(after.contains("speech: 短く\n\nvoice: \"\"\n"), "{after}");
    // 改行を含む値は | のブロックで書く。
    let after = set_field(text, "voice", &json!("一\n二")).unwrap();
    assert!(after.contains("voice: |-\n  一\n  二\n---\n"), "{after}");
    // リストは flow 形式。
    let after = set_field(text, "aliases", &json!(["あかり"])).unwrap();
    assert!(after.contains("aliases: [あかり]\n"), "{after}");
    // 空にした値。
    let after = set_field(text, "name", &Value::Null).unwrap();
    assert!(after.contains("name:   # 表示名\n"), "{after}");
}

/// 直したキーの近くのコメントも残す。値の途中の引用符でコメントを見落とさない。値の途中にコメントがあって
/// 残せないときは、書かずに知らせる。
#[test]
fn comments_near_the_key_are_kept() {
    let after = set_field(
        "---\nname: It's me   # 表示名\n---\n",
        "name",
        &json!("You"),
    )
    .unwrap();
    assert_eq!(after, "---\nname: You   # 表示名\n---\n");
    let after = set_field("---\nheight: 5\" tall # メモ\n---\n", "height", &json!("6")).unwrap();
    assert_eq!(after, "---\nheight: \"6\" # メモ\n---\n");
    let after = set_field("---\nname: 'a # b' # c\n---\n", "name", &json!("x")).unwrap();
    assert_eq!(after, "---\nname: x # c\n---\n");
    // 素の文字列の途中の引用符（空白の後ろでも）は、引用の始まりではない。
    let after = set_field("---\nname: 灯 'x # メモ\n---\n", "name", &json!("x")).unwrap();
    assert_eq!(after, "---\nname: x # メモ\n---\n");
    let after = set_field("---\nname: a \"b # c\n---\n", "name", &json!("x")).unwrap();
    assert_eq!(after, "---\nname: x # c\n---\n");
    let after = set_field(
        "---\ntags: [\"a # b\", c] # d\n---\n",
        "tags",
        &json!(["x"]),
    )
    .unwrap();
    assert_eq!(after, "---\ntags: [x] # d\n---\n");
    // 値の後ろの、字下げしたコメントの行は残す。
    let text = "---\nname: 灯\n  # 名前のメモ\naliases: [あかり]\n---\n";
    assert_eq!(
        set_field(text, "name", &json!("あかり")).unwrap(),
        "---\nname: あかり\n  # 名前のメモ\naliases: [あかり]\n---\n"
    );
    // リストの項目の行末のコメントや、項目の間のコメントは、差し替えると消えるので書かない。
    for text in [
        "---\nmembers:\n  - akari   # 幼なじみ\n  - minato\n---\n",
        "---\nmembers:\n  - akari\n  # - 灯（没）\n  - minato\n---\n",
    ] {
        assert_eq!(
            set_field(text, "members", &json!(["akari"])),
            Err(EditError::Unsupported),
            "{text}"
        );
        // ほかのキーは直せる。
        assert!(set_field(text, "name", &json!("x")).is_ok());
    }
    // `|` のブロックの中の `#` は中身なので、直せる。
    let text = "---\nspeech: |\n  # 見出し\n  本文\nvoice: \"\"\n---\n";
    assert_eq!(
        set_field(text, "speech", &json!("短く")).unwrap(),
        "---\nspeech: 短く\nvoice: \"\"\n---\n"
    );
}

/// コメント・ブロックのリスト・ブロックの文字列を含むエントリ。
const COMMENTED: &str = "\u{feff}---\r\n# 人物の設定\r\nid: akari   # ID\r\nname: 灯 # 表示名\r\naliases:\r\n  - あかり\r\n  - 灯ちゃん\r\n# 口調\r\nspeech: |\r\n  明るい\r\n  # 見出しではない\r\n\r\nvoice: '' # 未定\r\n---\r\n本文 # です\r\n";

#[test]
fn editing_commented_entry_keeps_other_lines() {
    let before = front_fields(COMMENTED).unwrap();
    for key in ["name", "aliases", "speech", "voice", "new_key"] {
        for value in [
            json!("ぼく"),
            json!("一\n二"),
            json!(["a", "b"]),
            Value::Null,
        ] {
            let after = set_field(COMMENTED, key, &value).unwrap();
            assert_eq!(body_of(&after), body_of(COMMENTED));
            let mut expected = before.clone();
            expected.insert(key.to_owned(), value.clone());
            assert_eq!(front_fields(&after).unwrap(), expected, "{key}");
            assert_only_key_lines_changed(COMMENTED, &after, key);
            for comment in ["# 人物の設定", "# ID", "# 口調", "本文 # です"] {
                assert!(after.contains(comment), "{key} {value}: {comment}\n{after}");
            }
            if key != "name" {
                assert!(after.contains("# 表示名"));
            }
            if key != "voice" {
                assert!(after.contains("# 未定"));
            }
            // 改行はファイルのまま（CRLF）。
            assert!(
                !after.replace("\r\n", "").contains('\n'),
                "{key}\n{after:?}"
            );
        }
    }
}

#[test]
fn missing_key_and_missing_front_matter() {
    let text = "---\nid: hairpin\n---\n本文\n";
    assert_eq!(
        set_field(text, "payoff", &json!("s040")).unwrap(),
        "---\nid: hairpin\npayoff: s040\n---\n本文\n"
    );
    assert_eq!(
        set_field("本文だけ\n", "name", &json!("髪留め")).unwrap(),
        "---\nname: 髪留め\n---\n本文だけ\n"
    );
    assert_eq!(
        set_field("", "name", &json!("x")).unwrap(),
        "---\nname: x\n---\n"
    );
}

#[test]
fn crlf_and_bom_are_kept() {
    let text = "\u{feff}---\r\nid: a\r\nname: 灯\r\n---\r\n本文\r\n";
    let after = set_field(text, "name", &json!("一\n二")).unwrap();
    assert_eq!(
        after,
        "\u{feff}---\r\nid: a\r\nname: |-\r\n  一\r\n  二\r\n---\r\n本文\r\n"
    );
    assert_eq!(body_of(&after), "本文\r\n");
    let after = set_body(text, "新しい\n本文\n");
    assert_eq!(
        after,
        "\u{feff}---\r\nid: a\r\nname: 灯\r\n---\r\n新しい\r\n本文\r\n"
    );
}

#[test]
fn body_only_replaces_after_front_matter() {
    let text = "---\nid: a # c\n---\n古い本文\n";
    assert_eq!(set_body(text, "新しい"), "---\nid: a # c\n---\n新しい");
    assert_eq!(set_body("\u{feff}古い", "新しい"), "\u{feff}新しい");
}

/// ほかの行を変えずには書き換えられない書き方は、書かずに知らせる。
#[test]
fn unsupported_shapes_are_refused() {
    // ルートが flow 形式の対応表。
    assert_eq!(
        set_field("---\n{a: 1, b: 2}\n---\n", "a", &json!(3)),
        Err(EditError::Unsupported)
    );
    // アンカーをほかの項目が使っている。
    assert_eq!(
        set_field("---\na: &x 1\nb: *x\n---\n", "a", &json!(3)),
        Err(EditError::Unsupported)
    );
    // 同じキーが2つ（読めないか、書き換えられない）。
    assert!(set_field("---\na: 1\na: 2\n---\n", "a", &json!(3)).is_err());
    // 読めないフロントマター。
    assert!(matches!(
        set_field("---\na: [1\n---\n", "a", &json!(3)),
        Err(EditError::Unreadable(_))
    ));
    // id は直さない。
    assert_eq!(
        set_field("---\nid: a\n---\n", "id", &json!("b")),
        Err(EditError::ReadOnly("id".to_owned()))
    );
}

proptest! {
    /// どんな文字列とリストを書いても、読み直すと同じ値になり、ほかの項目と本文は変わらない。
    #[test]
    fn random_values_round_trip(
        text in "[ -~ぁ-んァ-ン一-龠「」…―　\n\t:#'\"\\\\]{0,24}",
        items in proptest::collection::vec("[ -~ぁ-ん　,\\[\\]:#]{0,8}", 0..4),
        key in prop::sample::select(vec!["name", "speech", "org", "new_key"]),
    ) {
        let (_, entry) = sample_entries()
            .into_iter()
            .find(|(p, _)| p.ends_with("akari.md"))
            .unwrap();
        for source in [entry.as_str(), COMMENTED] {
            for value in [json!(text), json!(items)] {
                let after = set_field(source, key, &value)
                    .unwrap_or_else(|e| panic!("{key} {value}: {e}"));
                prop_assert_eq!(body_of(&after), body_of(source));
                let mut expected = front_fields(source).unwrap();
                expected.insert(key.to_owned(), value.clone());
                prop_assert_eq!(front_fields(&after).unwrap(), expected);
                assert_only_key_lines_changed(source, &after, key);
            }
        }
    }
}

/// 逆引き：キャラクターは登場シーン・台詞の数・初出・紐づく変数、どのエントリも参照の両方の向き、
/// キャラクター以外は名前の出る行。
#[test]
fn usage_of_sample_entries() {
    let project = load_project(&sample()).unwrap();
    let ir = analyze(&project);
    let index = build_index(&project, &ir);

    let akari = entry_usage(&project, &ir, &index, "bible/entries/character/akari.md").unwrap();
    assert!(akari.character);
    assert!(akari.lines > 0);
    assert_eq!(
        akari.lines,
        akari.scenes.iter().map(|s| s.count).sum::<usize>()
    );
    assert_eq!(
        akari.first.as_deref(),
        akari.scenes.first().map(|s| s.scene.as_str())
    );
    assert!(akari.refs_out.iter().any(|r| r.field == "org"
        && r.id == "kitakaze_high"
        && r.exists
        && r.name.as_deref() == Some("北風高校")));
    assert!(
        akari
            .refs_in
            .iter()
            .any(|r| r.id == "kitakaze_high" && r.field == "members")
    );
    let keys: Vec<&str> = akari.variables.iter().map(|v| v.key.as_str()).collect();
    assert_eq!(keys, ["akari.好感度", "akari.転校済み"]);
    let affection = &akari.variables[0];
    assert_eq!(affection.default.as_deref(), Some("0"));
    assert_eq!(affection.range, Some((0, 10)));
    assert_eq!(affection.scope.as_deref(), Some("save"));

    let school = entry_usage(
        &project,
        &ir,
        &index,
        "bible/entries/organization/kitakaze_high.md",
    )
    .unwrap();
    assert!(!school.character);
    assert_eq!(school.refs_out.len(), 4);
    assert!(
        school
            .refs_out
            .iter()
            .all(|r| r.exists && r.kind == "entry")
    );
    assert_eq!(
        school.refs_in.iter().filter(|r| r.field == "org").count(),
        project
            .bible
            .entries
            .iter()
            .filter(|e| e.fields.get("org") == Some(&json!("kitakaze_high")))
            .count()
    );

    let hairpin =
        entry_usage(&project, &ir, &index, "bible/entries/foreshadow/hairpin.md").unwrap();
    assert!(
        hairpin
            .refs_out
            .iter()
            .any(|r| r.kind == "scene" && r.id == "s021" && r.exists)
    );
    assert!(
        hairpin
            .refs_out
            .iter()
            .any(|r| r.kind == "scene" && r.id == "s040" && r.exists)
    );
    // 「髪留め」は本文に出る。
    assert!(hairpin.lines >= 1);
    assert!(hairpin.scenes.iter().all(|s| !s.title.is_empty()));

    assert!(entry_usage(&project, &ir, &index, "bible/entries/character/none.md").is_none());
}

#[test]
fn variable_rows_of_sample() {
    let project = load_project(&sample()).unwrap();
    let ir = analyze(&project);
    let index = build_index(&project, &ir);
    let rows = variable_rows(&project, &index);
    let courage = rows.iter().find(|r| r.key == "flag.勇気").unwrap();
    assert!(courage.declared);
    assert_eq!(courage.ty, Some("int"));
    assert_eq!(courage.default.as_deref(), Some("0"));
    assert!(courage.desc.as_deref().is_some_and(|d| d.contains("勇気")));
    assert!(courage.span.is_some());
    assert!(!courage.reads.is_empty() || !courage.writes.is_empty());
    let route = rows.iter().find(|r| r.key == "flag.ルート").unwrap();
    assert_eq!(route.values, ["共通", "灯", "紬"]);
    assert_eq!(route.default.as_deref(), Some("共通"));
    let true_end = rows.iter().find(|r| r.key == "周回.トゥルー解放").unwrap();
    assert_eq!(true_end.scope.as_deref(), Some("persistent"));
}
