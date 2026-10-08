// 変数の宣言のフォーム（T2-10、D061）の判断：型ごとの入力欄（設定DBの `FieldInput` を使う）と、一覧の行から
// 入力欄の値を作る。React と DOM に依存させない（テストする）。

import type { FieldDef, JsonValue, VariableRow } from "../core/types.ts";

const def = (
  key: string,
  label: string,
  kind: FieldDef["kind"],
  options: readonly string[] = [],
): FieldDef => ({ key, label, type: kind.kind, kind, options });

/** 宣言の入力欄（型によって、選択肢・既定値・範囲の欄が変わる）。範囲は `range_min`・`range_max` の2つ。 */
export function declFields(row: VariableRow): FieldDef[] {
  const type = row.type ?? "string";
  const fields: FieldDef[] = [
    def("type", "型", { kind: "select" }, ["bool", "int", "string", "enum"]),
  ];
  if (type === "enum")
    fields.push(def("values", "値", { kind: "list", item: { kind: "text" } }));
  const defaultKind: FieldDef["kind"] =
    type === "int"
      ? { kind: "number" }
      : type === "bool"
        ? { kind: "bool" }
        : type === "enum"
          ? { kind: "select" }
          : { kind: "text" };
  fields.push(
    def(
      "default",
      "既定値",
      defaultKind,
      type === "enum" ? [...row.values] : [],
    ),
  );
  if (type === "int") {
    fields.push(def("range_min", "範囲（最小）", { kind: "number" }));
    fields.push(def("range_max", "範囲（最大）", { kind: "number" }));
  }
  fields.push(
    def("scope", "スコープ", { kind: "select" }, ["save", "persistent"]),
  );
  fields.push(def("desc", "説明", { kind: "longtext" }));
  return fields;
}

/** 一覧の行から、入力欄の値（既定値は型どおりに読む。読めなければ書かれた文字のまま）。 */
export function declValue(
  row: VariableRow,
  key: string,
): JsonValue | undefined {
  switch (key) {
    case "type":
      return row.type ?? undefined;
    case "values":
      return row.values.length > 0 ? [...row.values] : undefined;
    case "default": {
      if (row.default === null) return undefined;
      if (row.type === "int") {
        const n = Number(row.default);
        return Number.isInteger(n) ? n : row.default;
      }
      if (row.type === "bool") {
        if (row.default === "true") return true;
        if (row.default === "false") return false;
      }
      return row.default;
    }
    case "range_min":
      return row.range?.[0];
    case "range_max":
      return row.range?.[1];
    case "scope":
      return row.scope ?? undefined;
    case "desc":
      return row.desc ?? undefined;
    default:
      return undefined;
  }
}

/**
 * 範囲の片方を直した値（`range` に書く値）。両方そろえば `[最小, 最大]`、両方とも無くなれば null（項目を消す）。
 * 片方だけなら書かない（undefined）。もう片方は、入れかけの値（`pending`。範囲の無い変数で先に入れた片方）が
 * あればそれを、無ければ宣言の範囲を使う。
 */
export function rangeValue(
  row: VariableRow,
  which: "range_min" | "range_max",
  value: JsonValue,
  pending?: JsonValue,
): JsonValue | undefined {
  const written = (i: 0 | 1) =>
    pending !== undefined ? pending : (row.range?.[i] ?? null);
  const min = which === "range_min" ? value : written(0);
  const max = which === "range_max" ? value : written(1);
  if (min === null && max === null) return null;
  if (typeof min === "number" && typeof max === "number") return [min, max];
  return undefined;
}

/** 範囲の入れかけ（片方だけ入れて、まだ書いていない値。`null` は空にした）。選んでいる変数の分だけ持つ。 */
export interface RangeDraft {
  readonly key: string;
  readonly range_min?: JsonValue;
  readonly range_max?: JsonValue;
}

/**
 * 範囲の欄を直したとき：書く値（`write`。片方だけなら undefined で書かない）と、次の入れかけ（書くなら null）。
 * 入れかけは、ほかの変数のものなら使わない。
 */
export function commitRange(
  row: VariableRow,
  draft: RangeDraft | null,
  which: "range_min" | "range_max",
  value: JsonValue,
): {
  readonly write: JsonValue | undefined;
  readonly draft: RangeDraft | null;
} {
  const mine = draft !== null && draft.key === row.key ? draft : null;
  const other = which === "range_min" ? "range_max" : "range_min";
  const pending = mine !== null && other in mine ? mine[other] : undefined;
  const write = rangeValue(row, which, value, pending);
  if (write !== undefined) return { write, draft: null };
  return {
    write,
    draft:
      which === "range_min"
        ? { ...mine, key: row.key, range_min: value }
        : { ...mine, key: row.key, range_max: value },
  };
}

/** 範囲の欄に出す値：入れかけがあればそれ（空にしたなら空）、無ければ宣言の範囲。 */
export function rangeShown(
  row: VariableRow,
  draft: RangeDraft | null,
  which: "range_min" | "range_max",
): JsonValue | undefined {
  if (draft !== null && draft.key === row.key && which in draft)
    return draft[which] ?? undefined;
  return declValue(row, which);
}
