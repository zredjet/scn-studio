//! 言語サーバーの結合テスト（T3-15）：テストの中で JSON-RPC の要求をサービスに渡し、応答とサーバーからの知らせ
//! （診断）を受け取る。入力は houkago の写し。位置は UTF-16（サロゲートペアと全角文字を含む行で確かめる）。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::StreamExt;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tower::{Service, ServiceExt};
use tower_lsp_server::LspService;
use tower_lsp_server::jsonrpc::Request;
use tower_lsp_server::ls_types::Uri;

use scn_lsp::Backend;

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn houkago(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scn-lsp-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago"),
        &dir,
    );
    dir
}

fn uri(path: &Path) -> String {
    Uri::from_file_path(path).unwrap().to_string()
}

/// テストの中の言語サーバーとのやり取り。
struct Session {
    service: LspService<Backend>,
    messages: mpsc::UnboundedReceiver<Request>,
    next: i64,
}

impl Session {
    /// 作品フォルダを開いて初期化する。
    async fn start(root: &Path) -> Self {
        let (service, mut socket) = scn_lsp::service();
        let (sender, messages) = mpsc::unbounded_channel();
        // サーバーからの知らせ（診断）を受け取り続ける。
        tokio::spawn(async move {
            while let Some(message) = socket.next().await {
                if sender.send(message).is_err() {
                    break;
                }
            }
        });
        let mut session = Self {
            service,
            messages,
            next: 1,
        };
        let result = session
            .request(
                "initialize",
                json!({
                    "capabilities": {},
                    "workspaceFolders": [{ "uri": uri(root), "name": "houkago" }],
                }),
            )
            .await;
        assert_eq!(result["capabilities"]["positionEncoding"], "utf-16");
        session.notify("initialized", json!({})).await;
        session
    }

    async fn request(&mut self, method: &'static str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        let request = Request::build(method).params(params).id(id).finish();
        let response = self
            .service
            .ready()
            .await
            .unwrap()
            .call(request)
            .await
            .unwrap()
            .unwrap();
        let (_, result) = response.into_parts();
        result.unwrap_or_else(|e| json!({ "error": e.message }))
    }

    async fn notify(&mut self, method: &'static str, params: Value) {
        let request = Request::build(method).params(params).finish();
        let response = self
            .service
            .ready()
            .await
            .unwrap()
            .call(request)
            .await
            .unwrap();
        assert!(response.is_none());
    }

    /// そのファイルの、いちばん新しい診断（届くまで待つ）。
    async fn diagnostics(&mut self, file: &str) -> Vec<Value> {
        let mut found = None;
        loop {
            let next = tokio::time::timeout(Duration::from_millis(500), self.messages.recv()).await;
            let Ok(Some(message)) = next else { break };
            let (method, _, params) = message.into_parts();
            let params = params.unwrap_or_default();
            if method == "textDocument/publishDiagnostics" && params["uri"] == file {
                found = Some(
                    params["diagnostics"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default(),
                );
            }
        }
        found.unwrap_or_else(|| panic!("{file} の診断が届かない"))
    }
}

fn position(line: u32, character: u32) -> Value {
    json!({ "line": line, "character": character })
}

/// 本文の中の `needle` の行と、行の中の UTF-16 の位置（`skip` 文字目）。
fn find(text: &str, needle: &str, skip: usize) -> (u32, u32) {
    let start = text
        .find(needle)
        .unwrap_or_else(|| panic!("{needle} が無い"));
    let before = &text[..start];
    let line = before.matches('\n').count() as u32;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let column: usize = before[line_start..]
        .chars()
        .map(char::len_utf16)
        .sum::<usize>()
        + needle
            .chars()
            .take(skip)
            .map(char::len_utf16)
            .sum::<usize>();
    (line, column as u32)
}

#[tokio::test(flavor = "current_thread")]
async fn diagnostics_follow_edits_with_utf16_positions() {
    let root = houkago("diag");
    let chapter = root.join("story/chapter01.scn");
    let file = uri(&chapter);
    let mut session = Session::start(&root).await;
    let first = session.diagnostics(&file).await;
    assert!(first.iter().any(|d| d["code"] == "W503"), "{first:?}");
    // サロゲートペア（😀）と全角の文字のある行に、無いシーンへの飛び先を書く。
    let mut text = fs::read_to_string(&chapter).unwrap();
    text.push_str("\n## 足した場面 #s900\n@選択\n- 😀ｘ行く -> s999\n@end\n");
    session
        .notify(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": file, "languageId": "scn", "version": 1, "text": text } }),
        )
        .await;
    let diagnostics = session.diagnostics(&file).await;
    let e501 = diagnostics
        .iter()
        .find(|d| d["code"] == "E501")
        .unwrap_or_else(|| panic!("{diagnostics:?}"));
    let (line, column) = find(&text, "s999", 0);
    assert_eq!(e501["range"]["start"], position(line, column));
    assert_eq!(e501["range"]["end"], position(line, column + 4));
    assert_eq!(e501["severity"], 1);
    // 直したら消える。
    let fixed = text.replace("-> s999", "-> s012");
    session
        .notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": file, "version": 2 },
                "contentChanges": [{ "text": fixed }],
            }),
        )
        .await;
    let diagnostics = session.diagnostics(&file).await;
    assert!(
        diagnostics.iter().all(|d| d["code"] != "E501"),
        "{diagnostics:?}"
    );
    // 閉じると、ディスクの中身に戻る。
    session
        .notify(
            "textDocument/didClose",
            json!({ "textDocument": { "uri": file } }),
        )
        .await;
    let diagnostics = session.diagnostics(&file).await;
    assert_eq!(diagnostics.len(), first.len());
    let _ = fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "current_thread")]
