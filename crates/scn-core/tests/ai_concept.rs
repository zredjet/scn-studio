//! 企画書の下書き（T4-4、D080）：応答の読み方、空いている項目にだけ書くこと（人の項目は提案）、`endings` の
//! 書き方、本文がひな形のままなら書くこと、企画書が無ければ作ること、提案の採用、出自、模擬のサーバーでの実行。

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use scn_core::ai::{self, CallOptions, Connection, Provider};
use scn_core::provenance::{AI, BODY_KEY, read_provenance};
use scn_core::scaffold::CONCEPT_TEMPLATE;
use scn_core::workspace::{EntryEdit, Workspace};
use scn_core::{analyze, load_project};
use serde_json::{Value, json};

fn temp_project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scn-core-concept-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    for (path, text) in files {
        let path = dir.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    dir
}

const DRAFT: &str = r##"下書きです。
```json
{
  "logline": "AIの一文",
  "theme": "言わないことは優しさか",
  "tone": "静か",
  "volume": {"chars": 120000, "routes": 2},
  "main_characters": ["akari", "minato"],
  "endings": [
    {"id": "akari_end", "name": "灯エンド", "intent": "見送る"},
    {"id": "true_end", "name": "トゥルー", "intent": "残る理由"}
  ],
  "body": "# 企画書\n\n## やりたいこと\n\n屋上の三週間。\n",
  "unknown": "x"
}
```
"##;

/// 応答の読み方：コードブロックと前置きの文があっても読む。知らないキーと形の合わない値は読まずに知らせる。
#[test]
fn parse_concept_draft() {
    let (draft, notes) = ai::parse_concept_draft(DRAFT);
    let draft = draft.unwrap();
    assert_eq!(draft.values["logline"], json!("AIの一文"));
    assert_eq!(
        draft.values["volume"],
        json!({"chars": 120000, "routes": 2})
    );
    assert_eq!(draft.values["endings"].as_array().unwrap().len(), 2);
    assert_eq!(
        draft.body.as_deref(),
        Some("# 企画書\n\n## やりたいこと\n\n屋上の三週間。\n")
    );
    assert!(!draft.values.contains_key("unknown"));
    assert!(notes.iter().any(|n| n.contains("unknown")), "{notes:?}");

    let (draft, notes) = ai::parse_concept_draft(
        r#"{"logline": "", "theme": 3, "tone": "改\n行", "volume": {"chars": -1, "routes": 2, "pages": 3},
            "main_characters": ["akari", ""], "endings": [{"id": "bad id"}, {"id": "ok", "name": "良", "intent": ""}, {"id": "ok"}]}"#,
    );
    let draft = draft.unwrap();
    assert!(!draft.values.contains_key("logline"));
    assert!(!draft.values.contains_key("theme"));
    assert!(!draft.values.contains_key("tone"));
    assert_eq!(draft.values["volume"], json!({"routes": 2}));
    assert_eq!(draft.values["main_characters"], json!(["akari"]));
    assert_eq!(draft.values["endings"], json!([{"id": "ok", "name": "良"}]));
    assert!(notes.len() >= 8, "{notes:?}");
    assert!(
        notes.iter().any(|n| n.contains("volume.pages")),
        "{notes:?}"
    );
    assert!(
        notes.iter().any(|n| n.contains("ok の intent")),
        "{notes:?}"
    );
    let (_, notes) = ai::parse_concept_draft(r#"{"volume": {}}"#);
    assert!(
        notes.iter().any(|n| n.starts_with("volume は")),
        "{notes:?}"
    );

    let (draft, notes) = ai::parse_concept_draft("企画書は書けませんでした。");
    assert!(draft.is_none());
    assert_eq!(notes, ["企画書は書けませんでした。"]);
}

fn project_with_concept(name: &str, concept: Option<&str>) -> PathBuf {
    let mut files = vec![
        ("project.toml", "format_version = 0\n"),
        ("story/a.scn", "## a #s1\n　地の文。\n"),
    ];
    if let Some(concept) = concept {
        files.push(("concept.md", concept));
    }
    temp_project(name, &files)
}

