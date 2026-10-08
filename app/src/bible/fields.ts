// 設定DBの画面（T1-10、D049）の、テストできる部分：項目の型から入力欄を選ぶ、入力を値にする、
// 一覧の検索、診断を項目に結びつける。値を書き換えるのはコア（直したキーの値だけを差し替える）。

import type {
  Entry,
  EntryFile,
  EntryType,
  FieldDef,
  FieldKind,
  IrDiagnostic,
  JsonValue,
} from "../core/types.ts";

/** 入力欄の種類。`readonly` は、フォームでは直せない（型に合わない値、入れ子のリストなど）。 */
export type Widget =
  | "text"
  | "longtext"
  | "number"
  | "bool"
  | "date"
  | "select"
  | "multiselect"
  | "asset"
  | "ref"
  | "list"
  | "list-ref"
  | "readonly";

/** 項目の型の見せ方（参照は型の名前で。例：`ref<組織>`）。 */
export function kindLabel(
  kind: FieldKind,
  typeNames: ReadonlyMap<string, string>,
): string {
  switch (kind.kind) {
    case "ref":
      return `ref<${kind.target === "scene" ? "シーン" : (typeNames.get(kind.target) ?? kind.target)}>`;
    case "list":
      return `list<${kindLabel(kind.item, typeNames)}>`;
    default:
      return kind.kind;
  }
}

function isScalar(
  value: JsonValue | undefined,
): value is string | number | boolean {
  return (
    typeof value === "string" ||
    typeof value === "number" ||
    typeof value === "boolean"
  );
}

/** 書いていない値（キーが無い、null、空の文字列）。 */
export function isUnset(value: JsonValue | undefined): boolean {
  return value === undefined || value === null || value === "";
}

/** 値を文字列にする（文字列・数・真偽）。ほかは null。 */
export function scalarText(value: JsonValue | undefined): string | null {
  return isScalar(value) ? String(value) : null;
}

/**
 * 項目の型と今の値から、入力欄を選ぶ。値が型に合わない形（リストの項目に1つの値など）なら、フォームでは
 * 直さずに読むだけにする（ソース表示で直す。W805 が出ている）。
 */
export function widgetOf(
  kind: FieldKind,
  value: JsonValue | undefined,
): Widget {
  const unset = isUnset(value);
  switch (kind.kind) {
    case "bool":
      return unset || typeof value === "boolean" ? "bool" : "readonly";
    case "number":
      return unset || typeof value === "number" ? "number" : "readonly";
    case "multiselect":
      return unset || (Array.isArray(value) && value.every(isScalar))
        ? "multiselect"
        : "readonly";
    case "list": {
      if (!unset && !(Array.isArray(value) && value.every(isScalar))) {
        return "readonly";
      }
      if (kind.item.kind === "ref") return "list-ref";
      if (kind.item.kind === "list") return "readonly";
      return "list";
    }
    default:
      return unset || isScalar(value) ? kind.kind : "readonly";
  }
}

/** `YYYY-MM-DD` の正しい日付か。 */
export function isDate(text: string): boolean {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(text);
  if (m === null) return false;
  const [y, mo, d] = [Number(m[1]), Number(m[2]), Number(m[3])];
  if (mo < 1 || mo > 12 || d < 1) return false;
  const days = new Date(Date.UTC(y, mo, 0)).getUTCDate();
  return d <= days;
}

/** 入力を値にした結果。 */
export type Parsed =
  | { readonly ok: true; readonly value: JsonValue }
  | { readonly ok: false; readonly error: string };

/**
 * 1行・複数行の入力を、項目の型の値にする。空の入力は、文字列の型なら空の文字列、ほかの型なら値の無いキー
 * （null）にする（D049）。
 */
export function parseInput(kind: FieldKind, text: string): Parsed {
  switch (kind.kind) {
    case "number": {
      const trimmed = text.trim();
      if (trimmed === "") return { ok: true, value: null };
      const n = Number(
        trimmed.replace(/[０-９．－]/g, (c) =>
          String.fromCharCode(c.charCodeAt(0) - 0xfee0),
        ),
      );
      return Number.isFinite(n)
        ? { ok: true, value: n }
        : { ok: false, error: "数で書きます" };
    }
    case "date": {
      const trimmed = text.trim();
      if (trimmed === "") return { ok: true, value: null };
      return isDate(trimmed)
        ? { ok: true, value: trimmed }
        : { ok: false, error: "日付（YYYY-MM-DD）で書きます" };
    }
    case "text":
    case "asset":
      // 1行の入力欄。前後の空白は、書き手が入れたものとして残す（黙って変えない）。
      return { ok: true, value: text };
    case "longtext":
      return { ok: true, value: text };
    default:
      return { ok: false, error: "この型は入力欄では直せません" };
  }
}