async fn language_features() {
    let root = houkago("features");
    let chapter = root.join("story/chapter01.scn");
    let file = uri(&chapter);
    let mut text = fs::read_to_string(&chapter).unwrap();
    text.push_str("\n## 足した場面 #s900\n@選択\n- 😀ｘ行く -> s012\n@end\n@背");
    let mut session = Session::start(&root).await;
    session
        .notify(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": file, "languageId": "scn", "version": 1, "text": text } }),
        )
        .await;
    let doc = json!({ "uri": file });

    // 補完：行末の `@背` → `@背景`。置き換える範囲は `@背`。
    let last_line = text.matches('\n').count() as u32;
    let items = session
        .request(
            "textDocument/completion",
            json!({ "textDocument": doc, "position": position(last_line, 2) }),
        )
        .await;
    let item = items["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["label"] == "@背景")
        .unwrap_or_else(|| panic!("{items}"));
    assert_eq!(item["textEdit"]["range"]["start"], position(last_line, 0));
    assert_eq!(item["textEdit"]["newText"], "@背景 ");

    // 定義へ移動：サロゲートペアの後ろの `s012` から、見出しへ。
    let (line, column) = find(&text, "s012\n@end", 1);
    let location = session
        .request(
            "textDocument/definition",
            json!({ "textDocument": doc, "position": position(line, column) }),
        )
        .await;
    assert_eq!(location["uri"], file);
    let (heading, _) = find(&text, "## 放課後の屋上 #s012", 0);
    assert_eq!(location["range"]["start"], position(heading, 0));

    // 参照：話者の灯（定義を含めると、設定DBのエントリも）。
    let (line, column) = find(&text, "灯「やっと来た", 0);
    let locations = session
        .request(
            "textDocument/references",
            json!({
                "textDocument": doc,
                "position": position(line, column),
                "context": { "includeDeclaration": true },
            }),
        )
        .await;
    let locations = locations.as_array().unwrap();
    assert_eq!(
        locations[0]["uri"],
        uri(&root.join("bible/entries/character/akari.md"))
    );
    assert!(locations.len() > 5, "{locations:?}");

    // ホバー：変数。
    let (line, column) = find(&text, "勇気 += 1", 0);
    let hover = session
        .request(
            "textDocument/hover",
            json!({ "textDocument": doc, "position": position(line, column) }),
        )
        .await;
    let value = hover["contents"]["value"].as_str().unwrap();
    assert!(
        value.contains("flag.勇気") && value.contains("型：int"),
        "{value}"
    );

    // 名前の変更：シーンID。全角とサロゲートペアのある行の位置も正しく置き換える。
    let (line, column) = find(&text, "s012\n@end", 0);
    let prepared = session
        .request(
            "textDocument/prepareRename",
            json!({ "textDocument": doc, "position": position(line, column) }),
        )
        .await;
    assert_eq!(prepared["placeholder"], "s012");
    assert_eq!(prepared["range"]["start"], position(line, column));
    assert_eq!(prepared["range"]["end"], position(line, column + 4));
    let edit = session
        .request(
            "textDocument/rename",
            json!({ "textDocument": doc, "position": position(line, column), "newName": "s012x" }),
        )
        .await;
    let edits = edit["changes"][&file].as_array().unwrap();
    assert!(edits.iter().all(|e| e["newText"] == "s012x"), "{edits:?}");
    assert!(
        edits
            .iter()
            .any(|e| e["range"]["start"] == position(line, column)),
        "{edits:?}"
    );
    // 使われているIDには変えない（誤りを返す）。
    let refused = session
        .request(
            "textDocument/rename",
            json!({ "textDocument": doc, "position": position(line, column), "newName": "s010" }),
        )
        .await;
    assert!(
        refused["error"]
            .as_str()
            .is_some_and(|e| e.contains("s010")),
        "{refused}"
    );
    let _ = fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "current_thread")]
