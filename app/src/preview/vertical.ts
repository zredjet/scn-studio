// 縦書きビュー（T2-5、D056。docs/ui/mockups/06b-preview-vertical.html）の組み方：通読の並びを、段落と行と
// ページにする。React と DOM に依存させない（テストする）。
//
// - 小説の形にする：話者名は出さず、台詞は表示スタイルの括弧で囲む。シーンの見出し、選んだ選択肢とエンディングの
//   注記（`〔選択：…〕`）。
// - 1行の字数で折り返し、1ページの行数でページを分ける。全角を1、半角を0.5と数える（はみ出しの W203 と同じ）。
//   ルビは親文字の字数、2桁の半角数字は縦中横で1字。ルビと縦中横は途中で折り返さない。禁則は考えない
//   （確認用。厳密な組版は PDF・EPUB の書き出しで行う）。
// - 原稿用紙換算：20字×20行で、本文（見出しと注記を除く）を折り返した行の数から求める。

import type { IrInline } from "../core/ir.ts";
import type { ReadingItem } from "../core/types.ts";
import { charWidth } from "./text.ts";

/** 縦書きの1つのまとまり。 */
export type Token =
  | { readonly kind: "text"; readonly text: string }
  | { readonly kind: "ruby"; readonly base: string; readonly reading: string }
  /** 傍点の付いた文字。 */
  | { readonly kind: "dots"; readonly text: string }
  /** 縦中横（2桁の半角数字）。 */
  | { readonly kind: "tcy"; readonly text: string };

export type ParagraphKind = "heading" | "body" | "note" | "blank";

export interface Paragraph {
  readonly kind: ParagraphKind;
  readonly tokens: readonly Token[];
  /** シーンの見出しなら、そのシーン。 */
  readonly scene: string | null;
  /** 本文の位置（エディタで見せる）。 */
  readonly file: string | null;
  readonly span: readonly [number, number] | null;
  /** 台詞と地の文の行ID（エディタで見せる。無ければ位置で探す）。 */
  readonly lineId: string | null;
}

/** 文字の並びを、縦中横（ちょうど2桁の半角数字）とそれ以外に分ける。 */
function textTokens(text: string): Token[] {
  const out: Token[] = [];
  const pattern = /(?<![0-9])[0-9]{2}(?![0-9])/g;
  let at = 0;
  for (const match of text.matchAll(pattern)) {
    const index = match.index ?? 0;
    if (index > at) out.push({ kind: "text", text: text.slice(at, index) });
    out.push({ kind: "tcy", text: match[0] });
    at = index + match[0].length;
  }
  if (at < text.length) out.push({ kind: "text", text: text.slice(at) });
  return out;
}

/** 文中の記法を、縦書きのまとまりにする。複数行の台詞の改行で、まとまりの列を分ける。 */
export function tokensOf(text: readonly IrInline[]): Token[][] {
  const lines: Token[][] = [[]];
  const push = (token: Token) => lines[lines.length - 1]?.push(token);
  for (const inline of text) {
    switch (inline.kind) {
      case "text":
        for (const token of textTokens(inline.text)) push(token);
        break;
      case "ruby":
        push({ kind: "ruby", base: inline.base, reading: inline.reading });
        break;
      case "emphasis":
        for (const c of inline.text) push({ kind: "dots", text: c });
        break;
      case "interpolation":
        push({ kind: "text", text: `{=${inline.expr_text}}` });
        break;
      case "line_break":
        lines.push([]);
        break;
      default:
        break;
    }
  }
  return lines;
}

/** まとまりの幅（字数）。 */
export function tokenWidth(token: Token): number {
  switch (token.kind) {
    case "text":
    case "dots":
      return [...token.text].reduce((sum, c) => sum + charWidth(c), 0);
    case "ruby":
      return [...token.base].reduce((sum, c) => sum + charWidth(c), 0);
    case "tcy":
      return 1;
  }
}

/** 段落のまとまりを、1行の字数で折り返す。文字は途中で分け、ルビと縦中横は分けない。 */
export function wrap(tokens: readonly Token[], perLine: number): Token[][] {
  const lines: Token[][] = [];
  let line: Token[] = [];
  let width = 0;
  const flush = () => {
    lines.push(line);
    line = [];
    width = 0;
  };
  const max = Math.max(1, perLine);
  for (const token of tokens) {
    if (token.kind === "text" || token.kind === "dots") {
      let chunk = "";
      for (const c of token.text) {
        const w = charWidth(c);
        if (width + w > max && (width > 0 || chunk !== "")) {
          if (chunk !== "") line.push({ kind: token.kind, text: chunk });
          chunk = "";
          flush();
        }
        chunk += c;
        width += w;
      }
      if (chunk !== "") line.push({ kind: token.kind, text: chunk });
      continue;
    }
    const w = tokenWidth(token);
    if (width + w > max && width > 0) flush();
    line.push(token);
    width += w;
  }
  if (line.length > 0 || lines.length === 0) lines.push(line);
  return lines;
}

/** 表示スタイルの括弧（見つからなければ「」）。 */
export interface Brackets {
  readonly open: string;
  readonly close: string;
  readonly style: string;
}

