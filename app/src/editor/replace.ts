// 本文を別の中身に置き換えるときの変更（外部の変更の読み込み直し、保存で付いた行ID）。
// エディタの位置（UTF-16。`\r\n` は1つに数える。position.ts と同じ）で返すので、そのまま CodeMirror に渡せる。
// 文全体を置き換えると、カーソルが先頭か末尾へ飛ぶので、共通の頭と尻尾を除いた範囲だけを置き換える。

import { type ChangeSet, MapMode, type Text } from "@codemirror/state";

export interface Replacement {
  /** 置き換える範囲（エディタの位置）。 */
  readonly from: number;
  readonly to: number;
  /** 入れる文字列（`\r\n` を含んでよい）。 */
  readonly insert: string;
}

function isHighSurrogate(code: number): boolean {
  return code >= 0xd800 && code <= 0xdbff;
}

function isLowSurrogate(code: number): boolean {
  return code >= 0xdc00 && code <= 0xdfff;
}

/** 生の文字列の位置（UTF-16）を、エディタの位置にする（それより前の `\r\n` を1つに数える）。 */
function editorOffset(text: string, index: number): number {
  let pairs = 0;
  let at = text.indexOf("\r\n");
  while (at !== -1 && at + 1 < index) {
    pairs += 1;
    at = text.indexOf("\r\n", at + 2);
  }
  return index - pairs;
}

/** 位置が、サロゲートペアか `\r\n` の間に入るか。 */
function splitsPair(text: string, index: number): boolean {
  if (index <= 0 || index >= text.length) return false;
  const before = text.charCodeAt(index - 1);
  const after = text.charCodeAt(index);
  return (
    (isHighSurrogate(before) && isLowSurrogate(after)) ||
    (before === 0x0d && after === 0x0a)
  );
}

/** `before` を `after` にする、いちばん小さい1つの置き換え。同じなら null。 */
export function replacement(before: string, after: string): Replacement | null {
  if (before === after) return null;
  const limit = Math.min(before.length, after.length);
  let prefix = 0;
  while (prefix < limit && before[prefix] === after[prefix]) prefix += 1;
  let suffix = 0;
  while (
    suffix < limit - prefix &&
    before[before.length - 1 - suffix] === after[after.length - 1 - suffix]
  ) {
    suffix += 1;
  }
  // サロゲートペアと `\r\n` を割らないよう、境目を外へずらす。
  while (
    prefix > 0 &&
    (splitsPair(before, prefix) || splitsPair(after, prefix))
  ) {
    prefix -= 1;
  }
  while (
    suffix > 0 &&
    (splitsPair(before, before.length - suffix) ||
      splitsPair(after, after.length - suffix))
  ) {
    suffix -= 1;
  }
  return {
    from: editorOffset(before, prefix),
    to: editorOffset(before, before.length - suffix),
    insert: after.slice(prefix, after.length - suffix),
  };
}

/** 行末に足す文字列（エディタの位置）。 */
export interface Insertion {
  readonly at: number;
  readonly insert: string;
}

const LINE_BREAK = /\r\n|\r|\n/;

/** 行末の行IDの候補（仕様書9節：半角空白、`^`、英数字。その後ろの行末の空白）。 */
const LINE_ID_AT_END = / \^[0-9A-Za-z]+[ \t]*$/;

/**
 * `after` が、`before` の行末に文字列を足しただけのもの（保存で付いた行ID）なら、足した位置（`before` の
 * エディタの位置）と文字列。ほかの違いがあれば null。
 */
export function appendedAtLineEnds(
  before: string,
  after: string,
): Insertion[] | null {
  const a = before.split(LINE_BREAK);
  const b = after.split(LINE_BREAK);
  if (a.length !== b.length) return null;
  const result: Insertion[] = [];
  let offset = 0;
  for (const [i, x] of a.entries()) {
    const y = b[i] ?? "";
    if (x !== y) {
      if (!y.startsWith(x)) return null;
      result.push({ at: offset + x.length, insert: y.slice(x.length) });
    }
    offset += x.length + 1;
  }
  return result;
}

/**
 * 保存で付いた行IDを、今の本文 `doc` に入れる変更（エディタの位置）。`base` は保存を頼んだ本文、
 * `saved` は書いた本文、`since` は保存を頼んでから今までの編集。
 *
 * 保存を待つ間に打った文字を消さず、付いた行IDも捨てない（捨てると、次の保存で同じ行に別の行IDが付き、
 * 書いた行IDが変わってしまう。設計の原則5）。行IDは、保存したときにあった行の、今の行末に足す
 * （その行の後ろに打った文字の後ろ。改行して足した行には付けない）。行が消されたか空になっていれば足さない。
 * 今の行がすでに行IDで終わっていれば（保存を待つ間に、行IDのある行と結合した、行IDごと貼り付けた）足さない
 * （2つ並べると、前の行IDが本文の文字列として黙って残るため）。
 * `saved` が行末に足しただけのものでなければ、編集が無いときだけ、全体を置き換える。
 */
export function savedChanges(
  base: string,
  saved: string,
  since: ChangeSet,
  doc: Text,
): Replacement[] {
  const insertions = appendedAtLineEnds(base, saved);
  if (insertions === null) {
    const whole = since.empty ? replacement(base, saved) : null;
    return whole === null ? [] : [whole];
  }
  const lines = new Set<number>();
  const changes: Replacement[] = [];
  for (const { at, insert } of insertions) {
    const pos = since.mapPos(at, -1, MapMode.TrackDel);
    if (pos === null) continue;
    const line = doc.lineAt(pos);
    if (
      lines.has(line.number) ||
      line.text.trim() === "" ||
      LINE_ID_AT_END.test(line.text)
    ) {
      continue;
    }
    lines.add(line.number);
    changes.push({ from: line.to, to: line.to, insert });
  }
  return changes;
}
