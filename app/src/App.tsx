// アプリの骨格（docs/ui/README.md）：タイトルバー、左端のバー、画面（本文・フロー・検証）、ステータスバー。
// 作品フォルダを開く・作る・保存する。外部の変更（Claude Codeなどの書き換え）は、コアがファイルを監視して
// 取り込み、`workspace-changed` で知らせてくる（T1-6、D045）。
// 変換中は画面を変えない（D039）：解析の結果、外部の変更の知らせ、保存の結果で表示が変わるものは、
// 変換が終わってから反映する（whenIdle）。

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AssetsScreen } from "./assets/AssetsScreen.tsx";
import { type BibleRequest, BibleScreen } from "./bible/BibleScreen.tsx";
import { CheckList } from "./check/CheckList.tsx";
import { countBySeverity } from "./check/group.ts";
import { PlaythroughPanel } from "./check/PlaythroughPanel.tsx";
import { ConceptScreen } from "./concept/ConceptScreen.tsx";
import {
  addChapter,
  applyMerge,
  applyRename,
  assetUrl,
  confirm,
  createProject,
  deleteChapter,
  devReport,
  devSettings,
  exportRenpy,
  exportScript,
  exportText,
  focusThisWindow,
  guardClose,
  keepMine,
  onPreviewLine as listenPreviewLine,
  onShowInEditor as listenShowInEditor,
  mergeView,
  onPreviewWindowClosed,
  onWorkspaceChanged,
  onWorkspaceError,
  openPreviewWindow,
  openProject,
  parseBuffer,
  pickNewProjectFolder,
  pickProjectFolder,
  previewData,
  readingChoices,
  recentProjects,
  reloadFile,
  renameChapter,
  renamePreview,
  routeStats,
  runPlaythrough,
  saveFile,
  sceneInfo,
  scriptSheet,
  sendTextChanged,
  setViewMode,
  symbolAt,
  viewModes,
} from "./core/api.ts";
import type { PreviewData } from "./core/ir.ts";
import { PositionMap } from "./core/position.ts";
import type {
  AssistData,
  Bracket,
  EditorSettings,
  GraphLevel,
  IrDiagnostic,
  MergeChoice,
  MergeView,
  OpenedProject,
  Playthrough,
  PreviewRequest,
  RecentProject,
  RenameSymbol,
  RouteStat,
  SceneInfo,
  SceneSummary,
  Summary,
  ViewMode,
  WorkspaceChanged,
} from "./core/types.ts";
import { ConflictBar } from "./editor/ConflictBar.tsx";
import {
  type Analysis,
  Editor,
  type Focus,
  type Replace,
} from "./editor/Editor.tsx";
import { LineBreakBar } from "./editor/LineBreakBar.tsx";
import { MergePanel } from "./editor/MergePanel.tsx";
import { ModeBar } from "./editor/ModeBar.tsx";
import { rememberBase } from "./editor/merge.ts";
import type { Definition } from "./editor/navigate.ts";
import { hasMixedLineBreaks, lineBreakOf } from "./editor/state.ts";
import { ExportScreen } from "./export/ExportScreen.tsx";
import { SceneGraph } from "./graph/SceneGraph.tsx";
import { locateLine } from "./preview/locate.ts";
import { type PreviewLine, PreviewScreen } from "./preview/PreviewScreen.tsx";
import { SceneList } from "./scenes/SceneList.tsx";
import { CommandPalette } from "./shell/CommandPalette.tsx";
import { ContextPanel } from "./shell/ContextPanel.tsx";
import { LeftBar, type Screen } from "./shell/LeftBar.tsx";
import type { Command } from "./shell/palette.ts";
import { QuickOpen } from "./shell/QuickOpen.tsx";
import { RenameDialog } from "./shell/RenameDialog.tsx";
import { StatusBar } from "./shell/StatusBar.tsx";
import {
  fileChars,
  routeStatText,
  sceneAt,
  statOfRoute,
} from "./shell/scene.ts";
import { TitleBar } from "./shell/TitleBar.tsx";
import { Welcome } from "./shell/Welcome.tsx";
import { VariablesScreen } from "./variables/VariablesScreen.tsx";

interface ProjectView {
  readonly root: string;
  readonly title: string;
  readonly brackets: readonly Bracket[];
  readonly summary: Summary;
  /** 検証の段階1の診断。 */
  readonly checks: readonly IrDiagnostic[];
  /** 本文のファイル（`story/` の下の `*.scn`。下のフォルダも）。 */
  readonly files: readonly string[];
}

interface Conflict {
  readonly deleted: boolean;
}

function analysisFor(summary: Summary, file: string, text: string): Analysis {
  return {
    file,
    text,
    diagnostics: summary.diagnostics.filter((d) => d.file === file),
  };
}

function withFile(files: readonly string[], file: string): string[] {
  return files.includes(file) ? [...files] : [...files, file].sort();
}

function withoutFiles(
  set: ReadonlySet<string>,
  files: ReadonlySet<string>,
): ReadonlySet<string> {
  if (![...files].some((f) => set.has(f))) return set;
  return new Set([...set].filter((f) => !files.has(f)));
}

/** 設定DBのエントリか `variables.yaml` か素材カタログか（検証の画面から、その画面を開ける）。 */
function isSettingsFile(file: string): boolean {
  return (
    file.startsWith("bible/entries/") ||
    file === "variables.yaml" ||
    file === "assets/catalog.yaml"
  );
}

function withoutConflict(
  map: ReadonlyMap<string, Conflict>,
  file: string,
): ReadonlyMap<string, Conflict> {
  if (!map.has(file)) return map;
  const next = new Map(map);
  next.delete(file);
  return next;
}

/** 表示の好み（集中モード、タイプライタースクロール、ゴシック体。T2-8）。この端末の WebView に覚える。 */
interface ViewPrefs {
  readonly focus: boolean;
  readonly typewriter: boolean;
  readonly gothic: boolean;
}

const VIEW_PREFS_KEY = "scn-studio.view";

function loadViewPrefs(): ViewPrefs {
  const none = { focus: false, typewriter: false, gothic: false };
  try {
    const raw = localStorage.getItem(VIEW_PREFS_KEY);
    if (raw === null) return none;
    const v = JSON.parse(raw) as Partial<ViewPrefs>;
    return {
      focus: v.focus === true,
      typewriter: v.typewriter === true,
      gothic: v.gothic === true,
    };
  } catch {
    return none;
  }
}

function saveViewPrefs(prefs: ViewPrefs): void {
  try {
    localStorage.setItem(VIEW_PREFS_KEY, JSON.stringify(prefs));
  } catch {
    // 覚えられなくても、今の表示には使える。
  }
}

