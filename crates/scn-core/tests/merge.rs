//! 外部の変更とのマージ（`merge`、T2-7、D058）：3方向のマージ（元＝読み込んだときの中身、自分、外）。行IDのある
//! 行は行IDで、無い行は中身で対応させる。ぶつかった行は両方を残して印を付ける（外の行はコメントにする）。
//! 完了条件：行IDのある行・無い行、ぶつかり、行IDが外で消えた行のテスト。

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use proptest::prelude::*;
use proptest::sample::Index;
use scn_core::line_id::LineIdGenerator;
use scn_core::merge::{Choice, Hunk, merge_scn, merge_text};
use scn_core::workspace::{Change, EntryEdit, SaveOutcome, Workspace};

const BASE: &str =
    "## 屋上 #s1\n灯「やっと来た」 ^aaaa\n　夕日が差す。 ^bbbb\n湊「待たせた」 ^cccc\n";

/// 行IDがあって、コメントでない行の行ID。
fn live_ids(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| !l.trim_start_matches([' ', '\t']).starts_with("//"))
        .filter_map(|l| l.rsplit_once(" ^").map(|(_, id)| id.to_owned()))
        .collect()
}

#[test]
fn edits_on_different_lines_merge_cleanly() {
    let mine = BASE.replace("やっと来た", "やっと来たね");
    let theirs = format!(
        "{}灯「帰ろう」 ^dddd\n",
        BASE.replace("夕日が差す", "夕日が傾く")
    );
    let merged = merge_scn(BASE, &mine, &theirs);
    assert_eq!(merged.conflicts, 0);
    assert_eq!(
        merged.resolve(&[]).unwrap(),
        "## 屋上 #s1\n灯「やっと来たね」 ^aaaa\n　夕日が傾く。 ^bbbb\n湊「待たせた」 ^cccc\n灯「帰ろう」 ^dddd\n"
    );
}

/// 行IDで対応させるので、行の中身が変わっても同じ行として扱う（自分は行を直し、外はその前に行を足した）。
#[test]
fn line_ids_align_edited_lines() {
    let mine = BASE.replace("夕日が差す", "夕日がまぶしい");
    let theirs = BASE.replace(
        "　夕日が差す。 ^bbbb\n",
        "　風が吹く。\n　夕日が差す。 ^bbbb\n",
    );
    let merged = merge_scn(BASE, &mine, &theirs);
    assert_eq!(merged.conflicts, 0);
    assert_eq!(
        merged.resolve(&[]).unwrap(),
        "## 屋上 #s1\n灯「やっと来た」 ^aaaa\n　風が吹く。\n　夕日がまぶしい。 ^bbbb\n湊「待たせた」 ^cccc\n"
    );
}

/// 行IDの無い行は中身で対応させる（自分は前に、外は後ろに地の文を足した）。
#[test]
fn lines_without_ids_align_by_content() {
    let base = "## a #s1\n　一行目\n　二行目\n";
    let mine = "## a #s1\n　前に足した\n　一行目\n　二行目\n";
    let theirs = "## a #s1\n　一行目\n　二行目\n　後ろに足した\n";
    let merged = merge_scn(base, mine, theirs);
    assert_eq!(merged.conflicts, 0);
    assert_eq!(
        merged.resolve(&[]).unwrap(),
        "## a #s1\n　前に足した\n　一行目\n　二行目\n　後ろに足した\n"
    );
    // 同じ行を両方が違うように直した。
    let mine = base.replace("二行目", "二行目（自分）");
    let theirs = base.replace("二行目", "二行目（外）");
    let merged = merge_scn(base, &mine, &theirs);
    assert_eq!(merged.conflicts, 1);
}

