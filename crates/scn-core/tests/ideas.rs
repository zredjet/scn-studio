//! アイデアメモと企画書（`ideas`・`concept`、T2-6、D057）：メモの読み方、昇格（完了条件：ほかの行が
//! 変わらない）、新しいメモ、外部の変更、企画書の画面のデータ。

use std::fs;
use std::path::{Path, PathBuf};

use scn_core::concept::concept_data;
use scn_core::ideas::{IdeaTarget, Promotion, ideas};
use scn_core::workspace::{Change, Promoted, Workspace};
use scn_core::{analyze, load_project};

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

/// サンプルを一時フォルダに写す。
fn copy_sample(name: &str, tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scn-core-ideas-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    copy_dir(&sample(name), &dir);
    dir
}

/// 行の並びの差：`before` にだけある行と、`after` にだけある行。
fn line_diff(before: &str, after: &str) -> (Vec<String>, Vec<String>) {
    let a: Vec<&str> = before.split_inclusive('\n').collect();
    let b: Vec<&str> = after.split_inclusive('\n').collect();
    let only_a = a
        .iter()
        .filter(|l| !b.contains(l))
        .map(|l| (*l).to_owned())
        .collect();
    let only_b = b
        .iter()
        .filter(|l| !a.contains(l))
        .map(|l| (*l).to_owned())
        .collect();
    (only_a, only_b)
}

