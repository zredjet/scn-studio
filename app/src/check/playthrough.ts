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
