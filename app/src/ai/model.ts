// AI実行の画面（docs/ui/mockups/07-ai.html、T3-7、D068）の判断：タスクの並べ方、指摘の持ち方、行の印、文。

import type {
  AiAssembled,
  AiConnectionChoice,
  AiFinding,
  AiPrepared,
  AiProposal,
  AiProposalFile,
  AiProposalView,
  AiTask,
  AiTaskInfo,
  AutoEvent,
  AutoResult,
  AutoScene,
  BranchProposal,
  IdeaSuggestion,
  PlotWriteResult,
  SceneSummary,
  SkeletonScene,
  SummarizeResult,
  SummaryDrift,
} from "../core/types.ts";
import { locateLine } from "../preview/locate.ts";

/** 段階の名前（モックの並び：自律、生成、提案、指摘）。 */
export const LEVEL_LABEL: Readonly<Record<AiTask["level"], string>> = {
  auto: "自律",
  generate: "生成",
  propose: "提案",
  review: "指摘",
};

const LEVEL_ORDER: readonly AiTask["level"][] = [
  "auto",
  "generate",
  "propose",
  "review",
];

export const SEVERITY_LABEL: Readonly<Record<AiFinding["severity"], string>> = {
  error: "誤り",
  warning: "警告",
  info: "情報",
};

/** タスクを段階ごとにまとめる（空の段階は出さない。段階の中はファイル名の順のまま）。 */
export function groupTasks(
  tasks: readonly AiTaskInfo[],
): { level: AiTask["level"]; label: string; tasks: AiTaskInfo[] }[] {
  return LEVEL_ORDER.map((level) => ({
    level,
    label: LEVEL_LABEL[level],
    tasks: tasks.filter((t) => t.task.level === level),
  })).filter((g) => g.tasks.length > 0);
}

/** 今のシーンを選んで実行するタスクか（文脈に `scene` か `previous_summaries` がある）。 */
export function needsScene(task: AiTask): boolean {
  return task.context.some(
    (c) => c.kind === "scene" || c.kind === "previous_summaries",
  );
}

/** 実行できない理由（無ければ null）。 */
export function blocker(info: AiTaskInfo): string | null {
  if (info.unsupported !== null) return info.unsupported;
  if (info.connection_problem !== null) return info.connection_problem;
  if (info.task.prompt === null)
    return `${info.task.path} に prompt がありません`;
  if (info.connection?.cloud === true && !info.connection.allowed)
    return "この端末からクラウドへ送ることを許可していません（環境設定）";
  return null;
}

/** タスクの一覧に見せる行き先（T4-1：この端末で選んだ接続先には「端末」を添える）。 */
export function placeText(info: AiTaskInfo): string {
  if (info.connection === null) return "接続先なし";
  const place = info.connection.cloud ? "クラウド" : "ローカル";
  return info.source === "machine" ? `${place} · 端末` : place;
}

/**
 * クラウドへ送るタスクの送り先（行き先とモデル。重ねずに並べる）。人が確かめた送り先として、自律と同期の実行に
 * 渡す（コアが、実行のときの接続先の送り先と照らす。T4-1）。
 */
export function cloudDestinations(infos: readonly AiTaskInfo[]): string[] {
  return [
    ...new Set(
      infos
        .filter((t) => t.connection?.cloud === true)
        .map((t) => t.connection?.destination ?? ""),
    ),
  ].sort();
}

/** タスクの接続先の選択肢（T4-1）。`value` は接続先の名前で、定義のままは ""。 */
export interface ConnectionOption {
  readonly value: string;
  readonly label: string;
}

/** 「定義のまま」と作品の接続先。この端末で選んだ接続先が作品に無ければ、それも並べる（選び直せるように）。 */
export function connectionOptions(
  info: AiTaskInfo,
  connections: readonly AiConnectionChoice[],
): ConnectionOption[] {
  const place = (cloud: boolean | null) =>
    cloud === null ? "設定に誤り" : cloud ? "クラウド" : "ローカル";
  const options: ConnectionOption[] = [
    {
      value: "",
      label: `定義のまま（${info.task.model ?? "既定の接続先"}）`,
    },
    ...connections.map((c) => ({
      value: c.name,
      label: `${c.name}（${place(c.cloud)}）`,
    })),
  ];
  if (
    info.override !== null &&
    !connections.some((c) => c.name === info.override)
  ) {
    options.push({
      value: info.override,
      label: `${info.override}（作品に無い）`,
    });
  }
  return options;
}

