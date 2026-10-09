//! 名前の変更（`rename`、T2-8、D059）：シーンID、キャラクターの名前、変数、エントリのID を全体で置き換える。
//! 完了条件：行IDが変わらないこと、置き換え漏れが無いこと。章の名前の変更と削除も。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use scn_core::index::build_index;
use scn_core::ir::Ir;
use scn_core::rename::{RenameTarget, plan_rename, symbol_at};
use scn_core::workspace::Workspace;
use scn_core::{Project, analyze, load_project};

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
    let dir = std::env::temp_dir().join(format!("scn-core-rename-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    copy_dir(&sample(name), &dir);
    dir
}

/// 本文のファイルごとの行ID（並びの順）。
fn line_ids(project: &Project) -> BTreeMap<String, Vec<String>> {
    project
        .files
        .iter()
        .map(|f| {
            let ids = f
                .text
                .lines()
                .filter_map(|l| l.rsplit_once(" ^").map(|(_, id)| id.trim().to_owned()))
                .collect();
            (f.path.clone(), ids)
        })
        .collect()
}

/// 診断のコードと数。
fn codes(ir: &Ir) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for d in &ir.diagnostics {
        *out.entry(d.code.to_owned()).or_default() += 1;
    }
    out
}

fn story_text(project: &Project) -> String {
    project.files.iter().map(|f| f.text.as_str()).collect()
}

fn read(root: &Path, file: &str) -> String {
    fs::read_to_string(root.join(file)).unwrap()
}

/// シーンID：見出し、ジャンプ・選択肢の飛び先・呼び出し、開始シーン、ref<scene> の項目、昇格先、フロー図の位置。
#[test]
fn rename_scene() {
    let root = copy_sample("houkago", "scene");
    fs::write(
        root.join("layout.json"),
        "{\n  \"scene\": { \"s012\": { \"x\": 1, \"y\": 2 } }\n}\n",
    )
    .unwrap();
    let mut ws = Workspace::open(&root).unwrap();
    let before_ids = line_ids(ws.project());
    let before = analyze(ws.project());
    let renamed = ws
        .rename(
            &RenameTarget::Scene {
                id: "s012".to_owned(),
            },
            "s100",
        )
        .unwrap();
    assert_eq!(renamed.story_edits, 2);
    let after = analyze(ws.project());
    assert_eq!(line_ids(ws.project()), before_ids);
    assert_eq!(codes(&after), codes(&before));
    assert!(after.scenes.iter().any(|s| s.id == "s100"));
    // 置き換え漏れが無い。
    assert!(!story_text(ws.project()).contains("s012"));
    assert_eq!(after.edges.len(), before.edges.len());
    assert!(read(&root, "ideas/rooftop-confession.md").contains("promoted_to: scene/s100"));
    assert!(read(&root, "layout.json").contains("\"s100\""));
    assert!(!read(&root, "layout.json").contains("\"s012\""));
    // ディスクにも書いた。
    for f in &ws.project().files {
        assert_eq!(read(&root, &f.path), f.text);
    }
    // 開始シーンと、伏線の回収（ref<scene>）。
    ws.rename(
        &RenameTarget::Scene {
            id: "s010".to_owned(),
        },
        "start",
    )
    .unwrap();
    assert!(read(&root, "project.toml").contains("start_scene = \"start\""));
    ws.rename(
        &RenameTarget::Scene {
            id: "s040".to_owned(),
        },
        "s040b",
    )
    .unwrap();
    assert!(read(&root, "bible/entries/foreshadow/hairpin.md").contains("payoff: s040b"));
    let after = analyze(ws.project());
    assert_eq!(codes(&after), codes(&before));
    let _ = fs::remove_dir_all(&root);
}

