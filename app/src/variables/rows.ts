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
