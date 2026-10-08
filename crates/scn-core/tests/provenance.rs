//! 出自の記録（T3-4、D065）：書き足しでほかの行が変わらないこと、アプリで直した AI の行が `ai-edited` になる
//! こと、AI の行の一覧。

use std::fs;
use std::path::PathBuf;

use scn_core::line_id::LineIdGenerator;
use scn_core::provenance::{
    AI, AI_EDITED, PROVENANCE_FILE, Record, append_provenance, provenance_csv, provenance_rows,
    read_provenance,
};
use scn_core::workspace::{SaveOutcome, Workspace};
use scn_core::{analyze, load_project};

fn temp_project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scn-core-prov-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    for (path, text) in files {
        let path = dir.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    dir
}

fn record(line: &str, origin: &str) -> Record {
    Record {
        line: line.to_owned(),
        origin: origin.to_owned(),
        task: "external:claude-code".to_owned(),
        model: "m".to_owned(),
        at: "2026-10-08T00:00:00Z".to_owned(),
    }
}

/// 完了条件：書き足しで、今の中身は1バイトも変わらない（終わりに改行が無ければ足し、改行は今のファイルに
/// そろえる）。ファイルもフォルダも無ければ作る。
#[test]
fn append_keeps_existing_lines() {
    let dir = temp_project("append", &[("project.toml", "format_version = 0\n")]);
    append_provenance(&dir, &[record("aaaa", AI)]).unwrap();
    let path = dir.join(PROVENANCE_FILE);
    let first = fs::read_to_string(&path).unwrap();
    assert_eq!(
        first,
        "{\"line\":\"aaaa\",\"origin\":\"ai\",\"task\":\"external:claude-code\",\"model\":\"m\",\"at\":\"2026-10-08T00:00:00Z\"}\n"
    );
    // 手で書いた行（CRLF、終わりに改行なし、読めない行）はそのまま。
    let hand = "{\"line\":\"bbbb\",\"origin\":\"ai\"}\r\n壊れた行\r\n{\"line\":\"cccc\",\"origin\":\"human\"}";
    fs::write(&path, hand).unwrap();
    append_provenance(&dir, &[record("bbbb", AI_EDITED)]).unwrap();
    let after = fs::read_to_string(&path).unwrap();
    assert!(after.starts_with(&format!("{hand}\r\n")), "{after}");
    assert!(after.ends_with("\"at\":\"2026-10-08T00:00:00Z\"}\r\n"));
    let read = read_provenance(&dir).unwrap();
    assert_eq!(read.records.len(), 3);
    assert_eq!(read.problems.len(), 1);
    assert_eq!(read.latest()["bbbb"].origin, AI_EDITED);
    let _ = fs::remove_dir_all(&dir);
}

/// 完了条件：アプリで直した AI の行は、保存で `ai-edited` が書き足される。人の行と、もう `ai-edited` の行には
/// 書かない。直していない AI の行にも書かない。
#[test]
fn saving_edited_ai_lines() {
    let text = "## a #s1\n灯「AIの台詞」 ^aaaa\n　AIの地の文。 ^aaab\n　人の地の文。 ^aaac\n";
    let dir = temp_project(
        "save",
        &[
            ("project.toml", "format_version = 0\n"),
            ("story/a.scn", text),
            (
                PROVENANCE_FILE,
                "{\"line\":\"aaaa\",\"origin\":\"ai\",\"task\":\"draft\",\"model\":\"m1\",\"at\":\"x\"}\n{\"line\":\"aaab\",\"origin\":\"ai\",\"task\":\"draft\",\"model\":\"m1\",\"at\":\"x\"}\n",
            ),
        ],
    );
    let mut ws = Workspace::open(&dir).unwrap();
    let save = |ws: &mut Workspace, edited: &str, seen: &str| {
        ws.set_buffer("story/a.scn", edited);
        match ws
            .save(
                "story/a.scn",
                Some(seen),
                &mut LineIdGenerator::with_seed(1),
            )
            .unwrap()
        {
            SaveOutcome::Saved(saved) => saved,
            SaveOutcome::Conflict { .. } => panic!("衝突"),
        }
    };
    let edited = text
        .replace("AIの台詞", "人が直した台詞")
        .replace("人の地の文", "人が直した地の文");
    let saved = save(&mut ws, &edited, text);
    assert_eq!(saved.provenance, ["aaaa"]);
    assert_eq!(saved.provenance_problem, None);
    let read = read_provenance(&dir).unwrap();
    let last = read.records.last().unwrap();
    assert_eq!(
        (
            last.line.as_str(),
            last.origin.as_str(),
            last.task.as_str(),
            last.model.as_str()
        ),
        ("aaaa", AI_EDITED, "draft", "m1")
    );
    // もう ai-edited の行を直しても、書き足さない。
    let again = edited.replace("人が直した台詞", "もう一度直した");
    let saved = save(&mut ws, &again, &edited);
    assert!(saved.provenance.is_empty());
    assert_eq!(read_provenance(&dir).unwrap().records.len(), 3);
    let _ = fs::remove_dir_all(&dir);
}

/// AI の行の一覧：本文にある行だけ、今の出自で数え、CSV にする。ゲートの短編は、全部の台詞と地の文が AI。
#[test]
fn rows_and_csv() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/amayadori");
    let project = load_project(&dir).unwrap();
    let ir = analyze(&project);
    let provenance = read_provenance(&dir).unwrap();
    assert!(provenance.problems.is_empty());
    let (rows, counts) = provenance_rows(&project, &ir, &provenance);
    assert_eq!(counts.ai, rows.len());
    assert_eq!(counts.ai_edited, 0);
    assert!(counts.ai > 0 && counts.ai <= counts.lines);
    let first = &rows[0];
    assert!(
        !first.text.contains('^') && !first.scene.is_empty(),
        "{first:?}"
    );
    let csv = provenance_csv(&rows);
    assert!(csv.starts_with("\u{feff}行ID,ファイル,シーン,出自,タスク,モデル,日時,本文\r\n"));
    assert!(csv.matches("\r\n").count() > rows.len());
}