/// キャラクターの名前：名前で書いた話者・立ち絵・変数の名前空間だけを変え、地の文の同じ文字は変えない。
#[test]
fn rename_character_name() {
    let root = copy_sample("houkago", "name");
    let mut ws = Workspace::open(&root).unwrap();
    let before_ids = line_ids(ws.project());
    let before = analyze(ws.project());
    let speakers_before = before
        .scenes
        .iter()
        .flat_map(|s| &s.ops)
        .filter(|op| format!("{op:?}").contains("akari"))
        .count();
    ws.rename(
        &RenameTarget::CharacterName {
            id: "akari".to_owned(),
        },
        "明里",
    )
    .unwrap();
    let after = analyze(ws.project());
    assert_eq!(line_ids(ws.project()), before_ids);
    assert_eq!(codes(&after), codes(&before));
    let text = story_text(ws.project());
    assert!(text.contains("明里「やっと来た。遅いよ」 ^k3f9"));
    assert!(text.contains("@立ち絵 明里 笑顔 中"));
    assert!(text.contains("@設定 明里.好感度 += 2"));
    // 話者・立ち絵・名前空間としての「灯」は残らない。地の文や台詞の中の「灯」は残る。
    assert!(!text.contains("\n灯「"));
    assert!(!text.contains("@立ち絵 灯 "));
    assert!(!text.contains("灯.好感度"));
    assert!(text.contains("湊「灯が元気すぎるんだよ」"));
    assert!(read(&root, "bible/entries/character/akari.md").contains("name: 明里"));
    let speakers_after = after
        .scenes
        .iter()
        .flat_map(|s| &s.ops)
        .filter(|op| format!("{op:?}").contains("akari"))
        .count();
    assert_eq!(speakers_after, speakers_before);
    let _ = fs::remove_dir_all(&root);
}

/// 変数：宣言のキーと、そろえると同じ変数になるすべての書き方（名前空間の書き方は残す）。
#[test]
fn rename_variable() {
    let root = copy_sample("houkago", "var");
    let mut ws = Workspace::open(&root).unwrap();
    let before_ids = line_ids(ws.project());
    let before = analyze(ws.project());
    let uses = |project: &Project, ir: &Ir, key: &str| {
        let index = build_index(project, ir);
        index
            .variables
            .iter()
            .find(|v| v.key == key)
            .map(|v| (v.reads.len(), v.writes.len(), v.declared))
    };
    let old = uses(ws.project(), &before, "akari.好感度").unwrap();
    ws.rename(
        &RenameTarget::Variable {
            key: "akari.好感度".to_owned(),
        },
        "akari.信頼",
    )
    .unwrap();
    let after = analyze(ws.project());
    assert_eq!(line_ids(ws.project()), before_ids);
    assert_eq!(codes(&after), codes(&before));
    assert_eq!(uses(ws.project(), &after, "akari.信頼"), Some(old));
    assert_eq!(uses(ws.project(), &after, "akari.好感度"), None);
    let text = story_text(ws.project());
    assert!(text.contains("@設定 灯.信頼 += 2"));
    assert!(read(&root, "variables.yaml").contains("\nakari.信頼:\n"));
    // 名前空間を省いた変数（flag）。
    let old = uses(ws.project(), &after, "flag.勇気").unwrap();
    ws.rename(
        &RenameTarget::Variable {
            key: "flag.勇気".to_owned(),
        },
        "覚悟",
    )
    .unwrap();
    let after = analyze(ws.project());
    assert_eq!(uses(ws.project(), &after, "flag.覚悟"), Some(old));
    let text = story_text(ws.project());
    assert!(text.contains("@設定 覚悟 += 1"));
    assert!(!text.contains("勇気 >="));
    assert!(read(&root, "variables.yaml").contains("\nflag.覚悟:\n"));
    let _ = fs::remove_dir_all(&root);
}

/// 変わる場所（T4-20）：置き換えのある行は、置き換える前の本文でその名前を含む行で、重ねずに小さい順。
#[test]
fn rename_plan_lines() {
    let root = copy_sample("houkago", "lines");
    let ws = Workspace::open(&root).unwrap();
    let plan = plan_rename(
        ws.project(),
        ws.root(),
        &RenameTarget::Variable {
            key: "akari.好感度".to_owned(),
        },
        "akari.信頼",
    )
    .unwrap();
    assert!(!plan.story.is_empty());
    for change in &plan.story {
        let original = &ws
            .project()
            .files
            .iter()
            .find(|f| f.path == change.file)
            .unwrap()
            .text;
        let rows: Vec<&str> = original.split('\n').collect();
        assert!(!change.lines.is_empty());
        assert!(change.lines.windows(2).all(|w| w[0] < w[1]));
        assert!(change.lines.len() <= change.edits.len());
        for &line in &change.lines {
            assert!(rows[line - 1].contains("好感度"), "{}:{line}", change.file);
        }
    }
    let _ = fs::remove_dir_all(&root);
}

