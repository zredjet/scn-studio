//! 提案と行ごとの採用（T3-8）：提案の読み方（アプリの実行の JSON と `ai/proposals/*.md`）、今の本文との照らし方、
//! 採用した行だけが変わり行IDが変わらないこと、採用した行の出自、外部の変更とぶつかったときに採用しないこと。

use std::fs;
use std::path::{Path, PathBuf};

use scn_core::ai::{self, Proposal, ProposalStatus};
use scn_core::line_id::LineIdGenerator;
use scn_core::provenance::read_provenance;
use scn_core::workspace::Workspace;
use scn_core::{ParseOptions, analyze, load_project};

fn opts() -> ParseOptions {
    ParseOptions::default()
}

fn sample() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago")
}

fn proposal(line: &str, before: Option<&str>, after: &str) -> Proposal {
    Proposal {
        line: line.to_owned(),
        before: before.map(str::to_owned),
        after: after.to_owned(),
        reason: Some("理由".to_owned()),
    }
}

#[test]
fn parse_diff_lines() {
    let (found, unread) = ai::parse_diff(
        "```json\n{\"line\": \"^k3fg\", \"before\": \"灯「やっぱり、君には言っておく」\", \"after\": \"灯「やっぱり、君には言っとく」\", \"reason\": \"軽口に合わせる\"}\n{\"line\": \"k3f9\"}\n```",
    );
    assert_eq!(
        found,
        [Proposal {
            line: "k3fg".to_owned(),
            before: Some("灯「やっぱり、君には言っておく」".to_owned()),
            after: "灯「やっぱり、君には言っとく」".to_owned(),
            reason: Some("軽口に合わせる".to_owned()),
        }]
    );
    assert_eq!(unread, ["{\"line\": \"k3f9\"}"]);
}