/** リストの中身を文字列にする。 */
export function listItems(value: JsonValue | undefined): string[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap((v) => {
    const text = scalarText(v);
    return text === null ? [] : [text];
  });
}

/** リストに1つ足す（項目の型に合わせて値にする）。空なら足さない。 */
export function addItem(
  kind: FieldKind,
  value: JsonValue | undefined,
  text: string,
): Parsed {
  const items: JsonValue[] = Array.isArray(value) ? [...value] : [];
  if (text.trim() === "") return { ok: true, value: items };
  const item = kind.kind === "list" ? kind.item : kind;
  const parsed = parseInput(item, text.trim());
  if (!parsed.ok) return parsed;
  return { ok: true, value: [...items, parsed.value] };
}

/** リストから i 番目を除く。 */
export function removeItem(
  value: JsonValue | undefined,
  index: number,
): JsonValue {
  if (!Array.isArray(value)) return [];
  return value.filter((_, i) => i !== index);
}

/** 2つのJSONの値が同じか。 */
export function sameValue(
  a: JsonValue | undefined,
  b: JsonValue | undefined,
): boolean {
  return JSON.stringify(a ?? null) === JSON.stringify(b ?? null);
}

/** エントリの名前（`name`。無ければID）。 */
export function entryName(entry: Entry): string {
  return scalarText(entry.fields.name) ?? entry.id;
}

/** 名前の最初の1文字（頭文字の丸）。 */
export function initialOf(name: string): string {
  return [...name.trim()][0] ?? "？";
}

/** エントリのファイルの見出し（読めたものは名前、読めなければファイル名）。 */
export function fileLabel(file: EntryFile): string {
  if (file.entry !== null) return entryName(file.entry);
  return file.path.split("/").pop()?.replace(/\.md$/, "") ?? file.path;
}

/** 一覧の検索：名前・呼び名・ID・ファイル名に、空白で区切ったどの語も含まれる（大文字小文字は区別しない）。 */
export function matchesQuery(file: EntryFile, query: string): boolean {
  const words = query
    .trim()
    .toLowerCase()
    .split(/\s+/)
    .filter((w) => w !== "");
  if (words.length === 0) return true;
  const haystack = [file.path, fileLabel(file)];
  if (file.entry !== null) {
    haystack.push(file.entry.id, ...listItems(file.entry.fields.aliases));
  }
  const text = haystack.join("\n").toLowerCase();
  return words.every((w) => text.includes(w));
}

/** エントリのファイルの型（フォルダ名）。 */
export function typeOfPath(path: string): string {
  return path.split("/")[2] ?? "";
}

/** フォームに出す項目：名前（型に無ければ先頭に足す）、型の項目、型に無い項目（読むだけ）。 */
export function formFields(
  entryType: EntryType | undefined,
  entry: Entry,
): { fields: FieldDef[]; extra: string[] } {
  const typeFields = entryType?.fields ?? [];
  const fields: FieldDef[] = [];
  if (!typeFields.some((f) => f.key === "name")) {
    fields.push({
      key: "name",
      label: "名前",
      type: "text",
      kind: { kind: "text" },
      options: [],
    });
  }
  fields.push(...typeFields);
  const known = new Set(["id", ...fields.map((f) => f.key)]);
  const extra = Object.keys(entry.fields).filter((k) => !known.has(k));
  return { fields, extra };
}

/** エントリのファイルの診断を、項目（キーの位置が同じもの）ごとに分ける。キーに当たらないものは `""`。 */
export function diagnosticsByKey(
  entry: Entry | null,
  diagnostics: readonly IrDiagnostic[],
): Map<string, IrDiagnostic[]> {
  const byKey = new Map<string, IrDiagnostic[]>();
  for (const d of diagnostics) {
    const key =
      entry === null
        ? ""
        : (Object.entries(entry.key_spans).find(
            ([, span]) => span[0] === d.span[0] && span[1] === d.span[1],
          )?.[0] ?? "");
    const list = byKey.get(key) ?? [];
    list.push(d);
    byKey.set(key, list);
  }
  return byKey;
}