#[test]
fn houkago_ideas() {
    let root = sample("houkago");
    let project = load_project(&root).unwrap();
    let ir = analyze(&project);
    let list = ideas(&root, &project, &ir);
    let files: Vec<&str> = list.iter().map(|i| i.file.as_str()).collect();
    assert_eq!(
        files,
        [
            "ideas/hairpin.md",
            "ideas/hero-transfer.md",
            "ideas/rooftop-confession.md",
            "ideas/rooftop-key.md",
            "ideas/tsumugi-knows.md",
        ]
    );
    let by = |name: &str| list.iter().find(|i| i.file.ends_with(name)).unwrap();
    let hairpin = by("hairpin.md");
    assert_eq!(hairpin.status, "採用");
    assert_eq!(hairpin.tags, ["灯"]);
    assert_eq!(hairpin.body, "灯の髪留めは母の形見。\n");
    match &hairpin.target {
        Some(IdeaTarget::Entry {
            type_id,
            type_name,
            name,
            path,
            ..
        }) => {
            assert_eq!(type_id, "foreshadow");
            assert_eq!(type_name, "伏線");
            assert_eq!(name, "髪留め");
            assert_eq!(path, "bible/entries/foreshadow/hairpin.md");
        }
        other => panic!("{other:?}"),
    }
    match &by("rooftop-confession.md").target {
        Some(IdeaTarget::Scene {
            id, title, file, ..
        }) => {
            assert_eq!(id, "s012");
            assert_eq!(title, "放課後の屋上");
            assert_eq!(file, "story/chapter01.scn");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(by("hero-transfer.md").status, "保留");
    assert!(by("rooftop-key.md").target.is_none());
    assert!(list.iter().all(|i| i.problems.is_empty()));
}

/// status が無い・知らない値、読めないフロントマター、無い昇格先。
#[test]
fn odd_ideas() {
    let root = copy_sample("amayadori", "odd");
    fs::create_dir_all(root.join("ideas/sub")).unwrap();
    fs::write(root.join("ideas/a.md"), "フロントマターの無いメモ\n").unwrap();
    fs::write(
        root.join("ideas/b.md"),
        "---\nstatus: 済\ntags: 雨\n---\n本文\n",
    )
    .unwrap();
    fs::write(root.join("ideas/c.md"), "---\nstatus: [\n---\n本文\n").unwrap();
    fs::write(
        root.join("ideas/d.md"),
        "---\nstatus: 採用\npromoted_to: scene/s999\n---\n",
    )
    .unwrap();
    fs::write(root.join("ideas/.hidden.md"), "隠し\n").unwrap();
    fs::write(root.join("ideas/e.txt"), "メモでない\n").unwrap();
    fs::write(root.join("ideas/sub/f.md"), "下のフォルダ\n").unwrap();
    fs::write(root.join("ideas/g.md"), b"\xff\xfe").unwrap();
    let project = load_project(&root).unwrap();
    let ir = analyze(&project);
    let list = ideas(&root, &project, &ir);
    let files: Vec<&str> = list.iter().map(|i| i.file.as_str()).collect();
    assert_eq!(
        files,
        [
            "ideas/a.md",
            "ideas/b.md",
            "ideas/c.md",
            "ideas/d.md",
            "ideas/g.md"
        ]
    );
    let [a, b, c, d, g] = list.as_slice() else {
        panic!()
    };
    assert_eq!(a.status, "未整理");
    assert!(a.problems.is_empty());
    assert_eq!(a.body, "フロントマターの無いメモ\n");
    assert_eq!(b.status, "未整理");
    assert_eq!(b.tags, ["雨"]);
    assert!(
        b.problems.iter().any(|p| p.contains("済")),
        "{:?}",
        b.problems
    );
    assert_eq!(c.status, "未整理");
    assert!(!c.problems.is_empty());
    assert!(matches!(
        &d.target,
        Some(IdeaTarget::Missing { value }) if value == "scene/s999"
    ));
    assert!(g.text.is_none());
    assert!(!g.problems.is_empty());
    let _ = fs::remove_dir_all(&root);
}

/// 完了条件：昇格で、メモの直したキーのほかの行が変わらない。章は末尾に足すだけで、ほかのファイルは変わらない。
#[test]
fn promote_to_scene_keeps_other_lines() {
    let root = copy_sample("houkago", "scene");
    let memo = "---\r\n# 屋上のメモ\r\nstatus: 未整理   # 状態\r\ntags: [灯, 屋上]\r\nsource: 夢\r\n---\r\n屋上の鍵を持っているのは灯だけ。\r\n主人公はなぜ屋上に入れるのか。\r\n";
    fs::write(root.join("ideas/key.md"), memo).unwrap();
    let chapter_before = fs::read_to_string(root.join("story/chapter02.scn")).unwrap();
    let others: Vec<(String, Vec<u8>)> = ["story/chapter01.scn", "story/chapter03.scn"]
        .iter()
        .map(|f| ((*f).to_owned(), fs::read(root.join(f)).unwrap()))
        .collect();
    let mut ws = Workspace::open(&root).unwrap();
    let promoted = ws
        .promote_idea(
            "ideas/key.md",
            memo,
            &Promotion::Scene {
                file: "story/chapter02.scn".to_owned(),
                id: "s099".to_owned(),
                title: "屋上の鍵".to_owned(),
            },
        )
        .unwrap();
    let Promoted::Saved {
        text,
        target,
        scene,
    } = promoted
    else {
        panic!("{promoted:?}")
    };
    assert_eq!(target, "scene/s099");
    // メモ：status の行（行末のコメントは残る）と、足した promoted_to の行だけが違う。
    assert_eq!(fs::read_to_string(root.join("ideas/key.md")).unwrap(), text);
    let (gone, added) = line_diff(memo, &text);
    assert_eq!(gone, ["status: 未整理   # 状態\r\n"]);
    assert_eq!(
        added,
        ["status: 採用   # 状態\r\n", "promoted_to: scene/s099\r\n"]
    );
    assert!(text.ends_with(
        "---\r\n屋上の鍵を持っているのは灯だけ。\r\n主人公はなぜ屋上に入れるのか。\r\n"
    ));
    // 章：前の中身はそのまま、末尾にスタブのシーンを足す。
    let written = scene.unwrap();
    assert_eq!(written.file, "story/chapter02.scn");
    let chapter_after = fs::read_to_string(root.join("story/chapter02.scn")).unwrap();
    assert_eq!(chapter_after, written.text);
    assert!(chapter_after.starts_with(&chapter_before));
    assert_eq!(
        chapter_after.get(chapter_before.len()..).unwrap(),
        "\n## 屋上の鍵 #s099\n---\nsummary: 屋上の鍵を持っているのは灯だけ。\n---\n"
    );
    for (file, bytes) in &others {
        assert_eq!(&fs::read(root.join(file)).unwrap(), bytes, "{file}");
    }
    // 足したシーンはスタブとして読め、ワークスペースの本文も変わっている。
    assert_eq!(ws.text("story/chapter02.scn"), Some(chapter_after.as_str()));
    assert!(!ws.is_dirty("story/chapter02.scn"));
    let ir = analyze(ws.project());
    let stub = ir.scenes.iter().find(|s| s.id == "s099").unwrap();
    assert!(stub.is_stub);
    assert_eq!(
        stub.meta.summary.as_deref(),
        Some("屋上の鍵を持っているのは灯だけ。")
    );
    // 自分で書いたメモと章の監視の知らせは無視する。
    assert_eq!(ws.external_change(&root.join("ideas/key.md")), Change::None);
    assert_eq!(
        ws.external_change(&root.join("story/chapter02.scn")),
        Change::None
    );
    let _ = fs::remove_dir_all(&root);
}

/// 設定のエントリと伏線への昇格：エントリの本文はメモの本文。伏線の型定義が無ければ、標準の定義を作る。
#[test]
fn promote_to_entries() {
    let root = copy_sample("amayadori", "entry");
    fs::create_dir_all(root.join("ideas")).unwrap();
    let memo = "---\nstatus: 保留\ntags: [志乃]\n---\n志乃は駅の時刻表を全部覚えている。\n";
    fs::write(root.join("ideas/timetable.md"), memo).unwrap();
    let mut ws = Workspace::open(&root).unwrap();
    let promoted = ws
        .promote_idea(
            "ideas/timetable.md",
            memo,
            &Promotion::Entry {
                type_id: "foreshadow".to_owned(),
                id: "timetable".to_owned(),
                name: "時刻表".to_owned(),
            },
        )
        .unwrap();
    let Promoted::Saved {
        text,
        target,
        scene,
    } = promoted
    else {
        panic!("{promoted:?}")
    };
    assert_eq!(target, "foreshadow/timetable");
    assert!(scene.is_none());
    assert_eq!(
        text,
        "---\nstatus: 採用\ntags: [志乃]\npromoted_to: foreshadow/timetable\n---\n志乃は駅の時刻表を全部覚えている。\n"
    );
    let entry = fs::read_to_string(root.join("bible/entries/foreshadow/timetable.md")).unwrap();
    assert_eq!(
        entry,
        "---\nid: timetable\nname: 時刻表\n---\n志乃は駅の時刻表を全部覚えている。\n"
    );
    let ty = fs::read_to_string(root.join("bible/types/foreshadow.yaml")).unwrap();
    assert!(ty.contains("key: payoff"), "{ty}");
    assert!(
        ws.project()
            .bible
            .entries
            .iter()
            .any(|e| e.type_id == "foreshadow" && e.id == "timetable")
    );
    // 設定（キャラクター）のエントリ。メモの本文が空でも作る。
    let empty = "---\nstatus: 未整理\n---\n";
    fs::write(root.join("ideas/stationmaster.md"), empty).unwrap();
    let promoted = ws
        .promote_idea(
            "ideas/stationmaster.md",
            empty,
            &Promotion::Entry {
                type_id: "character".to_owned(),
                id: "ekicho".to_owned(),
                name: "駅長".to_owned(),
            },
        )
        .unwrap();
    assert!(matches!(promoted, Promoted::Saved { .. }), "{promoted:?}");
    assert_eq!(
        fs::read_to_string(root.join("bible/entries/character/ekicho.md")).unwrap(),
        "---\nid: ekicho\nname: 駅長\n---\n"
    );
    let _ = fs::remove_dir_all(&root);
}

/// 昇格できないとき：何も作らず、メモも変えない。
#[test]
fn promote_refusals() {
    let root = copy_sample("houkago", "refuse");
    let memo = "---\nstatus: 未整理\n---\nメモ\n";
    fs::write(root.join("ideas/x.md"), memo).unwrap();
    let chapter = fs::read_to_string(root.join("story/chapter01.scn")).unwrap();
    let akari = fs::read(root.join("bible/entries/character/akari.md")).unwrap();
    let mut ws = Workspace::open(&root).unwrap();
    let scene = |id: &str, file: &str| Promotion::Scene {
        file: file.to_owned(),
        id: id.to_owned(),
        title: "題".to_owned(),
    };
    // 使われているシーンID、形の誤ったID、無い章。
    for promotion in [
        scene("s012", "story/chapter01.scn"),
        scene("s-1", "story/chapter01.scn"),
        scene("s100", "story/none.scn"),
    ] {
        assert!(ws.promote_idea("ideas/x.md", memo, &promotion).is_err());
    }
    // 題名が空、改行を含む。
    for title in ["", " ", "a\nb"] {
        let promotion = Promotion::Scene {
            file: "story/chapter01.scn".to_owned(),
            id: "s100".to_owned(),
            title: title.to_owned(),
        };
        assert!(ws.promote_idea("ideas/x.md", memo, &promotion).is_err());
    }
    // 保存していない変更のある章には足さない。
    assert!(ws.set_buffer("story/chapter01.scn", &format!("{chapter}x\n")));
    assert!(
        ws.promote_idea("ideas/x.md", memo, &scene("s100", "story/chapter01.scn"))
            .is_err()
    );
    assert!(ws.set_buffer("story/chapter01.scn", &chapter));
    // 使われているエントリのID。
    let entry = Promotion::Entry {
        type_id: "character".to_owned(),
        id: "akari".to_owned(),
        name: "灯".to_owned(),
    };
    assert!(ws.promote_idea("ideas/x.md", memo, &entry).is_err());
    // メモが外で変わっていた：書かずに今の中身を返す。
    let promoted = ws
        .promote_idea(
            "ideas/x.md",
            "古い中身",
            &scene("s100", "story/chapter01.scn"),
        )
        .unwrap();
    assert_eq!(
        promoted,
        Promoted::Conflict {
            text: memo.to_owned()
        }
    );
    // メモのパスでないもの。
    assert!(
        ws.promote_idea(
            "story/chapter01.scn",
            memo,
            &scene("s100", "story/chapter01.scn")
        )
        .is_err()
    );
    assert_eq!(fs::read_to_string(root.join("ideas/x.md")).unwrap(), memo);
    assert_eq!(
        fs::read_to_string(root.join("story/chapter01.scn")).unwrap(),
        chapter
    );
    assert_eq!(
        fs::read(root.join("bible/entries/character/akari.md")).unwrap(),
        akari
    );
    let _ = fs::remove_dir_all(&root);
}

/// 章に、まだ取り込んでいない外部の変更があるとき（選ぶ前の外の中身、監視の知らせの前に変わったディスク）は
/// 足さない。
#[test]
fn promote_refuses_changed_chapter() {
    let root = copy_sample("houkago", "changed");
    let memo = "---\nstatus: 未整理\n---\nメモ\n";
    fs::write(root.join("ideas/x.md"), memo).unwrap();
    let path = root.join("story/chapter03.scn");
    let chapter = fs::read_to_string(&path).unwrap();
    let mut ws = Workspace::open(&root).unwrap();
    let promotion = Promotion::Scene {
        file: "story/chapter03.scn".to_owned(),
        id: "s100".to_owned(),
        title: "題".to_owned(),
    };
    // 監視の知らせの前に、外で変わった。
    fs::write(&path, format!("{chapter}// 外で足した\n")).unwrap();
    assert!(ws.promote_idea("ideas/x.md", memo, &promotion).is_err());
    // 保存していない変更があるときに外で変わり、まだ選んでいない（自分の中身に戻しても足さない）。
    assert!(ws.set_buffer("story/chapter03.scn", &format!("{chapter}x\n")));
    assert!(matches!(ws.external_change(&path), Change::Conflict { .. }));
    assert!(ws.set_buffer("story/chapter03.scn", &chapter));
    assert!(ws.promote_idea("ideas/x.md", memo, &promotion).is_err());
    assert_eq!(fs::read_to_string(root.join("ideas/x.md")).unwrap(), memo);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        format!("{chapter}// 外で足した\n")
    );
    let _ = fs::remove_dir_all(&root);
}

/// 伏線の型の定義があるのに読めないときは、上書きせずに断る。メモが読み取り専用なら、何も作らない。
#[test]
fn promote_refuses_unreadable_type_and_readonly_memo() {
    let root = copy_sample("amayadori", "unreadable");
    fs::create_dir_all(root.join("ideas")).unwrap();
    let memo = "---\nstatus: 未整理\n---\nメモ\n";
    fs::write(root.join("ideas/x.md"), memo).unwrap();
    let broken = "name: 伏線\n# 人の書いた定義\nfields:\n  - { key: setup, label: 仕込み, type: list<ref<scene>>\n";
    fs::write(root.join("bible/types/foreshadow.yaml"), broken).unwrap();
    let mut ws = Workspace::open(&root).unwrap();
    let foreshadow = Promotion::Entry {
        type_id: "foreshadow".to_owned(),
        id: "umbrella".to_owned(),
        name: "傘".to_owned(),
    };
    assert!(ws.promote_idea("ideas/x.md", memo, &foreshadow).is_err());
    assert_eq!(
        fs::read_to_string(root.join("bible/types/foreshadow.yaml")).unwrap(),
        broken
    );
    assert!(!root.join("bible/entries/foreshadow/umbrella.md").exists());
    assert_eq!(fs::read_to_string(root.join("ideas/x.md")).unwrap(), memo);
    // 読み取り専用のメモ。
    let chapter = fs::read_to_string(root.join("story/chapter02.scn")).unwrap();
    let memo_path = root.join("ideas/x.md");
    let mut perms = fs::metadata(&memo_path).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&memo_path, perms.clone()).unwrap();
    let scene = Promotion::Scene {
        file: "story/chapter02.scn".to_owned(),
        id: "zz1".to_owned(),
        title: "題".to_owned(),
    };
    assert!(ws.promote_idea("ideas/x.md", memo, &scene).is_err());
    assert_eq!(
        fs::read_to_string(root.join("story/chapter02.scn")).unwrap(),
        chapter
    );
    assert_eq!(ws.text("story/chapter02.scn"), Some(chapter.as_str()));
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(&memo_path, perms).unwrap();
    let _ = fs::remove_dir_all(&root);
}

