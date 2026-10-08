// AI実行の画面（docs/ui/mockups/07-ai.html、T3-7、D068）の判断：タスクの並べ方、指摘の持ち方、行の印、文。

import type {
  AiAssembled,
  AiFinding,
  AiPrepared,
  AiTask,
  AiTaskInfo,
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
  /** 届いた文字の数。 */
  readonly chars: number;
}

/** 実行の結果（タスクごとに最後のもの）。 */
export type RunResult =
  | {
      readonly kind: "ok";
      readonly run: number;
      readonly scene: string | null;
      readonly summary: string | null;
      readonly summaryProblem: string | null;
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