#[test]
fn parse_proposal_markdown() {
    let file = ai::parse_proposal_file(
        "ai/proposals/2026-10-08-tone.md",
        "---\ntask: external:claude-code\nmodel: claude-opus-5-5\nat: 2026-10-08T12:00:00Z\n---\n\n前書き。\n\n## ^k3fg\n- 今：灯「やっぱり、君には言っておく」\n- 案：`灯「やっぱり、君には言っとく」`\n- 理由：口調メモの軽口に合わせる\n\n## ^k3f9\n- 今：灯「やっと来た。遅いよ」\n\n## 見出し\n- 案：x\n",
    );
    assert_eq!(file.task.as_deref(), Some("external:claude-code"));
    assert_eq!(file.model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(file.at.as_deref(), Some("2026-10-08T12:00:00Z"));
    assert_eq!(
        file.proposals,
        [Proposal {
            line: "k3fg".to_owned(),
            before: Some("灯「やっぱり、君には言っておく」".to_owned()),
            after: "灯「やっぱり、君には言っとく」".to_owned(),
            reason: Some("口調メモの軽口に合わせる".to_owned()),
        }]
    );
    assert_eq!(file.problems.len(), 2, "{:?}", file.problems);
    // フロントマターが無くてもよい。
    let file = ai::parse_proposal_file("ai/proposals/x.md", "## k3fa\n- before: a\n- after: b\n");
    assert_eq!(
        file.proposals,
        [Proposal {
            reason: None,
            ..proposal("k3fa", Some("a"), "b")
        }]
    );
}

#[test]
fn views_against_current_text() {
    let project = load_project(&sample()).unwrap();
    let ir = analyze(&project);
    let views = ai::view_proposals(
        &project,
        &ir,
        &[
            proposal(
                "k3fg",
                Some("灯「やっぱり、君には言っておく」"),
                "灯「やっぱり、君には言っとく」",
            ),
            proposal(
                "k3fg",
                Some("灯「違う文」"),
                "灯「やっぱり、君には言っとく」",
            ),
            proposal("k3fg", None, "灯「やっぱり、君には言っとく」"),
            proposal("k3fg", Some("x"), "灯「やっぱり、君には言っておく」"),
            proposal("zzzz", Some("x"), "y"),
            proposal("k3fg", Some("灯「やっぱり、君には言っておく」"), "a\nb"),
        ],
    );
    let kinds: Vec<String> = views
        .iter()
        .map(|v| match &v.status {
            ProposalStatus::Ready => "ready".to_owned(),
            ProposalStatus::Same => "same".to_owned(),
            ProposalStatus::Blocked { reason } => reason.clone(),
        })
        .collect();
    assert_eq!(kinds.first().map(String::as_str), Some("ready"));
    assert!(kinds.get(1).unwrap().contains("本文が変わっています"));
    assert!(kinds.get(2).unwrap().contains("「今」の文がありません"));
    assert_eq!(kinds.get(3).map(String::as_str), Some("same"));
    assert!(kinds.get(4).unwrap().contains("^zzzz"));
    assert!(kinds.get(5).unwrap().contains("改行"));
    let ready = views.first().unwrap();
    assert_eq!(ready.scene.as_deref(), Some("s012"));
    assert_eq!(ready.file.as_deref(), Some("story/chapter01.scn"));
    assert_eq!(
        ready.current.as_deref(),
        Some("灯「やっぱり、君には言っておく」")
    );
    let parts: Vec<(&str, &str)> = ready
        .diff
        .iter()
        .map(|p| (p.kind, p.text.as_str()))
        .collect();
    assert_eq!(
        parts,
        [
            ("equal", "灯「やっぱり、君には言っ"),
            ("delete", "てお"),
            ("insert", "と"),
            ("equal", "く」")
        ]
    );
}

/// 採用した行だけが変わり、字下げと行ID、ほかの行のバイトは変わらない。CRLF も保つ。
#[test]
fn apply_only_adopted_lines() {
    let text = "## a #s1\r\n灯「やあ」 ^aaaa\r\n  湊「おう」 ^bbbb\t\r\n　地の文。 ^cccc\r\n@if x\r\n  灯「中」 ^dddd\r\n@end\r\n";
    let result = ai::apply_proposals(
        text,
        &[
            proposal("bbbb", Some("湊「おう」"), "湊「……おう」"),
            proposal("dddd", Some("灯「中」"), "灯「中だよ」"),
            proposal("cccc", Some("違う"), "x"),
            proposal("zzzz", Some("a"), "b"),
            proposal("aaaa", Some("灯「やあ」"), "灯「やあ」"),
        ],
        &[],
        &opts(),
    );
    assert_eq!(
        result.text,
        "## a #s1\r\n灯「やあ」 ^aaaa\r\n  湊「……おう」 ^bbbb\t\r\n　地の文。 ^cccc\r\n@if x\r\n  灯「中だよ」 ^dddd\r\n@end\r\n"
    );
    assert_eq!(result.applied, ["bbbb", "dddd"]);
    let skipped: Vec<&str> = result.skipped.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(skipped, ["cccc", "zzzz", "aaaa"]);
    // 行IDが重なっていれば採用しない。
    let dup = ai::apply_proposals(
        "a ^aaaa\nb ^aaaa\n",
        &[proposal("aaaa", Some("a"), "c")],
        &[],
        &opts(),
    );
    assert!(dup.applied.is_empty());
    assert!(dup.skipped.first().unwrap().1.contains("重なって"));
    // 終わりに改行の無い行も。
    let last = ai::apply_proposals("a ^aaaa", &[proposal("aaaa", Some("a"), "c")], &[], &opts());
    assert_eq!(last.text, "c ^aaaa");
}

/// 段落頭の全角空白（字下げ）は、案に無ければ残す。行IDの前の空白はすべて残す。コメントの行と `@raw` の中の
/// 行IDは数えない。
#[test]
fn keep_paragraph_indent_and_spacing() {
    let text = "　彼は歩いた。  ^aaaa\n// 彼は歩いた。 ^bbbb\n灯「中」 ^bbbb\n@raw renpy\n  $ x = 1 ^cccc\n@end\n地の文 ^cccc\n";
    let result = ai::apply_proposals(
        text,
        &[
            proposal("aaaa", Some("彼は歩いた。"), "彼は走った。"),
            proposal("bbbb", Some("灯「中」"), "灯「中だよ」"),
            proposal("cccc", Some("地の文"), "地の文。"),
        ],
        &[],
        &opts(),
    );
    assert_eq!(
        result.text,
        "　彼は走った。  ^aaaa\n// 彼は歩いた。 ^bbbb\n灯「中だよ」 ^bbbb\n@raw renpy\n  $ x = 1 ^cccc\n@end\n地の文。 ^cccc\n"
    );
    assert_eq!(result.applied, ["aaaa", "bbbb", "cccc"]);
    // ファイルの提案でも、全角空白を落とさない。
    let file = ai::parse_proposal_file(
        "ai/proposals/x.md",
        "## ^aaaa\n- 今：　彼は歩いた。\n- 案：　彼は走った。\n",
    );
    assert_eq!(file.proposals.first().unwrap().after, "　彼は走った。");
    // 案に `\r` があれば改行として断る。行の種類が変わる案も断る。
    for (after, reason) in [
        ("前半\r後半", "改行"),
        ("// 彼は走った。", "種類"),
        ("## 見出し #s2", "種類"),
        ("-> s2", "種類"),
        ("@背景 屋上", "種類"),
    ] {
        let r = ai::apply_proposals(
            "　彼は歩いた。 ^aaaa\n",
            &[proposal("aaaa", Some("彼は歩いた。"), after)],
            &[],
            &opts(),
        );
        assert!(r.applied.is_empty(), "{after}");
        assert!(
            r.skipped.first().unwrap().1.contains(reason),
            "{after}：{:?}",
            r.skipped
        );
    }
    // 選択肢の行は選択肢のまま。
    let r = ai::apply_proposals(
        "- 「ごめん」 ^dddd\n",
        &[proposal("dddd", Some("- 「ごめん」"), "- 「ごめんね」")],
        &[],
        &opts(),
    );
    assert_eq!(r.text, "- 「ごめんね」 ^dddd\n");
    // 応答の値の末尾に自分の行IDが付いていたら除く。
    let (found, _) = ai::parse_diff(
        "{\"line\": \"aaaa\", \"before\": \"彼は歩いた。 ^aaaa\", \"after\": \"彼は走った。 ^aaaa\"}",
    );
    assert_eq!(
        found.first().unwrap().before.as_deref(),
        Some("彼は歩いた。")
    );
    assert_eq!(found.first().unwrap().after, "彼は走った。");
}

/// 本文全体で見て採用しない：ほかの章との行IDの重なり（E301）、複数行の台詞。
#[test]
fn project_level_blocks() {
    let dir = std::env::temp_dir().join(format!("scn-blocks-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("story")).unwrap();
    fs::write(dir.join("project.toml"), "format_version = 0\n").unwrap();
    fs::write(
        dir.join("story/a.scn"),
        "## a #s1\n地の文。 ^aaaa\n灯「一行目\n二行目」 ^bbbb\n地の文2。 ^cccc\n",
    )
    .unwrap();
    fs::write(dir.join("story/b.scn"), "## b #s2\n別の文。 ^aaaa\n").unwrap();
    let project = load_project(&dir).unwrap();
    let ir = analyze(&project);
    let views = ai::view_proposals(
        &project,
        &ir,
        &[
            proposal("aaaa", Some("地の文。"), "地の文だ。"),
            proposal("bbbb", Some("二行目」"), "二行目だよ」"),
            proposal("cccc", Some("地の文2。"), "地の文の2。"),
        ],
    );
    let reasons: Vec<String> = views
        .iter()
        .map(|v| match &v.status {
            ProposalStatus::Blocked { reason } => reason.clone(),
            other => format!("{other:?}"),
        })
        .collect();
    assert!(reasons.first().unwrap().contains("E301"), "{reasons:?}");
    assert!(reasons.get(1).unwrap().contains("複数行"), "{reasons:?}");
    assert_eq!(reasons.get(2).map(String::as_str), Some("Ready"));
    // ワークスペースでも採用しない。
    let mut ws = Workspace::open(&dir).unwrap();
    let disk = fs::read_to_string(dir.join("story/a.scn")).unwrap();
    let adopted = ws
        .adopt_proposals(
            "story/a.scn",
            &[
                proposal("aaaa", Some("地の文。"), "地の文だ。"),
                proposal("bbbb", Some("二行目」"), "二行目だよ」"),
            ],
            "t",
            "m",
            Some(&disk),
            &mut LineIdGenerator::new(),
        )
        .unwrap();
    assert!(adopted.applied.is_empty() && adopted.saved.is_none());
    assert_eq!(adopted.skipped.len(), 2);
    let _ = fs::remove_dir_all(&dir);
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let target = to.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &target);
        } else {
            fs::copy(&path, &target).unwrap();
        }
    }
}

