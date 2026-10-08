//! scn-studio のデスクトップアプリ（Tauri）。
//!
//! コマンドは薄く保ち、処理は scn-core に置く（`.claude/rules/rust.md`）。開いている作品フォルダは
//! コアの `Workspace` が持ち、ファイルを読み書きするのはコアだけにする。フロントには任意のパスを読ませない。
//!
//! 入力が止まるたびに渡すのは、表示に要る分（診断、シーンの一覧、辺）だけにする（D042）。命令列（IR全体）は
//! 渡さない（プレビューの画面を開いているときだけ `preview_data` で渡す）。検証の段階1は、開いたとき・保存したとき・外部の変更を取り込んだときに行う。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use scn_core::assets;
use scn_core::assist::{self, completion_data, scene_chars};
use scn_core::bible;
use scn_core::check::{check, overflow};
use scn_core::concept::concept_data;
use scn_core::graph::{Level, build_graph};
use scn_core::ideas::{self, Promotion};
use scn_core::index::build_index;
use scn_core::ir::{Edge, IrDiagnostic, IrMeta};
use scn_core::layout::{Point, read_layout as read_layout_file};
use scn_core::line_id::LineIdGenerator;
use scn_core::preview;
use scn_core::renpy;
use scn_core::route;
use scn_core::scaffold::create_project as scaffold_project;
use scn_core::script;
use scn_core::workspace::{AssetKey, Change, SaveOutcome, Workspace};
use scn_core::{Project, Span, analyze};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::ipc::Response;
use tauri::{AppHandle, Emitter, Manager, State};

/// 解析を走らせるスレッドのスタックの大きさ。ブロックの入れ子が深い本文でもあふれないよう、
/// メインスレッド（Windowsでは1MB）より大きくする。
const ANALYZE_STACK_SIZE: usize = 64 * 1024 * 1024;

/// ファイルの監視の知らせを、まとめて取り込むまで待つ時間。
const WATCH_DEBOUNCE: Duration = Duration::from_millis(150);

/// 最近開いた作品フォルダを覚える数。
const RECENT_LIMIT: usize = 10;

/// シーンの一覧の1つ（IRのシーンから命令列を除いたもの）。
#[derive(Serialize)]
struct SceneSummary<'a> {
    id: &'a str,
    title: &'a str,
    file: &'a str,
    span: Span,
    meta: &'a IrMeta,
    is_stub: bool,
    /// 表示する文字の数（D047）。
    chars: usize,
}

/// 表示に要る分（D042）。フロントの型は `app/src/core/types.ts` の `Summary`。
#[derive(Serialize)]
struct Summary<'a> {
    start_scene: Option<&'a str>,
    scenes: Vec<SceneSummary<'a>>,
    edges: &'a [Edge],
    /// 解析の診断と、はみ出し（W203）。はみ出しは書いている最中に波線を引くため、入力が止まるたびに出す（D047）。
    diagnostics: Vec<&'a IrDiagnostic>,
}

/// 解析の結果（JSONにしたもの）。
struct Analyzed {
    summary: Value,
    /// 検証の段階1の診断（はみ出しは `summary` に入れるので除く）。
    checks: Option<Value>,
    /// 頼まれたシーンの情報（文脈パネル）。
    scene: Option<Value>,
}

fn to_json<T: Serialize>(value: &T, what: &str) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| format!("{what}をJSONにできません：{e}"))
}

/// 大きなスタックのスレッドで解析し、表示に要る分と、`with_checks` なら検証の段階1の診断と、
/// `scene` があればそのシーンの情報を返す。
///
/// 入れ子の深いIRは、作るのも、索引を作るのも、捨てるのも深く再帰する。IRはこのスレッドの中で捨て、
/// 外へは平らな値だけを返す。
fn analyze_summary(
    project: &Project,
    with_checks: bool,
    scene: Option<&str>,
) -> Result<Analyzed, String> {
    thread::scope(|scope| {
        thread::Builder::new()
            .name("analyze".into())
            .stack_size(ANALYZE_STACK_SIZE)
            .spawn_scoped(scope, || {
                let ir = analyze(project);
                let overflow = overflow(project, &ir);
                let summary = to_json(
                    &Summary {
                        start_scene: ir.start_scene.as_deref(),
                        scenes: ir
                            .scenes
                            .iter()
                            .map(|s| SceneSummary {
                                id: &s.id,
                                title: &s.title,
                                file: &s.file,
                                span: s.span,
                                meta: &s.meta,
                                is_stub: s.is_stub,
                                chars: scene_chars(&s.ops),
                            })
                            .collect(),
                        edges: &ir.edges,
                        diagnostics: ir.diagnostics.iter().chain(&overflow).collect(),
                    },
                    "解析の結果",
                )?;
                let index = (with_checks || scene.is_some()).then(|| build_index(project, &ir));
                let checks = match (&index, with_checks) {
                    (Some(index), true) => {
                        let checks: Vec<_> = check(project, &ir, index)
                            .into_iter()
                            .filter(|d| d.code != "W203")
                            .collect();
                        Some(to_json(&checks, "検証の結果")?)
                    }
                    _ => None,
                };
                let scene = match (&index, scene) {
                    (Some(index), Some(id)) => Some(to_json(
                        &assist::scene_info(project, index, id),
                        "シーンの情報",
                    )?),
                    _ => None,
                };
                Ok(Analyzed {
                    summary,
                    checks,
                    scene,
                })
            })
            .map_err(|e| format!("解析のスレッドを作れません：{e}"))?
            .join()
            .map_err(|_| "解析が途中で止まりました".to_owned())?
    })
}

