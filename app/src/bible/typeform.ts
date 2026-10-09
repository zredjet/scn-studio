// 型定義の編集（T2-10、D061）の判断：項目の型の候補と、選択肢の読み方。React と DOM に依存させない（テストする）。
// 型を作るときの元（テンプレートと複製。T4-9、D082）の選び方も。

import type { EntryType, TypeSource, TypeTemplate } from "../core/types.ts";

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

// ---- 型を作るときの元（「＋ 型」。T4-9、D082） ----

/** 元にするものの選択肢の値（`empty`、`template:<ID>`、`type:<ID>`）を、コアに渡す元にする。 */
export function typeSourceOf(value: string): TypeSource {
  if (value.startsWith("template:"))
    return { kind: "template", id: value.slice("template:".length) };
  if (value.startsWith("type:"))
    return { kind: "type", id: value.slice("type:".length) };
  return { kind: "empty" };
}

/** `base`（英数字と _）を元に、`taken` と重ならない型ID（`place`、`place_2`、`place_3` …）。 */
export function freeTypeId(base: string, taken: readonly string[]): string {
  const clean = base.replace(/[^A-Za-z0-9_]/g, "");
  if (clean === "") return "";
  if (!taken.includes(clean)) return clean;
  for (let n = 2; ; n++) {
    const id = `${clean}_${n}`;
    if (!taken.includes(id)) return id;
  }
}

/**
 * 元を選んだときに入れておく名前とID：テンプレートはその名前と型ID（IDが使われていれば、IDと名前に同じ番号を
 * 付ける：`place_2`・「場所2」）、今ある型の複製は「〇〇のコピー」と `<ID>_copy`。空の型は空。
 */
export function typeSourceDefaults(
  source: TypeSource,
  templates: readonly TypeTemplate[],
  types: readonly EntryType[],
  taken: readonly string[],
): { readonly name: string; readonly id: string } {
  if (source.kind === "template") {
    const t = templates.find((t) => t.id === source.id);
    if (t === undefined) return { name: "", id: "" };
    const id = freeTypeId(t.id, taken);
    const n = id.startsWith(`${t.id}_`) ? id.slice(t.id.length + 1) : "";
    return { name: `${t.name}${n}`, id };
  }
  if (source.kind === "type") {
    const t = types.find((t) => t.id === source.id);
    return t === undefined
      ? { name: "", id: "" }
      : { name: `${t.name}のコピー`, id: freeTypeId(`${t.id}_copy`, taken) };
  }
  return { name: "", id: "" };
}

/** 説明に並べる項目の名前の数（ほかは数だけ）。 */
const SHOWN_LABELS = 8;

/** 型を作る欄の名前とID。 */
export interface TypeFormNames {
  readonly name: string;
  readonly id: string;
}

/**
 * 元を選び直したときの欄：前に入れた値（`filled`）のままの欄だけを新しい既定値（`next`）にし、人が書き換えた欄は
 * 残す（空の欄は入れたままとみなす）。
 */
export function refillTypeForm(
  current: TypeFormNames,
  filled: TypeFormNames,
  next: TypeFormNames,
): TypeFormNames {
  const keep = (now: string, before: string, after: string) =>
    now === before || now === "" ? after : now;
  return {
    name: keep(current.name, filled.name, next.name),
    id: keep(current.id, filled.id, next.id),
  };
}

/** 元の説明（テンプレートの説明と項目、複製する型の項目）。空の型は `null`。 */
export function typeSourceText(
  source: TypeSource,
  templates: readonly TypeTemplate[],
  types: readonly EntryType[],
): string | null {
  const labels = (list: readonly string[]) => {
    if (list.length === 0) return "項目なし";
    const shown = list.slice(0, SHOWN_LABELS).join("・");
    const rest = list.length - SHOWN_LABELS;
    return `項目 ${list.length}：${shown}${rest > 0 ? ` ほか ${rest}` : ""}`;
  };
  if (source.kind === "template") {
    const t = templates.find((t) => t.id === source.id);
    return t === undefined ? null : `${t.description}（${labels(t.labels)}）`;
  }
  if (source.kind === "type") {
    const t = types.find((t) => t.id === source.id);
    return t === undefined
      ? null
      : `${t.name} の項目を写した新しい型を作ります。元の型とはつながりません（${labels(t.fields.map((f) => f.label))}）`;
  }
  return null;
}
