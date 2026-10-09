//! フォームでの編集（T2-10、D061）：変数の宣言と、設定DBの型定義。完了条件：書き換えで、ほかの行が変わらない。

use std::fs;
use std::path::{Path, PathBuf};

use scn_core::scaffold::type_template;
use scn_core::typedef::TypeSource;
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
    let file = ws.create_type("place", "場所", &TypeSource::Empty).unwrap();
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
    assert!(ws.create_type("place", "場所", &TypeSource::Empty).is_err());
    assert!(ws.create_type("a/b", "x", &TypeSource::Empty).is_err());
    let _ = fs::remove_dir_all(&root);
}

/// 完了条件（T4-9）：型のテンプレートと今ある型から、項目ごと写した新しい型を作れる。名前を替え、`builtin` は
/// 写さない。使われている型IDと、無いテンプレート・型は断る。
#[test]
fn types_from_templates_and_copies() {
    let root = copy_sample("houkago", "type-sources");
    let mut ws = Workspace::open(&root).unwrap();
    let field_keys = |ws: &Workspace, id: &str| -> Vec<String> {
        ws.project()
            .bible
            .types
            .iter()
            .find(|t| t.id == id)
            .unwrap()
            .fields
            .iter()
            .map(|f| f.key.clone())
            .collect()
    };
    // テンプレートから：テンプレートの中身のまま（名前だけ替える）。
    let file = ws
        .create_type(
            "dungeon",
            "ダンジョン",
            &TypeSource::Template {
                id: "place".to_owned(),
            },
        )
        .unwrap();
    assert_eq!(file, "bible/types/dungeon.yaml");
    let template = type_template("place").unwrap().text;
    assert_eq!(
        fs::read_to_string(root.join(&file)).unwrap(),
        template.replacen("name: 場所\n", "name: ダンジョン\n", 1)
    );
    assert_eq!(field_keys(&ws, "dungeon")[..2], ["aliases", "kind"]);
    // キャラクターのテンプレートから作っても、組み込みの型（話者）にはならない。
    ws.create_type(
        "npc",
        "脇役",
        &TypeSource::Template {
            id: "character".to_owned(),
        },
    )
    .unwrap();
    let npc = fs::read_to_string(root.join("bible/types/npc.yaml")).unwrap();
    assert!(npc.starts_with("name: 脇役\nfields:\n"), "{npc}");
    assert!(!npc.contains("builtin"));
    assert_eq!(ws.project().bible.character_type(), "character");
    assert!(field_keys(&ws, "npc").contains(&"role".to_owned()));
    // 今ある型の複製：項目もコメントも写し、builtin を除く。元のファイルは変えない。
    let character = fs::read_to_string(root.join("bible/types/character.yaml")).unwrap();
    ws.create_type(
        "rival",
        "ライバル: 2人目",
        &TypeSource::Type {
            id: "character".to_owned(),
        },
    )
    .unwrap();
    let rival = fs::read_to_string(root.join("bible/types/rival.yaml")).unwrap();
    assert!(rival.starts_with("name: \"ライバル: 2人目\"\n"), "{rival}");
    assert!(!rival.contains("builtin"));
    assert_eq!(field_keys(&ws, "rival"), field_keys(&ws, "character"));
    assert_eq!(
        fs::read_to_string(root.join("bible/types/character.yaml")).unwrap(),
        character
    );
    assert!(
        ws.project().load_diagnostics.is_empty(),
        "{:?}",
        ws.project().load_diagnostics
    );
    // 断る：使われている型ID、無いテンプレート、無い型、写せない書き方（名前をブロックで書いた型）。
    let place = TypeSource::Template {
        id: "place".to_owned(),
    };
    assert!(ws.create_type("dungeon", "x", &place).is_err());
    assert!(ws.create_type("character", "x", &place).is_err());
    let missing = TypeSource::Template {
        id: "nothing".to_owned(),
    };
    assert!(ws.create_type("a1", "x", &missing).is_err());
    let missing = TypeSource::Type {
        id: "nothing".to_owned(),
    };
    assert!(ws.create_type("a2", "x", &missing).is_err());
    fs::write(
        root.join("bible/types/odd.yaml"),
        "name: |\n  変わった名前\nfields:\n  - { key: a, label: A, type: text }\n",
    )
    .unwrap();
    let error = ws
        .create_type(
            "a3",
            "x",
            &TypeSource::Type {
                id: "odd".to_owned(),
            },
        )
        .unwrap_err();
    assert!(error.to_string().contains("ソース表示"), "{error}");
    assert!(!root.join("bible/types/a3.yaml").exists());
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
