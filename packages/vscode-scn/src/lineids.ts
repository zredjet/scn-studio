// 行ID（行末の ` ^xxxx`。仕様書9節）の見つけ方と、見せ方の切り替え（T3-16）。VS Code に依存させず、テストする。
//
// - 行IDの候補は「1つ以上の半角空白、`^`、英数字1文字以上」で終わる行（行末の半角空白とタブは除く）。形の合わない
//   候補（大文字、長さ）も行IDの位置なので同じに扱う（見た目だけ。診断は言語サーバーが出す）。
// - コメントの行（`//`）と、`@raw` から `@end` までの中身は本文ではないので見ない（仕様書4節・15節）。
// - 位置は行の中の UTF-16 の位置（VS Code の文書の位置と同じ）。

/** 行IDの見せ方：薄く・隠す（カーソルのある行は見せる）・そのまま。 */
export type LineIdMode = "dim" | "hide" | "show";

export const LINE_ID_MODES: readonly LineIdMode[] = ["dim", "hide", "show"];

/** 次の見せ方（切り替えのコマンド）。 */
export function nextMode(mode: LineIdMode): LineIdMode {
  const index = LINE_ID_MODES.indexOf(mode);
  return LINE_ID_MODES[(index + 1) % LINE_ID_MODES.length] ?? "dim";
}

/** 設定の値を見せ方にする（知らない値は薄く）。 */
export function modeOf(value: unknown): LineIdMode {
  return LINE_ID_MODES.find((m) => m === value) ?? "dim";
}

/** 1つの行IDの位置。`gap` は前の空白の始まり（隠すときは空白ごと隠す）、`start` は `^`、`end` は終わり。 */
export interface LineIdRange {
  readonly line: number;
  readonly gap: number;
  readonly start: number;
  readonly end: number;
}

const LINE_ID = /( +)(\^[0-9A-Za-z]+)[ \t]*$/;

/** 本文の中の行IDの位置。 */
export function lineIdRanges(text: string): LineIdRange[] {
  const out: LineIdRange[] = [];
  let raw = false;
  const lines = text.split("\n");
  for (const [line, full] of lines.entries()) {
    const content = full.endsWith("\r") ? full.slice(0, -1) : full;
    const trimmed = content.replace(/^[ \t]+/, "");
    if (raw) {
      // 前後の空白（全角の空白も）を除いて `@end` だけの行で閉じる。字下げの無い `## ` の行はシーン見出しとして
      // 閉じる。
      if (
        content.replace(/^[ \t　]+|[ \t　]+$/g, "") === "@end" ||
        content.startsWith("## ")
      ) {
        raw = false;
      }
      if (raw) continue;
    }
    if (trimmed.startsWith("//")) continue;
    // 命令は半角の `@` だけ（全角の `＠raw` は地の文。仕様書7節）。
    if (/^@raw(?:[ \t　]|$)/.test(trimmed)) {
      raw = true;
      continue;
    }
    const m = LINE_ID.exec(content);
    if (m === null || m.index === 0) continue;
    const gap = m.index;
    const start = gap + (m[1]?.length ?? 0);
    out.push({ line, gap, start, end: start + (m[2]?.length ?? 0) });
  }
  return out;
}
