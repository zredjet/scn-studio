// 企画書と設定DBの項目の出自の印（T4-3、D080）：AI が書いた項目（`ai`）と、人が直した AI の項目（`ai-edited`）。
// エディタの AI の行の印と同じく、`ai` は実線、`ai-edited` は点線（色だけで分けず、文でも見せる）。

/** 項目の今の出自。 */
export type FieldOrigin = "ai" | "ai-edited";

import type { AiFieldRecord } from "../core/types.ts";

/** フロントマターの後ろの本文の項目（コアの `provenance::BODY_KEY`）。 */
export const BODY_KEY = "#body";

/** 項目の出自の表のキー。 */
export function fieldKey(file: string, key: string): string {
  return `${file}\u0000${key}`;
}

/** 項目の出自の表（`ai`・`ai-edited` だけ）。 */
export function fieldOrigins(
  records: readonly AiFieldRecord[],
): ReadonlyMap<string, FieldOrigin> {
  const map = new Map<string, FieldOrigin>();
  for (const r of records) {
    if (r.origin === "ai" || r.origin === "ai-edited")
      map.set(fieldKey(r.file, r.key), r.origin);
  }
  return map;
}

/** 項目の印の文と、印の class（無ければ null）。 */
export function originMark(origin: FieldOrigin | null | undefined): {
  readonly text: string;
  readonly title: string;
  readonly className: string;
} | null {
  switch (origin) {
    case "ai":
      return {
        text: "AI",
        title: "AI が書いた項目（ai/provenance.jsonl に記録）",
        className: "field-origin ai",
      };
    case "ai-edited":
      return {
        text: "AI・直した",
        title: "AI が書き、人が直した項目",
        className: "field-origin ai-edited",
      };
    default:
      return null;
  }
}

/** AI が作ったエントリ（フロントマターの `status: ai-draft`。人が消すか変えれば確かめたとみなす）。 */
export function isAiDraftEntry(
  fields: Readonly<Record<string, unknown>> | null | undefined,
): boolean {
  return fields?.status === "ai-draft";
}
