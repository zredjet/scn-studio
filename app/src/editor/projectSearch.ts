// 作品全体の検索（すべての章。T4-22、N1）：アプリの持つ本文（保存していない変更も）を、検索の欄と同じ問い合わせで
// 探し、章ごとに並べる。「本文だけ」はコアの本文の範囲（`story_search_ranges`。バイト位置）に入る一致だけ。執筆表示
// では、隠れた行ID（同じくコアから）にかかる一致を除く（この章の検索の欄の件数とそろえる）。文字どおりに探すときの
// `\n`・`\t` などと、正規表現を1行ずつ当てることは CodeMirror の検索にそろえる。React と DOM に依存させない（テストする）。

import type { SceneSummary, SearchRanges } from "../core/types.ts";

/** 検索の欄の問い合わせ（CodeMirror の `SearchQuery` の要るところ）。 */
export interface FindSpec {
  readonly search: string;
  readonly regexp: boolean;
  readonly caseSensitive: boolean;
  readonly wholeWord: boolean;
  /** 台詞と地の文だけ。 */
  readonly bodyOnly: boolean;
}

/** 1つの一致。位置は UTF-8 のバイト（`goTo` に渡す）。 */
export interface FoundMatch {
  readonly file: string;
  readonly start: number;
  readonly end: number;
  /** 行の番号（1から）。 */
  readonly line: number;
  /** その行の文字（行IDの前まで。長ければ一致の前後だけ）。 */
  readonly before: string;
  readonly match: string;
  readonly after: string;
}

export interface FoundFile {
  readonly file: string;
  readonly matches: readonly FoundMatch[];
}

export interface FoundAll {
  readonly files: readonly FoundFile[];
  readonly total: number;
  /** 上限で探すのをやめた。 */
  readonly capped: boolean;
  /** 正規表現が正しくない。 */
  readonly invalid: boolean;
}

/** 探す一致の上限。 */
export const FIND_ALL_LIMIT = 1000;
/** 一致の前後に見せる字数。 */
const CONTEXT_BEFORE = 16;
const CONTEXT_AFTER = 32;

/** UTF-16 の文字列の UTF-8 での長さ（サロゲートペアは2つで4バイト）。 */
function utf8Length(text: string): number {
  let n = 0;
  for (let i = 0; i < text.length; i++) {
    const c = text.charCodeAt(i);
    if (c < 0x80) n += 1;
    else if (c < 0x800) n += 2;
    else if (c >= 0xd800 && c <= 0xdbff) {
      n += 4;
      i += 1;
    } else n += 3;
  }
  return n;
}

/** CodeMirror と同じく、文字どおりに探す文字の `\n`・`\r`・`\t`・`\\` を読む（`literal: false`）。 */
function unquote(text: string): string {
  return text.replace(/\\([nrt\\])/g, (_, ch: string) =>
    ch === "n" ? "\n" : ch === "r" ? "\r" : ch === "t" ? "\t" : "\\",
  );
}

function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

const WORD = /[\p{L}\p{N}_]/u;

/** 問い合わせを正規表現にする（正しくなければ null）。 */
export function findRegExp(spec: FindSpec): RegExp | null {
  if (spec.search === "") return null;
  const source = spec.regexp ? spec.search : escapeRegExp(unquote(spec.search));
  try {
    return new RegExp(source, spec.caseSensitive ? "gmu" : "gimu");
  } catch {
    return null;
  }
}

/** バイトの範囲 `[start, end)` が、並んだ範囲のどれかに入るか。 */
export function insideRanges(
  ranges: readonly (readonly [number, number])[],
  start: number,
  end: number,
): boolean {
  let low = 0;
  let high = ranges.length - 1;
  while (low <= high) {
    const mid = (low + high) >> 1;
    const r = ranges[mid];
    if (r === undefined) return false;
    if (end <= r[0]) high = mid - 1;
    else if (start >= r[1]) low = mid + 1;
    else return start >= r[0] && end <= r[1];
  }
  return false;
}

/** バイトの範囲 `[start, end)` が、並んだ範囲のどれかに重なるか。 */
export function overlapsRanges(
  ranges: readonly (readonly [number, number])[],
  start: number,
  end: number,
): boolean {
  return ranges.some(([s, e]) => start < e && end > s);
}

