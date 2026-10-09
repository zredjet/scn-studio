//! 設定の下書き（T4-5、D080）：足りない設定の求め方、文脈の節（設定DBの型・足りない設定）、応答の読み方、
//! 新しいエントリ（`status: ai-draft`、型の順、出自）、今あるエントリの空欄と案、対象のエントリだけに書くこと、
//! 模擬のサーバーでの実行。

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
use scn_core::workspace::Workspace;
use scn_core::{analyze, load_project};
use serde_json::{Value, json};

fn temp_project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scn-core-entries-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    for (path, text) in files {
        let path = dir.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    dir
}

const AKARI: &str = "bible/entries/character/akari.md";

fn project(name: &str) -> PathBuf {
    temp_project(
        name,
        &[
            ("project.toml", "format_version = 0\n"),
            (
                "story/a.scn",
                "## 屋上 #s1\n灯「やあ」\n店員「いらっしゃい」\n店員「またどうぞ」\n",
            ),
            (
                "concept.md",
                "---\nlogline: 一文\nmain_characters: [akari, sora]\n---\n本文。\n",
            ),
            (
                "bible/types/character.yaml",
                "name: キャラクター\nbuiltin: character\nfields:\n  - { key: aliases, label: 呼び名, type: list<text> }\n  - { key: first_person, label: 一人称, type: text }\n  - { key: age, label: 年齢, type: number }\n  - { key: org, label: 所属, type: ref<place> }\n",
            ),
            (
                "bible/types/place.yaml",
                "name: 場所\nfields:\n  - { key: description, label: 説明, type: longtext }\n",
            ),
            (
                AKARI,
                "---\nid: akari\nname: 灯\nfirst_person: わたし\norg: rooftop_club\n---\n",
            ),
        ],
    )
}

/// 足りない設定：本文の未登録の話者（回数）、企画書の主要キャラにあってエントリの無いキャラ、参照先の無い参照。
/// 今あるエントリの空いている項目。
#[test]
fn missing_entries_and_empty_fields() {
    let dir = project("missing");
    let project = load_project(&dir).unwrap();
    let ir = analyze(&project);
    let concept = fs::read_to_string(dir.join("concept.md")).unwrap();
    let missing = ai::missing_entries(&project, &ir, Some(&concept));
    let got: Vec<(&str, Option<&str>, Option<&str>)> = missing
        .iter()
        .map(|m| (m.type_id.as_str(), m.id.as_deref(), m.name.as_deref()))
        .collect();
    assert_eq!(
        got,
        [
            ("character", None, Some("店員")),
            ("character", Some("sora"), None),
            ("place", Some("rooftop_club"), None),
        ]
    );
    assert!(missing[0].reason.contains("2回"), "{:?}", missing[0]);
    let empty = ai::empty_fields(&project);
    assert_eq!(empty.len(), 1);
    assert_eq!(empty[0].0, AKARI);
    assert_eq!(empty[0].2, ["aliases", "age", BODY_KEY]);
    let _ = fs::remove_dir_all(&dir);
}

fn entries_task() -> &'static str {
    "name: 設定の下書き\nlevel: generate\nmodel: local\ncontext: [concept, ideas, characters, terms, types, missing_entries]\noutput: entries\nprompt: 足りない設定を作る\n"
}

/// 文脈の節：設定DBの型と足りない設定。文体（style.md）は要らない（本文を書かない生成）。
#[test]
fn context_sections() {
    let dir = project("context");
    fs::create_dir_all(dir.join("ai")).unwrap();
    fs::write(dir.join("ai/draft_entries.yaml"), entries_task()).unwrap();
    let project = load_project(&dir).unwrap();
    let ir = analyze(&project);
    let sources = ai::read_sources(&dir);
    let task = project
        .ai_tasks
        .iter()
        .find(|t| t.id == "draft_entries")
        .unwrap();
    assert_eq!(task.output, ai::TaskOutput::Entries);
    assert!(!ai::is_skeleton(task));
    let assembled = ai::assemble(&project, &ir, &sources, task, None, 32768);
    assert_eq!(assembled.problem, None);
    assert!(
        assembled.user.contains("## 設定DBの型"),
        "{}",
        assembled.user
    );
    assert!(
        assembled.user.contains("first_person（一人称、text）"),
        "{}",
        assembled.user
    );
    assert!(
        assembled.user.contains("## 足りない設定"),
        "{}",
        assembled.user
    );
    assert!(assembled.user.contains("店員"), "{}", assembled.user);
    assert!(
        assembled.user.contains("akari（灯）：呼び名、年齢、本文"),
        "{}",
        assembled.user
    );
    assert!(!assembled.sections.iter().any(|s| s.item == "style"));
    assert!(
        assembled.system.contains("\"entries\""),
        "{}",
        assembled.system
    );
    // 対象のエントリを選んだとき。
    let target = ai::with_entry_target(task, &project, AKARI).unwrap();
    assert!(
        target
            .prompt
            .as_deref()
            .unwrap()
            .contains("対象のエントリ：bible/entries/character/akari.md")
    );
    assert!(ai::with_entry_target(task, &project, "bible/entries/character/none.md").is_err());
    let _ = fs::remove_dir_all(&dir);
}