/** 画面が持つ指摘（実行ごと）。 */
export interface FindingRecord {
  readonly id: string;
  readonly run: number;
  readonly task: string;
  readonly taskName: string;
  /** 実行の対象のシーン（同じタスクを同じシーンでやり直したら、前の指摘を置き換える）。 */
  readonly target: string | null;
  /** 接続先の名前とモデル（`local · qwen3`）。 */
  readonly source: string;
  readonly finding: AiFinding;
}

/** 新しい実行の指摘を足す。同じタスクと同じ対象の前の指摘は置き換える。 */
export function mergeFindings(
  old: readonly FindingRecord[],
  fresh: readonly FindingRecord[],
  task: string,
  target: string | null,
): FindingRecord[] {
  return [
    ...old.filter((r) => !(r.task === task && r.target === target)),
    ...fresh,
  ];
}

/** シーンの指摘（文脈パネルの「AIの指摘」）。 */
export function findingsForScene(
  records: readonly FindingRecord[],
  scene: string | null,
): FindingRecord[] {
  if (scene === null) return [];
  return records.filter((r) => r.finding.scene === scene);
}

/** 行IDから、その行の指摘の文（エディタの行の印）。 */
export function findingLines(
  records: readonly FindingRecord[],
): ReadonlyMap<string, string> {
  const lines = new Map<string, string[]>();
  for (const r of records) {
    const id = r.finding.line;
    if (id === null || r.finding.unresolved) continue;
    const list = lines.get(id) ?? [];
    list.push(`${r.taskName}：${r.finding.message}`);
    lines.set(id, list);
  }
  return new Map([...lines].map(([id, list]) => [id, list.join("\n")]));
}

/** 見積もりと上限の文。 */
export function estimateText(a: AiAssembled): string {
  return `見積もり ${a.tokens.toLocaleString("ja-JP")} / 上限 ${a.limit.toLocaleString("ja-JP")} トークン（文脈の長さ ${a.context_tokens.toLocaleString("ja-JP")}）`;
}

/** 削った節の文。 */
export function trimmedText(a: AiAssembled): string[] {
  return a.trimmed.map(
    (t) =>
      `${t.how === "shortened" ? "短くした" : "削った"}：${t.title}（${t.tokens.toLocaleString("ja-JP")}）`,
  );
}

/** 走っている実行。 */
export interface RunState {
  readonly run: number;
  readonly task: string;
  readonly scene: string | null;
  /** 届いた文字の数（考えた中身も含む）。 */
  readonly chars: number;
  /** 考えている間（考えるモデルの考えた中身が届いている。T4-2）。 */
  readonly thinking: boolean;
}

/** 送る中身に構造化出力を付けるときの知らせ（T4-2）。 */
export const STRUCTURED_NOTE =
  "構造化出力：返す形（JSON Schema）を response_format で付けて送ります（環境設定で切れます）";

/** 実行中の進み具合の文（T4-2：考えている間は「考えています」）。 */
export function progressText(state: RunState): string {
  const chars = state.chars.toLocaleString("ja-JP");
  return state.thinking
    ? `考えています · 受け取った文字 ${chars}`
    : `受け取った文字 ${chars}`;
}

/** 実行の結果（タスクごとに最後のもの）。 */
export type RunResult =
  | {
      readonly kind: "ok";
      readonly run: number;
      readonly scene: string | null;
      readonly summary: string | null;
      readonly summaryProblem: string | null;
      /** 提案（`output: diff`。T3-8）と、出自に記録するモデル。 */
      readonly proposals: readonly AiProposal[];
      readonly model: string;
      /** 生成の下書き・ルートの骨格を書き込んだ結果の文（T3-9・T3-10。生成でなければ null）。 */
      readonly written: string | null;
      /** 書き込めたか（`written` があるとき）。 */
      readonly wroteOk: boolean;
      /** ルートの骨格（書き直すときに使う。T3-10）。 */
      readonly skeleton: readonly SkeletonScene[];
      /** 提案ノード（`output: proposals`。T3-10）。 */
      readonly branches: readonly BranchProposal[];
      /** アイデアメモの分類案（`output: ideas`。T3-10）。 */
      readonly ideas: readonly IdeaSuggestion[];
      readonly unread: readonly string[];
      readonly tokens: string;
    }
  | {
      readonly kind: "failed";
      readonly run: number;
      readonly scene: string | null;
      readonly message: string;
    }
  | {
      readonly kind: "cancelled";
      readonly run: number;
      readonly scene: string | null;
    };

