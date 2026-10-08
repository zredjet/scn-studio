// プレビューのメッセージウィンドウの文（T1-11、D050）：文中の記法（ルビ、傍点、`{w}`、`{p}`、`{speed}`、
// `{se}`、変数の埋め込み、複数行の台詞の改行）を、ページごとの部品にする。
//
// ページは `{p}` で改め、ウィンドウ（`lines` 行×`chars_per_line` 字）に収まらなければ、はみ出し（W203）と
// 同じ数え方（全角1、半角0.5。ルビは親文字、傍点は中身、変数の埋め込みは0。話者名と括弧は数えない）で分ける。

import type { IrInline } from "../core/ir.ts";
import { evaluate, show, type Variables } from "./expr.ts";

export type Piece =
  | {
      readonly kind: "text";
      readonly text: string;
      readonly emphasis: boolean;
      /** 変数の埋め込みの値（はみ出しの数え方では0。W203 と同じ）。 */
      readonly uncounted?: true;
    }
  | { readonly kind: "ruby"; readonly base: string; readonly reading: string }
  | { readonly kind: "newline" }
  | { readonly kind: "wait"; readonly seconds: number }
  | { readonly kind: "se"; readonly name: string }
  | { readonly kind: "speed"; readonly speed: "slow" | "normal" | "fast" };

export type Page = readonly Piece[];

/** 半角（ASCIIの印字できる文字と半角カタカナ）を0.5、ほかを1と数える。 */
export function charWidth(c: string): number {
  const code = c.codePointAt(0) ?? 0;
  if (code >= 0x20 && code <= 0x7e) return 0.5;
  if (code >= 0xff61 && code <= 0xff9f) return 0.5;
  return 1;
}

/** 文中の記法を部品にする（変数の埋め込みは今の値で。読めなければ `{=式}` のまま見せて知らせる）。 */
export function piecesOf(
  inlines: readonly IrInline[],
  vars: Variables,
): { pieces: Piece[]; warnings: string[] } {
  const pieces: Piece[] = [];
  const warnings: string[] = [];
  for (const inline of inlines) {
    switch (inline.kind) {
      case "text":
        pieces.push({ kind: "text", text: inline.text, emphasis: false });
        break;
      case "emphasis":
        pieces.push({ kind: "text", text: inline.text, emphasis: true });
        break;
      case "ruby":
        pieces.push({
          kind: "ruby",
          base: inline.base,
          reading: inline.reading,
        });
        break;
      case "wait":
        pieces.push({ kind: "wait", seconds: inline.seconds });
        break;
      case "se":
        pieces.push({ kind: "se", name: inline.name });
        break;
      case "speed":
        pieces.push({ kind: "speed", speed: inline.speed });
        break;
      case "line_break":
        pieces.push({ kind: "newline" });
        break;
      case "page_break":
        // ページの区切りの印（負の秒数の待ち）。ページに分けるときに使う。
        pieces.push({ kind: "wait", seconds: -1 });
        break;
      case "interpolation": {
        const v = inline.expr === null ? null : evaluate(inline.expr, vars);
        if (v?.ok === true) {
          pieces.push({
            kind: "text",
            text: show(v.value),
            emphasis: false,
            uncounted: true,
          });
        } else {
          pieces.push({
            kind: "text",
            text: `{=${inline.expr_text}}`,
            emphasis: false,
            uncounted: true,
          });
          warnings.push(
            `変数の埋め込み {=${inline.expr_text}} を読めません${v === null ? "" : `：${v.error}`}`,
          );
        }
        break;
      }
    }
  }
  return { pieces, warnings };
}

/** 部品の、表示する文字（数える文字）。 */
function visible(piece: Piece): string {
  if (piece.kind === "text") return piece.text;
  if (piece.kind === "ruby") return piece.base;
  return "";
}

/** 1行の部品（表示する文字と、文字の出方の部品）。 */
type Line = Piece[];

/** 数える幅（半角を1、全角を2）。変数の埋め込みは数えない（W203 と同じ）。 */
function countedWidth(piece: Piece): number {
  if (piece.kind === "text" && piece.uncounted === true) return 0;
  return [...visible(piece)].reduce((n, c) => n + charWidth(c) * 2, 0);
}

function lineWidth(line: Line): number {
  return line.reduce((n, p) => n + countedWidth(p), 0);
}

function hasText(line: Line): boolean {
  return line.some((p) => visible(p) !== "");
}

