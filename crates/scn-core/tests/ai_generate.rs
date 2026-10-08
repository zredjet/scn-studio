//! 生成（T3-9）：応答から書き込む本文の行を取り、スタブのシーンに差し込む（今ある行は変えない、`ai-draft`、
//! 差し込んだ行にだけ行ID、出自）。スタブでないシーンには書き込まない。

use std::fs;
use std::path::{Path, PathBuf};

use scn_core::ai;
use scn_core::line_id::LineIdGenerator;
use scn_core::provenance::read_provenance;
use scn_core::workspace::Workspace;
use scn_core::{analyze, load_project};

fn sample() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago")
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

#[test]
fn body_lines_from_response() {
    let text = "```scn
## 沈黙のあと #s014
---
summary: x
status: draft
---
@背景 屋上/夕方
// メモ
灯「……」
　風が止んだ。


湊「うん」 ^k3f9
@選択 どうする
- 行く
  灯「中」
@end
@設定 flag.勇気 += 1
-> s020
@BGM 切ない
```";
    assert_eq!(
        ai::generated_body(text),
        [
            "@背景 屋上/夕方",
            "灯「……」",
            "　風が止んだ。",
            "",
            "@BGM 切ない"
        ]
    );
    assert!(ai::generated_body("```\n```").is_empty());
    // コードブロックの外の前置きと後書きは本文にしない。2つ目のシーンからは読まない。
    let text = "下書きを書きました。\n\n```\n灯「雨だね」\n## 次 #s015\n湊「うん」\n```\n\n気になる点があれば直します。";
    assert_eq!(ai::generated_body(text), ["灯「雨だね」"]);
    // 自律の直しの応答：点検のために付けた仮の行IDは除いて読む（ほかの行IDの行は書かない）。
    let temp = vec!["t001".to_owned()];
    let fixed = ai::without_temp_ids("灯「直した」 ^t001\n湊「写し」 ^k3f9", &temp);
    assert_eq!(ai::generated_body(&fixed), ["灯「直した」"]);
}

#[test]
fn insert_into_stub() {
    let body: Vec<String> = ["灯「……」", "　風が止んだ。"].map(str::to_owned).to_vec();
    // メタ情報の status を書き換え、演出の命令の後ろ、ジャンプの手前に差し込む。
    let scene = "## 雨の図書室 #s022\n---\nsummary: 雨の日\nstatus: draft\nroute: 灯\n---\n@背景 図書室\n-> s040\n\n";
    let drafted = ai::draft_into_scene(scene, &body).unwrap();
    assert_eq!(
        drafted.text,
        "## 雨の図書室 #s022\n---\nsummary: 雨の日\nstatus: ai-draft\nroute: 灯\n---\n@背景 図書室\n灯「……」\n　風が止んだ。\n\n-> s040\n\n"
    );
    assert_eq!(
        drafted.text.get(drafted.inserted.0..drafted.inserted.1),
        Some("灯「……」\n　風が止んだ。\n\n")
    );
    // メタ情報が無ければ作る。流れの行が無ければ終わりの空行の手前。CRLF はそろえる。
    let drafted = ai::draft_into_scene("## 沈黙 #s014\r\n\r\n", &body).unwrap();
    assert_eq!(
        drafted.text,
        "## 沈黙 #s014\r\n---\r\nstatus: ai-draft\r\n---\r\n灯「……」\r\n　風が止んだ。\r\n\r\n"
    );
    // status の無いメタ情報には足す。ファイルの終わりで改行が無くても。
    let drafted = ai::draft_into_scene("## a #s1\n---\nsummary: x\n---", &body).unwrap();
    assert_eq!(
        drafted.text,
        "## a #s1\n---\nsummary: x\nstatus: ai-draft\n---\n灯「……」\n　風が止んだ。\n"
    );
    assert!(ai::draft_into_scene(scene, &[]).is_err());
    // スタブに今ある行（写して返した演出の命令）は差し込まない。それしか無ければ書かない。
    let copied: Vec<String> = ["@背景 図書室", "灯「……」"].map(str::to_owned).to_vec();
    let drafted = ai::draft_into_scene(scene, &copied).unwrap();
    assert_eq!(
        drafted.text.get(drafted.inserted.0..drafted.inserted.1),
        Some("灯「……」\n\n")
    );
    assert!(ai::draft_into_scene(scene, &copied[..1]).is_err());
    // 途中で同じ演出に戻る行は差し込む。
    let back: Vec<String> = ["@背景 図書室", "灯「……」", "@背景 廊下", "@背景 図書室"]
        .map(str::to_owned)
        .to_vec();
    let drafted = ai::draft_into_scene(scene, &back).unwrap();
    assert_eq!(
        drafted.text.get(drafted.inserted.0..drafted.inserted.1),
        Some("灯「……」\n@背景 廊下\n@背景 図書室\n\n")
    );
    // メタ情報が閉じていない（E102）なら書かない。
    let unclosed = "## a #s1\n---\nsummary: x\n@背景 図書室\n";
    assert!(
        ai::draft_into_scene(unclosed, &body)
            .unwrap_err()
            .contains("E102")
    );
}

