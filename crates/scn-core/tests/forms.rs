//! フォームでの編集（T2-10、D061）：変数の宣言と、設定DBの型定義。完了条件：書き換えで、ほかの行が変わらない。

use std::fs;
use std::path::{Path, PathBuf};

use scn_core::workspace::{EntryEdit, Workspace};
use serde_json::json;

fn sample(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples")
        .join(name)
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn copy_sample(name: &str, tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scn-core-forms-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    copy_dir(&sample(name), &dir);
    dir
}

/// 2つの中身の、違う行（前だけにある行、後だけにある行）。前と後の頭と終わりの同じ行を除いた残り。
fn changed_lines(before: &str, after: &str) -> (Vec<String>, Vec<String>) {
    let a: Vec<&str> = before.lines().collect();
    let b: Vec<&str> = after.lines().collect();
    let head = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let tail = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count()
        .min(a.len() - head)
        .min(b.len() - head);
    let own = |v: &[&str]| v.iter().map(|l| (*l).to_owned()).collect::<Vec<_>>();
    (own(&a[head..a.len() - tail]), own(&b[head..b.len() - tail]))
}

#[test]
fn variable_fields() {
    let root = copy_sample("houkago", "vars");
    let path = root.join("variables.yaml");
    let before = fs::read_to_string(&path).unwrap();
    let mut ws = Workspace::open(&root).unwrap();
    let edit = ws
        .set_variable_field("akari.好感度", "default", &json!(2), &before)
        .unwrap();
    let EntryEdit::Saved { text } = edit else {
        panic!("{edit:?}")
    };
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
    let (gone, added) = changed_lines(&before, &text);
    assert_eq!(gone, ["  default: 0"]);
    assert_eq!(added, ["  default: 2"]);
    // 宣言が変わった（初めの値）。
    let decl = ws
        .project()
        .variables
        .iter()
        .find(|v| v.key == "akari.好感度")
        .unwrap();
    assert_eq!(decl.default.as_deref(), Some("2"));
    // 外で同じ項目が変わっていたら書かない。ほかの項目なら今の中身に重ねる。
    let outside = text.replace("  default: 2", "  default: 5");
    fs::write(&path, &outside).unwrap();
    let edit = ws
        .set_variable_field("akari.好感度", "default", &json!(3), &text)
        .unwrap();
    assert!(matches!(edit, EntryEdit::Conflict { .. }), "{edit:?}");
    let edit = ws
        .set_variable_field("akari.好感度", "desc", &json!("灯の気持ち"), &text)
        .unwrap();
    let EntryEdit::Saved { text } = edit else {
        panic!("{edit:?}")
    };
    assert!(text.contains("  default: 5") && text.contains("  desc: 灯の気持ち"));
    // 宣言を足す。
    let before = text;
    let edit = ws.add_variable("tsumugi.信頼", "int", &[]).unwrap();
    let EntryEdit::Saved { text } = edit else {
        panic!("{edit:?}")
    };
    assert!(text.starts_with(&before));
    assert!(
        ws.project()
            .variables
            .iter()
            .any(|v| v.key == "tsumugi.信頼")
    );
    assert!(ws.add_variable("tsumugi.信頼", "int", &[]).is_err());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn type_fields() {
    use scn_core::typedef::TypeEdit;
    let root = copy_sample("houkago", "types");
    let path = root.join("bible/types/character.yaml");
    let before = fs::read_to_string(&path).unwrap();
    let mut ws = Workspace::open(&root).unwrap();
    assert_eq!(ws.type_text("character").unwrap(), before);
    // 名前を直す：その項目の行だけが変わる。
    let edit = ws
        .edit_type(
            "character",
            &TypeEdit::Set {
                key: "want".to_owned(),
                label: "望み".to_owned(),
                ty: "text".to_owned(),
                options: Vec::new(),
            },
            &before,
        )
        .unwrap();
    let EntryEdit::Saved { text } = edit else {
        panic!("{edit:?}")
    };
    let (gone, added) = changed_lines(&before, &text);
    assert_eq!(gone.len(), 1);
    assert!(gone[0].contains("key: want"));
    assert_eq!(added, ["  - { key: want, label: 望み, type: text }"]);
    let ty = ws
        .project()
        .bible
        .types
        .iter()
        .find(|t| t.id == "character")
        .unwrap();
    assert_eq!(ty.fields[3].label, "望み");
    // 並べ替えと、項目を足す。
    let edit = ws
        .edit_type(
            "character",
            &TypeEdit::Move {
                key: "want".to_owned(),
                delta: 1,
            },
            &text,
        )
        .unwrap();
    let EntryEdit::Saved { text } = edit else {
        panic!("{edit:?}")
    };
    let ty = ws
        .project()
        .bible
        .types
        .iter()
        .find(|t| t.id == "character")
        .unwrap();
    assert_eq!(ty.fields[4].key, "want");
    let edit = ws
        .edit_type(
            "character",
            &TypeEdit::Add {
                key: "birthday".to_owned(),
                label: "誕生日".to_owned(),
                ty: "date".to_owned(),
                options: Vec::new(),
            },
            &text,
        )
        .unwrap();
    assert!(matches!(edit, EntryEdit::Saved { .. }), "{edit:?}");
    // 外で変わっていれば書かない。
    let edit = ws
        .edit_type(
            "character",
            &TypeEdit::Remove {
                key: "aliases".to_owned(),
            },
            &before,
        )
        .unwrap();
    assert!(matches!(edit, EntryEdit::Conflict { .. }), "{edit:?}");
    // 型を作って、項目を足す。
    let file = ws.create_type("place", "場所").unwrap();
    assert_eq!(file, "bible/types/place.yaml");
    let text = ws.type_text("place").unwrap();
    let edit = ws
        .edit_type(
            "place",
            &TypeEdit::Add {
                key: "kind".to_owned(),
                label: "種類".to_owned(),
                ty: "select".to_owned(),
                options: vec!["屋内".to_owned(), "屋外".to_owned()],
            },
            &text,
        )
        .unwrap();
    assert!(matches!(edit, EntryEdit::Saved { .. }), "{edit:?}");
    let place = ws
        .project()
        .bible
        .types
        .iter()
        .find(|t| t.id == "place")
        .unwrap();
    assert_eq!(place.fields.len(), 1);
    assert!(ws.create_type("place", "場所").is_err());
    assert!(ws.create_type("a/b", "x").is_err());
    let _ = fs::remove_dir_all(&root);
}

/// BOM（U+FEFF）で始まる設定ファイル（Windows のエディタが付けることがある。T2-10、D061）：読み込みで E801 に
/// せず、宣言と型と素材を読む。フォームで書き換えても BOM を残し、ほかの行は変えない。
#[test]
fn settings_with_bom() {
    let root = std::env::temp_dir().join(format!("scn-core-forms-bom-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    for dir in ["story", "assets", "bible/types"] {
        fs::create_dir_all(root.join(dir)).unwrap();
    }
    let files = [
        (
            "project.toml",
            "\u{feff}format_version = 0\nstart_scene = \"s1\"\n",
        ),
        (
            "variables.yaml",
            "\u{feff}flag.a:\n  type: int\n  default: 0\nflag.b:\n  type: bool\n",
        ),
        (
            "assets/catalog.yaml",
            "\u{feff}backgrounds:\n  屋上: bg/a.png\nbgm:\n  夕方: bgm/b.ogg\n",
        ),
        (
            "bible/types/place.yaml",
            "\u{feff}name: 場所\nfields:\n  - { key: a, label: A, type: text }\n",
        ),
        (
            "story/a.scn",
            "## 一 #s1\n@背景 屋上\n@BGM 夕方\n{flag.a = 1} ^a001\n@エンディング end ^a002\n",
        ),
    ];
    for (path, text) in files {
        fs::write(root.join(path), text).unwrap();
    }
    let mut ws = Workspace::open(&root).unwrap();
    let ir = scn_core::analyze(ws.project());
    let codes: Vec<&str> = ir.diagnostics.iter().map(|d| d.code).collect();
    assert!(!codes.contains(&"E801"), "{codes:?}");
    assert!(
        !codes.contains(&"W602") && !codes.contains(&"I403"),
        "{codes:?}"
    );
    let place = ws
        .project()
        .bible
        .types
        .iter()
        .find(|t| t.id == "place")
        .unwrap();
    assert_eq!(place.name, "場所");
    // 最初の宣言の項目を直しても、BOM とほかの行は変わらない。
    let before = fs::read_to_string(root.join("variables.yaml")).unwrap();
    let edit = ws
        .set_variable_field("flag.a", "default", &json!(3), &before)
        .unwrap();
    assert!(matches!(edit, EntryEdit::Saved { .. }), "{edit:?}");
    let after = fs::read_to_string(root.join("variables.yaml")).unwrap();
    assert_eq!(after, before.replace("  default: 0\n", "  default: 3\n"));
    let edit = ws.add_variable("flag.c", "int", &[]).unwrap();
    assert!(matches!(edit, EntryEdit::Saved { .. }), "{edit:?}");
    assert!(
        fs::read_to_string(root.join("variables.yaml"))
            .unwrap()
            .starts_with("\u{feff}flag.a:\n")
    );
    let text = ws.type_text("place").unwrap();
    let edit = ws
        .edit_type(
            "place",
            &scn_core::typedef::TypeEdit::Set {
                key: "a".to_owned(),
                label: "エー".to_owned(),
                ty: "text".to_owned(),
                options: Vec::new(),
            },
            &text,
        )
        .unwrap();
    assert!(matches!(edit, EntryEdit::Saved { .. }), "{edit:?}");
    assert_eq!(
        fs::read_to_string(root.join("bible/types/place.yaml")).unwrap(),
        "\u{feff}name: 場所\nfields:\n  - { key: a, label: エー, type: text }\n"
    );
    let catalog = fs::read_to_string(root.join("assets/catalog.yaml")).unwrap();
    let next = scn_core::assets::add_entry(&catalog, "background", None, "教室").unwrap();
    assert_eq!(
        next,
        catalog.replace("bg/a.png\n", "bg/a.png\n  教室: {}\n")
    );
    let _ = fs::remove_dir_all(&root);
}