/// 外で変わって「自分の変更を残す」をした後に保存しても、外で変わっただけの AI の行は数えない（人が直した
/// 行だけ）。名前の変更（F2）で保存していない変更ごと書くときも、人が直した AI の行を記録する（名前の置き換え
/// そのものは数えない）。
#[test]
fn keep_mine_and_rename() {
    use scn_core::rename::RenameTarget;
    let text = "## a #s1\n灯「AIの台詞」 ^aaaa\n　AIの地の文。 ^aaab\n　人の行。 ^aaac\n-> s2\n\n## b #s2\n@エンディング end1\n";
    let records = "{\"line\":\"aaaa\",\"origin\":\"ai\",\"task\":\"t\",\"model\":\"m\",\"at\":\"x\"}\n{\"line\":\"aaab\",\"origin\":\"ai\",\"task\":\"t\",\"model\":\"m\",\"at\":\"x\"}\n";
    let dir = temp_project(
        "keep",
        &[
            ("project.toml", "format_version = 0\n"),
            ("story/a.scn", text),
            (PROVENANCE_FILE, records),
        ],
    );
    let mut ws = Workspace::open(&dir).unwrap();
    // 人は人の行だけを直す。外で AI の台詞が書き直される。自分の変更を残して保存する。
    let mine = text.replace("人の行", "人が直した行");
    ws.set_buffer("story/a.scn", &mine);
    fs::write(
        dir.join("story/a.scn"),
        text.replace("AIの台詞", "AIが書き直した台詞"),
    )
    .unwrap();
    ws.external_change(&dir.join("story/a.scn"));
    let disk = ws.keep_mine("story/a.scn", &mine).unwrap();
    let saved = match ws
        .save(
            "story/a.scn",
            disk.as_deref(),
            &mut LineIdGenerator::with_seed(1),
        )
        .unwrap()
    {
        SaveOutcome::Saved(saved) => saved,
        SaveOutcome::Conflict { .. } => panic!("衝突"),
    };
    assert!(saved.provenance.is_empty(), "{:?}", saved.provenance);
    assert_eq!(read_provenance(&dir).unwrap().records.len(), 2);

    // 名前の変更：AI の地の文を直して（保存しない）、シーンIDを変える。地の文だけを記録し、置き換わった
    // ジャンプの行（行IDが無い）は数えない。
    let edited = mine.replace("AIの地の文", "人が直した地の文");
    ws.set_buffer("story/a.scn", &edited);
    let renamed = ws
        .rename(
            &RenameTarget::Scene {
                id: "s2".to_owned(),
            },
            "s3",
        )
        .unwrap();
    assert_eq!(renamed.provenance, ["aaab"]);
    let read = read_provenance(&dir).unwrap();
    assert_eq!(read.latest()["aaab"].origin, AI_EDITED);
    // 書いた後は、それが元になる（もう一度保存しても数えない）。
    let saved = match ws
        .save(
            "story/a.scn",
            Some(&fs::read_to_string(dir.join("story/a.scn")).unwrap()),
            &mut LineIdGenerator::with_seed(1),
        )
        .unwrap()
    {
        SaveOutcome::Saved(saved) => saved,
        SaveOutcome::Conflict { .. } => panic!("衝突"),
    };
    assert!(saved.provenance.is_empty());
    let _ = fs::remove_dir_all(&dir);
}

/// コアが読み込み直した後に、アプリにまだ送っていない編集があった競合（アプリが元を覚えている）：その元を
/// `remember_base` で渡せば、外で変わっただけの AI の行は数えない。
#[test]
fn remembered_base_after_reload() {
    let text = "## a #s1\n灯「AIの台詞」 ^aaaa\n　人の行。 ^aaac\n";
    let dir = temp_project(
        "remember",
        &[
            ("project.toml", "format_version = 0\n"),
            ("story/a.scn", text),
            (
                PROVENANCE_FILE,
                "{\"line\":\"aaaa\",\"origin\":\"ai\",\"task\":\"t\",\"model\":\"m\",\"at\":\"x\"}\n",
            ),
        ],
    );
    let mut ws = Workspace::open(&dir).unwrap();
    let external = text.replace("AIの台詞", "外で書き直した台詞");
    fs::write(dir.join("story/a.scn"), &external).unwrap();
    // コアには保存していない変更が無いので、読み込み直す。
    ws.external_change(&dir.join("story/a.scn"));
    // アプリは、まだ送っていない編集（人の行だけを直した）を残す。元はアプリが覚えていた中身。
    let mine = text.replace("人の行", "人が直した行");
    ws.remember_base("story/a.scn", text);
    let disk = ws.keep_mine("story/a.scn", &mine).unwrap();
    let saved = match ws
        .save(
            "story/a.scn",
            disk.as_deref(),
            &mut LineIdGenerator::with_seed(1),
        )
        .unwrap()
    {
        SaveOutcome::Saved(saved) => saved,
        SaveOutcome::Conflict { .. } => panic!("衝突"),
    };
    assert!(saved.provenance.is_empty(), "{:?}", saved.provenance);
    let _ = fs::remove_dir_all(&dir);
}
