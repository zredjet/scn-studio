// マージの画面（T2-7、D058）の判断：まとまった部分の畳み方、選び方の既定、ソース表示の印。React と DOM に
// 依存させない（テストする）。マージそのものはコア（`merge`）が行う。

import type { MergeChoice, MergeView } from "../core/types.ts";

/** ソース表示の下書きに入れる印の始めと終わり（コアの `Merge::with_markers` と同じ）。 */
const MARKER_START = "<<<<<<< 自分の変更";
const MARKER_END = ">>>>>>> 外の変更";

/** 選び方の既定：すべて「両方」（コアの既定のマージと同じ）。 */
export function defaultChoices(view: MergeView): MergeChoice[] {
  return Array.from({ length: view.conflicts }, () => "both" as const);
}

/** その側を取るか（両方を残すときは、どちらも取る。選んだ側を藍の薄い面で示すため。T4-20）。 */
export function picked(choice: MergeChoice, side: "mine" | "theirs"): boolean {
  return choice === "both" || choice === side;
}

/** テキストを行に分ける（終わりの改行は行を作らない）。 */
export function linesOf(text: string): string[] {
  if (text === "") return [];
  const lines = text.split(/\r?\n/);
  if (lines[lines.length - 1] === "") lines.pop();
  return lines;
}

/**
 * まとまった部分の見せ方：ぶつかりの前後 `keep` 行だけを見せ、間は行数で畳む。`before` は前にぶつかりがあるか
 * （あれば頭の行を見せる）、`after` は後ろにぶつかりがあるか（あれば終わりの行を見せる）。
 */
export function contextOf(
  text: string,
  before: boolean,
  after: boolean,
  keep: number,
): { head: string[]; omitted: number; tail: string[] } {
  const lines = linesOf(text);
  const headCount = before ? Math.min(keep, lines.length) : 0;
  const tailCount = after ? Math.min(keep, lines.length - headCount) : 0;
  return {
    head: lines.slice(0, headCount),
    omitted: lines.length - headCount - tailCount,
    tail: lines.slice(lines.length - tailCount),
  };
}

/** ソース表示の下書きに、マージの印が残っているか（残っていれば保存させない）。 */
export function hasMergeMarkers(text: string): boolean {
  return linesOf(text).some(
    (line) => line === MARKER_START || line === MARKER_END,
  );
}

/**
 * マージの元を覚える（知らせの途中の競合、置き換えを見送ったとき）。すでに覚えていれば上書きしない：自分の本文は
 * 最初に覚えた元から作ったもので、その後に読み込み直しが続いても元は変わらない（上書きすると、間の外の変更が
 * 「自分が戻した」と読まれて黙って戻る）。保存・読み込み直す・自分の変更を残す・マージで忘れる。
 */
export function rememberBase(
  bases: Map<string, string>,
  file: string,
  base: string | undefined,
): void {
  if (base === undefined || bases.has(file)) return;
  bases.set(file, base);
}
