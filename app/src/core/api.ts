// コアの呼び出し（Tauriの invoke）と、Tauriの機能（ダイアログ、知らせ、ウィンドウ）。
// コンポーネントから直接 invoke を呼ばない（.claude/rules/frontend.md）。

import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ask, open, save } from "@tauri-apps/plugin-dialog";
import type { PreviewData } from "./ir.ts";
import type {
  AddedChapter,
  AssetKey,
  AssetKind,
  AssetList,
  BibleData,
  BibleWritten,
  CatalogWritten,
  DevSettings,
  EntryUsage,
  GraphLevel,
  JsonValue,
  KeptFile,
  LoadedLayout,
  OpenedProject,
  RecentProject,
  ReloadedFile,
  SavedFile,
  SceneGraphData,
  SceneInfo,
  ScriptSheet,
  Summary,
  VariablesData,
  VariablesWritten,
  ViewMode,
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

/**
 * 保存していない編集中の本文で、プロジェクト全体を解析し直す。表示に要る分だけを受け取る。
 * `scene` を渡すと、そのシーンの情報（文脈パネル）も受け取る。
 */
export function parseBuffer(
  file: string,
  text: string,
  scene: string | null = null,
): Promise<{ summary: Summary; scene: SceneInfo | null }> {
  return invoke<{ summary: Summary; scene: SceneInfo | null }>("parse_buffer", {
    file,
    text,
    scene,
  });
}

/** シーンの情報（文脈パネル）。 */
export async function sceneInfo(scene: string): Promise<SceneInfo | null> {
  const result = await invoke<{ scene: SceneInfo | null }>("scene_info", {
    scene,
  });
  return result.scene;
}

// ---- 設定DBと変数（T1-10、D049） ----

/** 設定DBの中身。 */
export function readBible(): Promise<BibleData> {
  return invoke<BibleData>("read_bible");
}

/** エントリの逆引き（無いエントリなら null）。 */
export function entryUsage(path: string): Promise<EntryUsage | null> {
  return invoke<EntryUsage | null>("entry_usage", { path });
}

/**
 * エントリの1つの項目を直す（直したキーの値だけを差し替える）。`seen` は画面が知っているファイルの中身。
 * 外でその項目も変わっていれば、書かずに衝突になる。
 */
export function setEntryField(
  path: string,
  key: string,
  value: JsonValue,
  seen: string,
): Promise<BibleWritten> {
  return invoke<BibleWritten>("set_entry_field", { path, key, value, seen });
}

/** エントリの本文（フロントマターの後ろ）を直す。 */
export function setEntryBody(
  path: string,
  body: string,
  seen: string,
): Promise<BibleWritten> {
  return invoke<BibleWritten>("set_entry_body", { path, body, seen });
}

/** エントリのファイル全体を書く（ソース表示）。`seen` が null なら上書きする。 */
export function writeEntrySource(
  path: string,
  text: string,
  seen: string | null,
): Promise<BibleWritten> {
  return invoke<BibleWritten>("write_entry_source", { path, text, seen });
}

/** エントリを新しく作る（未登録の話者の登録も。D047、D049）。 */
export function createEntry(
  typeId: string,
  id: string,
  name: string,
): Promise<BibleWritten> {
  return invoke<BibleWritten>("create_entry", { typeId, id, name });
}

/** 変数の画面の中身。 */
export function readVariables(): Promise<VariablesData> {
  return invoke<VariablesData>("read_variables");
}

/** `variables.yaml` を書く。`seen` が null なら上書きする。 */
export function writeVariables(
  text: string,
  seen: string | null,
): Promise<VariablesWritten> {
  return invoke<VariablesWritten>("write_variables", { text, seen });
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

/** 開いている作品フォルダの、ファイルごとの表示（覚えていないファイルは含まない）。 */
export function viewModes(): Promise<Record<string, ViewMode>> {
  return invoke<Record<string, ViewMode>>("view_modes");
}

/** ファイルの表示を覚える（アプリの設定のフォルダに。作品フォルダには書かない。D046）。 */
export function setViewMode(file: string, mode: ViewMode): Promise<void> {
  return invoke<void>("set_view_mode", { file, mode });
}

/** 構造グラフ（粒度ごと。D048）。 */
export function sceneGraph(level: GraphLevel): Promise<SceneGraphData> {
  return invoke<SceneGraphData>("scene_graph", { level });
}

/** フロー図の配置（`layout.json`）を読む。 */
export function readLayout(): Promise<LoadedLayout> {
  return invoke<LoadedLayout>("read_layout");
}

/**
 * フロー図の配置を書く。`positions` をその粒度の位置に重ねる。`replace` なら、その粒度の位置を
 * 捨ててから書く（自動整列）。
 */
export function writeLayout(
  level: GraphLevel,
  positions: Readonly<Record<string, readonly [number, number]>>,
  replace: boolean,
): Promise<void> {
  return invoke<void>("write_layout", { level, positions, replace });
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

// ---- プレビュー（T1-11、D050） ----

/** プレビューのデータ（IR全体と、画面・変数・素材）。 */
export function previewData(): Promise<PreviewData> {
  return invoke<PreviewData>("preview_data");
}

/** 素材のファイル（絶対パス）を、アセットプロトコルのURLにする（作品の `assets/` の中だけ見せる）。 */
export function assetUrl(path: string): string {
  return convertFileSrc(path);
}

// ---- 台本（T1-12、D051） ----

/** 台本（書き出す前に中身を見る）。 */
export function scriptSheet(): Promise<ScriptSheet> {
  return invoke<ScriptSheet>("script_sheet");
}

/** 台本を作品フォルダの `export/script.csv` に書き出す。 */
export function exportScript(): Promise<{ path: string; rows: number }> {
  return invoke<{ path: string; rows: number }>("export_script");
}

/** 素材の一覧（素材カタログの画面）。 */
export function assetList(): Promise<AssetList> {
  return invoke<AssetList>("asset_list");
}

/** カタログに意味名を足す（仮素材）。 */
export function addCatalogEntries(
  items: readonly AssetKey[],
): Promise<CatalogWritten> {
  return invoke<CatalogWritten>("add_catalog_entries", { items });
}

/** カタログの意味名に、選んだファイルを割り当てる（`assets/` の中だけ。コアで確かめる）。 */
export function assignAssetFile(
  item: AssetKey,
  path: string,
): Promise<CatalogWritten> {
  return invoke<CatalogWritten>("assign_asset_file", { item, path });
}

/** 発注リストを `export/orders.csv` に書き出す。 */
export function exportOrders(): Promise<{ path: string; rows: number }> {
  return invoke<{ path: string; rows: number }>("export_orders");
}

/** 素材のファイルを選ぶ（作品フォルダの `assets/` から始める）。選ばなければ null。 */
export async function pickAssetFile(
  root: string,
  kind: AssetKind,
): Promise<string | null> {
  const separator = root.includes("\\") ? "\\" : "/";
  const image = kind === "background" || kind === "expression";
  const picked = await open({
    directory: false,
    multiple: false,
    title: "素材のファイルを選ぶ（assets/ の中）",
    defaultPath: `${root}${separator}assets`,
    filters: [
      image
        ? { name: "画像", extensions: ["png", "jpg", "jpeg", "webp", "gif"] }
        : {
            name: "音",
            extensions: ["ogg", "opus", "mp3", "wav", "m4a", "flac"],
          },
      { name: "すべてのファイル", extensions: ["*"] },
    ],
  });
  return typeof picked === "string" ? picked : null;
}

/** Ren'Py の書き出しの結果（`export_renpy`）。 */
export interface RenpyResult {
  /** 書き出し先（Ren'Py のプロジェクトのフォルダ）。 */
  readonly path: string;
  readonly files: number;
  /** 知らせ（仮素材、対応の無いトランジション、行き止まり、フォントなど）。 */
  readonly warnings: readonly string[];
  /** 出力プロファイル（export/renpy.yaml）の誤り。読み飛ばして書き出した。 */
  readonly problems: readonly string[];
}

export function exportRenpy(): Promise<RenpyResult> {
  return invoke<RenpyResult>("export_renpy");
}