/// 同じ行IDの行を両方が違うように直した：ぶつかり。自分・外・両方（外はコメントにして印で囲む）を選べる。
#[test]
fn conflicting_edits() {
    let mine = BASE.replace("やっと来た", "遅いよ");
    let theirs = BASE.replace("やっと来た", "来てくれた");
    let merged = merge_scn(BASE, &mine, &theirs);
    assert_eq!(merged.conflicts, 1);
    let conflict = merged
        .hunks
        .iter()
        .find_map(|h| match h {
            Hunk::Conflict { base, mine, theirs } => Some((base, mine, theirs)),
            Hunk::Merged { .. } => None,
        })
        .unwrap();
    assert_eq!(conflict.0, "灯「やっと来た」 ^aaaa\n");
    assert_eq!(conflict.1, "灯「遅いよ」 ^aaaa\n");
    assert_eq!(conflict.2, "灯「来てくれた」 ^aaaa\n");
    assert_eq!(merged.resolve(&[Choice::Mine]).unwrap(), mine);
    assert_eq!(merged.resolve(&[Choice::Theirs]).unwrap(), theirs);
    let both = merged.resolve(&[Choice::Both]).unwrap();
    assert_eq!(
        both,
        "## 屋上 #s1\n灯「遅いよ」 ^aaaa\n// ▼ マージ：外の変更（コメントにして残した）\n// 灯「来てくれた」 ^aaaa\n// ▲ マージ\n　夕日が差す。 ^bbbb\n湊「待たせた」 ^cccc\n"
    );
    // 行IDは2つにならない。
    assert_eq!(live_ids(&both), ["aaaa", "bbbb", "cccc"]);
    // 選んだ数が合わなければ作らない。
    assert!(merged.resolve(&[]).is_none());
    // 印は既定（両方）と同じ。
    assert_eq!(merged.default_text(), both);
}

/// 行IDのある行が外で消えた：自分が直していなければ消し、直していればぶつかり。
#[test]
fn line_removed_externally() {
    let theirs = BASE.replace("　夕日が差す。 ^bbbb\n", "");
    let merged = merge_scn(BASE, BASE, &theirs);
    assert_eq!(merged.conflicts, 0);
    assert_eq!(merged.resolve(&[]).unwrap(), theirs);
    let mine = BASE.replace("夕日が差す", "夕日が沈む");
    let merged = merge_scn(BASE, &mine, &theirs);
    assert_eq!(merged.conflicts, 1);
    assert_eq!(merged.resolve(&[Choice::Theirs]).unwrap(), theirs);
    assert_eq!(merged.resolve(&[Choice::Mine]).unwrap(), mine);
    assert_eq!(
        merged.resolve(&[Choice::Both]).unwrap(),
        "## 屋上 #s1\n灯「やっと来た」 ^aaaa\n　夕日が沈む。 ^bbbb\n// ▲ マージ：外の変更では、上の行が消されていた\n湊「待たせた」 ^cccc\n"
    );
    // 自分が消して、外が直した：両方なら外の行をコメントで残す。
    let theirs = BASE.replace("夕日が差す", "夕日が沈む");
    let mine = BASE.replace("　夕日が差す。 ^bbbb\n", "");
    let merged = merge_scn(BASE, &mine, &theirs);
    assert_eq!(merged.conflicts, 1);
    assert_eq!(
        merged.resolve(&[Choice::Both]).unwrap(),
        "## 屋上 #s1\n灯「やっと来た」 ^aaaa\n// ▼ マージ：外の変更（コメントにして残した）\n// 　夕日が沈む。 ^bbbb\n// ▲ マージ\n湊「待たせた」 ^cccc\n"
    );
}

