// プレビュー中の行を、エディタの本文の中で探す（T1-11、D050）。プレビューのIRを取った後に本文を書き換えると
// バイト位置がずれるので、行IDで探し、無ければ（行IDの無い行、行IDを消した行）バイト位置に戻る。

import { PositionMap } from "../core/position.ts";

/** 行末の行ID（` ^xxxx`）。行IDの形は仕様書9節（英小文字と数字、4〜12文字）。 */
function lineIdPattern(id: string): RegExp {
  const escaped = id.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(`[ \\t]\\^${escaped}[ \\t]*\\r?$`, "m");
}

/**
 * 本文の中の、プレビュー中の行の位置（エディタの位置。`core/position.ts`）。行IDがあれば、その行の行頭。無ければ
 * IRのバイト位置（`span` の始まり）を変換する。本文の外なら null。
 */
export function locateLine(
  text: string,
  lineId: string | null,
  byteOffset: number,
): number | null {
  const map = new PositionMap(text);
  if (lineId !== null) {
    const match = lineIdPattern(lineId).exec(text);
    if (match !== null) {
      const head = text.lastIndexOf("\n", match.index) + 1;
      return map.byteToUtf16(
        new TextEncoder().encode(text.slice(0, head)).length,
      );
    }
  }
  if (byteOffset < 0 || byteOffset > map.byteLength) return null;
  return map.byteToUtf16(byteOffset);
}