async fn settings_changes_are_picked_up() {
    let root = houkago("watch");
    let chapter = root.join("story/chapter01.scn");
    let file = uri(&chapter);
    let mut session = Session::start(&root).await;
    let before = session.diagnostics(&file).await;
    // 設定DBから灯を消すと、灯の台詞が未登録の話者（W202）になる。
    fs::remove_file(root.join("bible/entries/character/akari.md")).unwrap();
    session
        .notify(
            "workspace/didChangeWatchedFiles",
            json!({ "changes": [{ "uri": uri(&root.join("bible/entries/character/akari.md")), "type": 3 }] }),
        )
        .await;
    let after = session.diagnostics(&file).await;
    assert!(!before.iter().any(|d| d["code"] == "W202"));
    assert!(after.iter().any(|d| d["code"] == "W202"), "{after:?}");
    let _ = fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "current_thread")]
async fn save_then_close_keeps_the_saved_text() {
    let root = houkago("save");
    let chapter = root.join("story/chapter01.scn");
    let file = uri(&chapter);
    let mut session = Session::start(&root).await;
    session.diagnostics(&file).await;
    let mut text = fs::read_to_string(&chapter).unwrap();
    session
        .notify(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": file, "languageId": "scn", "version": 1, "text": text } }),
        )
        .await;
    session.diagnostics(&file).await;
    text.push_str("\n## 足した場面 #s900\n-> s999\n");
    session
        .notify(
            "textDocument/didChange",
            json!({ "textDocument": { "uri": file, "version": 2 }, "contentChanges": [{ "text": text }] }),
        )
        .await;
    session.diagnostics(&file).await;
    // エディタが保存して（ディスクに書いて）から閉じる。
    fs::write(&chapter, &text).unwrap();
    session
        .notify(
            "textDocument/didSave",
            json!({ "textDocument": { "uri": file } }),
        )
        .await;
    session.diagnostics(&file).await;
    session
        .notify(
            "textDocument/didClose",
            json!({ "textDocument": { "uri": file } }),
        )
        .await;
    let diagnostics = session.diagnostics(&file).await;
    assert!(
        diagnostics.iter().any(|d| d["code"] == "E501"),
        "保存した中身のまま：{diagnostics:?}"
    );
    let _ = fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "current_thread")]
