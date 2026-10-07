// 位置の変換。コアのスパンはUTF-8のバイト位置、CodeMirrorはUTF-16の位置を使う。
// 変換はこのモジュールだけで行う（.claude/rules/frontend.md、CLAUDE.md「コードの約束」）。
//
// CodeMirrorは、改行の `\r\n` を1つの位置として数える。そのため、ここで言う「エディタの位置」は、
// UTF-16の位置から、`\r\n` の `\r` の分を除いたものになる。

/** 1つの文字（コードポイント）のUTF-8でのバイト数。 */
function utf8Length(codePoint: number): number {
  if (codePoint < 0x80) return 1;
  if (codePoint < 0x800) return 2;
  if (codePoint < 0x10000) return 3;
  return 4;
}

/**
 * 1つの本文の中で、UTF-8のバイト位置とエディタの位置を相互に変換する。
 * 作るときに本文を1回だけ走査する。文字や `\r\n` の途中を指す位置は、その先頭に丸める。
 */
export class PositionMap {
  /** 各文字の先頭のバイト位置（最後に全体の長さ）。 */
  private readonly bytes: number[] = [];
  /** 各文字の先頭のエディタの位置（最後に全体の長さ）。`\r\n` の `\r` と `\n` は同じ位置。 */
  private readonly units: number[] = [];

  constructor(text: string) {
    let byte = 0;
    let unit = 0;
    let previous = "";
    for (const ch of text) {
      const codePoint = ch.codePointAt(0) ?? 0;
      this.bytes.push(byte);
      // `\r\n` の `\n` は、直前の `\r` と同じエディタの位置にする（合わせて1つの改行）。
      const crlf = ch === "\n" && previous === "\r";
      if (crlf) unit -= 1;
      this.units.push(unit);
      byte += utf8Length(codePoint);
      unit += ch.length;
      previous = ch;
    }
    this.bytes.push(byte);
    this.units.push(unit);
  }

  /** 全体のUTF-8のバイト数。 */
  get byteLength(): number {
    return this.bytes[this.bytes.length - 1] ?? 0;
  }

  /** 全体のエディタでの長さ。 */
  get utf16Length(): number {
    return this.units[this.units.length - 1] ?? 0;
  }

  /** UTF-8のバイト位置を、エディタの位置にする。範囲の外は両端に切り詰める。 */
  byteToUtf16(byte: number): number {
    const index = lastAtOrBefore(this.bytes, byte);
    return this.units[index] ?? 0;
  }

  /** エディタの位置を、UTF-8のバイト位置にする。範囲の外は両端に切り詰める。 */
  utf16ToByte(unit: number): number {
    const index = firstAtOrLastBefore(this.units, unit);
    return this.bytes[index] ?? 0;
  }
}

/** 昇順の配列で、`value` 以下の最後の要素の添字（無ければ0）。 */
function lastAtOrBefore(sorted: readonly number[], value: number): number {
  let low = 0;
  let high = sorted.length - 1;
  if (high < 0 || value <= (sorted[0] ?? 0)) return 0;
  while (low < high) {
    const mid = (low + high + 1) >> 1;
    if ((sorted[mid] ?? 0) <= value) low = mid;
    else high = mid - 1;
  }
  return low;
}

/**
 * 広義の昇順の配列で、`value` と等しい最初の要素の添字。等しいものが無ければ、
 * `value` より小さい最後の要素の添字（無ければ0）。`\r\n` の位置を `\r` に丸めるのに使う。
 */
function firstAtOrLastBefore(sorted: readonly number[], value: number): number {
  let low = 0;
  let high = sorted.length;
  while (low < high) {
    const mid = (low + high) >> 1;
    if ((sorted[mid] ?? 0) < value) low = mid + 1;
    else high = mid;
  }
  if (low < sorted.length && sorted[low] === value) return low;
  return Math.max(0, low - 1);
}
