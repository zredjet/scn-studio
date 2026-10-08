//! 開いている作品フォルダ（アプリ、T1-6）：保存と、外部の変更の取り込み。

use std::fs;
use std::path::{Path, PathBuf};

use scn_core::line_id::LineIdGenerator;
use scn_core::workspace::{Change, EntryEdit, SaveOutcome, Saved, Workspace};

fn temp_project(name: &str, files: &[(&str, &[u8])]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scn-core-ws-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    for (path, bytes) in files {
        let path = dir.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    dir
}

const TOML: &[u8] = b"format_version = 0\n";

fn generator() -> LineIdGenerator {
    LineIdGenerator::with_seed(1)
}

/// ディスクの中身を知っているものとして保存し、書いた本文を返す（衝突なら失敗させる）。
fn save(ws: &mut Workspace, file: &str) -> Saved {
    let seen = disk_of(ws, file);
    match ws.save(file, seen.as_deref(), &mut generator()).unwrap() {
        SaveOutcome::Saved(saved) => saved,
        SaveOutcome::Conflict { deleted } => panic!("衝突になった（deleted: {deleted}）"),
    }
}

/// 完了条件：保存の前後で、編集していない部分のバイトが変わらない（CRLF、BOM、全角空白、絵文字）。
/// 行IDの揃った本文は、渡したとおりのバイトで書く。
#[test]
fn save_keeps_bytes() {
    let original = "\u{feff}## a #s1\r\n　全角空白で始まる地の文😀 ^aaaa\r\n灯「台詞」 ^aaab\r\n";
    let dir = temp_project(
        "bytes",
        &[("project.toml", TOML), ("story/a.scn", original.as_bytes())],
    );
    let mut ws = Workspace::open(&dir).unwrap();
    let edited = original.replace("台詞", "書き換えた台詞😀");
    assert!(ws.set_buffer("story/a.scn", &edited));
    assert!(ws.is_dirty("story/a.scn"));
    let saved = save(&mut ws, "story/a.scn");
    assert_eq!(saved.text, edited);
    assert!(saved.assigned.is_empty());
    assert_eq!(
        fs::read(dir.join("story/a.scn")).unwrap(),
        edited.as_bytes()
    );
    assert!(!ws.is_dirty("story/a.scn"));
    let _ = fs::remove_dir_all(&dir);
}

/// 保存のときに、行IDの無い行に行IDを付ける（D045）。足した ` ^ID` のほかのバイトは変えない。
#[test]
fn save_assigns_line_ids() {
    let dir = temp_project(
        "ids",
        &[("project.toml", TOML), ("story/a.scn", b"## a #s1\r\n")],
    );
    let mut ws = Workspace::open(&dir).unwrap();
    let edited = "## a #s1\r\n　地の文😀\r\n灯「台詞」 ^aaaa\r\n";
    ws.set_buffer("story/a.scn", edited);
    let saved = save(&mut ws, "story/a.scn");
    let [assigned] = saved.assigned.as_slice() else {
        panic!("1行に付くはず：{:?}", saved.assigned);
    };
    let expected = format!(
        "## a #s1\r\n　地の文😀 ^{}\r\n灯「台詞」 ^aaaa\r\n",
        assigned.id
    );
    assert_eq!(saved.text, expected);
    assert_eq!(
        fs::read_to_string(dir.join("story/a.scn")).unwrap(),
        expected
    );
    assert_eq!(ws.text("story/a.scn"), Some(expected.as_str()));
    let _ = fs::remove_dir_all(&dir);
}

/// `project.toml` を読めなければ（台詞の括弧が既定に戻るので）、行IDを付けずに保存する（D042、D045）。
#[test]
fn save_without_line_ids_when_project_toml_is_broken() {
    let dir = temp_project(
        "broken",
        &[
            ("project.toml", b"format_version = 0\n[dialogue\n"),
            ("story/a.scn", b""),
        ],
    );
    let mut ws = Workspace::open(&dir).unwrap();
    ws.set_buffer("story/a.scn", "## a #s1\n地の文\n");
    let saved = save(&mut ws, "story/a.scn");
    assert!(saved.assigned.is_empty());
    assert_eq!(
        fs::read_to_string(dir.join("story/a.scn")).unwrap(),
        "## a #s1\n地の文\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// `title` など、本文の読み方に関わらない設定の誤りでは、行IDの付与を止めない（D045）。
#[test]
fn save_assigns_line_ids_when_only_title_is_broken() {
    let dir = temp_project(
        "title",
        &[
            ("project.toml", b"format_version = 0\ntitle = 1\n"),
            ("story/a.scn", b""),
        ],
    );
    let mut ws = Workspace::open(&dir).unwrap();
    assert!(!ws.project().load_diagnostics.is_empty());
    ws.set_buffer("story/a.scn", "## a #s1\n地の文\n");
    let saved = save(&mut ws, "story/a.scn");
    assert_eq!(saved.assigned.len(), 1);
    let _ = fs::remove_dir_all(&dir);
}

/// 「読み込み直す」「自分の変更を残す」「保存」は、開いているファイルだけを受け付ける。
/// 作品フォルダの外や、開いていないファイルのパスは断り、読みも書きもしない（フロントに任意のパスを読ませない）。
#[test]
fn only_open_files_are_read_or_written() {
    let dir = temp_project(
        "paths",
        &[
            ("project.toml", TOML),
            ("story/a.scn", b"## a #s1\n"),
            ("story/b.scn", b"## b #s2\n"),
        ],
    );
    let name = dir.file_name().unwrap().to_string_lossy().into_owned();
    let outside = dir.with_file_name(format!("{name}-outside.txt"));
    fs::write(&outside, "外のファイル").unwrap();
    let mut ws = Workspace::open(&dir).unwrap();
    // 開いた後に外で足された章は、知らせを受けるまで開いていない。
    write(&dir, "story/c.scn", "## c #s3\n");
    let relative = format!("../{name}-outside.txt");
    let absolute = outside.to_string_lossy().into_owned();
    for path in [
        relative.as_str(),
        absolute.as_str(),
        "project.toml",
        "story/c.scn",
        "story/../story/a.scn",
    ] {
        assert!(ws.reload_from_disk(path).is_err(), "{path}");
        assert!(ws.keep_mine(path, "書き込み").is_err(), "{path}");
        assert!(!ws.set_buffer(path, "書き込み"), "{path}");
        assert!(ws.save(path, None, &mut generator()).is_err(), "{path}");
    }
    let paths: Vec<&str> = ws.project().files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["story/a.scn", "story/b.scn"]);
    assert_eq!(fs::read_to_string(&outside).unwrap(), "外のファイル");
    assert_eq!(
        fs::read_to_string(dir.join("project.toml")).unwrap(),
        "format_version = 0\n"
    );
    let _ = fs::remove_file(&outside);
    let _ = fs::remove_dir_all(&dir);
}

/// 外で消された章を、コアが閉じた後でも（アプリにまだ送っていない編集があったとき）、
/// 「自分の変更を残す」で開き直し、保存でファイルを作り直せる。「閉じる」（読み込み直す）は閉じたままにする。
#[test]
fn keep_mine_reopens_removed_file() {
    let dir = temp_project(
        "reopen",
        &[
            ("project.toml", TOML),
            ("story/a.scn", b"## a #s1\n"),
            ("story/b.scn", b"## b #s2\n"),
        ],
    );
    let mut ws = Workspace::open(&dir).unwrap();
    fs::remove_file(dir.join("story/a.scn")).unwrap();
    assert_eq!(
        ws.external_change(&dir.join("story/a.scn")),
        Change::Removed {
            file: "story/a.scn".into()
        }
    );
    assert_eq!(
        ws.keep_mine("story/a.scn", "## a #s1\n残す\n").unwrap(),
        None
    );
    assert!(ws.is_dirty("story/a.scn"));
    let saved = save(&mut ws, "story/a.scn");
    assert_eq!(saved.assigned.len(), 1);
    assert_eq!(
        fs::read_to_string(dir.join("story/a.scn")).unwrap(),
        saved.text
    );

    fs::remove_file(dir.join("story/b.scn")).unwrap();
    ws.external_change(&dir.join("story/b.scn"));
    assert_eq!(ws.reload_from_disk("story/b.scn").unwrap(), None);
    assert_eq!(ws.text("story/b.scn"), None);
    let _ = fs::remove_dir_all(&dir);
}

/// 読めない本文のファイル（UTF-8でない、書いている途中など）は、消されたとは扱わない。取り込まずに知らせる。
#[test]
fn unreadable_file_is_not_removed() {
    let dir = temp_project(
        "unreadable",
        &[("project.toml", TOML), ("story/a.scn", b"## a #s1\n")],
    );
    let mut ws = Workspace::open(&dir).unwrap();
    fs::write(dir.join("story/a.scn"), [0x82, 0xa0, 0x0a]).unwrap();
    assert_eq!(
        ws.external_change(&dir.join("story/a.scn")),
        Change::Unreadable {
            file: "story/a.scn".into()
        }
    );
    assert_eq!(ws.text("story/a.scn"), Some("## a #s1\n"));
    assert!(ws.reload_from_disk("story/a.scn").is_err());
    assert_eq!(ws.text("story/a.scn"), Some("## a #s1\n"));
    let _ = fs::remove_dir_all(&dir);
}

/// まだ取り込んでいない外部の変更（監視の知らせが届く前）の上には書かない。衝突にして外の中身を退避し、
/// 後から届く知らせでは、もう聞かない。「自分の変更を残す」の後なら書ける。
#[test]
fn save_does_not_overwrite_unseen_external_change() {
    let dir = temp_project(
        "unseen",
        &[
            ("project.toml", TOML),
            ("story/a.scn", b"## a #s1\n"),
            ("story/b.scn", b"## b #s2\n"),
        ],
    );
    let mut ws = Workspace::open(&dir).unwrap();
    ws.set_buffer("story/a.scn", "## a #s1\n自分の編集 ^aaaa\n");
    write(&dir, "story/a.scn", "## a #s1\n外の編集 ^aaab\n");
    assert_eq!(
        ws.save("story/a.scn", Some("## a #s1\n"), &mut generator())
            .unwrap(),
        SaveOutcome::Conflict { deleted: false }
    );
    assert_eq!(
        fs::read_to_string(dir.join("story/a.scn")).unwrap(),
        "## a #s1\n外の編集 ^aaab\n"
    );
    // 後から届いた知らせでは、もう聞かない。選ぶまでは、何度保存しても書かない。
    assert_eq!(ws.external_change(&dir.join("story/a.scn")), Change::None);
    assert_eq!(
        ws.save("story/a.scn", Some("## a #s1\n"), &mut generator())
            .unwrap(),
        SaveOutcome::Conflict { deleted: false }
    );
    // 自分の変更を残すと、書ける。
    ws.keep_mine("story/a.scn", "## a #s1\n自分の編集 ^aaaa\n")
        .unwrap();
    assert_eq!(
        save(&mut ws, "story/a.scn").text,
        "## a #s1\n自分の編集 ^aaaa\n"
    );

    // 外で消されたのに、まだ知らせが届いていない。
    ws.set_buffer("story/b.scn", "## b #s2\n編集 ^aaac\n");
    fs::remove_file(dir.join("story/b.scn")).unwrap();
    assert_eq!(
        ws.save("story/b.scn", Some("## b #s2\n"), &mut generator())
            .unwrap(),
        SaveOutcome::Conflict { deleted: true }
    );
    assert!(!dir.join("story/b.scn").exists());
    let _ = fs::remove_dir_all(&dir);
}

/// コアが知っているディスクの中身（アプリの画面が受け取っているはずのもの）。
fn disk_of(ws: &Workspace, file: &str) -> Option<String> {
    ws.disk_text(file).map(str::to_owned)
}

/// コアが外部の変更を取り込んだのに、画面がまだその知らせを受け取っていない（古いディスクの中身を知っている）
/// ときの保存は、書かずに衝突にする。
#[test]
fn save_with_stale_view_of_disk_is_a_conflict() {
    let dir = temp_project(
        "stale",
        &[("project.toml", TOML), ("story/a.scn", b"## a #s1\n")],
    );
    let mut ws = Workspace::open(&dir).unwrap();
    write(&dir, "story/a.scn", "## a #s1\n外の編集 ^aaab\n");
    assert!(matches!(
        ws.external_change(&dir.join("story/a.scn")),
        Change::Reloaded { .. }
    ));
    ws.set_buffer("story/a.scn", "## a #s1\n古い本文の編集 ^aaaa\n");
    assert_eq!(
        ws.save("story/a.scn", Some("## a #s1\n"), &mut generator())
            .unwrap(),
        SaveOutcome::Conflict { deleted: false }
    );
    assert_eq!(
        fs::read_to_string(dir.join("story/a.scn")).unwrap(),
        "## a #s1\n外の編集 ^aaab\n"
    );
    // 知らせを受け取った後（今のディスクの中身を知っている）なら書ける。
    assert_eq!(
        save(&mut ws, "story/a.scn").text,
        "## a #s1\n古い本文の編集 ^aaaa\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// 未登録の話者を、設定DBのキャラクターとして登録する（D047）。IDは英数字と `_` だけで、
/// 同じIDがあれば断る。登録すると、話者として解決できるようになる。
#[test]
fn register_character() {
    let dir = temp_project(
        "register",
        &[
            ("project.toml", TOML),
            ("story/a.scn", "## a #s1\n店員「いらっしゃい」\n".as_bytes()),
        ],
    );
    let mut ws = Workspace::open(&dir).unwrap();
    for bad in ["", "店員", "a b", "../x", "a/b"] {
        assert!(ws.register_character(bad, "店員").is_err(), "{bad}");
    }
    assert!(ws.register_character("clerk", "").is_err());
    let file = ws.register_character("clerk", "店員: 夜").unwrap();
    assert_eq!(file, "bible/entries/character/clerk.md");
    assert_eq!(
        fs::read_to_string(dir.join(&file)).unwrap(),
        "---\nid: clerk\nname: \"店員: 夜\"\n---\n"
    );
    let character = ws
        .project()
        .characters
        .iter()
        .find(|c| c.id == "clerk")
        .unwrap();
    assert_eq!(character.name, "店員: 夜");
    assert!(ws.register_character("clerk", "別の人").is_err());
    let _ = fs::remove_dir_all(&dir);
}

fn write(dir: &Path, path: &str, text: &str) {
    fs::write(dir.join(path), text).unwrap();
}

/// 完了条件：外部の変更の取り込み。保存していない変更が無ければ読み込み直す。
/// 自分で保存した変更は、外部の変更として扱わない。
#[test]
fn external_change_without_unsaved_edits() {
    let dir = temp_project(
        "ext-clean",
        &[("project.toml", TOML), ("story/a.scn", b"## a #s1\n")],
    );
    let mut ws = Workspace::open(&dir).unwrap();
    write(&dir, "story/a.scn", "## a #s1\n外で書いた ^aaaa\n");
    assert_eq!(
        ws.external_change(&dir.join("story/a.scn")),
        Change::Reloaded {
            file: "story/a.scn".into(),
            text: "## a #s1\n外で書いた ^aaaa\n".into()
        }
    );
    assert_eq!(ws.text("story/a.scn"), Some("## a #s1\n外で書いた ^aaaa\n"));
    assert!(!ws.is_dirty("story/a.scn"));
    // 同じ中身の知らせ（同じ変更の2回目）は無視する。
    assert_eq!(ws.external_change(&dir.join("story/a.scn")), Change::None);

    // 自分で保存した変更は無視する。
    ws.set_buffer("story/a.scn", "## a #s1\n自分で書いた ^aaab\n");
    save(&mut ws, "story/a.scn");
    assert_eq!(ws.external_change(&dir.join("story/a.scn")), Change::None);
    let _ = fs::remove_dir_all(&dir);
}

/// 完了条件：保存していない変更があれば、読み込み直さずに、選ばせる（衝突）。
/// 「読み込み直す」は外の中身にし、「自分の変更を残す」は保存していない変更を残す（同じ外の中身ではもう聞かない）。
#[test]
fn external_change_with_unsaved_edits() {
    let dir = temp_project(
        "ext-dirty",
        &[("project.toml", TOML), ("story/a.scn", b"## a #s1\n")],
    );
    let mut ws = Workspace::open(&dir).unwrap();
    ws.set_buffer("story/a.scn", "## a #s1\n自分の編集 ^aaaa\n");
    write(&dir, "story/a.scn", "## a #s1\n外の編集 ^aaab\n");
    assert_eq!(
        ws.external_change(&dir.join("story/a.scn")),
        Change::Conflict {
            file: "story/a.scn".into(),
            deleted: false
        }
    );
    // まだ決めていないので、本文は自分の編集のまま。
    assert_eq!(ws.text("story/a.scn"), Some("## a #s1\n自分の編集 ^aaaa\n"));

    // 自分の変更を残す：保存していない変更のまま。同じ外の中身では、もう聞かない。
    // ディスクの中身（外の中身）を返す。
    assert_eq!(
        ws.keep_mine("story/a.scn", "## a #s1\n自分の編集 ^aaaa\n")
            .unwrap()
            .as_deref(),
        Some("## a #s1\n外の編集 ^aaab\n")
    );
    assert!(ws.is_dirty("story/a.scn"));
    assert_eq!(ws.external_change(&dir.join("story/a.scn")), Change::None);

    // また外で変わったら、また聞く。読み込み直すと、外の中身になる。
    write(&dir, "story/a.scn", "## a #s1\n外の2回目 ^aaac\n");
    assert!(matches!(
        ws.external_change(&dir.join("story/a.scn")),
        Change::Conflict { .. }
    ));
    assert_eq!(
        ws.reload_from_disk("story/a.scn").unwrap().as_deref(),
        Some("## a #s1\n外の2回目 ^aaac\n")
    );
    assert!(!ws.is_dirty("story/a.scn"));
    let _ = fs::remove_dir_all(&dir);
}

/// 外で足された章は加え、消された章は（保存していない変更が無ければ）除く。変更があれば選ばせる。
#[test]
fn external_added_and_removed_files() {
    let dir = temp_project(
        "ext-files",
        &[
            ("project.toml", TOML),
            ("story/a.scn", b"## a #s1\n"),
            ("story/b.scn", b"## b #s2\n"),
        ],
    );
    let mut ws = Workspace::open(&dir).unwrap();
    write(&dir, "story/c.scn", "## c #s3\n");
    assert_eq!(
        ws.external_change(&dir.join("story/c.scn")),
        Change::Added {
            file: "story/c.scn".into(),
            text: "## c #s3\n".into()
        }
    );
    let paths: Vec<&str> = ws.project().files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["story/a.scn", "story/b.scn", "story/c.scn"]);

    fs::remove_file(dir.join("story/b.scn")).unwrap();
    assert_eq!(
        ws.external_change(&dir.join("story/b.scn")),
        Change::Removed {
            file: "story/b.scn".into()
        }
    );
    ws.set_buffer("story/a.scn", "## a #s1\n編集\n");
    fs::remove_file(dir.join("story/a.scn")).unwrap();
    assert_eq!(
        ws.external_change(&dir.join("story/a.scn")),
        Change::Conflict {
            file: "story/a.scn".into(),
            deleted: true
        }
    );
    // 自分の変更を残すと、保存でファイルを作り直す。
    assert_eq!(
        ws.keep_mine("story/a.scn", "## a #s1\n編集\n").unwrap(),
        None
    );
    save(&mut ws, "story/a.scn");
    assert!(dir.join("story/a.scn").is_file());
    // 下のフォルダの章は本文として取り込む（T2-10、D061）。ほかの拡張子、一時ファイルは本文として扱わない。
    fs::create_dir_all(dir.join("story/sub")).unwrap();
    write(&dir, "story/sub/d.scn", "## d #s4\n");
    write(&dir, "story/.a.scn.123.tmp", "x");
    assert_eq!(
        ws.external_change(&dir.join("story/sub/d.scn")),
        Change::Added {
            file: "story/sub/d.scn".into(),
            text: "## d #s4\n".into()
        }
    );
    assert_eq!(
        ws.external_change(&dir.join("story/.a.scn.123.tmp")),
        Change::None
    );
    let _ = fs::remove_dir_all(&dir);
}

/// 設定ファイルが変われば、設定を読み直す。保存していない本文は残す。
#[test]
fn external_settings_change() {
    let dir = temp_project(
        "ext-settings",
        &[("project.toml", TOML), ("story/a.scn", b"## a #s1\n")],
    );
    let mut ws = Workspace::open(&dir).unwrap();
    ws.set_buffer("story/a.scn", "## a #s1\n編集\n");
    write(&dir, "variables.yaml", "flag.a:\n  type: int\n");
    assert_eq!(
        ws.external_change(&dir.join("variables.yaml")),
        Change::Settings
    );
    assert_eq!(ws.project().variables.len(), 1);
    assert_eq!(ws.text("story/a.scn"), Some("## a #s1\n編集\n"));
    assert!(ws.is_dirty("story/a.scn"));
    // AIタスクの定義（`ai/` の直下の `*.yaml`。T3-6）を足す・直すと読み直す。下のフォルダは読まない。
    fs::create_dir_all(dir.join("ai/proposals")).unwrap();
    write(&dir, "ai/tone.yaml", "name: 口調\nprompt: p\n");
    assert_eq!(
        ws.external_change(&dir.join("ai/tone.yaml")),
        Change::Settings
    );
    assert_eq!(ws.project().ai_tasks.len(), 1);
    assert_eq!(ws.text("story/a.scn"), Some("## a #s1\n編集\n"));
    write(&dir, "ai/proposals/x.yaml", "a: 1\n");
    // 提案のファイル（T3-8）は、提案の一覧を読み直す知らせ。
    write(&dir, "ai/proposals/x.md", "## ^aaaa\n");
    assert_eq!(
        ws.external_change(&dir.join("ai/proposals/x.md")),
        Change::Proposals
    );
    assert_eq!(
        ws.external_change(&dir.join("ai/proposals/x.yaml")),
        Change::None
    );
    // 作品フォルダの外や、本文でも設定でもないファイルは無視する。
    write(&dir, "FORMAT.md", "x");
    assert_eq!(ws.external_change(&dir.join("FORMAT.md")), Change::None);
    assert_eq!(
        ws.external_change(Path::new("/elsewhere/x.scn")),
        Change::None
    );
    let _ = fs::remove_dir_all(&dir);
}

/// 章を足す。名前に `.scn` を補い、あるファイルや、フォルダを含む名前は断る。
#[test]
fn add_chapter() {
    let dir = temp_project(
        "chapter",
        &[("project.toml", TOML), ("story/a.scn", b"## a #s1\n")],
    );
    let mut ws = Workspace::open(&dir).unwrap();
    assert_eq!(ws.add_chapter("chapter02").unwrap(), "story/chapter02.scn");
    assert!(dir.join("story/chapter02.scn").is_file());
    assert_eq!(ws.text("story/chapter02.scn"), Some(""));
    // 自分で作ったファイルは、外部の変更として扱わない。
    assert_eq!(
        ws.external_change(&dir.join("story/chapter02.scn")),
        Change::None
    );
    assert!(ws.add_chapter("a.scn").is_err());
    assert!(ws.add_chapter("../x").is_err());
    assert!(ws.add_chapter("").is_err());
    let _ = fs::remove_dir_all(&dir);
}

// ---- 設定DBと変数（T1-10、D049） ----

const CHARACTER_TYPE: &[u8] = "name: キャラクター\nbuiltin: character\nfields:\n  - { key: aliases, label: 呼び名, type: list<text> }\n  - { key: speech, label: 口調メモ, type: longtext }\n".as_bytes();
const AKARI: &str = "---\n# 主人公の幼なじみ\nid: akari\nname: 灯   # 表示名\naliases: [あかり]\nspeech: 明るい\n---\n本文。\n";

fn bible_project(name: &str) -> PathBuf {
    temp_project(
        name,
        &[
            ("project.toml", TOML),
            ("bible/types/character.yaml", CHARACTER_TYPE),
            ("bible/types/place.yaml", "name: 場所\n".as_bytes()),
            ("bible/entries/character/akari.md", AKARI.as_bytes()),
            ("story/a.scn", "## a #s1\nあかり「やあ」 ^aaaa\n".as_bytes()),
        ],
    )
}

const AKARI_FILE: &str = "bible/entries/character/akari.md";

/// 項目を直すと、その項目の値だけが変わり、設定を読み直す。自分で書いた変更の知らせは無視する。
#[test]
fn set_entry_field_writes_only_that_key() {
    let dir = bible_project("entry-field");
    let mut ws = Workspace::open(&dir).unwrap();
    let edit = ws
        .set_entry_field(
            AKARI_FILE,
            "aliases",
            &serde_json::json!(["あかり", "灯ちゃん"]),
            AKARI,
        )
        .unwrap();
    let written = AKARI.replace("aliases: [あかり]", "aliases: [あかり, 灯ちゃん]");
    assert_eq!(
        edit,
        EntryEdit::Saved {
            text: written.clone()
        }
    );
    assert_eq!(fs::read_to_string(dir.join(AKARI_FILE)).unwrap(), written);
    let akari = &ws.project().characters[0];
    assert_eq!(akari.aliases, ["あかり", "灯ちゃん"]);
    assert_eq!(ws.external_change(&dir.join(AKARI_FILE)), Change::None);
    // その後の外の変更は拾う。外で自分の書いた中身に戻されたとき（取り消し）も拾う。
    write(&dir, AKARI_FILE, AKARI);
    assert_eq!(ws.external_change(&dir.join(AKARI_FILE)), Change::Settings);
    write(&dir, AKARI_FILE, &written);
    assert_eq!(ws.external_change(&dir.join(AKARI_FILE)), Change::Settings);
    let _ = fs::remove_dir_all(&dir);
}

/// 外で変わっていても、直した項目が外で変わっていなければ、外の中身に重ねて書く。直した項目も外で
/// 変わっていれば、書かずに今の中身を返す。
#[test]
fn set_entry_field_merges_or_conflicts() {
    let dir = bible_project("entry-merge");
    let mut ws = Workspace::open(&dir).unwrap();
    let outside = AKARI.replace("speech: 明るい", "speech: 外で直した");
    write(&dir, AKARI_FILE, &outside);
    let edit = ws
        .set_entry_field(AKARI_FILE, "name", &serde_json::json!("あかり"), AKARI)
        .unwrap();
    let merged = outside.replace("name: 灯 ", "name: あかり ");
    assert_eq!(
        edit,
        EntryEdit::Saved {
            text: merged.clone()
        }
    );
    let edit = ws
        .set_entry_field(AKARI_FILE, "speech", &serde_json::json!("静か"), AKARI)
        .unwrap();
    assert_eq!(
        edit,
        EntryEdit::Conflict {
            text: merged.clone()
        }
    );
    assert_eq!(fs::read_to_string(dir.join(AKARI_FILE)).unwrap(), merged);
    let _ = fs::remove_dir_all(&dir);
}

/// フォームで書き換えられない書き方なら、書かずに知らせる。エントリでないパスは読み書きしない。
#[test]
fn entry_edits_refuse_unsupported_and_other_paths() {
    let dir = bible_project("entry-refuse");
    let flow = "---\n{id: akari, name: 灯}\n---\n";
    write(&dir, AKARI_FILE, flow);
    let mut ws = Workspace::open(&dir).unwrap();
    assert!(matches!(
        ws.set_entry_field(AKARI_FILE, "name", &serde_json::json!("x"), flow)
            .unwrap(),
        EntryEdit::Unsupported { .. }
    ));
    assert_eq!(fs::read_to_string(dir.join(AKARI_FILE)).unwrap(), flow);
    for bad in [
        "story/a.scn",
        "project.toml",
        "bible/types/character.yaml",
        "bible/entries/character/../../project.toml",
        "bible/entries/../x.md",
        "bible/entries/character/.hidden.md",
        "/etc/passwd",
    ] {
        assert!(ws.write_entry_source(bad, "x", None).is_err(), "{bad}");
    }
    assert_eq!(
        fs::read_to_string(dir.join("project.toml"))
            .unwrap()
            .as_bytes(),
        TOML
    );
    let _ = fs::remove_dir_all(&dir);
}

/// 本文とソースの書き換え。本文は外で本文が変わっていなければ重ねて書き、ソースは外で変わっていれば
/// 書かない（上書きを選べば書く）。改行は今のファイルにそろえる。
#[test]
fn entry_body_and_source() {
    let dir = bible_project("entry-body");
    let crlf = AKARI.replace('\n', "\r\n");
    write(&dir, AKARI_FILE, &crlf);
    let mut ws = Workspace::open(&dir).unwrap();
    let edit = ws
        .set_entry_body(AKARI_FILE, "新しい本文\n", &crlf)
        .unwrap();
    let EntryEdit::Saved { text } = edit else {
        panic!("{edit:?}");
    };
    assert!(text.ends_with("---\r\n新しい本文\r\n"));
    assert!(text.starts_with("---\r\n# 主人公の幼なじみ\r\n"));
    // 古い中身を知っているまま本文を直すと、本文が外（今）と違うので衝突。
    assert!(matches!(
        ws.set_entry_body(AKARI_FILE, "別の本文", &crlf).unwrap(),
        EntryEdit::Conflict { .. }
    ));
    assert!(matches!(
        ws.write_entry_source(AKARI_FILE, "---\nid: akari\nname: 灯\n---\n", Some(&crlf))
            .unwrap(),
        EntryEdit::Conflict { .. }
    ));
    let edit = ws
        .write_entry_source(AKARI_FILE, "---\nid: akari\nname: 灯\n---\n", None)
        .unwrap();
    assert_eq!(
        edit,
        EntryEdit::Saved {
            text: "---\r\nid: akari\r\nname: 灯\r\n---\r\n".to_owned()
        }
    );
    let _ = fs::remove_dir_all(&dir);
}

/// エントリを新しく作る。型の定義のある型だけ。IDは英数字と `_`。同じIDは断る。
#[test]
fn create_entry() {
    let dir = bible_project("entry-create");
    let mut ws = Workspace::open(&dir).unwrap();
    assert!(ws.create_entry("item", "key", "鍵").is_err());
    assert!(ws.create_entry("../x", "key", "鍵").is_err());
    assert!(ws.create_entry("place", "屋上", "屋上").is_err());
    let file = ws.create_entry("place", "rooftop", "屋上").unwrap();
    assert_eq!(file, "bible/entries/place/rooftop.md");
    assert_eq!(
        fs::read_to_string(dir.join(&file)).unwrap(),
        "---\nid: rooftop\nname: 屋上\n---\n"
    );
    assert!(ws.project().bible.entries.iter().any(|e| e.id == "rooftop"));
    assert!(ws.create_entry("place", "rooftop", "別").is_err());
    assert_eq!(ws.external_change(&dir.join(&file)), Change::None);
    // 素のままでは数や null として読まれるIDは、引用して書く（ファイル名と id がそろう）。
    for id in ["0x1F", "1e3", "null", "true", "001"] {
        let file = ws.create_entry("place", id, "場所").unwrap();
        let entry = ws
            .project()
            .bible
            .entries
            .iter()
            .find(|e| e.path == file)
            .unwrap();
        assert_eq!(entry.fields.get("id"), Some(&serde_json::json!(id)), "{id}");
    }
    let _ = fs::remove_dir_all(&dir);
}

/// `variables.yaml` をテキストで直す。外で変わっていれば書かない。設定を読み直す。
#[test]
fn write_variables() {
    let dir = bible_project("variables");
    let mut ws = Workspace::open(&dir).unwrap();
    assert_eq!(ws.variables_text().unwrap(), None);
    let edit = ws
        .write_variables("flag.a:\n  type: int\n  default: 1\n", Some(""))
        .unwrap();
    assert!(matches!(edit, EntryEdit::Saved { .. }));
    assert_eq!(ws.project().variables[0].default.as_deref(), Some("1"));
    assert_eq!(
        ws.external_change(&dir.join("variables.yaml")),
        Change::None
    );
    assert!(matches!(
        ws.write_variables("x", Some("")).unwrap(),
        EntryEdit::Conflict { .. }
    ));
    let _ = fs::remove_dir_all(&dir);
}

/// エントリのファイルの一覧は、読めないものも含める（ソース表示で直せるように）。
#[test]
fn entry_files_include_unreadable_entries() {
    let dir = bible_project("entry-files");
    write(
        &dir,
        "bible/entries/character/broken.md",
        "---\nid: [\n---\n",
    );
    fs::write(dir.join("bible/entries/character/latin1.md"), b"\xff").unwrap();
    let ws = Workspace::open(&dir).unwrap();
    let files = ws.entry_files();
    assert_eq!(
        files
            .iter()
            .map(|(p, t)| (p.as_str(), t.is_some()))
            .collect::<Vec<_>>(),
        [
            (AKARI_FILE, true),
            ("bible/entries/character/broken.md", true),
            ("bible/entries/character/latin1.md", false),
        ]
    );
    let _ = fs::remove_dir_all(&dir);
}

/// 素材カタログ（T2-3、D054）：意味名を足し、ファイルを割り当てる。書いた中身の監視の知らせは無視し、
/// 設定を読み直す（I403 が消える）。`assets/` の外のファイルは割り当てない。
#[test]
fn catalog_edits() {
    use scn_core::workspace::{AssetKey, CatalogEdit};
    let dir = temp_project(
        "catalog",
        &[
            ("project.toml", TOML),
            ("story/a.scn", "## a #s1\n@背景 屋上\n@SE ドア\n".as_bytes()),
            ("assets/files/bg/rooftop.png", b"png"),
            ("outside.png", b"png"),
        ],
    );
    let mut ws = Workspace::open(&dir).unwrap();
    assert_eq!(ws.catalog_text().unwrap(), None);
    let key = |kind: &str, name: &str| AssetKey {
        kind: kind.to_owned(),
        character: None,
        name: name.to_owned(),
    };
    let edit = ws
        .add_catalog_entries(&[key("background", "屋上"), key("se", "ドア")])
        .unwrap();
    assert_eq!(
        edit,
        CatalogEdit::Saved {
            text: "backgrounds:\n  屋上: {}\nse:\n  ドア: {}\n".to_owned()
        }
    );
    assert!(ws.project().catalog.backgrounds.contains("屋上"));
    let catalog = dir.join("assets/catalog.yaml");
    assert_eq!(ws.external_change(&catalog), Change::None);
    assert_eq!(
        ws.add_catalog_entries(&[key("se", "ドア")]).unwrap(),
        CatalogEdit::Unchanged
    );

    let edit = ws
        .assign_asset_file(
            &key("background", "屋上"),
            &dir.join("assets/files/bg/rooftop.png"),
        )
        .unwrap();
    assert!(matches!(edit, CatalogEdit::Saved { .. }), "{edit:?}");
    assert_eq!(
        fs::read_to_string(&catalog).unwrap(),
        "backgrounds:\n  屋上: { file: files/bg/rooftop.png }\nse:\n  ドア: {}\n"
    );
    assert_eq!(ws.external_change(&catalog), Change::None);
    assert!(matches!(
        ws.assign_asset_file(&key("background", "屋上"), &dir.join("outside.png"))
            .unwrap(),
        CatalogEdit::Unsupported { .. }
    ));
    // 設定DBに無いキャラクターの差分は足さない。
    let sprite = AssetKey {
        kind: "expression".to_owned(),
        character: Some("nobody".to_owned()),
        name: "笑顔".to_owned(),
    };
    assert!(matches!(
        ws.add_catalog_entries(&[sprite]).unwrap(),
        CatalogEdit::Unsupported { .. }
    ));
    // 外で変わった中身にも、足すだけなので重ねて書く。
    write(
        &dir,
        "assets/catalog.yaml",
        "# 人が書いた\nbackgrounds:\n  屋上: {}\n",
    );
    assert_eq!(ws.external_change(&catalog), Change::Settings);
    let edit = ws.add_catalog_entries(&[key("se", "ドア")]).unwrap();
    assert_eq!(
        edit,
        CatalogEdit::Saved {
            text: "# 人が書いた\nbackgrounds:\n  屋上: {}\nse:\n  ドア: {}\n".to_owned()
        }
    );
    let _ = fs::remove_dir_all(&dir);
}

/// `story/` の下のフォルダ（T2-10、D061）：章を足す、外で足された章を取り込む、同じフォルダの中で名前を変える。
/// 隠しフォルダの中は取り込まない。
#[test]
fn story_subfolders() {
    let dir = temp_project(
        "subfolders",
        &[("project.toml", TOML), ("story/a.scn", b"## a #s1\n")],
    );
    let mut ws = Workspace::open(&dir).unwrap();
    assert_eq!(
        ws.add_chapter("part1/ch01").unwrap(),
        "story/part1/ch01.scn"
    );
    assert!(dir.join("story/part1/ch01.scn").exists());
    for bad in ["../x", "part1/.x", "part1//x", "", ".x"] {
        assert!(ws.add_chapter(bad).is_err(), "{bad}");
    }
    fs::create_dir_all(dir.join("story/part2")).unwrap();
    fs::write(dir.join("story/part2/ch02.scn"), "## b #s2\n").unwrap();
    assert_eq!(
        ws.external_change(&dir.join("story/part2/ch02.scn")),
        Change::Added {
            file: "story/part2/ch02.scn".to_owned(),
            text: "## b #s2\n".to_owned()
        }
    );
    fs::create_dir_all(dir.join("story/.trash")).unwrap();
    fs::write(dir.join("story/.trash/x.scn"), "## x #s9\n").unwrap();
    assert_eq!(
        ws.external_change(&dir.join("story/.trash/x.scn")),
        Change::None
    );
    let to = ws.rename_chapter("story/part2/ch02.scn", "ch03").unwrap();
    assert_eq!(to, "story/part2/ch03.scn");
    assert!(ws.rename_chapter("story/part2/ch03.scn", "x/y").is_err());
    // フォルダごとの変更（中のファイルの知らせが来ないことがある）：フォルダの知らせで、下の章をまとめて調べる。
    fs::rename(dir.join("story/part2"), dir.join("story/part3")).unwrap();
    assert_eq!(
        ws.external_changes(&dir.join("story/part2")),
        [Change::Removed {
            file: "story/part2/ch03.scn".to_owned()
        }]
    );
    assert_eq!(
        ws.external_changes(&dir.join("story/part3")),
        [Change::Added {
            file: "story/part3/ch03.scn".to_owned(),
            text: "## b #s2\n".to_owned()
        }]
    );
    // ファイルの知らせは今までどおり。隠しフォルダは見ない。
    assert!(ws.external_changes(&dir.join("story/.trash")).is_empty());
    assert!(
        ws.external_changes(&dir.join("story/part3/ch03.scn"))
            .is_empty()
    );
    let _ = fs::remove_dir_all(&dir);
}

/// 読めない下のフォルダは、E801 で知らせて読み飛ばす（T2-10、D061）。
#[cfg(unix)]
#[test]
fn unreadable_story_subfolder() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_project(
        "subfolder-unreadable",
        &[
            ("project.toml", TOML),
            ("story/a.scn", b"## a #s1\n"),
            ("story/locked/b.scn", b"## b #s2\n"),
        ],
    );
    let locked = dir.join("story/locked");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    // 権限を無視できる（root など）環境では確かめられない。
    if fs::read_dir(&locked).is_ok() {
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        let _ = fs::remove_dir_all(&dir);
        return;
    }
    let project = scn_core::load_project(&dir);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    let project = project.unwrap();
    let paths: Vec<&str> = project.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["story/a.scn"]);
    assert!(
        project
            .load_diagnostics
            .iter()
            .any(|d| d.diag.code == scn_core::diag::Code::E801 && d.file == "story/locked"),
        "{:?}",
        project.load_diagnostics
    );
    let _ = fs::remove_dir_all(&dir);
}
