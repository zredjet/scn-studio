// 素材カタログの画面（T2-3、D054）の、表示に使う計算：絞り込み、件数、キャラ×差分の表。

import type {
  AssetKey,
  AssetKind,
  AssetList,
  AssetRow,
  AssetState,
} from "../core/types.ts";

/** 種類のタブ。 */
export type AssetTab = "all" | AssetKind;

export const TABS: readonly {
  readonly tab: AssetTab;
  readonly label: string;
}[] = [
  { tab: "all", label: "すべて" },
  { tab: "background", label: "背景" },
  { tab: "expression", label: "立ち絵" },
  { tab: "bgm", label: "BGM" },
  { tab: "se", label: "SE" },
];

export const KIND_LABEL: Readonly<Record<AssetKind, string>> = {
  background: "背景",
  expression: "立ち絵",
  bgm: "BGM",
  se: "SE",
};

export const STATE_LABEL: Readonly<Record<AssetState, string>> = {
  file: "あり",
  placeholder: "仮素材",
  unregistered: "未登録",
};

/** 素材を指すもの（コマンドに渡す）。 */
export function assetKey(row: AssetRow): AssetKey {
  return { kind: row.kind, character: row.character, name: row.name };
}

/** 行の見分け（React の key と選択）。 */
export function rowId(row: AssetRow): string {
  return `${row.kind}\u0000${row.character ?? ""}\u0000${row.name}`;
}

/** 表に出す意味名（立ち絵は `キャラの名前/差分`）。 */
export function displayName(row: AssetRow): string {
  return row.kind === "expression"
    ? `${row.character_name ?? row.character ?? ""}/${row.name}`
    : row.name;
}

export interface RowFilter {
  readonly tab: AssetTab;
  /** 仮素材と未登録だけ。 */
  readonly missingOnly: boolean;
  readonly query: string;
}

/**
 * 表に出す行：種類・状態・意味名（立ち絵はキャラの名前も）で絞り込み、未登録を先に、ほかはコアの順
 * （種類・キャラクター・意味名）。
 */
export function filterRows(
  rows: readonly AssetRow[],
  filter: RowFilter,
): AssetRow[] {
  const query = filter.query.trim().toLowerCase();
  const kept = rows.filter(
    (r) =>
      (filter.tab === "all" || r.kind === filter.tab) &&
      (!filter.missingOnly || r.state !== "file") &&
      (query === "" || displayName(r).toLowerCase().includes(query)),
  );
  return [
    ...kept.filter((r) => r.state === "unregistered"),
    ...kept.filter((r) => r.state !== "unregistered"),
  ];
}

export interface AssetCounts {
  /** カタログにある意味名。 */
  readonly registered: number;
  readonly placeholders: number;
  /** 本文で使われているが、カタログに無い意味名。 */
  readonly unregistered: number;
  /** 種類ごとの、カタログにある意味名。 */
  readonly byKind: Readonly<Record<AssetKind, number>>;
}

export function countAssets(rows: readonly AssetRow[]): AssetCounts {
  const byKind: Record<AssetKind, number> = {
    background: 0,
    expression: 0,
    bgm: 0,
    se: 0,
  };
  let registered = 0;
  let placeholders = 0;
  let unregistered = 0;
  for (const r of rows) {
    if (r.state === "unregistered") {
      unregistered += 1;
      continue;
    }
    registered += 1;
    byKind[r.kind] += 1;
    if (r.state === "placeholder") placeholders += 1;
  }
  return { registered, placeholders, unregistered, byKind };
}

/** カタログに足せる行（「未登録をカタログに追加」）。 */
export function addableKeys(rows: readonly AssetRow[]): AssetKey[] {
  return rows.filter((r) => r.addable).map(assetKey);
}

export interface MatrixCell {
  readonly row: AssetRow;
  readonly uses: number;
}

export interface MatrixRow {
  readonly id: string;
  readonly name: string;
  readonly registered: boolean;
  /** 列（差分）ごとのセル。その差分が無ければ null。 */
  readonly cells: readonly (MatrixCell | null)[];
}

export interface ExpressionMatrix {
  /** 差分の名前（初めて出た順）。 */
  readonly columns: readonly string[];
  readonly rows: readonly MatrixRow[];
}

/** 立ち絵のキャラ×差分の表。キャラクターは一覧の順、差分は初めて出た順。 */
export function expressionMatrix(list: AssetList): ExpressionMatrix {
  const sprites = list.rows.filter((r) => r.kind === "expression");
  const columns: string[] = [];
  for (const r of sprites) {
    if (!columns.includes(r.name)) columns.push(r.name);
  }
  const rows = list.characters.map((c) => {
    const mine = sprites.filter((r) => r.character === c.id);
    return {
      id: c.id,
      name: c.name,
      registered: c.registered,
      cells: columns.map((name) => {
        const row = mine.find((r) => r.name === name);
        return row === undefined ? null : { row, uses: row.uses.length };
      }),
    };
  });
  return { columns, rows };
}

/** 使用箇所を、シーンごとにまとめる（最初の位置へ移る）。 */
export function usesByScene(
  row: AssetRow,
): { scene: string; file: string; offset: number; count: number }[] {
  const out: { scene: string; file: string; offset: number; count: number }[] =
    [];
  for (const use of row.uses) {
    const found = out.find((s) => s.scene === use.scene);
    if (found === undefined) {
      out.push({
        scene: use.scene,
        file: use.file,
        offset: use.span[0],
        count: 1,
      });
    } else {
      found.count += 1;
    }
  }
  return out;
}

/** 発注リストの列（コアの `assets::ORDER_COLUMNS`）。 */
export const ORDER_COLUMNS = [
  "種別",
  "名前",
  "キャラ",
  "差分",
  "使用回数",
  "初出",
  "使用シーン",
  "備考",
] as const;