/// 昇格先を作った後でメモを書けなかったとき（フォルダに書けない）：エラーにせず、作ったものを返す。
#[cfg(unix)]
#[test]
fn promote_reports_unwritten_memo() {
    use std::os::unix::fs::PermissionsExt;
    let root = copy_sample("amayadori", "unwritten");
    fs::create_dir_all(root.join("ideas")).unwrap();
    let memo = "---\nstatus: 未整理\n---\nメモ\n";
    fs::write(root.join("ideas/x.md"), memo).unwrap();
    let mut ws = Workspace::open(&root).unwrap();
    // 一時ファイルを作れないように、メモのフォルダを読み取り専用にする。
    fs::set_permissions(root.join("ideas"), fs::Permissions::from_mode(0o555)).unwrap();
    let promoted = ws
        .promote_idea(
            "ideas/x.md",
            memo,
            &Promotion::Scene {
                file: "story/chapter02.scn".to_owned(),
                id: "zz1".to_owned(),
                title: "題".to_owned(),
            },
        )
        .unwrap();
    fs::set_permissions(root.join("ideas"), fs::Permissions::from_mode(0o755)).unwrap();
    match promoted {
        Promoted::MemoUnwritten { target, scene, .. } => {
            assert_eq!(target, "scene/zz1");
            let scene = scene.unwrap();
            assert!(scene.text.contains("## 題 #zz1"));
            assert_eq!(ws.text("story/chapter02.scn"), Some(scene.text.as_str()));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(fs::read_to_string(root.join("ideas/x.md")).unwrap(), memo);
    let _ = fs::remove_dir_all(&root);
}

/// 新しいメモ：`ideas/idea-NNN.md`（使われていない最初の番号）。status は未整理、タグは渡したもの。
#[test]
fn create_ideas() {
    let root = copy_sample("amayadori", "create");
    let mut ws = Workspace::open(&root).unwrap();
    let (file, text) = ws.create_idea("雨の日だけ開く売店。", &[]).unwrap();
    assert_eq!(file, "ideas/idea-001.md");
    assert_eq!(text, "---\nstatus: 未整理\n---\n雨の日だけ開く売店。\n");
    assert_eq!(fs::read_to_string(root.join(&file)).unwrap(), text);
    let (file, text) = ws
        .create_idea(
            "  志乃の傘は誰のもの？\n",
            &["志乃".to_owned(), "伏線候補".to_owned()],
        )
        .unwrap();
    assert_eq!(file, "ideas/idea-002.md");
    assert_eq!(
        text,
        "---\nstatus: 未整理\ntags: [志乃, 伏線候補]\n---\n  志乃の傘は誰のもの？\n"
    );
    assert!(ws.create_idea(" \n ", &[]).is_err());
    // 自分で書いたメモの知らせは無視し、外で変わったら知らせる。
    assert_eq!(ws.external_change(&root.join(&file)), Change::None);
    fs::write(root.join(&file), "---\nstatus: 没\n---\n").unwrap();
    assert_eq!(ws.external_change(&root.join(&file)), Change::Ideas);
    fs::write(root.join("ideas/new.md"), "外で足した\n").unwrap();
    assert_eq!(
        ws.external_change(&root.join("ideas/new.md")),
        Change::Ideas
    );
    assert_eq!(ws.external_change(&root.join("ideas/x.txt")), Change::None);
    let _ = fs::remove_dir_all(&root);
}

/// メモの項目と本文を、設定DBのエントリと同じ書き換えで直す。
#[test]
fn edit_idea_fields() {
    let root = copy_sample("houkago", "edit");
    let mut ws = Workspace::open(&root).unwrap();
    let seen = fs::read_to_string(root.join("ideas/rooftop-key.md")).unwrap();
    let edit = ws
        .set_entry_field(
            "ideas/rooftop-key.md",
            "status",
            &serde_json::json!("保留"),
            &seen,
        )
        .unwrap();
    let after = fs::read_to_string(root.join("ideas/rooftop-key.md")).unwrap();
    assert_eq!(after, seen.replace("status: 未整理", "status: 保留"));
    assert!(format!("{edit:?}").contains("Saved"));
    assert_eq!(
        ws.external_change(&root.join("ideas/rooftop-key.md")),
        Change::None
    );
    // 企画書も同じ書き換えで直す。
    let concept = fs::read_to_string(root.join("concept.md")).unwrap();
    ws.set_entry_field(
        "concept.md",
        "theme",
        &serde_json::json!("言わない優しさ"),
        &concept,
    )
    .unwrap();
    let after = fs::read_to_string(root.join("concept.md")).unwrap();
    assert_eq!(
        after,
        concept.replace("theme: 言わないことは優しさか", "theme: 言わない優しさ")
    );
    let volume = serde_json::json!({ "chars": 120000, "routes": 3 });
    ws.set_entry_field("concept.md", "volume", &volume, &after)
        .unwrap();
    let after2 = fs::read_to_string(root.join("concept.md")).unwrap();
    assert!(
        after2.contains("\nvolume: { chars: 120000, routes: 3 }\n"),
        "{after2}"
    );
    // ほかのパスは書かない。
    assert!(
        ws.set_entry_field("story/chapter01.scn", "x", &serde_json::json!(1), "")
            .is_err()
    );
    assert!(
        ws.set_entry_field("ideas/../concept.md", "x", &serde_json::json!(1), "")
            .is_err()
    );
    let _ = fs::remove_dir_all(&root);
}

/// 企画書の画面のデータ：フロントマターの項目、主要キャラの欲求と葛藤、エンディングと本文の対応。
#[test]
fn houkago_concept() {
    let root = sample("houkago");
    let project = load_project(&root).unwrap();
    let ir = analyze(&project);
    let data = concept_data(&root, &project, &ir);
    assert!(data.text.is_some());
    assert!(data.problems.is_empty(), "{:?}", data.problems);
    assert_eq!(
        data.logline,
        "転校を言い出せない少女と、屋上でだけ本音を話す主人公の、夏休みまでの三週間"
    );
    assert_eq!(data.theme, "言わないことは優しさか");
    assert_eq!(data.volume_chars, Some(300_000));
    assert_eq!(data.volume_routes, Some(2));
    let ids: Vec<&str> = data.main_characters.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["akari", "tsumugi", "minato"]);
    let akari = &data.main_characters[0];
    assert_eq!(akari.name.as_deref(), Some("灯"));
    assert_eq!(
        akari.path.as_deref(),
        Some("bible/entries/character/akari.md")
    );
    let labels: Vec<&str> = akari.traits.iter().map(|t| t.label.as_str()).collect();
    assert_eq!(labels, ["欲求", "葛藤"]);
    let endings: Vec<(&str, usize)> = data
        .endings
        .iter()
        .map(|e| (e.id.as_str(), e.scenes.len()))
        .collect();
    assert_eq!(
        endings,
        [
            ("akari_end", 1),
            ("tsumugi_end", 1),
            ("true_end", 0),
            ("bad_end", 1)
        ]
    );
    assert_eq!(data.endings[0].name, "灯エンド");
    assert_eq!(data.endings[0].intent, "秘密を受け入れて見送る");
    assert!(data.endings_listed);
    assert!(data.unplanned.is_empty());
    assert!(data.body.starts_with("\n# 企画書"));
}

/// 企画書が無い・読めないとき、本文にだけあるエンディング。
#[test]
fn concept_edge_cases() {
    let root = copy_sample("amayadori", "concept");
    fs::remove_file(root.join("concept.md")).unwrap();
    let project = load_project(&root).unwrap();
    let ir = analyze(&project);
    let data = concept_data(&root, &project, &ir);
    assert!(data.text.is_none());
    assert!(!data.endings_listed);
    // 企画書が無ければ、本文のエンディングはすべて「企画書に無い」に並べる。
    assert_eq!(data.unplanned.len(), 3);
    // 企画書を作る（ひな形）。もうあれば作らない。
    let mut ws = Workspace::open(&root).unwrap();
    let text = ws.create_concept().unwrap();
    assert!(text.contains("logline:"));
    assert!(ws.create_concept().is_err());
    fs::write(root.join("concept.md"), "---\nlogline: [\n---\n本文\n").unwrap();
    let project = load_project(&root).unwrap();
    let data = concept_data(&root, &project, &analyze(&project));
    assert!(!data.problems.is_empty());
    assert_eq!(data.body, "本文\n");
    let _ = fs::remove_dir_all(&root);
}