/// エントリのID（キャラクター）：ファイル名、`id`、変数の名前空間、素材カタログ、企画書の主要キャラ。
#[test]
fn rename_character_id() {
    let root = copy_sample("houkago", "id");
    let mut ws = Workspace::open(&root).unwrap();
    let before_ids = line_ids(ws.project());
    let before = analyze(ws.project());
    ws.rename(
        &RenameTarget::EntryId {
            type_id: "character".to_owned(),
            id: "akari".to_owned(),
        },
        "akari2",
    )
    .unwrap();
    assert!(!root.join("bible/entries/character/akari.md").exists());
    let entry = read(&root, "bible/entries/character/akari2.md");
    assert!(entry.starts_with("---\nid: akari2\n"), "{entry}");
    let after = analyze(ws.project());
    assert_eq!(line_ids(ws.project()), before_ids);
    assert_eq!(codes(&after), codes(&before));
    let vars = read(&root, "variables.yaml");
    assert!(vars.contains("\nakari2.好感度:\n") && !vars.contains("\nakari.好感度:"));
    let catalog = read(&root, "assets/catalog.yaml");
    assert!(catalog.contains("\n  akari2:\n"), "{catalog}");
    assert!(read(&root, "concept.md").contains("main_characters: [akari2, tsumugi, minato]"));
    // IR の話者は新しいID。
    assert!(format!("{:?}", after.scenes).contains("\"akari2\""));
    let _ = fs::remove_dir_all(&root);
}

/// 名前が正しくない、もう使われている、外で変わっているときは、何も変えない。
#[test]
fn rename_refusals() {
    let root = copy_sample("houkago", "refuse");
    let project = load_project(&root).unwrap();
    let scene = |id: &str| RenameTarget::Scene { id: id.to_owned() };
    for (target, new) in [
        (scene("s012"), "s-1"),
        (scene("s012"), "s040"),
        (
            RenameTarget::CharacterName {
                id: "akari".to_owned(),
            },
            "紬",
        ),
        (
            RenameTarget::CharacterName {
                id: "akari".to_owned(),
            },
            "灯 ちゃん",
        ),
        (
            RenameTarget::CharacterName {
                id: "akari".to_owned(),
            },
            "灯「",
        ),
        (
            RenameTarget::Variable {
                key: "akari.好感度".to_owned(),
            },
            "tsumugi.好感度",
        ),
        (
            RenameTarget::Variable {
                key: "akari.好感度".to_owned(),
            },
            "1x",
        ),
        (
            RenameTarget::EntryId {
                type_id: "character".to_owned(),
                id: "akari".to_owned(),
            },
            "tsumugi",
        ),
        (
            RenameTarget::EntryId {
                type_id: "character".to_owned(),
                id: "akari".to_owned(),
            },
            "灯",
        ),
    ] {
        assert!(
            plan_rename(&project, &root, &target, new).is_err(),
            "{target:?} {new}"
        );
    }
    // 章が外で変わっていてまだ取り込んでいない。
    let mut ws = Workspace::open(&root).unwrap();
    let chapter = read(&root, "story/chapter01.scn");
    fs::write(
        root.join("story/chapter01.scn"),
        format!("{chapter}// 外\n"),
    )
    .unwrap();
    assert!(ws.rename(&scene("s012"), "s100").is_err());
    assert!(read(&root, "ideas/rooftop-confession.md").contains("scene/s012"));
    let _ = fs::remove_dir_all(&root);
}

/// F2：カーソルのところの名前。
#[test]
fn symbols() {
    let root = sample("houkago");
    let project = load_project(&root).unwrap();
    let text = &project
        .files
        .iter()
        .find(|f| f.path == "story/chapter01.scn")
        .unwrap()
        .text;
    let at = |needle: &str, skip: usize| text.find(needle).unwrap() + skip;
    let symbol = |offset: usize| symbol_at(&project, "story/chapter01.scn", offset);
    let s = symbol(at("#s012", 2)).unwrap();
    assert_eq!(
        s.target,
        RenameTarget::Scene {
            id: "s012".to_owned()
        }
    );
    let s = symbol(at("灯「やっと来た", 0)).unwrap();
    assert_eq!(
        s.target,
        RenameTarget::CharacterName {
            id: "akari".to_owned()
        }
    );
    assert_eq!(s.current, "灯");
    let s = symbol(at("@立ち絵 灯", "@立ち絵 ".len())).unwrap();
    assert_eq!(s.current, "灯");
    let s = symbol(at("灯.好感度 += 2", "灯.".len() + 1)).unwrap();
    assert_eq!(
        s.target,
        RenameTarget::Variable {
            key: "akari.好感度".to_owned()
        }
    );
    let s = symbol(at("灯.好感度 += 2", 0)).unwrap();
    assert_eq!(
        s.target,
        RenameTarget::CharacterName {
            id: "akari".to_owned()
        }
    );
    // 地の文の中には無い。
    assert!(symbol(at("湊「灯が元気", "湊「".len())).is_none());
}

