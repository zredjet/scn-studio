// 変数の画面（T1-10、D049）の、テストできる部分：型・既定値・使用箇所の見せ方、名前空間のキャラクター名。

import type { VariableRow } from "../core/types.ts";

/** 型の見せ方（`int 0〜10`、`enum（共通・灯・紬）`、宣言が無ければ「宣言なし」）。 */
export function typeText(row: VariableRow): string {
  if (row.type === null) return "宣言なし";
  if (row.type === "enum") return `enum（${row.values.join("・")}）`;
  if (row.range !== null) return `${row.type} ${row.range[0]}〜${row.range[1]}`;
  return row.type;
}

/** 使用箇所の数（`書き 9 · 読み 4`）。 */
export function usageText(row: VariableRow): string {
  return `書き ${row.writes.length} · 読み ${row.reads.length}`;
}

/** 名前空間がキャラクターIDなら、そのキャラクターの名前（なければ null）。 */
export function characterOf(
  key: string,
  characters: readonly { readonly id: string; readonly name: string }[],
): string | null {
  const namespace = key.split(".")[0] ?? "";
  return characters.find((c) => c.id === namespace)?.name ?? null;
}

/** 一覧の絞り込み：名前と説明に、空白で区切ったどの語も含まれる。 */
export function matchesVariable(row: VariableRow, query: string): boolean {
  const words = query
    .trim()
    .toLowerCase()
    .split(/\s+/)
    .filter((w) => w !== "");
  const text = `${row.key}\n${row.desc ?? ""}`.toLowerCase();
  return words.every((w) => text.includes(w));
}

/** シーンごとの読み書きの数。 */
export interface SceneUsage {
  scene: string;
  file: string;
  /** そのシーンで最初に読み書きする位置。 */
  offset: number;
  writes: number;
  reads: number;
}

/** 読み書きの位置を、シーンごとにまとめる（書きが先）。 */
export function locationsByScene(row: VariableRow): SceneUsage[] {
  const scenes = new Map<string, SceneUsage>();
  const add = (l: VariableRow["reads"][number], write: boolean) => {
    const s = scenes.get(l.scene) ?? {
      scene: l.scene,
      file: l.file,
      offset: l.span[0],
      writes: 0,
      reads: 0,
    };
    if (write) s.writes += 1;
    else s.reads += 1;
    s.offset = Math.min(s.offset, l.span[0]);
    scenes.set(l.scene, s);
  };
  for (const l of row.writes) add(l, true);
  for (const l of row.reads) add(l, false);
  return [...scenes.values()];
}

/** 名前空間（`akari.好感度` の `akari`。点が無ければ名前そのもの）。 */
export function namespaceOf(key: string): string {
  const dot = key.indexOf(".");
  return dot < 0 ? key : key.slice(0, dot);
}

/** 問題で絞る（T4-18、見本 08-variables.html）：書いて読まない・使っていない・宣言されていない。 */
export type VariableProblem = "written-not-read" | "unused" | "undeclared";

export const PROBLEM_LABEL: Readonly<Record<VariableProblem, string>> = {
  "written-not-read": "書いて読まない",
  unused: "使っていない",
  undeclared: "宣言されていない",
};

/** その変数に当たる問題（無ければ空）。 */
export function problemsOf(row: VariableRow): VariableProblem[] {
  const list: VariableProblem[] = [];
  if (!row.declared) list.push("undeclared");
  if (row.writes.length > 0 && row.reads.length === 0)
    list.push("written-not-read");
  if (row.declared && row.writes.length === 0 && row.reads.length === 0)
    list.push("unused");
  return list;
}

/** 名前空間ごとの数（書いた順。キャラクターなら名前を添える）。 */
export function namespaceCounts(
  rows: readonly VariableRow[],
  characters: readonly { readonly id: string; readonly name: string }[],
): {
  readonly namespace: string;
  readonly who: string | null;
  readonly count: number;
}[] {
  const counts = new Map<string, number>();
  for (const r of rows) {
    const ns = namespaceOf(r.key);
    counts.set(ns, (counts.get(ns) ?? 0) + 1);
  }
  return [...counts].map(([namespace, count]) => ({
    namespace,
    who: characters.find((c) => c.id === namespace)?.name ?? null,
    count,
  }));
}

/** 一覧の絞り込み（名前空間・問題・語）と、名前空間ごとのまとまり（出てきた順）。 */
export function groupRows(
  rows: readonly VariableRow[],
  filter: {
    readonly namespace: string | null;
    readonly problem: VariableProblem | null;
    readonly query: string;
  },
): { readonly namespace: string; readonly rows: VariableRow[] }[] {
  const groups = new Map<string, VariableRow[]>();
  for (const r of rows) {
    const ns = namespaceOf(r.key);
    if (filter.namespace !== null && ns !== filter.namespace) continue;
    if (filter.problem !== null && !problemsOf(r).includes(filter.problem))
      continue;
    if (!matchesVariable(r, filter.query)) continue;
    groups.set(ns, [...(groups.get(ns) ?? []), r]);
  }
  return [...groups].map(([namespace, list]) => ({ namespace, rows: list }));
}

/** 範囲・値の見せ方（列挙は値、範囲は `0〜10`、無ければ「—」）。 */
export function rangeText(row: VariableRow): string {
  if (row.type === "enum") return row.values.join("・") || "—";
  if (row.range !== null) return `${row.range[0]}〜${row.range[1]}`;
  return "—";
}

/** 書く場所・読む場所の1行：シーン、その行（前後の空白を除く）、行の番号。 */
export function locationLines(
  locations: readonly VariableRow["reads"][number][],
  textOf: (file: string) => string | undefined,
): {
  readonly scene: string;
  readonly file: string;
  readonly offset: number;
  readonly text: string;
  readonly line: number | null;
}[] {
  // 章の中身は章ごとに1回だけバイトにする（多く使う変数でも、描くたびに章を何度も変換しない）。
  const encoded = new Map<string, Uint8Array | null>();
  const bytesOf = (file: string): Uint8Array | null => {
    if (!encoded.has(file)) {
      const text = textOf(file);
      encoded.set(
        file,
        text === undefined ? null : new TextEncoder().encode(text),
      );
    }
    return encoded.get(file) ?? null;
  };
  return locations.map((l) => {
    const bytes = bytesOf(l.file);
    if (bytes === null)
      return {
        scene: l.scene,
        file: l.file,
        offset: l.span[0],
        text: "",
        line: null,
      };
    const start = Math.min(l.span[0], bytes.length);
    let line = 1;
    let head = 0;
    for (let i = 0; i < start; i += 1) {
      if (bytes[i] === 0x0a) {
        line += 1;
        head = i + 1;
      }
    }
    let end = bytes.indexOf(0x0a, start);
    if (end < 0) end = bytes.length;
    const lineText = new TextDecoder().decode(bytes.subarray(head, end)).trim();
    return {
      scene: l.scene,
      file: l.file,
      offset: l.span[0],
      text: lineText,
      line,
    };
  });
}
