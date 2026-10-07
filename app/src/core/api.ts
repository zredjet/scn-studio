// コアの呼び出し（Tauriの invoke）と、Tauriの機能（ダイアログ、知らせ、ウィンドウ）。
// コンポーネントから直接 invoke を呼ばない（.claude/rules/frontend.md）。

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ask, open, save } from "@tauri-apps/plugin-dialog";
import type {
  AddedChapter,
  DevSettings,
  KeptFile,
  OpenedProject,
  RecentProject,
  ReloadedFile,
  SavedFile,
  Summary,
  WorkspaceChanged,
} from "./types.ts";

/** 作品フォルダを開く。ファイルの監視も始まる。 */
export function openProject(path: string): Promise<OpenedProject> {
  return invoke<OpenedProject>("open_project", { path });
}

/** 新しい作品フォルダを作って開く（無いフォルダか、空のフォルダに）。 */
export function createProject(path: string): Promise<OpenedProject> {
  return invoke<OpenedProject>("create_project", { path, title: null });
}

/** 保存していない編集中の本文で、プロジェクト全体を解析し直す。表示に要る分だけを受け取る。 */
export async function parseBuffer(
  file: string,
  text: string,
): Promise<Summary> {
  const result = await invoke<{ summary: Summary }>("parse_buffer", {
    file,
    text,
  });
  return result.summary;
}

/**
 * 本文を保存する。行IDの無い行に行IDが付く（D045）。`disk` は、画面が知っているディスクの中身（外で消されて
 * いれば null）。コアの知っている中身と違えば（外部の変更の知らせの途中）、書かずに衝突になる。
 */
export function saveFile(
  file: string,
  text: string,
  disk: string | null,
): Promise<SavedFile> {
  return invoke<SavedFile>("save_file", { file, text, disk });
}

/** 外部の変更と衝突したファイルを、外の中身で読み込み直す。 */
export function reloadFile(file: string): Promise<ReloadedFile> {
  return invoke<ReloadedFile>("reload_file", { file });
}

/**
 * 外部の変更と衝突したファイルで、自分の変更（`text`）を残す。外で消されていれば、保存で作り直す。
 * ディスクの中身（外の中身）を受け取る。
 */
export function keepMine(file: string, text: string): Promise<KeptFile> {
  return invoke<KeptFile>("keep_mine", { file, text });
}

/** `story/` に章を足す。 */
export function addChapter(name: string): Promise<AddedChapter> {
  return invoke<AddedChapter>("add_chapter", { name });
}

/** 最近開いた作品フォルダ（新しい順）。 */
export function recentProjects(): Promise<RecentProject[]> {
  return invoke<RecentProject[]>("recent_projects");
}

/** 開発用の設定。 */
export function devSettings(): Promise<DevSettings> {
  return invoke<DevSettings>("dev_settings");
}

/** 開発用：測った値をターミナルに書く。 */
export function devReport(text: string): Promise<void> {
  return invoke<void>("dev_report", { text });
}

/** 外部の変更の知らせを受け取る。 */
export function onWorkspaceChanged(
  handler: (event: WorkspaceChanged) => void,
): Promise<UnlistenFn> {
  return listen<WorkspaceChanged>("workspace-changed", (event) =>
    handler(event.payload),
  );
}

/** 外部の変更を取り込めなかった知らせを受け取る。 */
export function onWorkspaceError(
  handler: (message: string) => void,
): Promise<UnlistenFn> {
  return listen<string>("workspace-error", (event) => handler(event.payload));
}

/** 作品フォルダを選ぶ。選ばなければ null。 */
export async function pickProjectFolder(): Promise<string | null> {
  const picked = await open({
    directory: true,
    multiple: false,
    title: "作品フォルダを開く",
  });
  return typeof picked === "string" ? picked : null;
}

/** 新しい作品フォルダの場所と名前を選ぶ。選ばなければ null。 */
export async function pickNewProjectFolder(): Promise<string | null> {
  return save({ title: "新しい作品フォルダ" });
}

/** はい・いいえを聞く（ボタンの名前は日本語にする。OSの言語に合わせると Yes・No になるため）。 */
export function confirm(message: string, title: string): Promise<boolean> {
  return ask(message, {
    title,
    kind: "warning",
    okLabel: "はい",
    cancelLabel: "いいえ",
  });
}

/**
 * ウィンドウを閉じる前に聞く。`shouldClose` が false を返せば閉じない。
 * 片づけの関数を返す。
 */
export function guardClose(
  shouldClose: () => Promise<boolean>,
): Promise<UnlistenFn> {
  return getCurrentWindow().onCloseRequested(async (event) => {
    if (!(await shouldClose())) event.preventDefault();
  });
}