/// 章の名前の変更（保存していない変更は持ち越し、フロー図の章の位置も移す）と削除。
#[test]
fn chapters() {
    let root = copy_sample("houkago", "chapter");
    fs::write(
        root.join("layout.json"),
        "{\n  \"chapter\": { \"story/chapter03.scn\": { \"x\": 1, \"y\": 2 } }\n}\n",
    )
    .unwrap();
    let mut ws = Workspace::open(&root).unwrap();
    let text = ws.text("story/chapter03.scn").unwrap().to_owned();
    assert!(ws.set_buffer("story/chapter03.scn", &format!("{text}// 編集中\n")));
    let to = ws
        .rename_chapter("story/chapter03.scn", "epilogue")
        .unwrap();
    assert_eq!(to, "story/epilogue.scn");
    assert!(!root.join("story/chapter03.scn").exists());
    assert_eq!(read(&root, "story/epilogue.scn"), text);
    assert!(ws.is_dirty("story/epilogue.scn"));
    assert!(read(&root, "layout.json").contains("story/epilogue.scn"));
    assert!(
        ws.rename_chapter("story/epilogue.scn", "chapter01")
            .is_err()
    );
    assert!(ws.rename_chapter("story/epilogue.scn", "a/b").is_err());
    ws.delete_chapter("story/epilogue.scn").unwrap();
    assert!(!root.join("story/epilogue.scn").exists());
    assert!(ws.text("story/epilogue.scn").is_none());
    let _ = fs::remove_dir_all(&root);
}

/// 新しい名前が本文で使えないとき（話者として書けない、名前空間として書けない）と、本文の別のものを取り込むとき
/// （まだ登録していない話者、行き先の無いジャンプ）は断る。
#[test]
fn rename_refuses_unusable_names() {
    let root = copy_sample("houkago", "unusable");
    let chapter = read(&root, "story/chapter03.scn");
    fs::write(
        root.join("story/chapter03.scn"),
        format!("{chapter}\n## 店 #s900\n店員「いらっしゃい」\n-> s999\n"),
    )
    .unwrap();
    let project = load_project(&root).unwrap();
    let name = RenameTarget::CharacterName {
        id: "akari".to_owned(),
    };
    for new in ["@灯", "//灯", "Dr.灯", "アリス・ハート", "店員"] {
        assert!(plan_rename(&project, &root, &name, new).is_err(), "{new}");
    }
    let id = RenameTarget::EntryId {
        type_id: "character".to_owned(),
        id: "akari".to_owned(),
    };
    for new in ["1akari", "and"] {
        assert!(plan_rename(&project, &root, &id, new).is_err(), "{new}");
    }
    let scene = RenameTarget::Scene {
        id: "s012".to_owned(),
    };
    assert!(plan_rename(&project, &root, &scene, "s999").is_err());
    let _ = fs::remove_dir_all(&root);
}

