//! 自律（T3-9）：模擬のサーバー（決めた順に応答する）で、展開メモ → 本文 → 点検 → 直す → 書き込む、を確かめる。
//! スタブへの書き込み（人の行が変わらない）、`ai-draft`、出自、反復の上限、トークン数の上限、取り消し。

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use scn_core::ai::{
    self, AiError, AiTask, AutoEvent, AutoHost, CallOptions, ChatRequest, Completion, Connection,
    ContextSources, Provider,
};
use scn_core::line_id::LineIdGenerator;
use scn_core::provenance::read_provenance;
use scn_core::workspace::Workspace;
use scn_core::{Project, analyze, load_project};

/// 応答を、受け取った頼み（本文）から作る。
type Reply = Box<dyn Fn(&str) -> String + Send>;

/// 決めた順に応答する模擬の OpenAI 互換サーバー。受け取った頼み（本文）を覚える。
fn serve_sequence(replies: Vec<&str>) -> (String, Arc<Mutex<Vec<String>>>) {
    serve_replies(
        replies
            .into_iter()
            .map(|r| {
                let r = r.to_owned();
                Box::new(move |_: &str| r.clone()) as Reply
            })
            .collect(),
    )
}

fn serve_replies(replies: Vec<Reply>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let received = Arc::new(Mutex::new(Vec::new()));
    let slot = Arc::clone(&received);
    thread::spawn(move || {
        for reply in replies {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = 0;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 {
                    break;
                }
                if line.trim().is_empty() {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let body = String::from_utf8(body).unwrap();
            let content = reply(&body);
            slot.lock().unwrap().push(body);
            let reply = serde_json::json!({
                "choices": [{"message": {"content": content}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 100, "completion_tokens": 10}
            })
            .to_string();
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            );
        }
    });
    (format!("http://{addr}/v1"), received)
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
    let dir = std::env::temp_dir().join(format!("scn-auto-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    copy_dir(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago"),
        &dir,
    );
    dir
}

struct TestHost {
    root: PathBuf,
    ws: Workspace,
    conn: Connection,
    task: String,
    generator: LineIdGenerator,
    events: Vec<AutoEvent>,
    cancel: bool,
    /// この数だけ呼んだら取り消す。
    cancel_after: Option<usize>,
    calls: usize,
}

impl AutoHost for TestHost {
    fn project(&mut self) -> Result<Project, String> {
        Ok(self.ws.project().clone())
    }
    fn sources(&mut self) -> ContextSources {
        ai::read_sources(&self.root)
    }
    fn connection(&mut self, _task: &AiTask) -> Result<(Connection, Option<String>), String> {
        Ok((self.conn.clone(), None))
    }
    fn complete(
        &mut self,
        conn: &Connection,
        key: Option<&str>,
        request: &ChatRequest,
    ) -> Result<Completion, AiError> {
        self.calls += 1;
        let options = CallOptions {
            connect_timeout: Duration::from_secs(2),
            idle_timeout: Duration::from_secs(5),
        };
        ai::complete(
            conn,
            key,
            request,
            &options,
            &AtomicBool::new(false),
            &mut |_| {},
        )
    }
    fn write(&mut self, file: &str, scene: &str, body: &[String]) -> Result<Vec<String>, String> {
        let disk = fs::read_to_string(self.root.join(file)).ok();
        let written = self
            .ws
            .write_draft(
                file,
                scene,
                body,
                &self.task,
                "test-model",
                disk.as_deref(),
                &mut self.generator,
            )
            .map_err(|e| e.to_string())?;
        match written.refused {
            Some(why) => Err(why),
            None => Ok(written.ids),
        }
    }
    fn event(&mut self, event: AutoEvent) {
        self.events.push(event);
    }
    fn cancelled(&self) -> bool {
        self.cancel || self.cancel_after.is_some_and(|n| self.calls >= n)
    }
}

fn host(root: &Path, url: String, task: &str) -> TestHost {
    TestHost {
        root: root.to_owned(),
        ws: Workspace::open(root).unwrap(),
        conn: Connection {
            name: "local".to_owned(),
            provider: Provider::OpenaiCompatible,
            endpoint: format!("openai-compatible {url}"),
            base_url: url,
            model: "test-model".to_owned(),
            cloud: false,
            allowed: true,
            context_tokens: 200_000,
        },
        task: task.to_owned(),
        generator: LineIdGenerator::with_seed(3),
        events: Vec::new(),
        cancel: false,
        cancel_after: None,
        calls: 0,
    }
}

fn draft_chapter(root: &Path) -> (AiTask, Vec<AiTask>) {
    let project = load_project(root).unwrap();
    let task = project
        .ai_tasks
        .iter()
        .find(|t| t.id == "draft_chapter")
        .unwrap()
        .clone();
    (task, project.ai_tasks.clone())
}

/// 展開メモ → 本文（未登録の話者で W202）→ 点検 → 直す → 点検 → 書き込む。
#[test]
fn auto_drafts_and_fixes() {
    let root = houkago_copy("fix");
    let file = "story/chapter02.scn";
    let before = fs::read_to_string(root.join(file)).unwrap();
    let (url, received) = serve_sequence(vec![
        "- 雨で屋上に行けない\n- 図書室で灯の母の話を聞く",
        "謎の人「雨だね」\n　図書室は静かだった。",
        "",
        "",
        "灯「雨、やまないね」\n　図書室は静かだった。",
        "",
        "",
    ]);
    let (task, all) = draft_chapter(&root);
    let mut h = host(&root, url, "draft_chapter");
    let outcome = ai::run_auto(&mut h, &task, file, &all);
    assert_eq!(outcome.stopped, None, "{outcome:?}");
    assert_eq!(outcome.scenes.len(), 1);
    let s022 = outcome.scenes.first().unwrap();
    assert_eq!((s022.scene.as_str(), s022.status), ("s022", "written"));
    assert_eq!(s022.iterations, 1);
    assert!(s022.issues.is_empty(), "{:?}", s022.issues);
    assert_eq!(s022.ids.len(), 2);
    assert_eq!(outcome.tokens, 7 * 110);
    // 直しの頼みに、点検の結果（W202）が入っている。
    let sent = received.lock().unwrap();
    assert_eq!(sent.len(), 7);
    assert!(
        sent.get(4).unwrap().contains("W202"),
        "{}",
        sent.get(4).unwrap()
    );
    drop(sent);
    let after = fs::read_to_string(root.join(file)).unwrap();
    assert!(after.contains("灯「雨、やまないね」 ^"));
    assert!(!after.contains("謎の人"));
    // 前の行はそのまま（s022 の status と、保存で付いた行IDを除く）。
    let mut rest = after.lines();
    for line in before.lines() {
        let found = rest.by_ref().any(|l| {
            l == line
                || (l.starts_with(line) && l.get(line.len()..).is_some_and(|t| t.starts_with(" ^")))
                || (line.starts_with("status:") && l == "status: ai-draft")
        });
        assert!(found, "前の行が無くなった：{line}");
    }
    let project = load_project(&root).unwrap();
    let ir = analyze(&project);
    assert_eq!(
        ir.scenes
            .iter()
            .find(|s| s.id == "s022")
            .unwrap()
            .meta
            .status
            .as_deref(),
        Some("ai-draft")
    );
    let provenance = read_provenance(&root).unwrap();
    for id in &s022.ids {
        let r = provenance
            .records
            .iter()
            .rev()
            .find(|r| &r.line == id)
            .unwrap();
        assert_eq!(
            (r.origin.as_str(), r.task.as_str()),
            ("ai", "draft_chapter")
        );
    }
    let steps: Vec<&str> = h
        .events
        .iter()
        .filter_map(|e| match e {
            AutoEvent::Step { step, .. } => Some(*step),
            _ => None,
        })
        .collect();
    assert_eq!(steps, ["beats", "draft", "check", "fix", "check", "write"]);
    let _ = fs::remove_dir_all(&root);
}

/// 反復の上限に達したら、点検の結果を残したまま下書きとして書き込む。
#[test]
fn auto_stops_at_iteration_limit() {
    let root = houkago_copy("iter");
    let file = "story/chapter02.scn";
    let (url, _) = serve_sequence(vec![
        "- メモ",
        "謎の人「一」",
        "",
        "",
        "謎の人「二」",
        "",
        "",
    ]);
    let (mut task, all) = draft_chapter(&root);
    task.max_iterations = Some(1);
    let mut h = host(&root, url, "draft_chapter");
    let outcome = ai::run_auto(&mut h, &task, file, &all);
    let s022 = outcome.scenes.first().unwrap();
    assert_eq!((s022.status, s022.iterations), ("written", 1));
    assert!(
        s022.issues.iter().any(|i| i.starts_with("W202")),
        "{:?}",
        s022.issues
    );
    assert!(
        fs::read_to_string(root.join(file))
            .unwrap()
            .contains("謎の人「二」")
    );
    let _ = fs::remove_dir_all(&root);
}

/// トークン数の上限に達したら、次のシーンを始めない。取り消したら何も送らない。
#[test]
fn auto_budget_and_cancel() {
    let root = std::env::temp_dir().join(format!("scn-auto-budget-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("story")).unwrap();
    fs::write(
        root.join("project.toml"),
        "format_version = 0\nstart_scene = \"s1\"\n",
    )
    .unwrap();
    fs::write(root.join("style.md"), "# 文体\n短く書く。\n").unwrap();
    fs::write(
        root.join("story/ch.scn"),
        "## 始まり #s1\n地の文。 ^aaaa\n-> s2\n\n## 二つめ #s2\n-> s3\n\n## 三つめ #s3\n",
    )
    .unwrap();
    let task = ai::parse_task(
        "auto",
        "ai/auto.yaml",
        &yaml_serde::from_str(
            "name: 自律\nlevel: auto\ncontext: [style]\nchecks: []\nbudget: { tokens: 200 }\nprompt: 書いてください\n",
        )
        .unwrap(),
    )
    .0;
    let (url, received) =
        serve_sequence(vec!["- メモ", "　雨が降っていた。", "- メモ", "　晴れた。"]);
    let mut h = host(&root, url.clone(), "auto");
    let outcome = ai::run_auto(&mut h, &task, "story/ch.scn", std::slice::from_ref(&task));
    assert_eq!(outcome.scenes.len(), 1, "{outcome:?}");
    assert_eq!(outcome.scenes.first().unwrap().status, "written");
    assert!(outcome.stopped.unwrap().contains("上限"));
    assert_eq!(received.lock().unwrap().len(), 2);
    let text = fs::read_to_string(root.join("story/ch.scn")).unwrap();
    assert!(text.contains("## 二つめ #s2\n---\nstatus: ai-draft\n---\n　雨が降っていた。 ^"));
    assert!(text.ends_with("## 三つめ #s3\n"));
    // 上限に達したら、点検の段を始めない（できている下書きは書く）。
    let steps: Vec<&str> = h
        .events
        .iter()
        .filter_map(|e| match e {
            AutoEvent::Step { step, .. } => Some(*step),
            _ => None,
        })
        .collect();
    assert_eq!(steps, ["beats", "draft", "write"]);
    assert_eq!(
        outcome.scenes.first().unwrap().message.as_deref(),
        Some("点検の途中で止めました")
    );

    let mut h = host(&root, url, "auto");
    h.cancel = true;
    let outcome = ai::run_auto(&mut h, &task, "story/ch.scn", std::slice::from_ref(&task));
    assert!(outcome.scenes.is_empty());
    assert_eq!(outcome.stopped.as_deref(), Some("取り消しました"));
    let _ = fs::remove_dir_all(&root);
}

/// 本文を書いた後に取り消したら、書き込まない。
#[test]
fn auto_cancel_after_draft_does_not_write() {
    let root = houkago_copy("cancel");
    let file = "story/chapter02.scn";
    let before = fs::read_to_string(root.join(file)).unwrap();
    let (url, received) = serve_sequence(vec!["- メモ", "灯「雨」"]);
    let (task, all) = draft_chapter(&root);
    let mut h = host(&root, url, "draft_chapter");
    h.cancel_after = Some(2);
    let outcome = ai::run_auto(&mut h, &task, file, &all);
    assert_eq!(outcome.stopped.as_deref(), Some("取り消しました"));
    let s022 = outcome.scenes.first().unwrap();
    assert_eq!(s022.status, "skipped");
    assert_eq!(
        s022.message.as_deref(),
        Some("取り消したので書き込みませんでした")
    );
    assert_eq!(received.lock().unwrap().len(), 2);
    assert_eq!(fs::read_to_string(root.join(file)).unwrap(), before);
    let _ = fs::remove_dir_all(&root);
}

/// 点検には、下書きの範囲の表記の検査（一人称の表記ゆれ I204 など）も入る。
#[test]
fn auto_inspects_wording() {
    let root = houkago_copy("wording");
    let file = "story/chapter02.scn";
    let (url, _) = serve_sequence(vec!["- メモ", "灯「私も行く」", "", ""]);
    let (mut task, all) = draft_chapter(&root);
    task.max_iterations = Some(0);
    let mut h = host(&root, url, "draft_chapter");
    let outcome = ai::run_auto(&mut h, &task, file, &all);
    let s022 = outcome.scenes.first().unwrap();
    assert_eq!(s022.status, "written", "{outcome:?}");
    assert!(
        s022.issues.iter().any(|i| i.starts_with("I204")),
        "{:?}",
        s022.issues
    );
    let _ = fs::remove_dir_all(&root);
}

/// 直しの応答が、点検のために付けた仮の行IDを残して返しても、その行を読んで書き込む（書く行IDは付け直す）。
#[test]
fn auto_fix_reply_with_temp_ids() {
    let root = houkago_copy("temp");
    let file = "story/chapter02.scn";
    let fixed = |body: &str| {
        // 直しの頼みの今のシーンにある、下書きの行の仮の行ID。
        let at = body.find("謎の人「雨だね」 ^").unwrap() + "謎の人「雨だね」 ^".len();
        let id: String = body[at..]
            .chars()
            .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            .collect();
        format!("```\n灯「雨、やまないね」 ^{id}\n```")
    };
    let empty = || Box::new(|_: &str| String::new()) as Reply;
    let (url, _) = serve_replies(vec![
        Box::new(|_: &str| "- メモ".to_owned()),
        Box::new(|_: &str| "謎の人「雨だね」".to_owned()),
        empty(),
        empty(),
        Box::new(move |b: &str| fixed(b)),
        empty(),
        empty(),
    ]);
    let (task, all) = draft_chapter(&root);
    let mut h = host(&root, url, "draft_chapter");
    let outcome = ai::run_auto(&mut h, &task, file, &all);
    let s022 = outcome.scenes.first().unwrap();
    assert_eq!(
        (s022.status, s022.iterations),
        ("written", 1),
        "{outcome:?}"
    );
    assert!(s022.issues.is_empty(), "{:?}", s022.issues);
    let after = fs::read_to_string(root.join(file)).unwrap();
    let id = s022.ids.first().unwrap();
    assert!(
        after.contains(&format!("灯「雨、やまないね」 ^{id}\n")),
        "{after}"
    );
    assert!(!after.contains("謎の人"));
    let _ = fs::remove_dir_all(&root);
}
