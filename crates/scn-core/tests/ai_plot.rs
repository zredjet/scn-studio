//! プロットとフローへの AI 支援（T3-10）：模擬のサーバーで、発散（提案ノードの採用でスタブと選択肢が書き足され、
//! 出自が記録される）、生成（ルートの骨格のスタブが足され、人の行が変わらない）、レビュー（文脈にコアの計算結果が
//! 入る）、同期（要約のずれが差分の形で出る）、問い返し（質問と分類案を読める）を確かめる。実際の LLM は呼ばない。

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use scn_core::ai::{self, CallOptions, Connection, Provider, RunOutcome};
use scn_core::line_id::LineIdGenerator;
use scn_core::provenance::read_provenance;
use scn_core::workspace::Workspace;
use scn_core::{Project, analyze, load_project};

/// 1回だけ応答する模擬の OpenAI 互換サーバー。受け取った頼み（本文）を覚える。
fn serve(body: &str) -> (String, Arc<Mutex<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let received = Arc::new(Mutex::new(String::new()));
    let slot = Arc::clone(&received);
    let body = body.to_owned();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line.trim().is_empty() {
                break;
            }
            if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = v.trim().parse().unwrap();
            }
        }
        let mut request = vec![0; length];
        reader.read_exact(&mut request).unwrap();
        *slot.lock().unwrap() = String::from_utf8(request).unwrap();
        let reply = serde_json::json!({
            "choices": [{"message": {"content": body}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 900, "completion_tokens": 40}
        })
        .to_string();
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
            reply.len()
        );
    });
    (format!("http://{addr}/v1"), received)
}