/// 開いている作品フォルダ。
#[derive(Default)]
struct AppState {
    workspace: Mutex<Option<Workspace>>,
    /// ファイルの監視。作品フォルダを開き直すと、前の監視を止める（捨てる）。
    watcher: Mutex<Option<RecommendedWatcher>>,
    generator: Mutex<Option<LineIdGenerator>>,
    /// プレビューのウィンドウを開いたときに渡す、再生の頼み（ウィンドウが読み込みを終えてから受け取る）。
    preview_request: Mutex<Option<Value>>,
}

fn locked<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, String> {
    mutex.lock().map_err(|_| "状態を読めません".to_owned())
}

/// 台詞の括弧（`project.toml` の `[dialogue.brackets]`）。ハイライトに使う。
fn brackets(project: &Project) -> Value {
    project
        .settings
        .brackets
        .iter()
        .map(|b| json!({ "open": b.open.to_string(), "close": b.close.to_string() }))
        .collect()
}

fn title_of(workspace: &Workspace) -> String {
    workspace
        .project()
        .settings
        .title
        .clone()
        .unwrap_or_else(|| {
            workspace
                .root()
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
}

fn json_response(value: &Value) -> Result<Response, String> {
    serde_json::to_string(value)
        .map(Response::new)
        .map_err(|e| format!("JSONにできません：{e}"))
}

fn open_workspace<'a>(
    guard: &'a mut MutexGuard<'_, Option<Workspace>>,
) -> Result<&'a mut Workspace, String> {
    guard
        .as_mut()
        .ok_or_else(|| "作品フォルダが開かれていません".to_owned())
}

/// 作品フォルダを開き、本文と、表示に要る分と、検証の段階1の診断を返す。ファイルの監視を始め、
/// 最近開いたものに加える。フロントの型は `OpenedProject`。
#[tauri::command]
fn open_project(
    path: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let root = PathBuf::from(&path);
    let workspace = Workspace::open(&root).map_err(|e| e.to_string())?;
    let Analyzed {
        summary, checks, ..
    } = analyze_summary(workspace.project(), true, None)?;
    let title = title_of(&workspace);
    let opened = json!({
        "root": root.display().to_string(),
        "title": title,
        "files": workspace
            .project()
            .files
            .iter()
            .map(|f| json!({ "path": f.path, "text": f.text }))
            .collect::<Vec<_>>(),
        "brackets": brackets(workspace.project()),
        "editor": workspace.project().settings.editor,
        "assist": completion_data(workspace.project()),
        "summary": summary,
        "checks": checks,
    });
    *locked(&state.watcher)? = Some(watch(&app, &root)?);
    *locked(&state.workspace)? = Some(workspace);
    // プレビューの素材（`assets/` の中だけ）をアセットプロトコルで見せる（T1-11、D050）。前に開いた作品の
    // `assets/` は、開き直しても許したまま（このアプリを閉じるまで）。
    // 開いた後に `assets/` を作っても見せられるよう、無くても許す。
    let _ = app
        .asset_protocol_scope()
        .allow_directory(preview::assets_dir(&root), true);
    remember_recent(&app, &root, &title);
    json_response(&opened)
}

