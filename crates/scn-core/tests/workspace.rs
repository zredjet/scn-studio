//! 開いている作品フォルダ（アプリ、T1-6）：保存と、外部の変更の取り込み。

use std::fs;
use std::path::{Path, PathBuf};

use scn_core::line_id::LineIdGenerator;
use scn_core::workspace::{Change, SaveOutcome, Saved, Workspace};

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
    // サブディレクトリやほかの拡張子、一時ファイルは本文として扱わない。
    fs::create_dir_all(dir.join("story/sub")).unwrap();
    write(&dir, "story/sub/d.scn", "## d #s4\n");
    write(&dir, "story/.a.scn.123.tmp", "x");
    assert_eq!(
        ws.external_change(&dir.join("story/sub/d.scn")),
        Change::None
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