async fn bom_is_not_a_character() {
    let root = houkago("bom");
    let chapter = root.join("story/chapter09.scn");
    // 開いていない、BOM の付いた章（Windows のエディタで作った）。
    fs::write(&chapter, "\u{feff}## 前日 #s901\n　地の文。\n-> s999\n").unwrap();
    let file = uri(&chapter);
    let mut session = Session::start(&root).await;
    let diagnostics = session.diagnostics(&file).await;
    let first_line: Vec<&Value> = diagnostics
        .iter()
        .filter(|d| d["range"]["start"]["line"] == 0)
        .collect();
    assert!(!first_line.is_empty(), "{diagnostics:?}");
    assert!(
        first_line
            .iter()
            .all(|d| d["range"]["start"]["character"] == 0),
        "{first_line:?}"
    );
    let e501 = diagnostics.iter().find(|d| d["code"] == "E501").unwrap();
    assert_eq!(e501["range"]["start"], position(2, 3));
    let _ = fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "current_thread")]
async fn completion_filter_and_snippets() {
    let root = houkago("snippet");
    let chapter = root.join("story/chapter01.scn");
    let file = uri(&chapter);
    // スニペットを受け付けるクライアント。
    let (service, mut socket) = scn_lsp::service();
    let (sender, messages) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(message) = socket.next().await {
            if sender.send(message).is_err() {
                break;
            }
        }
    });
    let mut session = Session {
        service,
        messages,
        next: 1,
    };
    session
        .request(
            "initialize",
            json!({
                "capabilities": { "textDocument": { "completion": { "completionItem": { "snippetSupport": true } } } },
                "workspaceFolders": [{ "uri": uri(&root), "name": "houkago" }],
            }),
        )
        .await;
    session.notify("initialized", json!({})).await;
    let mut text = fs::read_to_string(&chapter).unwrap();
    text.push_str("\nあか");
    session
        .notify(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": file, "languageId": "scn", "version": 1, "text": text } }),
        )
        .await;
    let line = text.matches('\n').count() as u32;
    let doc = json!({ "uri": file });
    let list = session
        .request(
            "textDocument/completion",
            json!({ "textDocument": doc, "position": position(line, 2) }),
        )
        .await;
    // 打った文字で絞り込まれても消えないよう、打った文字を絞り込みの文字にする。打つたびに問い合わせ直す。
    assert_eq!(list["isIncomplete"], true);
    let item = &list["items"][0];
    assert_eq!(item["label"], "灯");
    assert_eq!(item["filterText"], "あか");
    assert_eq!(item["textEdit"]["newText"], "灯「$0」");
    assert_eq!(item["insertTextFormat"], 2);
    // エントリのID（ファイルの名前が変わる）は、名前を入れる前に断る。
    text.push_str("\nakari「やあ」");
    session
        .notify(
            "textDocument/didChange",
            json!({ "textDocument": { "uri": file, "version": 2 }, "contentChanges": [{ "text": text }] }),
        )
        .await;
    let last = text.matches('\n').count() as u32;
    let refused = session
        .request(
            "textDocument/prepareRename",
            json!({ "textDocument": doc, "position": position(last, 1) }),
        )
        .await;
    assert!(
        refused["error"]
            .as_str()
            .is_some_and(|e| e.contains("アプリ")),
        "{refused}"
    );
    let _ = fs::remove_dir_all(&root);
}

/// ファイルの監視を登録できないクライアントでも、設定ファイルを保存すれば読み直す。
#[tokio::test(flavor = "current_thread")]
async fn saved_settings_are_reloaded() {
    let root = houkago("settings-save");
    let chapter = uri(&root.join("story/chapter01.scn"));
    let entry = root.join("bible/entries/character/akari.md");
    let mut session = Session::start(&root).await;
    let before = session.diagnostics(&chapter).await;
    assert!(!before.iter().any(|d| d["code"] == "W202"));
    // 灯の名前を変えて保存すると、本文の「灯」が未登録の話者（W202）になる。
    let text = fs::read_to_string(&entry)
        .unwrap()
        .replace("name: 灯", "name: 明かり");
    fs::write(&entry, &text).unwrap();
    session
        .notify(
            "textDocument/didSave",
            json!({ "textDocument": { "uri": uri(&entry) } }),
        )
        .await;
    let after = session.diagnostics(&chapter).await;
    assert!(after.iter().any(|d| d["code"] == "W202"), "{after:?}");
    let _ = fs::remove_dir_all(&root);
}