/** 本文の改行を `\n` にそろえる（入力欄は `\n` で返すので、変わったかを比べるため）。 */
export function normalizeNewlines(text: string): string {
  return text.replace(/\r\n/g, "\n");
}

/** 一覧の項目に、重ならないキーを付ける（同じキーには何番目かを足す。リストに同じ値が2つあることがある）。 */
export function keyed<T>(
  items: readonly T[],
  keyOf: (item: T) => string,
): [string, T, number][] {
  const counts = new Map<string, number>();
  return items.map((item, index) => {
    const key = keyOf(item);
    const n = counts.get(key) ?? 0;
    counts.set(key, n + 1);
    return [n === 0 ? key : `${key}#${n}`, item, index];
  });
}

/**
 * 項目の直し方。`set` は値を決める（`seen` は直し始めたときのファイルの中身。null なら書く時点の中身）。
 * `update` は、書く時点の値から新しい値を作る（リストの足し引きを、続けて押しても取りこぼさない）。
 */
export type FieldChange =
  | {
      readonly kind: "set";
      readonly value: JsonValue;
      readonly seen: string | null;
    }
  | {
      readonly kind: "update";
      readonly update: (current: JsonValue | undefined) => Parsed;
    };

/** リストから、その値の最初の1つを除く。 */
export function withoutItem(
  value: JsonValue | undefined,
  item: string,
): JsonValue {
  if (!Array.isArray(value)) return [];
  const index = value.findIndex((v) => scalarText(v) === item);
  return index === -1 ? [...value] : removeItem(value, index);
}

/** 複数選択の選択肢を入れる・外す（書く時点の値に対して）。 */
export function toggled(
  value: JsonValue | undefined,
  option: string,
  on: boolean,
): JsonValue {
  const chosen = listItems(value);
  if (on) return chosen.includes(option) ? chosen : [...chosen, option];
  return chosen.filter((x) => x !== option);
}

/** リストの最後に足す（書く時点の値に対して。もうあれば足さない）。 */
export function appended(
  value: JsonValue | undefined,
  item: string,
): JsonValue {
  const items: JsonValue[] = Array.isArray(value) ? [...value] : [];
  return items.some((v) => scalarText(v) === item) ? items : [...items, item];
}

/** 変換中のキー操作か（変換を確定する Enter や、変換を取り消す Escape を、入力欄の操作と取り違えない）。 */
export function isComposingKey(event: {
  readonly nativeEvent: { readonly isComposing: boolean };
  readonly keyCode: number;
}): boolean {
  return event.nativeEvent.isComposing || event.keyCode === 229;
}

/**
 * ファイルの、フロントマターの後ろの本文（コアの `bible::body_of` と同じ読み方：先頭の `---` の行から次の `---`
 * の行まで。無ければ BOM を除いた全体）。
 */
export function bodyOfText(text: string): string {
  const body = text.startsWith("\uFEFF") ? text.slice(1) : text;
  const lines = body.split(/(?<=\n)/);
  if ((lines[0] ?? "").trimEnd() !== "---") return body;
  let at = lines[0]?.length ?? 0;
  for (const line of lines.slice(1)) {
    at += line.length;
    if (line.trimEnd() === "---") return body.slice(at);
  }
  return body;
}

/**
 * 立ち絵の差分の新しい名前の誤り（「＋ 差分を追加」。T3-6）。空、空白を含む（`@立ち絵 灯 笑顔 左` の区切りと
 * 取り違える）、もうある、なら理由。足せるなら null。
 */
export function expressionNameProblem(
  name: string,
  existing: readonly string[],
): string | null {
  const trimmed = name.trim();
  if (trimmed === "") return "差分の名前を入れます";
  if (/[\s\u3000]/u.test(trimmed))
    return "差分の名前に空白は使えません（@立ち絵 の区切りになります）";
  if (existing.includes(trimmed)) return `${trimmed} はもうあります`;
  return null;
}
