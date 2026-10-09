// アプリの骨格（docs/ui/README.md）：タイトルバー、左端のバー、画面（本文・フロー・検証）、ステータスバー。
// 作品フォルダを開く・作る・保存する。外部の変更（Claude Codeなどの書き換え）は、コアがファイルを監視して
// 取り込み、`workspace-changed` で知らせてくる（T1-6、D045）。
// 変換中は画面を変えない（D039）：解析の結果、外部の変更の知らせ、保存の結果で表示が変わるものは、
// 変換が終わってから反映する（whenIdle）。

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AiScreen } from "./ai/AiScreen.tsx";
import type { FieldDecision } from "./ai/FieldProposals.tsx";
import {
  entriesWrittenText,
  fieldProposalKey,
  fieldsWrittenText,
} from "./ai/fields.ts";
import {
  type AutoRun,
  applyAutoEvent,
  type FindingRecord,
  findingLines,
  findingOffset,
  findingsForScene,
  findTask,
  type HistoryEntry,
  historyStatus,
  isSkeleton,
  mergeFindings,
  type ProposalSet,
  plotWrittenText,
  proposalsByFile,
  type RunResult,
  type RunState,
  resultKey,
  type SkeletonTarget,
  type SummarizeRun,
  type TaskRunner,
  tokensText,
} from "./ai/model.ts";
import { AssetsScreen } from "./assets/AssetsScreen.tsx";
import { type BibleRequest, BibleScreen } from "./bible/BibleScreen.tsx";
import { type FieldOrigin, fieldOrigins } from "./bible/origin.ts";
import { CheckScreen } from "./check/CheckScreen.tsx";
import { countBySeverity } from "./check/group.ts";
import { ConceptScreen } from "./concept/ConceptScreen.tsx";
import {
  addChapter,
  adoptBranchProposal,
  adoptFieldProposal,
  adoptProposals,
  aiBranchDelete,
  aiBranchDiff,
  aiBranches,
  aiBranchMerge,
  aiConnections,
  aiLines,
  aiTaskList,
  applyMerge,
  applyRename,
  assetUrl,
  bookSettings,
  cancelAi,
  confirm,
  createProject,
  deleteAiKey,
  deleteChapter,
  devReport,
  devSettings,
  exportBook,
  exportOrders,
  exportPlugin,
  exportPlugins,
  exportProvenance,
  exportRenpy,
  exportScript,
  exportText,
  exportTyrano,
  exportUtage,
  focusThisWindow,
  guardClose,
  keepMine,
  onPreviewLine as listenPreviewLine,
  onShowInEditor as listenShowInEditor,
  mergeView,
  onAiAutoProgress,
  onAiDraftWritten,
  onAiRunProgress,
  onAiSummarizeProgress,
  onPreviewWindowClosed,
  onWorkspaceChanged,
  onWorkspaceError,
  openPreviewWindow,
  openProject,
  parseBuffer,
  pickNewProjectFolder,
  pickProjectFolder,
  prepareAiTask,
  previewData,
  readingChoices,
  proposalFiles as readProposalFiles,
  recentProjects,
  reloadFile,
  renameChapter,
  renamePreview,
  revealProject,
  routeStats,
  runAiAuto,
  runAiSummarize,
  runAiTask,
  runPlaythrough,
  saveFile,
  sceneInfo,
  scriptSheet,
  sendTextChanged,
  setAiCloudAllowed,
  setAiKey,
  setAiOverride,
  setAiTaskConnection,
  setAiVia,
  setSceneSummary,
  setViewMode,
  storySearchRanges,
  summaryDrift,
  symbolAt,
  testAiConnection,
  viewModes,
  writeAiDraft,
  writeAiSkeleton,
  writeConceptDraft,
  writeEntriesDraft,
} from "./core/api.ts";
import type { PreviewData } from "./core/ir.ts";
import { PositionMap } from "./core/position.ts";
import type {
  AiBranches,
  AiConnectionChoice,
  AiConnections,
  AiProposalFile,
  AiProposalView,
  AiTaskInfo,
  AssistData,
  AutoResult,
  AutoStages,
  Bracket,
  BranchProposal,
  ConceptDraft,
  EditorSettings,
  EntriesDraft,
  FieldProposal,
  FieldsWritten,
  GraphLevel,
  IrDiagnostic,
  MergeChoice,
  MergeView,
  OpenedProject,
  Playthrough,
  PlotWriteResult,
  PreviewRequest,
  ProjectSettingsWritten,
  RecentProject,
  RenameSymbol,
  RouteStat,
  SceneInfo,
  SceneSummary,
  SearchRanges,
  SettingsWrittenExtra,
  SkeletonScene,
  SummarizeResult,
  Summary,
  SummaryDrift,
  ViewMode,
  WorkspaceChanged,
} from "./core/types.ts";

/** 書き込んだ結果の文と、書けたか（T3-9・T3-10）。 */
interface Written {
  readonly text: string;
  readonly ok: boolean;
  /** 企画書・設定の下書きを書いた結果（T4-4・T4-5）。 */
  readonly fields?: readonly FieldsWritten[];
  readonly created?: readonly string[];
  /** 書かなかったもののわけ（T4-5）。 */
  readonly notes?: readonly string[];
}

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
import { type FindSpec, findAll } from "./editor/projectSearch.ts";
import { hasMixedLineBreaks, lineBreakOf } from "./editor/state.ts";
import { ExportScreen } from "./export/ExportScreen.tsx";
import { SceneGraph } from "./graph/SceneGraph.tsx";
import {
  aiStatus,
  type TestRecord,
  type TestState,
  testTarget,
} from "./preferences/connection.ts";
import { PreferencesScreen } from "./preferences/PreferencesScreen.tsx";
import { locateLine } from "./preview/locate.ts";
import { type PreviewLine, PreviewScreen } from "./preview/PreviewScreen.tsx";
import { outlineRows, sceneDiagnostics, sceneRange } from "./scenes/list.ts";
import { SceneList } from "./scenes/SceneList.tsx";
import { CommandPalette } from "./shell/CommandPalette.tsx";
import { ContextPanel } from "./shell/ContextPanel.tsx";
import { KeepAlive } from "./shell/KeepAlive.tsx";
import { LeftBar, type Screen } from "./shell/LeftBar.tsx";
import {
  CONTEXT_SCREENS,
  closeDrawer,
  isNarrow,
  loadClosedPanels,
  type PanelState,
  panelMode,
  resized,
  saveClosedPanels,
  showPanel,
  togglePanel,
} from "./shell/layout.ts";
import {
  type Command,
  type FindTarget,
  IS_MAC,
  keyForScreen,
  keyLabel,
  screenForKey,
} from "./shell/palette.ts";
import { QuickOpen } from "./shell/QuickOpen.tsx";
import { RenameDialog } from "./shell/RenameDialog.tsx";
import {
  CursorPosition,
  cursorStore,
  StatusBar,
  StatusContext,
} from "./shell/StatusBar.tsx";
import {
  fileChars,
  formatChars,
  routeStatText,
  sceneAt,
  statOfRoute,
} from "./shell/scene.ts";
import {
  aiRunStatus,
  checkStatus,
  preferencesStatus,
  SCREEN_LABEL,
  type StatusItems,
  saveState,
  textStatus,
} from "./shell/status.ts";
import { TitleBar } from "./shell/TitleBar.tsx";
import { AskUnsavedContext } from "./shell/UnsavedContext.ts";
import { UnsavedDialog } from "./shell/UnsavedDialog.tsx";
import {
  type DraftScreen,
  leaveProjectAsk,
  leaveSourceAsk,
  type UnsavedAsk,
  type UnsavedChoice,
} from "./shell/unsaved.ts";
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

/** ステータスバーの知らせを出しておく時間（ミリ秒）。 */
const NOTICE_MS = 8000;

/**
 * 画面を移る前に、フォーカスを外す。入れかけの欄は離れたときに書くので、キーで切り替えるとき（⌘数字、F5）も
 * 先に書かせる（T4-12・T4-14）。
 */
function blurActive(): void {
  if (document.activeElement instanceof HTMLElement) {
    document.activeElement.blur();
  }
}

/** 保存の確認の「保存する」で、エディタの保存を待つ長さ（ミリ秒。変換中なら変換が終わるまで待つ）。 */
const SAVE_WAIT_MS = 15000;