/// ワークスペースで書き込む：スタブのシーンだけが変わり、差し込んだ行にだけ行IDが付き、出自を ai と記録する。
#[test]
fn workspace_writes_draft() {
    let dir = std::env::temp_dir().join(format!("scn-draft-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    copy_dir(&sample(), &dir);
    let file = "story/chapter02.scn";
    let before = fs::read_to_string(dir.join(file)).unwrap();
    let mut ws = Workspace::open(&dir).unwrap();
    let mut generator = LineIdGenerator::with_seed(7);
    let body: Vec<String> = ["灯「雨、やまないね」", "　窓の外が白く煙っていた。"]
        .map(str::to_owned)
        .to_vec();
    let written = ws
        .write_draft(
            file,
            "s022",
            &body,
            "draft_stub",
            "claude",
            Some(&before),
            &mut generator,
        )
        .unwrap();
    assert_eq!(written.refused, None);
    assert_eq!(written.ids.len(), 2);
    let after = fs::read_to_string(dir.join(file)).unwrap();
    // 前の本文の行は（行IDの無い行に保存で行IDが付くこと、s022 の status を除いて）そのまま、順も変わらない。
    let mut rest = after.lines();
    for line in before.lines() {
        let found = rest.by_ref().any(|l| {
            l == line
                || (l.starts_with(line) && l.get(line.len()..).is_some_and(|t| t.starts_with(" ^")))
                || (line.starts_with("status:") && l == "status: ai-draft")
        });
        assert!(found, "前の行が無くなった：{line}");
    }
    for id in &written.ids {
        assert!(after.contains(&format!(" ^{id}")));
    }
    assert!(after.contains("灯「雨、やまないね」 ^"));
    let project = load_project(&dir).unwrap();
    let ir = analyze(&project);
    let s022 = ir.scenes.iter().find(|s| s.id == "s022").unwrap();
    assert_eq!(s022.meta.status.as_deref(), Some("ai-draft"));
    assert!(!s022.is_stub);
    // 辺は変わらない。
    assert!(ir.edges.iter().any(|e| e.from == "s022" && e.to == "s040"));
    let provenance = read_provenance(&dir).unwrap();
    for id in &written.ids {
        let record = provenance
            .records
            .iter()
            .rev()
            .find(|r| &r.line == id)
            .unwrap();
        assert_eq!(
            (record.origin.as_str(), record.task.as_str()),
            ("ai", "draft_stub")
        );
    }

    // もうスタブでないシーンには書き込まない。
    let disk = fs::read_to_string(dir.join(file)).unwrap();
    let again = ws
        .write_draft(
            file,
            "s022",
            &body,
            "draft_stub",
            "claude",
            Some(&disk),
            &mut generator,
        )
        .unwrap();
    assert!(again.refused.unwrap().contains("スタブ"));
    assert_eq!(fs::read_to_string(dir.join(file)).unwrap(), disk);
    let other = ws
        .write_draft(
            file,
            "s012",
            &body,
            "draft_stub",
            "claude",
            Some(&disk),
            &mut generator,
        )
        .unwrap();
    assert!(other.refused.unwrap().contains("ありません"));
    let _ = fs::remove_dir_all(&dir);
}

/// 本文の読み方の設定（`project.toml` の `[dialogue]`）を読めなければ、行IDを付けられないので書き込まない。
#[test]
fn refuses_without_reading_settings() {
    let dir = std::env::temp_dir().join(format!("scn-draft-reading-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    copy_dir(&sample(), &dir);
    let toml = fs::read_to_string(dir.join("project.toml")).unwrap();
    fs::write(
        dir.join("project.toml"),
        toml.replace("[dialogue.brackets]", "[dialogue]\nbrackets = 3\n[x]"),
    )
    .unwrap();
    let file = "story/chapter02.scn";
    let before = fs::read_to_string(dir.join(file)).unwrap();
    let mut ws = Workspace::open(&dir).unwrap();
    assert!(ws.project().has_reading_errors());
    let body = vec!["灯「雨」".to_owned()];
    let written = ws
        .write_draft(
            file,
            "s022",
            &body,
            "draft_stub",
            "claude",
            Some(&before),
            &mut LineIdGenerator::with_seed(7),
        )
        .unwrap();
    assert!(written.refused.unwrap().contains("行ID"));
    assert_eq!(fs::read_to_string(dir.join(file)).unwrap(), before);
    let _ = fs::remove_dir_all(&dir);
}

/// 範囲の中の行IDの無い行にだけ行IDを付ける。
#[test]
fn ids_only_in_range() {
    let project = load_project(&sample()).unwrap();
    let file = "story/chapter01.scn";
    let text = project
        .files
        .iter()
        .find(|f| f.path == file)
        .unwrap()
        .text
        .clone();
    // s013b には行IDの無い行がある（サンプルの約束）。範囲をその外にすれば、付かない。
    let added = format!("{text}\n## 足したシーン #s099\n灯「新しい」\n");
    let start = text.len();
    let (with_ids, ids) = ai::with_line_ids(
        &project,
        file,
        &added,
        (start, added.len()),
        &mut LineIdGenerator::with_seed(1),
    );
    assert_eq!(ids.len(), 1);
    assert!(with_ids.ends_with(&format!("灯「新しい」 ^{}\n", ids.first().unwrap())));
    assert_eq!(with_ids.get(..start), Some(text.as_str()));
}

/// 「AI下書きを含める」を切った IR：ai-draft のシーンの台詞と地の文を省き、流れは残す。
#[test]
fn ir_without_ai_drafts() {
    let project = load_project(&sample()).unwrap();
    let ir = analyze(&project);
    let drafts: Vec<&str> = ir
        .scenes
        .iter()
        .filter(|s| s.meta.status.as_deref() == Some("ai-draft"))
        .map(|s| s.id.as_str())
        .collect();
    assert!(!drafts.is_empty());
    let stripped = ai::without_ai_drafts(&ir);
    assert_eq!(stripped.edges, ir.edges);
    let rows = scn_core::script::script(&project, &stripped).rows;
    assert!(
        rows.iter()
            .all(|r| !drafts.iter().any(|d| r.scene.starts_with(d)))
    );
    let all = scn_core::script::script(&project, &ir).rows;
    assert!(all.len() > rows.len());
    let first = stripped
        .scenes
        .iter()
        .find(|s| s.id == *drafts.first().unwrap())
        .unwrap();
    let json = serde_json::to_string(&first.ops).unwrap();
    assert!(json.contains("AI下書きのため省いたシーン"), "{json}");
    assert!(!json.contains("\"say\""), "{json}");
}
