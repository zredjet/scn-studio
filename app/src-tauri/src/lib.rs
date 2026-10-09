//! scn-studio のデスクトップアプリ（Tauri）。
//!
//! コマンドは薄く保ち、処理は scn-core に置く（`.claude/rules/rust.md`）。開いている作品フォルダは
//! コアの `Workspace` が持ち、ファイルを読み書きするのはコアだけにする。フロントには任意のパスを読ませない。
//!
//! 入力が止まるたびに渡すのは、表示に要る分（診断、シーンの一覧、辺）だけにする（D042）。命令列（IR全体）は
//! 渡さない（プレビューの画面を開いているときだけ `preview_data` で渡す）。検証の段階1は、開いたとき・保存したとき・外部の変更を取り込んだときに行う。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use scn_core::ai::{self, KeyStore, MachineSettings, SystemKeys};
use scn_core::assets;
use scn_core::assist::{self, completion_data, scene_chars};
use scn_core::bible;
use scn_core::check::{check, overflow};
use scn_core::concept::concept_data;
use scn_core::flowedit::FlowEdge;
use scn_core::graph::{Level, build_graph};
use scn_core::ideas::{self, Promotion};
use scn_core::index::build_index;
use scn_core::ir::{Edge, IrDiagnostic, IrMeta};
use scn_core::layout::{Point, read_layout as read_layout_file};
use scn_core::line_id::LineIdGenerator;
use scn_core::merge::{Choice, merge_text};
use scn_core::plugin::{self, PluginKind};
use scn_core::preview;
use scn_core::rename::{self, RenameTarget};
use scn_core::renpy;
use scn_core::route;
use scn_core::scaffold::create_project as scaffold_project;
use scn_core::script;
use scn_core::typedef::TypeEdit;
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
    /// 「接続を試す」の取り消し（T3-5）。
    ai_cancel: Mutex<AiCancel>,
    /// 走っている自律の実行（T3-9）。1つだけ。
    auto: Mutex<Option<AutoTarget>>,
}

/// 走っている自律の実行の書き込み先。
#[derive(Clone)]
struct AutoTarget {
    root: PathBuf,
    file: String,
    /// Git のブランチで書いている（作業ツリーが AI のブランチなので、本文を保存しない）。
    branch: bool,
    /// 企画と設定の段がある（企画書と設定DBも書くので、人の書き換えを止める。T4-6）。
    settings: bool,
}

/// 企画と設定の段のある自律の実行中に、企画書と設定DBのエントリを書き換える操作を止める（T4-6）。`path` が
/// アイデアメモなら止めない（段はメモを書かない）。
fn refuse_settings_during_auto(
    state: &AppState,
    root: &Path,
    path: Option<&str>,
) -> Result<(), String> {
    if path.is_some_and(|p| !scn_core::provenance::is_field_file(p)) {
        return Ok(());
    }
    let auto = locked(&state.auto)?;
    if auto.as_ref().is_some_and(|t| t.root == root && t.settings) {
        return Err(
            "自律で実行しています（企画書と設定DBにも書いています）。終わってから書き換えます"
                .to_owned(),
        );
    }
    Ok(())
}

/// 自律の実行中に、本文を書き換える操作を止める（T3-9）。`file` は書き換える章（`None` は、どの章も書き換えうる
/// 操作）。ブランチで実行している間は、どの章も書き換えない。
fn refuse_during_auto(state: &AppState, root: &Path, file: Option<&str>) -> Result<(), String> {
    let auto = locked(&state.auto)?;
    let Some(target) = auto.as_ref().filter(|t| t.root == root) else {
        return Ok(());
    };
    if target.branch {
        return Err(
            "自律で実行しています（Git のブランチに書いています）。終わってから保存します"
                .to_owned(),
        );
    }
    match file {
        Some(f) if f != target.file => Ok(()),
        _ => Err(format!(
            "自律で実行しています（{} に書いています）。終わってから保存します",
            target.file
        )),
    }
}

/// 走っている自律の実行の登録。外れるときに消す。
struct AutoGuard<'a> {
    state: &'a AppState,
}

impl Drop for AutoGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut auto) = self.state.auto.lock() {
            *auto = None;
        }
    }
}

/// 「接続を試す」の取り消し。画面が付けた回の番号で照らす（取り消しと次の回の頼みが前後して届いても、
/// 違う回を止めない。始まる前に届いた取り消しも覚えておく）。
#[derive(Default)]
struct AiCancel {
    /// 走っている回と、その取り消しの旗。
    running: Vec<(u64, Arc<AtomicBool>)>,
    /// 始まる前に取り消された回（新しいものを数件だけ）。
    early: Vec<u64>,
}

/// 走っている回の登録。外れるときに消す。
struct AiRunGuard<'a> {
    state: &'a AppState,
    run: u64,
}

impl Drop for AiRunGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut cancel) = self.state.ai_cancel.lock() {
            cancel.running.retain(|(r, _)| *r != self.run);
        }
    }
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
    // 本文として読むのは `story/` の下（下のフォルダも）の *.scn だけ（仕様書17節）。知らないファイルは受け付けない。
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
    refuse_during_auto(&state, workspace.root(), Some(&file))?;
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
                "provenance": saved.provenance,
                "provenance_problem": saved.provenance_problem,
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
fn keep_mine(
    file: String,
    text: String,
    base: Option<String>,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    refuse_during_auto(&state, workspace.root(), Some(&file))?;
    // アプリが覚えていた元（知らせの途中の競合）は、人が直し始めた元でもある（出自の記録。T3-4）。
    if let Some(base) = &base {
        workspace.remember_base(&file, base);
    }
    let disk = workspace
        .keep_mine(&file, &text)
        .map_err(|e| e.to_string())?;
    let Analyzed {
        summary, checks, ..
    } = analyze_summary(workspace.project(), true, None)?;
    json_response(&json!({ "disk": disk, "summary": summary, "checks": checks }))
}

/// 外部の変更とのマージの結果（T2-7、D058）：まとまった部分とぶつかり。`base` は、アプリが覚えている元
/// （知らせの途中の競合のとき）。
#[tauri::command]
fn merge_view(
    file: String,
    base: Option<String>,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let merge = workspace
        .merge_view(&file, base.as_deref())
        .map_err(|e| e.to_string())?;
    json_response(&to_json(&merge, "マージの結果")?)
}

/// マージを当てる（ぶつかりごとに選んだもの）。`token` は見せたマージの見分けで、入力が変わっていれば断る。
/// マージした本文と、ディスクの中身（外の中身）と、表示に要る分と、検証の段階1の診断を返す。
#[tauri::command]
fn apply_merge(
    file: String,
    base: Option<String>,
    choices: Vec<Choice>,
    token: String,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    refuse_during_auto(&state, workspace.root(), Some(&file))?;
    let applied = workspace
        .apply_merge(&file, base.as_deref(), &choices, &token)
        .map_err(|e| e.to_string())?;
    let Analyzed {
        summary, checks, ..
    } = analyze_summary(workspace.project(), true, None)?;
    json_response(&json!({
        "text": applied.text,
        "disk": applied.disk,
        "summary": summary,
        "checks": checks,
    }))
}

