//! `.scn` の言語サーバー（T3-15、D076）：コアの言語サービス（診断、補完、定義へ移動、参照、名前の変更、
//! ホバー）を LSP で公開する。`scn lsp` が標準入出力で動かす。処理はすべて `scn-core` にあり、ここは LSP の
//! 形との受け渡しだけ（位置の変換は `position` の1か所）。
//!
//! - 作品フォルダは、初めに渡されたフォルダ（`project.toml` があれば）か、開いた文書から上へ探して見つけた
//!   `project.toml` のあるフォルダ。開いた文書の中身は保存していない編集として持ち（アプリと同じ `Workspace`）、
//!   変わるたびに作品全体を解析して、診断（読み込み・構文・意味と、検証の段階1）を出す。
//! - 文書の同期は丸ごと（`TextDocumentSyncKind::FULL`）。位置は UTF-16（LSP の既定）。

pub mod position;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use scn_core::analyze;
use scn_core::check::check;
use scn_core::complete::{CompletionKind, complete_at};
use scn_core::index::build_index;
use scn_core::ir::Ir;
use scn_core::layout::{LAYOUT_FILE, renamed_layout_text};
use scn_core::navigate::{Place, definition, hover, references};
use scn_core::rename::{RenameTarget, plan_rename, symbol_at};
use scn_core::workspace::Workspace;
use tower_lsp_server::jsonrpc::{Error, Result};
use tower_lsp_server::ls_types::{
    CompletionItem, CompletionItemKind, CompletionList, CompletionOptions, CompletionParams,
    CompletionResponse, CompletionTextEdit, Diagnostic, DiagnosticSeverity,
    DidChangeTextDocumentParams, DidChangeWatchedFilesParams,
    DidChangeWatchedFilesRegistrationOptions, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, FileSystemWatcher, GlobPattern,
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams,
    HoverProviderCapability, InitializeParams, InitializeResult, InitializedParams,
    InsertTextFormat, Location, MarkupContent, MarkupKind, MessageType, NumberOrString, OneOf,
    Position, PositionEncodingKind, PrepareRenameResponse, Range, ReferenceParams, Registration,
    RenameOptions, RenameParams, ServerCapabilities, ServerInfo, TextDocumentPositionParams,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Uri, WorkspaceEdit,
};
use tower_lsp_server::{Client, LanguageServer, LspService, Server};

/// 標準入出力で言語サーバーを動かす（`scn lsp`）。
pub async fn serve_stdio() {
    let (service, socket) = LspService::new(Backend::new);
    Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
        .serve(service)
        .await;
}

/// 言語サーバー。
pub struct Backend {
    client: Client,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    workspace: Option<Workspace>,
    /// 今の中身の解析（変わったら捨てる）。
    ir: Option<Ir>,
    /// 診断を出したファイル（診断が無くなったら、空の診断を出す）。
    published: BTreeSet<String>,
    /// クライアントがファイルの監視を登録できるか。
    watch: bool,
    /// クライアントが補完のスニペット（`$0`）を受け付けるか。
    snippets: bool,
}

impl State {
    fn ir(&mut self) -> Option<&Ir> {
        let workspace = self.workspace.as_ref()?;
        if self.ir.is_none() {
            self.ir = Some(analyze(workspace.project()));
        }
        self.ir.as_ref()
    }

    fn changed(&mut self) {
        self.ir = None;
    }
}

/// `dir` から上へ、`project.toml` のあるフォルダを探す。
fn project_root(dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .find(|d| d.join("project.toml").is_file())
        .map(Path::to_owned)
}

/// 作品フォルダの中のファイルの URI。
fn uri_of(workspace: &Workspace, file: &str) -> Option<Uri> {
    let root = std::path::absolute(workspace.root()).ok()?;
    Uri::from_file_path(root.join(file))
}

/// ファイルの中身（本文は保存していない編集を含む今の中身、ほかはディスク）。
fn text_of(workspace: &Workspace, file: &str) -> Option<String> {
    workspace
        .text(file)
        .map(str::to_owned)
        .or_else(|| std::fs::read_to_string(workspace.root().join(file)).ok())
}

fn location(workspace: &Workspace, place: &Place) -> Option<Location> {
    locations(workspace, std::slice::from_ref(place)).pop()
}