/** 実行の履歴（この画面を開いている間だけ）。 */
export interface HistoryEntry {
  readonly run: number;
  readonly taskName: string;
  /** 始めた時刻（`11:42`）。 */
  readonly at: string;
  readonly status: string;
}

/** 実行の結果の、履歴の文。 */
export function historyStatus(
  result: RunResult,
  findings: number,
  output: AiTask["output"],
): string {
  switch (result.kind) {
    case "ok":
      if (output === "summary")
        return result.summary === null
          ? "要約を読めません"
          : "要約を作りました";
      if (output === "diff")
        return result.proposals.length === 0
          ? "提案なし"
          : `提案 ${result.proposals.length}件`;
      if (output === "proposals")
        return result.branches.length === 0
          ? "分岐案なし"
          : `分岐案 ${result.branches.length}件`;
      if (output === "ideas")
        return result.ideas.length === 0
          ? "分類案なし"
          : `分類案 ${result.ideas.length}件`;
      if (output === "draft")
        return result.written === null
          ? "書き込みなし"
          : result.wroteOk
            ? "書き込みました"
            : "書き込めませんでした";
      return findings === 0 ? "指摘なし" : `指摘 ${findings}件`;
    case "failed":
      return "失敗";
    case "cancelled":
      return "取り消し";
  }
}

/** 実行の結果のトークン数の文。 */
export function tokensText(usage: {
  readonly input_tokens: number;
  readonly output_tokens: number;
  readonly estimated: boolean;
}): string {
  return `入力 ${usage.input_tokens.toLocaleString("ja-JP")}・出力 ${usage.output_tokens.toLocaleString("ja-JP")} トークン${usage.estimated ? "（文字数からの見積もり）" : ""}`;
}

/**
 * 指摘の行へ移る位置（エディタの位置）。行IDで今の本文の中を探し（実行の後に本文が変わっても付いていく）、
 * 無ければ実行したときのバイト位置に戻る。どちらも無ければ null。
 */
export function findingOffset(text: string, finding: AiFinding): number | null {
  if (finding.line === null && finding.span === null) return null;
  return locateLine(
    text,
    finding.unresolved ? null : finding.line,
    finding.span?.[0] ?? -1,
  );
}

/** 見た送る中身が、今のタスクと対象のシーンのものか（違えば古いので使わない）。 */
export function preparedFor(
  prepared: AiPrepared | null,
  task: string,
  scene: string | null,
): prepared is AiPrepared {
  return (
    prepared !== null &&
    prepared.assembled.task === task &&
    prepared.assembled.scene === scene
  );
}

/** 提案のまとまり（実行の結果か、`ai/proposals/` のファイル。T3-8）。 */
export interface ProposalSet {
  /** `run-12`・`file:ai/proposals/x.md`。 */
  readonly id: string;
  readonly label: string;
  /** 採用した行の出自の記録に書くタスクとモデル。 */
  readonly task: string;
  readonly model: string;
  readonly proposals: readonly AiProposal[];
}

/** 提案の中身から作る鍵（ファイルが外で書き換わったら、別のまとまりとして照らし直し、却下も忘れる）。 */
export function proposalsKey(proposals: readonly AiProposal[]): string {
  let hash = 0x811c9dc5;
  for (const ch of JSON.stringify(proposals)) {
    hash ^= ch.codePointAt(0) ?? 0;
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16);
}