/**
 * 正規表現が行をまたいで当たってよいか（CodeMirror と同じく、改行や `\s` などを含む式だけ。ほかは1行ずつ）。
 * 文字どおりに探すときは、探す文字に改行があるときだけ。
 */
function multiline(spec: FindSpec): boolean {
  return spec.regexp
    ? /\\[sWDnr]|\n|\r|\[\^/.test(spec.search)
    : /[\n\r]/.test(unquote(spec.search));
}

/** 行の見せ方から、終わりの行IDを除く（見せるだけ）。 */
function withoutLineId(line: string): string {
  return line.replace(/[ \t]+\^[0-9a-z]{4,12}\s*$/, "");
}

/**
 * すべての章を探す。`files` は章と今の本文（保存していない変更も）、`ranges` は章ごとの検索の範囲（本文は「本文だけ」
 * のとき、行IDは `skipLineIds`（執筆表示）のときに使う。範囲の無い章は、本文だけなら一致なし、行IDは除かない）。
 * 章の順は `files` の順。
 */
export function findAll(
  files: readonly { readonly file: string; readonly text: string }[],
  spec: FindSpec,
  ranges: ReadonlyMap<string, SearchRanges>,
  skipLineIds: boolean,
  limit = FIND_ALL_LIMIT,
): FoundAll {
  const re = findRegExp(spec);
  if (re === null)
    return {
      files: [],
      total: 0,
      capped: false,
      invalid: spec.search !== "",
    };
  const lines = multiline(spec);
  const out: FoundFile[] = [];
  let total = 0;
  for (const { file, text } of files) {
    const matches: FoundMatch[] = [];
    const bodyRanges = spec.bodyOnly ? (ranges.get(file)?.text ?? []) : null;
    const idRanges = skipLineIds ? (ranges.get(file)?.line_ids ?? []) : null;
    re.lastIndex = 0;
    let at = 0;
    let atByte = 0;
    let line = 1;
    let lineStart = 0;
    for (let m = re.exec(text); m !== null; m = re.exec(text)) {
      if (m[0] === "") {
        re.lastIndex += 1;
        continue;
      }
      const from = m.index;
      const to = from + m[0].length;
      // 行と位置を進める。
      for (let i = at; i < from; i++) {
        if (text.charCodeAt(i) === 10) {
          line += 1;
          lineStart = i + 1;
        }
      }
      atByte += utf8Length(text.slice(at, from));
      at = from;
      const start = atByte;
      const end = start + utf8Length(m[0]);
      if (spec.wholeWord) {
        const prev = text.slice(Math.max(0, from - 1), from);
        const next = text.slice(to, to + 1);
        if (
          (prev !== "" && WORD.test(prev)) ||
          (next !== "" && WORD.test(next))
        )
          continue;
      }
      if (!lines && /[\n\r]/.test(m[0])) continue;
      if (bodyRanges !== null && !insideRanges(bodyRanges, start, end))
        continue;
      if (idRanges !== null && overlapsRanges(idRanges, start, end)) continue;
      const lineEnd = text.indexOf("\n", from);
      const lineText = text
        .slice(lineStart, lineEnd === -1 ? text.length : lineEnd)
        .replace(/\r$/, "");
      const col = from - lineStart;
      const shown = withoutLineId(lineText);
      matches.push({
        file,
        start,
        end,
        line,
        before: shown.slice(Math.max(0, col - CONTEXT_BEFORE), col),
        match: m[0],
        after: shown.slice(
          col + m[0].length,
          col + m[0].length + CONTEXT_AFTER,
        ),
      });
      total += 1;
      if (total >= limit) {
        out.push({ file, matches });
        return { files: out, total, capped: true, invalid: false };
      }
    }
    if (matches.length > 0) out.push({ file, matches });
  }
  return { files: out, total, capped: false, invalid: false };
}

/** 一致のあるシーン（その章で、一致より前（同じ位置を含む）に見出しのある最後のシーン）。無ければ null。 */
export function sceneAt(
  scenes: readonly SceneSummary[],
  match: { readonly file: string; readonly start: number },
): SceneSummary | null {
  let at: SceneSummary | null = null;
  for (const s of scenes) {
    if (s.file !== match.file || s.span[0] > match.start) continue;
    if (at === null || s.span[0] >= at.span[0]) at = s;
  }
  return at;
}
