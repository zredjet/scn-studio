// プレビューのメッセージウィンドウの文（T1-11、D050）：文中の記法（ルビ、傍点、`{w}`、`{p}`、`{speed}`、
// `{se}`、変数の埋め込み、複数行の台詞の改行）を、ページごとの部品にする。
//
// ページは `{p}` で改め、ウィンドウ（`lines` 行×`chars_per_line` 字）に収まらなければ、はみ出し（W203）と
// 同じ数え方（全角1、半角0.5。ルビは親文字、傍点は中身。話者名と括弧は数えない）で折り返して分ける。

import type { IrInline } from "../core/ir.ts";
import { evaluate, show, type Variables } from "./expr.ts";

export type Piece =
  | { readonly kind: "text"; readonly text: string; readonly emphasis: boolean }
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
          pieces.push({ kind: "text", text: show(v.value), emphasis: false });
        } else {
          pieces.push({
            kind: "text",
            text: `{=${inline.expr_text}}`,
            emphasis: false,
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

function hasText(line: Line): boolean {
  return line.some((p) => visible(p) !== "");
}

/**
 * 部品を行に分ける。改行の部品で行を改め、`chars_per_line` 字を超えるところで折り返す（ルビは分けない）。
 */
function linesOf(pieces: readonly Piece[], limit: number): Line[] {
  const lines: Line[] = [[]];
  let width = 0;
  const current = () => lines[lines.length - 1] as Line;
  for (const piece of pieces) {
    if (piece.kind === "newline") {
      lines.push([]);
      width = 0;
      continue;
    }
    const text = visible(piece);
    if (text === "") {
      current().push(piece);
      continue;
    }
    const chars = piece.kind === "ruby" ? [text] : [...text];
    let buffer = "";
    const flush = () => {
      if (buffer === "") return;
      current().push(
        piece.kind === "ruby"
          ? piece
          : {
              kind: "text",
              text: buffer,
              emphasis: piece.kind === "text" && piece.emphasis,
            },
      );
      buffer = "";
    };
    for (const c of chars) {
      const w = [...c].reduce((n, x) => n + charWidth(x), 0);
      if (width + w > limit && width > 0) {
        flush();
        lines.push([]);
        width = 0;
      }
      buffer += c;
      width += w;
    }
    flush();
  }
  return lines;
}

/**
 * 部品をページに分ける。`{p}`（`wait` の -1 秒の印）で改め、`chars_per_line` 字で折り返して `lines` 行を超えれば
 * 次のページにする（はみ出し。W203 と同じ数え方）。各ページの最初と最後の空の行は数えず、描かない（台詞の
 * 括弧だけの行や、`{p}` の前後の改行は何も見せないため）。空の行にあった待ちや効果音は、隣の行へ移す。
 * 行の間には改行の部品を置く（折り返しも、ここで決めたところで改める）。
 */
export function paginate(
  pieces: readonly Piece[],
  window: { readonly lines: number; readonly chars_per_line: number },
): { pages: Page[]; overflowed: boolean } {
  const limit = Math.max(1, window.chars_per_line);
  const maxLines = Math.max(1, window.lines);
  // `{p}` で分ける。
  const chunks: Piece[][] = [[]];
  for (const piece of pieces) {
    if (piece.kind === "wait" && piece.seconds < 0) chunks.push([]);
    else (chunks[chunks.length - 1] as Piece[]).push(piece);
  }
  const pages: Page[] = [];
  let overflowed = false;
  for (const chunk of chunks) {
    const lines = linesOf(chunk, limit);
    // 最初と最後の空の行を除く（中の部品は隣の行へ移す）。
    const first = lines.findIndex(hasText);
    if (first === -1) {
      const rest = lines.flat();
      if (rest.length > 0) pages.push(rest);
      continue;
    }
    let last = lines.length - 1;
    while (last > first && !hasText(lines[last] as Line)) last -= 1;
    const kept = lines.slice(first, last + 1).map((l) => [...l]);
    (kept[0] as Line).unshift(...lines.slice(0, first).flat());
    (kept[kept.length - 1] as Line).push(...lines.slice(last + 1).flat());
    for (let i = 0; i < kept.length; i += maxLines) {
      const group = kept.slice(i, i + maxLines);
      if (i > 0) overflowed = true;
      const page: Piece[] = [];
      group.forEach((line, j) => {
        if (j > 0) page.push({ kind: "newline" });
        page.push(...line);
      });
      pages.push(page);
    }
  }
  if (pages.length === 0) pages.push([]);
  return { pages, overflowed };
}