/// 両方が同じように直したものは、ぶつからない。改行（CRLF）と、終わりの改行の無いファイルを保つ。
#[test]
fn same_edits_and_newlines() {
    let base = BASE.replace('\n', "\r\n");
    let mine = base.replace("待たせた", "ごめん");
    let merged = merge_scn(&base, &mine, &mine);
    assert_eq!(merged.conflicts, 0);
    assert_eq!(merged.resolve(&[]).unwrap(), mine);
    let theirs = base.replace("待たせた", "悪い");
    let both = merge_scn(&base, &mine, &theirs).default_text();
    assert!(both.contains("\r\n// ▼ マージ：外の変更（コメントにして残した）\r\n"));
    assert!(!both.replace("\r\n", "").contains('\n'));
    // 終わりの改行の無い行を自分が直し、外はその後ろに行を足した（終わりの改行の有無は、行と別に3方向で決める）。
    let base = "## a #s1\n　一行目";
    let mine = "## a #s1\n　一行目（直した）";
    let theirs = "## a #s1\n　一行目\n　足した\n";
    let merged = merge_scn(base, mine, theirs);
    assert_eq!(merged.conflicts, 0);
    assert_eq!(
        merged.resolve(&[]).unwrap(),
        "## a #s1\n　一行目（直した）\n　足した\n"
    );
    let base = "a\nb";
    let merged = merge_scn(base, "a\nb", "a\nb\nc\n");
    assert_eq!(merged.resolve(&[]).unwrap(), "a\nb\nc\n");
    let merged = merge_scn("x\n", "x\ny", "z\nx\n");
    assert_eq!(merged.resolve(&[]).unwrap(), "z\nx\ny");
}

/// 設定ファイルのテキスト（行IDを見ない）。ぶつかりは `<<<<<<<` の印で下書きに入れる。
#[test]
fn plain_text_markers() {
    let base = "name: 灯\nage: 16\n";
    let mine = "name: 灯\nage: 17\n";
    let theirs = "name: 灯\nage: 18\n";
    let merged = merge_text(base, mine, theirs);
    assert_eq!(merged.conflicts, 1);
    assert_eq!(
        merged.with_markers(),
        "name: 灯\n<<<<<<< 自分の変更\nage: 17\n=======\nage: 18\n>>>>>>> 外の変更\n"
    );
    // 隣り合う行を、それぞれが直した：ぶつからない。
    let merged = merge_text(base, "name: 灯\nage: 17\n", "name: 明かり\nage: 16\n");
    assert_eq!(merged.conflicts, 0);
    assert_eq!(merged.with_markers(), "name: 明かり\nage: 17\n");
    // 同じところに、それぞれが行を足した：ぶつかる。
    let merged = merge_text(
        base,
        "name: 灯\nvoice: 甲\nage: 16\n",
        "name: 灯\nvoice: 乙\nage: 16\n",
    );
    assert_eq!(merged.conflicts, 1);
}

fn temp_project(name: &str, story: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scn-core-merge-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("story")).unwrap();
    fs::write(dir.join("project.toml"), "format_version = 0\n").unwrap();
    fs::write(dir.join("story/a.scn"), story).unwrap();
    dir
}

/// ワークスペース：保存していない変更があるのに外で変わったら、元（読み込んだときの中身）・自分・外でマージし、
/// 選んだ結果を保存していない変更として開く（ディスクの中身は外の中身）。保存すると書ける。
#[test]
fn workspace_merge() {
    let dir = temp_project("ws", BASE);
    let mut ws = Workspace::open(&dir).unwrap();
    let mine = BASE.replace("やっと来た", "遅いよ");
    assert!(ws.set_buffer("story/a.scn", &mine));
    let theirs = BASE
        .replace("やっと来た", "来てくれた")
        .replace("待たせた", "ごめん");
    fs::write(dir.join("story/a.scn"), &theirs).unwrap();
    assert_eq!(
        ws.external_change(&dir.join("story/a.scn")),
        Change::Conflict {
            file: "story/a.scn".to_owned(),
            deleted: false
        }
    );
    let view = ws.merge_view("story/a.scn", None).unwrap();
    assert_eq!(view.conflicts, 1);
    // 選んだ数が合わなければ断る。
    assert!(
        ws.apply_merge("story/a.scn", None, &[], &view.token)
            .is_err()
    );
    let applied = ws
        .apply_merge("story/a.scn", None, &[Choice::Mine], &view.token)
        .unwrap();
    let expected = mine.replace("待たせた", "ごめん");
    assert_eq!(applied.text, expected);
    assert_eq!(applied.disk.as_deref(), Some(theirs.as_str()));
    assert_eq!(ws.text("story/a.scn"), Some(expected.as_str()));
    assert!(ws.is_dirty("story/a.scn"));
    // 保存する（画面が知っているディスクの中身は外の中身）。
    let mut generator = LineIdGenerator::with_seed(1);
    match ws
        .save("story/a.scn", Some(&theirs), &mut generator)
        .unwrap()
    {
        SaveOutcome::Saved(saved) => assert_eq!(saved.text, expected),
        SaveOutcome::Conflict { .. } => panic!("衝突になった"),
    }
    assert_eq!(
        fs::read_to_string(dir.join("story/a.scn")).unwrap(),
        expected
    );
    // ぶつかっていなければ（元が分からなければ）マージできない。
    assert!(ws.merge_view("story/a.scn", None).is_err());
    let _ = fs::remove_dir_all(&dir);
}