export function App() {
  const [project, setProject] = useState<ProjectView | null>(null);
  const [screen, setScreen] = useState<Screen>("text");
  /** 一度開いた画面（ほかの画面へ移っても隠して残す。作品を開き直したら空にする。T4-13、U5）。 */
  const [visited, setVisited] = useState<ReadonlySet<Screen>>(() => new Set());
  /**
   * 画面を見せた回数。外で直しても版の数に入らないファイル（`export/*.yaml`、`assets/` に置いたファイル）は、
   * 残した画面を見せるたびに読み直す（前は開くたびに作り直していたため。T4-13）。
   */
  const [shownCount, setShownCount] = useState(0);
  useEffect(() => {
    void screen;
    setShownCount((n) => n + 1);
    // プレビューは残さない（離れたら閉じて音を止める）。
    if (screen === "preview") return;
    setVisited((v) => (v.has(screen) ? v : new Set(v).add(screen)));
  }, [screen]);
  const loadBookSettings = useCallback(() => {
    void shownCount;
    return bookSettings();
  }, [shownCount]);
  const loadExportPlugins = useCallback(() => {
    void shownCount;
    return exportPlugins();
  }, [shownCount]);
  /** 自動巡回（検証の段階2。T3-2）の結果と、走らせている途中か。本文が変わったら捨てる。 */
  const [stage2, setStage2] = useState<Playthrough | null>(null);
  const [stage2Running, setStage2Running] = useState(false);
  const [stage2Error, setStage2Error] = useState<string | null>(null);
  const [stage2Discarded, setStage2Discarded] = useState(false);
  /** 段階2を最後に調べた時刻（検証の画面。T4-19）。 */
  const [stage2At, setStage2At] = useState<string | null>(null);
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
  /** 本文の解析が失敗したときの文（解析がうまくいったら、この文の帯だけを消す。T4-14、U13）。 */
  const parseError = useRef<string | null>(null);
  // ステータスバーの知らせは、しばらくしたら消す（T4-14、U13）。
  useEffect(() => {
    if (notice === null) return;
    const timer = setTimeout(() => setNotice(null), NOTICE_MS);
    return () => clearTimeout(timer);
  }, [notice]);
  const [recent, setRecent] = useState<RecentProject[]>([]);
  /** 右の欄の開け閉め（画面ごとに覚える）と、狭い窓で重ねて開いているか（T4-17、U06）。 */
  const [panels, setPanels] = useState<PanelState>(() => ({
    closed: loadClosedPanels(),
    drawer: false,
  }));
  /** 窓が 1280 より狭いか。 */
  const [narrow, setNarrow] = useState(() =>
    typeof window === "undefined" ? false : isNarrow(window.innerWidth),
  );
  /** 画面ごとのステータスバーの左（各画面が出す。T4-17、U05）。 */
  const [screenStatus, setScreenStatus] = useState<
    ReadonlyMap<Screen, StatusItems>
  >(new Map());
  const publishStatus = useCallback((s: Screen, items: StatusItems) => {
    setScreenStatus((map) =>
      JSON.stringify(map.get(s)) === JSON.stringify(items)
        ? map
        : new Map(map).set(s, items),
    );
  }, []);
  /** 今のブランチ（タイトルバー。Git でなければ null）。 */
  const [branch, setBranch] = useState<string | null>(null);
  /** 保存している本文の数と、保存できなかったわけ（ステータスバーの保存の様子。U08）。 */
  const [saving, setSaving] = useState(0);
  const [saveFailed, setSaveFailed] = useState<{
    readonly file: string;
    readonly message: string;
  } | null>(null);
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
  /**
   * 章ごとの検索の範囲（本文と行ID。検索の「本文だけ」と「すべての章」。要るときだけ読む。T4-22）と、範囲を頼んだ
   * ときの本文（コアに送ってあった本文。エディタは、自分の本文と同じときだけ範囲を使う）。
   */
  const [searchRanges, setSearchRanges] = useState<{
    readonly ranges: Readonly<Record<string, SearchRanges>>;
    readonly sources: ReadonlyMap<string, string>;
  } | null>(null);
  /** 検索の範囲を使う（検索の欄が「本文だけ」か「すべての章」の間。閉じたらやめる）。 */
  const rangesWanted = useRef(false);
  /** すべての章を探す問い合わせ（検索の欄の「すべての章」。null なら探さない。T4-22）。 */
  const [findAllSpec, setFindAllSpec] = useState<FindSpec | null>(null);
  /** 素材の画面で選ぶ素材（パレットから。番号は増え続ける。T4-20）。 */
  const [assetRequest, setAssetRequest] = useState<{
    readonly id: string;
    readonly seq: number;
  } | null>(null);
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
  /** 保存していない変更の確認のダイアログ（3択。T4-20）。答えは `resolve` で返す。 */
  const [unsavedDialog, setUnsavedDialog] = useState<{
    readonly ask: UnsavedAsk;
    readonly resolve: (choice: UnsavedChoice) => void;
  } | null>(null);
  const [unsavedSaving, setUnsavedSaving] = useState(false);
  const [unsavedError, setUnsavedError] = useState<string | null>(null);
  /** エディタに今のファイルの保存を頼む（増え続ける番号。保存の確認の「保存する」。T4-20）。 */
  const [saveRequest, setSaveRequest] = useState(0);
  /** 保存の終わりを待っているファイル（保存できたか）。待ち始めた後に始まった保存だけが答える。 */
  const saveWaiters = useRef(new Map<string, (ok: boolean) => void>());
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
  /** 本文の検索と置換の欄を開く頼み（見出しの帯の検索。T4-18）。 */
  const [openSearch, setOpenSearch] = useState(0);
  /** フローで選んでいるシーン（タイトルバーの再生。T4-17）。 */
  const [flowScene, setFlowScene] = useState<string | null>(null);
  const [layoutVersion, setLayoutVersion] = useState(0);
  /** ファイルごとの表示（覚えていないファイルは執筆表示。D046）。 */
  const [modes, setModes] = useState<ReadonlyMap<string, ViewMode>>(new Map());
  /** ディスクの中身が変わった回数（エディタに渡す savedText を作り直すため）。 */
  const [savedVersion, setSavedVersion] = useState(0);
  /** AIが書いた行（行IDから、今の出自。T3-4）。作品を開いたとき、保存・読み込み直しのたびに取り直す。 */
  const [aiLineMap, setAiLineMap] = useState<ReadonlyMap<string, string>>(
    () => new Map(),
  );
  /** 企画書と設定DBの AI の項目（ファイルとキーから、今の出自。T4-3）。 */
  const [aiFieldMap, setAiFieldMap] = useState<
    ReadonlyMap<string, FieldOrigin>
  >(() => new Map());
  /** AIの接続先（環境設定とタイトルバー。T3-5）。 */
  const [aiState, setAiState] = useState<AiConnections | null>(null);
  const [aiError, setAiError] = useState<string | null>(null);
  /** 接続先の設定（上書き・許可・キー）を変えた回数。変えたら取り直す。 */
  const [aiVersion, setAiVersion] = useState(0);
  /** 接続先ごとの「接続を試す」の様子と、試したときの接続先（設定が変わったら前の結果は見せない）。 */
  const [aiTests, setAiTests] = useState<ReadonlyMap<string, TestRecord>>(
    () => new Map(),
  );
  /** 試す回の番号。取り消した回・設定を変える前の回・前の作品の回の結果は捨てる。画面を読み込み直しても
   * コアが覚えている番号と重ならないよう、時刻から始める。 */
  const aiRun = useRef(Date.now());
  /** 走っている回の番号（取り消しに使う）。 */
  const aiRunning = useRef<number | null>(null);
  /** AI実行の画面のタスク（T3-7）。 */
  const [aiTasks, setAiTasks] = useState<readonly AiTaskInfo[] | null>(null);
  /** タスクに選べる接続先（T4-1）。 */
  const [aiTaskConnections, setAiTaskConnections] = useState<
    readonly AiConnectionChoice[]
  >([]);
  const [aiTasksError, setAiTasksError] = useState<string | null>(null);
  /** 走っている AI タスクの実行と、その回の番号（結果を受け取る回）。 */
  const [aiRunState, setAiRunState] = useState<RunState | null>(null);
  const taskRun = useRef<number | null>(null);
  /** 走っている実行のタスクと対象（取り消したときの結果に使う）。 */
  const taskRunTarget = useRef<{ task: string; scene: string | null } | null>(
    null,
  );
  /** タスクごとの最後の結果、実行の履歴、AIの指摘（この画面を開いている間だけ持つ）。 */
  const [aiResults, setAiResults] = useState<ReadonlyMap<string, RunResult>>(
    () => new Map(),
  );
  const [aiHistory, setAiHistory] = useState<readonly HistoryEntry[]>([]);
  const [aiFindings, setAiFindings] = useState<readonly FindingRecord[]>([]);
  /** 外部のAIエージェントが置いた提案（`ai/proposals/*.md`。T3-8）と、外で変わった回数。 */
  const [aiProposalFiles, setAiProposalFiles] = useState<
    readonly AiProposalFile[]
  >([]);
  const [proposalsVersion, setProposalsVersion] = useState(0);
  /** 自律の実行（T3-9）：走っている回、最後の結果、AI のブランチ。 */
  const [aiAutoRun, setAiAutoRun] = useState<AutoRun | null>(null);
  const [aiAutoResult, setAiAutoResult] = useState<
    (AutoResult & { readonly run: number }) | null
  >(null);
  const [aiAutoError, setAiAutoError] = useState<string | null>(null);
  const [aiBranchState, setAiBranchState] = useState<AiBranches | null>(null);
  const [branchesVersion, setBranchesVersion] = useState(0);
  /** 走っている自律の回の番号（取り消しに使う）。 */
  const autoRunning = useRef<number | null>(null);
  /** 却下した提案（まとまりの id ごと。画面を切り替えても残す）。 */
  const [rejectedProposals, setRejectedProposals] = useState<
    ReadonlyMap<string, ReadonlySet<number>>
  >(() => new Map());
  /**
   * プロットとフローへの支援（T3-10）：採用・却下した分岐案、当てた・閉じた分類案、却下した要約のずれ、要約の
   * ずれ、章の要約の作り直し、ほかの画面からAI実行の画面で開くタスク。
   */
  const [branchHidden, setBranchHidden] = useState<ReadonlySet<string>>(
    () => new Set(),
  );
  const [ideaHidden, setIdeaHidden] = useState<ReadonlySet<string>>(
    () => new Set(),
  );
  const [driftHidden, setDriftHidden] = useState<ReadonlySet<string>>(
    () => new Set(),
  );
  const [drift, setDrift] = useState<readonly SummaryDrift[] | null>(null);
  const [driftPending, setDriftPending] = useState<
    Readonly<Record<string, number>>
  >({});
  const [driftError, setDriftError] = useState<string | null>(null);
  const [driftVersion, setDriftVersion] = useState(0);
  const [summarizeRun, setSummarizeRun] = useState<SummarizeRun | null>(null);
  const [summarizeResult, setSummarizeResult] =
    useState<SummarizeResult | null>(null);
  const [summarizeError, setSummarizeError] = useState<string | null>(null);
  const summarizing = useRef<number | null>(null);
  const [aiScreenRequest, setAiScreenRequest] = useState<{
    readonly task: string;
    readonly seq: number;
  } | null>(null);
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
    aiResults,
    narrow,
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
    aiResults,
    narrow,
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
    setSaveFailed(null);
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
    setVisited(new Set());
    setError(null);
    setNotice(`${opened.title} を開きました`);
    setAssist(opened.assist);
    setEditorSettings(opened.editor);
    setCurrentScene(null);
    setScene(null);
    setBibleRequest(null);
    setAssetRequest(null);
    setSearchRanges(null);
    rangesWanted.current = false;
    setFindAllSpec(null);
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

  /** 保存していない変更の確認のダイアログを出し、答えを待つ（T4-20）。 */
  const askUnsaved = useCallback(
    (ask: UnsavedAsk) =>
      new Promise<UnsavedChoice>((resolve) => {
        setUnsavedError(null);
        setUnsavedSaving(false);
        setUnsavedDialog({ ask, resolve });
      }),
    [],
  );

  /**
   * 作品を閉じる・切り替える・アプリを終える前に、保存していない変更があれば聞く（保存しない・キャンセル・
   * 保存する）。「保存する」は本文を保存し終えてから答える（`chooseUnsaved`）。続けてよければ true。
   */
  const confirmDiscard = useCallback(async () => {
    const { dirty: files, settingsDrafts: drafts } = latest.current;
    if (files.size === 0 && drafts.size === 0) return true;
    const choice = await askUnsaved(
      leaveProjectAsk(files, drafts as ReadonlySet<DraftScreen>),
    );
    return choice !== "cancel";
  }, [askUnsaved]);

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
          case "provenance":
            // 出自の記録が外で変わった：AI の行の印を取り直す（T3-4）。
            setSavedVersion((n) => n + 1);
            break;
          case "proposals":
            // 外部のAIエージェントの提案が変わった：一覧を読み直す（T3-8）。
            setProposalsVersion((n) => n + 1);
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
          // 消すのは解析の失敗の帯だけ（ほかの誤りは、人が閉じるか、次の誤りで替わるまで残す）。
          const failed = parseError.current;
          parseError.current = null;
          if (failed !== null) setError((e) => (e === failed ? null : e));
        });
      } catch (e) {
        whenIdle(() => {
          parseError.current = String(e);
          setError(String(e));
        });
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
    async (
      file: string,
      text: string,
      base: string | null = mergeBases.current.get(file) ?? null,
    ) => {
      // 知らせの途中の競合で覚えた元は、人が直し始めた元としてコアにも渡す（出自の記録。T3-4）。
      const kept = await keepMine(file, text, base);
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
      setSaving((n) => n + 1);
      // 保存の確認の「保存する」が待っているなら、この保存が答える（待つ前から走っていた ⌘S の保存は答えない。
      // その後にエディタが続けて保存する分が答える）。
      const waiter = saveWaiters.current.get(file);
      let ok = false;
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
        setSaveFailed((f) => (f?.file === file ? null : f));
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
          setError(
            result.provenance_problem === null
              ? null
              : `出自の記録（ai/provenance.jsonl）を書けませんでした：${result.provenance_problem}`,
          );
        });
        ok = true;
        return result.text;
      } catch (e) {
        setSaveFailed({
          file,
          message: `${file} を保存できません：${String(e)}`,
        });
        whenIdle(() => setError(String(e)));
        return null;
      } finally {
        setSaving((n) => n - 1);
        if (waiter !== undefined && saveWaiters.current.get(file) === waiter) {
          saveWaiters.current.delete(file);
          waiter(ok);
        }
      }
    },
    [keep, showSummary, whenIdle],
  );

  /**
   * 保存していない本文をすべて保存する（保存の確認の「保存する」。T4-20）。エディタで開いている章はエディタに
   * 保存を頼み（付いた行IDをエディタの本文に入れるため）、ほかの章はそのまま保存する。保存できなければわけを返す。
   */
  const saveAllStory = useCallback(async (): Promise<string | null> => {
    for (const file of [...latest.current.dirty].sort()) {
      const ok =
        file === latest.current.currentFile
          ? await new Promise<boolean>((resolve) => {
              saveWaiters.current.set(file, resolve);
              setSaveRequest((n) => n + 1);
              // エディタが保存を始めなければ（エディタが無いなど）、いつまでも待たない。
              setTimeout(() => {
                if (saveWaiters.current.get(file) !== resolve) return;
                saveWaiters.current.delete(file);
                resolve(false);
              }, SAVE_WAIT_MS);
            })
          : (await onSave(file, buffers.current.get(file) ?? "")) !== null;
      if (!ok)
        return `${file} を保存できませんでした。キャンセルして、本文の画面で確かめてください。`;
    }
    return null;
  }, [onSave]);

  /** 保存の確認で選んだ。「保存する」は保存し終えてから答える（保存できなければダイアログに残す）。 */
  const chooseUnsaved = useCallback(
    (choice: UnsavedChoice) => {
      const open = unsavedDialog;
      if (open === null) return;
      if (choice !== "save") {
        setUnsavedDialog(null);
        open.resolve(choice);
        return;
      }
      setUnsavedSaving(true);
      setUnsavedError(null);
      void saveAllStory().then((problem) => {
        setUnsavedSaving(false);
        if (problem !== null) {
          setUnsavedError(problem);
          return;
        }
        setUnsavedDialog(null);
        open.resolve("save");
      });
    },
    [unsavedDialog, saveAllStory],
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
      // 元は消す前に読んでおく（「自分の変更を残す」でコアに渡す。T3-4）。
      const base = mergeBases.current.get(file) ?? null;
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
          await keep(file, buffers.current.get(file) ?? "", base);
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
      cursorStore.set({ file, offset });
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

  /** 設定DBの画面への頼みの番号（増え続ける。画面は残るので、処理した番号を覚えている。T4-13）。 */
  const bibleSeq = useRef(0);
  /** 設定DBの画面を開いて頼む（D049）。 */
  const requestBible = useCallback((make: (seq: number) => BibleRequest) => {
    bibleSeq.current += 1;
    setBibleRequest(make(bibleSeq.current));
    setScreen("bible");
  }, []);

  /** 未登録の話者を、設定DBの画面の「作る」の欄で登録する（D047、D049）。 */
  const onRegister = useCallback(
    (name: string) =>
      requestBible((seq) => ({ kind: "new", typeId: null, name, seq })),
    [requestBible],
  );

  /** 環境設定で project.toml を書いた後（T4-25）：外で変わったときと同じに、作品名・台詞の括弧・エディタの設定・
   * 補完の材料・解析の結果を入れる（自分で書いた変更は、監視の知らせが来ないため）。 */
  const onProjectWritten = useCallback(
    (result: ProjectSettingsWritten) => {
      if (result.edit.kind !== "saved") return;
      setProject((p) =>
        p === null
          ? null
          : { ...p, title: result.title, brackets: result.brackets },
      );
      setAssist(result.assist);
      setEditorSettings(result.editor);
      showSummary(result.summary, result.checks);
    },
    [showSummary],
  );

  /** 設定DBか変数を書いた後：補完の材料と、解析の結果を入れる。 */
  const onSettingsWritten = useCallback(
    (
      result: {
        readonly assist: AssistData;
        readonly summary: Summary;
        readonly checks: readonly IrDiagnostic[];
      } & SettingsWrittenExtra,
    ) => {
      setAssist(result.assist);
      showSummary(result.summary, result.checks);
      // AI が書いた項目の印と、直した AI の項目の出自を書けなかったとき（T4-3）。
      if (result.ai_fields !== undefined)
        setAiFieldMap(fieldOrigins(result.ai_fields));
      if (
        result.provenance_problem !== undefined &&
        result.provenance_problem !== null
      )
        setError(
          `出自の記録（ai/provenance.jsonl）を書けませんでした：${result.provenance_problem}`,
        );
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
  const leaveSettings = useCallback(
    (go: () => void) => {
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
      void askUnsaved(
        leaveSourceAsk(
          here as DraftScreen,
          "ソース表示に保存していない変更があります。捨てて移りますか？",
        ),
      ).then((choice) => {
        if (choice !== "discard") return;
        // 画面は隠して残すので、その画面を外して作り直し、書きかけを本当に捨てる（T4-13）。
        setVisited((v) => {
          const next = new Set(v);
          next.delete(here);
          return next;
        });
        go();
      });
    },
    [askUnsaved],
  );
  const selectScreen = useCallback(
    (next: Screen) => {
      if (next === latest.current.screen) return;
      leaveSettings(() => {
        blurActive();
        setScreen(next);
      });
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
      leaveSettings(() => {
        blurActive();
        setScreen("preview");
      });
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
      // どの画面でも、エディタの今のカーソル（無ければ今の章の初め）から再生する（T4-14、U10）。
      if (latest.current.screen !== "preview") playAtCursor();
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
        // 本体の窓がソース表示に保存していない変更を持っていれば、捨ててよいかを聞いてから移る（T4-12、U2）。
        leaveSettings(() => showPreviewLine(line));
      }),
      onPreviewWindowClosed(() => {
        setPreviewWindow(false);
        setPreviewMarker(null);
      }),
    ];
    return () => {
      for (const sub of subs) void sub.then((f) => f());
    };
  }, [onPreviewLine, showPreviewLine, leaveSettings]);

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

  /** パレット（⌘K）で選んだものを開く（T4-20）。画面を移るものは、ソース表示の書きかけを聞いてから。 */
  const openFound = useCallback(
    (target: FindTarget) => {
      switch (target.kind) {
        case "command":
          target.command.run();
          return;
        case "scene":
          leaveSettings(() => selectScene(target.scene));
          return;
        case "entry":
          leaveSettings(() =>
            requestBible((seq) => ({ kind: "entry", path: target.path, seq })),
          );
          return;
        case "asset":
          leaveSettings(() => {
            setAssetRequest((r) => ({ id: target.id, seq: (r?.seq ?? 0) + 1 }));
            setScreen("assets");
          });
          return;
      }
    },
    [leaveSettings, selectScene, requestBible],
  );

  /** 検索の範囲を読む（頼んだときの本文を添える）。 */
  const loadRanges = useCallback(() => {
    const sources = new Map(buffers.current);
    return storySearchRanges().then((ranges) => ({ ranges, sources }));
  }, []);
  /** 検索の範囲が要る（検索の欄で「本文だけ」か「すべての章」を選んだ。T4-22）。 */
  const needRanges = useCallback(() => {
    if (rangesWanted.current) return;
    rangesWanted.current = true;
    void loadRanges()
      .then(setSearchRanges)
      .catch(() => {});
  }, [loadRanges]);
  /** 検索の欄を閉じた：すべての章の結果を消し、範囲を読むのをやめる。 */
  const onFindClosed = useCallback(() => {
    setFindAllSpec(null);
    rangesWanted.current = false;
  }, []);
  const onFindAll = useCallback((spec: FindSpec | null) => {
    setFindAllSpec((now) =>
      JSON.stringify(now) === JSON.stringify(spec) ? now : spec,
    );
  }, []);
  // すべての章を探し始めたら、結果を見せる右の欄を出す（閉じていても、狭い窓でも）。
  const findingAll = findAllSpec !== null;
  useEffect(() => {
    if (findingAll) setPanels((p) => showPanel(p, "text", narrow));
  }, [findingAll, narrow]);
  // 本文が変わって解析し直したら、本文の範囲を読み直す（使っているときだけ）。
  const analyzedSummary = project?.summary;
  useEffect(() => {
    if (analyzedSummary === undefined || !rangesWanted.current) return;
    let live = true;
    void loadRanges()
      .then((r) => {
        if (live) setSearchRanges(r);
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [analyzedSummary, loadRanges]);
  /**
   * すべての章の一致（右の欄の「検索の結果」）。保存していない本文も探す。執筆表示では、隠れた行IDの中の一致を
   * 除く（この章の検索の欄の件数とそろえる。表示は今の章のもの）。
   */
  const projectFiles = project?.files;
  const writingView =
    currentFile !== null && (modes.get(currentFile) ?? "writing") === "writing";
  const found = useMemo(() => {
    void analyzedSummary; // 本文が変わったら探し直す。
    if (findAllSpec === null || projectFiles === undefined) return null;
    return findAll(
      projectFiles.map((file) => ({
        file,
        text: buffers.current.get(file) ?? "",
      })),
      findAllSpec,
      new Map(Object.entries(searchRanges?.ranges ?? {})),
      writingView,
    );
  }, [findAllSpec, projectFiles, searchRanges, analyzedSummary, writingView]);

  /** 本文の今の中身（検証の画面の行の番号と該当の行）。 */
  const textOfBuffer = useCallback(
    (file: string) => buffers.current.get(file),
    [],
  );
  /**
   * 検証の画面の行き先：本文ならその行へ、設定DBのエントリ・変数・素材・企画書とメモならその画面へ（検証の画面には
   * 書きかけが無いので、`leaveSettings` を通さない）。
   */
  const onSelectDiagnostic = useCallback(
    (d: IrDiagnostic) => {
      if (d.file.startsWith("bible/entries/")) {
        requestBible((seq) => ({ kind: "entry", path: d.file, seq }));
      } else if (d.file === "variables.yaml") {
        setScreen("variables");
      } else if (d.file === "assets/catalog.yaml") {
        setScreen("assets");
      } else if (d.file === "concept.md" || d.file.startsWith("ideas/")) {
        setScreen("concept");
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

  // クイックオープン（Mod-P）、コマンドパレット（Mod-Shift-P と Mod-K）、集中モード（Mod-Shift-F）、右の欄の
  // 開け閉め（Alt-Mod-B）、作品を開く・新しく作る（Mod-O・Mod-N。作品を開く前も）。エディタの外にフォーカスが
  // あるときも効く。変換中は動かさない。
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      if (event.isComposing || event.keyCode === 229) return;
      if (!(event.metaKey || event.ctrlKey)) return;
      const key = event.key.toLowerCase();
      // 新しく足したキーは、macOS では ⌘ だけで受ける（Ctrl+K・Ctrl+O・Ctrl+N は入力欄の OS のキー）。
      const mod = IS_MAC ? event.metaKey : event.ctrlKey;
      if (
        mod &&
        !event.shiftKey &&
        !event.altKey &&
        (key === "o" || key === "n")
      ) {
        event.preventDefault();
        if (key === "o") void openPicker();
        else void create();
        return;
      }
      if (latest.current.project === null) return;
      // Option を押すと key が別の文字になるので、キーの場所で見る。
      if (mod && event.altKey && !event.shiftKey && event.code === "KeyB") {
        event.preventDefault();
        setPanels((p) =>
          togglePanel(p, latest.current.screen, latest.current.narrow),
        );
      } else if (mod && key === "k" && !event.shiftKey && !event.altKey) {
        event.preventDefault();
        setPalette(true);
      } else if (key === "p" && event.shiftKey) {
        event.preventDefault();
        setPalette(true);
      } else if (key === "p") {
        event.preventDefault();
        setQuickOpen(true);
      } else if (key === "f" && event.shiftKey) {
        event.preventDefault();
        updateViewPrefs((p) => ({ ...p, focus: !p.focus }));
      } else if (!event.shiftKey && !event.altKey) {
        // ⌘1〜⌘9（左端のバーの順）と ⌘,（環境設定）で画面を移る（T4-14、U10）。
        const target = screenForKey(event.key);
        if (target !== null) {
          event.preventDefault();
          selectScreen(target);
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [updateViewPrefs, selectScreen, openPicker, create]);

  // 窓の幅：1280 を境に、右の欄を並べるか重ねるかを変える（T4-17、U06）。
  useEffect(() => {
    const onResize = () => {
      const next = isNarrow(window.innerWidth);
      const was = latest.current.narrow;
      if (next === was) return;
      latest.current.narrow = next;
      setNarrow(next);
      setPanels((p) => resized(p, latest.current.screen, was, next));
    };
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);
  // 右の欄の開け閉めを覚える（広い窓で閉じた画面だけ）。
  const closedPanels = panels.closed;
  useEffect(() => saveClosedPanels(closedPanels), [closedPanels]);
  // 重ねた右の欄は、画面を移ったら閉じる。
  useEffect(() => {
    void screen;
    setPanels(closeDrawer);
  }, [screen]);
  // 重ねた右の欄は、Esc と外を押したときに閉じる（右の欄の開け閉めのボタンは自分で閉じる）。
  const drawerOpen = panels.drawer;
  useEffect(() => {
    if (!drawerOpen) return;
    const onDown = (event: MouseEvent) => {
      if (!(event.target instanceof Element)) return;
      if (event.target.closest(".ctx-column, .tb-panel-toggle") !== null)
        return;
      setPanels(closeDrawer);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      if (event.isComposing || event.keyCode === 229) return;
      setPanels(closeDrawer);
    };
    // フローの図などが押した知らせを止めるので、捕捉の段階で受ける。
    document.addEventListener("mousedown", onDown, true);
    window.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown, true);
      window.removeEventListener("keydown", onKey);
    };
  }, [drawerOpen]);
  // 今のブランチ（タイトルバー）：作品を開いたときと、窓に戻ってきたときに読む。
  const branchRoot = project?.root ?? null;
  useEffect(() => {
    if (branchRoot === null) {
      setBranch(null);
      return;
    }
    let live = true;
    const read = () => {
      aiBranches()
        .then((b) => {
          if (live) setBranch(b.repo ? b.current : null);
        })
        .catch(() => {
          if (live) setBranch(null);
        });
    };
    read();
    window.addEventListener("focus", read);
    return () => {
      live = false;
      window.removeEventListener("focus", read);
    };
  }, [branchRoot]);

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
        const now = new Date();
        setStage2At(
          `${String(now.getHours()).padStart(2, "0")}:${String(now.getMinutes()).padStart(2, "0")}`,
        );
      })
      .catch((e: unknown) => setStage2Error(String(e)))
      .finally(() => setStage2Running(false));
  }, [currentSummary]);
  const checkDiagnostics = useMemo(
    () => [...allDiagnostics, ...(stage2?.diagnostics ?? [])],
    [allDiagnostics, stage2],
  );
  // 変数の画面の取りうる値（T4-24、N9）。
  const variablesStage2 = useMemo(
    () => ({
      report: stage2,
      running: stage2Running,
      discarded: stage2Discarded,
      error: stage2Error,
    }),
    [stage2, stage2Running, stage2Discarded, stage2Error],
  );
  const counts = countBySeverity(allDiagnostics);
  const conflict =
    currentFile === null ? undefined : conflicts.get(currentFile);
  const savedText =
    currentFile === null ? "" : (saved.current.get(currentFile) ?? "");
  // AIが書いた行と項目を取り直す（作品を開いたとき、保存・読み込み直し・設定ファイルが変わったたびに）。
  const projectRoot = project?.root ?? null;
  useEffect(() => {
    void savedVersion;
    void settingsVersion;
    if (projectRoot === null) return;
    let live = true;
    aiLines()
      .then((result) => {
        if (!live) return;
        setAiLineMap(new Map(Object.entries(result.lines)));
        setAiFieldMap(fieldOrigins(result.fields));
      })
      .catch(() => {
        // 読めなければ印を消す（前の作品の印を残さない）。
        if (!live) return;
        setAiLineMap(new Map());
        setAiFieldMap(new Map());
      });
    return () => {
      live = false;
    };
  }, [projectRoot, savedVersion, settingsVersion]);

  // AIの接続先を取り直す（作品を開いたとき、設定ファイルが変わったとき、環境設定で変えたとき。T3-5）。
  useEffect(() => {
    void settingsVersion;
    void aiVersion;
    if (projectRoot === null) {
      setAiState(null);
      return;
    }
    let live = true;
    aiConnections()
      .then((result) => {
        if (!live) return;
        setAiState(result);
        setAiError(null);
      })
      .catch((e: unknown) => {
        if (!live) return;
        setAiState(null);
        setAiError(String(e));
      });
    return () => {
      live = false;
    };
  }, [projectRoot, settingsVersion, aiVersion]);
  /**
   * 走っている回を取り消し、後で届く結果を捨てる（走っていた接続先は「取り消した」にする）。
   * `forget` は結果を忘れる接続先。`all` なら全部を忘れる。
   */
  const dropAiTests = useCallback((forget: readonly string[], all = false) => {
    const running = aiRunning.current;
    aiRunning.current = null;
    aiRun.current += 1;
    if (running !== null) void cancelAi(running);
    setAiTests((m) => {
      if (all) return new Map();
      const next = new Map<string, TestRecord>();
      for (const [name, record] of m) {
        if (forget.includes(name)) continue;
        next.set(
          name,
          record.state.kind === "running"
            ? { ...record, state: { kind: "cancelled" } }
            : record,
        );
      }
      return next;
    });
  }, []);
  // 作品を開き直したら、試した結果を忘れる（走っている回も取り消す）。
  useEffect(() => {
    void projectRoot;
    dropAiTests([], true);
  }, [projectRoot, dropAiTests]);
  // AI実行の画面のタスク（作品を開いたとき、設定ファイル（ai/*.yaml を含む）が変わったとき、接続先を変えたとき）。
  useEffect(() => {
    void settingsVersion;
    void aiVersion;
    if (projectRoot === null) {
      setAiTasks(null);
      return;
    }
    let live = true;
    aiTaskList()
      .then((result) => {
        if (!live) return;
        setAiTasks(result.tasks);
        setAiTaskConnections(result.connections);
        setAiTasksError(null);
      })
      .catch((e: unknown) => {
        if (!live) return;
        setAiTasks(null);
        setAiTasksError(String(e));
      });
    return () => {
      live = false;
    };
  }, [projectRoot, settingsVersion, aiVersion]);
  // 作品を開き直したら（同じフォルダでも）、AI の実行と指摘を忘れる（走っている回は取り消す）。
  useEffect(() => {
    void projectRoot;
    void openSeq;
    if (taskRun.current !== null) void cancelAi(taskRun.current);
    taskRun.current = null;
    setAiRunState(null);
    setAiResults(new Map());
    setAiHistory([]);
    setAiFindings([]);
    setRejectedProposals(new Map());
    if (summarizing.current !== null) void cancelAi(summarizing.current);
    summarizing.current = null;
    setSummarizeRun(null);
    setSummarizeResult(null);
    setSummarizeError(null);
    setBranchHidden(new Set());
    setIdeaHidden(new Set());
    setDriftHidden(new Set());
    setDrift(null);
  }, [projectRoot, openSeq]);
  // 実行の進み具合（届いた文字の数）。
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let live = true;
    void onAiRunProgress((p) => {
      setAiRunState((r) =>
        r !== null && r.run === p.run
          ? { ...r, chars: p.chars, thinking: p.thinking }
          : r,
      );
    }).then((u) => {
      if (live) unlisten = u;
      else u();
    });
    return () => {
      live = false;
      unlisten?.();
    };
  }, []);
  /**
   * コアが書いた章の本文（提案の採用、生成の下書き、自律の書き込み）を、エディタと App の本文に入れる。`before` は
   * 書く前にフロントが知っていた本文（エディタがそれと違えば、置き換えを見送って選ばせる）。
   */
  const applyWrittenText = useCallback(
    (
      file: string,
      text: string,
      before: string,
      summary: Summary,
      checks: readonly IrDiagnostic[],
    ) => {
      saved.current.set(file, text);
      buffers.current.set(file, text);
      mergeBases.current.delete(file);
      setDirty((d) => withoutFiles(d, new Set([file])));
      if (file === latest.current.currentFile)
        replaceText(file, text, before, before);
      setSavedVersion((n) => n + 1);
      showSummary(summary, checks);
    },
    [replaceText, showSummary],
  );
  /** 企画書の下書きを書く（T4-4）。空いている項目にだけ書き、値のある項目は案として返す。 */
  const writeConcept = useCallback(
    async (
      draft: ConceptDraft | null,
      task: string,
      model: string,
    ): Promise<Written> => {
      if (draft === null)
        return { text: "応答から企画書の下書きを読めませんでした", ok: false };
      try {
        const result = await writeConceptDraft(draft, task, model);
        onSettingsWritten(result);
        setSettingsVersion((n) => n + 1);
        return {
          text: fieldsWrittenText(result.edit),
          ok: result.edit.provenance_problem === null,
          fields: [result.edit],
        };
      } catch (e) {
        return { text: String(e), ok: false };
      }
    },
    [onSettingsWritten],
  );
  /** 設定の下書きを書く（T4-5）。`only` があれば、そのエントリにだけ書く。 */
  const writeEntries = useCallback(
    async (
      draft: EntriesDraft | null,
      task: string,
      model: string,
      only: string | null,
    ): Promise<Written> => {
      if (draft === null)
        return { text: "応答から設定の下書きを読めませんでした", ok: false };
      try {
        const result = await writeEntriesDraft(draft, task, model, only);
        onSettingsWritten(result);
        setSettingsVersion((n) => n + 1);
        return {
          text: entriesWrittenText(result.edit),
          ok: result.edit.provenance_problem === null,
          fields: result.edit.files,
          created: result.edit.created,
          notes: result.edit.notes,
        };
      } catch (e) {
        return { text: String(e), ok: false };
      }
    },
    [onSettingsWritten],
  );
  /** 企画書・エントリの下書きの案の決め方（採用・却下。画面の中だけ。T4-4）。 */
  const [fieldDecisions, setFieldDecisions] = useState<
    ReadonlyMap<string, FieldDecision>
  >(() => new Map());
  /** 案を採用する（人の操作。T4-4）。誤りの文を返す。 */
  const adoptField = useCallback(
    async (
      run: number | string,
      proposal: FieldProposal,
      written: FieldsWritten,
      task: string,
      model: string,
    ): Promise<string | null> => {
      try {
        const result = await adoptFieldProposal(
          proposal,
          written.text,
          task,
          model,
        );
        switch (result.edit.kind) {
          case "saved":
            onSettingsWritten(result);
            setSettingsVersion((n) => n + 1);
            setFieldDecisions((m) =>
              new Map(m).set(fieldProposalKey(run, proposal), "adopted"),
            );
            return null;
          case "conflict":
            return "外でこの項目が変わっていたので、書きませんでした（今の中身を見てから、もう一度下書きを作ります）";
          case "unsupported":
            return result.edit.message;
          default:
            return null;
        }
      } catch (e) {
        return String(e);
      }
    },
    [onSettingsWritten],
  );
  const rejectField = useCallback(
    (run: number | string, proposal: FieldProposal) => {
      setFieldDecisions((m) =>
        new Map(m).set(fieldProposalKey(run, proposal), "rejected"),
      );
    },
    [],
  );
  /** 生成の下書きをスタブのシーンに書き込む（T3-9）。結果の文と、書けたかを返す。 */
  const writeDraft = useCallback(
    async (
      scene: string,
      body: readonly string[],
      task: string,
      model: string,
    ): Promise<Written> => {
      const file = latest.current.project?.summary.scenes.find(
        (s) => s.id === scene,
      )?.file;
      if (file === undefined)
        return { text: `シーン ${scene} が見つかりません`, ok: false };
      if (body.length === 0)
        return { text: "下書きの本文がありませんでした", ok: false };
      if (latest.current.conflicts.has(file))
        return {
          text: `${file} は外部の変更とぶつかっています。選んでから実行します`,
          ok: false,
        };
      const text = buffers.current.get(file);
      if (text === undefined)
        return { text: `${file} が開かれていません`, ok: false };
      const result = await writeAiDraft(
        file,
        scene,
        body,
        task,
        model,
        text,
        saved.current.get(file) ?? null,
      );
      switch (result.kind) {
        case "saved":
          applyWrittenText(
            file,
            result.text,
            text,
            result.summary,
            result.checks,
          );
          return {
            text:
              result.provenance_problem === null
                ? `${scene} に下書きを書き込みました（${result.ids.length}行。status: ai-draft）`
                : `${scene} に下書きを書き込みましたが、出自を記録できませんでした：${result.provenance_problem}`,
            ok: true,
          };
        case "conflict":
          setConflicts((map) =>
            new Map(map).set(file, { deleted: result.deleted }),
          );
          return {
            text: `${file} は外部で変更されていたので、書き込んでいません`,
            ok: false,
          };
        case "refused":
          return { text: result.reason, ok: false };
      }
    },
    [applyWrittenText],
  );
  /**
   * AI が書いた結果（提案ノードの採用、ルートの骨格。T3-10）を、エディタと App の本文に入れる。新しい章なら一覧に
   * 足す。
   */
  const applyPlotWritten = useCallback(
    (result: PlotWriteResult) => {
      if (result.kind === "saved") {
        const file = result.file;
        if (!(latest.current.project?.files.includes(file) ?? true)) {
          buffers.current.set(file, "");
          saved.current.set(file, "");
          setProject((p) =>
            p === null ? null : { ...p, files: withFile(p.files, file) },
          );
        }
        const before = buffers.current.get(file) ?? "";
        applyWrittenText(
          file,
          result.text,
          before,
          result.summary,
          result.checks,
        );
      }
    },
    [applyWrittenText],
  );
  /**
   * ルートの骨格を書く先の問題（T3-10）。LLM を呼ぶ前にも確かめる（書けないと分かっているのに送らない）。無ければ
   * null。
   */
  const skeletonProblem = useCallback(
    (target: SkeletonTarget | undefined): string | null => {
      if (target === undefined) return "書く章を選んでいません";
      if (target.route === null) return "対象のルートを入れていません";
      if (autoRunning.current !== null)
        return "自律で実行しています。終わってから書きます";
      const file = target.file;
      if (file === null) {
        const name = (target.newChapter ?? "").trim();
        if (name === "") return "書く章を選んでいません";
        const path = `story/${name.endsWith(".scn") ? name : `${name}.scn`}`;
        if (latest.current.project?.files.includes(path) === true)
          return `同じ名前の章（${path}）があります`;
        return null;
      }
      if (latest.current.dirty.has(file))
        return `${file} に保存していない変更があります。保存してから実行します`;
      if (latest.current.conflicts.has(file))
        return `${file} は外部の変更とぶつかっています。選んでから実行します`;
      return null;
    },
    [],
  );
  /** ルートの骨格を書く（T3-10）。結果の文と、書けたかを返す。 */
  const writeSkeleton = useCallback(
    async (
      scenes: readonly SkeletonScene[],
      model: string,
      task: string,
      target: SkeletonTarget | undefined,
    ): Promise<Written> => {
      if (scenes.length === 0)
        return { text: "足すスタブのシーンを読めませんでした", ok: false };
      const problem = skeletonProblem(target);
      if (problem !== null || target === undefined)
        return { text: problem ?? "書く章を選んでいません", ok: false };
      const file = target.file;
      try {
        const result = await writeAiSkeleton(
          file,
          target.newChapter,
          scenes,
          target.route,
          task,
          model,
        );
        applyPlotWritten(result);
        if (result.kind === "conflict" && file !== null)
          setConflicts((map) =>
            new Map(map).set(file, { deleted: result.deleted }),
          );
        const ids = scenes.map((s) => s.id).join("、");
        return {
          text: plotWrittenText(
            result,
            `${scenes.length}シーン（${ids}）を足しました`,
          ),
          ok: result.kind === "saved",
        };
      } catch (e) {
        return { text: String(e), ok: false };
      }
    },
    [applyPlotWritten, skeletonProblem],
  );
  const runTask = useCallback(
    (
      info: AiTaskInfo,
      scene: string | null,
      digest: string | null,
      target?: SkeletonTarget,
    ) => {
      if (taskRun.current !== null) return;
      aiRun.current += 1;
      const run = aiRun.current;
      const task = info.task;
      // ルートの骨格は、書く先を送る前に確かめる（書けないのに送らない）。
      const blocked = isSkeleton(task) ? skeletonProblem(target) : null;
      if (blocked !== null) {
        setAiResults((m) =>
          new Map(m).set(task.id, {
            kind: "failed",
            run,
            scene,
            message: blocked,
          }),
        );
        return;
      }
      taskRun.current = run;
      taskRunTarget.current = { task: task.id, scene };
      const at = new Date().toLocaleTimeString("ja-JP", {
        hour: "2-digit",
        minute: "2-digit",
      });
      setAiRunState({ run, task: task.id, scene, chars: 0, thinking: false });
      setAiHistory((h) => [
        { run, taskName: task.name, at, status: "実行中" },
        ...h,
      ]);
      const finish = (result: RunResult, findings: number) => {
        setAiResults((m) => new Map(m).set(task.id, result));
        setAiHistory((h) =>
          h.map((e) =>
            e.run === run
              ? { ...e, status: historyStatus(result, findings, task.output) }
              : e,
          ),
        );
      };
      runAiTask(task.id, scene, run, digest, target?.route ?? null)
        .then(async (outcome) => {
          if (taskRun.current !== run) return;
          // 生成は、下書きをスタブに書き込む・ルートの骨格のスタブを足す（人が書いた行は変えない）。企画書と設定の
          // 下書きは空いている項目にだけ書き、値のある項目は案にする（T4-4・T4-5）。
          const written =
            task.output === "concept"
              ? await writeConcept(outcome.concept, task.id, outcome.model)
              : task.output === "entries"
                ? await writeEntries(
                    outcome.entries,
                    task.id,
                    outcome.model,
                    target?.route ?? null,
                  )
                : isSkeleton(task)
                  ? await writeSkeleton(
                      outcome.skeleton,
                      outcome.model,
                      task.id,
                      target,
                    )
                  : task.level === "generate" && scene !== null
                    ? await writeDraft(
                        scene,
                        outcome.draft,
                        task.id,
                        outcome.model,
                      )
                    : null;
          if (taskRun.current !== run) return;
          taskRun.current = null;
          setAiRunState(null);
          const source = `${outcome.connection} · ${outcome.model}`;
          const records: FindingRecord[] = outcome.findings.map(
            (finding, i) => ({
              id: `${run}-${i}`,
              run,
              task: task.id,
              taskName: task.name,
              target: scene,
              source,
              finding,
            }),
          );
          // 変換中は画面を変えない（変換が終わってから入れる）。
          whenIdle(() =>
            setAiFindings((old) => mergeFindings(old, records, task.id, scene)),
          );
          finish(
            {
              kind: "ok",
              run,
              scene,
              summary: outcome.summary,
              summaryProblem: outcome.summary_problem,
              proposals: outcome.proposals,
              model: outcome.model,
              written: written?.text ?? null,
              wroteOk: written?.ok ?? false,
              branches: outcome.branches,
              skeleton: outcome.skeleton,
              ideas: outcome.ideas,
              fields: written?.fields ?? [],
              created: written?.created ?? [],
              notes: written?.notes ?? [],
              unread: outcome.unread,
              tokens: tokensText(outcome.completion.usage),
            },
            records.length,
          );
        })
        .catch((e: unknown) => {
          if (taskRun.current !== run) return;
          taskRun.current = null;
          setAiRunState(null);
          finish({ kind: "failed", run, scene, message: String(e) }, 0);
        });
    },
    [
      whenIdle,
      writeDraft,
      writeSkeleton,
      writeConcept,
      writeEntries,
      skeletonProblem,
    ],
  );
  /** 骨格を書き直している（二度押しで同じ骨格を2回書かない）。 */
  const rewriting = useRef(false);
  /** ルートの骨格を、LLM を呼ばずに書き直す（前の回の結果の骨格。T3-10）。 */
  const rewriteSkeleton = useCallback(
    async (info: AiTaskInfo, target: SkeletonTarget) => {
      const result = latest.current.aiResults.get(info.task.id);
      if (result?.kind !== "ok" || rewriting.current) return;
      rewriting.current = true;
      const written = await writeSkeleton(
        result.skeleton,
        result.model,
        info.task.id,
        target,
      ).finally(() => {
        rewriting.current = false;
      });
      setAiResults((m) => {
        const now = m.get(info.task.id);
        return now?.kind === "ok" && now.run === result.run
          ? new Map(m).set(info.task.id, {
              ...now,
              written: written.text,
              wroteOk: written.ok,
            })
          : m;
      });
    },
    [writeSkeleton],
  );
  /** 提案ノードを採用する（人の操作。T3-10）。結果の文を返す。 */
  const adoptBranch = useCallback(
    async (
      proposal: BranchProposal,
      key: string,
      model: string,
      task: string,
    ): Promise<string> => {
      const file = latest.current.project?.summary.scenes.find(
        (s) => s.id === proposal.from,
      )?.file;
      if (file !== undefined && latest.current.dirty.has(file))
        return `${file} に保存していない変更があります。保存してから採用します`;
      if (file !== undefined && latest.current.conflicts.has(file))
        return `${file} は外部の変更とぶつかっています。選んでから採用します`;
      try {
        const result = await adoptBranchProposal(proposal, task, model);
        applyPlotWritten(result);
        if (result.kind === "saved")
          setBranchHidden((h) => new Set(h).add(key));
        if (result.kind === "conflict" && file !== undefined)
          setConflicts((map) =>
            new Map(map).set(file, { deleted: result.deleted }),
          );
        return plotWrittenText(
          result,
          `「${proposal.label}」を ${proposal.from} に足しました`,
        );
      } catch (e) {
        return String(e);
      }
    },
    [applyPlotWritten],
  );
  // 要約のずれ（T3-10）：AI実行の画面を開いているとき、本文や要約が変わったときに取り直す。
  useEffect(() => {
    void savedVersion;
    void driftVersion;
    if (projectRoot === null || screen !== "ai") return;
    let live = true;
    summaryDrift()
      .then((d) => {
        if (!live) return;
        setDrift(d.drift);
        setDriftPending(d.pending);
        setDriftError(null);
      })
      .catch((e: unknown) => {
        if (live) setDriftError(String(e));
      });
    return () => {
      live = false;
    };
  }, [projectRoot, screen, savedVersion, driftVersion]);
  /** 要約のずれを summary に書く（T3-10）。誤りの文を返す。 */
  const adoptDrift = useCallback(
    async (d: SummaryDrift): Promise<string | null> => {
      const text = buffers.current.get(d.file);
      if (text === undefined) return `${d.file} が開かれていません`;
      if (latest.current.conflicts.has(d.file))
        return `${d.file} は外部の変更とぶつかっています。選んでから書きます`;
      try {
        const result = await setSceneSummary(
          d.file,
          d.scene,
          d.proposed,
          text,
          saved.current.get(d.file) ?? null,
        );
        switch (result.kind) {
          case "saved":
            applyWrittenText(
              d.file,
              result.text,
              text,
              result.summary,
              result.checks,
            );
            setDriftVersion((n) => n + 1);
            return result.cache_problem === null
              ? null
              : `summary は書きましたが、要約のキャッシュを書けませんでした：${result.cache_problem}`;
          case "conflict":
            setConflicts((map) =>
              new Map(map).set(d.file, { deleted: result.deleted }),
            );
            return `${d.file} は外部で変更されていたので、書いていません`;
          case "refused":
            return result.reason;
        }
      } catch (e) {
        return String(e);
      }
    },
    [applyWrittenText],
  );
  /** 章のシーンの要約をまとめて作り直す（T3-10）。 */
  const runSummarize = useCallback(
    (task: string, file: string, confirmed: readonly string[]) => {
      if (summarizing.current !== null) return;
      aiRun.current += 1;
      const run = aiRun.current;
      summarizing.current = run;
      setSummarizeError(null);
      setSummarizeResult(null);
      setSummarizeRun({ run, file, scene: null, done: 0, total: 0 });
      runAiSummarize(task, file, run, confirmed)
        .then((result) => {
          if (summarizing.current !== run) return;
          setSummarizeResult(result);
        })
        .catch((e: unknown) => {
          if (summarizing.current === run) setSummarizeError(String(e));
        })
        .finally(() => {
          if (summarizing.current !== run) return;
          summarizing.current = null;
          setSummarizeRun(null);
          setDriftVersion((n) => n + 1);
        });
    },
    [],
  );
  const cancelSummarize = useCallback(() => {
    const run = summarizing.current;
    if (run !== null) void cancelAi(run);
  }, []);
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let live = true;
    void onAiSummarizeProgress((p) => {
      setSummarizeRun((r) =>
        r !== null && r.run === p.run
          ? { ...r, scene: p.scene, done: p.done, total: p.total }
          : r,
      );
    }).then((u) => {
      if (live) unlisten = u;
      else u();
    });
    return () => {
      live = false;
      unlisten?.();
    };
  }, []);
  // 自律の進み具合と、自律が章に書き込んだ本文（T3-9）。
  useEffect(() => {
    const offs: (() => void)[] = [];
    let live = true;
    const keep = (u: () => void) => {
      if (live) offs.push(u);
      else u();
    };
    void onAiAutoProgress((p) => {
      setAiAutoRun((r) =>
        r !== null && r.run === p.run ? applyAutoEvent(r, p.event) : r,
      );
    }).then(keep);
    void onAiDraftWritten((w) => {
      // コアが書く前の本文（エディタがまだ送っていない編集があれば、置き換えずに選ばせる）。
      const before = w.before ?? buffers.current.get(w.file) ?? w.text;
      whenIdle(() =>
        applyWrittenText(w.file, w.text, before, w.summary, w.checks),
      );
    }).then(keep);
    return () => {
      live = false;
      for (const off of offs) off();
    };
  }, [applyWrittenText, whenIdle]);
  /** 自律で章のスタブを下書きにする。 */
  const runAuto = useCallback(
    (
      info: AiTaskInfo,
      file: string,
      branch: boolean,
      confirmed: readonly string[],
      stages: AutoStages,
    ) => {
      if (latest.current.dirty.size > 0) {
        setAiAutoError(
          "保存していない変更があります。保存してから自律で実行します",
        );
        return;
      }
      aiRun.current += 1;
      const run = aiRun.current;
      autoRunning.current = run;
      const task = info.task;
      setAiAutoError(null);
      setAiAutoResult(null);
      setAiAutoRun({
        run,
        task: task.id,
        file,
        planned: [],
        current: null,
        done: [],
        tokens: 0,
        budget: task.budget_tokens,
        stage: null,
        stagesDone: [],
      });
      const at = new Date().toLocaleTimeString("ja-JP", {
        hour: "2-digit",
        minute: "2-digit",
      });
      setAiHistory((h) => [
        { run, taskName: task.name, at, status: "自律で実行中" },
        ...h,
      ]);
      const settle = (status: string) =>
        setAiHistory((h) =>
          h.map((e) => (e.run === run ? { ...e, status } : e)),
        );
      runAiAuto(task.id, file, run, branch, confirmed, stages)
        .then((result) => {
          autoRunning.current = null;
          setAiAutoResult({ ...result, run });
          // 企画と設定の段で書いたら、設定を読み直す（T4-6）。
          if (result.settings !== null) {
            onSettingsWritten(result.settings);
            setSettingsVersion((n) => n + 1);
          }
          setAiAutoRun(null);
          setBranchesVersion((n) => n + 1);
          const written = result.outcome.scenes.filter(
            (s) => s.status === "written",
          ).length;
          settle(
            result.outcome.stopped === null
              ? `${written}シーンを下書き`
              : `${written}シーンを下書き（途中で止めた）`,
          );
        })
        .catch((e: unknown) => {
          autoRunning.current = null;
          setAiAutoError(String(e));
          setAiAutoRun(null);
          settle("失敗");
        });
    },
    [onSettingsWritten],
  );
  const cancelAuto = useCallback(() => {
    // 次の段の合間で止まる（結果は届いたときに見せる）。
    if (autoRunning.current !== null) void cancelAi(autoRunning.current);
  }, []);
  // AI のブランチ（AI の画面を開いたとき、自律の後、取り込み・破棄の後）。
  useEffect(() => {
    void branchesVersion;
    if (projectRoot === null || screen !== "ai") return;
    let live = true;
    aiBranches()
      .then((b) => {
        if (live) setAiBranchState(b);
      })
      .catch(() => {
        if (live) setAiBranchState(null);
      });
    return () => {
      live = false;
    };
  }, [projectRoot, branchesVersion, screen]);
  const cancelTaskRun = useCallback(() => {
    const run = taskRun.current;
    const target = taskRunTarget.current;
    if (run === null || target === null) return;
    taskRun.current = null;
    taskRunTarget.current = null;
    void cancelAi(run);
    setAiRunState(null);
    setAiResults((m) =>
      new Map(m).set(target.task, {
        kind: "cancelled",
        run,
        scene: target.scene,
      }),
    );
    setAiHistory((h) =>
      h.map((e) => (e.run === run ? { ...e, status: "取り消し" } : e)),
    );
  }, []);
  /** 指摘の行へ移る（行IDで今の本文の中を探す。無ければ実行したときの位置）。 */
  const goToFinding = useCallback((r: FindingRecord) => {
    const file = r.finding.file;
    if (file === null) return;
    const text = buffers.current.get(file);
    if (text === undefined) return;
    const offset = findingOffset(text, r.finding);
    setScreen("text");
    setCurrentFile(file);
    const p = latest.current.project;
    if (p !== null && file !== latest.current.currentFile)
      setAnalysis(analysisFor(p.summary, file, text));
    if (offset !== null) setFocus((f) => ({ offset, seq: (f?.seq ?? 0) + 1 }));
  }, []);
  // 提案ファイルの一覧（作品を開いたとき、外で変わったとき）。
  useEffect(() => {
    void proposalsVersion;
    void openSeq;
    if (projectRoot === null) {
      setAiProposalFiles([]);
      return;
    }
    let live = true;
    readProposalFiles()
      .then((files) => {
        if (live) setAiProposalFiles(files);
      })
      .catch(() => {
        if (live) setAiProposalFiles([]);
      });
    return () => {
      live = false;
    };
  }, [projectRoot, proposalsVersion, openSeq]);
  /**
   * 提案を採用する（人の操作。T3-8）：章ごとに、今の本文に採用できる行だけを当てて保存し（コア）、エディタの本文を
   * 置き換える。外部の変更とぶつかっている章には採用しない。結果の文を返す。
   */
  const adopt = useCallback(
    async (
      set: ProposalSet,
      views: readonly AiProposalView[],
    ): Promise<string> => {
      const parts: string[] = [];
      for (const [file, proposals] of proposalsByFile(views)) {
        if (latest.current.conflicts.has(file)) {
          parts.push(
            `${file} は外部の変更とぶつかっています。選んでから採用します`,
          );
          continue;
        }
        const text = buffers.current.get(file);
        if (text === undefined) continue;
        const result = await adoptProposals(
          file,
          text,
          saved.current.get(file) ?? null,
          proposals,
          set.task,
          set.model,
        );
        if (result.kind === "saved") {
          saved.current.set(file, result.text);
          buffers.current.set(file, result.text);
          mergeBases.current.delete(file);
          setDirty((d) => withoutFiles(d, new Set([file])));
          if (file === latest.current.currentFile)
            replaceText(file, result.text, text, text);
          setSavedVersion((n) => n + 1);
          showSummary(result.summary, result.checks);
          parts.push(
            `${file}：${result.applied.length}行を採用して保存しました`,
          );
          if (result.provenance_problem !== null)
            parts.push(
              `出自の記録（ai/provenance.jsonl）を書けませんでした：${result.provenance_problem}`,
            );
        } else if (result.kind === "conflict") {
          setConflicts((map) =>
            new Map(map).set(file, { deleted: result.deleted }),
          );
          parts.push(
            `${file} は外部で${result.deleted ? "削除" : "変更"}されていたので、採用していません`,
          );
        }
        for (const s of result.skipped)
          parts.push(`^${s.line} は採用しませんでした：${s.reason}`);
      }
      return parts.join("\n");
    },
    [replaceText, showSummary],
  );
  const toggleReject = useCallback((setId: string, index: number) => {
    setRejectedProposals((map) => {
      const next = new Set(map.get(setId) ?? []);
      if (next.has(index)) next.delete(index);
      else next.add(index);
      return new Map(map).set(setId, next);
    });
  }, []);
  /** 提案の行へ移る（行IDで今の本文の中を探す）。 */
  const goToProposal = useCallback((view: AiProposalView) => {
    const file = view.file;
    if (file === null) return;
    const text = buffers.current.get(file);
    if (text === undefined) return;
    const offset = locateLine(text, view.proposal.line, view.span?.[0] ?? -1);
    setScreen("text");
    setCurrentFile(file);
    const p = latest.current.project;
    if (p !== null && file !== latest.current.currentFile)
      setAnalysis(analysisFor(p.summary, file, text));
    if (offset !== null) setFocus((f) => ({ offset, seq: (f?.seq ?? 0) + 1 }));
  }, []);
  const dismissFinding = useCallback((id: string) => {
    setAiFindings((list) => list.filter((r) => r.id !== id));
  }, []);
  const findingMap = useMemo(() => findingLines(aiFindings), [aiFindings]);
  const runAiTest = useCallback(
    (name: string) => {
      const connection = aiState?.connections.find((c) => c.name === name);
      if (connection === undefined) return;
      const target = testTarget(connection);
      // 前の設定で走っている回が残っていれば（project.toml が途中で変わったなど）、取り消す。
      if (aiRunning.current !== null) void cancelAi(aiRunning.current);
      aiRun.current += 1;
      const run = aiRun.current;
      aiRunning.current = run;
      const settle = (state: TestState) => {
        if (aiRun.current !== run) return;
        aiRunning.current = null;
        setAiTests((m) => new Map(m).set(name, { target, state }));
      };
      setAiTests((m) =>
        new Map(m).set(name, { target, state: { kind: "running" } }),
      );
      testAiConnection(name, run)
        .then((result) => settle({ kind: "ok", result }))
        .catch((e: unknown) => settle({ kind: "failed", message: String(e) }));
    },
    [aiState],
  );
  // 取り消し：画面ではすぐに取り消したことにし、後で届く結果は捨てる。
  const cancelTest = useCallback(() => dropAiTests([]), [dropAiTests]);
  /** 接続先の設定を変えた：取り直し、その接続先の試した結果を忘れる（走っている回も取り消す）。 */
  const aiChanged = useCallback(
    (names: readonly string[]) => {
      setAiVersion((n) => n + 1);
      dropAiTests(names);
    },
    [dropAiTests],
  );
  /** 同じ行き先の接続先すべて（キーと許可は行き先ごとなので、どれの結果も古くなる）。 */
  const namesOfEndpoint = (endpoint: string): string[] =>
    (aiState?.connections ?? [])
      .filter((c) => c.endpoint === endpoint)
      .map((c) => c.name);

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
      keys: keyForScreen(target) ?? undefined,
      target,
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
      screens: ["text"],
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
      go("ai", "AI実行", "ai run tasks"),
      go("export", "出力", "export"),
      go("preferences", "環境設定", "preferences settings connection"),
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
    ];
    // 表示の切り替え（本文の画面で、書く操作の後に並べる）。
    const view: Command[] = [
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
        screens: ["text"],
        run: () => updateViewPrefs((p) => ({ ...p, gothic: !p.gothic })),
      },
    ];
    const file = currentFile;
    if (file === null) return [...list, ...view];
    return [
      ...list,
      {
        id: "play-here",
        label: "ここから再生",
        keywords: "play here",
        keys: "F5",
        screens: ["text"],
        run: () => playAtCursor(),
      },
      {
        id: "rename",
        label: "名前を変える（カーソルのところ）",
        keywords: "rename F2",
        keys: "F2",
        screens: ["text"],
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
        screens: ["text"],
        run: () => toggleMode(file),
      },
      ...view,
      {
        id: "delete-chapter",
        label: `章を削除（${file}）`,
        keywords: "delete chapter",
        screens: ["text"],
        run: () => void onDeleteChapter(file),
      },
    ];
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

  /** 集中モードで左端のバーを隠すのは、本文の画面だけ（ほかの画面では移れなくなるため。T4-13、U16）。 */
  const focusing = viewPrefs.focus && screen === "text";

  const mixedLineBreak = useMemo(
    () => (hasMixedLineBreaks(savedText) ? lineBreakOf(savedText) : null),
    [savedText],
  );

  // プロットとフローへの支援（T3-10）に使うタスク。組み込みの名前のタスク、無ければ出力の形が合うもの。
  const branchTask = findTask(
    aiTasks,
    "branch_ideas",
    (t) => t.level === "propose" && t.output === "proposals",
  );
  const questionTask = findTask(aiTasks, "concept_questions", () => false);
  const sortTask = findTask(aiTasks, "idea_sort", (t) => t.output === "ideas");
  const skeletonTask = findTask(aiTasks, "route_skeleton", isSkeleton);
  const conceptDraftTask = findTask(
    aiTasks,
    "draft_concept",
    (t) => t.output === "concept",
  );
  const conceptDraftResult =
    conceptDraftTask === null
      ? undefined
      : aiResults.get(conceptDraftTask.task.id);
  const entriesTask = findTask(
    aiTasks,
    "draft_entries",
    (t) => t.output === "entries",
  );
  const entriesResult =
    entriesTask === null ? undefined : aiResults.get(entriesTask.task.id);
  /** 設定の下書き（T4-5）：対象のエントリ（null ならまとめて）で実行する。 */
  const entriesRunner = (target: string | null): TaskRunner => ({
    info: entriesTask,
    running: aiRunState,
    onPrepare: (task, scene) => prepareAiTask(task, scene, target),
    onRun: (info, scene, digest) =>
      runTask(
        info,
        scene,
        digest,
        target === null
          ? undefined
          : { route: target, file: null, newChapter: null },
      ),
    onCancel: cancelTaskRun,
  });
  /** ほかの画面から1つのタスクを実行する（実行はAI実行の画面と同じ）。 */
  const runnerOf = (info: AiTaskInfo | null): TaskRunner => ({
    info,
    running: aiRunState,
    onPrepare: prepareAiTask,
    onRun: runTask,
    onCancel: cancelTaskRun,
  });
  const sortResult =
    sortTask === null ? undefined : aiResults.get(sortTask.task.id);
  const ideaSuggestions =
    sortResult?.kind === "ok"
      ? sortResult.ideas
          .map((suggestion, i) => ({
            key: resultKey(sortResult.run, i),
            suggestion,
          }))
          .filter(({ key }) => !ideaHidden.has(key))
      : [];

  /** AI の実行中の数（AI実行・自律・要約の作り直し）。タイトルバーの AI の様子と、左端のバーの点。 */
  const aiActive =
    (aiRunState !== null ? 1 : 0) +
    (aiAutoRun !== null ? 1 : 0) +
    (summarizeRun !== null ? 1 : 0);
  const ai =
    project === null || aiState === null
      ? null
      : aiStatus(aiState.connections, aiTests, aiActive);
  /** 右の欄の出し方（今の画面）。集中モードの本文には右の欄が無い。 */
  const focusText = screen === "text" && viewPrefs.focus;
  const panel = focusText ? "hidden" : panelMode(screen, narrow, panels);
  const panelAvailable =
    project !== null && CONTEXT_SCREENS.has(screen) && !focusText;
  /** 「このシーンから」のシーン（フローは選んだシーン、ほかは本文のカーソルのシーン）。 */
  const playScene = screen === "flow" ? flowScene : currentScene;
  const sceneToPlay =
    playScene === null
      ? null
      : {
          scene: playScene,
          run: () =>
            sendPreview((seq) => ({ seq, kind: "scene", scene: playScene })),
        };
  /** 再生：本文ではカーソルの位置から、フローでは選んだシーンから、ほかの画面では最初から（U02）。 */
  const playStart = () => sendPreview((seq) => ({ seq, kind: "start" }));
  const play =
    project === null
      ? null
      : screen === "text" && currentFile !== null
        ? { title: "カーソルの位置から再生", run: playAtCursor }
        : screen === "flow" && flowScene !== null
          ? {
              title: `選んだシーン（${flowScene}）から再生`,
              run: () =>
                sendPreview((seq) => ({
                  seq,
                  kind: "scene",
                  scene: flowScene,
                })),
            }
          : { title: "最初から再生", run: playStart };
  /** ステータスバーの左（U05）。本文・検証・AI実行・環境設定は App が作り、ほかは画面が出す。 */
  const currentSceneSummary =
    currentScene === null
      ? undefined
      : project?.summary.scenes.find((s) => s.id === currentScene);
  // 検索の欄に渡す、この章の本文の範囲と今のシーン（T4-22）。
  const findScene =
    currentScene === null || project === null
      ? null
      : sceneRange(project.summary.scenes, currentScene);
  const findSceneStart =
    findScene !== null && findScene.file === currentFile
      ? findScene.start
      : null;
  const findSceneEnd =
    findScene !== null && findScene.file === currentFile ? findScene.end : null;
  const findBodyRanges =
    currentFile === null ? undefined : searchRanges?.ranges[currentFile]?.text;
  const findBodySource =
    currentFile === null ? undefined : searchRanges?.sources.get(currentFile);
  // シーンの範囲は、要約を作った本文（要約と一緒に見せた診断の本文。送った後、結果が届くまでの buffers ではない）で
  // 求めたもの。
  const findSceneSource =
    analysis !== null && analysis.file === currentFile
      ? analysis.text
      : undefined;
  const editorFind = useMemo(
    () => ({
      body:
        findBodyRanges === undefined || findBodySource === undefined
          ? null
          : { ranges: findBodyRanges, source: findBodySource },
      scene:
        findSceneStart === null ||
        findSceneEnd === null ||
        findSceneSource === undefined
          ? null
          : {
              start: findSceneStart,
              end: findSceneEnd,
              source: findSceneSource,
            },
      onFindAll,
      onNeedRanges: needRanges,
      onClosed: onFindClosed,
    }),
    [
      findBodyRanges,
      findBodySource,
      findSceneStart,
      findSceneEnd,
      findSceneSource,
      onFindAll,
      needRanges,
      onFindClosed,
    ],
  );
  const statusItems: StatusItems =
    project === null
      ? []
      : screen === "text"
        ? textStatus({
            mode: currentFile === null ? null : mode,
            scene:
              currentSceneSummary === undefined
                ? null
                : {
                    id: currentSceneSummary.id,
                    title: currentSceneSummary.title,
                    chars: formatChars(currentSceneSummary.chars),
                  },
            file: currentFile,
            fileChars:
              currentFile === null
                ? null
                : formatChars(fileChars(project.summary.scenes, currentFile)),
            route: routeStat === null ? null : routeStatText(routeStat),
          })
        : screen === "check"
          ? checkStatus({
              stage2: stage2Running
                ? "running"
                : stage2 !== null
                  ? "done"
                  : stage2Discarded
                    ? "stale"
                    : "none",
              counts,
            })
          : screen === "ai"
            ? aiRunStatus({
                running: aiActive,
                pending: aiBranchState?.branches.length ?? 0,
                history: aiHistory.length,
              })
            : screen === "preferences"
              ? preferencesStatus()
              : (screenStatus.get(screen) ?? [
                  { text: SCREEN_LABEL[screen], strong: true },
                ]);

  return (
    <StatusContext.Provider value={publishStatus}>
      <AskUnsavedContext.Provider value={askUnsaved}>
        <div className={narrow ? "app is-narrow" : "app"} data-panel={panel}>
          <TitleBar
            title={project?.title ?? null}
            branch={branch}
            recent={recent}
            onOpen={() => void openPicker()}
            onCreate={() => void create()}
            onOpenRecent={(path) => void open(path)}
            onReveal={() => {
              revealProject().catch((e: unknown) => setError(String(e)));
            }}
            onProjectSettings={() => selectScreen("preferences")}
            onSearch={() => setPalette(true)}
            ai={ai}
            onOpenAi={() => selectScreen("ai")}
            onPreferences={() => selectScreen("preferences")}
            play={
              play === null
                ? null
                : {
                    ...play,
                    fromStart: playStart,
                    fromScene: sceneToPlay,
                  }
            }
            panel={{
              shown: panel !== "hidden",
              available: panelAvailable,
              onToggle: () => setPanels((p) => togglePanel(p, screen, narrow)),
            }}
          />
          {error !== null && (
            <div className="error app-error" role="alert">
              <p>{error}</p>
              <button
                type="button"
                className="error-close"
                aria-label="誤りの知らせを閉じる"
                onClick={() => setError(null)}
              >
                閉じる
              </button>
            </div>
          )}
          <div className="body">
            {project !== null && focusing && (
              <button
                type="button"
                className="btn btn-sm focus-exit"
                onClick={() => updateViewPrefs((p) => ({ ...p, focus: false }))}
              >
                集中モードを終える（{keyLabel("Mod-Shift-F", IS_MAC)}）
              </button>
            )}
            {project !== null && !focusing && (
              <LeftBar
                // プレビューは本文から開くので、本文を選んだ形にする（T4-13、U16）。
                screen={screen === "preview" ? "text" : screen}
                counts={counts}
                aiRunning={aiActive > 0}
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
                        chapters={project.summary.chapters}
                        currentFile={currentFile}
                        currentScene={currentScene}
                        outline={
                          scene === null ||
                          scene.scene !== currentScene ||
                          currentFile === null
                            ? []
                            : outlineRows(
                                scene.outline,
                                buffers.current.get(currentFile) ?? "",
                              )
                        }
                        dirty={dirty}
                        onSelectScene={selectScene}
                        onSelectFile={(file) => goTo(file, null)}
                        onGoTo={(file, offset) => goTo(file, offset)}
                        onAddChapter={onAddChapter}
                        onRenameChapter={onRenameChapter}
                        onDeleteChapter={(file) => void onDeleteChapter(file)}
                      />
                    )}
                    <section
                      className={
                        viewPrefs.gothic
                          ? "editor-pane font-gothic"
                          : "editor-pane"
                      }
                      aria-label="エディタ"
                    >
                      {currentFile !== null && (
                        <ModeBar
                          file={currentFile}
                          scene={
                            currentSceneSummary === undefined ||
                            currentSceneSummary.file !== currentFile
                              ? null
                              : {
                                  id: currentSceneSummary.id,
                                  title: currentSceneSummary.title,
                                }
                          }
                          mode={mode}
                          onSelect={(m) => selectMode(currentFile, m)}
                          onSearch={() => setOpenSearch((n) => n + 1)}
                          view={{
                            focus: viewPrefs.focus,
                            typewriter: viewPrefs.typewriter,
                            gothic: viewPrefs.gothic,
                            onFocus: () =>
                              updateViewPrefs((p) => ({
                                ...p,
                                focus: !p.focus,
                              })),
                            onTypewriter: () =>
                              updateViewPrefs((p) => ({
                                ...p,
                                typewriter: !p.typewriter,
                              })),
                            onGothic: () =>
                              updateViewPrefs((p) => ({
                                ...p,
                                gothic: !p.gothic,
                              })),
                          }}
                        />
                      )}
                      {currentFile !== null && conflict !== undefined && (
                        <ConflictBar
                          file={currentFile}
                          deleted={conflict.deleted}
                          onReload={() =>
                            void resolveConflict(currentFile, true)
                          }
                          onKeep={() =>
                            void resolveConflict(currentFile, false)
                          }
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
                          saveRequest={saveRequest}
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
                          aiLines={aiLineMap}
                          aiFindings={findingMap}
                          previewLine={previewMarker}
                          openSearch={openSearch}
                          find={editorFind}
                        />
                      ) : (
                        <p className="placeholder">
                          章がありません。左の「＋ 章」で足してください。
                        </p>
                      )}
                    </section>
                    {!viewPrefs.focus && (
                      <ContextPanel
                        found={found}
                        onOpenFound={(m) => goTo(m.file, m.start)}
                        onCloseFound={() => setFindAllSpec(null)}
                        scene={scene}
                        // シーンにいれば、そのシーンの問題だけ（無ければファイルの問題。U13）。
                        diagnostics={sceneDiagnostics(
                          allDiagnostics,
                          project.summary.scenes,
                          currentFile,
                          scene === null ? null : currentScene,
                        )}
                        files={project.files}
                        textOf={(file) => buffers.current.get(file)}
                        scenes={project.summary.scenes}
                        onSelectDiagnostic={(d) => goTo(d.file, d.span[0])}
                        onRegister={onRegister}
                        onOpenCharacter={(id) =>
                          requestBible((seq) => ({
                            kind: "character",
                            id,
                            seq,
                          }))
                        }
                        findings={findingsForScene(aiFindings, currentScene)}
                        onGoToFinding={goToFinding}
                        onDismissFinding={dismissFinding}
                      />
                    )}
                  </section>
                  {(screen === "flow" || visited.has("flow")) && (
                    <section
                      className="screen flow-screen"
                      aria-label="フロー"
                      hidden={screen !== "flow"}
                    >
                      <KeepAlive active={screen === "flow"}>
                        <SceneGraph
                          summary={project.summary}
                          checks={project.checks}
                          level={flowLevel}
                          onLevel={setFlowLevel}
                          layoutVersion={layoutVersion}
                          onSelectScene={setFlowScene}
                          onOpenConcept={() => selectScreen("concept")}
                          onOpen={selectScene}
                          chapters={project.files}
                          dirtyChapters={dirty}
                          onStoryWritten={(result) => {
                            onStoryWritten(result.file, result.text);
                            showSummary(result.summary, result.checks);
                          }}
                          ai={{
                            runner: runnerOf(branchTask),
                            result:
                              branchTask === null
                                ? undefined
                                : aiResults.get(branchTask.task.id),
                            hidden: branchHidden,
                            onAdopt: (proposal, key, model) =>
                              adoptBranch(
                                proposal,
                                key,
                                model,
                                branchTask?.task.id ?? "branch_ideas",
                              ),
                            onReject: (key) =>
                              setBranchHidden((h) => new Set(h).add(key)),
                          }}
                        />
                      </KeepAlive>
                    </section>
                  )}
                  {(screen === "concept" || visited.has("concept")) && (
                    <section
                      className="screen concept-screen"
                      aria-label="企画"
                      hidden={screen !== "concept"}
                    >
                      <KeepAlive active={screen === "concept"}>
                        <ConceptScreen
                          scenes={project.summary.scenes}
                          chapters={project.files}
                          dirtyChapters={dirty}
                          settingsVersion={settingsVersion}
                          ideasVersion={ideasVersion}
                          aiFields={aiFieldMap}
                          onWritten={onSettingsWritten}
                          onStoryWritten={onStoryWritten}
                          onOpenLocation={openFromSettings}
                          onOpenEntry={openEntry}
                          onUnsaved={onConceptDraft}
                          ai={{
                            questions: runnerOf(questionTask),
                            sort: runnerOf(sortTask),
                            findings:
                              questionTask === null
                                ? []
                                : aiFindings.filter(
                                    (f) => f.task === questionTask.task.id,
                                  ),
                            onDismissFinding: dismissFinding,
                            suggestions: ideaSuggestions,
                            onHideSuggestion: (key) =>
                              setIdeaHidden((h) => new Set(h).add(key)),
                            onSkeleton:
                              skeletonTask === null
                                ? null
                                : () => {
                                    setAiScreenRequest((r) => ({
                                      task: skeletonTask.task.id,
                                      seq: (r?.seq ?? 0) + 1,
                                    }));
                                    selectScreen("ai");
                                  },
                            draft: {
                              runner: runnerOf(conceptDraftTask),
                              result:
                                conceptDraftResult?.kind === "ok"
                                  ? {
                                      run: conceptDraftResult.run,
                                      text: conceptDraftResult.written ?? "",
                                      ok: conceptDraftResult.wroteOk,
                                      fields:
                                        conceptDraftResult.fields[0] ?? null,
                                    }
                                  : conceptDraftResult?.kind === "failed"
                                    ? {
                                        run: conceptDraftResult.run,
                                        text: conceptDraftResult.message,
                                        ok: false,
                                        fields: null,
                                      }
                                    : null,
                              decisions: fieldDecisions,
                              onAdopt: (run, proposal, written) =>
                                conceptDraftTask === null ||
                                conceptDraftResult?.kind !== "ok"
                                  ? Promise.resolve(null)
                                  : adoptField(
                                      run,
                                      proposal,
                                      written,
                                      conceptDraftTask.task.id,
                                      conceptDraftResult.model,
                                    ),
                              onReject: rejectField,
                            },
                          }}
                        />
                      </KeepAlive>
                    </section>
                  )}
                  {(screen === "bible" || visited.has("bible")) && (
                    <section
                      className="screen bible-screen"
                      aria-label="設定DB"
                      hidden={screen !== "bible"}
                    >
                      <KeepAlive active={screen === "bible"}>
                        <BibleScreen
                          scenes={project.summary.scenes}
                          diagnostics={allDiagnostics}
                          settingsVersion={settingsVersion}
                          request={bibleRequest}
                          ai={{
                            runner: entriesRunner,
                            result:
                              entriesResult?.kind === "ok"
                                ? {
                                    run: entriesResult.run,
                                    text: entriesResult.written ?? "",
                                    ok: entriesResult.wroteOk,
                                    fields: entriesResult.fields,
                                    notes: [
                                      ...entriesResult.notes,
                                      ...entriesResult.unread,
                                    ],
                                  }
                                : entriesResult?.kind === "failed"
                                  ? {
                                      run: entriesResult.run,
                                      text: entriesResult.message,
                                      ok: false,
                                      fields: [],
                                      notes: [],
                                    }
                                  : null,
                            decisions: fieldDecisions,
                            onAdopt: (run, proposal, written) =>
                              entriesTask === null ||
                              entriesResult?.kind !== "ok"
                                ? Promise.resolve(null)
                                : adoptField(
                                    run,
                                    proposal,
                                    written,
                                    entriesTask.task.id,
                                    entriesResult.model,
                                  ),
                            onReject: rejectField,
                          }}
                          aiFields={aiFieldMap}
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
                      </KeepAlive>
                    </section>
                  )}
                  {(screen === "assets" || visited.has("assets")) && (
                    <section
                      className="screen assets-screen"
                      aria-label="素材"
                      hidden={screen !== "assets"}
                    >
                      <KeepAlive active={screen === "assets"}>
                        <AssetsScreen
                          root={project.root}
                          request={assetRequest}
                          summary={project.summary}
                          scenes={project.summary.scenes}
                          settingsVersion={settingsVersion}
                          shown={shownCount}
                          onWritten={onSettingsWritten}
                          onOpenLocation={openFromSettings}
                        />
                      </KeepAlive>
                    </section>
                  )}
                  {(screen === "variables" || visited.has("variables")) && (
                    <section
                      className="screen variables-screen"
                      aria-label="変数"
                      hidden={screen !== "variables"}
                    >
                      <KeepAlive active={screen === "variables"}>
                        <VariablesScreen
                          scenes={project.summary.scenes}
                          diagnostics={allDiagnostics}
                          characters={assist?.characters ?? []}
                          settingsVersion={settingsVersion}
                          onWritten={onSettingsWritten}
                          onOpenLocation={openFromSettings}
                          onUnsaved={onVariablesDraft}
                          textOf={(file) => buffers.current.get(file)}
                          onOpenCharacter={(id) =>
                            requestBible((seq) => ({
                              kind: "character",
                              id,
                              seq,
                            }))
                          }
                          stage2={variablesStage2}
                          onOpenCheck={() => selectScreen("check")}
                        />
                      </KeepAlive>
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
                        assetUrl={assetUrl}
                        onShowInEditor={showPreviewLine}
                        onLine={onPreviewLine}
                        onClose={closePreview}
                        request={previewRequest}
                        onOpenWindow={openWindow}
                      />
                    </section>
                  )}
                  {(screen === "export" || visited.has("export")) && (
                    <section
                      className="screen export-screen"
                      aria-label="出力"
                      hidden={screen !== "export"}
                    >
                      <KeepAlive active={screen === "export"}>
                        <ExportScreen
                          summary={project.summary}
                          unsaved={dirty.size}
                          load={scriptSheet}
                          write={exportScript}
                          writeRenpy={exportRenpy}
                          writeTyrano={exportTyrano}
                          writeUtage={exportUtage}
                          writeBook={exportBook}
                          loadBookSettings={loadBookSettings}
                          loadPlugins={loadExportPlugins}
                          runPlugin={exportPlugin}
                          ask={confirm}
                          loadChoices={readingChoices}
                          writeText={exportText}
                          loadProvenance={aiLines}
                          writeProvenance={exportProvenance}
                          writeOrders={exportOrders}
                          onShowPanel={() =>
                            setPanels((p) => showPanel(p, "export", narrow))
                          }
                        />
                      </KeepAlive>
                    </section>
                  )}
                  {(screen === "ai" || visited.has("ai")) && (
                    <section
                      className="screen ai-screen"
                      aria-label="AI実行"
                      hidden={screen !== "ai"}
                    >
                      <KeepAlive active={screen === "ai"}>
                        <AiScreen
                          tasks={aiTasks}
                          tasksError={aiTasksError}
                          connections={aiTaskConnections}
                          onChooseConnection={(task, connection) =>
                            setAiTaskConnection(task, connection).then(() =>
                              setAiVersion((n) => n + 1),
                            )
                          }
                          scenes={project.summary.scenes}
                          currentScene={currentScene}
                          running={aiRunState}
                          results={aiResults}
                          history={aiHistory}
                          findings={aiFindings}
                          onPrepare={prepareAiTask}
                          onRun={runTask}
                          onCancel={cancelTaskRun}
                          onGoToFinding={goToFinding}
                          onDismissFinding={dismissFinding}
                          proposalFiles={aiProposalFiles}
                          textVersion={savedVersion}
                          onAdopt={adopt}
                          onGoToProposal={goToProposal}
                          rejected={rejectedProposals}
                          onToggleReject={toggleReject}
                          fieldDecisions={fieldDecisions}
                          onAdoptField={adoptField}
                          onRejectField={rejectField}
                          autoRun={aiAutoRun}
                          autoResult={aiAutoResult}
                          autoError={aiAutoError}
                          onRunAuto={runAuto}
                          onCancelAuto={cancelAuto}
                          branches={aiBranchState}
                          onBranchDiff={aiBranchDiff}
                          onBranchMerge={(name) =>
                            aiBranchMerge(name).finally(() =>
                              setBranchesVersion((n) => n + 1),
                            )
                          }
                          onBranchDelete={async (name) => {
                            const ok = await confirm(
                              `AI のブランチ ${name} を、取り込まずに捨てます。元に戻せません。`,
                              "AIのブランチを破棄",
                            );
                            if (!ok) return false;
                            try {
                              await aiBranchDelete(name);
                            } finally {
                              setBranchesVersion((n) => n + 1);
                            }
                            return true;
                          }}
                          request={aiScreenRequest}
                          onRewriteSkeleton={(info, target) =>
                            void rewriteSkeleton(info, target)
                          }
                          plot={{
                            chapters: project.files,
                            branchHidden,
                            onAdoptBranch: adoptBranch,
                            onRejectBranch: (key) =>
                              setBranchHidden((h) => new Set(h).add(key)),
                            drift,
                            driftPending,
                            driftError,
                            driftHidden,
                            onRejectDrift: (key) =>
                              setDriftHidden((h) => new Set(h).add(key)),
                            onAdoptDrift: adoptDrift,
                            summarizeRun,
                            summarizeResult,
                            summarizeError,
                            onSummarize: runSummarize,
                            onCancelSummarize: cancelSummarize,
                          }}
                        />
                      </KeepAlive>
                    </section>
                  )}
                  {(screen === "preferences" || visited.has("preferences")) && (
                    <section
                      className="screen preferences-screen"
                      aria-label="環境設定"
                      hidden={screen !== "preferences"}
                    >
                      <KeepAlive active={screen === "preferences"}>
                        <PreferencesScreen
                          data={aiState}
                          error={aiError}
                          tests={aiTests}
                          onOverride={(
                            name,
                            baseUrl,
                            model,
                            contextTokens,
                            outputTokens,
                            structured,
                          ) =>
                            setAiOverride(
                              name,
                              baseUrl,
                              model,
                              contextTokens,
                              outputTokens,
                              structured,
                            ).finally(() => aiChanged([name]))
                          }
                          onAllow={(endpoint, allowed) =>
                            setAiCloudAllowed(endpoint, allowed).finally(() =>
                              aiChanged(namesOfEndpoint(endpoint)),
                            )
                          }
                          onSaveKey={(endpoint, key) =>
                            setAiKey(endpoint, key).finally(() =>
                              aiChanged(namesOfEndpoint(endpoint)),
                            )
                          }
                          onDeleteKey={(endpoint) =>
                            deleteAiKey(endpoint).finally(() =>
                              aiChanged(namesOfEndpoint(endpoint)),
                            )
                          }
                          onVia={(name, via, cli) =>
                            setAiVia(name, via, cli).finally(() =>
                              aiChanged([name]),
                            )
                          }
                          onTest={runAiTest}
                          onCancel={cancelTest}
                          view={viewPrefs}
                          onView={(next) =>
                            updateViewPrefs((p) => ({ ...p, ...next }))
                          }
                          scenes={project.summary.scenes}
                          settingsVersion={settingsVersion}
                          onProjectWritten={onProjectWritten}
                        />
                      </KeepAlive>
                    </section>
                  )}
                  {(screen === "check" || visited.has("check")) && (
                    <section
                      className="screen check-screen"
                      aria-label="検証"
                      hidden={screen !== "check"}
                    >
                      <KeepAlive active={screen === "check"}>
                        <CheckScreen
                          diagnostics={checkDiagnostics}
                          files={project.files}
                          scenes={project.summary.scenes}
                          textOf={textOfBuffer}
                          onOpen={onSelectDiagnostic}
                          onRegister={onRegister}
                          onShowPanel={() =>
                            setPanels((p) => showPanel(p, "check", narrow))
                          }
                          stage2={{
                            report: stage2,
                            running: stage2Running,
                            error: stage2Error,
                            discarded: stage2Discarded,
                            at: stage2At,
                            onRun: runStage2,
                          }}
                        />
                      </KeepAlive>
                    </section>
                  )}
                </>
              )}
            </main>
          </div>
          {palette && project !== null && (
            <CommandPalette
              commands={commands}
              screen={screen}
              scenes={project.summary.scenes}
              onOpen={openFound}
              onClose={() => setPalette(false)}
            />
          )}
          {renaming !== null && project !== null && (
            <RenameDialog
              symbol={renaming}
              onApply={doRename}
              onClose={() => setRenaming(null)}
            />
          )}
          {unsavedDialog !== null && (
            <UnsavedDialog
              ask={unsavedDialog.ask}
              saving={unsavedSaving}
              error={unsavedError}
              onChoose={chooseUnsaved}
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
            items={statusItems}
            extra={
              screen === "text" && currentFile !== null ? (
                <CursorPosition
                  key="cursor"
                  file={currentFile}
                  textOf={(file) => buffers.current.get(file)}
                />
              ) : undefined
            }
            // 行と列はシーンの後ろ（シーンが無ければ表示モードの後ろ）。
            extraAt={currentSceneSummary === undefined ? 1 : 2}
            notice={notice}
            save={
              project === null
                ? null
                : saveState(
                    saving,
                    // 失敗した章をまだ保存していないときだけ（読み込み直して捨てたら消す）。
                    saveFailed !== null && dirty.has(saveFailed.file)
                      ? saveFailed.message
                      : null,
                    // ソース表示（設定DB・変数・企画）の書きかけも、保存していない変更に数える。
                    dirty.size + settingsDrafts.size,
                  )
            }
            onSaveProblem={(message) => setError(message)}
            counts={counts}
            onProblems={project === null ? null : () => selectScreen("check")}
          />
        </div>
      </AskUnsavedContext.Provider>
    </StatusContext.Provider>
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