/// 場所を LSP の場所にする（ファイルごとに中身を一度読み、行頭の表を一度作る）。
fn locations(workspace: &Workspace, places: &[Place]) -> Vec<Location> {
    let mut texts: BTreeMap<&str, String> = BTreeMap::new();
    for place in places {
        if !texts.contains_key(place.file.as_str())
            && let Some(text) = text_of(workspace, &place.file)
        {
            texts.insert(place.file.as_str(), text);
        }
    }
    let lines: BTreeMap<&str, position::Lines<'_>> = texts
        .iter()
        .map(|(file, text)| (*file, position::Lines::new(text)))
        .collect();
    places
        .iter()
        .filter_map(|place| {
            let lines = lines.get(place.file.as_str())?;
            Some(Location::new(
                uri_of(workspace, &place.file)?,
                lines.range(place.span),
            ))
        })
        .collect()
}

fn severity(severity: &str) -> DiagnosticSeverity {
    match severity {
        "error" => DiagnosticSeverity::ERROR,
        "warning" => DiagnosticSeverity::WARNING,
        _ => DiagnosticSeverity::INFORMATION,
    }
}

fn not_found(message: impl Into<String>) -> Error {
    Error::invalid_params(message.into())
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            state: Mutex::new(State::default()),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// 作品フォルダを開く（まだ開いていなければ）。開けなければ知らせる文。
    fn open(state: &mut State, dir: &Path) -> Option<String> {
        if state.workspace.is_some() {
            return None;
        }
        let root = project_root(dir)?;
        match Workspace::open(&root) {
            Ok(workspace) => {
                state.workspace = Some(workspace);
                state.changed();
                None
            }
            Err(e) => Some(format!("作品フォルダを開けません：{e}")),
        }
    }

    /// 文書の、作品フォルダから数えたパス。作品フォルダが開いていなければ、文書から上へ探して開く。
    fn file_of(&self, state: &mut State, uri: &Uri) -> Option<String> {
        let path = uri.to_file_path()?.into_owned();
        if state.workspace.is_none()
            && let Some(dir) = path.parent()
            && let Some(message) = Self::open(state, dir)
        {
            let client = self.client.clone();
            tokio::spawn(async move {
                client
                    .log_message(tower_lsp_server::ls_types::MessageType::ERROR, message)
                    .await;
            });
        }
        state.workspace.as_ref()?.relative(&path)
    }

    /// 作品全体の診断を、ファイルごとに出す（無くなったファイルには空の診断）。
    async fn publish(&self) {
        let batches = {
            let mut state = self.state();
            let Some(workspace) = state.workspace.as_ref() else {
                return;
            };
            let project = workspace.project();
            let ir = analyze(project);
            let index = build_index(project, &ir);
            let mut all = ir.diagnostics.clone();
            all.extend(check(project, &ir, &index));
            let mut by_file: BTreeMap<String, Vec<Diagnostic>> = BTreeMap::new();
            // 位置は、ファイルごとに一度作った行頭の表で引く（診断の多い長い章でも重くしない）。
            let mut texts: BTreeMap<String, String> = BTreeMap::new();
            for d in &all {
                if !texts.contains_key(&d.file) {
                    texts.insert(
                        d.file.clone(),
                        text_of(workspace, &d.file).unwrap_or_default(),
                    );
                }
            }
            let lines: BTreeMap<&str, position::Lines<'_>> = texts
                .iter()
                .map(|(file, text)| (file.as_str(), position::Lines::new(text)))
                .collect();
            for d in &all {
                let Some(lines) = lines.get(d.file.as_str()) else {
                    continue;
                };
                by_file.entry(d.file.clone()).or_default().push(Diagnostic {
                    range: lines.range(d.span),
                    severity: Some(severity(d.severity)),
                    code: Some(NumberOrString::String(d.code.to_owned())),
                    source: Some("scn".to_owned()),
                    message: d.message.clone(),
                    ..Diagnostic::default()
                });
            }
            let mut batches = Vec::new();
            let files: BTreeSet<String> = by_file.keys().cloned().collect();
            for file in state.published.difference(&files) {
                if let Some(uri) = uri_of(workspace, file) {
                    batches.push((uri, Vec::new()));
                }
            }
            for (file, diagnostics) in by_file {
                if let Some(uri) = uri_of(workspace, &file) {
                    batches.push((uri, diagnostics));
                }
            }
            state.ir = Some(ir);
            state.published = files;
            batches
        };
        for (uri, diagnostics) in batches {
            self.client
                .publish_diagnostics(uri, diagnostics, None)
                .await;
        }
    }

    /// 文書の位置を、作品フォルダのファイルとバイト位置にして `f` に渡す。
    fn at<T>(
        &self,
        params: &TextDocumentPositionParams,
        f: impl FnOnce(&Workspace, &Ir, &str, &str, usize) -> T,
    ) -> Option<T> {
        let mut state = self.state();
        let file = self.file_of(&mut state, &params.text_document.uri)?;
        state.ir()?;
        let workspace = state.workspace.as_ref()?;
        let ir = state.ir.as_ref()?;
        let text = workspace.text(&file)?;
        let offset = position::offset(text, params.position);
        Some(f(workspace, ir, &file, text, offset))
    }
}