/// アプリで採用すると保存し、採用した行の出自を ai として書き足す。外部の変更とぶつかれば採用しない。
#[test]
fn workspace_adopts_and_records() {
    let dir = std::env::temp_dir().join(format!("scn-adopt-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    copy_dir(&sample(), &dir);
    let file = "story/chapter01.scn";
    let before = fs::read_to_string(dir.join(file)).unwrap();
    let mut ws = Workspace::open(&dir).unwrap();
    let mut generator = LineIdGenerator::new();
    let adopted = ws
        .adopt_proposals(
            file,
            &[
                proposal(
                    "k3fg",
                    Some("灯「やっぱり、君には言っておく」"),
                    "灯「やっぱり、君には言っとく」",
                ),
                proposal("k3f9", Some("違う"), "x"),
            ],
            "rephrase",
            "qwen3",
            Some(&before),
            &mut generator,
        )
        .unwrap();
    assert_eq!(adopted.applied, ["k3fg"]);
    assert_eq!(adopted.skipped.len(), 1);
    let saved = adopted.saved.unwrap();
    let after = fs::read_to_string(dir.join(file)).unwrap();
    assert_eq!(saved.text, after);
    assert!(after.contains("灯「やっぱり、君には言っとく」 ^k3fg"));
    // 採用した行のほかは、行IDの無い行に行IDが付いたほかは変わらない（行IDも変わらない）。
    let changed: Vec<(&str, &str)> = before
        .lines()
        .zip(after.lines())
        .filter(|(a, b)| a != b)
        .collect();
    assert!(
        changed
            .iter()
            .all(|(a, b)| a.contains("^k3fg") || b.starts_with(a)),
        "{changed:?}"
    );
    let provenance = read_provenance(&dir).unwrap();
    let record = provenance
        .records
        .iter()
        .rev()
        .find(|r| r.line == "k3fg")
        .unwrap();
    assert_eq!(
        (
            record.origin.as_str(),
            record.task.as_str(),
            record.model.as_str()
        ),
        ("ai", "rephrase", "qwen3")
    );

    // 外で変わっていたら採用しない（本文は戻す）。
    let disk = fs::read_to_string(dir.join(file)).unwrap();
    fs::write(dir.join(file), format!("{disk}\n// 外の変更\n")).unwrap();
    let buffer = ws.text(file).unwrap().to_owned();
    let adopted = ws
        .adopt_proposals(
            file,
            &[proposal(
                "k3f9",
                Some("灯「やっと来た。遅いよ」"),
                "灯「やっと来たね」",
            )],
            "rephrase",
            "qwen3",
            Some(&disk),
            &mut generator,
        )
        .unwrap();
    assert_eq!(adopted.conflict, Some(false));
    assert!(adopted.applied.is_empty() && adopted.saved.is_none());
    assert_eq!(ws.text(file), Some(buffer.as_str()));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn proposal_files_in_folder() {
    let dir = std::env::temp_dir().join(format!("scn-proposals-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("ai/proposals")).unwrap();
    fs::write(
        dir.join("ai/proposals/b.md"),
        "## ^k3fg\n- 今：a\n- 案：b\n",
    )
    .unwrap();
    fs::write(
        dir.join("ai/proposals/a.md"),
        "## ^k3f9\n- 今：c\n- 案：d\n",
    )
    .unwrap();
    fs::write(dir.join("ai/proposals/c.md"), [0xff, 0xfe]).unwrap();
    fs::write(dir.join("ai/proposals/notes.txt"), "x").unwrap();
    let files = ai::read_proposal_files(&dir);
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "ai/proposals/a.md",
            "ai/proposals/b.md",
            "ai/proposals/c.md"
        ]
    );
    assert!(
        files
            .get(2)
            .unwrap()
            .problems
            .first()
            .unwrap()
            .contains("UTF-8")
    );
    assert!(ai::read_proposal_files(&dir.join("none")).is_empty());
    let _ = fs::remove_dir_all(&dir);
}

/// 当てると行IDが効かなくなる案（台詞の閉じ括弧を落とす、`@選択` をほかの命令にする）は採用しない。
#[test]
fn proposals_that_break_line_ids() {
    let text = "## a #s1\n灯「やあ」 ^aaaa\n@選択 どうする ^bbbb\n- 行く ^cccc\n  -> s1\n@end\n";
    let r = ai::apply_proposals(
        text,
        &[
            proposal("aaaa", Some("灯「やあ」"), "灯「やあ、"),
            proposal("bbbb", Some("@選択 どうする"), "@背景 屋上"),
            proposal("cccc", Some("- 行く"), "- 行こう"),
        ],
        &[],
        &opts(),
    );
    assert_eq!(r.applied, ["cccc"]);
    let reasons: Vec<&str> = r.skipped.iter().map(|(_, why)| why.as_str()).collect();
    assert!(
        reasons.first().unwrap().contains("構文に誤り"),
        "{reasons:?}"
    );
    assert!(
        reasons.get(1).unwrap().contains("行IDが効かなく"),
        "{reasons:?}"
    );
    // 閉じ括弧を落として、次の地の文まで台詞が続く（行IDが効かなくなる）。
    let cut = ai::apply_proposals(
        "## a #s1\n灯「やあ」 ^aaaa\n地の文。 ^eeee\n",
        &[proposal("aaaa", Some("灯「やあ」"), "灯「やあ、")],
        &[],
        &opts(),
    );
    assert!(cut.applied.is_empty(), "{:?}", cut.text);
    assert!(r.text.contains("- 行こう ^cccc"));
    // 閉じ忘れの `@raw` は、字下げの無い見出しで閉じる（後ろの行IDは数える）。
    let raw = "## a #s1\n@raw renpy\n  $ x = 1\n## b #s2\n地の文。 ^dddd\n";
    let r = ai::apply_proposals(
        raw,
        &[proposal("dddd", Some("地の文。"), "地の文だ。")],
        &[],
        &opts(),
    );
    assert_eq!(r.applied, ["dddd"]);
}