/// 同じ名前のキャラクターが2人いて解決できない話者や、IDの段で別のキャラクターに決まる話者は変えない。
#[test]
fn rename_follows_resolution() {
    let root = std::env::temp_dir().join(format!("scn-core-rename-{}-resolve", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let files = [
        ("project.toml", "format_version = 0\n".to_owned()),
        (
            "bible/types/character.yaml",
            "name: キャラクター\nbuiltin: character\nfields: []\n".to_owned(),
        ),
        (
            "bible/entries/character/a.md",
            "---\nid: a\nname: 先生\n---\n".to_owned(),
        ),
        (
            "bible/entries/character/b.md",
            "---\nid: b\nname: 先生\n---\n".to_owned(),
        ),
        (
            "bible/entries/character/c.md",
            "---\nid: c\nname: 先輩\n---\n".to_owned(),
        ),
        (
            "bible/entries/character/先輩.md",
            "---\nid: 先輩\nname: 部長\n---\n".to_owned(),
        ),
        (
            "story/a.scn",
            "## a #s1\n先生「曖昧」\nc「名前でなくIDで」\n先輩「名前で」\n".to_owned(),
        ),
    ];
    for (path, text) in &files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let project = load_project(&root).unwrap();
    // a の名前「先生」は b と重なるので、`先生「…」` は解決できない話者のまま。変えない。
    let plan = plan_rename(
        &project,
        &root,
        &RenameTarget::CharacterName { id: "a".to_owned() },
        "山田",
    )
    .unwrap();
    assert!(plan.story.is_empty(), "{plan:?}");
    // c の名前「先輩」：本文の `先輩「…」` はIDの段でキャラクター「先輩」に決まるので変えない。IDで書いた
    // `c「…」` も変えない。
    let plan = plan_rename(
        &project,
        &root,
        &RenameTarget::CharacterName { id: "c".to_owned() },
        "先輩2",
    )
    .unwrap();
    assert!(plan.story.is_empty(), "{plan:?}");
    let _ = fs::remove_dir_all(&root);
}

/// 設定ファイルの古いキーを、ほかの行を変えずに書き換えられなければ断る（flow の書き方など）。Ren'Py の
/// 出力プロファイルの `variables` のキーも書き換える。
#[test]
fn rename_settings_must_follow() {
    let root = copy_sample("houkago", "settings");
    let profile = read(&root, "export/renpy.yaml");
    fs::write(
        root.join("export/renpy.yaml"),
        format!("{profile}variables:\n  akari.好感度: aka_love\n"),
    )
    .unwrap();
    let mut ws = Workspace::open(&root).unwrap();
    ws.rename(
        &RenameTarget::Variable {
            key: "akari.好感度".to_owned(),
        },
        "akari.信頼",
    )
    .unwrap();
    assert!(read(&root, "export/renpy.yaml").contains("\n  akari.信頼: aka_love\n"));
    // Ren'Py のキャラクターを flow で書いたプロファイル：書き換えられないので断る。
    let profile = read(&root, "export/renpy.yaml");
    fs::write(
        root.join("export/renpy.yaml"),
        format!("{profile}characters: {{ akari: aka }}\n"),
    )
    .unwrap();
    let project = load_project(&root).unwrap();
    let err = plan_rename(
        &project,
        &root,
        &RenameTarget::EntryId {
            type_id: "character".to_owned(),
            id: "akari".to_owned(),
        },
        "akari2",
    )
    .unwrap_err();
    assert!(err.contains("renpy.yaml"), "{err}");
    let _ = fs::remove_dir_all(&root);
}

/// 書くファイルが読み取り専用なら、何も書かずに断る。エントリのファイルを移した元の知らせは無視する。
#[test]
fn rename_checks_before_writing() {
    let root = copy_sample("houkago", "readonly");
    let entry = root.join("bible/entries/character/akari.md");
    let mut perms = fs::metadata(&entry).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&entry, perms.clone()).unwrap();
    let chapter = read(&root, "story/chapter01.scn");
    let mut ws = Workspace::open(&root).unwrap();
    assert!(
        ws.rename(
            &RenameTarget::CharacterName {
                id: "akari".to_owned()
            },
            "明里",
        )
        .is_err()
    );
    assert_eq!(read(&root, "story/chapter01.scn"), chapter);
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(&entry, perms).unwrap();
    let renamed = ws
        .rename(
            &RenameTarget::EntryId {
                type_id: "character".to_owned(),
                id: "akari".to_owned(),
            },
            "akari2",
        )
        .unwrap();
    assert!(renamed.problem.is_none());
    assert_eq!(
        ws.external_change(&entry),
        scn_core::workspace::Change::None
    );
    let _ = fs::remove_dir_all(&root);
}

/// 「」を書いていない作品（`[dialogue.brackets]` が『』だけ）でも、作品の括弧で話者として書けるかを確かめる。
#[test]
fn rename_uses_project_brackets() {
    let root =
        std::env::temp_dir().join(format!("scn-core-rename-{}-brackets", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let files = [
        (
            "project.toml",
            "format_version = 0\n\n[dialogue.brackets]\n\"『』\" = \"normal\"\n",
        ),
        (
            "bible/types/character.yaml",
            "name: キャラクター\nbuiltin: character\nfields: []\n",
        ),
        (
            "bible/entries/character/akari.md",
            "---\nid: akari\nname: 灯\n---\n",
        ),
        ("story/a.scn", "## a #s1\n灯『x』 ^aaaa01\n"),
    ];
    for (path, text) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let project = load_project(&root).unwrap();
    let plan = plan_rename(
        &project,
        &root,
        &RenameTarget::CharacterName {
            id: "akari".to_owned(),
        },
        "明里",
    )
    .unwrap();
    assert_eq!(plan.story[0].text, "## a #s1\n明里『x』 ^aaaa01\n");
    assert!(
        plan_rename(
            &project,
            &root,
            &RenameTarget::EntryId {
                type_id: "character".to_owned(),
                id: "akari".to_owned(),
            },
            "akari2",
        )
        .is_ok()
    );
    let _ = fs::remove_dir_all(&root);
}