/// 名前の変更の中身を、LSP の編集にする（本文は置き換えごと、設定ファイルと `layout.json` は丸ごと）。
fn workspace_edit(
    workspace: &Workspace,
    target: &RenameTarget,
    new: &str,
) -> std::result::Result<(WorkspaceEdit, Vec<String>), String> {
    let plan = plan_rename(workspace.project(), workspace.root(), target, new)?;
    if !plan.moves.is_empty() {
        return Err(MOVE_REFUSED.to_owned());
    }
    let mut changes: std::collections::HashMap<Uri, Vec<TextEdit>> =
        std::collections::HashMap::new();
    let whole = |text: &str| Range::new(Position::new(0, 0), position::position(text, text.len()));
    for story in &plan.story {
        let (Some(uri), Some(text)) = (uri_of(workspace, &story.file), workspace.text(&story.file))
        else {
            continue;
        };
        let lines = position::Lines::new(text);
        let edits = story
            .edits
            .iter()
            .map(|e| TextEdit::new(lines.range(e.span), e.text.clone()))
            .collect();
        changes.insert(uri, edits);
    }
    let mut files: Vec<(String, String)> = plan
        .settings
        .iter()
        .map(|c| (c.file.clone(), c.text.clone()))
        .collect();
    if let Some(layout) = &plan.layout
        && let Some(text) =
            renamed_layout_text(workspace.root(), &layout.level, &layout.from, &layout.to)
    {
        files.push((LAYOUT_FILE.to_owned(), text));
    }
    for (file, next) in files {
        let (Some(uri), Some(current)) = (uri_of(workspace, &file), text_of(workspace, &file))
        else {
            continue;
        };
        changes.insert(uri, vec![TextEdit::new(whole(&current), next)]);
    }
    Ok((WorkspaceEdit::new(changes), plan.notes))
}

/// 言語サーバーで断る名前の変更（エントリのファイルの名前が変わる）。
const MOVE_REFUSED: &str = "エントリのIDの変更（ファイルの名前が変わる）は、アプリの名前の変更（F2）か設定DBの画面で行います";

fn completion_kind(kind: CompletionKind) -> CompletionItemKind {
    match kind {
        CompletionKind::Scene => CompletionItemKind::REFERENCE,
        CompletionKind::Character => CompletionItemKind::CLASS,
        CompletionKind::Variable => CompletionItemKind::VARIABLE,
        CompletionKind::Asset => CompletionItemKind::FILE,
        CompletionKind::Directive => CompletionItemKind::KEYWORD,
        CompletionKind::Keyword => CompletionItemKind::ENUM_MEMBER,
    }
}