fn connection(base_url: String) -> Connection {
    Connection {
        name: "local".to_owned(),
        provider: Provider::OpenaiCompatible,
        endpoint: format!("openai-compatible {base_url}"),
        base_url,
        model: "test-model".to_owned(),
        cloud: false,
        allowed: true,
        context_tokens: 200_000,
    }
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

fn houkago_copy(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scn-plot-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    copy_dir(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago"),
        &dir,
    );
    dir
}

/// タスクを模擬のサーバーで実行する。
fn run(
    root: &Path,
    project: &Project,
    task: &ai::AiTask,
    scene: Option<&str>,
    reply: &str,
) -> (RunOutcome, String) {
    let ir = analyze(project);
    let sources = ai::read_sources(root);
    let (url, received) = serve(reply);
    let outcome = ai::run_task(
        project,
        &ir,
        &sources,
        task,
        scene,
        &connection(url),
        None,
        None,
        &CallOptions {
            connect_timeout: Duration::from_secs(2),
            idle_timeout: Duration::from_secs(5),
        },
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    let request = received.lock().unwrap().clone();
    (outcome, request)
}

fn task(project: &Project, id: &str) -> ai::AiTask {
    project
        .ai_tasks
        .iter()
        .find(|t| t.id == id)
        .unwrap()
        .clone()
}

/// 前の本文の行が、順も変えずに残っている（保存で付いた行IDと、`except` の行を除く）。
fn keeps_lines(before: &str, after: &str, except: &dyn Fn(&str, &str) -> bool) {
    let mut rest = after.lines();
    for line in before.lines() {
        let found = rest.by_ref().any(|l| {
            l == line
                || (l.starts_with(line) && l.get(line.len()..).is_some_and(|t| t.starts_with(" ^")))
                || except(line, l)
        });
        assert!(found, "前の行が無くなった：{line}");
    }
}

/// 発散：分岐案の応答を提案ノードとして読み、採用するとスタブ（`ai-draft`）と選択肢が書き足され、選択肢の行の
/// 出自が `ai` と記録される。採用した後に検証する。採用できない案は断る。
#[test]
fn branch_proposals_are_adopted() {
    let root = houkago_copy("branch");
    let file = "story/chapter01.scn";
    let before = fs::read_to_string(root.join(file)).unwrap();
    let mut ws = Workspace::open(&root).unwrap();
    let project = ws.project().clone();
    let reply = "分岐案です。

### 提案 1：灯を追いかける
- 灯を追いかける
  @設定 灯.好感度 += 1
  -> s013a
理由：灯の葛藤に向き合わせる

### 提案 2：その場に残る
- その場に残る -> s015
## 紬と会う #s015
---
summary: 紬と図書室で会う
---
紬「あ、湊くん」

### 提案 3：行き先の無い案
- どこかへ行く -> s999
";
    let (outcome, request) = run(
        &root,
        &project,
        &task(&project, "branch_ideas"),
        Some("s014"),
        reply,
    );
    assert!(request.contains("### 提案 1：案の名前"), "{request}");
    let b = &outcome.branches;
    assert_eq!(b.len(), 3, "{b:?}");
    let first = b.first().unwrap();
    assert_eq!(
        (
            first.name.as_str(),
            first.label.as_str(),
            first.target.as_str()
        ),
        ("灯を追いかける", "灯を追いかける", "s013a")
    );
    assert_eq!(first.effects, ["@設定 灯.好感度 += 1"]);
    assert_eq!(first.reason.as_deref(), Some("灯の葛藤に向き合わせる"));
    assert!(first.problems.is_empty());
    let second = b.get(1).unwrap();
    assert_eq!(second.target, "s015");
    assert_eq!(second.stubs.len(), 1);
    assert_eq!(
        second.stubs.first().unwrap().summary.as_deref(),
        Some("紬と図書室で会う")
    );
    let third = b.get(2).unwrap();
    assert!(
        third.problems.iter().any(|p| p.contains("s999")),
        "{third:?}"
    );

    // 案2を採用：スタブを章の末尾に足し、s014 に選択ブロックを足す。
    let mut generator = LineIdGenerator::with_seed(11);
    let adopted = ws
        .adopt_branch(second, "branch_ideas", "test-model", &mut generator)
        .unwrap();
    assert_eq!(adopted.written.refused, None);
    assert_eq!(adopted.file, file);
    // `@選択` の行と選択肢の行に行IDが付き、出自が ai になる。
    assert_eq!(adopted.written.ids.len(), 2, "{adopted:?}");
    let after = fs::read_to_string(root.join(file)).unwrap();
    keeps_lines(&before, &after, &|_, _| false);
    assert!(
        after.contains(
            "\n\n## 紬と会う #s015\n---\nsummary: 紬と図書室で会う\nstatus: ai-draft\n---\n"
        ),
        "{after}"
    );
    assert!(!after.contains("紬「あ、湊くん」"), "本文は書かない");
    let option = format!(
        "- その場に残る -> s015 ^{}",
        adopted.written.ids.get(1).unwrap()
    );
    assert!(after.contains(&option), "{after}");
    let provenance = read_provenance(&root).unwrap();
    for id in &adopted.written.ids {
        let r = provenance
            .records
            .iter()
            .rev()
            .find(|r| &r.line == id)
            .unwrap();
        assert_eq!((r.origin.as_str(), r.task.as_str()), ("ai", "branch_ideas"));
    }
    let ir = analyze(ws.project());
    assert!(
        ir.edges
            .iter()
            .any(|e| e.from == "s014" && e.to == "s015" && e.via == "choice")
    );
    assert!(
        ir.scenes
            .iter()
            .any(|s| s.id == "s015" && s.meta.status.as_deref() == Some("ai-draft"))
    );
    // 検証の結果（足したシーンと s014 の誤りと警告）。誤り（E）は無い。
    assert!(
        adopted.checks.iter().all(|c| !c.starts_with('E')),
        "{:?}",
        adopted.checks
    );

    // 案1を採用：s014 の最後の選択ブロックに、`@設定` つきの選択肢を足す。
    let adopted = ws
        .adopt_branch(first, "branch_ideas", "test-model", &mut generator)
        .unwrap();
    assert_eq!(adopted.written.refused, None);
    assert_eq!(adopted.written.ids.len(), 1);
    let after = fs::read_to_string(root.join(file)).unwrap();
    let option = format!(
        "- 灯を追いかける ^{}\n  @設定 灯.好感度 += 1\n  -> s013a\n@end",
        adopted.written.ids.first().unwrap()
    );
    assert!(after.contains(&option), "{after}");

    // 採用できない案、もう使われたシーンIDのスタブは断る（書かない）。
    let refused = ws
        .adopt_branch(third, "branch_ideas", "test-model", &mut generator)
        .unwrap();
    assert!(refused.written.refused.unwrap().contains("s999"));
    let again = ws
        .adopt_branch(second, "branch_ideas", "test-model", &mut generator)
        .unwrap();
    assert!(
        again.written.refused.unwrap().contains("s015"),
        "同じスタブは2度足さない"
    );
    assert_eq!(fs::read_to_string(root.join(file)).unwrap(), after);
    // 保存していない変更のある章には書き足さない。
    ws.set_buffer(file, &format!("{after}// メモ\n"));
    let dirty = ws
        .adopt_branch(
            &ai::BranchProposal {
                stubs: Vec::new(),
                target: "s013b".to_owned(),
                ..first.clone()
            },
            "branch_ideas",
            "test-model",
            &mut generator,
        )
        .unwrap();
    assert!(dirty.written.refused.unwrap().contains("保存していない"));
    let _ = fs::remove_dir_all(&root);
}

/// 生成：ルートの骨格の応答から、スタブのシーン（見出し、summary・route、流れの行）だけを読み、章の末尾に足す。
/// 人の行は変わらず、足したシーンは `ai-draft`、選択肢の行の出自は `ai`。行き先の無い骨格・重なるIDは書かない。
#[test]
fn skeleton_adds_stub_scenes() {
    let root = houkago_copy("skeleton");
    let file = "story/chapter02.scn";
    let before = fs::read_to_string(root.join(file)).unwrap();
    let mut ws = Workspace::open(&root).unwrap();
    let project = ws.project().clone();
    let skeleton_task = ai::with_route(&task(&project, "route_skeleton"), "紬");
    assert!(ai::is_skeleton(&skeleton_task));
    let reply = "骨格を作りました。
```scn
## 図書室の約束 #s070
---
summary: 紬と図書室で約束をする
route: 紬
---
紬「また明日ね」
@背景 図書室
@選択 どうする
- 約束する -> s071
- 断る
  @設定 紬.好感度 -= 1
  -> s060
@end

## 約束の日 #s071
---
summary: 約束の日、紬が待っている
---
-> s060
## 見出しだけ
```
";
    let (outcome, request) = run(&root, &project, &skeleton_task, None, reply);
    assert!(request.contains("対象のルート：紬"), "{request}");
    let scenes = &outcome.skeleton;
    assert_eq!(scenes.len(), 2, "{scenes:?}");
    let s070 = scenes.first().unwrap();
    assert_eq!(
        s070.lines,
        [
            "@選択 どうする",
            "- 約束する -> s071",
            "- 断る",
            "  @設定 紬.好感度 -= 1",
            "  -> s060",
            "@end"
        ]
    );
    assert_eq!(s070.route.as_deref(), Some("紬"));
    assert!(outcome.unread.iter().any(|u| u.contains("見出しだけ")));

    let mut generator = LineIdGenerator::with_seed(5);
    let written = ws
        .write_skeleton(
            file,
            scenes,
            Some("紬"),
            "route_skeleton",
            "test-model",
            &mut generator,
        )
        .unwrap();
    assert_eq!(written.written.refused, None, "{written:?}");
    // `@選択` と2つの選択肢の行。
    assert_eq!(written.written.ids.len(), 3);
    let after = fs::read_to_string(root.join(file)).unwrap();
    keeps_lines(&before, &after, &|_, _| false);
    assert!(after.contains("## 約束の日 #s071\n---\nsummary: 約束の日、紬が待っている\nroute: 紬\nstatus: ai-draft\n---\n-> s060\n"), "{after}");
    assert!(!after.contains("紬「また明日ね」"));
    let ir = analyze(ws.project());
    for id in ["s070", "s071"] {
        let s = ir.scenes.iter().find(|s| s.id == id).unwrap();
        assert_eq!(s.meta.status.as_deref(), Some("ai-draft"));
        assert!(s.is_stub);
    }
    assert!(ir.edges.iter().any(|e| e.from == "s070" && e.to == "s071"));
    let provenance = read_provenance(&root).unwrap();
    for id in &written.written.ids {
        let r = provenance
            .records
            .iter()
            .rev()
            .find(|r| &r.line == id)
            .unwrap();
        assert_eq!(
            (r.origin.as_str(), r.task.as_str()),
            ("ai", "route_skeleton")
        );
    }

    // 行き先の無い骨格、もう使われたID は書かない。
    let disk = fs::read_to_string(root.join(file)).unwrap();
    let broken = vec![ai::SkeletonScene {
        id: "s080".to_owned(),
        title: "迷子".to_owned(),
        summary: None,
        route: None,
        lines: vec!["-> s999".to_owned()],
    }];
    let refused = ws
        .write_skeleton(
            file,
            &broken,
            None,
            "route_skeleton",
            "test-model",
            &mut generator,
        )
        .unwrap();
    assert!(refused.written.refused.unwrap().contains("s999"));
    let refused = ws
        .write_skeleton(
            file,
            scenes,
            None,
            "route_skeleton",
            "test-model",
            &mut generator,
        )
        .unwrap();
    assert!(
        refused
            .written
            .refused
            .unwrap()
            .contains("もう使われています")
    );
    // 足すと誤り（閉じていないブロック E103 など）が出る骨格は書かない。
    let unclosed = vec![ai::SkeletonScene {
        id: "s081".to_owned(),
        title: "閉じない".to_owned(),
        summary: None,
        route: None,
        lines: vec!["@選択".to_owned(), "- 行く -> s060".to_owned()],
    }];
    let refused = ws
        .write_skeleton(
            file,
            &unclosed,
            None,
            "route_skeleton",
            "test-model",
            &mut generator,
        )
        .unwrap();
    assert!(refused.written.refused.unwrap().contains("誤り"));
    assert_eq!(fs::read_to_string(root.join(file)).unwrap(), disk);
    let _ = fs::remove_dir_all(&root);
}

/// 同期：シーン要約（キャッシュ）とメタ情報の summary のずれを、差分の形（今の summary、案、文字の差分）で出す。
/// 採用すると summary の1行だけが変わる。
#[test]
fn summary_drift_as_proposal() {
    let root = houkago_copy("drift");
    let file = "story/chapter01.scn";
    let mut ws = Workspace::open(&root).unwrap();
    let project = ws.project().clone();
    let summary = "灯が転校を告げ、主人公は黙って受け止める。";
    let (outcome, _) = run(
        &root,
        &project,
        &task(&project, "scene_summary"),
        Some("s013a"),
        summary,
    );
    assert_eq!(outcome.summary.as_deref(), Some(summary));
    let ir = analyze(&project);
    let mut cache = ai::read_sources(&root).summaries;
    ai::store_summary(
        &root,
        &mut cache,
        "s013a",
        ai::scene_source(&project, &ir, "s013a").unwrap(),
        summary,
    )
    .unwrap();
    let drift = ai::summary_drift(&project, &ir, &cache);
    let d = drift.iter().find(|d| d.scene == "s013a").unwrap();
    assert_eq!(
        d.current.as_deref(),
        Some("灯が転校を打ち明ける。主人公は受け止める")
    );
    assert_eq!(d.proposed, summary);
    assert!(d.diff.iter().any(|p| p.kind == "insert"));
    assert!(d.diff.iter().any(|p| p.kind == "equal"));
    // キャッシュの無いシーン、同じ要約のシーンは出さない。
    assert!(drift.iter().all(|d| d.scene == "s013a"));

    let before = fs::read_to_string(root.join(file)).unwrap();
    let written = ws
        .set_scene_summary(
            file,
            "s013a",
            &d.proposed,
            Some(&before),
            &mut LineIdGenerator::with_seed(2),
        )
        .unwrap();
    assert_eq!(written.refused, None);
    let after = fs::read_to_string(root.join(file)).unwrap();
    keeps_lines(&before, &after, &|old, new| {
        old == "summary: 灯が転校を打ち明ける。主人公は受け止める"
            && new == format!("summary: {summary}")
    });
    assert!(after.contains(&format!("## 打ち明ける #s013a\n---\nsummary: {summary}\n")));
    // メタ情報の無いシーンには作る。
    let made = ai::set_meta_line("## a #s1\n灯「x」\n", "summary", "要約").unwrap();
    assert_eq!(made, "## a #s1\n---\nsummary: 要約\n---\n灯「x」\n");
    let added =
        ai::set_meta_line("## a #s1\r\n---\r\nroute: 灯\r\n---\r\n", "summary", "要約").unwrap();
    assert_eq!(
        added,
        "## a #s1\r\n---\r\nroute: 灯\r\nsummary: 要約\r\n---\r\n"
    );
    assert!(ai::set_meta_line("## a #s1\n---\nroute: 灯\n", "summary", "x").is_err());
    let _ = fs::remove_dir_all(&root);
}

/// 問い返し：企画書とメモへの質問（指摘の形。行もシーンも無い）と、メモの分類案（無いメモへの案は読まない）。
#[test]
fn questions_and_idea_suggestions() {
    let root = houkago_copy("questions");
    let project = load_project(&root).unwrap();
    let (outcome, request) = run(
        &root,
        &project,
        &task(&project, "concept_questions"),
        None,
        "{\"severity\": \"info\", \"message\": \"灯の転校はいつ決まったのですか？\"}\n{\"message\": \"屋上の鍵を主人公が使える理由は？\"}",
    );
    assert!(request.contains("## アイデアメモ"), "{request}");
    let questions: Vec<&str> = outcome
        .findings
        .iter()
        .map(|f| f.message.as_str())
        .collect();
    assert_eq!(
        questions,
        [
            "灯の転校はいつ決まったのですか？",
            "屋上の鍵を主人公が使える理由は？"
        ]
    );
    assert!(
        outcome
            .findings
            .iter()
            .all(|f| f.line.is_none() && f.scene.is_none())
    );

    let (outcome, _) = run(
        &root,
        &project,
        &task(&project, "idea_sort"),
        None,
        "{\"file\": \"ideas/rooftop-key.md\", \"status\": \"保留\", \"tags\": [\"伏線候補\"], \"reason\": \"屋上の謎として残す\"}\n{\"file\": \"ideas/none.md\", \"status\": \"没\"}\n{\"file\": \"ideas/tsumugi-knows.md\", \"status\": \"たぶん\"}",
    );
    assert_eq!(outcome.ideas.len(), 1, "{:?}", outcome.ideas);
    let s = outcome.ideas.first().unwrap();
    assert_eq!(
        (s.file.as_str(), s.status.as_deref(), s.tags.as_slice()),
        (
            "ideas/rooftop-key.md",
            Some("保留"),
            &["伏線候補".to_owned()][..]
        )
    );
    assert!(outcome.unread.iter().any(|u| u.contains("ideas/none.md")));
    let _ = fs::remove_dir_all(&root);
}

/// レビュー：分岐の薄さの指摘の文脈に、コアが計算した構造の検証の結果（ルートの分量、エンディングへの到達、
/// カバレッジ、着かないシーンなど）が入る。
#[test]
fn review_context_has_core_results() {
    let root = houkago_copy("review");
    let project = load_project(&root).unwrap();
    let (_, request) = run(
        &root,
        &project,
        &task(&project, "thin_branches"),
        None,
        "{\"scene\": \"s012\", \"message\": \"x\"}",
    );
    for expected in [
        "構造の検証の結果",
        "灯ルート：",
        "bad_end（s061）：着かない",
        "akari_end（s040）：着く",
        "シーン 11/13",
        "W509 s061",
    ] {
        assert!(request.contains(expected), "{expected}：{request}");
    }
    let _ = fs::remove_dir_all(&root);
}

/// 結果が同じになる選択：どれを選んでも、行き先と変数の書き込みが同じ選択だけ。
#[test]
fn same_outcome_choices() {
    let dir = std::env::temp_dir().join(format!("scn-plot-same-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("story")).unwrap();
    fs::write(
        dir.join("project.toml"),
        "format_version = 0\nstart_scene = \"s1\"\n",
    )
    .unwrap();
    fs::write(
        dir.join("story/a.scn"),
        "## a #s1\n@選択 ^aaaa\n- 右 -> s2 ^aaab\n- 左 -> s2 ^aaac\n@end\n\n## b #s2\n@選択 ^bbba\n- 右 ^bbbb\n  @設定 x += 1\n  -> s3\n- 左 -> s3 ^bbbc\n@end\n\n## c #s3\n@エンディング end\n",
    )
    .unwrap();
    let project = load_project(&dir).unwrap();
    let ir = analyze(&project);
    assert_eq!(
        ai::same_outcome_choices(&ir),
        [(
            "s1".to_owned(),
            Some("aaaa".to_owned()),
            Some("s2".to_owned())
        )]
    );
    let text = ai::structure_text(&project, &ir);
    assert!(
        text.contains("- s1 の選択（^aaaa）：どれを選んでも s2 へ進み、変数の書き込みも同じ"),
        "{text}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// 案ごとに .scn をコードブロックで囲み、見出しと理由をブロックの外に書く応答も読む。選択肢もスタブも無い節
/// （`### まとめ`）は案にしない。
#[test]
fn proposals_with_fences_per_section() {
    let project =
        load_project(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago"))
            .unwrap();
    let ir = analyze(&project);
    let text = "### 提案 1：灯を追いかける
```scn
- 灯を追いかける
  -> s013a
```
理由：葛藤に向き合う

### 提案 2：その場に残る
```scn
- その場に残る -> s015
## 紬と会う #s015
---
summary: 紬と会う
---
```

### まとめ
どちらも灯の葛藤に結びつきます。
";
    let found = ai::parse_branch_proposals(text, &ir, "s014");
    let names: Vec<&str> = found.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["灯を追いかける", "その場に残る"], "{found:?}");
    let first = found.first().unwrap();
    assert_eq!(
        (first.target.as_str(), first.reason.as_deref()),
        ("s013a", Some("葛藤に向き合う"))
    );
    assert!(first.problems.is_empty(), "{first:?}");
    let second = found.get(1).unwrap();
    assert_eq!(second.stubs.first().unwrap().id, "s015");
    assert!(second.problems.is_empty(), "{second:?}");
    // 骨格も、ブロックの外のシーンの見出しを読む。
    let (scenes, _) =
        ai::parse_skeleton("## 約束 #s070\n```scn\n---\nsummary: x\n---\n-> s060\n```\n");
    assert_eq!(scenes.len(), 1);
    assert_eq!(scenes.first().unwrap().lines, ["-> s060"]);
    // 同じ見出しをブロックの外と中にくり返しても、1つのシーン。
    let (scenes, _) = ai::parse_skeleton(
        "## 約束 #s070\n```scn\n## 約束 #s070\n---\nsummary: x\n---\n-> s060\n```\n",
    );
    assert_eq!(scenes.len(), 1, "{scenes:?}");
    assert_eq!(scenes.first().unwrap().summary.as_deref(), Some("x"));
}

/// 複数行の要約は、1行にした案にする（キャッシュの要約は変えない）。要約を作り直すシーン。
#[test]
fn drift_of_multiline_summary() {
    let root = houkago_copy("multiline");
    let project = load_project(&root).unwrap();
    let ir = analyze(&project);
    let mut cache = ai::SummaryCache::default();
    ai::store_summary(
        &root,
        &mut cache,
        "s013a",
        ai::scene_source(&project, &ir, "s013a").unwrap(),
        "灯が転校を告げる。\n主人公は黙って受け止める。",
    )
    .unwrap();
    let drift = ai::summary_drift(&project, &ir, &cache);
    let d = drift.first().unwrap();
    assert_eq!(d.proposed, "灯が転校を告げる。主人公は黙って受け止める。");
    // 今の本文から作った要約（キャッシュの要約のまま）。本文が変われば無い。
    assert_eq!(
        ai::fresh_summary(&project, &ir, &cache, "s013a").as_deref(),
        Some("灯が転校を告げる。\n主人公は黙って受け止める。")
    );
    let mut changed = project.clone();
    let f = changed
        .files
        .iter_mut()
        .find(|f| f.path == "story/chapter01.scn")
        .unwrap();
    f.text = f.text.replace(
        "summary: 灯が転校を打ち明ける。主人公は受け止める",
        "summary: 直した",
    );
    let changed_ir = analyze(&changed);
    assert_eq!(
        ai::fresh_summary(&changed, &changed_ir, &cache, "s013a"),
        None
    );
    assert_eq!(ai::one_line("A summary\nof x"), "A summary of x");
    // 要約を作り直すシーン：キャッシュに無いか本文が変わったもの（s013a は要約がある）。
    let refresh = ai::summaries_to_refresh(&project, &ir, &cache, Some("story/chapter01.scn"));
    assert!(!refresh.contains(&"s013a".to_owned()));
    assert!(refresh.contains(&"s010".to_owned()), "{refresh:?}");
    assert!(refresh.iter().all(|id| {
        ir.scenes
            .iter()
            .any(|s| &s.id == id && s.file == "story/chapter01.scn")
    }));
    let _ = fs::remove_dir_all(&root);
}