/// 読み込み直した後に、アプリがまだ送っていなかった編集を戻した（知らせの途中の競合）：アプリが覚えている、
/// 読み込む前の中身を元にしてマージする。その後にまた外で変わっても、同じ元を使う。
#[test]
fn workspace_merge_after_reload() {
    let dir = temp_project("reload", BASE);
    let mut ws = Workspace::open(&dir).unwrap();
    let theirs = BASE.replace("待たせた", "ごめん");
    fs::write(dir.join("story/a.scn"), &theirs).unwrap();
    assert!(matches!(
        ws.external_change(&dir.join("story/a.scn")),
        Change::Reloaded { .. }
    ));
    let mine = BASE.replace("やっと来た", "遅いよ");
    assert!(ws.set_buffer("story/a.scn", &mine));
    // 元を渡さなければ、マージできない（コアは元を知らない）。
    assert!(ws.merge_view("story/a.scn", None).is_err());
    let view = ws.merge_view("story/a.scn", Some(BASE)).unwrap();
    assert_eq!(view.conflicts, 0);
    // もう一度外で変わった（今度は保存していない変更があるので、選ぶ前の外の中身になる）。
    let again = theirs.replace("夕日が差す", "夕日が沈む");
    fs::write(dir.join("story/a.scn"), &again).unwrap();
    assert!(matches!(
        ws.external_change(&dir.join("story/a.scn")),
        Change::Conflict { .. }
    ));
    // 前に開いたマージは古いので断る。
    assert!(
        ws.apply_merge("story/a.scn", Some(BASE), &[], &view.token)
            .is_err()
    );
    let view = ws.merge_view("story/a.scn", Some(BASE)).unwrap();
    assert_eq!(view.conflicts, 0);
    let applied = ws
        .apply_merge("story/a.scn", Some(BASE), &[], &view.token)
        .unwrap();
    // 最初の外の変更（ごめん）も、次の外の変更（沈む）も、自分の変更（遅いよ）も入る。
    assert_eq!(
        applied.text,
        BASE.replace("やっと来た", "遅いよ")
            .replace("待たせた", "ごめん")
            .replace("夕日が差す", "夕日が沈む")
    );
    let _ = fs::remove_dir_all(&dir);
}

/// マージの画面を開いた後に、また外で変わった：見せたマージと違うので、何も変えずに断る。
#[test]
fn workspace_merge_refuses_stale_view() {
    let dir = temp_project("stale", BASE);
    let mut ws = Workspace::open(&dir).unwrap();
    let mine = BASE.replace("やっと来た", "壱").replace("待たせた", "参");
    assert!(ws.set_buffer("story/a.scn", &mine));
    fs::write(dir.join("story/a.scn"), BASE.replace("やっと来た", "1")).unwrap();
    ws.external_change(&dir.join("story/a.scn"));
    let view = ws.merge_view("story/a.scn", None).unwrap();
    assert_eq!(view.conflicts, 1);
    fs::write(dir.join("story/a.scn"), BASE.replace("待たせた", "3")).unwrap();
    assert!(matches!(
        ws.external_change(&dir.join("story/a.scn")),
        Change::Conflict { .. }
    ));
    assert!(
        ws.apply_merge("story/a.scn", None, &[Choice::Theirs], &view.token)
            .is_err()
    );
    assert_eq!(ws.text("story/a.scn"), Some(mine.as_str()));
    let _ = fs::remove_dir_all(&dir);
}

