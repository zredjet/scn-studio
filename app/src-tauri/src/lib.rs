//! scn-studio のデスクトップアプリ（Tauri）。
//!
//! コマンドは薄く保ち、処理は scn-core に置く（`.claude/rules/rust.md`）。ファイルを読むのは
//! コアの `load_project` だけで、フロントには任意のパスを読ませない。保存はフェーズ1で作る。

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread;

use scn_core::{Ir, Project, analyze, load_project};
use serde::Serialize;
use tauri::State;
use tauri::ipc::Response;

/// 解析を走らせるスレッドのスタックの大きさ。ブロックの入れ子が深い本文でもあふれないよう、
/// メインスレッド（Windowsでは1MB）より大きくする。
const ANALYZE_STACK_SIZE: usize = 64 * 1024 * 1024;

/// 大きなスタックのスレッドで、プロジェクトを解析し、結果をJSONにする。
///
/// 入れ子の深いIRは、直列化と破棄も深く再帰する。IRをそのまま返すと、Tauriがコマンドの
/// スレッドで直列化するので、直列化と破棄もこのスレッドの中で行い、JSONの文字列だけを返す。
fn analyze_to_json<F>(project: Project, to_json: F) -> Result<(Project, String), String>
where
    F: FnOnce(&Project, Ir) -> serde_json::Result<String> + Send + 'static,
{
    let (project, json) = thread::Builder::new()
        .name("analyze".into())
        .stack_size(ANALYZE_STACK_SIZE)
        .spawn(move || {
            let ir = analyze(&project);
            let json = to_json(&project, ir);
            (project, json)
        })
        .map_err(|e| format!("解析のスレッドを作れません：{e}"))?
        .join()
        .map_err(|_| "解析が途中で止まりました".to_owned())?;
    let json = json.map_err(|e| format!("解析の結果をJSONにできません：{e}"))?;
    Ok((project, json))
}

/// 開いているプロジェクト。編集中で保存していない本文も、ここに持つ。
#[derive(Default)]
struct AppState {
    project: Mutex<Option<Project>>,
}

/// 本文のファイル。
#[derive(Serialize)]
struct FileText<'a> {
    /// プロジェクトの根からの相対パス（`story/chapter01.scn`）。
    path: &'a str,
    text: &'a str,
}

/// プロジェクトを開いた結果。フロントの型は `app/src/core/types.ts` の `OpenedProject`。
#[derive(Serialize)]
struct OpenedProject<'a> {
    root: String,
    files: Vec<FileText<'a>>,
    /// 台詞の括弧（`project.toml` の `[dialogue.brackets]`）。ハイライトに使う。
    brackets: Vec<Bracket>,
    ir: Ir,
}

#[derive(Serialize)]
struct Bracket {
    open: String,
    close: String,
}

/// プロジェクトを開き、本文とIR（診断を含む）を返す。
#[tauri::command]
fn open_project(path: String, state: State<'_, AppState>) -> Result<Response, String> {
    let root = PathBuf::from(&path);
    let project = load_project(&root).map_err(|e| e.to_string())?;
    let root = root.display().to_string();
    let (project, json) = analyze_to_json(project, move |project, ir| {
        serde_json::to_string(&OpenedProject {
            root,
            files: project
                .files
                .iter()
                .map(|f| FileText {
                    path: &f.path,
                    text: &f.text,
                })
                .collect(),
            brackets: project
                .settings
                .brackets
                .iter()
                .map(|b| Bracket {
                    open: b.open.to_string(),
                    close: b.close.to_string(),
                })
                .collect(),
            ir,
        })
    })?;
    *state
        .project
        .lock()
        .map_err(|_| "状態を読めません".to_owned())? = Some(project);
    Ok(Response::new(json))
}

/// 保存していない編集中の本文で、プロジェクト全体を解析し直し、IRを返す。
/// 本文は状態に残すので、別のファイルを編集しても、それまでの編集は解析に含まれる。
#[tauri::command]
fn parse_buffer(
    file: String,
    text: String,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let mut guard = state
        .project
        .lock()
        .map_err(|_| "状態を読めません".to_owned())?;
    let mut project = guard
        .take()
        .ok_or_else(|| "プロジェクトが開かれていません".to_owned())?;
    // 本文として読むのは story/*.scn だけ（仕様書17節）。開いたときに無かったファイルは受け付けない。
    let Some(existing) = project.files.iter_mut().find(|f| f.path == file) else {
        *guard = Some(project);
        return Err(format!("プロジェクトに無いファイルです：{file}"));
    };
    existing.text = text;
    let (project, json) = analyze_to_json(project, |_, ir| serde_json::to_string(&ir))?;
    *guard = Some(project);
    Ok(Response::new(json))
}

/// 開発用：起動時に開くサンプル作品（`samples/houkago`）のパス。リリースビルドでは `None`。
#[tauri::command]
fn dev_project_path() -> Option<String> {
    if cfg!(debug_assertions) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago");
        path.canonicalize().ok().map(|p| p.display().to_string())
    } else {
        None
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            open_project,
            parse_buffer,
            dev_project_path
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
        let (_, json) = thread::Builder::new()
            .stack_size(1024 * 1024)
            .spawn(move || analyze_to_json(project, |_, ir| serde_json::to_string(&ir)))
            .unwrap()
            .join()
            .unwrap()
            .unwrap();
        assert!(json.contains("\"ir_version\":1"));
    }
}