function plainOf(text: readonly IrInline[]): string {
  return text
    .map((i) =>
      i.kind === "text" || i.kind === "emphasis"
        ? i.text
        : i.kind === "ruby"
          ? i.base
          : "",
    )
    .join("");
}

/** 通読の並びを、段落にする。`notes` なら、選んだ選択肢とエンディングを注記する。 */
export function paragraphsOf(
  items: readonly ReadingItem[],
  brackets: readonly Brackets[],
  notes: boolean,
): Paragraph[] {
  const out: Paragraph[] = [];
  const blank = (): Paragraph => ({
    kind: "blank",
    tokens: [],
    scene: null,
    file: null,
    span: null,
    lineId: null,
  });
  const lastBlank = () =>
    out.length === 0 || out[out.length - 1]?.kind === "blank";
  for (const item of items) {
    switch (item.kind) {
      case "scene":
        if (!lastBlank()) out.push(blank());
        out.push({
          kind: "heading",
          tokens: textTokens(item.title === "" ? item.id : item.title),
          scene: item.id,
          file: item.file,
          span: item.span,
          lineId: null,
        });
        break;
      case "say":
      case "narrate": {
        const lines = tokensOf(item.text);
        if (item.kind === "say") {
          const b = brackets.find((x) => x.style === item.style);
          lines[0]?.unshift({ kind: "text", text: b?.open ?? "「" });
          lines[lines.length - 1]?.push({
            kind: "text",
            text: b?.close ?? "」",
          });
        }
        for (const tokens of lines) {
          out.push({
            kind: "body",
            tokens,
            scene: null,
            file: item.file,
            span: item.span,
            lineId: item.line_id,
          });
        }
        break;
      }
      case "break":
        if (!lastBlank()) out.push(blank());
        break;
      case "choice": {
        if (!notes) break;
        const chosen = item.options[item.chosen];
        out.push({
          kind: "note",
          tokens: textTokens(
            `〔選択：${chosen === undefined ? "" : plainOf(chosen.text)}〕`,
          ),
          scene: null,
          file: item.file,
          span: item.span,
          lineId: null,
        });
        break;
      }
      case "ending":
        if (!notes) break;
        out.push({
          kind: "note",
          tokens: textTokens(`〔エンディング：${item.id ?? ""}〕`),
          scene: null,
          file: null,
          span: null,
          lineId: null,
        });
        break;
    }
  }
  while (out.length > 0 && out[out.length - 1]?.kind === "blank") out.pop();
  return out;
}

/** ページの1行。 */
export interface PageLine {
  readonly kind: ParagraphKind;
  readonly tokens: readonly Token[];
  /** 段落の番号（エディタで見せる、シーンの位置）。 */
  readonly paragraph: number;
}

/** 見出しの字下げ（字数。CSS の `.v-line.heading` と同じ）。 */
export const HEADING_INDENT = 3;

/** 段落を、1行の字数と1ページの行数でページにする。空行も1行に数える。見出しは字下げの分を引いて折り返す。 */
export function paginate(
  paragraphs: readonly Paragraph[],
  perLine: number,
  perPage: number,
): PageLine[][] {
  const pages: PageLine[][] = [];
  let page: PageLine[] = [];
  const lines = Math.max(1, perPage);
  paragraphs.forEach((p, paragraph) => {
    const width =
      p.kind === "heading" ? Math.max(1, perLine - HEADING_INDENT) : perLine;
    const wrapped = p.kind === "blank" ? [[]] : wrap(p.tokens, width);
    for (const tokens of wrapped) {
      if (page.length >= lines) {
        pages.push(page);
        page = [];
      }
      // ページの初めの空行は飛ばす。
      if (p.kind === "blank" && page.length === 0) continue;
      page.push({ kind: p.kind, tokens, paragraph });
    }
  });
  if (page.length > 0 || pages.length === 0) pages.push(page);
  return pages;
}

/** 原稿用紙換算の枚数（20字×20行。本文だけを数える）。 */
export function manuscriptSheets(paragraphs: readonly Paragraph[]): number {
  let lines = 0;
  for (const p of paragraphs) {
    if (p.kind !== "body") continue;
    lines += wrap(p.tokens, 20).length;
  }
  return Math.ceil(lines / 20);
}

/** 段落のあるページ（シーンの一覧から、見出しの段落の番号で移る。同じシーンが2度出ても、その見出しへ）。 */
export function pageOfParagraph(
  pages: readonly PageLine[][],
  paragraph: number,
): number | null {
  const page = pages.findIndex((lines) =>
    lines.some((l) => l.paragraph === paragraph),
  );
  return page === -1 ? null : page;
}

/** 字数・行数の入力を、使える範囲の数にする（数でなければ既定の値）。 */
export function clampSize(
  raw: string,
  min: number,
  max: number,
  fallback: number,
): number {
  const n = Math.trunc(Number(raw));
  if (raw.trim() === "" || !Number.isFinite(n)) return fallback;
  return Math.min(max, Math.max(min, n));
}