/** 複数行の台詞の改行で、行に分ける（折り返しは数えるだけで、描くときはブラウザに任せる）。 */
function linesOf(pieces: readonly Piece[]): Line[] {
  const lines: Line[] = [[]];
  for (const piece of pieces) {
    if (piece.kind === "newline") lines.push([]);
    else (lines[lines.length - 1] as Line).push(piece);
  }
  return lines;
}

/** 1行を、幅が `max` 以下の切れ端に分ける（文字の単位で。ルビは分けない）。 */
function splitLine(line: Line, max: number): Line[] {
  const parts: Line[] = [[]];
  let width = 0;
  for (const piece of line) {
    const w = countedWidth(piece);
    if (w === 0 || piece.kind !== "text") {
      if (width + w > max && width > 0) {
        parts.push([]);
        width = 0;
      }
      (parts[parts.length - 1] as Line).push(piece);
      width += w;
      continue;
    }
    let buffer = "";
    for (const c of piece.text) {
      const cw = charWidth(c) * 2;
      if (width + cw > max && width > 0) {
        if (buffer !== "")
          (parts[parts.length - 1] as Line).push({ ...piece, text: buffer });
        buffer = "";
        parts.push([]);
        width = 0;
      }
      buffer += c;
      width += cw;
    }
    if (buffer !== "")
      (parts[parts.length - 1] as Line).push({ ...piece, text: buffer });
  }
  return parts;
}

/**
 * 部品をページに分ける。数え方は W203 と同じ（`check::max_page_lines`）：`{p}`（`wait` の -1 秒の印）でページを
 * 改め、複数行の台詞の改行で行を改め、1行は幅（全角2・半角1）を `chars_per_line` の2倍で割って切り上げた
 * 行数と数える（空の行も1行）。各ページの最初と最後の空の行を1つずつ数えない（描かない。中の待ちや効果音は
 * 隣の行へ移す）。`lines` 行を超えるページは、行の単位で次のページへ送り（1行だけで超えれば、その行を文字で
 * 分け）、はみ出しとして知らせる。行の中の折り返しは、描くときにブラウザに任せる。
 */
export function paginate(
  pieces: readonly Piece[],
  window: { readonly lines: number; readonly chars_per_line: number },
): { pages: Page[]; overflowed: boolean } {
  const limit = Math.max(1, window.chars_per_line) * 2;
  const maxLines = Math.max(1, window.lines);
  const rows = (line: Line) => Math.max(1, Math.ceil(lineWidth(line) / limit));
  // `{p}` で分ける。
  const chunks: Piece[][] = [[]];
  for (const piece of pieces) {
    if (piece.kind === "wait" && piece.seconds < 0) chunks.push([]);
    else (chunks[chunks.length - 1] as Piece[]).push(piece);
  }
  const pages: Page[] = [];
  let overflowed = false;
  for (const chunk of chunks) {
    const lines = linesOf(chunk).map((l) => [...l]);
    // 最初と最後の空の行を1つずつ除く（中の部品は隣の行へ）。
    if (
      lines.length > 1 &&
      lineWidth(lines[0] as Line) === 0 &&
      !hasText(lines[0] as Line)
    ) {
      const first = lines.shift() as Line;
      (lines[0] as Line).unshift(...first);
    }
    if (
      lines.length > 1 &&
      lineWidth(lines[lines.length - 1] as Line) === 0 &&
      !hasText(lines[lines.length - 1] as Line)
    ) {
      const last = lines.pop() as Line;
      (lines[lines.length - 1] as Line).push(...last);
    }
    if (lines.length === 1 && !hasText(lines[0] as Line)) {
      if ((lines[0] as Line).length > 0) pages.push(lines[0] as Line);
      continue;
    }
    // 行の単位でページに詰める。1行で超える行は、文字で分ける。
    let page: Line[] = [];
    let used = 0;
    const flush = () => {
      if (page.length === 0) return;
      const out: Piece[] = [];
      page.forEach((line, j) => {
        if (j > 0) out.push({ kind: "newline" });
        out.push(...line);
      });
      pages.push(out);
      page = [];
      used = 0;
    };
    for (const line of lines) {
      const parts =
        rows(line) > maxLines ? splitLine(line, limit * maxLines) : [line];
      if (parts.length > 1) overflowed = true;
      for (const part of parts) {
        const r = rows(part);
        if (used + r > maxLines && page.length > 0) {
          overflowed = true;
          flush();
        }
        page.push(part);
        used += r;
      }
    }
    flush();
  }
  if (pages.length === 0) pages.push([]);
  return { pages, overflowed };
}
