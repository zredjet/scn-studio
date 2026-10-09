// 自動巡回（検証の段階2。T3-2、D063）の結果の見せ方。React と DOM に依存させない（テストする）。

import type { EndingReach, Playthrough } from "../core/types.ts";

/** 道の例で、前の周回のエンディングからもう一度始めたところの印（コアの `REPLAY_MARK`）。 */
export const REPLAY_MARK = "〔周回〕";

/** 選択肢の表示文を「」で囲む（もう「」で囲まれた台詞の形と、周回の印なら、そのまま。コアの `quote` と同じ）。 */
export function quote(label: string): string {
  return label === REPLAY_MARK ||
    (label.startsWith("「") && label.endsWith("」"))
    ? label
    : `「${label}」`;
}

/** 道の例の文（コアの `example_text` と同じ。多ければ12個まで）。 */
export function exampleText(labels: readonly string[]): string {
  if (labels.length === 0) return "選択肢を選ばずに";
  const shown = labels.slice(0, 12).map(quote);
  if (labels.length > 12) shown.push("…");
  return `選択肢 ${shown.join(" → ")}`;
}

/** 着いたときの変数の値（`akari.好感度 = 2 | 3`）。 */
export function valuesText(ending: EndingReach): string[] {
  return Object.entries(ending.values).map(([key, values]) => {
    const shown = values.map((v) => JSON.stringify(v));
    // 書ききれなかった変数には「…」を足す。
    if (ending.more.includes(key)) shown.push("…");
    return `${key} = ${shown.join(" | ")}`;
  });
}

/** 要約の1行（状態、周回、シーン、選択肢）。 */
export function summaryText(report: Playthrough): string {
  return `シーン ${report.scenes.reached}/${report.scenes.total} · 選べた選択肢 ${report.options.reached}/${report.options.total} · 状態 ${report.states}${report.starts > 1 ? ` · 周回 ${report.starts}通り` : ""}`;
}

/** エンディングを、着かないもの・着くものの順に並べる（着かないものを先に見せる）。 */
export function sortedEndings(report: Playthrough): EndingReach[] {
  return [...report.endings].sort(
    (a, b) => Number(a.reached) - Number(b.reached) || a.id.localeCompare(b.id),
  );
}

/** 段階2の様子（App が持つもの）。 */
export interface Stage2State {
  readonly report: Playthrough | null;
  readonly running: boolean;
  readonly discarded: boolean;
  readonly error: string | null;
}

/** 変数の取りうる値（変数の画面の「取りうる値（検証の段階2）」。T4-24、N9）。 */
export interface PossibleValues {
  /** 値の文（`0〜3`・`false | true`・`"共通" | "灯"`）。値が出ていなければ null。 */
  readonly text: string | null;
  /** 添える知らせ（段階2を調べていない、出ない値があり得る、など）。 */
  readonly notes: readonly string[];
  /** 検証の画面へ行くボタンを出すか（調べていない・捨てた・誤り）。 */
  readonly openCheck: boolean;
}

/** 値の並び（整数だけで、3つ以上が切れ目なく続くなら `最小〜最大`）。 */
export function valueRangeText(
  values: readonly (boolean | number | string)[],
  more: boolean,
): string {
  const ints = values.every((v) => typeof v === "number");
  if (ints && !more && values.length >= 3) {
    const sorted = [...(values as readonly number[])].sort((a, b) => a - b);
    const first = sorted[0] ?? 0;
    const last = sorted[sorted.length - 1] ?? 0;
    if (last - first === sorted.length - 1) return `${first}〜${last}`;
  }
  const shown = values.map((v) => JSON.stringify(v));
  if (more) shown.push("…");
  return shown.join(" | ");
}

/** 変数の取りうる値。段階2を調べていなければ、検証の画面で調べるよう知らせる。 */
export function possibleValues(
  stage2: Stage2State,
  key: string,
): PossibleValues {
  if (stage2.running)
    return { text: null, notes: ["段階2を調べています"], openCheck: false };
  if (stage2.report === null) {
    const why =
      stage2.error !== null
        ? `段階2を調べられませんでした：${stage2.error}`
        : stage2.discarded
          ? "本文が変わったので、前の段階2の結果は捨てました。検証の画面で調べ直すと出ます"
          : "検証の画面の「段階2を調べる」で、調べた道で出た値が出ます";
    return { text: null, notes: [why], openCheck: true };
  }
  const seen = stage2.report.variables[key];
  const notes: string[] = [];
  if (!stage2.report.complete)
    notes.push("調べきれなかったので、出ない値があるかもしれません");
  if (seen === undefined)
    return {
      text: null,
      notes: ["調べた道では値が出ませんでした", ...notes],
      openCheck: false,
    };
  if (seen.more) notes.push("値が多いので、先に出たものだけを並べています");
  if (!seen.flow)
    notes.push(
      "流れ（条件）に関わらない変数なので、出ない値があるかもしれません（関わる変数が同じ道は、まとめて調べます）",
    );
  return {
    text: valueRangeText(seen.values, seen.more),
    notes,
    openCheck: false,
  };
}