/// 設定ファイルのソース表示の3方向のマージ（行IDを見ない）。ぶつかりは `<<<<<<<` の印で囲んだテキストにする。
#[tauri::command]
fn merge_source(base: String, mine: String, theirs: String) -> Result<Response, String> {
    let merge = merge_text(&base, &mine, &theirs);
    json_response(&json!({ "text": merge.with_markers(), "conflicts": merge.conflicts }))
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

/// 企画書と設定DBのエントリの、今の出自が `ai`・`ai-edited` の項目（画面の印。T4-3）。記録を読めなければ空。
fn ai_field_list(workspace: &Workspace) -> Vec<Value> {
    let Ok(provenance) = scn_core::provenance::read_provenance(workspace.root()) else {
        return Vec::new();
    };
    let (rows, _) = scn_core::provenance::provenance_field_rows(
        workspace.root(),
        workspace.project(),
        &provenance,
    );
    rows.iter()
        .map(|r| json!({ "file": r.file, "key": r.key, "origin": r.origin }))
        .collect()
}

/// 設定DBか変数を書いた後に返すもの：書いた結果、設定DBの中身、補完の材料、表示に要る分、検証の段階1の診断、
/// AI の項目と、その出自を書けなかったわけ（T4-3）。
fn after_settings_write(workspace: &mut Workspace, edit: Value) -> Result<Response, String> {
    let Analyzed {
        summary, checks, ..
    } = analyze_summary(workspace.project(), true, None)?;
    let provenance_problem = workspace.take_field_provenance_problem();
    json_response(&json!({
        "edit": edit,
        "bible": bible_data(workspace),
        "assist": completion_data(workspace.project()),
        "summary": summary,
        "checks": checks,
        "ai_fields": ai_field_list(workspace),
        "provenance_problem": provenance_problem,
    }))
}

/// 企画書の下書き（T4-4）を書く：空いている項目にだけ書き、値のある項目は提案にして返す（書くのはコア）。
#[tauri::command]
fn write_concept_draft(
    draft: ai::ConceptDraft,
    task: String,
    model: String,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    refuse_settings_during_auto(&state, workspace.root(), None)?;
    let written = workspace
        .write_concept_draft(&draft, &task, &model)
        .map_err(|e| e.to_string())?;
    after_settings_write(workspace, to_json(&written, "書いた結果")?)
}

/// 設定の下書き（T4-5）を書く：足りないエントリを作り、今あるエントリの空いている項目に書き、値のある項目は
/// 案にして返す（書くのはコア）。`only` があれば、そのエントリにだけ書く。
#[tauri::command]
fn write_entries_draft(
    draft: ai::EntriesDraft,
    task: String,
    model: String,
    only: Option<String>,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    refuse_settings_during_auto(&state, workspace.root(), None)?;
    let written = workspace
        .write_entries_draft(&draft, &task, &model, only.as_deref())
        .map_err(|e| e.to_string())?;
    after_settings_write(workspace, to_json(&written, "書いた結果")?)
}

/// AI の下書きの項目の案を採用する（人の操作。T4-4）。`seen` は案を見せたときの中身。
#[tauri::command]
fn adopt_field_proposal(
    file: String,
    key: String,
    value: Value,
    seen: String,
    task: String,
    model: String,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    refuse_settings_during_auto(&state, workspace.root(), Some(&file))?;
    let edit = workspace
        .adopt_field_proposal(&file, &key, &value, &seen, &task, &model)
        .map_err(|e| e.to_string())?;
    after_settings_write(workspace, to_json(&edit, "書いた結果")?)
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
    let mut usage = with_index(project, |ir, index| {
        to_json(&bible::entry_usage(project, ir, index, &path), "逆引き")
    })?;
    // 「AIに渡す内容」（T3-6）：どのタスクに、どの部分が渡るか。
    if let Value::Object(map) = &mut usage {
        map.insert(
            "ai".to_owned(),
            to_json(&ai::entry_context(project, &path), "AIに渡す内容")?,
        );
    }
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
    refuse_settings_during_auto(&state, workspace.root(), Some(&path))?;
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
    refuse_settings_during_auto(&state, workspace.root(), Some(&path))?;
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
    refuse_settings_during_auto(&state, workspace.root(), Some(&path))?;
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
    refuse_settings_during_auto(&state, workspace.root(), None)?;
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
    refuse_settings_during_auto(&state, workspace.root(), None)?;
    if let Promotion::Scene { file, .. } = &promotion {
        refuse_during_auto(&state, workspace.root(), Some(file))?;
    }
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
    refuse_settings_during_auto(&state, workspace.root(), None)?;
    let text = workspace.create_concept().map_err(|e| e.to_string())?;
    after_settings_write(workspace, json!({ "kind": "saved", "text": text }))
}

/// 型定義のファイルの中身（型の編集の画面。T2-10）。
#[tauri::command]
fn type_text(type_id: String, state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let text = workspace.type_text(&type_id).map_err(|e| e.to_string())?;
    json_response(&json!(text))
}

/// 型定義を編集する（項目を足す・直す・動かす・消す。T2-10）。
#[tauri::command]
fn edit_type(
    type_id: String,
    edit: TypeEdit,
    seen: String,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let result = workspace
        .edit_type(&type_id, &edit, &seen)
        .map_err(|e| e.to_string())?;
    after_settings_write(workspace, to_json(&result, "書いた結果")?)
}

/// 型を作る（T2-10）。`source` は元にするもの（空の型、型のテンプレート、今ある型の複製。T4-9）。
#[tauri::command]
fn create_type(
    type_id: String,
    name: String,
    source: scn_core::typedef::TypeSource,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let file = workspace
        .create_type(&type_id, &name, &source)
        .map_err(|e| e.to_string())?;
    after_settings_write(workspace, json!({ "kind": "created", "path": file }))
}

/// 設定DBの型のテンプレート集（「＋ 型」の元。T4-9）。作品に依らない。
#[tauri::command]
fn type_templates() -> Vec<scn_core::scaffold::TypeTemplateRow> {
    scn_core::scaffold::type_template_rows()
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

/// 変数の宣言を書いた後に返すもの（`write_variables` と同じ形）。
fn after_variables_write(workspace: &Workspace, edit: Value) -> Result<Response, String> {
    let Analyzed {
        summary, checks, ..
    } = analyze_summary(workspace.project(), true, None)?;
    json_response(&json!({
        "edit": edit,
        "variables": variables_data(workspace)?,
        "assist": completion_data(workspace.project()),
        "summary": summary,
        "checks": checks,
    }))
}

/// 変数の宣言の1つの項目を直す（フォーム。T2-10）。`value` が null なら項目を消す。
#[tauri::command]
fn set_variable_field(
    key: String,
    field: String,
    value: Value,
    seen: String,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let edit = workspace
        .set_variable_field(&key, &field, &value, &seen)
        .map_err(|e| e.to_string())?;
    after_variables_write(workspace, to_json(&edit, "書いた結果")?)
}

/// 変数を宣言する（フォーム。T2-10）。
#[tauri::command]
fn add_variable(
    key: String,
    ty: String,
    values: Vec<String>,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let edit = workspace
        .add_variable(&key, &ty, &values)
        .map_err(|e| e.to_string())?;
    after_variables_write(workspace, to_json(&edit, "書いた結果")?)
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
/// 書き出しと通読の IR。`ai_drafts` が偽なら、AI下書きのシーンの台詞と地の文を省く（T3-9、D070）。
fn export_ir(project: &Project, ai_drafts: bool) -> scn_core::Ir {
    let ir = analyze(project);
    if ai_drafts {
        ir
    } else {
        ai::without_ai_drafts(&ir)
    }
}

fn script_of(project: &Project, ai_drafts: bool) -> Result<script::ScriptSheet, String> {
    thread::scope(|scope| {
        thread::Builder::new()
            .name("script".into())
            .stack_size(ANALYZE_STACK_SIZE)
            .spawn_scoped(scope, || {
                script::script(project, &export_ir(project, ai_drafts))
            })
            .map_err(|e| format!("解析のスレッドを作れません：{e}"))?
            .join()
            .map_err(|_| "解析が途中で止まりました".to_owned())
    })
}

/// 台本に、収録の基準と比べた再収録候補を書き足す（T2-10、D061）。基準を読めなければ、そのわけを返す。
fn with_retakes(root: &Path, sheet: &mut script::ScriptSheet) -> Option<String> {
    match script::read_baseline(root) {
        Ok(Some(base)) => {
            script::mark_retakes(sheet, &base);
            None
        }
        Ok(None) => None,
        Err(problem) => Some(problem),
    }
}

// ---- 出自の記録（T3-4、D065） ----

/// AI が書いた行（行IDから、今の出自 `ai`・`ai-edited`）と、数と、出自の記録の読めない行の数。
/// エディタの印と、出力の画面の「AIの出自」に使う。保存していない本文も含む。
#[tauri::command]
fn ai_lines(state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let provenance =
        scn_core::provenance::read_provenance(workspace.root()).map_err(|e| e.to_string())?;
    let project = workspace.project();
    let (rows, counts) =
        scn_core::provenance::provenance_rows(project, &analyze(project), &provenance);
    let lines: BTreeMap<&str, &str> = rows
        .iter()
        .map(|r| (r.id.as_str(), r.origin.as_str()))
        .collect();
    let (field_rows, field_counts) =
        scn_core::provenance::provenance_field_rows(workspace.root(), project, &provenance);
    let fields: Vec<Value> = field_rows
        .iter()
        .map(|r| json!({ "file": r.file, "key": r.key, "origin": r.origin }))
        .collect();
    json_response(&json!({
        "lines": lines,
        "counts": counts,
        // 企画書と設定DBの AI の項目（T4-3）。
        "fields": fields,
        "field_counts": field_counts,
        "problems": provenance.problems.len(),
    }))
}

/// AI が書いた行の一覧を、作品フォルダの `export/provenance.csv` に書き出す（`scn provenance -o` と同じ）。
#[tauri::command]
fn export_provenance(state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let provenance =
        scn_core::provenance::read_provenance(workspace.root()).map_err(|e| e.to_string())?;
    let project = workspace.project();
    let (mut rows, _) =
        scn_core::provenance::provenance_rows(project, &analyze(project), &provenance);
    let (field_rows, _) =
        scn_core::provenance::provenance_field_rows(workspace.root(), project, &provenance);
    rows.extend(field_rows);
    let path = scn_core::provenance::write_provenance_csv(workspace.root(), &rows)
        .map_err(|e| e.to_string())?;
    json_response(&json!({
        "path": path.display().to_string(),
        "rows": rows.len(),
    }))
}

// ---- 検証の段階2（T3-2、D063） ----

/// 自動巡回（検証の画面の「自動巡回」）。保存していない本文も含む。大きなスタックのスレッドで行う。
/// 時間がかかることがあるので、作品の写しを取ってワークスペースのロックを放してから調べる（その間も、
/// ほかのコマンドが動くように）。
#[tauri::command]
fn playthrough(state: State<'_, AppState>) -> Result<Response, String> {
    let project = {
        let mut guard = locked(&state.workspace)?;
        open_workspace(&mut guard)?.project().clone()
    };
    let report = thread::scope(|scope| {
        thread::Builder::new()
            .name("playthrough".into())
            .stack_size(ANALYZE_STACK_SIZE)
            .spawn_scoped(scope, || {
                scn_core::playthrough::stage2(&project, &analyze(&project))
            })
            .map_err(|e| format!("解析のスレッドを作れません：{e}"))?
            .join()
            .map_err(|_| "自動巡回が途中で止まりました".to_owned())
    })?;
    json_response(&to_json(&report, "自動巡回の結果")?)
}

/// 台本（出力の画面で、書き出す前に中身を見る）。保存していない本文も含む。収録の基準があれば、変わった台詞を
/// 再収録候補にする（`baseline_problem` は基準を読めなかったわけ）。
#[tauri::command]
fn script_sheet(ai_drafts: Option<bool>, state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let mut sheet = script_of(workspace.project(), ai_drafts.unwrap_or(true))?;
    let problem = with_retakes(workspace.root(), &mut sheet);
    let mut value = to_json(&sheet, "台本")?;
    if let Value::Object(map) = &mut value {
        map.insert("baseline_problem".to_owned(), json!(problem));
    }
    json_response(&value)
}

/// 台本を作品フォルダの `export/script.csv`（Excel 形式は `export/script.xlsx`）に書き出す（`scn export script`
/// と同じ）。`baseline` なら、書き出した台本を収録の基準として記録する。書いたパスと行数を返す。
#[tauri::command]
fn export_script(
    format: script::ScriptFormat,
    baseline: bool,
    ai_drafts: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let mut sheet = script_of(workspace.project(), ai_drafts.unwrap_or(true))?;
    with_retakes(workspace.root(), &mut sheet);
    let path = script::write_script(workspace.root(), &sheet, format).map_err(|e| e.to_string())?;
    if baseline {
        script::write_baseline(workspace.root(), &sheet).map_err(|e| e.to_string())?;
    }
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
    with_export_ir(project, true, f)
}

/// [`with_ir`] で、AI下書きを含めるかを選ぶ（T3-9）。
fn with_export_ir<T: Send>(
    project: &Project,
    ai_drafts: bool,
    f: impl FnOnce(&scn_core::Ir) -> Result<T, String> + Send,
) -> Result<T, String> {
    thread::scope(|scope| {
        thread::Builder::new()
            .name("route".into())
            .stack_size(ANALYZE_STACK_SIZE)
            .spawn_scoped(scope, || f(&export_ir(project, ai_drafts)))
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
    ai_drafts: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let project = open_workspace(&mut guard)?.project();
    let persistent = persistent.unwrap_or_default();
    let reading = with_export_ir(project, ai_drafts.unwrap_or(true), |ir| {
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
    ai_drafts: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let (project, root) = (workspace.project(), workspace.root());
    let (label, text, message) = with_export_ir(project, ai_drafts.unwrap_or(true), |ir| {
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

/// 出力プロファイル（`export/book.yaml`）の書き方と注記（「出力」の画面のチェックの初めの値。T3-13）と、PDF を
/// 組むコマンド（`pdf_command`。見せるための1行。T3-14）。
#[tauri::command]
fn book_settings(state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let (profile, _) = scn_core::book::read_profile(workspace.root());
    json_response(&json!({
        "vertical": profile.vertical,
        "notes": profile.notes,
        "pdf_command": profile.pdf_command.as_deref().map(plugin::command_line),
    }))
}

/// 通読の道を、EPUB（`export/epub/<道の名前>.epub`）か、縦書きPDFを組むための印刷用の HTML
/// （`export/pdf/<道の名前>.html`）に書き出す（`scn export epub`・`scn export pdf` と同じ。T3-13、D074）。
/// `horizontal`・`notes` はプロファイル（`export/book.yaml`）を上書きする。書いたパスとプロファイルの誤りを返す。
/// `typeset` なら、印刷用の HTML を書いた後に `pdf_command` で PDF を組む（`scn export pdf --typeset`。T3-14）。
/// 作品フォルダのコマンドなので、この端末で確かめていなければ、何も書かずに `needs_approval`（コマンドの1行）を
/// 返す。`approved`（人が見て「はい」と言ったコマンドの1行）が今のコマンドと同じなら、確かめたことを覚えてから
/// 実行する。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn export_book(
    target: route::ReadingTarget,
    print: bool,
    horizontal: bool,
    notes: bool,
    ai_drafts: Option<bool>,
    typeset: Option<bool>,
    approved: Option<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Response, String> {
    use scn_core::book;
    let (root, path, problems, command, timeout) = {
        let mut guard = locked(&state.workspace)?;
        let workspace = open_workspace(&mut guard)?;
        let (project, root) = (workspace.project(), workspace.root());
        let (mut profile, problems) = book::read_profile(root);
        let command = match (typeset.unwrap_or(false) && print, &profile.pdf_command) {
            (false, _) => None,
            (true, None) => {
                return Err(format!(
                    "{} に pdf_command（PDF を組むコマンド）がありません",
                    book::BOOK_PROFILE
                ));
            }
            (true, Some(argv)) => {
                if !command_approved(&app, root, book::BOOK_PROFILE, argv, approved.as_deref())? {
                    return json_response(&json!({
                        "needs_approval": plugin::command_line(argv),
                        "profile": book::BOOK_PROFILE,
                    }));
                }
                Some(argv.clone())
            }
        };
        if horizontal {
            profile.vertical = false;
        }
        profile.notes = notes;
        let modified = scn_core::provenance::now();
        let (label, bytes, message) = with_export_ir(project, ai_drafts.unwrap_or(true), |ir| {
            let reading = route::read(project, ir, &target, &route::Variables::new());
            let label = route::target_label(ir, &target);
            let bytes = if print {
                book::print_html(project, &reading, &label, &profile).into_bytes()
            } else {
                book::epub(project, &reading, &label, &profile, &modified)
                    .map_err(|e| e.to_string())?
            };
            Ok((label, bytes, reading.message))
        })?;
        if let Some(message) = message {
            return Err(format!("{label}：{message}"));
        }
        let path = if print {
            book::book_path(root, book::PRINT_DIR, &label, "html")
        } else {
            book::book_path(root, book::EPUB_DIR, &label, "epub")
        };
        book::write_book(&path, &bytes).map_err(|e| e.to_string())?;
        (
            root.to_owned(),
            path,
            problems,
            command,
            profile.pdf_timeout,
        )
    };
    let Some(argv) = command else {
        return json_response(&json!({ "path": path.display().to_string(), "problems": problems }));
    };
    let pdf = path.with_extension("pdf");
    let (input, output) = (path.clone(), pdf.clone());
    let result = tauri::async_runtime::spawn_blocking(move || {
        plugin::typeset(&root, &argv, timeout, &input, &output, true)
    })
    .await
    .map_err(|e| format!("PDF を組めません：{e}"))?;
    let (ok, error, output) = run_outcome(result);
    json_response(&json!({
        "path": path.display().to_string(),
        "problems": problems,
        "pdf": ok.then(|| pdf.display().to_string()),
        "error": error,
        "stdout": output.stdout,
        "stderr": output.stderr,
    }))
}

// ---- 書き出しのプラグイン（T3-14、D075） ----

/// 作品フォルダのコマンドを、この端末で実行してよいと確かめたもの（アプリの設定のフォルダの
/// `export-commands.json`）。作品フォルダの実際のパスごとに、プロファイル（`export/<名前>.yaml`・
/// `export/book.yaml`）と、確かめたコマンド（置き換える前の引数の列）を覚える。作品フォルダには書かない（人から
/// 受け取った作品フォルダのコマンドが、確かめずに動かないように）。
type Approvals = BTreeMap<String, BTreeMap<String, Vec<String>>>;

fn approvals_file(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|d| d.join("export-commands.json"))
        .map_err(|e| format!("アプリの設定のフォルダが分かりません：{e}"))
}

/// 作品フォルダのコマンドを確かめたか。`approved`（人に見せて「はい」と言われたコマンドの1行）が今のコマンドと
/// 同じなら、確かめたこととして覚える（書けなければ実行しない）。見せている間にプロファイルが変わっていれば、
/// 覚えずに確かめ直す。コマンドが変われば、確かめ直す。
fn command_approved(
    app: &AppHandle,
    root: &Path,
    profile: &str,
    argv: &[String],
    approved: Option<&str>,
) -> Result<bool, String> {
    let file = approvals_file(app)?;
    let mut approvals: Approvals = std::fs::read_to_string(&file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let project = std::fs::canonicalize(root)
        .unwrap_or_else(|_| root.to_owned())
        .display()
        .to_string();
    let known = approvals
        .get(&project)
        .and_then(|p| p.get(profile))
        .is_some_and(|a| a.as_slice() == argv);
    if known || approved != Some(plugin::command_line(argv).as_str()) {
        return Ok(known);
    }
    approvals
        .entry(project)
        .or_default()
        .insert(profile.to_owned(), argv.to_vec());
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{} を作れません：{e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(&approvals).map_err(|e| e.to_string())?;
    std::fs::write(&file, text).map_err(|e| format!("{} に書けません：{e}", file.display()))?;
    Ok(true)
}

/// 外部コマンドの結果を、成功か、誤りの1行と、標準出力と標準エラーにする。
fn run_outcome(
    result: Result<plugin::CommandOutput, plugin::PluginError>,
) -> (bool, Option<String>, plugin::CommandOutput) {
    match result {
        Ok(output) => (true, None, output),
        Err(e) => {
            let output = e.output().cloned().unwrap_or_default();
            (false, Some(e.to_string()), output)
        }
    }
}

/// 作品フォルダのプラグインのプロファイル（`export/*.yaml` のうち `template` か `command` のあるもの）の一覧。
#[tauri::command]
fn export_plugins(state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let items: Vec<Value> = plugin::list_plugins(workspace.root())
        .into_iter()
        .map(|(name, result)| match result {
            Ok(p) => {
                let (kind, detail) = match &p.kind {
                    PluginKind::Template { template } => ("template", template.clone()),
                    PluginKind::Command { argv } => ("command", plugin::command_line(argv)),
                };
                json!({
                    "name": name,
                    "profile": p.profile,
                    "kind": kind,
                    "detail": detail,
                    "output": p.output,
                    "problems": p.problems,
                    "error": null,
                })
            }
            Err(e) => json!({
                "name": name,
                "profile": format!("{}/{name}.yaml", plugin::PROFILE_DIR),
                "kind": "error",
                "detail": "",
                "output": "",
                "problems": [],
                "error": e.to_string(),
            }),
        })
        .collect();
    json_response(&json!(items))
}

/// プラグインで書き出す（`scn export <名前>` と同じ。保存していない本文も含める）。テンプレートは書いたファイルを
/// 返す。外部コマンドは、この端末で確かめていなければ、実行せずに `needs_approval`（コマンドの1行）を返し、
/// `approved`（人が見て「はい」と言ったコマンドの1行）が今のコマンドと同じなら、確かめたことを覚えてから実行する。
/// コマンドの失敗は `error` と標準出力・標準エラーで返す。
#[tauri::command]
async fn export_plugin(
    name: String,
    ai_drafts: Option<bool>,
    approved: Option<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Response, String> {
    let (root, found, ir) = {
        let mut guard = locked(&state.workspace)?;
        let workspace = open_workspace(&mut guard)?;
        let (project, root) = (workspace.project(), workspace.root());
        let found = plugin::read_plugin(root, &name).map_err(|e| e.to_string())?;
        match &found.kind {
            PluginKind::Template { .. } => {
                let text = with_export_ir(project, ai_drafts.unwrap_or(true), |ir| {
                    plugin::render_template(root, &found, project, ir).map_err(|e| e.to_string())
                })?;
                let path = root.join(&found.output);
                plugin::write_output(&path, &text).map_err(|e| e.to_string())?;
                return json_response(&json!({
                    "path": path.display().to_string(),
                    "problems": found.problems,
                }));
            }
            PluginKind::Command { argv } => {
                if !command_approved(&app, root, &found.profile, argv, approved.as_deref())? {
                    return json_response(&json!({
                        "needs_approval": plugin::command_line(argv),
                        "profile": found.profile,
                    }));
                }
            }
        }
        let ir = with_export_ir(project, ai_drafts.unwrap_or(true), |ir| Ok(ir.clone()))?;
        (root.to_owned(), found, ir)
    };
    let output = root.join(&found.output);
    let path = output.display().to_string();
    let problems = found.problems.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        plugin::export_command(&root, &found, &ir, &output, true)
    })
    .await
    .map_err(|e| format!("書き出せません：{e}"))?;
    let (ok, error, output) = run_outcome(result);
    json_response(&json!({
        "path": ok.then_some(path),
        "problems": problems,
        "error": error,
        "stdout": output.stdout,
        "stderr": output.stderr,
    }))
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

/// 素材の場所（T3-1、R191）：作品フォルダの `assets/`（`file` が無いとき）か、素材のファイル（`assets/` から数えた
/// パス。カタログの書き方）。場所の決め方はコア（`assets::asset_location`。無いファイルはいちばん近いある親のフォルダ、
/// `assets/` の外は断る）。`reveal` なら OS のファイルの画面（Finder、エクスプローラー）で見せる（`assets/` が無ければ
/// 作る）。返すのは絶対のパス（画面に見せて、コピーできるように）。
#[tauri::command]
fn asset_location(
    file: Option<String>,
    reveal: bool,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let root = {
        let mut guard = locked(&state.workspace)?;
        open_workspace(&mut guard)?.root().to_owned()
    };
    let assets = plain_path(&std::path::absolute(root.join("assets")).map_err(|e| e.to_string())?);
    if reveal {
        std::fs::create_dir_all(&assets)
            .map_err(|e| format!("{} を作れません：{e}", assets.display()))?;
    }
    let target = plain_path(&assets::asset_location(&assets, file.as_deref())?);
    if reveal {
        // 素材のフォルダそのものはそのまま開く（リンクなら、起動しないよう選んだ状態で見せる）。
        let linked = std::fs::symlink_metadata(&assets).is_ok_and(|m| m.file_type().is_symlink());
        reveal_in_file_manager(&target, target == assets && !linked)?;
    }
    Ok(target.display().to_string())
}

/// 作品フォルダを OS のファイルの画面で見せる（タイトルバーの作品メニューの「フォルダを表示」。T4-17）。
#[tauri::command]
fn reveal_project(state: State<'_, AppState>) -> Result<(), String> {
    let root = {
        let mut guard = locked(&state.workspace)?;
        open_workspace(&mut guard)?.root().to_owned()
    };
    let root = plain_path(&std::path::absolute(root).map_err(|e| e.to_string())?);
    reveal_in_file_manager(&root, true)
}

/// Windows の長いパスの印（`\\?\`）を外す（エクスプローラーと、人が写して使うパスのため）。ほかの OS ではそのまま。
fn plain_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path.to_owned(),
    }
}

/// OS のファイルの画面で見せる。素材のフォルダ（`folder`）はそのまま開き、ほかは親の中で選んだ状態で見せる
/// （macOS の `open` にフォルダを渡すと、`.app` のような束はアプリとして起動してしまうため）。
fn reveal_in_file_manager(path: &Path, folder: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let spawned = {
        let mut command = std::process::Command::new("open");
        if !folder {
            command.arg("-R");
        }
        command.arg(path).spawn()
    };
    #[cfg(target_os = "windows")]
    let spawned = {
        use std::os::windows::process::CommandExt;
        let mut command = std::process::Command::new("explorer");
        if folder {
            command.arg(path);
        } else {
            // `/select,"パス"` は1つの引数として、引用符ごとそのまま渡す。
            command.raw_arg(format!("/select,\"{}\"", path.display()));
        }
        command.spawn()
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let spawned = {
        let dir = if folder {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        std::process::Command::new("xdg-open").arg(dir).spawn()
    };
    let mut child = spawned.map_err(|e| format!("ファイルの画面を開けません：{e}"))?;
    // すぐ終わるコマンドを待って片づける（画面は止めない）。
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
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
fn export_renpy(ai_drafts: Option<bool>, state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let (project, root) = (workspace.project(), workspace.root());
    let (profile, problems) = renpy::read_profile(root);
    let export = thread::scope(|scope| {
        thread::Builder::new()
            .name("renpy".into())
            .stack_size(ANALYZE_STACK_SIZE)
            .spawn_scoped(scope, || {
                renpy::export_renpy(
                    project,
                    root,
                    &export_ir(project, ai_drafts.unwrap_or(true)),
                    &profile,
                )
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

/// ティラノスクリプトのシナリオを、出力プロファイル（`export/tyrano.yaml`）に従って書き出す（`scn export tyrano`
/// と同じ。T3-11、D072）。保存していない本文も含める。書き出し先、ファイルの数、知らせ、プロファイルの誤りを返す。
#[tauri::command]
fn export_tyrano(ai_drafts: Option<bool>, state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let (project, root) = (workspace.project(), workspace.root());
    let (profile, problems) = scn_core::tyrano::read_profile(root);
    let export = thread::scope(|scope| {
        thread::Builder::new()
            .name("tyrano".into())
            .stack_size(ANALYZE_STACK_SIZE)
            .spawn_scoped(scope, || {
                scn_core::tyrano::export_tyrano(
                    project,
                    root,
                    &export_ir(project, ai_drafts.unwrap_or(true)),
                    &profile,
                )
            })
            .map_err(|e| format!("解析のスレッドを作れません：{e}"))?
            .join()
            .map_err(|_| "解析が途中で止まりました".to_owned())
    })?;
    let path =
        scn_core::tyrano::write_tyrano(root, &profile, &export, None).map_err(|e| e.to_string())?;
    json_response(&json!({
        "path": path.display().to_string(),
        "files": export.files.len(),
        "warnings": export.warnings,
        "problems": problems,
    }))
}

/// 宴のシナリオと設定のシートを、出力プロファイル（`export/utage.yaml`）に従って書き出す（`scn export utage` と
/// 同じ。T3-12、D073）。保存していない本文も含める。書き出し先、ファイルの数、知らせ、プロファイルの誤りを返す。
#[tauri::command]
fn export_utage(ai_drafts: Option<bool>, state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let (project, root) = (workspace.project(), workspace.root());
    let (profile, problems) = scn_core::utage::read_profile(root);
    let export = thread::scope(|scope| {
        thread::Builder::new()
            .name("utage".into())
            .stack_size(ANALYZE_STACK_SIZE)
            .spawn_scoped(scope, || {
                scn_core::utage::export_utage(
                    project,
                    root,
                    &export_ir(project, ai_drafts.unwrap_or(true)),
                    &profile,
                )
            })
            .map_err(|e| format!("解析のスレッドを作れません：{e}"))?
            .join()
            .map_err(|_| "解析が途中で止まりました".to_owned())
    })?;
    let path =
        scn_core::utage::write_utage(root, &profile, &export, None).map_err(|e| e.to_string())?;
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

// ---- フロー図からの編集（T2-9、D060） ----

/// 書き足した章と、表示に要る分と、検証の段階1の診断。
fn story_written(
    workspace: &Workspace,
    written: &scn_core::workspace::StoryWritten,
) -> Result<Response, String> {
    let Analyzed {
        summary, checks, ..
    } = analyze_summary(workspace.project(), true, None)?;
    json_response(&json!({
        "file": written.file,
        "text": written.text,
        "summary": summary,
        "checks": checks,
    }))
}

/// 章の末尾にスタブのシーンを足す（フロー図の「＋ スタブ」）。
#[tauri::command]
fn add_stub(
    file: String,
    id: String,
    title: String,
    summary: Option<String>,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    refuse_during_auto(&state, workspace.root(), Some(&file))?;
    let written = workspace
        .add_stub(&file, &id, &title, summary.as_deref())
        .map_err(|e| e.to_string())?;
    story_written(workspace, &written)
}

/// フロー図で引いた辺を、本文に書き足す（ジャンプか選択肢）。
#[tauri::command]
fn add_flow_edge(edge: FlowEdge, state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    refuse_during_auto(&state, workspace.root(), None)?;
    let written = workspace.add_edge(&edge).map_err(|e| e.to_string())?;
    story_written(workspace, &written)
}

// ---- 名前の変更と章（T2-8、D059） ----

/// カーソルのところ（バイト位置）の、名前を変えられるもの（F2）。無ければ null。
#[tauri::command]
fn symbol_at(file: String, offset: usize, state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let project = open_workspace(&mut guard)?.project();
    json_response(&to_json(
        &rename::symbol_at(project, &file, offset),
        "名前",
    )?)
}

/// 名前の変更の見積もり（変わる本文の数と、ファイル）。名前が正しくなければ誤りの文。
#[tauri::command]
fn rename_preview(
    target: RenameTarget,
    new: String,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let plan = rename::plan_rename(workspace.project(), workspace.root(), &target, &new)?;
    let story: Vec<&str> = plan.story.iter().map(|c| c.file.as_str()).collect();
    let settings: Vec<&str> = plan.settings.iter().map(|c| c.file.as_str()).collect();
    json_response(&json!({
        "story_edits": plan.story_edits,
        "story": story,
        "settings": settings,
        "moves": plan.moves,
        "notes": plan.notes,
    }))
}

/// 名前を変える。変わる本文のファイル（保存していない変更ごと書く）と、設定を書いた後と同じものを返す。
#[tauri::command]
fn apply_rename(
    target: RenameTarget,
    new: String,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    refuse_during_auto(&state, workspace.root(), None)?;
    let renamed = workspace.rename(&target, &new).map_err(|e| e.to_string())?;
    after_settings_write(workspace, to_json(&renamed, "名前の変更")?)
}

/// 章のファイル名を変える。
#[tauri::command]
fn rename_chapter(
    file: String,
    name: String,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    refuse_during_auto(&state, workspace.root(), None)?;
    let to = workspace
        .rename_chapter(&file, &name)
        .map_err(|e| e.to_string())?;
    let Analyzed {
        summary, checks, ..
    } = analyze_summary(workspace.project(), true, None)?;
    json_response(&json!({ "file": to, "summary": summary, "checks": checks }))
}

/// 章を消す。
#[tauri::command]
fn delete_chapter(file: String, state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    refuse_during_auto(&state, workspace.root(), Some(&file))?;
    workspace.delete_chapter(&file).map_err(|e| e.to_string())?;
    let Analyzed {
        summary, checks, ..
    } = analyze_summary(workspace.project(), true, None)?;
    json_response(&json!({ "summary": summary, "checks": checks }))
}

/// 共通とルートごとの文字数と、読み終えるまでの目安（ステータスバー）。
#[tauri::command]
fn route_stats(state: State<'_, AppState>) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let project = open_workspace(&mut guard)?.project();
    let stats = with_ir(project, |ir| {
        to_json(&route::route_stats(project, ir), "ルートの文字数")
    })?;
    json_response(&stats)
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
        .flat_map(|path| workspace.external_changes(path))
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

// ---- AIの接続（T3-5、D066） ----

/// 端末の AI の設定（アプリの設定のフォルダの `ai.toml`）。作品フォルダには書かない。
fn ai_settings_file(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|d| d.join(ai::MACHINE_SETTINGS_FILE))
        .map_err(|e| format!("アプリの設定のフォルダが分かりません：{e}"))
}

fn ai_decls(state: &AppState) -> Result<Vec<scn_core::project::AiConnectionDecl>, String> {
    let mut guard = locked(&state.workspace)?;
    Ok(open_workspace(&mut guard)?.project().settings.ai.clone())
}

/// 今の作品フォルダの接続先の行き先か（キーと許可は、その行き先にだけ書く）。
fn known_endpoint(state: &AppState, app: &AppHandle, endpoint: &str) -> Result<(), String> {
    let (machine, _) = ai::read_machine_settings(&ai_settings_file(app)?);
    let known = ai_decls(state)?
        .iter()
        .filter_map(|d| ai::resolve(d, &machine).ok())
        .any(|c| c.endpoint == endpoint);
    if known {
        Ok(())
    } else {
        Err(format!("接続先の行き先「{endpoint}」が見つかりません"))
    }
}

fn update_ai_settings(
    app: &AppHandle,
    change: impl FnOnce(&mut MachineSettings),
) -> Result<(), String> {
    let path = ai_settings_file(app)?;
    let (mut machine, problem) = ai::read_machine_settings(&path);
    if let Some(problem) = problem {
        // 読めないファイルを、既定の値で黙って上書きしない。
        return Err(format!(
            "{problem}。書き換えません（直すか消してから、もう一度行います）"
        ));
    }
    change(&mut machine);
    ai::write_machine_settings(&path, &machine)
        .map_err(|e| format!("AIの端末の設定を書けません：{e}"))
}

/// 環境設定の接続先の一覧。キーそのものは渡さない（あるかどうかだけ）。キーチェーンを読む（確認の窓が
/// 出ることがある）ので、メインスレッドでは動かさない。
#[tauri::command(async)]
fn ai_connections(state: State<'_, AppState>, app: AppHandle) -> Result<Response, String> {
    let decls = ai_decls(&state)?;
    let (machine, settings_problem) = ai::read_machine_settings(&ai_settings_file(&app)?);
    let (connections, key_problem) = ai::connection_statuses(&decls, &machine, &SystemKeys);
    json_response(&json!({
        "connections": to_json(&connections, "接続先")?,
        "settings_problem": settings_problem,
        "key_problem": key_problem,
    }))
}

/// この端末での、接続先の URL・モデル・文脈の長さ（T4-1）・返す分の上限（T4-10）・構造化出力（T4-2）の上書き。
/// 空なら上書きをやめる。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn set_ai_override(
    name: String,
    base_url: Option<String>,
    model: Option<String>,
    context_tokens: Option<u64>,
    output_tokens: Option<u64>,
    structured_output: Option<bool>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    if !ai_decls(&state)?.iter().any(|d| d.name == name) {
        return Err(format!("接続先「{name}」が project.toml にありません"));
    }
    update_ai_settings(&app, |m| {
        m.set_override(
            &name,
            ai::Override {
                base_url,
                model,
                context_tokens,
                output_tokens,
                structured_output,
            },
        );
    })
}

/// この端末から、クラウドの行き先へ送ることを許可する・やめる。
#[tauri::command]
fn set_ai_cloud_allowed(
    endpoint: String,
    allowed: bool,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    known_endpoint(&state, &app, &endpoint)?;
    update_ai_settings(&app, |m| m.set_cloud_allowed(&endpoint, allowed))
}

/// キーを OS のキーチェーンに保存する（フロントからコアへ渡すだけで、返さない）。
#[tauri::command(async)]
fn set_ai_key(
    endpoint: String,
    key: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    known_endpoint(&state, &app, &endpoint)?;
    SystemKeys.set(&endpoint, &key).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn delete_ai_key(
    endpoint: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    known_endpoint(&state, &app, &endpoint)?;
    SystemKeys.delete(&endpoint).map_err(|e| e.to_string())
}

/// 接続を試す：決まった短い問い（`ai::test_request`）を送り、応答とトークン数と時間を返す。
/// 呼び出しは別のスレッドで行い、その間もほかのコマンドが動く。`run` は画面が付けた回の番号（取り消しに使う）。
#[tauri::command]
async fn ai_test(
    name: String,
    run: u64,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Response, String> {
    // 取り消しの旗は最初に登録する（作品やキーチェーンを読む間に届いた取り消しも効くように）。
    let (cancel, _registered) = register_ai_run(&state, run)?;
    let decls = ai_decls(&state)?;
    let (machine, _) = ai::read_machine_settings(&ai_settings_file(&app)?);
    let decl = decls
        .iter()
        .find(|d| d.name == name)
        .ok_or_else(|| format!("接続先「{name}」が project.toml にありません"))?;
    let conn = ai::resolve(decl, &machine)?;
    let key = key_for(&conn)?;
    let flag = Arc::clone(&cancel);
    let result = tauri::async_runtime::spawn_blocking(move || {
        let started = std::time::Instant::now();
        let done = ai::complete(
            &conn,
            key.as_deref(),
            &ai::test_request(),
            &ai::CallOptions::default(),
            &flag,
            &mut |_| {},
        );
        (done, started.elapsed())
    })
    .await
    .map_err(|e| format!("接続を試せません：{e}"))?;
    let (done, elapsed) = result;
    let done = done.map_err(|e| e.to_string())?;
    json_response(&json!({
        "text": done.text,
        "usage": to_json(&done.usage, "トークン数")?,
        "stop_reason": done.stop_reason,
        "thinking_chars": done.thinking_chars,
        "elapsed_ms": u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
    }))
}

/// 走る回の取り消しの旗を登録する（外れるときに消す）。始まる前に届いた取り消しも拾う。
fn register_ai_run(
    state: &AppState,
    run: u64,
) -> Result<(Arc<AtomicBool>, AiRunGuard<'_>), String> {
    let cancel = Arc::new(AtomicBool::new(false));
    let mut slot = locked(&state.ai_cancel)?;
    if let Some(at) = slot.early.iter().position(|r| *r == run) {
        slot.early.remove(at);
        cancel.store(true, Ordering::Relaxed);
    }
    slot.running.push((run, Arc::clone(&cancel)));
    Ok((cancel, AiRunGuard { state, run }))
}

/// 接続先のキー。キーの要らないローカルの OpenAI 互換は、キーチェーンを読めなくても続ける。
fn key_for(conn: &ai::Connection) -> Result<Option<String>, String> {
    match SystemKeys.get(&conn.endpoint) {
        Ok(key) => Ok(key),
        Err(_) if !conn.cloud && conn.provider == ai::Provider::OpenaiCompatible => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// タスクの接続先（`model`、無ければ既定の接続先）を解決する。
fn task_connection(
    project: &Project,
    machine: &MachineSettings,
    task: &ai::AiTask,
) -> Result<ai::Connection, String> {
    ai::task_connection(&project.settings.ai, machine, task)
}

/// AI実行の画面のタスクの一覧（T3-7）：定義と、接続先の様子、実行できるか。
#[tauri::command]
fn ai_tasks(state: State<'_, AppState>, app: AppHandle) -> Result<Response, String> {
    let project = {
        let mut guard = locked(&state.workspace)?;
        open_workspace(&mut guard)?.project().clone()
    };
    let (machine, _) = ai::read_machine_settings(&ai_settings_file(&app)?);
    let tasks: Vec<Value> = project
        .ai_tasks
        .iter()
        .map(|task| {
            let conn = task_connection(&project, &machine, task);
            let source = ai::task_connection_name(&project.settings.ai, &machine, task)
                .map(|(_, source)| source);
            json!({
                "task": task,
                // どこで決めた接続先か（この端末の上書き・定義・既定。T4-1）。
                "source": source,
                "override": machine.tasks.get(&task.id).and_then(|t| t.model.clone()),
                "connection": conn.as_ref().ok().map(|c| json!({
                    "name": c.name,
                    "model": c.model,
                    // 人がクラウドへ送ることを確かめる単位（行き先とモデル。T4-1）。
                    "destination": c.destination(),
                    "cloud": c.cloud,
                    "allowed": c.allowed,
                    "context_tokens": c.context_tokens,
                })),
                "connection_problem": conn.err(),
                // 自律は章を選んで `ai_auto` で実行する（T3-9）。
                "unsupported": if task.level == ai::TaskLevel::Auto {
                    None
                } else {
                    ai::runnable(task).err()
                },
            })
        })
        .collect();
    // タスクに選べる接続先（T4-1）。
    let connections: Vec<Value> = project
        .settings
        .ai
        .iter()
        .map(|decl| {
            let conn = ai::resolve(decl, &machine);
            json!({
                "name": decl.name,
                "cloud": conn.as_ref().ok().map(|c| c.cloud),
            })
        })
        .collect();
    json_response(&json!({ "tasks": tasks, "connections": connections }))
}

/// この端末で、タスクに使う接続先を選ぶ（T4-1）。`connection` が無ければ定義のまま。
#[tauri::command]
fn set_ai_task_connection(
    task: String,
    connection: Option<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    let project = {
        let mut guard = locked(&state.workspace)?;
        open_workspace(&mut guard)?.project().clone()
    };
    if !project.ai_tasks.iter().any(|t| t.id == task) {
        return Err(format!("ai/{task}.yaml がありません"));
    }
    if let Some(name) = connection
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        && !project.settings.ai.iter().any(|d| d.name == name)
    {
        return Err(format!("接続先「{name}」が project.toml にありません"));
    }
    update_ai_settings(&app, |m| {
        m.set_task_connection(&task, connection.as_deref())
    })
}

/// タスクの定義（ルートの骨格を作るタスクでは、対象のルートを依頼に書き足したもの。T3-10）。
fn task_for(project: &Project, task: &str, route: Option<&str>) -> Result<ai::AiTask, String> {
    let definition = project
        .ai_tasks
        .iter()
        .find(|t| t.id == task)
        .ok_or_else(|| format!("ai/{task}.yaml がありません"))?;
    Ok(match route.map(str::trim).filter(|r| !r.is_empty()) {
        Some(route) if ai::is_skeleton(definition) => ai::with_route(definition, route),
        // 設定の下書きでは、対象のエントリのパス（T4-5）。
        Some(path) if definition.output == ai::TaskOutput::Entries => {
            ai::with_entry_target(definition, project, path)?
        }
        _ => definition.clone(),
    })
}

/// 実行する前に、組み立てた文脈（送る中身と見積もり）を見る。保存していない本文も含む。
#[tauri::command(async)]
fn ai_prepare(
    task: String,
    scene: Option<String>,
    route: Option<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Response, String> {
    let (project, root) = {
        let mut guard = locked(&state.workspace)?;
        let workspace = open_workspace(&mut guard)?;
        (workspace.project().clone(), workspace.root().to_owned())
    };
    let (machine, _) = ai::read_machine_settings(&ai_settings_file(&app)?);
    let definition = task_for(&project, &task, route.as_deref())?;
    let definition = &definition;
    let conn = task_connection(&project, &machine, definition)?;
    let sources = ai::read_sources(&root);
    let assembled = thread::scope(|scope| {
        thread::Builder::new()
            .name("ai-context".into())
            .stack_size(ANALYZE_STACK_SIZE)
            .spawn_scoped(scope, || {
                let ir = analyze(&project);
                ai::assemble(
                    &project,
                    &ir,
                    &sources,
                    definition,
                    scene.as_deref(),
                    conn.budget(),
                )
            })
            .map_err(|e| format!("解析のスレッドを作れません：{e}"))?
            .join()
            .map_err(|_| "文脈の組み立てが途中で止まりました".to_owned())
    })?;
    json_response(&json!({
        "digest": ai::content_digest(&conn, &assembled),
        "assembled": to_json(&assembled, "文脈")?,
        "connection": {
            "name": conn.name,
            "model": conn.model,
            "cloud": conn.cloud,
            "allowed": conn.allowed,
            // 構造化出力（返す形）を付けて送るか（T4-2）。
            "structured": conn.structured_output && ai::output_schema(definition.output).is_some(),
        },
    }))
}

/// AIタスクを実行する（T3-7。今は指摘の段階）。届いた文字の数を `ai-run-progress` で知らせる。シーン要約の
/// 結果は `.cache/summaries.json` に入れる。`run` は画面が付けた回の番号（取り消しに使う）。`digest` は人が
/// 確かめた送る中身の要約値（`ai_prepare`）。クラウドの接続先では、組み立て直した中身と同じときだけ送る。
#[tauri::command]
async fn ai_run(
    task: String,
    scene: Option<String>,
    route: Option<String>,
    run: u64,
    digest: Option<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Response, String> {
    let (cancel, _registered) = register_ai_run(&state, run)?;
    let (project, root) = {
        let mut guard = locked(&state.workspace)?;
        let workspace = open_workspace(&mut guard)?;
        (workspace.project().clone(), workspace.root().to_owned())
    };
    let (machine, _) = ai::read_machine_settings(&ai_settings_file(&app)?);
    let definition = task_for(&project, &task, route.as_deref())?;
    let conn = task_connection(&project, &machine, &definition)?;
    let key = key_for(&conn)?;
    let emitter = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        thread::Builder::new()
            .name("ai-run".into())
            .stack_size(ANALYZE_STACK_SIZE)
            .spawn(move || {
                let ir = analyze(&project);
                let sources = ai::read_sources(&root);
                let mut received = 0usize;
                let mut last = std::time::Instant::now();
                let mut thinking = false;
                let outcome = ai::run_task(
                    &project,
                    &ir,
                    &sources,
                    &definition,
                    scene.as_deref(),
                    &conn,
                    key.as_deref(),
                    digest.as_deref(),
                    &ai::CallOptions::default(),
                    &cancel,
                    &mut |r| {
                        received += r.text().chars().count();
                        // 考えている間か（T4-2）。変わったときはすぐ知らせる。
                        let changed = thinking != r.thinking();
                        thinking = r.thinking();
                        if changed || last.elapsed() >= Duration::from_millis(200) {
                            last = std::time::Instant::now();
                            let _ = emitter.emit_to(
                                "main",
                                "ai-run-progress",
                                json!({ "run": run, "chars": received, "thinking": thinking }),
                            );
                        }
                    },
                );
                // シーンの要約は、要約を作った本文のハッシュと一緒にキャッシュへ。取り消した回は書かない。
                // 実行の間にほかの回が書いたかもしれないので、キャッシュを読み直してから足す。
                let mut stored = None;
                if let Ok(outcome) = &outcome
                    && !cancel.load(Ordering::Relaxed)
                    && let (Some(summary), Some(scene)) = (&outcome.summary, &outcome.scene)
                    && let Some(source) = ai::scene_source(&project, &ir, scene)
                {
                    let mut fresh = ai::read_sources(&root).summaries;
                    stored = Some(ai::store_summary(&root, &mut fresh, scene, source, summary));
                }
                (outcome, stored)
            })
            .map_err(|e| format!("実行のスレッドを作れません：{e}"))?
            .join()
            .map_err(|_| "AIタスクの実行が途中で止まりました".to_owned())
    })
    .await
    .map_err(|e| format!("AIタスクを実行できません：{e}"))??;
    let (outcome, stored) = result;
    let outcome = outcome.map_err(|e| e.to_string())?;
    let mut value = to_json(&outcome, "実行の結果")?;
    if let Value::Object(map) = &mut value {
        map.insert(
            "summary_problem".to_owned(),
            json!(stored.and_then(Result::err)),
        );
    }
    json_response(&value)
}

/// 外部のAIエージェントが置いた提案（`ai/proposals/*.md`。T3-8）。
#[tauri::command]
fn proposal_files(state: State<'_, AppState>) -> Result<Response, String> {
    let root = {
        let mut guard = locked(&state.workspace)?;
        open_workspace(&mut guard)?.root().to_owned()
    };
    json_response(&to_json(&ai::read_proposal_files(&root), "提案")?)
}

/// 提案を今の本文（保存していない本文も）と照らす：ファイル、シーン、今の文、採用できるか、差分。
#[tauri::command(async)]
fn view_proposals(
    proposals: Vec<ai::Proposal>,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let project = {
        let mut guard = locked(&state.workspace)?;
        open_workspace(&mut guard)?.project().clone()
    };
    let views = thread::scope(|scope| {
        thread::Builder::new()
            .name("proposals".into())
            .stack_size(ANALYZE_STACK_SIZE)
            .spawn_scoped(scope, || {
                let ir = analyze(&project);
                ai::view_proposals(&project, &ir, &proposals)
            })
            .map_err(|e| format!("解析のスレッドを作れません：{e}"))?
            .join()
            .map_err(|_| "提案の照らし合わせが途中で止まりました".to_owned())
    })?;
    json_response(&to_json(&views, "提案")?)
}

/// 提案を採用する（人の操作。T3-8）：`text` を今の本文にして、採用できる行だけを置き換えて保存し、採用した行の
/// 出自を `ai` として書き足す。`disk` はフロントが知っているディスクの中身（外部の変更を見分ける）。
#[tauri::command]
fn adopt_proposals(
    file: String,
    text: String,
    disk: Option<String>,
    proposals: Vec<ai::Proposal>,
    task: String,
    model: String,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    refuse_during_auto(&state, workspace.root(), Some(&file))?;
    if !workspace.set_buffer(&file, &text) {
        return Err(format!("作品フォルダに無いファイルです：{file}"));
    }
    let adopted = {
        let mut generator = locked(&state.generator)?;
        let generator = generator.get_or_insert_with(LineIdGenerator::new);
        workspace
            .adopt_proposals(&file, &proposals, &task, &model, disk.as_deref(), generator)
            .map_err(|e| e.to_string())?
    };
    let skipped: Vec<Value> = adopted
        .skipped
        .iter()
        .map(|(line, reason)| json!({ "line": line, "reason": reason }))
        .collect();
    if let Some(deleted) = adopted.conflict {
        return json_response(
            &json!({ "kind": "conflict", "deleted": deleted, "skipped": skipped }),
        );
    }
    let Some(saved) = adopted.saved else {
        return json_response(&json!({ "kind": "none", "skipped": skipped }));
    };
    let Analyzed {
        summary, checks, ..
    } = analyze_summary(workspace.project(), true, None)?;
    json_response(&json!({
        "kind": "saved",
        "text": saved.text,
        "applied": adopted.applied,
        "skipped": skipped,
        "provenance_problem": saved.provenance_problem,
        "summary": summary,
        "checks": checks,
    }))
}

/// 生成の下書きをスタブのシーンに書き込む（T3-9）：`text` を今の本文にして、本文の行を差し込み、`ai-draft` にして
/// 保存し、差し込んだ行の出自を `ai` として書き足す。`disk` はフロントが知っているディスクの中身。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn write_ai_draft(
    file: String,
    scene: String,
    body: Vec<String>,
    task: String,
    model: String,
    text: String,
    disk: Option<String>,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    refuse_during_auto(&state, workspace.root(), Some(&file))?;
    if !workspace.set_buffer(&file, &text) {
        return Err(format!("作品フォルダに無いファイルです：{file}"));
    }
    let written = {
        let mut generator = locked(&state.generator)?;
        let generator = generator.get_or_insert_with(LineIdGenerator::new);
        workspace
            .write_draft(
                &file,
                &scene,
                &body,
                &task,
                &model,
                disk.as_deref(),
                generator,
            )
            .map_err(|e| e.to_string())?
    };
    if let Some(reason) = written.refused {
        return json_response(&json!({ "kind": "refused", "reason": reason }));
    }
    if let Some(deleted) = written.conflict {
        return json_response(&json!({ "kind": "conflict", "deleted": deleted }));
    }
    let Some(saved) = written.saved else {
        return json_response(&json!({ "kind": "refused", "reason": "書き込めませんでした" }));
    };
    let Analyzed {
        summary, checks, ..
    } = analyze_summary(workspace.project(), true, None)?;
    json_response(&json!({
        "kind": "saved",
        "text": saved.text,
        "ids": written.ids,
        "provenance_problem": saved.provenance_problem,
        "summary": summary,
        "checks": checks,
    }))
}

/// AI が書いた結果（T3-10）を、フロントの形にする：保存した本文、行ID、書いた後の検証、表示に要る分。
fn plot_response(
    workspace: &Workspace,
    written: &scn_core::workspace::PlotWritten,
) -> Result<Response, String> {
    if let Some(reason) = &written.written.refused {
        return json_response(&json!({ "kind": "refused", "reason": reason }));
    }
    if let Some(deleted) = written.written.conflict {
        return json_response(&json!({ "kind": "conflict", "deleted": deleted }));
    }
    let Some(saved) = &written.written.saved else {
        return json_response(&json!({ "kind": "refused", "reason": "書けませんでした" }));
    };
    let Analyzed {
        summary, checks, ..
    } = analyze_summary(workspace.project(), true, None)?;
    json_response(&json!({
        "kind": "saved",
        "file": written.file,
        "text": saved.text,
        "ids": written.written.ids,
        "verification": written.checks,
        "provenance_problem": saved.provenance_problem,
        "summary": summary,
        "checks": checks,
    }))
}

/// 提案ノードを採用する（人の操作。T3-10）：スタブを足し、選択肢を書き足して保存し、出自を記録して検証する。
#[tauri::command]
fn adopt_branch_proposal(
    proposal: ai::BranchProposal,
    task: String,
    model: String,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    let file = analyze(workspace.project())
        .scenes
        .iter()
        .find(|s| s.id == proposal.from)
        .map(|s| s.file.clone());
    refuse_during_auto(&state, workspace.root(), file.as_deref())?;
    let written = {
        let mut generator = locked(&state.generator)?;
        let generator = generator.get_or_insert_with(LineIdGenerator::new);
        workspace
            .adopt_branch(&proposal, &task, &model, generator)
            .map_err(|e| e.to_string())?
    };
    plot_response(workspace, &written)
}

/// ルートの骨格を書く（T3-10）：`file` の章（`new_chapter` なら、その名前の新しい章を作って）の末尾に、スタブの
/// シーンを足して保存する。新しい章に書けなければ、作った章を消す。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn write_ai_skeleton(
    file: Option<String>,
    new_chapter: Option<String>,
    scenes: Vec<ai::SkeletonScene>,
    route: Option<String>,
    task: String,
    model: String,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    refuse_during_auto(&state, workspace.root(), file.as_deref())?;
    let (file, created) = match (file, new_chapter) {
        (_, Some(name)) => (
            workspace.add_chapter(&name).map_err(|e| e.to_string())?,
            true,
        ),
        (Some(file), None) => (file, false),
        (None, None) => return Err("書く章を選びます".to_owned()),
    };
    let route = route.as_deref().map(str::trim).filter(|r| !r.is_empty());
    let written = {
        let mut generator = locked(&state.generator)?;
        let generator = generator.get_or_insert_with(LineIdGenerator::new);
        workspace.write_skeleton(&file, &scenes, route, &task, &model, generator)
    };
    // 新しい章に書けなかったら（断った・入出力の誤り）、作った章を消す。
    if created && !written.as_ref().is_ok_and(|w| w.written.saved.is_some()) {
        let _ = workspace.delete_chapter(&file);
    }
    plot_response(workspace, &written.map_err(|e| e.to_string())?)
}

/// 要約のずれ（T3-10）：シーン要約（`.cache/summaries.json` の、本文の変わっていないもの）とメタ情報の summary が
/// 違うシーン。
#[tauri::command(async)]
fn summary_drift(state: State<'_, AppState>) -> Result<Response, String> {
    let (project, root) = {
        let mut guard = locked(&state.workspace)?;
        let workspace = open_workspace(&mut guard)?;
        (workspace.project().clone(), workspace.root().to_owned())
    };
    let cache = ai::read_sources(&root).summaries;
    let ir = analyze(&project);
    // 章ごとの、要約を作り直すシーンの数（まとめて作り直すときに、送る回数として見せる）。
    let pending: BTreeMap<String, usize> = project
        .files
        .iter()
        .map(|f| {
            (
                f.path.clone(),
                ai::summaries_to_refresh(&project, &ir, &cache, Some(&f.path)).len(),
            )
        })
        .collect();
    json_response(&json!({
        "drift": to_json(&ai::summary_drift(&project, &ir, &cache), "要約のずれ")?,
        "pending": pending,
    }))
}

/// シーンのメタ情報の summary を書く（同期のずれの採用。T3-10）：`text` を今の本文にして summary の1行を書き換え、
/// 保存する。書く前のシーンの本文が要約を作った本文と同じなら、書いた後の本文で要約のキャッシュを書き直す（元の
/// 要約のまま。summary を書いただけで、作り直す要約にならないように）。違えば書き直さない（作り直すよう知らせる）。
#[tauri::command]
fn set_scene_summary(
    file: String,
    scene: String,
    summary: String,
    text: String,
    disk: Option<String>,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = locked(&state.workspace)?;
    let workspace = open_workspace(&mut guard)?;
    refuse_during_auto(&state, workspace.root(), Some(&file))?;
    if !workspace.set_buffer(&file, &text) {
        return Err(format!("作品フォルダに無いファイルです：{file}"));
    }
    let root = workspace.root().to_owned();
    // 書く前の本文で作った要約か（キャッシュの要約と、その本文のハッシュ）。
    let fresh = ai::fresh_summary(
        workspace.project(),
        &analyze(workspace.project()),
        &ai::read_sources(&root).summaries,
        &scene,
    );
    let written = {
        let mut generator = locked(&state.generator)?;
        let generator = generator.get_or_insert_with(LineIdGenerator::new);
        workspace
            .set_scene_summary(&file, &scene, &summary, disk.as_deref(), generator)
            .map_err(|e| e.to_string())?
    };
    if let Some(reason) = written.refused {
        return json_response(&json!({ "kind": "refused", "reason": reason }));
    }
    if let Some(deleted) = written.conflict {
        return json_response(&json!({ "kind": "conflict", "deleted": deleted }));
    }
    let Some(saved) = written.saved else {
        return json_response(&json!({ "kind": "refused", "reason": "書けませんでした" }));
    };
    let ir = analyze(workspace.project());
    let cache_problem = match (fresh, ai::scene_source(workspace.project(), &ir, &scene)) {
        (Some(cached), Some(source)) => {
            let mut cache = ai::read_sources(&root).summaries;
            ai::store_summary(&root, &mut cache, &scene, source, &cached).err()
        }
        _ => Some(
            "要約を作った後に本文が変わっていたので、要約のキャッシュは書き直しません（要約を作り直します）"
                .to_owned(),
        ),
    };
    let Analyzed {
        summary, checks, ..
    } = analyze_summary(workspace.project(), true, None)?;
    json_response(&json!({
        "kind": "saved",
        "text": saved.text,
        "summary": summary,
        "checks": checks,
        "cache_problem": cache_problem,
    }))
}

/// 章のシーンの要約をまとめて作り直す（同期。T3-10）：要約のキャッシュが無いか、本文が変わったシーンを本文の順に、
/// シーン要約のタスク（`task`）で要約してキャッシュに入れる。シーンごとに `ai-summarize-progress` で知らせる。
/// クラウドの接続先では、`confirmed_cloud`（人が送る中身を確かめた送り先。`Connection::destination`）に、その
/// 送り先が要る（確かめた後に接続先を選び直していれば断る。T4-1）。取り消しはシーンの合間で効く。
#[tauri::command]
async fn ai_summarize(
    task: String,
    file: String,
    run: u64,
    confirmed_cloud: Vec<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Response, String> {
    let (cancel, _registered) = register_ai_run(&state, run)?;
    let (project, root) = {
        let mut guard = locked(&state.workspace)?;
        let workspace = open_workspace(&mut guard)?;
        (workspace.project().clone(), workspace.root().to_owned())
    };
    let (machine, _) = ai::read_machine_settings(&ai_settings_file(&app)?);
    let definition = task_for(&project, &task, None)?;
    if definition.output != ai::TaskOutput::Summary {
        return Err(format!(
            "{} はシーン要約のタスクではありません",
            definition.name
        ));
    }
    let conn = task_connection(&project, &machine, &definition)?;
    if conn.cloud && !confirmed_cloud.contains(&conn.destination()) {
        return Err(format!(
            "{} はクラウド（{}）へ送ります。送る中身を確かめてから実行します",
            definition.name, conn.name
        ));
    }
    let key = key_for(&conn)?;
    let emitter = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        thread::Builder::new()
            .name("ai-summarize".into())
            .stack_size(ANALYZE_STACK_SIZE)
            .spawn(move || {
                let ir = analyze(&project);
                let sources = ai::read_sources(&root);
                let targets =
                    ai::summaries_to_refresh(&project, &ir, &sources.summaries, Some(&file));
                let total = targets.len();
                let mut done = Vec::new();
                let mut problems = Vec::new();
                let mut stopped = None;
                for (i, scene) in targets.iter().enumerate() {
                    if cancel.load(Ordering::Relaxed) {
                        stopped = Some("取り消しました".to_owned());
                        break;
                    }
                    let _ = emitter.emit_to(
                        "main",
                        "ai-summarize-progress",
                        json!({ "run": run, "scene": scene, "done": i, "total": total }),
                    );
                    // クラウドでは、人が確かめた（まとめて送る）ので、組み立てた中身の要約値を添える。
                    let assembled = ai::assemble(
                        &project,
                        &ir,
                        &sources,
                        &definition,
                        Some(scene),
                        conn.budget(),
                    );
                    let digest = ai::content_digest(&conn, &assembled);
                    match ai::run_task(
                        &project,
                        &ir,
                        &sources,
                        &definition,
                        Some(scene),
                        &conn,
                        key.as_deref(),
                        Some(&digest),
                        &ai::CallOptions::default(),
                        &cancel,
                        &mut |_| {},
                    ) {
                        Ok(outcome) if cancel.load(Ordering::Relaxed) => {
                            let _ = outcome;
                            stopped = Some("取り消しました".to_owned());
                            break;
                        }
                        Ok(outcome) => {
                            match (outcome.summary, ai::scene_source(&project, &ir, scene)) {
                                (Some(summary), Some(source)) => {
                                    let mut fresh = ai::read_sources(&root).summaries;
                                    match ai::store_summary(
                                        &root, &mut fresh, scene, source, &summary,
                                    ) {
                                        Ok(()) => done.push(scene.clone()),
                                        Err(e) => problems.push(format!("{scene}：{e}")),
                                    }
                                }
                                _ => problems.push(format!("{scene}：要約を読めませんでした")),
                            }
                        }
                        Err(ai::RunError::Ai(ai::AiError::Cancelled)) => {
                            stopped = Some("取り消しました".to_owned());
                            break;
                        }
                        Err(e) => problems.push(format!("{scene}：{e}")),
                    }
                }
                json!({ "done": done, "problems": problems, "stopped": stopped, "total": total })
            })
            .map_err(|e| format!("実行のスレッドを作れません：{e}"))?
            .join()
            .map_err(|_| "要約の作り直しが途中で止まりました".to_owned())
    })
    .await
    .map_err(|e| format!("要約を作り直せません：{e}"))??;
    json_response(&result)
}

/// 自律の実行を、アプリの作品フォルダにつなぐ（T3-9）。LLM を呼ぶ間はワークスペースをロックせず、書き込むときだけ
/// ロックする。書き込んだ本文と進み具合はフロントへ知らせる。
struct AppAutoHost<'a> {
    app: &'a AppHandle,
    state: &'a AppState,
    root: PathBuf,
    machine: MachineSettings,
    cancel: Arc<AtomicBool>,
    run: u64,
    task: String,
    model: String,
    /// Git のブランチで書いている（書く章に保存していない変更があれば書かない）。
    branch: bool,
    /// 書き込んだファイル（作品フォルダからの相対パス。ブランチのコミットに入れる）。
    written: BTreeSet<String>,
    /// 人がクラウドへ送ることを確かめた送り先（実行の途中で接続先が変わっても、ほかへは送らない。T4-1）。
    confirmed_cloud: Vec<String>,
}

impl AppAutoHost<'_> {
    /// 今の作品フォルダ（実行の途中で別の作品フォルダを開いたら止める）。
    fn workspace<'g>(
        &self,
        guard: &'g mut MutexGuard<'_, Option<Workspace>>,
    ) -> Result<&'g mut Workspace, String> {
        let workspace = open_workspace(guard)?;
        if workspace.root() != self.root {
            return Err("実行の途中で、別の作品フォルダを開きました".to_owned());
        }
        Ok(workspace)
    }
}

impl ai::AutoHost for AppAutoHost<'_> {
    fn project(&mut self) -> Result<Project, String> {
        let mut guard = locked(&self.state.workspace)?;
        Ok(self.workspace(&mut guard)?.project().clone())
    }

    fn sources(&mut self) -> ai::ContextSources {
        ai::read_sources(&self.root)
    }

    fn connection(
        &mut self,
        task: &ai::AiTask,
    ) -> Result<(ai::Connection, Option<String>), String> {
        let project = self.project()?;
        let conn = task_connection(&project, &self.machine, task)?;
        if conn.cloud && !self.confirmed_cloud.contains(&conn.destination()) {
            return Err(format!(
                "{} の接続先（{} · {}）は、実行の前に確かめたクラウドの送り先ではありません（実行の途中で project.toml が変わったかもしれません）。送りません",
                task.name, conn.name, conn.model
            ));
        }
        let key = key_for(&conn)?;
        Ok((conn, key))
    }

    fn complete(
        &mut self,
        conn: &ai::Connection,
        key: Option<&str>,
        request: &ai::ChatRequest,
    ) -> Result<ai::Completion, ai::AiError> {
        ai::complete(
            conn,
            key,
            request,
            &ai::CallOptions::default(),
            &self.cancel,
            &mut |_| {},
        )
    }

    fn write(&mut self, file: &str, scene: &str, body: &[String]) -> Result<Vec<String>, String> {
        let mut guard = locked(&self.state.workspace)?;
        let workspace = self.workspace(&mut guard)?;
        if self.branch && workspace.is_dirty(file) {
            return Err(format!(
                "{file} に保存していない変更があるので、ブランチに書き込みませんでした"
            ));
        }
        let seen = workspace.disk_text(file).map(str::to_owned);
        // 書く前の本文（フロントは、これと同じ本文なら置き換える）。
        let before = workspace.text(file).map(str::to_owned);
        let written = {
            let mut generator = locked(&self.state.generator)?;
            let generator = generator.get_or_insert_with(LineIdGenerator::new);
            workspace
                .write_draft(
                    file,
                    scene,
                    body,
                    &self.task,
                    &self.model,
                    seen.as_deref(),
                    generator,
                )
                .map_err(|e| e.to_string())?
        };
        if let Some(reason) = written.refused {
            return Err(reason);
        }
        if written.conflict.is_some() {
            return Err(format!(
                "{file} が外部で変更されていたので、書き込みませんでした"
            ));
        }
        if let Some(saved) = &written.saved {
            self.written.insert(file.to_owned());
            self.written.insert("ai/provenance.jsonl".to_owned());
            let Analyzed {
                summary, checks, ..
            } = analyze_summary(workspace.project(), true, None)?;
            let _ = self.app.emit_to(
                "main",
                "ai-draft-written",
                json!({
                    "file": file,
                    "before": before,
                    "text": saved.text,
                    "provenance_problem": saved.provenance_problem,
                    "summary": summary,
                    "checks": checks,
                }),
            );
        }
        Ok(written.ids)
    }

    fn write_concept(
        &mut self,
        draft: &ai::ConceptDraft,
        task: &str,
        model: &str,
    ) -> Result<scn_core::workspace::FieldsWritten, String> {
        let mut guard = locked(&self.state.workspace)?;
        let workspace = self.workspace(&mut guard)?;
        let written = workspace
            .write_concept_draft(draft, task, model)
            .map_err(|e| e.to_string())?;
        self.written.extend(written.written_paths());
        Ok(written)
    }

    fn write_entries(
        &mut self,
        draft: &ai::EntriesDraft,
        task: &str,
        model: &str,
    ) -> Result<scn_core::workspace::EntriesWritten, String> {
        let mut guard = locked(&self.state.workspace)?;
        let workspace = self.workspace(&mut guard)?;
        let written = workspace
            .write_entries_draft(draft, task, model, None)
            .map_err(|e| e.to_string())?;
        self.written.extend(written.written_paths());
        Ok(written)
    }

    fn event(&mut self, event: ai::AutoEvent) {
        let _ = self.app.emit_to(
            "main",
            "ai-auto-progress",
            json!({ "run": self.run, "event": event }),
        );
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// 自律で章のスタブを下書きにする（T3-9）。`branch` なら、Git のブランチ（`ai/<タスク>-<日時>`）を作って書き、
/// コミットして基のブランチへ戻る（人が差分を見て取り込む）。保存していない変更、コミットしていない変更があれば
/// 始めない。クラウドの接続先を使うときは、`confirmed_cloud`（人が送る中身を確かめた送り先）に、その送り先が要る。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn ai_auto(
    task: String,
    file: String,
    run: u64,
    branch: bool,
    confirmed_cloud: Vec<String>,
    stages: Option<ai::AutoStages>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Response, String> {
    let (cancel, _registered) = register_ai_run(&state, run)?;
    if locked(&state.auto)?.is_some() {
        return Err("ほかの自律の実行が走っています。終わってから実行します".to_owned());
    }
    let (project, root, unsaved) = {
        let mut guard = locked(&state.workspace)?;
        let workspace = open_workspace(&mut guard)?;
        let unsaved: Vec<String> = workspace
            .project()
            .files
            .iter()
            .filter(|f| workspace.is_dirty(&f.path))
            .map(|f| f.path.clone())
            .collect();
        (
            workspace.project().clone(),
            workspace.root().to_owned(),
            unsaved,
        )
    };
    if !unsaved.is_empty() {
        return Err(format!(
            "保存していない変更があります（{}）。保存してから自律で実行します",
            unsaved.join("、")
        ));
    }
    let (machine, _) = ai::read_machine_settings(&ai_settings_file(&app)?);
    let definition = project
        .ai_tasks
        .iter()
        .find(|t| t.id == task)
        .cloned()
        .ok_or_else(|| format!("ai/{task}.yaml がありません"))?;
    if definition.level != ai::TaskLevel::Auto {
        return Err(format!("{} は自律のタスクではありません", definition.name));
    }
    let conn = task_connection(&project, &machine, &definition)?;
    // 企画と設定の段（T4-6）。
    let stages = stages.unwrap_or_default();
    let stage_tasks: Vec<&str> = [
        (stages.concept, ai::CONCEPT_STAGE_TASK),
        (
            stages.entries || stages.entries_after,
            ai::ENTRIES_STAGE_TASK,
        ),
    ]
    .into_iter()
    .filter(|(on, _)| *on)
    .map(|(_, id)| id)
    .collect();
    // クラウドへ送るタスク（自律と、点検のタスクと、段のタスク）は、人が確かめてから。
    let mut involved = vec![&definition];
    involved.extend(
        project
            .ai_tasks
            .iter()
            .filter(|t| definition.checks.contains(&t.id) || stage_tasks.contains(&t.id.as_str())),
    );
    for t in involved {
        let c = task_connection(&project, &machine, t)?;
        if c.cloud && !confirmed_cloud.contains(&c.destination()) {
            return Err(format!(
                "{} はクラウド（{}）へ送ります。送る中身を確かめてから実行します",
                t.name, c.name
            ));
        }
    }
    let mut branch_info: Option<(String, String)> = None;
    if branch {
        if !scn_core::gitops::is_repo(&root) {
            return Err(
                "作品フォルダが Git の作業ツリーではありません。ブランチを使わずに実行します（「ブランチで実行」を外す）"
                    .to_owned(),
            );
        }
        let dirty = scn_core::gitops::uncommitted(&root)?;
        if !dirty.is_empty() {
            return Err(format!(
                "コミットしていない変更があります（{}件）。コミットしてから、ブランチで実行します",
                dirty.len()
            ));
        }
        let base = scn_core::gitops::current_branch(&root)?;
        if base == "HEAD" {
            return Err(
                "Git がブランチの上にありません（detached HEAD）。ブランチに切り替えてから実行します"
                    .to_owned(),
            );
        }
        let template = definition
            .branch
            .clone()
            .unwrap_or_else(|| "ai/{task}-{date}".to_owned());
        let name = scn_core::gitops::branch_name(
            &root,
            &template,
            &definition.id,
            &scn_core::gitops::date_stamp(),
        );
        branch_info = Some((base, name));
    }
    // 走っている間は、書く章（ブランチなら、どの章も）を人の保存から守る。
    {
        let mut auto = locked(&state.auto)?;
        if auto.is_some() {
            return Err("ほかの自律の実行が走っています。終わってから実行します".to_owned());
        }
        *auto = Some(AutoTarget {
            root: root.clone(),
            file: file.clone(),
            branch: branch_info.is_some(),
            settings: !stage_tasks.is_empty(),
        });
    }
    let _auto = AutoGuard { state: &state };
    if let Some((_, name)) = &branch_info {
        scn_core::gitops::start_branch(&root, name)?;
    }
    let all = project.ai_tasks.clone();
    let host_app = app.clone();
    let task_id = definition.id.clone();
    let model = conn.model.clone();
    let root_for_host = root.clone();
    let in_branch = branch_info.is_some();
    let (outcome, written) = tauri::async_runtime::spawn_blocking(move || {
        let state = host_app.state::<AppState>();
        let mut host = AppAutoHost {
            app: &host_app,
            state: &state,
            root: root_for_host,
            machine,
            cancel,
            run,
            task: task_id,
            model,
            branch: in_branch,
            written: BTreeSet::new(),
            confirmed_cloud,
        };
        let outcome = ai::run_auto(&mut host, &definition, &file, &all, &stages);
        (outcome, host.written)
    })
    .await
    .map_err(|e| format!("自律で実行できません：{e}"))?;
    let mut branch_json = Value::Null;
    if let Some((base, name)) = branch_info {
        branch_json = finish_branch(&root, &base, &name, &outcome, &written);
    }
    // 企画と設定の段で書いたら、設定を読み直して、補完の材料・表示に要る分・診断・AI の項目を返す（T4-6）。
    // 読み直せなくても、実行の結果は返す（設定は null。外部の変更の知らせか、次の操作で読み直す）。
    let mut settings_json = Value::Null;
    if outcome.stages.iter().any(|s| s.status == "written") {
        let refreshed = (|| -> Result<Value, String> {
            let mut guard = locked(&state.workspace)?;
            let workspace = open_workspace(&mut guard)?;
            let _ = workspace.reload_settings();
            let Analyzed {
                summary, checks, ..
            } = analyze_summary(workspace.project(), true, None)?;
            Ok(json!({
                "assist": completion_data(workspace.project()),
                "summary": summary,
                "checks": checks,
                "ai_fields": ai_field_list(workspace),
            }))
        })();
        settings_json = refreshed.unwrap_or(Value::Null);
    }
    json_response(&json!({
        "outcome": to_json(&outcome, "自律の結果")?,
        "branch": branch_json,
        "settings": settings_json,
    }))
}

/// 自律の実行の後のブランチ：書き込んだファイル（と出自の記録）だけをコミットして、基のブランチに戻る。何も
/// 書かなければブランチを消す。コミットできなければ、書いた分を失わないように、ブランチにとどまる。
fn finish_branch(
    root: &Path,
    base: &str,
    name: &str,
    outcome: &ai::AutoOutcome,
    written: &BTreeSet<String>,
) -> Value {
    let paths: Vec<String> = written.iter().cloned().collect();
    let committed = scn_core::gitops::commit_paths(
        root,
        &paths,
        &format!("ai: {} の下書き（{}）", outcome.task, outcome.file),
    );
    let (kept, problem) = match &committed {
        Err(e) => (
            true,
            Some(format!(
                "コミットできなかったので、ブランチ {name} にとどまりました（{e}）"
            )),
        ),
        Ok(done) => match scn_core::gitops::switch(root, base) {
            Err(e) => (true, Some(format!("{base} に戻れませんでした（{e}）"))),
            Ok(()) if !done => (
                false,
                scn_core::gitops::delete_branch(root, name)
                    .err()
                    .map(|e| format!("空のブランチ {name} を消せませんでした（{e}）")),
            ),
            Ok(()) => (true, None),
        },
    };
    json!({
        "name": name,
        "base": base,
        "committed": committed.as_ref().ok(),
        "kept": kept,
        "problem": problem,
    })
}

/// AI のブランチ（`ai/` で始まるもの）と、今のブランチ。Git でなければ `repo: false`。
#[tauri::command]
fn ai_branches(state: State<'_, AppState>) -> Result<Response, String> {
    let root = {
        let mut guard = locked(&state.workspace)?;
        open_workspace(&mut guard)?.root().to_owned()
    };
    if !scn_core::gitops::is_repo(&root) {
        return json_response(&json!({ "repo": false, "current": null, "branches": [] }));
    }
    json_response(&json!({
        "repo": true,
        "current": scn_core::gitops::current_branch(&root).ok(),
        "branches": scn_core::gitops::ai_branches(&root)?,
    }))
}

/// AI のブランチと今のブランチの差分（あらましと本文）。
#[tauri::command]
fn ai_branch_diff(name: String, state: State<'_, AppState>) -> Result<Response, String> {
    let root = {
        let mut guard = locked(&state.workspace)?;
        open_workspace(&mut guard)?.root().to_owned()
    };
    let base = scn_core::gitops::current_branch(&root)?;
    let (stat, patch) = scn_core::gitops::diff(&root, &base, &name)?;
    json_response(&json!({ "base": base, "stat": stat, "patch": patch }))
}

/// AI のブランチを今のブランチに取り込む。保存していない変更があれば取り込まない（ファイルが変わるため）。
#[tauri::command]
fn ai_branch_merge(name: String, state: State<'_, AppState>) -> Result<(), String> {
    let root = {
        let mut guard = locked(&state.workspace)?;
        let workspace = open_workspace(&mut guard)?;
        refuse_during_auto(&state, workspace.root(), None)?;
        if workspace
            .project()
            .files
            .iter()
            .any(|f| workspace.is_dirty(&f.path))
        {
            return Err("保存していない変更があります。保存してから取り込みます".to_owned());
        }
        workspace.root().to_owned()
    };
    scn_core::gitops::merge(&root, &name)
}

/// AI のブランチを取り込まずに捨てる。
#[tauri::command]
fn ai_branch_delete(name: String, state: State<'_, AppState>) -> Result<(), String> {
    let root = {
        let mut guard = locked(&state.workspace)?;
        open_workspace(&mut guard)?.root().to_owned()
    };
    scn_core::gitops::delete_branch(&root, &name)
}

/// 走っている回（「接続を試す」と AI タスクの実行）を取り消す（次のデータの合間で止まる）。まだ始まっていなければ、
/// 始まったときに止める。
#[tauri::command]
fn cancel_ai(run: u64, state: State<'_, AppState>) -> Result<(), String> {
    /// 覚えておく、始まる前の取り消しの数。
    const EARLY_LIMIT: usize = 16;
    let mut slot = locked(&state.ai_cancel)?;
    if let Some((_, cancel)) = slot.running.iter().find(|(r, _)| *r == run) {
        cancel.store(true, Ordering::Relaxed);
    } else {
        slot.early.push(run);
        let over = slot.early.len().saturating_sub(EARLY_LIMIT);
        slot.early.drain(..over);
    }
    Ok(())
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
            merge_view,
            apply_merge,
            merge_source,
            add_chapter,
            add_stub,
            add_flow_edge,
            symbol_at,
            rename_preview,
            apply_rename,
            rename_chapter,
            delete_chapter,
            route_stats,
            scene_info,
            read_bible,
            entry_usage,
            set_entry_field,
            set_entry_body,
            write_entry_source,
            create_entry,
            type_text,
            edit_type,
            create_type,
            type_templates,
            read_ideas,
            create_idea,
            promote_idea,
            read_concept,
            create_concept,
            read_variables,
            write_variables,
            set_variable_field,
            add_variable,
            preview_data,
            script_sheet,
            playthrough,
            ai_lines,
            export_provenance,
            ai_connections,
            set_ai_override,
            set_ai_task_connection,
            write_concept_draft,
            write_entries_draft,
            adopt_field_proposal,
            set_ai_cloud_allowed,
            set_ai_key,
            delete_ai_key,
            ai_test,
            cancel_ai,
            ai_tasks,
            ai_prepare,
            ai_run,
            proposal_files,
            view_proposals,
            adopt_proposals,
            write_ai_draft,
            adopt_branch_proposal,
            write_ai_skeleton,
            summary_drift,
            set_scene_summary,
            ai_summarize,
            ai_auto,
            ai_branches,
            ai_branch_diff,
            ai_branch_merge,
            ai_branch_delete,
            export_script,
            export_renpy,
            export_tyrano,
            export_utage,
            book_settings,
            export_book,
            export_plugins,
            export_plugin,
            asset_list,
            asset_location,
            reveal_project,
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
    /// Windows の長いパスの印を外す（T3-1、R191）。
    #[test]
    fn plain_paths() {
        assert_eq!(
            plain_path(Path::new(r"\\?\C:\作品\assets")),
            PathBuf::from(r"C:\作品\assets")
        );
        assert_eq!(
            plain_path(Path::new(r"\\?\UNC\server\share\a")),
            PathBuf::from(r"\\server\share\a")
        );
        assert_eq!(
            plain_path(Path::new("/Users/me/assets")),
            PathBuf::from("/Users/me/assets")
        );
    }

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