/// スニペットの文字（`$`・`}`・`\` を逃がす）。
fn snippet_text(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if matches!(c, '$' | '}' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let dir = params
            .workspace_folders
            .as_ref()
            .and_then(|folders| folders.first())
            .and_then(|f| f.uri.to_file_path().map(|p| p.into_owned()));
        {
            let mut state = self.state();
            state.watch = params
                .capabilities
                .workspace
                .as_ref()
                .and_then(|w| w.did_change_watched_files.as_ref())
                .and_then(|d| d.dynamic_registration)
                .unwrap_or(false);
            state.snippets = params
                .capabilities
                .text_document
                .as_ref()
                .and_then(|t| t.completion.as_ref())
                .and_then(|c| c.completion_item.as_ref())
                .and_then(|i| i.snippet_support)
                .unwrap_or(false);
            if let Some(dir) = dir {
                let _ = Self::open(&mut state, &dir);
            }
        }
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                position_encoding: Some(PositionEncodingKind::UTF16),
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(
                        ["@", "＠", ">", "#", " ", "　", "{"]
                            .iter()
                            .map(|s| (*s).to_owned())
                            .collect(),
                    ),
                    ..CompletionOptions::default()
                }),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: Default::default(),
                })),
                ..ServerCapabilities::default()
            },
            server_info: Some(ServerInfo {
                name: "scn".to_owned(),
                version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            }),
            offset_encoding: None,
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        let watch = self.state().watch;
        if watch {
            // 設定ファイルと本文の外の変更（git pull など）を受け取る。
            let options = DidChangeWatchedFilesRegistrationOptions {
                watchers: vec![FileSystemWatcher {
                    // フォルダの知らせも受けるため、すべてのファイルとフォルダ。
                    glob_pattern: GlobPattern::String("**/*".to_owned()),
                    kind: None,
                }],
            };
            let registration = Registration {
                id: "scn-watch".to_owned(),
                method: "workspace/didChangeWatchedFiles".to_owned(),
                register_options: serde_json::to_value(options).ok(),
            };
            let _ = self.client.register_capability(vec![registration]).await;
        }
        self.publish().await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        {
            let mut state = self.state();
            let Some(file) = self.file_of(&mut state, &params.text_document.uri) else {
                return;
            };
            let Some(workspace) = state.workspace.as_mut() else {
                return;
            };
            if !workspace.set_buffer(&file, &params.text_document.text) {
                // まだ知らない本文のファイル（外で足された）。
                if let Some(path) = params.text_document.uri.to_file_path() {
                    let _ = workspace.external_change(&path);
                }
                workspace.set_buffer(&file, &params.text_document.text);
            }
            state.changed();
        }
        self.publish().await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let Some(change) = params.content_changes.into_iter().last() else {
            return;
        };
        {
            let mut state = self.state();
            let Some(file) = self.file_of(&mut state, &params.text_document.uri) else {
                return;
            };
            if let Some(workspace) = state.workspace.as_mut() {
                workspace.set_buffer(&file, &change.text);
            }
            state.changed();
        }
        self.publish().await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        {
            let mut state = self.state();
            let file = self.file_of(&mut state, &params.text_document.uri);
            // エディタが書いたディスクの中身を取り込む：設定ファイルなら読み直し、知らない章なら足す（ファイルの
            // 監視を登録できないクライアントのため）。本文なら、保存した中身を知っている中身にそろえる。
            if let (Some(file), Some(path), Some(workspace)) = (
                file,
                params.text_document.uri.to_file_path(),
                state.workspace.as_mut(),
            ) {
                let _ = workspace.external_change(&path);
                if workspace.text(&file).is_some() {
                    let _ = workspace.reload_from_disk(&file);
                }
            }
            state.changed();
        }
        self.publish().await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        {
            let mut state = self.state();
            let Some(file) = self.file_of(&mut state, &params.text_document.uri) else {
                return;
            };
            // 閉じたら、保存していない編集を捨てて、今のディスクの中身に戻す（開いている間に外で変わった中身も）。
            if let Some(workspace) = state.workspace.as_mut() {
                let _ = workspace.reload_from_disk(&file);
            }
            state.changed();
        }
        self.publish().await;
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        {
            let mut state = self.state();
            let Some(workspace) = state.workspace.as_mut() else {
                return;
            };
            // フォルダの知らせ（`story/` の下のフォルダを動かした・消した）も受ける。
            for change in &params.changes {
                if let Some(path) = change.uri.to_file_path() {
                    let _ = workspace.external_changes(&path);
                }
            }
            state.changed();
        }
        self.publish().await;
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let snippets = self.state().snippets;
        let items = self.at(
            &params.text_document_position,
            |workspace, ir, _, text, offset| {
                let lines = position::Lines::new(text);
                complete_at(workspace.project(), ir, text, offset)
                    .into_iter()
                    .map(|c| {
                        let range = lines.range(c.span);
                        // 候補はコアが打った文字で絞ってある。エディタの絞り込みで消されないよう、打った文字を
                        // 絞り込みの文字にする（`＠bg` から `@背景`、呼び名から話者など）。
                        let typed = text.get(c.span.start..c.span.end).unwrap_or_default();
                        let (new_text, format) = match c.cursor.filter(|_| snippets) {
                            Some(cursor) => {
                                let (head, tail) = (
                                    c.insert.get(..cursor).unwrap_or_default(),
                                    c.insert.get(cursor..).unwrap_or_default(),
                                );
                                (
                                    format!("{}$0{}", snippet_text(head), snippet_text(tail)),
                                    InsertTextFormat::SNIPPET,
                                )
                            }
                            None => (c.insert.clone(), InsertTextFormat::PLAIN_TEXT),
                        };
                        CompletionItem {
                            label: c.label,
                            detail: Some(c.detail),
                            kind: Some(completion_kind(c.kind)),
                            text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(
                                range, new_text,
                            ))),
                            insert_text_format: Some(format),
                            filter_text: Some(typed.to_owned()),
                            ..CompletionItem::default()
                        }
                    })
                    .collect::<Vec<_>>()
            },
        );
        // 打つたびに問い合わせ直してもらう（文脈で候補が変わるため）。
        Ok(items.filter(|items| !items.is_empty()).map(|items| {
            CompletionResponse::List(CompletionList {
                is_incomplete: true,
                items,
            })
        }))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        Ok(self
            .at(
                &params.text_document_position_params,
                |workspace, ir, file, _, offset| {
                    definition(workspace.project(), ir, file, offset)
                        .and_then(|place| location(workspace, &place))
                },
            )
            .flatten()
            .map(GotoDefinitionResponse::Scalar))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let include = params.context.include_declaration;
        Ok(self
            .at(
                &params.text_document_position,
                |workspace, ir, file, _, offset| {
                    locations(
                        workspace,
                        &references(workspace.project(), ir, file, offset, include),
                    )
                },
            )
            .filter(|locations| !locations.is_empty()))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        Ok(self
            .at(
                &params.text_document_position_params,
                |workspace, ir, file, text, offset| {
                    hover(workspace.project(), ir, file, offset).map(|(markdown, span)| Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: markdown,
                        }),
                        range: Some(position::range(text, span)),
                    })
                },
            )
            .flatten())
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let result = self
            .at(&params, |workspace, _, file, text, offset| {
                symbol_at(workspace.project(), file, offset).map(|symbol| {
                    if matches!(symbol.target, RenameTarget::EntryId { .. }) {
                        return Err(MOVE_REFUSED.to_owned());
                    }
                    Ok(PrepareRenameResponse::RangeWithPlaceholder {
                        range: position::range(text, symbol.span),
                        placeholder: symbol.current,
                    })
                })
            })
            .flatten();
        match result {
            None => Ok(None),
            Some(Ok(response)) => Ok(Some(response)),
            Some(Err(message)) => Err(not_found(message)),
        }
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let new = params.new_name.clone();
        let result = self.at(
            &params.text_document_position,
            |workspace, _, file, _, offset| {
                let symbol = symbol_at(workspace.project(), file, offset)
                    .ok_or_else(|| "ここには名前を変えられるものがありません".to_owned())?;
                workspace_edit(workspace, &symbol.target, &new)
            },
        );
        match result {
            None => Ok(None),
            Some(Ok((edit, notes))) => {
                // 変えないもの・変わってしまうものの知らせ（生コードの中の名前など）を見せる。
                for note in notes {
                    self.client.show_message(MessageType::INFO, note).await;
                }
                Ok(Some(edit))
            }
            Some(Err(message)) => Err(not_found(message)),
        }
    }
}

/// テストのために、サービスを作る（`LspService::new(Backend::new)` と同じ）。
pub fn service() -> (LspService<Backend>, tower_lsp_server::ClientSocket) {
    LspService::new(Backend::new)
}