/// 片方が行を動かし、もう片方がその行を直した：動かした先に直した中身を置き、行IDは2つにならない。
#[test]
fn moved_and_edited_line() {
    let moved = "## 屋上 #s1\n　夕日が差す。 ^bbbb\n湊「待たせた」 ^cccc\n灯「やっと来た」 ^aaaa\n";
    let edited = BASE.replace("やっと来た", "やっと来たね");
    // 外が動かし、自分が直した。
    let merged = merge_scn(BASE, &edited, moved);
    assert_eq!(merged.conflicts, 0);
    assert_eq!(
        merged.resolve(&[]).unwrap(),
        moved.replace("やっと来た", "やっと来たね")
    );
    // 自分が動かし、外が直した。
    let merged = merge_scn(BASE, moved, &edited);
    assert_eq!(merged.conflicts, 0);
    assert_eq!(
        merged.resolve(&[]).unwrap(),
        moved.replace("やっと来た", "やっと来たね")
    );
    // 動かして直した、もう片方も直した：どの選び方でも行IDは2つにならない（後のものをコメントにする）。
    let moved_edited = moved.replace("やっと来た", "来た");
    let merged = merge_scn(BASE, &edited, &moved_edited);
    for choice in [Choice::Mine, Choice::Theirs, Choice::Both] {
        let text = merged.resolve(&vec![choice; merged.conflicts]).unwrap();
        let ids = live_ids(&text);
        let unique: BTreeSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "{text}");
    }
}

/// 設定DBの本文：外で本文が変わっていても、ぶつからなければマージして書く。
#[test]
fn entry_body_merges() {
    let dir = temp_project("entry", BASE);
    fs::create_dir_all(dir.join("bible/types")).unwrap();
    fs::create_dir_all(dir.join("bible/entries/character")).unwrap();
    fs::write(
        dir.join("bible/types/character.yaml"),
        "name: キャラクター\nbuiltin: character\nfields: []\n",
    )
    .unwrap();
    let seen = "---\nid: akari\nname: 灯\n---\n一行目\n二行目\n";
    fs::write(dir.join("bible/entries/character/akari.md"), seen).unwrap();
    let mut ws = Workspace::open(&dir).unwrap();
    // 外で二行目が変わった。
    let outside = "---\nid: akari\nname: 灯\n---\n一行目\n二行目（外）\n";
    fs::write(dir.join("bible/entries/character/akari.md"), outside).unwrap();
    let edit = ws
        .set_entry_body(
            "bible/entries/character/akari.md",
            "一行目（自分）\n二行目\n",
            seen,
        )
        .unwrap();
    assert_eq!(
        edit,
        EntryEdit::Saved {
            text: "---\nid: akari\nname: 灯\n---\n一行目（自分）\n二行目（外）\n".to_owned()
        }
    );
    // 同じ行を直していれば、書かずに衝突。
    let now = fs::read_to_string(dir.join("bible/entries/character/akari.md")).unwrap();
    let edit = ws
        .set_entry_body(
            "bible/entries/character/akari.md",
            "一行目（別）\n二行目\n",
            seen,
        )
        .unwrap();
    assert!(matches!(edit, EntryEdit::Conflict { .. }), "{edit:?}");
    assert_eq!(
        fs::read_to_string(dir.join("bible/entries/character/akari.md")).unwrap(),
        now
    );
    let _ = fs::remove_dir_all(&dir);
}