/// 空いている項目（無い、空の文字列、空のリスト）と、ひな形のままの本文にだけ書く。人が書いた項目は変えずに
/// 提案にする。`endings` はブロックのリスト。書いた項目ごとに出自 `ai` を記録する。
#[test]
fn writes_empty_fields_only() {
    let concept = CONCEPT_TEMPLATE.replace("logline: \"\"", "logline: 人が書いた一文");
    let dir = project_with_concept("empty", Some(&concept));
    let mut ws = Workspace::open(&dir).unwrap();
    let (draft, _) = ai::parse_concept_draft(DRAFT);
    let done = ws
        .write_concept_draft(&draft.unwrap(), "draft_concept", "m")
        .unwrap();
    assert_eq!(done.provenance_problem, None);
    let mut written = done.written.clone();
    written.sort();
    assert_eq!(
        written,
        [
            BODY_KEY,
            "endings",
            "main_characters",
            "theme",
            "tone",
            "volume"
        ]
    );
    assert_eq!(done.proposals.len(), 1);
    let p = &done.proposals[0];
    assert_eq!(
        (p.file.as_str(), p.key.as_str(), &p.current, &p.proposed),
        (
            "concept.md",
            "logline",
            &json!("人が書いた一文"),
            &json!("AIの一文")
        )
    );
    let text = fs::read_to_string(dir.join("concept.md")).unwrap();
    assert_eq!(text, done.text);
    assert!(text.contains("logline: 人が書いた一文\n"), "{text}");
    assert!(
        text.contains("endings:\n  - { id: akari_end, name: 灯エンド, intent: 見送る }\n  - { id: true_end, name: トゥルー, intent: 残る理由 }\n"),
        "{text}"
    );
    assert!(
        text.contains("# エンディング（本文の @エンディング のIDと照らす）。例："),
        "{text}"
    );
    assert!(text.ends_with("屋上の三週間。\n"), "{text}");
    // 書いた企画書は読める（エンディングを照らせる）。
    let project = load_project(&dir).unwrap();
    let endings = project.concept.unwrap().endings.unwrap();
    assert_eq!(endings.len(), 2);
    let provenance = read_provenance(&dir).unwrap();
    let fields = provenance.latest_fields();
    assert_eq!(fields.len(), 6);
    assert!(
        fields
            .values()
            .all(|r| r.origin == AI && r.task == "draft_concept")
    );
    assert!(!fields.contains_key(&("concept.md", "logline")));

    // もう一度書いても、値のある項目には書かず、同じ値なら提案にもしない。
    let (draft, _) = ai::parse_concept_draft(DRAFT);
    let again = ws
        .write_concept_draft(&draft.unwrap(), "draft_concept", "m")
        .unwrap();
    assert!(again.written.is_empty());
    assert_eq!(again.proposals.len(), 1);
    assert_eq!(read_provenance(&dir).unwrap().records.len(), 6);
    let _ = fs::remove_dir_all(&dir);
}

/// 人が書いた本文（ひな形でない）には書かず、提案にする。企画書が無ければ、ひな形で作ってから書く。
#[test]
fn body_and_missing_concept() {
    let concept = "---\nlogline: \"\"\n---\n# 企画書\n\n人が書いた本文。\n";
    let dir = project_with_concept("body", Some(concept));
    let mut ws = Workspace::open(&dir).unwrap();
    let (draft, _) = ai::parse_concept_draft(DRAFT);
    let done = ws
        .write_concept_draft(&draft.unwrap(), "draft_concept", "m")
        .unwrap();
    assert!(!done.written.contains(&BODY_KEY.to_owned()));
    assert!(done.proposals.iter().any(|p| p.key == BODY_KEY));
    assert!(done.text.ends_with("人が書いた本文。\n"));
    let _ = fs::remove_dir_all(&dir);

    let dir = project_with_concept("missing", None);
    let mut ws = Workspace::open(&dir).unwrap();
    let (draft, _) = ai::parse_concept_draft(DRAFT);
    let done = ws
        .write_concept_draft(&draft.unwrap(), "draft_concept", "m")
        .unwrap();
    assert!(done.proposals.is_empty());
    assert_eq!(done.written.len(), 7);
    assert!(
        fs::read_to_string(dir.join("concept.md"))
            .unwrap()
            .contains("logline: AIの一文")
    );
    let _ = fs::remove_dir_all(&dir);

    // CRLF の企画書でも、ひな形のままの本文には書く。
    let dir = project_with_concept("crlf", Some(&CONCEPT_TEMPLATE.replace('\n', "\r\n")));
    let mut ws = Workspace::open(&dir).unwrap();
    let (draft, _) = ai::parse_concept_draft(DRAFT);
    let done = ws
        .write_concept_draft(&draft.unwrap(), "draft_concept", "m")
        .unwrap();
    assert!(
        done.written.contains(&BODY_KEY.to_owned()),
        "{:?}",
        done.written
    );
    let _ = fs::remove_dir_all(&dir);

    // 読めない企画書には書かない。
    let dir = project_with_concept("broken", Some("---\nlogline: \"閉じない\n---\n"));
    let mut ws = Workspace::open(&dir).unwrap();
    let (draft, _) = ai::parse_concept_draft(DRAFT);
    assert!(
        ws.write_concept_draft(&draft.unwrap(), "draft_concept", "m")
            .is_err()
    );
    let _ = fs::remove_dir_all(&dir);
}

