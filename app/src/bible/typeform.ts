// 型定義の編集（T2-10、D061）の判断：項目の型の候補と、選択肢の読み方。React と DOM に依存させない（テストする）。

import type { EntryType } from "../core/types.ts";

/** 項目の型の候補：基本の型と、型・シーンへの参照（リストを含む）。今の型が候補に無ければ足す。 */
export function typeChoices(
  types: readonly EntryType[],
  current?: string,
): string[] {
  const base = [
    "text",
    "longtext",
    "number",
    "bool",
    "date",
    "select",
    "multiselect",
    "asset",
    "list<text>",
  ];
  const targets = ["scene", ...types.map((t) => t.id)];
  const refs = targets.flatMap((t) => [`ref<${t}>`, `list<ref<${t}>>`]);
  const all = [...base, ...refs];
  if (current !== undefined && !all.includes(current)) all.push(current);
  return all;
}

/** 選択肢の欄（`、`・`,`・改行で区切る）を、選択肢のリストにする（前後の半角空白を除き、空と重なりを除く）。 */
export function parseOptions(text: string): string[] {
  const out: string[] = [];
  for (const part of text.split(/[、,\n]/)) {
    const item = part.replace(/^[ \t]+|[ \t]+$/g, "");
    if (item !== "" && !out.includes(item)) out.push(item);
  }
  return out;
}

/** 選択肢の要る型か。 */
export function needsOptions(type: string): boolean {
  return type === "select" || type === "multiselect";
}