const DRAFT: &str = r#"```json
{"entries": [
  {"type": "character", "id": "tenin", "name": "店員", "fields": {"first_person": "わたくし", "age": 40, "aliases": ["おばさん"], "org": "rooftop_club", "unknown": 1, "status": "x"}, "body": "駅前の売店の店員。"},
  {"type": "character", "id": "akari", "name": "灯", "fields": {"first_person": "あたし", "age": 17}, "body": "屋上の鍵を持つ。"},
  {"type": "place", "id": "rooftop_club", "name": "屋上部", "fields": {"description": "放課後に屋上で集まる"}},
  {"type": "character", "id": "bad id", "name": "x"},
  {"type": "monster", "id": "m", "name": "怪物"},
  {"type": "character", "id": "tenin", "name": "二つ目"},
  {"type": "character", "id": "sora", "name": "空", "fields": {"age": "十七"}}
]}
```"#;

/// 応答の読み方：読まないもの（型の無い型、IDの形、同じIDの2つ目、型に無い項目、`status`、型に合わない値）は知らせる。
#[test]
fn parse_entries_draft() {
    let dir = project("parse");
    let project = load_project(&dir).unwrap();
    let (draft, notes) = ai::parse_entries_draft(DRAFT, &project);
    let draft = draft.unwrap();
    let ids: Vec<(&str, &str)> = draft
        .entries
        .iter()
        .map(|e| (e.type_id.as_str(), e.id.as_str()))
        .collect();
    assert_eq!(
        ids,
        [
            ("character", "tenin"),
            ("character", "akari"),
            ("place", "rooftop_club"),
            ("character", "sora")
        ]
    );
    let tenin = &draft.entries[0];
    assert_eq!(
        tenin.fields.keys().collect::<Vec<_>>(),
        ["age", "aliases", "first_person", "org"]
    );
    assert!(draft.entries[3].fields.is_empty());
    for expected in ["unknown", "status", "bad id", "monster", "二つ目", "age"] {
        assert!(
            notes.iter().any(|n| n.contains(expected)),
            "{expected}: {notes:?}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

/// 新しいエントリは `status: ai-draft` で作り（項目は型の順）、今あるエントリは空欄だけに書き、値のある項目は案にする。
#[test]
fn write_entries() {
    let dir = project("write");
    let project = load_project(&dir).unwrap();
    let (draft, _) = ai::parse_entries_draft(DRAFT, &project);
    let mut ws = Workspace::open(&dir).unwrap();
    let done = ws
        .write_entries_draft(&draft.unwrap(), "draft_entries", "m", None)
        .unwrap();
    assert_eq!(done.provenance_problem, None);
    // 自律のブランチにコミットするファイル（T4-6）：作ったエントリ、書いたエントリ、出自の記録。
    assert_eq!(
        done.written_paths(),
        [
            "ai/provenance.jsonl",
            AKARI,
            "bible/entries/character/sora.md",
            "bible/entries/character/tenin.md",
            "bible/entries/place/rooftop_club.md"
        ]
    );
    assert_eq!(
        done.created,
        [
            "bible/entries/character/tenin.md",
            "bible/entries/place/rooftop_club.md",
            "bible/entries/character/sora.md"
        ]
    );
    let tenin = fs::read_to_string(dir.join("bible/entries/character/tenin.md")).unwrap();
    assert_eq!(
        tenin,
        "---\nid: tenin\nname: 店員\nstatus: ai-draft\naliases: [おばさん]\nfirst_person: わたくし\nage: 40\norg: rooftop_club\n---\n駅前の売店の店員。\n"
    );
    // 今あるエントリ：空いている age と本文に書き、人の一人称は案。
    let akari = done.files.iter().find(|f| f.file == AKARI).unwrap();
    let mut written = akari.written.clone();
    written.sort();
    assert_eq!(written, [BODY_KEY, "age"]);
    assert_eq!(akari.proposals.len(), 1);
    assert_eq!(akari.proposals[0].key, "first_person");
    assert_eq!(akari.proposals[0].proposed, json!("あたし"));
    let akari_text = fs::read_to_string(dir.join(AKARI)).unwrap();
    assert!(
        akari_text.contains("first_person: わたし\n"),
        "{akari_text}"
    );
    assert!(akari_text.contains("age: 17\n"), "{akari_text}");
    assert!(!akari_text.contains("status:"), "{akari_text}");
    // 出自：新しいエントリの name・項目・本文、今あるエントリの書いた項目。
    let provenance = read_provenance(&dir).unwrap();
    let fields = provenance.latest_fields();
    assert_eq!(
        fields[&("bible/entries/character/tenin.md", "name")].origin,
        AI
    );
    assert_eq!(
        fields[&("bible/entries/character/tenin.md", "org")].origin,
        AI
    );
    assert_eq!(
        fields[&("bible/entries/character/tenin.md", BODY_KEY)].origin,
        AI
    );
    assert_eq!(fields[&(AKARI, "age")].origin, AI);
    assert!(!fields.contains_key(&(AKARI, "first_person")));
    // 作ったエントリは読める（参照切れが消え、話者が解決する）。
    let project = load_project(&dir).unwrap();
    let ir = analyze(&project);
    let concept = fs::read_to_string(dir.join("concept.md")).unwrap();
    let missing = ai::missing_entries(&project, &ir, Some(&concept));
    assert!(missing.is_empty(), "{missing:?}");
    let _ = fs::remove_dir_all(&dir);
}

/// 対象のエントリを選んだときは、そのエントリにだけ書く（新しいエントリは作らない）。
#[test]
fn write_only_target() {
    let dir = project("only");
    let project = load_project(&dir).unwrap();
    let (draft, _) = ai::parse_entries_draft(DRAFT, &project);
    let mut ws = Workspace::open(&dir).unwrap();
    let done = ws
        .write_entries_draft(&draft.unwrap(), "draft_entries", "m", Some(AKARI))
        .unwrap();
    assert!(done.created.is_empty());
    assert_eq!(done.files.len(), 1);
    assert!(
        done.notes.iter().any(|n| n.contains("tenin")),
        "{:?}",
        done.notes
    );
    assert!(!dir.join("bible/entries/character/tenin.md").exists());
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

/// 模擬のサーバーで、サンプルの設定の下書きのタスクを実行する：構造化出力では返す形 `entries` を付け、応答から読む。
#[test]
fn run_entries_task() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago");
    let project = load_project(&root).unwrap();
    let ir = analyze(&project);
    let sources = ai::read_sources(&root);
    let task = project
        .ai_tasks
        .iter()
        .find(|t| t.id == "draft_entries")
        .unwrap();
    let (url, received) = serve(
        r#"{"entries": [{"type": "character", "id": "tenin", "name": "店員", "fields": {"first_person": "わたくし"}}]}"#,
    );
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
    let draft = outcome.entries.unwrap();
    assert_eq!(draft.entries[0].id, "tenin");
    let sent: Value = serde_json::from_str(&received.lock().unwrap()).unwrap();
    assert_eq!(sent["response_format"]["json_schema"]["name"], "entries");
}

/// 2人以上に当てはまる話者は未登録ではない（作っても解決しない）。型の定義の無い型への参照は、作れないので除く。
#[test]
fn ambiguous_speakers_and_undefined_types() {
    let dir = temp_project(
        "ambiguous",
        &[
            ("project.toml", "format_version = 0\n"),
            ("story/a.scn", "## 屋上 #s1\n先生「やあ」\n"),
            (
                "bible/entries/character/a.md",
                "---\nname: 先生\nclub: none_club\n---\n",
            ),
            ("bible/entries/character/b.md", "---\nname: 先生\n---\n"),
            (
                "bible/types/character.yaml",
                "name: キャラクター\nbuiltin: character\nfields:\n  - { key: club, label: 部, type: ref<club> }\n",
            ),
        ],
    );
    let project = load_project(&dir).unwrap();
    let ir = analyze(&project);
    assert!(ir.diagnostics.iter().any(|d| d.code == "W202"));
    let missing = ai::missing_entries(&project, &ir, None);
    assert!(missing.is_empty(), "{missing:?}");
    let _ = fs::remove_dir_all(&dir);
}

/// 応答の形：1行に1つの JSON も読む。伏線の回収はシーンIDのリストも受け付ける。今あるキャラクターと名前・呼び名が
/// 重なる新しいキャラクターは作らない。
#[test]
fn jsonl_payoff_and_overlap() {
    let dir = project("overlap");
    fs::write(
        dir.join("bible/types/foreshadow.yaml"),
        "name: 伏線\nfields:\n  - { key: payoff, label: 回収, type: ref<scene> }\n",
    )
    .unwrap();
    let project = load_project(&dir).unwrap();
    let (draft, notes) = ai::parse_entries_draft(
        "{\"type\": \"foreshadow\", \"id\": \"key\", \"name\": \"屋上の鍵\", \"fields\": {\"payoff\": [\"s1\", \"s2\"]}}\n{\"type\": \"character\", \"id\": \"akari2\", \"name\": \"灯\"}\n{\"type\": \"character\", \"id\": \"kei\", \"name\": \"圭\", \"fields\": {\"aliases\": [\"店員\"]}}\n",
        &project,
    );
    assert!(notes.is_empty(), "{notes:?}");
    let draft = draft.unwrap();
    assert_eq!(draft.entries.len(), 3);
    assert_eq!(draft.entries[0].fields["payoff"], json!(["s1", "s2"]));
    let mut ws = Workspace::open(&dir).unwrap();
    let done = ws
        .write_entries_draft(&draft, "draft_entries", "m", None)
        .unwrap();
    // 「灯」は akari と重なるので作らない。「店員」は未登録なので重ならない。
    assert_eq!(
        done.created,
        [
            "bible/entries/foreshadow/key.md",
            "bible/entries/character/kei.md"
        ]
    );
    assert!(
        done.notes
            .iter()
            .any(|n| n.contains("akari2") && n.contains("灯")),
        "{:?}",
        done.notes
    );
    let _ = fs::remove_dir_all(&dir);
}

/// エントリの案の採用（人の操作）：値を案で置き換え、出自 `ai`。外でその項目が変わっていれば書かない。
#[test]
fn adopt_entry_proposal() {
    let dir = project("adopt");
    let project = load_project(&dir).unwrap();
    let (draft, _) = ai::parse_entries_draft(DRAFT, &project);
    let mut ws = Workspace::open(&dir).unwrap();
    let done = ws
        .write_entries_draft(&draft.unwrap(), "draft_entries", "m", Some(AKARI))
        .unwrap();
    let akari = &done.files[0];
    let proposal = &akari.proposals[0];
    let edit = ws
        .adopt_field_proposal(
            AKARI,
            &proposal.key,
            &proposal.proposed,
            &akari.text,
            "draft_entries",
            "m",
        )
        .unwrap();
    assert!(
        matches!(&edit, scn_core::workspace::EntryEdit::Saved { text } if text.contains("first_person: あたし\n")),
        "{edit:?}"
    );
    assert_eq!(
        read_provenance(&dir).unwrap().latest_fields()[&(AKARI, "first_person")].origin,
        AI
    );
    // 外で一人称が変わっていた。
    let now = fs::read_to_string(dir.join(AKARI)).unwrap();
    fs::write(dir.join(AKARI), now.replace("あたし", "うち")).unwrap();
    let edit = ws
        .adopt_field_proposal(AKARI, "first_person", &json!("ぼく"), &now, "t", "m")
        .unwrap();
    assert!(
        matches!(edit, scn_core::workspace::EntryEdit::Conflict { .. }),
        "{edit:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}