/// 提案の採用（人の操作）：項目の値を案で置き換え（リストも丸ごと）、出自 `ai`。外でその項目が変わっていれば書かない。
#[test]
fn adopt_field_proposal() {
    let concept = "---\nlogline: 人の一文\nendings:\n  - { id: a, name: 人 }\n---\n人の本文。\n";
    let dir = project_with_concept("adopt", Some(concept));
    let mut ws = Workspace::open(&dir).unwrap();
    let seen = fs::read_to_string(dir.join("concept.md")).unwrap();
    let edit = ws
        .adopt_field_proposal(
            "concept.md",
            "endings",
            &json!([{"id": "b", "name": "AI"}]),
            &seen,
            "draft_concept",
            "m",
        )
        .unwrap();
    let EntryEdit::Saved { text } = edit else {
        panic!("{edit:?}")
    };
    assert!(
        text.contains("endings:\n  - { id: b, name: AI }\n---\n"),
        "{text}"
    );
    let edit = ws
        .adopt_field_proposal(
            "concept.md",
            BODY_KEY,
            &json!("AIの本文。\n"),
            &text,
            "draft_concept",
            "m",
        )
        .unwrap();
    let EntryEdit::Saved { text } = edit else {
        panic!("{edit:?}")
    };
    assert!(text.ends_with("---\nAIの本文。\n"), "{text}");
    let fields = read_provenance(&dir).unwrap();
    let fields = fields.latest_fields();
    assert_eq!(fields[&("concept.md", "endings")].origin, AI);
    assert_eq!(fields[&("concept.md", BODY_KEY)].origin, AI);
    // 外でその項目が変わっていた。
    fs::write(
        dir.join("concept.md"),
        text.replace("logline: 人の一文", "logline: 外で直した"),
    )
    .unwrap();
    let edit = ws
        .adopt_field_proposal(
            "concept.md",
            "logline",
            &json!("AIの一文"),
            &text,
            "draft_concept",
            "m",
        )
        .unwrap();
    assert!(matches!(edit, EntryEdit::Conflict { .. }), "{edit:?}");
    // 企画書とエントリでないファイルには書かない。
    assert!(
        ws.adopt_field_proposal("ideas/x.md", "status", &json!("採用"), "", "t", "m")
            .is_err()
    );
    let _ = fs::remove_dir_all(&dir);
}

/// 模擬の OpenAI 互換サーバー：頼みを1つ受け取り、決めた本文をまとめて返す。
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
        let reply = json!({
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

/// 模擬のサーバーで、サンプルの企画書の下書きのタスクを実行する：企画書・メモ・キャラクター・文体を送り、
/// 構造化出力では返す形 `concept` を付け、応答から下書きを読む。
#[test]
fn run_concept_task() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago");
    let project = load_project(&root).unwrap();
    let ir = analyze(&project);
    let sources = ai::read_sources(&root);
    let task = project
        .ai_tasks
        .iter()
        .find(|t| t.id == "draft_concept")
        .unwrap();
    assert_eq!(task.output, ai::TaskOutput::Concept);
    assert!(!ai::is_skeleton(task));
    let (url, received) = serve(DRAFT);
    let conn = Connection {
        name: "local".to_owned(),
        provider: Provider::OpenaiCompatible,
        endpoint: format!("openai-compatible {url}"),
        base_url: url,
        model: "test-model".to_owned(),
        cloud: false,
        allowed: true,
        context_tokens: 32768,
        output_tokens: None,
        structured_output: true,
    };
    let outcome = ai::run_task(
        &project,
        &ir,
        &sources,
        task,
        None,
        &conn,
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
    let draft = outcome.concept.unwrap();
    assert_eq!(draft.values["theme"], json!("言わないことは優しさか"));
    let sent: Value = serde_json::from_str(&received.lock().unwrap()).unwrap();
    assert_eq!(sent["response_format"]["json_schema"]["name"], "concept");
    let system = sent["messages"][0]["content"].as_str().unwrap();
    assert!(system.contains("\"logline\""), "{system}");
    let user = sent["messages"][1]["content"].as_str().unwrap();
    assert!(user.contains("企画書"), "{user}");
}