/** `ai/proposals/` のファイルの提案のまとまり。タスクとモデルが書かれていなければ、外部のものとして記録する。 */
export function fileProposalSet(file: AiProposalFile): ProposalSet {
  return {
    id: `file:${file.path}:${proposalsKey(file.proposals)}`,
    label: file.path.replace(/^ai\/proposals\//, ""),
    task: file.task ?? "external:proposals",
    model: file.model ?? "unknown",
    proposals: file.proposals,
  };
}

/** 提案の様子の文。 */
export function proposalStatusText(view: AiProposalView): string {
  switch (view.status.kind) {
    case "ready":
      return "採用できます";
    case "same":
      return "採用済み（今の文と同じ）";
    case "blocked":
      return view.status.reason;
  }
}

/** 採用できる提案（却下したものを除く）の、並びの番号。 */
export function adoptableIndexes(
  views: readonly AiProposalView[],
  rejected: ReadonlySet<number>,
): number[] {
  return views.flatMap((v, i) =>
    v.status.kind === "ready" && !rejected.has(i) ? [i] : [],
  );
}

/** 採用する提案を、章のファイルごとにまとめる（ファイルの分からない提案は除く）。 */
export function proposalsByFile(
  views: readonly AiProposalView[],
): Map<string, AiProposal[]> {
  const out = new Map<string, AiProposal[]>();
  for (const v of views) {
    if (v.file === null) continue;
    const list = out.get(v.file) ?? [];
    list.push(v.proposal);
    out.set(v.file, list);
  }
  return out;
}

/** 自律の段の名前（T3-9）。 */
export const STEP_LABEL: Readonly<Record<string, string>> = {
  beats: "展開メモ",
  draft: "本文",
  check: "点検",
  fix: "直し",
  write: "書き込み",
};

/** 走っている自律の実行（進み具合の知らせから作る）。 */
export interface AutoRun {
  readonly run: number;
  readonly task: string;
  readonly file: string;
  /** 下書きにするシーン（本文の順）。 */
  readonly planned: readonly string[];
  /** 今の段。 */
  readonly current: {
    readonly scene: string;
    readonly step: string;
    readonly iteration: number;
  } | null;
  /** 終わったシーン。 */
  readonly done: readonly AutoScene[];
  readonly tokens: number;
  readonly budget: number | null;
}

/** 進み具合の知らせを当てる。 */
export function applyAutoEvent(run: AutoRun, event: AutoEvent): AutoRun {
  switch (event.kind) {
    case "planned":
      return { ...run, planned: event.scenes };
    case "step":
      return {
        ...run,
        current: {
          scene: event.scene,
          step: event.step,
          iteration: event.iteration,
        },
      };
    case "scene_done":
      return { ...run, current: null, done: [...run.done, event.scene] };
    case "tokens":
      return { ...run, tokens: event.used };
  }
}

/** 自律をブランチで実行した後の文。 */
export function autoBranchText(
  branch: NonNullable<AutoResult["branch"]>,
): string {
  const { name, base, committed, kept, problem } = branch;
  if (committed === null)
    return `ブランチ ${name} に書きました。${problem ?? "コミットできませんでした"}`;
  if (!committed && !kept)
    return `何も書き込まなかったので、空のブランチ ${name} を消して ${base} に戻りました。${problem ?? ""}`;
  if (problem !== null)
    return committed
      ? `ブランチ ${name} にコミットしました。${problem}`
      : problem;
  return committed
    ? `ブランチ ${name} に書いてコミットし、${base} に戻りました。下の「AIのブランチ」で差分を見て取り込みます。`
    : `ブランチ ${name} に変更はありませんでした。${base} に戻りました。`;
}

/** シーンの様子の文（自律の進み具合の表）。 */
export function autoSceneText(run: AutoRun, scene: string): string {
  const done = run.done.find((d) => d.scene === scene);
  if (done !== undefined) {
    if (done.status === "written")
      return done.issues.length === 0
        ? done.iterations === 0
          ? "書き込みました（点検 問題なし）"
          : `書き込みました（${done.iterations}回直して 問題なし）`
        : `書き込みました（点検が ${done.issues.length}件残っています）`;
    return (
      done.message ??
      (done.status === "failed" ? "失敗" : "書き込みませんでした")
    );
  }
  if (run.current?.scene === scene) {
    const step = STEP_LABEL[run.current.step] ?? run.current.step;
    return run.current.iteration > 0
      ? `${step}中（${run.current.iteration}回目）`
      : `${step}中`;
  }
  return "待機";
}

// ---- プロットとフローへの AI 支援（T3-10） ----

/** ほかの画面（フロー図、企画の画面）から1つのタスクを実行するためのもの。 */
export interface TaskRunner {
  /** タスク（無ければ null。`ai/<名前>.yaml` が無い）。 */
  readonly info: AiTaskInfo | null;
  readonly running: RunState | null;
  readonly onPrepare: (
    task: string,
    scene: string | null,
  ) => Promise<AiPrepared>;
  readonly onRun: (
    info: AiTaskInfo,
    scene: string | null,
    digest: string | null,
  ) => void;
  readonly onCancel: () => void;
}

/** ルートの骨格を作るタスク（生成の段階で、今のシーンを文脈に入れない）か。 */
export function isSkeleton(task: AiTask): boolean {
  return (
    task.level === "generate" && !task.context.some((c) => c.kind === "scene")
  );
}

/** 使うタスク：`id` のタスク、無ければ `fits` に合う最初のタスク。 */
export function findTask(
  tasks: readonly AiTaskInfo[] | null,
  id: string,
  fits: (task: AiTask) => boolean,
): AiTaskInfo | null {
  if (tasks === null) return null;
  return (
    tasks.find((t) => t.task.id === id) ??
    tasks.find((t) => fits(t.task)) ??
    null
  );
}

/** 提案ノードと分類案の見分け（回と順番）。 */
export function resultKey(run: number, index: number): string {
  return `${run}-${index}`;
}

/** 提案ノードの記号（A、B、C…）。 */
export function proposalLetter(index: number): string {
  return String.fromCharCode(65 + (index % 26));
}

/** 提案ノードの行き先の文（`s013a に合流`、`新規 · 紬と会う`）。 */
export function branchTargetText(
  p: BranchProposal,
  scenes: readonly SceneSummary[],
): string {
  const stub = p.stubs.find((s) => s.id === p.target);
  if (stub !== undefined) return `新規 ${stub.id} · ${stub.title}`;
  const scene = scenes.find((s) => s.id === p.target);
  return scene === undefined
    ? `${p.target}（無いシーン）`
    : `${p.target}「${scene.title}」に合流`;
}

/** AI が書いた結果の文（提案ノードの採用、ルートの骨格）。 */
export function plotWrittenText(result: PlotWriteResult, what: string): string {
  switch (result.kind) {
    case "refused":
      return `書きませんでした：${result.reason}`;
    case "conflict":
      return result.deleted
        ? "章が外で消されていたので、書きませんでした"
        : "章が外で変わっていたので、書きませんでした。取り込んでからもう一度";
    case "saved": {
      const checks =
        result.verification.length === 0
          ? "検証：問題なし"
          : `検証：${result.verification.length}件（${result.verification.join("／")}）`;
      const problem =
        result.provenance_problem === null
          ? ""
          : `。出自を記録できませんでした：${result.provenance_problem}`;
      return `${what}（${result.file}）。${checks}${problem}`;
    }
  }
}

/** 分類案を当てたタグ（今のタグの後ろに、無いものだけを足す）。 */
export function mergedTags(
  current: readonly string[],
  suggested: readonly string[],
): string[] {
  const out = [...current];
  for (const t of suggested) if (!out.includes(t)) out.push(t);
  return out;
}

/** ルートの名前の候補（シーンのメタ情報の route。本文の順）。 */
export function routeNames(scenes: readonly SceneSummary[]): string[] {
  const out: string[] = [];
  for (const s of scenes) {
    const r = s.meta.route;
    if (r !== null && r !== "" && !out.includes(r)) out.push(r);
  }
  return out;
}

/** ルートの骨格を書く先（T3-10）：対象のルートと、書く章（今ある章か、新しい章の名前）。 */
export interface SkeletonTarget {
  readonly route: string | null;
  readonly file: string | null;
  readonly newChapter: string | null;
}

/** 章の要約をまとめて作り直している回（T3-10）。 */
export interface SummarizeRun {
  readonly run: number;
  readonly file: string;
  readonly scene: string | null;
  readonly done: number;
  readonly total: number;
}

/** 同期のずれの見分け（シーンと案）。 */
export function driftKey(d: SummaryDrift): string {
  return `${d.scene}\n${d.proposed}`;
}

/** AI実行の画面の、プロットとフローへの支援（T3-10）。 */
export interface PlotAi {
  readonly chapters: readonly string[];
  readonly branchHidden: ReadonlySet<string>;
  /** 提案ノードを採用する（`task` は出自に記録するタスク）。結果の文を返す。 */
  readonly onAdoptBranch: (
    proposal: BranchProposal,
    key: string,
    model: string,
    task: string,
  ) => Promise<string>;
  readonly onRejectBranch: (key: string) => void;
  readonly drift: readonly SummaryDrift[] | null;
  /** 章ごとの、要約を作り直すシーンの数。 */
  readonly driftPending: Readonly<Record<string, number>>;
  readonly driftError: string | null;
  readonly driftHidden: ReadonlySet<string>;
  readonly onRejectDrift: (key: string) => void;
  readonly onAdoptDrift: (drift: SummaryDrift) => Promise<string | null>;
  readonly summarizeRun: SummarizeRun | null;
  readonly summarizeResult: SummarizeResult | null;
  readonly summarizeError: string | null;
  readonly onSummarize: (
    task: string,
    file: string,
    confirmedCloud: readonly string[],
  ) => void;
  readonly onCancelSummarize: () => void;
}