/// 行の断片（行IDのある行、無い行、空行、コメント）。
fn fragment() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "## a #s1",
        "灯「台詞」 ^aaaa",
        "灯「別の台詞」 ^aaaa",
        "湊「返事」 ^bbbb",
        "　地の文",
        "　別の地の文",
        "",
        "// コメント",
        "@背景 屋上",
        "　地の文 ^cccc",
    ])
    .prop_map(str::to_owned)
}

fn text_of(lines: &[String], newline: bool) -> String {
    let mut text = lines.join("\n");
    if newline && !text.is_empty() {
        text.push('\n');
    }
    text
}

proptest! {
    /// どちらかが変えていなければ、もう片方の中身になる。どの選び方でもパニックせず、両方を選んでもコメントで
    /// ない行の行IDは、自分か外にあるものだけ。
    #[test]
    fn merge_properties(
        base in prop::collection::vec(fragment(), 0..12),
        mine in prop::collection::vec(fragment(), 0..12),
        theirs in prop::collection::vec(fragment(), 0..12),
        nl in any::<(bool, bool, bool)>(),
    ) {
        let (base, mine, theirs) = (text_of(&base, nl.0), text_of(&mine, nl.1), text_of(&theirs, nl.2));
        prop_assert_eq!(merge_scn(&base, &base, &theirs).resolve(&[]), Some(theirs.clone()));
        prop_assert_eq!(merge_scn(&base, &mine, &base).resolve(&[]), Some(mine.clone()));
        prop_assert_eq!(merge_scn(&base, &mine, &mine).resolve(&[]), Some(mine.clone()));
        let merged = merge_scn(&base, &mine, &theirs);
        let count = |text: &str, id: &str| live_ids(text).iter().filter(|x| *x == id).count();
        for choice in [Choice::Mine, Choice::Theirs, Choice::Both] {
            let text = merged.resolve(&vec![choice; merged.conflicts]).unwrap();
            let known: BTreeSet<String> = live_ids(&mine).into_iter().chain(live_ids(&theirs)).collect();
            for id in live_ids(&text) {
                prop_assert!(known.contains(&id), "{id}");
                // 同じ行IDは、自分か外にあった数より多く並ばない。
                prop_assert!(count(&text, &id) <= count(&mine, &id).max(count(&theirs, &id)), "{id}: {text}");
            }
        }
        let _ = merge_text(&base, &mine, &theirs).with_markers();
    }
}

/// 行IDが重ならない行。
fn unique_lines(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("灯「台詞{i}」 ^a{i:03}")).collect()
}

proptest! {
    /// 行IDが一意な本文で、片方が行を動かし、もう片方がその行を直した：ぶつからず、直した中身が動かした先に入り、
    /// 行IDは一意のまま。
    #[test]
    fn moves_keep_ids_unique(n in 2usize..10, from in any::<Index>(), to in any::<Index>(), mover_is_mine in any::<bool>()) {
        let lines = unique_lines(n);
        let (from, to) = (from.index(n), to.index(n));
        let mut moved = lines.clone();
        let line = moved.remove(from);
        moved.insert(to, line);
        let mut edited = lines.clone();
        if let Some(l) = edited.get_mut(from) {
            *l = l.replace("台詞", "直した台詞");
        }
        let base = text_of(&lines, true);
        let (moved, edited) = (text_of(&moved, true), text_of(&edited, true));
        let merged = if mover_is_mine { merge_scn(&base, &moved, &edited) } else { merge_scn(&base, &edited, &moved) };
        prop_assert_eq!(merged.conflicts, 0);
        let text = merged.resolve(&[]).unwrap();
        let ids = live_ids(&text);
        let unique: BTreeSet<&String> = ids.iter().collect();
        prop_assert_eq!(unique.len(), ids.len());
        prop_assert_eq!(ids.len(), n);
        let fixed = format!("直した台詞{from}");
        prop_assert!(text.contains(&fixed), "{}", text);
    }
}