/// 新しい作品フォルダを作って（`scn new` と同じ。D044）、開く。
#[tauri::command]
fn create_project(
    path: String,
    title: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let root = PathBuf::from(&path);
    let title = title.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| {
        root.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    scaffold_project(&root, &title).map_err(|e| e.to_string())?;
    open_project(path, app, state)
}

/// 保存していない編集中の本文で、プロジェクト全体を解析し直し、表示に要る分を返す。
/// 本文は状態に残すので、別のファイルを編集しても、それまでの編集は解析に含まれる。
/// `scene` があれば、そのシーンの情報（文脈パネル）も返す。
#[tauri::command]
fn parse_buffer(
    file: String,
    text: String,
    scene: Option<String>,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    // 本文として読むのは story/*.scn だけ（仕様書17節）。知らないファイルは受け付けない。
    if !workspace.set_buffer(&file, &text) {
        return Err(format!("作品フォルダに無いファイルです：{file}"));
    }
    let started = std::time::Instant::now();
    let Analyzed { summary, scene, .. } =
        analyze_summary(workspace.project(), false, scene.as_deref())?;
    let response = json_response(&json!({ "summary": summary, "scene": scene }));
    if measuring() {
        eprintln!(
            "[scn-studio] parse_buffer のコアの側（解析とJSON）：{:.1}ミリ秒",
            started.elapsed().as_secs_f64() * 1000.0
        );
    }
    response
}

/// 本文を保存する（D045：行IDの無い行に行IDを付ける）。書いた本文と、表示に要る分と、検証の段階1の診断を返す。
/// まだ取り込んでいない外部の変更があれば、書かずに衝突を返す。`disk` は、フロントが知っているディスクの中身
/// （コアが取り込んだ外部の変更の知らせを、フロントがまだ受け取っていなければ、それとも違うので衝突になる）。
/// フロントの型は `SavedFile`。
#[tauri::command]
fn save_file(
    file: String,
    text: String,
    disk: Option<String>,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    if !workspace.set_buffer(&file, &text) {
        return Err(format!("作品フォルダに無いファイルです：{file}"));
    }
    let outcome = {
        let mut generator = locked(&state.generator)?;
        let generator = generator.get_or_insert_with(LineIdGenerator::new);
        workspace
            .save(&file, disk.as_deref(), generator)
            .map_err(|e| e.to_string())?
    };
    match outcome {
        SaveOutcome::Saved(saved) => {
            let Analyzed {
                summary, checks, ..
            } = analyze_summary(workspace.project(), true, None)?;
            json_response(&json!({
                "kind": "saved",
                "text": saved.text,
                "assigned": saved.assigned.len(),
                "summary": summary,
                "checks": checks,
            }))
        }
        SaveOutcome::Conflict { deleted } => {
            json_response(&json!({ "kind": "conflict", "deleted": deleted }))
        }
    }
}

/// 外部の変更と衝突したファイルを「読み込み直す」。外で消されていれば `text` は null。
#[tauri::command]
fn reload_file(file: String, state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let text = workspace
        .reload_from_disk(&file)
        .map_err(|e| e.to_string())?;
    let Analyzed {
        summary, checks, ..
    } = analyze_summary(workspace.project(), true, None)?;
    json_response(&json!({ "text": text, "summary": summary, "checks": checks }))
}

/// 外部の変更と衝突したファイルで「自分の変更を残す」。`text` は、エディタの今の本文。
/// ディスクの中身（外の中身。消されていれば null）と、表示に要る分と、検証の段階1の診断を返す。
#[tauri::command]
fn keep_mine(file: String, text: String, state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let disk = workspace
        .keep_mine(&file, &text)
        .map_err(|e| e.to_string())?;
    let Analyzed {
        summary, checks, ..
    } = analyze_summary(workspace.project(), true, None)?;
    json_response(&json!({ "disk": disk, "summary": summary, "checks": checks }))
}

/// シーンの情報（文脈パネル。カーソルが別のシーンへ移ったとき）。
#[tauri::command]
fn scene_info(scene: String, state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let Analyzed { scene, .. } = analyze_summary(workspace.project(), false, Some(&scene))?;
    json_response(&json!({ "scene": scene }))
}

// ---- 設定DBと変数（T1-10、D049） ----

/// 設定DBの中身（設定DBの画面）。型、読めたエントリ、エントリのファイル（読めなかったものを含む。
/// ソース表示で直せるように）、キャラクターの型、立ち絵の差分の名前。フロントの型は `BibleData`。
fn bible_data(workspace: &Workspace) -> Value {
    let project = workspace.project();
    let bible = &project.bible;
    let files: Vec<Value> = workspace
        .entry_files()
        .into_iter()
        .map(|(path, text)| {
            let entry = bible.entries.iter().find(|e| e.path == path);
            json!({ "path": path, "text": text, "entry": entry })
        })
        .collect();
    json!({
        "character_type": bible.character_type(),
        "types": bible.types,
        "files": files,
        "expressions": project.catalog.characters,
        "placeholders": assets::placeholder_expressions(project, workspace.root()),
    })
}

/// 設定DBか変数を書いた後に返すもの：書いた結果、設定DBの中身、補完の材料、表示に要る分、検証の段階1の診断。
fn after_settings_write(workspace: &Workspace, edit: Value) -> Result<Response, String> {
    let Analyzed {
        summary, checks, ..
    } = analyze_summary(workspace.project(), true, None)?;
    json_response(&json!({
        "edit": edit,
        "bible": bible_data(workspace),
        "assist": completion_data(workspace.project()),
        "summary": summary,
        "checks": checks,
    }))
}

/// 設定DBの中身（設定DBの画面を開いたとき、設定が外で変わったとき）。
#[tauri::command]
fn read_bible(state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    json_response(&bible_data(workspace))
}

/// エントリの逆引き（登場シーン、参照、紐づく変数）。無いエントリなら null。
#[tauri::command]
fn entry_usage(path: String, state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let project = open_workspace(&mut guard)?.project();
    let usage = with_index(project, |ir, index| {
        to_json(&bible::entry_usage(project, ir, index, &path), "逆引き")
    })?;
    json_response(&usage)
}

/// エントリの1つの項目を直す（直したキーの値だけを差し替える。D049）。`seen` はフロントが知っている
/// ファイルの中身。
#[tauri::command]
fn set_entry_field(
    path: String,
    key: String,
    value: Value,
    seen: String,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let edit = workspace
        .set_entry_field(&path, &key, &value, &seen)
        .map_err(|e| e.to_string())?;
    after_settings_write(workspace, to_json(&edit, "書いた結果")?)
}

/// エントリの本文（フロントマターの後ろ）を直す。
#[tauri::command]
fn set_entry_body(
    path: String,
    body: String,
    seen: String,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let edit = workspace
        .set_entry_body(&path, &body, &seen)
        .map_err(|e| e.to_string())?;
    after_settings_write(workspace, to_json(&edit, "書いた結果")?)
}

/// エントリのファイル全体を書く（ソース表示）。`seen` が null なら上書きする。
#[tauri::command]
fn write_entry_source(
    path: String,
    text: String,
    seen: Option<String>,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let edit = workspace
        .write_entry_source(&path, &text, seen.as_deref())
        .map_err(|e| e.to_string())?;
    after_settings_write(workspace, to_json(&edit, "書いた結果")?)
}

/// エントリを新しく作る（未登録の話者の登録も、ここから。D047、D049）。
#[tauri::command]
fn create_entry(
    type_id: String,
    id: String,
    name: String,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let file = workspace
        .create_entry(&type_id, &id, &name)
        .map_err(|e| e.to_string())?;
    after_settings_write(workspace, json!({ "kind": "created", "path": file }))
}

/// アイデアメモの一覧（`ideas/*.md`。昇格先を解いたもの。T2-6、D057）。
#[tauri::command]
fn read_ideas(state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let root = workspace.root().to_owned();
    let project = workspace.project();
    let list = with_ir(project, |ir| {
        to_json(&ideas::ideas(&root, project, ir), "アイデアメモ")
    })?;
    json_response(&list)
}

/// アイデアメモを新しく作る（`ideas/idea-NNN.md`）。
#[tauri::command]
fn create_idea(
    body: String,
    tags: Vec<String>,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let (file, text) = workspace
        .create_idea(&body, &tags)
        .map_err(|e| e.to_string())?;
    json_response(&json!({ "file": file, "text": text }))
}

/// アイデアメモを昇格する（エントリかスタブのシーンを作り、メモに `promoted_to` を書く）。`seen` はフロントが
/// 知っているメモの中身。設定と本文が変わるので、設定DBを書いた後と同じものを返す。
#[tauri::command]
fn promote_idea(
    path: String,
    seen: String,
    promotion: Promotion,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let promoted = workspace
        .promote_idea(&path, &seen, &promotion)
        .map_err(|e| e.to_string())?;
    after_settings_write(workspace, to_json(&promoted, "昇格の結果")?)
}

/// 企画書の画面の中身（フロントマターの項目、主要キャラ、エンディングと本文の対応）。
#[tauri::command]
fn read_concept(state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let root = workspace.root().to_owned();
    let project = workspace.project();
    let data = with_ir(project, |ir| {
        to_json(&concept_data(&root, project, ir), "企画書")
    })?;
    json_response(&data)
}

/// 企画書（`concept.md`）をひな形で作る。
#[tauri::command]
fn create_concept(state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let text = workspace.create_concept().map_err(|e| e.to_string())?;
    after_settings_write(workspace, json!({ "kind": "saved", "text": text }))
}

/// 変数の画面の中身：`variables.yaml` の中身（無ければ null）と、変数の一覧（読み書きの位置を含む）。
fn variables_data(workspace: &Workspace) -> Result<Value, String> {
    let project = workspace.project();
    let text = workspace.variables_text().map_err(|e| e.to_string())?;
    let rows = with_index(project, |_, index| {
        to_json(&bible::variable_rows(project, index), "変数の一覧")
    })?;
    Ok(json!({ "text": text, "rows": rows }))
}

#[tauri::command]
fn read_variables(state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    json_response(&variables_data(workspace)?)
}

/// `variables.yaml` を書く（宣言はテキストで直す。フォームはフェーズ2）。`seen` が null なら上書きする。
#[tauri::command]
fn write_variables(
    text: String,
    seen: Option<String>,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let edit = workspace
        .write_variables(&text, seen.as_deref())
        .map_err(|e| e.to_string())?;
    let Analyzed {
        summary, checks, ..
    } = analyze_summary(workspace.project(), true, None)?;
    json_response(&json!({
        "edit": to_json(&edit, "書いた結果")?,
        "variables": variables_data(workspace)?,
        "assist": completion_data(workspace.project()),
        "summary": summary,
        "checks": checks,
    }))
}

/// 簡易ADVプレビューのデータ（T1-11、D050）：IR全体と、画面・ウィンドウ・括弧・キャラクター・変数・素材。
/// プレビューの画面を開いているときにだけ呼ぶ（入力のたびの解析には入れない。D042）。
#[tauri::command]
fn preview_data(state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let project = workspace.project();
    let root = workspace.root().to_owned();
    let data = thread::scope(|scope| {
        thread::Builder::new()
            .name("preview".into())
            .stack_size(ANALYZE_STACK_SIZE)
            .spawn_scoped(scope, || {
                let ir = analyze(project);
                serde_json::to_string(&preview::preview_data(project, &root, &ir))
                    .map_err(|e| format!("プレビューのデータをJSONにできません：{e}"))
            })
            .map_err(|e| format!("解析のスレッドを作れません：{e}"))?
            .join()
            .map_err(|_| "解析が途中で止まりました".to_owned())?
    })?;
    Ok(Response::new(data))
}

// ---- 台本（T1-12、D051） ----

/// 大きなスタックのスレッドで解析して、台本を作る。
fn script_of(project: &Project) -> Result<script::ScriptSheet, String> {
    thread::scope(|scope| {
        thread::Builder::new()
            .name("script".into())
            .stack_size(ANALYZE_STACK_SIZE)
            .spawn_scoped(scope, || script::script(project, &analyze(project)))
            .map_err(|e| format!("解析のスレッドを作れません：{e}"))?
            .join()
            .map_err(|_| "解析が途中で止まりました".to_owned())
    })
}

/// 台本（出力の画面で、書き出す前に中身を見る）。保存していない本文も含む。
#[tauri::command]
fn script_sheet(state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let project = open_workspace(&mut guard)?.project();
    json_response(&to_json(&script_of(project)?, "台本")?)
}

/// 台本を作品フォルダの `export/script.csv` に書き出す（`scn export script` と同じ）。書いたパスと行数を返す。
#[tauri::command]
fn export_script(state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let sheet = script_of(workspace.project())?;
    let path = script::write_script(workspace.root(), &sheet).map_err(|e| e.to_string())?;
    json_response(&json!({
        "path": path.display().to_string(),
        "rows": sheet.rows.len(),
    }))
}

// ---- カーソル位置から再生と、プレビューの別ウィンドウ（T2-4、D055） ----

/// プレビューの別ウィンドウのラベル。
const PREVIEW_WINDOW: &str = "preview";

/// 大きなスタックのスレッドで解析して、`f` に渡す。
fn with_ir<T: Send>(
    project: &Project,
    f: impl FnOnce(&scn_core::Ir) -> Result<T, String> + Send,
) -> Result<T, String> {
    thread::scope(|scope| {
        thread::Builder::new()
            .name("route".into())
            .stack_size(ANALYZE_STACK_SIZE)
            .spawn_scoped(scope, || f(&analyze(project)))
            .map_err(|e| format!("解析のスレッドを作れません：{e}"))?
            .join()
            .map_err(|_| "解析が途中で止まりました".to_owned())?
    })
}

/// ファイルのある位置（バイト）から再生するための最短経路（保存していない本文も含む）。始められる命令が
/// 無ければ null。`persistent` は、プレビューの永続の変数の今の値。
#[tauri::command]
fn route_at(
    file: String,
    offset: usize,
    persistent: Option<route::Variables>,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let project = open_workspace(&mut guard)?.project();
    let persistent = persistent.unwrap_or_default();
    let route = with_ir(project, |ir| {
        to_json(
            &route::route_at(project, ir, &file, offset, &persistent),
            "最短経路",
        )
    })?;
    json_response(&route)
}

/// シーンの頭から再生するための最短経路（デバッグの欄の「最短経路で開始」）。命令の無いシーンなら null。
#[tauri::command]
fn route_to_scene(
    scene: String,
    persistent: Option<route::Variables>,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let project = open_workspace(&mut guard)?.project();
    let persistent = persistent.unwrap_or_default();
    let route = with_ir(project, |ir| {
        let found =
            route::scene_target(ir, &scene).map(|t| route::route_to(project, ir, &t, &persistent));
        to_json(&found, "最短経路")
    })?;
    json_response(&route)
}

/// プレビューを別ウィンドウで開く（開いていれば前に出す）。`request` は再生の頼み：いつも預けて
/// （ウィンドウは読み込みを終えたときに `take_preview_request` で受け取る）、開いていれば知らせる（知らせを
/// 受けたウィンドウも、預けたものを受け取って消す）。読み込みの途中で知らせが届かなくても、失われない。
/// ウィンドウを作るのは、Windows で固まらないよう、非同期のコマンドで行う（Tauri の注意）。
#[tauri::command]
async fn open_preview_window(
    request: Option<Value>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if request.is_some() {
        locked(&state.preview_request)?.clone_from(&request);
    }
    if let Some(window) = app.get_webview_window(PREVIEW_WINDOW) {
        if let Some(request) = request {
            app.emit_to(PREVIEW_WINDOW, "preview-request", request)
                .map_err(|e| format!("プレビューのウィンドウに知らせられません：{e}"))?;
        }
        let _ = window.unminimize();
        return window
            .set_focus()
            .map_err(|e| format!("プレビューのウィンドウを前に出せません：{e}"));
    }
    tauri::WebviewWindowBuilder::new(&app, PREVIEW_WINDOW, tauri::WebviewUrl::default())
        .title("プレビュー — scn-studio")
        .inner_size(1120.0, 780.0)
        .build()
        .map_err(|e| format!("プレビューのウィンドウを開けません：{e}"))?;
    Ok(())
}

/// 開いたときの再生の頼みを受け取る（一度だけ）。
#[tauri::command]
fn take_preview_request(state: State<'_, AppState>) -> Result<Response, String> {
    let request = locked(&state.preview_request)?.take();
    json_response(&request.unwrap_or(Value::Null))
}

// ---- 通読（縦書きビュー）とプレーンテキスト（T2-5、D056） ----

/// 通読の道の候補（共通、ルート、エンディング）。
#[tauri::command]
fn reading_choices(state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let project = open_workspace(&mut guard)?.project();
    let choices = with_ir(project, |ir| {
        to_json(&route::reading_choices(ir), "通読の道の候補")
    })?;
    json_response(&choices)
}

/// 通読する（保存していない本文も含む）。`persistent` は、永続の変数の今の値。
#[tauri::command]
fn read_route(
    target: route::ReadingTarget,
    persistent: Option<route::Variables>,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let project = open_workspace(&mut guard)?.project();
    let persistent = persistent.unwrap_or_default();
    let reading = with_ir(project, |ir| {
        let reading = route::read(project, ir, &target, &persistent);
        let label = route::target_label(ir, &target);
        Ok(json!({
            "label": label,
            "reading": to_json(&reading, "通読")?,
        }))
    })?;
    json_response(&reading)
}

/// 通読の道を、作品フォルダの `export/text/<道の名前>.txt` に書き出す（`scn export text` と同じ）。
#[tauri::command]
fn export_text(
    target: route::ReadingTarget,
    notes: bool,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let (project, root) = (workspace.project(), workspace.root());
    let (label, text, message) = with_ir(project, |ir| {
        let reading = route::read(project, ir, &target, &route::Variables::new());
        let label = route::target_label(ir, &target);
        let text = scn_core::plaintext::plain_text(project, &reading, &label, notes);
        Ok((label, text, reading.message))
    })?;
    if let Some(message) = message {
        return Err(format!("{label}：{message}"));
    }
    let path =
        scn_core::plaintext::write_plain_text(root, &label, &text).map_err(|e| e.to_string())?;
    json_response(&json!({ "path": path.display().to_string() }))
}

// ---- 素材カタログ（T2-3、D054） ----

/// 素材の一覧（保存していない本文も含めて数える）。
fn asset_list_of(workspace: &Workspace) -> Result<Value, String> {
    let (project, root) = (workspace.project(), workspace.root());
    with_index(project, |_, index| {
        to_json(&assets::asset_list(project, root, index), "素材の一覧")
    })
}

/// カタログを書いた後に返すもの：書いた結果、素材の一覧、補完の材料、表示に要る分、検証の段階1の診断。
fn after_catalog_write(workspace: &Workspace, edit: Value) -> Result<Response, String> {
    let Analyzed {
        summary, checks, ..
    } = analyze_summary(workspace.project(), true, None)?;
    json_response(&json!({
        "edit": edit,
        "assets": asset_list_of(workspace)?,
        "assist": completion_data(workspace.project()),
        "summary": summary,
        "checks": checks,
    }))
}

/// 素材の一覧（素材カタログの画面を開いたとき、本文や設定が変わったとき）。
#[tauri::command]
fn asset_list(state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    json_response(&asset_list_of(workspace)?)
}

/// カタログに意味名を足す（仮素材。D054）。
#[tauri::command]
fn add_catalog_entries(
    items: Vec<AssetKey>,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let edit = workspace
        .add_catalog_entries(&items)
        .map_err(|e| e.to_string())?;
    after_catalog_write(workspace, to_json(&edit, "書いた結果")?)
}

/// カタログの意味名にファイルを割り当てる。`path` は選んだファイル（`assets/` の中だけ。コアで確かめる）。
#[tauri::command]
fn assign_asset_file(
    item: AssetKey,
    path: String,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let edit = workspace
        .assign_asset_file(&item, Path::new(&path))
        .map_err(|e| e.to_string())?;
    after_catalog_write(workspace, to_json(&edit, "書いた結果")?)
}

/// 発注リストを作品フォルダの `export/orders.csv` に書き出す（`scn export orders` と同じ）。
#[tauri::command]
fn export_orders(state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let (project, root) = (workspace.project(), workspace.root());
    let rows = with_index(project, |_, index| {
        Ok(assets::orders(&assets::asset_list(project, root, index)))
    })?;
    let path = assets::write_orders(root, &rows).map_err(|e| e.to_string())?;
    json_response(&json!({
        "path": path.display().to_string(),
        "rows": rows.len(),
    }))
}

/// Ren'Py のスクリプトを、出力プロファイル（`export/renpy.yaml`）に従って書き出す（`scn export renpy` と
/// 同じ。D053）。保存していない本文も含める。書き出し先、ファイルの数、知らせ、プロファイルの誤りを返す。
#[tauri::command]
fn export_renpy(state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let (project, root) = (workspace.project(), workspace.root());
    let (profile, problems) = renpy::read_profile(root);
    let export = thread::scope(|scope| {
        thread::Builder::new()
            .name("renpy".into())
            .stack_size(ANALYZE_STACK_SIZE)
            .spawn_scoped(scope, || {
                renpy::export_renpy(project, root, &analyze(project), &profile)
            })
            .map_err(|e| format!("解析のスレッドを作れません：{e}"))?
            .join()
            .map_err(|_| "解析が途中で止まりました".to_owned())
    })?;
    let path = renpy::write_renpy(root, &profile, &export, None).map_err(|e| e.to_string())?;
    json_response(&json!({
        "path": path.display().to_string(),
        "files": export.files.len(),
        "warnings": export.warnings,
        "problems": problems,
    }))
}

/// 大きなスタックのスレッドで解析して索引を作り、`f` に渡す（IRはこのスレッドの中で捨てる）。
fn with_index<T: Send>(
    project: &Project,
    f: impl FnOnce(&scn_core::Ir, &scn_core::index::Index) -> Result<T, String> + Send,
) -> Result<T, String> {
    thread::scope(|scope| {
        thread::Builder::new()
            .name("index".into())
            .stack_size(ANALYZE_STACK_SIZE)
            .spawn_scoped(scope, || {
                let ir = analyze(project);
                let index = build_index(project, &ir);
                f(&ir, &index)
            })
            .map_err(|e| format!("解析のスレッドを作れません：{e}"))?
            .join()
            .map_err(|_| "解析が途中で止まりました".to_owned())?
    })
}

/// 構造グラフ（T1-9、D048）：粒度（`scene`・`chapter`・`route`）ごとのまとまりと辺（コアの `graph`）。
/// フローの画面を開いているときにだけ呼ぶ（入力のたびの解析には入れない。D042）。
#[tauri::command]
fn scene_graph(level: String, state: State<'_, AppState>) -> Result<Response, String> {
    let level = match level.as_str() {
        "scene" => Level::Scene,
        "chapter" => Level::Chapter,
        "route" => Level::Route,
        _ => return Err(format!("知らない粒度です：{level}")),
    };
    let mut guard = locked(&state.workspace)?;
    let project = open_workspace(&mut guard)?.project();
    let graph = thread::scope(|scope| {
        thread::Builder::new()
            .name("graph".into())
            .stack_size(ANALYZE_STACK_SIZE)
            .spawn_scoped(scope, || {
                let ir = analyze(project);
                let index = build_index(project, &ir);
                to_json(&build_graph(&ir, &index, level), "構造グラフ")
            })
            .map_err(|e| format!("解析のスレッドを作れません：{e}"))?
            .join()
            .map_err(|_| "解析が途中で止まりました".to_owned())?
    })?;
    json_response(&graph)
}

/// フロー図の配置（`layout.json`）を読む。壊れていても止めず、読めた分と、読み飛ばしたことを返す。
#[tauri::command]
fn read_layout(state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let root = open_workspace(&mut guard)?.root().to_owned();
    json_response(&to_json(&read_layout_file(&root), "配置")?)
}

/// フロー図の配置を書く。`positions` をその粒度の位置に重ねる。`replace` なら、その粒度の位置を
/// すべて捨ててから書く（自動整列）。ほかの粒度と、無いシーンを指す位置は残す（コアの `update_layout`）。
#[tauri::command]
fn write_layout(
    level: String,
    positions: BTreeMap<String, (f64, f64)>,
    replace: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let positions = positions
        .into_iter()
        .map(|(id, (x, y))| (id, Point { x, y }))
        .collect();
    workspace
        .update_layout(&level, &positions, replace)
        .map_err(|e| e.to_string())
}

/// `story/` に章（空の `.scn`）を足す。
#[tauri::command]
fn add_chapter(name: String, state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let file = workspace.add_chapter(&name).map_err(|e| e.to_string())?;
    let Analyzed {
        summary, checks, ..
    } = analyze_summary(workspace.project(), true, None)?;
    json_response(&json!({ "file": file, "summary": summary, "checks": checks }))
}

/// ファイルの監視を始める。知らせは少し待ってまとめ、コアの `Workspace` で取り込み、
/// 変わったものがあれば `workspace-changed` をフロントへ送る。
fn watch(app: &AppHandle, root: &Path) -> Result<RecommendedWatcher, String> {
    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::recommended_watcher(move |event| {
        let _ = tx.send(event);
    })
    .map_err(|e| format!("ファイルの監視を始められません：{e}"))?;
    watcher
        .watch(root, RecursiveMode::Recursive)
        .map_err(|e| format!("ファイルの監視を始められません：{e}"))?;
    let app = app.clone();
    thread::Builder::new()
        .name("watch".into())
        .spawn(move || {
            // 監視が捨てられると送り手も捨てられ、受け取りが終わって、このスレッドも終わる。
            while let Ok(first) = rx.recv() {
                let mut paths = BTreeSet::new();
                let mut add = |event: notify::Result<notify::Event>| {
                    if let Ok(event) = event {
                        paths.extend(event.paths);
                    }
                };
                add(first);
                loop {
                    match rx.recv_timeout(WATCH_DEBOUNCE) {
                        Ok(event) => add(event),
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
                if let Err(e) = take_changes(&app, &paths) {
                    let _ = app.emit("workspace-error", e);
                }
            }
        })
        .map_err(|e| format!("ファイルの監視を始められません：{e}"))?;
    Ok(watcher)
}

/// 外部の変更を取り込み、変わったものがあればフロントへ知らせる。
fn take_changes(app: &AppHandle, paths: &BTreeSet<PathBuf>) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut guard = locked(&state.workspace)?;
    let Some(workspace) = guard.as_mut() else {
        return Ok(());
    };
    let changes: Vec<Change> = paths
        .iter()
        .map(|path| workspace.external_change(path))
        .filter(|change| *change != Change::None)
        .collect();
    if changes.is_empty() {
        return Ok(());
    }
    let Analyzed {
        summary, checks, ..
    } = analyze_summary(workspace.project(), true, None)?;
    let payload = json!({
        "changes": changes,
        "brackets": brackets(workspace.project()),
        "title": title_of(workspace),
        "editor": workspace.project().settings.editor,
        "assist": completion_data(workspace.project()),
        "summary": summary,
        "checks": checks,
    });
    app.emit("workspace-changed", payload)
        .map_err(|e| format!("画面に知らせられません：{e}"))
}

// ---- 最近開いた作品フォルダ ----

/// 最近開いた作品フォルダ（アプリの設定のフォルダの `recent.json`）。作品フォルダの中には書かない。
#[derive(Serialize, Deserialize, Clone)]
struct RecentProject {
    path: String,
    title: String,
}

fn recent_file(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("recent.json"))
}

fn read_recent(app: &AppHandle) -> Vec<RecentProject> {
    recent_file(app)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// 最近開いたものの先頭に加える。書けなくても、開くのは止めない。
fn remember_recent(app: &AppHandle, root: &Path, title: &str) {
    let Some(file) = recent_file(app) else {
        return;
    };
    let path = std::fs::canonicalize(root)
        .unwrap_or_else(|_| root.to_owned())
        .display()
        .to_string();
    let mut list = read_recent(app);
    list.retain(|r| r.path != path);
    list.insert(
        0,
        RecentProject {
            path,
            title: title.to_owned(),
        },
    );
    list.truncate(RECENT_LIMIT);
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(&list) {
        let _ = std::fs::write(file, text);
    }
}

/// 最近開いた作品フォルダ（新しい順）。
#[tauri::command]
fn recent_projects(app: AppHandle) -> Vec<RecentProject> {
    read_recent(&app)
}

// ---- ファイルごとの表示 ----

/// 表示の名前（フロントの `ViewMode`）。
const VIEW_MODES: [&str; 2] = ["writing", "source"];

/// ファイルごとの表示（執筆表示かソース表示か）。アプリの設定のフォルダの `view-modes.json` に、
/// 作品フォルダの実際のパスごとに覚える。作品フォルダには書かない（D046）。
type ViewModes = BTreeMap<String, BTreeMap<String, String>>;

fn view_modes_file(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("view-modes.json"))
}

fn read_view_modes(app: &AppHandle) -> ViewModes {
    view_modes_file(app)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// 開いている作品フォルダの、覚えるときの名前（実際のパス）。
fn project_key(workspace: &Workspace) -> String {
    std::fs::canonicalize(workspace.root())
        .unwrap_or_else(|_| workspace.root().to_owned())
        .display()
        .to_string()
}

/// 開いている作品フォルダの、ファイルごとの表示。覚えていないファイルは含めない（フロントで執筆表示にする）。
#[tauri::command]
fn view_modes(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<BTreeMap<String, String>, String> {
    let mut guard = locked(&state.workspace)?;
    let key = project_key(open_workspace(&mut guard)?);
    Ok(read_view_modes(&app).remove(&key).unwrap_or_default())
}

/// ファイルの表示を覚える。書けなくても、表示の切り替えは止めない（エラーは返す）。
#[tauri::command]
fn set_view_mode(
    file: String,
    mode: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if !VIEW_MODES.contains(&mode.as_str()) {
        return Err(format!("知らない表示です：{mode}"));
    }
    let key = {
        let mut guard = locked(&state.workspace)?;
        let workspace = open_workspace(&mut guard)?;
        if workspace.text(&file).is_none() {
            return Err(format!("作品フォルダに無いファイルです：{file}"));
        }
        project_key(workspace)
    };
    let Some(path) = view_modes_file(&app) else {
        return Err("アプリの設定のフォルダが分かりません".to_owned());
    };
    let mut all = read_view_modes(&app);
    all.entry(key).or_default().insert(file, mode);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text = serde_json::to_string_pretty(&all).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("{} に書けません：{e}", path.display()))
}

// ---- 開発用 ----

/// 受け渡しの時間を測っているか（環境変数 `SCN_MEASURE_IPC`。D045）。
fn measuring() -> bool {
    std::env::var_os("SCN_MEASURE_IPC").is_some()
}

/// 開発用の設定。
/// - `project`：起動時に開く作品フォルダ。環境変数 `SCN_DEV_PROJECT`。無ければ、デバッグビルドでは
///   `samples/houkago`、リリースビルドでは開かない
/// - `measure_ipc`：環境変数 `SCN_MEASURE_IPC` があれば、開いた後に受け渡しの時間を測る（D045）。
///   解析の速さはビルドで大きく違うので、リリースビルド（`tauri dev --release`）でも測れるようにする
#[tauri::command]
fn dev_settings() -> Value {
    let project = std::env::var("SCN_DEV_PROJECT").ok().or_else(|| {
        cfg!(debug_assertions)
            .then(|| {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../samples/houkago")
                    .canonicalize()
                    .ok()
                    .map(|p| p.display().to_string())
            })
            .flatten()
    });
    json!({ "project": project, "measure_ipc": measuring() })
}

/// 開発用：フロントで測った値を、ターミナルに書く（受け渡しの時間を測っているときだけ）。
#[tauri::command]
fn dev_report(text: String) {
    if measuring() {
        eprintln!("[scn-studio] {text}");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        // メインウィンドウを閉じたら、プレビューのウィンドウも閉じて終える。プレビューのウィンドウを閉じたら、
        // メインウィンドウに知らせる（再生をメインウィンドウの画面に戻す）。
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                if window.label() == "main" {
                    window.app_handle().exit(0);
                } else if window.label() == PREVIEW_WINDOW {
                    let _ = window
                        .app_handle()
                        .emit_to("main", "preview-window-closed", ());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            open_project,
            create_project,
            parse_buffer,
            save_file,
            reload_file,
            keep_mine,
            add_chapter,
            scene_info,
            read_bible,
            entry_usage,
            set_entry_field,
            set_entry_body,
            write_entry_source,
            create_entry,
            read_ideas,
            create_idea,
            promote_idea,
            read_concept,
            create_concept,
            read_variables,
            write_variables,
            preview_data,
            script_sheet,
            export_script,
            export_renpy,
            asset_list,
            route_at,
            route_to_scene,
            reading_choices,
            read_route,
            export_text,
            open_preview_window,
            take_preview_request,
            add_catalog_entries,
            assign_asset_file,
            export_orders,
            scene_graph,
            read_layout,
            write_layout,
            recent_projects,
            view_modes,
            set_view_mode,
            dev_settings,
            dev_report
        ])
        .run(tauri::generate_context!())
        .expect("Tauriのアプリを起動できません");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 入れ子の深い本文でも、解析と直列化と破棄が落ちない。Windowsのメインスレッド（1MB）を模して、
    /// スタックが1MBのスレッドから呼ぶ。
    #[test]
    fn deep_nesting_on_small_caller_stack() {
        let mut text = String::from("## a #s1\n");
        for _ in 0..1000 {
            text.push_str("@if a\n");
        }
        let project = Project {
            files: vec![scn_core::project::SourceFile {
                path: "story/a.scn".into(),
                text,
            }],
            ..Project::default()
        };
        let (summary, checks, scene) = thread::Builder::new()
            .stack_size(1024 * 1024)
            .spawn(move || {
                analyze_summary(&project, true, Some("s1")).map(|a| (a.summary, a.checks, a.scene))
            })
            .unwrap()
            .join()
            .unwrap()
            .unwrap();
        assert_eq!(summary["scenes"].as_array().unwrap().len(), 1);
        assert!(checks.is_some());
        assert_eq!(scene.unwrap()["scene"], "s1");
    }

    /// 渡すのは表示に要る分だけで、命令列（`ops`）を含まない（D042）。
    #[test]
    fn summary_has_no_ops() {
        let project = scn_core::load_project(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago"),
        )
        .unwrap();
        let Analyzed {
            summary,
            checks,
            scene,
        } = analyze_summary(&project, false, None).unwrap();
        assert!(scene.is_none());
        assert_eq!(summary["scenes"].as_array().unwrap().len(), 13);
        assert!(summary["scenes"][0].get("ops").is_none());
        assert_eq!(summary["edges"].as_array().unwrap().len(), 12);
        assert_eq!(summary["start_scene"], "s010");
        // シーンごとの文字数を持つ（D047）。
        assert!(summary["scenes"][0]["chars"].as_u64().unwrap() > 0);
        assert!(checks.is_none());
    }

    /// はみ出し（W203）は、入力が止まるたびの診断（summary）に入れ、検証の段階1（checks）からは除く（D047）。
    #[test]
    fn overflow_in_summary_not_in_checks() {
        let project = scn_core::load_project(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../crates/scn-core/tests/fixtures/check/overflow"),
        )
        .unwrap();
        let Analyzed {
            summary, checks, ..
        } = analyze_summary(&project, true, None).unwrap();
        let codes = |v: &Value| -> Vec<String> {
            v.as_array()
                .unwrap()
                .iter()
                .map(|d| d["code"].as_str().unwrap().to_owned())
                .collect()
        };
        assert!(codes(&summary["diagnostics"]).contains(&"W203".to_owned()));
        assert!(!codes(&checks.unwrap()).contains(&"W203".to_owned()));
    }
}