export function App() {
  const [project, setProject] = useState<ProjectView | null>(null);
  const [screen, setScreen] = useState<Screen>("text");
  /** 自動巡回（検証の段階2。T3-2）の結果と、走らせている途中か。本文が変わったら捨てる。 */
  const [stage2, setStage2] = useState<Playthrough | null>(null);
  const [stage2Running, setStage2Running] = useState(false);
  const [stage2Error, setStage2Error] = useState<string | null>(null);
  const [stage2Discarded, setStage2Discarded] = useState(false);
  /** 走らせた時点の本文（結果を受け取ったときに、本文が変わっていれば捨てる）。 */
  const stage2Summary = useRef<Summary | null>(null);
  const [currentFile, setCurrentFile] = useState<string | null>(null);
  const [analysis, setAnalysis] = useState<Analysis | null>(null);
  const [focus, setFocus] = useState<Focus | null>(null);
  const [replace, setReplace] = useState<Replace | null>(null);
  const [dirty, setDirty] = useState<ReadonlySet<string>>(new Set());
  const [conflicts, setConflicts] = useState<ReadonlyMap<string, Conflict>>(
    new Map(),
  );
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [recent, setRecent] = useState<RecentProject[]>([]);
  /** 補完の材料と、入力補助の設定（D047）。 */
  const [assist, setAssist] = useState<AssistData | null>(null);
  const [editorSettings, setEditorSettings] = useState<EditorSettings>({
    normalize_ellipsis: false,
    indent_narration: false,
  });
  /** カーソルのあるシーンと、その情報（文脈パネル）。 */
  const [currentScene, setCurrentScene] = useState<string | null>(null);
  const [scene, setScene] = useState<SceneInfo | null>(null);
  /** 設定DBの画面への頼み（エントリを開く、作る。D049）。 */
  const [bibleRequest, setBibleRequest] = useState<BibleRequest | null>(null);
  /** 設定ファイルが外で変わった回数（設定DBと変数の画面が読み直す）。 */
  const [settingsVersion, setSettingsVersion] = useState(0);
  /** アイデアメモが外で変わった回数（企画の画面が読み直す。T2-6）。 */
  const [ideasVersion, setIdeasVersion] = useState(0);
  /**
   * 開いているマージの画面（T2-7）。`mine` は開いたときに送った自分の本文、`sent` はコアに渡した元（null なら
   * コアが知っている元）、`base` はこのマージの元（置き換えを見送ったときに、次のマージの元にする）。
   */
  const [merge, setMerge] = useState<{
    readonly file: string;
    readonly mine: string;
    readonly sent: string | null;
    readonly base: string | null;
    readonly view: MergeView;
  } | null>(null);
  /** プレビューのデータ（プレビューの画面を開いているときだけ取る。T1-11、D050）。 */
  const [preview, setPreview] = useState<PreviewData | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  /** プレビューへの再生の頼み（メインウィンドウのプレビューの画面に渡す。T2-4）。 */
  const [previewRequest, setPreviewRequest] = useState<PreviewRequest | null>(
    null,
  );
  /** プレビューの別ウィンドウを開いているか。 */
  const [previewWindow, setPreviewWindow] = useState(false);
  const requestSeq = useRef(0);
  /** エディタのカーソルの最後の位置（エディタの位置）。「ここから再生」に使う。 */
  const lastCursor = useRef<{ file: string; offset: number } | null>(null);
  /** エディタに印を付ける、プレビュー中の行（「エディタと同期」）。 */
  const [previewMarker, setPreviewMarker] = useState<{
    file: string;
    offset: number;
  } | null>(null);
  /** 保存していない、設定ファイルのソース表示の変更（画面ごと）。閉じる前の確認に使う。 */
  const [settingsDrafts, setSettingsDrafts] = useState<ReadonlySet<string>>(
    new Set(),
  );
  const [quickOpen, setQuickOpen] = useState(false);
  /** コマンドパレット（Mod-Shift-P。T2-8）。 */
  const [palette, setPalette] = useState(false);
  /** 名前の変更の画面（F2。T2-8）。 */
  const [renaming, setRenaming] = useState<RenameSymbol | null>(null);
  /** 共通とルートごとの文字数（ステータスバー。T2-8）。 */
  const [stats, setStats] = useState<readonly RouteStat[]>([]);
  const [viewPrefs, setViewPrefs] = useState<ViewPrefs>(loadViewPrefs);
  const updateViewPrefs = useCallback(
    (change: (prefs: ViewPrefs) => ViewPrefs) =>
      setViewPrefs((prefs) => {
        const next = change(prefs);
        saveViewPrefs(next);
        return next;
      }),
    [],
  );
  /** フローの画面の粒度と、`layout.json` が外で変わった回数（D048）。 */
  const [flowLevel, setFlowLevel] = useState<GraphLevel>("scene");
  const [layoutVersion, setLayoutVersion] = useState(0);
  /** ファイルごとの表示（覚えていないファイルは執筆表示。D046）。 */
  const [modes, setModes] = useState<ReadonlyMap<string, ViewMode>>(new Map());
  /** ディスクの中身が変わった回数（エディタに渡す savedText を作り直すため）。 */
  const [savedVersion, setSavedVersion] = useState(0);
  /** プロジェクトを開いた回数。開くたびにエディタを作り直す。 */
  const [openSeq, setOpenSeq] = useState(0);

  /** 本文の今の中身（ファイルごと。保存していない編集を含む）。コアの状態と同じものを持つ。 */
  const buffers = useRef(new Map<string, string>());
  /** ディスクの中身（ファイルごと）。 */
  const saved = useRef(new Map<string, string>());
  /**
   * マージの元（ファイルごと。T2-7、D058）。知らせの途中の競合（コアが読み込み直した後に、まだ送っていなかった
   * 編集を戻した）のときだけ持つ。コアはこの元を知らないので、マージのときに渡す。
   */
  const mergeBases = useRef(new Map<string, string>());
  /** 本文の置き換えを見送ったときに、マージの元にする中身（置き換えを頼んだときの、エディタの本文の元）。 */
  const skipBases = useRef(new Map<string, string>());
  /** 解析の依頼の通し番号。古い結果を捨てるのに使う。 */
  const request = useRef(0);
  /** 置き換えの通し番号。 */
  const replaceSeq = useRef(0);
  /** 変換中か。変換中に届いたもので表示が変わるものは、変換が終わってから反映する。 */
  const composing = useRef(false);
  /** 変換が終わってから行う、表示の更新。 */
  const afterComposition = useRef<(() => void)[]>([]);
  // 知らせの受け取りや、閉じる前の確認の中では、最新の値を読む。
  const latest = useRef({
    project,
    currentFile,
    dirty,
    conflicts,
    modes,
    currentScene,
    assist,
    settingsDrafts,
    screen,
    previewMarker,
    previewWindow,
  });
  latest.current = {
    project,
    currentFile,
    dirty,
    conflicts,
    modes,
    currentScene,
    assist,
    settingsDrafts,
    screen,
    previewMarker,
    previewWindow,
  };

  /**
   * エディタの本文を置き換える。`expected` はエディタの今の本文（違えば置き換えを見送り、選ばせる）、`base` は
   * 見送ったときにマージの元にする中身（今の本文の元。省けば `expected`）。
   */
  const replaceText = useCallback(
    (
      file: string,
      text: string,
      expected: string | null,
      base: string | null = expected,
    ) => {
      replaceSeq.current += 1;
      if (base === null) skipBases.current.delete(file);
      else skipBases.current.set(file, base);
      setReplace({ file, text, expected, seq: replaceSeq.current });
    },
    [],
  );

  /** 表示を変える。変換中なら、変換が終わるまで待つ（D039）。 */
  const whenIdle = useCallback((update: () => void) => {
    if (composing.current) afterComposition.current.push(update);
    else update();
  }, []);

  /** 今のシーンの情報を読み直す（文脈パネル）。 */
  const refreshScene = useCallback(
    (id: string | null) => {
      if (id === null) {
        whenIdle(() => setScene(null));
        return;
      }
      void sceneInfo(id)
        .then((info) => {
          if (latest.current.currentScene === id)
            whenIdle(() => setScene(info));
        })
        .catch(() => {});
    },
    [whenIdle],
  );

  /**
   * 解析の結果を画面に出す。変換中なら、変換が終わるまで待つ。検証の段階1（`checks`）と一緒に届いたとき
   * （保存、外部の変更など）は、今のシーンの情報も読み直す。
   */
  const showSummary = useCallback(
    (summary: Summary, checks?: readonly IrDiagnostic[]) => {
      const file = latest.current.currentFile;
      const result =
        file === null
          ? null
          : analysisFor(summary, file, buffers.current.get(file) ?? "");
      whenIdle(() => {
        setProject((p) =>
          p === null ? null : { ...p, summary, checks: checks ?? p.checks },
        );
        setAnalysis(result);
      });
      if (checks !== undefined) refreshScene(latest.current.currentScene);
    },
    [whenIdle, refreshScene],
  );

  /** 外で消されたファイルを閉じる。 */
  const closeFile = useCallback((file: string) => {
    buffers.current.delete(file);
    saved.current.delete(file);
    const rest = (latest.current.project?.files ?? []).filter(
      (f) => f !== file,
    );
    setProject((p) =>
      p === null ? null : { ...p, files: p.files.filter((f) => f !== file) },
    );
    setDirty((set) => withoutFiles(set, new Set([file])));
    setConflicts((map) => withoutConflict(map, file));
    if (latest.current.currentFile === file) setCurrentFile(rest[0] ?? null);
  }, []);

  const load = useCallback((opened: OpenedProject) => {
    buffers.current = new Map(opened.files.map((f) => [f.path, f.text]));
    saved.current = new Map(opened.files.map((f) => [f.path, f.text]));
    const files = opened.files.map((f) => f.path);
    const first = files[0] ?? null;
    afterComposition.current = [];
    setOpenSeq((n) => n + 1);
    setFocus(null);
    setReplace(null);
    setDirty(new Set());
    setConflicts(new Map());
    setMerge(null);
    mergeBases.current.clear();
    skipBases.current.clear();
    setRenaming(null);
    setPalette(false);
    setStats([]);
    setProject({
      root: opened.root,
      title: opened.title,
      brackets: opened.brackets,
      summary: opened.summary,
      checks: opened.checks,
      files,
    });
    setCurrentFile(first);
    setAnalysis(
      first === null
        ? null
        : analysisFor(opened.summary, first, buffers.current.get(first) ?? ""),
    );
    setScreen("text");
    setError(null);
    setNotice(`${opened.title} を開きました`);
    setAssist(opened.assist);
    setEditorSettings(opened.editor);
    setCurrentScene(null);
    setScene(null);
    setBibleRequest(null);
    setSettingsDrafts(new Set());
    setPreview(null);
    setPreviewError(null);
    setPreviewMarker(null);
    setModes(new Map());
    void viewModes()
      .then((m) => setModes(new Map(Object.entries(m))))
      .catch(() => {});
    void recentProjects().then(setRecent);
  }, []);

  /** ファイルの表示を変え、覚える。 */
  const selectMode = useCallback(
    (file: string, mode: ViewMode) => {
      setModes((map) => new Map(map).set(file, mode));
      void setViewMode(file, mode).catch((e: unknown) =>
        whenIdle(() => setError(`表示を覚えられません：${String(e)}`)),
      );
    },
    [whenIdle],
  );

  const toggleMode = useCallback(
    (file: string) => {
      const now = latest.current.modes.get(file) ?? "writing";
      selectMode(file, now === "writing" ? "source" : "writing");
    },
    [selectMode],
  );

  /** 保存していない変更があれば、捨ててよいかを聞く。 */
  const confirmDiscard = useCallback(async () => {
    if (
      latest.current.dirty.size === 0 &&
      latest.current.settingsDrafts.size === 0
    )
      return true;
    return confirm(
      "保存していない変更があります。保存せずに続けますか？",
      "保存していない変更",
    );
  }, []);

  const open = useCallback(
    async (path: string) => {
      if (!(await confirmDiscard())) return;
      try {
        load(await openProject(path));
      } catch (e) {
        setError(String(e));
      }
    },
    [confirmDiscard, load],
  );

  const openPicker = useCallback(async () => {
    const path = await pickProjectFolder();
    if (path !== null) await open(path);
  }, [open]);

  const create = useCallback(async () => {
    if (!(await confirmDiscard())) return;
    const path = await pickNewProjectFolder();
    if (path === null) return;
    try {
      load(await createProject(path));
    } catch (e) {
      setError(String(e));
    }
  }, [confirmDiscard, load]);

  // 起動時：最近開いたものを読み、開発用の作品フォルダがあれば開く（IMEテストと、受け渡しの時間を測るため）。
  useEffect(() => {
    void recentProjects().then(setRecent);
    devSettings()
      .then(async (dev) => {
        if (dev.project === null) return;
        const opened = await openProject(dev.project);
        load(opened);
        if (dev.measure_ipc) await measureIpc(opened);
      })
      .catch((e: unknown) => setError(`コアに接続できません：${String(e)}`));
  }, [load]);

  // 閉じる前に、保存していない変更があれば聞く。
  useEffect(() => {
    const unlisten = guardClose(confirmDiscard);
    return () => {
      void unlisten.then((f) => f());
    };
  }, [confirmDiscard]);

  // 外部の変更の知らせ。
  useEffect(() => {
    // 変換中は取り込まず（表示を変えず）、変換が終わってから取り込む。取り込むときの「まだ送っていない
    // 編集があるか」は、そのときのエディタで決める。
    const onChanged = (event: WorkspaceChanged) => {
      const { currentFile: current, dirty: dirtyNow } = latest.current;
      let files = latest.current.project?.files ?? [];
      const newConflicts = new Map(latest.current.conflicts);
      const closed = new Set<string>();
      const changed: string[] = [];
      const unreadable: string[] = [];
      for (const change of event.changes) {
        switch (change.kind) {
          case "reloaded":
          case "added": {
            const before = saved.current.get(change.file);
            files = withFile(files, change.file);
            saved.current.set(change.file, change.text);
            if (dirtyNow.has(change.file)) {
              // コアが読み込み直す前に、まだ送っていない編集があった。コアに自分の編集を戻し、選ばせる。
              // コアは読み込み直す前の中身を知らないので、マージの元として覚えておく。
              rememberBase(mergeBases.current, change.file, before);
              newConflicts.set(change.file, { deleted: false });
              const mine = buffers.current.get(change.file);
              if (mine !== undefined)
                void parseBuffer(change.file, mine).catch(() => {});
            } else {
              buffers.current.set(change.file, change.text);
              if (change.file === current && before !== undefined)
                replaceText(change.file, change.text, before);
            }
            changed.push(change.file);
            break;
          }
          case "removed":
            if (dirtyNow.has(change.file)) {
              // コアが閉じる前に、まだ送っていない編集があった。閉じるか、残して保存で作り直すかを選ばせる。
              saved.current.delete(change.file);
              newConflicts.set(change.file, { deleted: true });
            } else {
              buffers.current.delete(change.file);
              saved.current.delete(change.file);
              files = files.filter((f) => f !== change.file);
              newConflicts.delete(change.file);
              closed.add(change.file);
            }
            changed.push(change.file);
            break;
          case "conflict":
            newConflicts.set(change.file, { deleted: change.deleted });
            changed.push(change.file);
            break;
          case "unreadable":
            unreadable.push(change.file);
            break;
          case "settings":
            changed.push("設定ファイル");
            setSettingsVersion((n) => n + 1);
            break;
          case "ideas":
            changed.push("アイデアメモ");
            setIdeasVersion((n) => n + 1);
            break;
          case "layout":
            setLayoutVersion((n) => n + 1);
            break;
        }
      }
      setConflicts(newConflicts);
      // 開いているマージの画面のファイルが変わったら、見せたマージは古いので閉じる。
      setMerge((m) =>
        m !== null &&
        event.changes.some((c) => "file" in c && c.file === m.file)
          ? null
          : m,
      );
      if (closed.size > 0) setDirty((set) => withoutFiles(set, closed));
      if (current !== null && closed.has(current))
        setCurrentFile(files[0] ?? null);
      setSavedVersion((n) => n + 1);
      setProject((p) =>
        p === null
          ? null
          : {
              ...p,
              title: event.title,
              brackets: event.brackets,
              files,
            },
      );
      setAssist(event.assist);
      setEditorSettings(event.editor);
      showSummary(event.summary, event.checks);
      if (changed.length > 0)
        setNotice(`外部の変更を取り込みました：${changed.join("、")}`);
      if (unreadable.length > 0)
        setError(
          `${unreadable.join("、")} をUTF-8で読めないので、外部の変更を取り込んでいません`,
        );
    };
    const unlistenChanged = onWorkspaceChanged((event) =>
      whenIdle(() => onChanged(event)),
    );
    const unlistenError = onWorkspaceError((message) =>
      whenIdle(() => setError(message)),
    );
    return () => {
      void unlistenChanged.then((f) => f());
      void unlistenError.then((f) => f());
    };
  }, [replaceText, showSummary, whenIdle]);

  const onIdle = useCallback(
    async (file: string, text: string) => {
      // 閉じたファイルの、まだ送っていなかった編集は捨てる。
      if (!buffers.current.has(file)) return;
      buffers.current.set(file, text);
      // 外で消されたのに残っているファイル（選ぶまで、コアは閉じている）は、選んだときに送る。
      if (latest.current.conflicts.get(file)?.deleted === true) return;
      const seq = ++request.current;
      const sceneId = latest.current.currentScene;
      try {
        const result = await parseBuffer(file, text, sceneId);
        if (seq !== request.current) return;
        showSummary(result.summary);
        whenIdle(() => {
          if (result.scene !== null && result.scene.scene === sceneId) {
            setScene(result.scene);
          }
          setError(null);
        });
      } catch (e) {
        whenIdle(() => setError(String(e)));
      }
    },
    [showSummary, whenIdle],
  );

  const onComposingChange = useCallback((value: boolean) => {
    composing.current = value;
    if (value) return;
    const waiting = afterComposition.current;
    afterComposition.current = [];
    for (const update of waiting) update();
  }, []);

  const onDirty = useCallback((file: string, value: boolean) => {
    if (!buffers.current.has(file)) return;
    setDirty((set) => {
      if (set.has(file) === value) return set;
      const next = new Set(set);
      if (value) next.add(file);
      else next.delete(file);
      return next;
    });
  }, []);

  /** 「自分の変更を残す」。ディスクの中身（外の中身）を覚え直す。 */
  const keep = useCallback(
    async (file: string, text: string) => {
      const kept = await keepMine(file, text);
      if (kept.disk === null) saved.current.delete(file);
      else saved.current.set(file, kept.disk);
      setConflicts((map) => withoutConflict(map, file));
      setSavedVersion((n) => n + 1);
      showSummary(kept.summary, kept.checks);
    },
    [showSummary],
  );

  /** 保存する（エディタが1つずつ呼ぶ）。書いた本文（付いた行IDを含む）を返す。 */
  const onSave = useCallback(
    async (file: string, text: string): Promise<string | null> => {
      buffers.current.set(file, text);
      try {
        // 外部の変更とぶつかっていたら、保存は「自分の変更を残す」ことにする。
        if (latest.current.conflicts.has(file)) await keep(file, text);
        const result = await saveFile(
          file,
          text,
          saved.current.get(file) ?? null,
        );
        if (result.kind === "conflict") {
          // まだ取り込んでいない外部の変更があったので、書かなかった。選ばせる。
          setConflicts((map) =>
            new Map(map).set(file, { deleted: result.deleted }),
          );
          whenIdle(() =>
            setNotice(
              `${file} は外部で${result.deleted ? "削除" : "変更"}されていたので、保存していません`,
            ),
          );
          return null;
        }
        saved.current.set(file, result.text);
        mergeBases.current.delete(file);
        // 保存を待つ間に編集を送っていなければ、書いた本文が今の本文。送っていれば、エディタが行IDを入れて送り直す。
        if (buffers.current.get(file) === text)
          buffers.current.set(file, result.text);
        setSavedVersion((n) => n + 1);
        showSummary(result.summary, result.checks);
        whenIdle(() => {
          setNotice(
            result.assigned > 0
              ? `${file} を保存しました（行IDを ${result.assigned} 行に付けました）`
              : `${file} を保存しました`,
          );
          setError(null);
        });
        return result.text;
      } catch (e) {
        whenIdle(() => setError(String(e)));
        return null;
      }
    },
    [keep, showSummary, whenIdle],
  );

  /** 読み込み直しの知らせの後、置き換える前に編集していた。コアに自分の編集を戻し、選ばせる。 */
  const onReplaceSkipped = useCallback((file: string, text: string) => {
    const base = skipBases.current.get(file);
    rememberBase(mergeBases.current, file, base);
    buffers.current.set(file, text);
    void parseBuffer(file, text).catch(() => {});
    setConflicts((map) => new Map(map).set(file, { deleted: false }));
  }, []);

  /** マージの画面を開く。まだ送っていない編集を先にコアへ送り、それを自分の変更にする。 */
  const startMerge = useCallback(async (file: string) => {
    try {
      const mine = buffers.current.get(file) ?? "";
      await parseBuffer(file, mine);
      const sent = mergeBases.current.get(file) ?? null;
      const view = await mergeView(file, sent);
      setMerge({
        file,
        mine,
        sent,
        base: sent ?? saved.current.get(file) ?? null,
        view,
      });
    } catch (e) {
      setError(String(e));
    }
  }, []);

  /** マージを当てる：結果を保存していない変更として開き、ディスクの中身を外の中身にする。 */
  const finishMerge = useCallback(
    async (choices: readonly MergeChoice[]) => {
      if (merge === null) return;
      const { file, mine, sent, base, view } = merge;
      try {
        const result = await applyMerge(file, sent, choices, view.token);
        mergeBases.current.delete(file);
        buffers.current.set(file, result.text);
        if (result.disk === null) saved.current.delete(file);
        else saved.current.set(file, result.disk);
        // 画面を開いた後に打った編集があれば、置き換えずに、もう一度選ばせる（onReplaceSkipped）。そのときは
        // このマージの元から、もう一度マージできる。
        if (latest.current.currentFile === file)
          replaceText(file, result.text, mine, base);
        setConflicts((map) => withoutConflict(map, file));
        setSavedVersion((n) => n + 1);
        showSummary(result.summary, result.checks);
        setMerge(null);
        whenIdle(() =>
          setNotice(`${file} をマージしました。保存すると書きます`),
        );
      } catch (e) {
        // 開いた後に本文か外の中身が変わった（コアが断った）。閉じて、開き直させる。
        setMerge(null);
        setError(String(e));
      }
    },
    [merge, replaceText, showSummary, whenIdle],
  );

  const resolveConflict = useCallback(
    async (file: string, reload: boolean) => {
      setMerge((m) => (m?.file === file ? null : m));
      mergeBases.current.delete(file);
      try {
        if (reload) {
          const result = await reloadFile(file);
          if (result.text === null) {
            closeFile(file);
          } else {
            buffers.current.set(file, result.text);
            saved.current.set(file, result.text);
            if (latest.current.currentFile === file)
              replaceText(file, result.text, null);
            setConflicts((map) => withoutConflict(map, file));
          }
          setSavedVersion((n) => n + 1);
          showSummary(result.summary, result.checks);
        } else {
          await keep(file, buffers.current.get(file) ?? "");
        }
      } catch (e) {
        setError(String(e));
      }
    },
    [closeFile, keep, replaceText, showSummary],
  );

  const onAddChapter = useCallback(
    async (name: string): Promise<string | null> => {
      try {
        const result = await addChapter(name);
        buffers.current.set(result.file, "");
        saved.current.set(result.file, "");
        setProject((p) =>
          p === null ? null : { ...p, files: withFile(p.files, result.file) },
        );
        showSummary(result.summary, result.checks);
        setCurrentFile(result.file);
        return null;
      } catch (e) {
        return String(e);
      }
    },
    [showSummary],
  );

  /** カーソルが止まった：そのシーンを今のシーンにし、情報を読み直す。 */
  const onCursor = useCallback(
    (file: string, offset: number) => {
      lastCursor.current = { file, offset };
      const p = latest.current.project;
      const text = buffers.current.get(file);
      if (p === null || text === undefined) return;
      const id = sceneAt(p.summary.scenes, file, text, offset);
      if (id === latest.current.currentScene) return;
      latest.current.currentScene = id;
      setCurrentScene(id);
      refreshScene(id);
    },
    [refreshScene],
  );

  /** 設定DBの画面を開いて頼む（D049）。 */
  const requestBible = useCallback((make: (seq: number) => BibleRequest) => {
    setBibleRequest((r) => make((r?.seq ?? 0) + 1));
    setScreen("bible");
  }, []);

  /** 未登録の話者を、設定DBの画面の「作る」の欄で登録する（D047、D049）。 */
  const onRegister = useCallback(
    (name: string) =>
      requestBible((seq) => ({ kind: "new", typeId: null, name, seq })),
    [requestBible],
  );

  /** 設定DBか変数を書いた後：補完の材料と、解析の結果を入れる。 */
  const onSettingsWritten = useCallback(
    (result: {
      readonly assist: AssistData;
      readonly summary: Summary;
      readonly checks: readonly IrDiagnostic[];
    }) => {
      setAssist(result.assist);
      showSummary(result.summary, result.checks);
    },
    [showSummary],
  );

  const onSettingsDraft = useCallback((screenName: string, value: boolean) => {
    setSettingsDrafts((set) => {
      if (set.has(screenName) === value) return set;
      const next = new Set(set);
      if (value) next.add(screenName);
      else next.delete(screenName);
      return next;
    });
  }, []);
  /**
   * 設定DBか変数の画面から移る前に、その画面のソース表示に保存していない変更があれば、捨ててよいかを聞く。
   */
  const leaveSettings = useCallback((go: () => void) => {
    const { settingsDrafts: drafts } = latest.current;
    const here = latest.current.screen;
    const draft =
      (here === "bible" && drafts.has("bible")) ||
      (here === "variables" && drafts.has("variables")) ||
      (here === "concept" && drafts.has("concept"));
    if (!draft) {
      go();
      return;
    }
    void confirm(
      "ソース表示に保存していない変更があります。捨てて移りますか？",
      "保存していない変更",
    ).then((ok) => {
      if (ok) go();
    });
  }, []);
  const selectScreen = useCallback(
    (next: Screen) => {
      if (next === latest.current.screen) return;
      leaveSettings(() => setScreen(next));
    },
    [leaveSettings],
  );
  const onBibleRequestHandled = useCallback(() => setBibleRequest(null), []);

  const onBibleDraft = useCallback(
    (value: boolean) => onSettingsDraft("bible", value),
    [onSettingsDraft],
  );
  const onVariablesDraft = useCallback(
    (value: boolean) => onSettingsDraft("variables", value),
    [onSettingsDraft],
  );
  const onConceptDraft = useCallback(
    (value: boolean) => onSettingsDraft("concept", value),
    [onSettingsDraft],
  );

  /** 企画の画面から設定DBのエントリを開く。 */
  const openEntry = useCallback(
    (path: string) =>
      leaveSettings(() =>
        requestBible((seq) => ({ kind: "entry", path, seq })),
      ),
    [leaveSettings, requestBible],
  );

  /**
   * コアが本文のファイルを書いた（アイデアメモの昇格で、章にスタブのシーンを足した。T2-6）。外部の変更を
   * 読み込み直したときと同じく、エディタの中身を差し替える。まだ送っていない編集があれば（ふつうは選べない）、
   * コアに自分の編集を戻し、読み込み直すか残すかを選ばせる。
   */
  const onStoryWritten = useCallback(
    (file: string, text: string) => {
      const before = saved.current.get(file);
      saved.current.set(file, text);
      if (latest.current.dirty.has(file)) {
        rememberBase(mergeBases.current, file, before);
        setConflicts((m) => new Map(m).set(file, { deleted: false }));
        const mine = buffers.current.get(file);
        if (mine !== undefined) void parseBuffer(file, mine).catch(() => {});
      } else {
        buffers.current.set(file, text);
        if (file === latest.current.currentFile && before !== undefined)
          replaceText(file, text, before);
      }
      setSavedVersion((n) => n + 1);
    },
    [replaceText],
  );

  /** F2：カーソルのところの名前を変える画面を開く。先に今の本文をコアへ送る（T2-8）。 */
  const onRename = useCallback(
    async (file: string, offset: number, text: string) => {
      try {
        buffers.current.set(file, text);
        await parseBuffer(file, text);
        const symbol = await symbolAt(
          file,
          new PositionMap(text).utf16ToByte(offset),
        );
        if (symbol === null) {
          whenIdle(() =>
            setNotice(
              "ここには名前を変えられるものがありません（シーンID、話者、立ち絵のキャラクター、変数の上で F2）",
            ),
          );
          return;
        }
        setRenaming(symbol);
      } catch (e) {
        setError(String(e));
      }
    },
    [whenIdle],
  );

  /** 名前を変える。変わった本文はエディタに入れ、設定の画面は読み直す。できなければ誤りの文。 */
  const doRename = useCallback(
    async (next: string): Promise<string | null> => {
      const symbol = renaming;
      if (symbol === null) return null;
      try {
        // 外部の変更とぶつかっている章（選ぶ前）を書き換えると、まだ選んでいない外の変更が消えるので断る。
        const preview = await renamePreview(symbol.target, next);
        const blocked = preview.story.filter((f) =>
          latest.current.conflicts.has(f),
        );
        if (blocked.length > 0)
          return `${blocked.join("、")} は外部の変更とぶつかっています。選んでから名前を変えてください`;
        const result = await applyRename(symbol.target, next);
        const written = new Set(result.edit.story.map((c) => c.file));
        setDirty((set) => withoutFiles(set, written));
        for (const change of result.edit.story) {
          const before = buffers.current.get(change.file);
          saved.current.set(change.file, change.text);
          buffers.current.set(change.file, change.text);
          mergeBases.current.delete(change.file);
          if (
            change.file === latest.current.currentFile &&
            before !== undefined
          )
            replaceText(change.file, change.text, before, before);
        }
        setSavedVersion((n) => n + 1);
        setSettingsVersion((n) => n + 1);
        setIdeasVersion((n) => n + 1);
        setLayoutVersion((n) => n + 1);
        onSettingsWritten(result);
        // エントリのIDを変えたら、設定DBの画面は新しいパスのエントリを開く。
        const target = symbol.target;
        if (target.kind === "entry_id" && latest.current.screen === "bible") {
          const path = `bible/entries/${target.type_id}/${next}.md`;
          requestBible((seq) => ({ kind: "entry", path, seq }));
        }
        if (result.edit.problem !== null) {
          setError(
            `名前の変更の途中で書けませんでした（書いたもの：本文 ${result.edit.story.length}ファイル、設定 ${result.edit.settings.length}ファイル）：${result.edit.problem}`,
          );
          return null;
        }
        whenIdle(() =>
          setNotice(
            `名前を変えました（本文 ${result.edit.story_edits}か所、設定 ${result.edit.settings.length}ファイル）`,
          ),
        );
        return null;
      } catch (e) {
        return String(e);
      }
    },
    [renaming, replaceText, onSettingsWritten, requestBible, whenIdle],
  );

  /** 章のファイル名を変える（T2-8）。保存していない変更と、エディタの状態は持ち越す。 */
  const onRenameChapter = useCallback(
    async (file: string, name: string): Promise<string | null> => {
      try {
        const result = await renameChapter(file, name);
        const to = result.file;
        for (const map of [
          buffers.current,
          saved.current,
          mergeBases.current,
          skipBases.current,
        ]) {
          const value = map.get(file);
          map.delete(file);
          if (value !== undefined) map.set(to, value);
        }
        setDirty((set) => {
          if (!set.has(file)) return set;
          const next = new Set(set);
          next.delete(file);
          next.add(to);
          return next;
        });
        setConflicts((map) => {
          const value = map.get(file);
          if (value === undefined) return map;
          const next = new Map(map);
          next.delete(file);
          next.set(to, value);
          return next;
        });
        const mode = latest.current.modes.get(file);
        if (mode !== undefined) selectMode(to, mode);
        setProject((p) =>
          p === null
            ? null
            : {
                ...p,
                files: withFile(
                  p.files.filter((f) => f !== file),
                  to,
                ),
              },
        );
        setLayoutVersion((n) => n + 1);
        if (latest.current.currentFile === file) {
          // 表示に要る分は、新しいファイル名で作る（`showSummary` は今のファイル名で作るため）。
          latest.current.currentFile = to;
          setCurrentFile(to);
        }
        showSummary(result.summary, result.checks);
        return null;
      } catch (e) {
        return String(e);
      }
    },
    [selectMode, showSummary],
  );

  /** 章を消す（確かめてから。T2-8）。 */
  const onDeleteChapter = useCallback(
    async (file: string) => {
      const scenes =
        latest.current.project?.summary.scenes.filter((s) => s.file === file)
          .length ?? 0;
      const ok = await confirm(
        `${file} を削除しますか？（シーン ${scenes}個${latest.current.dirty.has(file) ? "、保存していない変更あり" : ""}）。元に戻すには Git などを使います。`,
        "章を削除",
      );
      if (!ok) return;
      try {
        const result = await deleteChapter(file);
        closeFile(file);
        showSummary(result.summary, result.checks);
        whenIdle(() => setNotice(`${file} を削除しました`));
      } catch (e) {
        setError(String(e));
      }
    },
    [closeFile, showSummary, whenIdle],
  );

  /** 本文の画面で、あるファイルのある位置へ移る。 */
  const goTo = useCallback(
    (file: string, byteOffset: number | null) => {
      const text = buffers.current.get(file);
      if (text === undefined || project === null) return;
      setScreen("text");
      setCurrentFile(file);
      if (file !== currentFile)
        setAnalysis(analysisFor(project.summary, file, text));
      if (byteOffset !== null) {
        setFocus((f) => ({
          offset: new PositionMap(text).byteToUtf16(byteOffset),
          seq: (f?.seq ?? 0) + 1,
        }));
      }
    },
    [project, currentFile],
  );

  /** 本文の画面で、あるファイルの、プレビュー中の行（行IDで探し、無ければバイト位置）へ移る。 */
  const showPreviewLine = useCallback((line: PreviewLine) => {
    const text = buffers.current.get(line.file);
    if (text === undefined) return;
    const offset = locateLine(text, line.lineId, line.span[0]);
    setScreen("text");
    setCurrentFile(line.file);
    const p = latest.current.project;
    if (p !== null && line.file !== latest.current.currentFile)
      setAnalysis(analysisFor(p.summary, line.file, text));
    if (offset !== null) setFocus((f) => ({ offset, seq: (f?.seq ?? 0) + 1 }));
  }, []);

  /**
   * 「エディタへ戻る」：本文の画面へ戻る。プレビュー中の行の印があれば、その行のファイルを開く（カーソルは
   * 動かさない。印の行へはエディタがスクロールする）。
   */
  const closePreview = useCallback(() => {
    const marker = latest.current.previewMarker;
    if (marker !== null && marker.file !== latest.current.currentFile) {
      const text = buffers.current.get(marker.file);
      const p = latest.current.project;
      if (text !== undefined && p !== null) {
        setCurrentFile(marker.file);
        setAnalysis(analysisFor(p.summary, marker.file, text));
      }
    }
    setScreen("text");
  }, []);

  /** エディタに、プレビュー中の行の印を付ける（null で消す）。 */
  const onPreviewLine = useCallback((line: PreviewLine | null) => {
    if (line === null) {
      setPreviewMarker(null);
      return;
    }
    const text = buffers.current.get(line.file);
    const offset =
      text === undefined ? null : locateLine(text, line.lineId, line.span[0]);
    setPreviewMarker(offset === null ? null : { file: line.file, offset });
  }, []);

  /** 再生の頼みを、別ウィンドウ（開いていれば）か、メインウィンドウのプレビューの画面に渡す（T2-4）。 */
  const sendPreview = useCallback(
    (make: (seq: number) => PreviewRequest) => {
      requestSeq.current += 1;
      const request = make(requestSeq.current);
      if (latest.current.previewWindow) {
        openPreviewWindow(request).catch((e: unknown) => setError(String(e)));
        return;
      }
      setPreviewRequest(request);
      leaveSettings(() => setScreen("preview"));
    },
    [leaveSettings],
  );

  /** ある行（エディタの位置）から再生するための頼み（バイト位置にする。最短経路はプレビューが求める）。 */
  const atRequest = useCallback(
    (file: string, offset: number) =>
      (seq: number): PreviewRequest => {
        const text = buffers.current.get(file) ?? "";
        return {
          seq,
          kind: "at",
          file,
          offset: new PositionMap(text).utf16ToByte(offset),
        };
      },
    [],
  );

  /** ここから再生（F5、タイトルバー）：カーソルの行から、開始シーンからの最短経路で。 */
  const playHere = useCallback(
    (file: string, offset: number) => sendPreview(atRequest(file, offset)),
    [atRequest, sendPreview],
  );

  /** 「▶ ここから」：最後のカーソルの行から。カーソルが無ければ、そのファイルの最初のシーンの頭から。 */
  const playAtCursor = useCallback(() => {
    const file = latest.current.currentFile;
    if (file === null) return;
    const cursor = lastCursor.current;
    if (cursor !== null && cursor.file === file) {
      playHere(cursor.file, cursor.offset);
      return;
    }
    const first = latest.current.project?.summary.scenes.find(
      (scene) => scene.file === file,
    );
    if (first === undefined) {
      playHere(file, 0);
      return;
    }
    sendPreview((seq) => ({ seq, kind: "scene", scene: first.id }));
  }, [playHere, sendPreview]);

  /** プレビューを別ウィンドウで開く。プレビュー中の行（「エディタと同期」の印）があれば、その行から。 */
  const openWindow = useCallback(() => {
    const marker = latest.current.previewMarker;
    requestSeq.current += 1;
    const seq = requestSeq.current;
    const request: PreviewRequest =
      marker === null
        ? { seq, kind: "start" }
        : atRequest(marker.file, marker.offset)(seq);
    openPreviewWindow(request)
      .then(() => {
        setPreviewWindow(true);
        setScreen("text");
      })
      .catch((e: unknown) => setError(String(e)));
  }, [atRequest]);

  // F5 は「ここから再生」（エディタの外でも。WebView がページを読み込み直さないよう、いつも止める）。
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "F5") return;
      // エディタの中で押されたら、エディタが受けている（変換中は再生しない）。
      const handled = event.defaultPrevented;
      event.preventDefault();
      if (handled || event.isComposing || event.keyCode === 229) return;
      if (latest.current.screen === "text") playAtCursor();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [playAtCursor]);

  // 別ウィンドウから：プレビュー中の行の印、エディタで見せる、閉じた。
  useEffect(() => {
    const subs = [
      listenPreviewLine((line) => onPreviewLine(line)),
      listenShowInEditor((line) => {
        void focusThisWindow();
        showPreviewLine(line);
      }),
      onPreviewWindowClosed(() => {
        setPreviewWindow(false);
        setPreviewMarker(null);
      }),
    ];
    return () => {
      for (const sub of subs) void sub.then((f) => f());
    };
  }, [onPreviewLine, showPreviewLine]);

  // 別ウィンドウへ：本文が変わった（プレビューのデータを取り直す）。
  const summaryForWindow = previewWindow ? project?.summary : undefined;
  useEffect(() => {
    if (summaryForWindow === undefined) return;
    sendTextChanged().catch(() => {});
  }, [summaryForWindow]);

  const openFromSettings = useCallback(
    (file: string, offset: number) => leaveSettings(() => goTo(file, offset)),
    [leaveSettings, goTo],
  );

  const selectScene = useCallback(
    (scene: SceneSummary) => goTo(scene.file, scene.span[0]),
    [goTo],
  );

  /** 定義へ移る：シーンIDはそのシーンへ、話者名は設定DBの画面のそのキャラクターへ（D047、D049）。 */
  const onDefinition = useCallback(
    (definition: Definition) => {
      if (definition.kind === "scene") {
        const target = project?.summary.scenes.find(
          (s) => s.id === definition.id,
        );
        if (target === undefined)
          setNotice(`シーン ${definition.id} はありません`);
        else selectScene(target);
        return;
      }
      const name = definition.name;
      const character = latest.current.assist?.characters.find(
        (c) => c.id === name || c.name === name || c.aliases.includes(name),
      );
      if (character === undefined) {
        setNotice(`${name} は設定DBに登録されていません`);
        return;
      }
      requestBible((seq) => ({ kind: "character", id: character.id, seq }));
    },
    [project, selectScene, requestBible],
  );

  /** 検証の画面で診断を選んだ：本文ならその行へ、設定DBのエントリと変数ならその画面へ。 */
  const onSelectDiagnostic = useCallback(
    (d: IrDiagnostic) => {
      if (d.file.startsWith("bible/entries/")) {
        requestBible((seq) => ({ kind: "entry", path: d.file, seq }));
      } else if (d.file === "variables.yaml") {
        setScreen("variables");
      } else if (d.file === "assets/catalog.yaml") {
        setScreen("assets");
      } else {
        goTo(d.file, d.span[0]);
      }
    },
    [goTo, requestBible],
  );

  // プレビューの画面を離れたら、データを捨てる（次に開いたとき、古いデータで始めない）。
  useEffect(() => {
    if (screen === "preview") return;
    setPreview(null);
    setPreviewError(null);
  }, [screen]);

  // プレビューのデータを取る（プレビューの画面を開いたとき、本文が変わったとき。少し待ってから）。
  const previewSummary = screen === "preview" ? project?.summary : undefined;
  useEffect(() => {
    if (previewSummary === undefined) return;
    // 返事が来る前に画面を離れたり、取り直したりしたら、その返事は使わない。
    let cancelled = false;
    const timer = setTimeout(() => {
      previewData()
        .then((d) => {
          if (cancelled) return;
          setPreview(d);
          setPreviewError(null);
        })
        .catch((e: unknown) => {
          if (!cancelled) setPreviewError(String(e));
        });
    }, 300);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [previewSummary]);

  // クイックオープン（Mod-P）、コマンドパレット（Mod-Shift-P）、集中モード（Mod-Shift-F）。エディタの外に
  // フォーカスがあるときも開く。変換中は動かさない。
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.defaultPrevented || latest.current.project === null) return;
      if (event.isComposing || event.keyCode === 229) return;
      if (!(event.metaKey || event.ctrlKey)) return;
      const key = event.key.toLowerCase();
      if (key === "p" && event.shiftKey) {
        event.preventDefault();
        setPalette(true);
      } else if (key === "p") {
        event.preventDefault();
        setQuickOpen(true);
      } else if (key === "f" && event.shiftKey) {
        event.preventDefault();
        updateViewPrefs((p) => ({ ...p, focus: !p.focus }));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [updateViewPrefs]);

  // ルートごとの文字数（ステータスバー）。本文が変わってから少し待って取り直す。
  const statSummary = project?.summary;
  useEffect(() => {
    if (statSummary === undefined) return;
    let live = true;
    const timer = setTimeout(() => {
      routeStats()
        .then((s) => {
          if (live) setStats(s);
        })
        .catch(() => {});
    }, 1500);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [statSummary]);

  const allDiagnostics = useMemo(
    () =>
      project === null
        ? []
        : [...project.summary.diagnostics, ...project.checks],
    [project],
  );
  // 本文が変わったら、自動巡回の結果を捨てる（診断の位置がずれるため）。
  const currentSummary = project?.summary ?? null;
  /** 今の本文（走らせている途中で変わったかを、結果を受け取ったときに見る）。 */
  const latestSummary = useRef(currentSummary);
  latestSummary.current = currentSummary;
  const stage2Ref = useRef(stage2);
  stage2Ref.current = stage2;
  useEffect(() => {
    if (currentSummary === stage2Summary.current) return;
    stage2Summary.current = null;
    if (stage2Ref.current !== null) {
      setStage2(null);
      setStage2Discarded(true);
    }
  }, [currentSummary]);
  const runStage2 = useCallback(() => {
    setStage2Running(true);
    setStage2Error(null);
    const at = currentSummary;
    runPlaythrough()
      .then((report) => {
        // 走らせている途中で本文が変わった（作品を開き直したときも）なら、結果は古いので捨てる。
        if (latestSummary.current !== at) {
          setStage2(null);
          setStage2Discarded(true);
          return;
        }
        stage2Summary.current = at;
        setStage2Discarded(false);
        setStage2(report);
      })
      .catch((e: unknown) => setStage2Error(String(e)))
      .finally(() => setStage2Running(false));
  }, [currentSummary]);
  const checkDiagnostics = useMemo(
    () => [...allDiagnostics, ...(stage2?.diagnostics ?? [])],
    [allDiagnostics, stage2],
  );
  const counts = countBySeverity(allDiagnostics);
  const conflict =
    currentFile === null ? undefined : conflicts.get(currentFile);
  const savedText =
    currentFile === null ? "" : (saved.current.get(currentFile) ?? "");
  // savedVersion が変わるたびに、エディタへ渡すディスクの中身を読み直す。
  void savedVersion;
  const mode: ViewMode =
    currentFile === null ? "writing" : (modes.get(currentFile) ?? "writing");
  const titles = useMemo(
    () =>
      new Map(
        (project?.summary.scenes ?? []).map((scene) => [scene.id, scene.title]),
      ),
    [project?.summary.scenes],
  );
  const currentRoute =
    project?.summary.scenes.find((s) => s.id === currentScene)?.meta.route ??
    null;
  const routeStat = statOfRoute(stats, currentRoute);

  const commands = useMemo((): Command[] => {
    const go = (target: Screen, label: string, keywords: string): Command => ({
      id: `screen-${target}`,
      label: `画面：${label}`,
      keywords,
      run: () => selectScreen(target),
    });
    const toggle = (
      id: string,
      label: string,
      key: keyof ViewPrefs,
      keywords: string,
      keys?: string,
    ): Command => ({
      id,
      label: `${label}（${viewPrefs[key] ? "やめる" : "始める"}）`,
      keywords,
      keys,
      run: () => updateViewPrefs((p) => ({ ...p, [key]: !p[key] })),
    });
    const list: Command[] = [
      go("concept", "企画", "concept ideas"),
      go("text", "本文", "text editor"),
      go("flow", "フロー", "flow graph"),
      go("bible", "設定", "bible"),
      go("assets", "素材", "assets"),
      go("variables", "変数", "variables"),
      go("check", "検証", "check problems"),
      go("export", "出力", "export"),
      {
        id: "quick-open",
        label: "シーンを開く",
        keywords: "quick open scene",
        keys: "Mod-P",
        run: () => setQuickOpen(true),
      },
      {
        id: "play",
        label: "再生",
        keywords: "play preview",
        run: () => sendPreview((seq) => ({ seq, kind: "start" })),
      },
      toggle("focus", "集中モード", "focus", "focus zen", "Mod-Shift-F"),
      toggle(
        "typewriter",
        "タイプライタースクロール",
        "typewriter",
        "typewriter scroll",
      ),
      {
        id: "font",
        label: viewPrefs.gothic
          ? "本文の書体：明朝にする"
          : "本文の書体：ゴシックにする",
        keywords: "font mincho gothic 明朝 ゴシック",
        run: () => updateViewPrefs((p) => ({ ...p, gothic: !p.gothic })),
      },
    ];
    const file = currentFile;
    if (file !== null) {
      list.push(
        {
          id: "play-here",
          label: "ここから再生",
          keywords: "play here",
          keys: "F5",
          run: () => playAtCursor(),
        },
        {
          id: "rename",
          label: "名前を変える（カーソルのところ）",
          keywords: "rename F2",
          keys: "F2",
          run: () => {
            const cursor = lastCursor.current;
            const text = buffers.current.get(file);
            if (text === undefined) return;
            void onRename(
              file,
              cursor !== null && cursor.file === file ? cursor.offset : 0,
              text,
            );
          },
        },
        {
          id: "toggle-view",
          label: "執筆表示とソース表示を切り替える",
          keywords: "view writing source",
          run: () => toggleMode(file),
        },
        {
          id: "delete-chapter",
          label: `章を削除（${file}）`,
          keywords: "delete chapter",
          run: () => void onDeleteChapter(file),
        },
      );
    }
    return list;
  }, [
    viewPrefs,
    currentFile,
    selectScreen,
    updateViewPrefs,
    sendPreview,
    playAtCursor,
    onRename,
    toggleMode,
    onDeleteChapter,
  ]);

  const mixedLineBreak = useMemo(
    () => (hasMixedLineBreaks(savedText) ? lineBreakOf(savedText) : null),
    [savedText],
  );

  return (
    <div className="app">
      <TitleBar
        title={project?.title ?? null}
        recent={recent}
        onOpen={() => void openPicker()}
        onCreate={() => void create()}
        onOpenRecent={(path) => void open(path)}
        onPlay={
          project === null
            ? null
            : () => sendPreview((seq) => ({ seq, kind: "start" }))
        }
        onPlayHere={
          project === null || currentFile === null ? null : playAtCursor
        }
      />
      {error !== null && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      <div className="body">
        {project !== null && viewPrefs.focus && (
          <button
            type="button"
            className="small-button focus-exit"
            onClick={() => updateViewPrefs((p) => ({ ...p, focus: false }))}
          >
            集中モードを終える（⌘⇧F）
          </button>
        )}
        {project !== null && !viewPrefs.focus && (
          <LeftBar
            screen={screen}
            problems={counts.error + counts.warning}
            onSelect={selectScreen}
          />
        )}
        <main className="workspace">
          {project === null ? (
            <Welcome
              recent={recent}
              onOpen={() => void openPicker()}
              onCreate={() => void create()}
              onOpenRecent={(path) => void open(path)}
            />
          ) : (
            <>
              <section
                className="screen text-screen"
                hidden={screen !== "text"}
                aria-label="本文"
              >
                {!viewPrefs.focus && (
                  <SceneList
                    files={project.files}
                    scenes={project.summary.scenes}
                    currentFile={currentFile}
                    dirty={dirty}
                    onSelectScene={selectScene}
                    onSelectFile={(file) => goTo(file, null)}
                    onAddChapter={onAddChapter}
                    onRenameChapter={onRenameChapter}
                    onDeleteChapter={(file) => void onDeleteChapter(file)}
                  />
                )}
                <section
                  className={
                    viewPrefs.gothic ? "editor-pane font-gothic" : "editor-pane"
                  }
                  aria-label="エディタ"
                >
                  {currentFile !== null && (
                    <ModeBar
                      file={currentFile}
                      mode={mode}
                      onSelect={(m) => selectMode(currentFile, m)}
                    />
                  )}
                  {currentFile !== null && conflict !== undefined && (
                    <ConflictBar
                      file={currentFile}
                      deleted={conflict.deleted}
                      onReload={() => void resolveConflict(currentFile, true)}
                      onKeep={() => void resolveConflict(currentFile, false)}
                      onMerge={() => void startMerge(currentFile)}
                    />
                  )}
                  {merge !== null && merge.file === currentFile && (
                    <MergePanel
                      key={merge.view.token}
                      file={merge.file}
                      view={merge.view}
                      onApply={finishMerge}
                      onCancel={() => setMerge(null)}
                    />
                  )}
                  {currentFile !== null && mixedLineBreak !== null && (
                    <LineBreakBar lineBreak={mixedLineBreak} />
                  )}
                  {currentFile !== null ? (
                    <Editor
                      key={openSeq}
                      file={currentFile}
                      text={buffers.current.get(currentFile) ?? ""}
                      savedText={savedText}
                      brackets={project.brackets}
                      viewMode={mode}
                      titles={titles}
                      analysis={analysis}
                      focus={focus}
                      replace={replace}
                      onIdle={onIdle}
                      onComposingChange={onComposingChange}
                      onDirty={onDirty}
                      onSave={onSave}
                      onReplaceSkipped={onReplaceSkipped}
                      onToggleView={toggleMode}
                      assist={assist}
                      editorSettings={editorSettings}
                      onCursor={onCursor}
                      onDefinition={onDefinition}
                      onQuickOpen={() => setQuickOpen(true)}
                      onPlayHere={playHere}
                      onRename={(file, offset, text) =>
                        void onRename(file, offset, text)
                      }
                      typewriter={viewPrefs.typewriter}
                      previewLine={previewMarker}
                    />
                  ) : (
                    <p className="placeholder">
                      章がありません。左の「＋ 章」で足してください。
                    </p>
                  )}
                </section>
                {!viewPrefs.focus && (
                  <ContextPanel
                    scene={scene}
                    diagnostics={allDiagnostics.filter(
                      (d) => d.file === currentFile,
                    )}
                    files={project.files}
                    onSelectDiagnostic={(d) => goTo(d.file, d.span[0])}
                    onRegister={onRegister}
                    onOpenCharacter={(id) =>
                      requestBible((seq) => ({ kind: "character", id, seq }))
                    }
                  />
                )}
              </section>
              {screen === "flow" && (
                <section className="screen flow-screen" aria-label="フロー">
                  <SceneGraph
                    summary={project.summary}
                    checks={project.checks}
                    level={flowLevel}
                    onLevel={setFlowLevel}
                    layoutVersion={layoutVersion}
                    onOpen={selectScene}
                    chapters={project.files}
                    dirtyChapters={dirty}
                    onStoryWritten={(result) => {
                      onStoryWritten(result.file, result.text);
                      showSummary(result.summary, result.checks);
                    }}
                  />
                </section>
              )}
              {screen === "concept" && (
                <section className="screen concept-screen" aria-label="企画">
                  <ConceptScreen
                    scenes={project.summary.scenes}
                    chapters={project.files}
                    dirtyChapters={dirty}
                    settingsVersion={settingsVersion}
                    ideasVersion={ideasVersion}
                    onWritten={onSettingsWritten}
                    onStoryWritten={onStoryWritten}
                    onOpenLocation={openFromSettings}
                    onOpenEntry={openEntry}
                    onUnsaved={onConceptDraft}
                  />
                </section>
              )}
              {screen === "bible" && (
                <section className="screen bible-screen" aria-label="設定DB">
                  <BibleScreen
                    scenes={project.summary.scenes}
                    diagnostics={allDiagnostics}
                    settingsVersion={settingsVersion}
                    request={bibleRequest}
                    onWritten={onSettingsWritten}
                    onOpenLocation={openFromSettings}
                    onUnsaved={onBibleDraft}
                    onRequestHandled={onBibleRequestHandled}
                    onRenameEntry={(typeId, id, name) =>
                      setRenaming({
                        target: { kind: "entry_id", type_id: typeId, id },
                        current: id,
                        label: `エントリのID（${name}）`,
                      })
                    }
                  />
                </section>
              )}
              {screen === "assets" && (
                <section className="screen assets-screen" aria-label="素材">
                  <AssetsScreen
                    root={project.root}
                    summary={project.summary}
                    scenes={project.summary.scenes}
                    settingsVersion={settingsVersion}
                    onWritten={onSettingsWritten}
                    onOpenLocation={openFromSettings}
                  />
                </section>
              )}
              {screen === "variables" && (
                <section className="screen variables-screen" aria-label="変数">
                  <VariablesScreen
                    scenes={project.summary.scenes}
                    diagnostics={allDiagnostics}
                    characters={assist?.characters ?? []}
                    settingsVersion={settingsVersion}
                    onWritten={onSettingsWritten}
                    onOpenLocation={openFromSettings}
                    onUnsaved={onVariablesDraft}
                  />
                </section>
              )}
              {screen === "preview" && (
                <section
                  className="screen preview-screen"
                  aria-label="プレビュー"
                >
                  <PreviewScreen
                    data={preview}
                    error={previewError}
                    title={project.title}
                    assetUrl={assetUrl}
                    onShowInEditor={showPreviewLine}
                    onLine={onPreviewLine}
                    onClose={closePreview}
                    request={previewRequest}
                    onOpenWindow={openWindow}
                  />
                </section>
              )}
              {screen === "export" && (
                <section className="screen export-screen" aria-label="出力">
                  <ExportScreen
                    summary={project.summary}
                    unsaved={dirty.size}
                    load={scriptSheet}
                    write={exportScript}
                    writeRenpy={exportRenpy}
                    loadChoices={readingChoices}
                    writeText={exportText}
                  />
                </section>
              )}
              {screen === "check" && (
                <section className="screen check-screen" aria-label="検証">
                  <PlaythroughPanel
                    report={stage2}
                    running={stage2Running}
                    error={stage2Error}
                    discarded={stage2Discarded}
                    onRun={runStage2}
                  />
                  <CheckList
                    heading="検証（プロジェクト全体）"
                    diagnostics={checkDiagnostics}
                    files={project.files}
                    onSelect={onSelectDiagnostic}
                    onRegister={onRegister}
                    canOpen={isSettingsFile}
                  />
                </section>
              )}
            </>
          )}
        </main>
      </div>
      {palette && project !== null && (
        <CommandPalette commands={commands} onClose={() => setPalette(false)} />
      )}
      {renaming !== null && project !== null && (
        <RenameDialog
          symbol={renaming}
          onApply={doRename}
          onClose={() => setRenaming(null)}
        />
      )}
      {quickOpen && project !== null && (
        <QuickOpen
          scenes={project.summary.scenes}
          onSelect={(s) => leaveSettings(() => selectScene(s))}
          onClose={() => setQuickOpen(false)}
        />
      )}
      <StatusBar
        route={routeStat === null ? null : routeStatText(routeStat)}
        mode={currentFile === null ? null : mode}
        file={currentFile}
        scene={
          currentScene === null
            ? null
            : {
                id: currentScene,
                chars:
                  project?.summary.scenes.find((s) => s.id === currentScene)
                    ?.chars ?? 0,
              }
        }
        fileChars={
          project === null || currentFile === null
            ? null
            : fileChars(project.summary.scenes, currentFile)
        }
        unsaved={dirty.size}
        counts={counts}
        notice={notice}
      />
    </div>
  );
}

/** 受け渡しの時間を測ったか。 */
let measured = false;

/**
 * 開発用：入力が止まったときと同じ受け渡し（parse_buffer）を何回か行い、かかった時間をターミナルに書く
 * （環境変数 `SCN_MEASURE_IPC` のとき。D045）。
 */
async function measureIpc(opened: OpenedProject) {
  // 開発時の React は効果を2回走らせるので、測るのは1回だけにする（同時に測ると、解析を待ち合って遅く出る）。
  if (measured) return;
  measured = true;
  const first = opened.files[0];
  if (first === undefined) return;
  const lines = opened.files.reduce((n, f) => n + f.text.split("\n").length, 0);
  // 今のシーンの情報も返すとき（T1-8、D047。索引も作る）と、返さないときの両方を測る。
  const scene =
    opened.summary.scenes.find((s) => s.file === first.path)?.id ?? null;
  for (const target of [null, scene]) {
    const times: number[] = [];
    let size = 0;
    for (let i = 0; i < 10; i++) {
      const start = performance.now();
      const result = await parseBuffer(first.path, first.text, target);
      times.push(performance.now() - start);
      if (i === 0) size = JSON.stringify(result).length;
    }
    times.sort((a, b) => a - b);
    await devReport(
      `受け渡しの時間（parse_buffer、今のシーン ${target ?? "なし"}、${opened.files.length}ファイル、${lines}行、JSON 約${size}文字）：` +
        `中央値 ${times[5]?.toFixed(1)}ミリ秒、最小 ${times[0]?.toFixed(1)}、最大 ${times[9]?.toFixed(1)}`,
    );
  }
}
