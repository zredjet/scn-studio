// 診断のクイックフィックス（T2-8、D059）。React と DOM に依存させない（テストする）。
//
// - W406（行頭の全角の `＠`・`－＞`・`ー＞`・`＞`・`＃＃`）：半角にする（`@`・`->`・`->`・`->`・`## `）か、
//   地の文のままにする（行頭に `\` を足す。仕様書4節）。

/** W406 の全角記号と、半角にした書き方。 */
const FULLWIDTH: Readonly<Record<string, string>> = {
  "＠": "@",
  "－＞": "->",
  "ー＞": "->",
  "＞": "->",
  "＃＃": "##",
};

/**
 * W406 の記号を半角にした書き方。シーン見出し（`##`）とジャンプ（`->`）は後ろに半角空白が要る（`## ` でないと
 * 見出しにならない）ので、後ろの文字（`next`）が全角空白かタブならそれを半角空白に置き換え（`consume` が1）、
 * 空白でなければ半角空白を足す。知らない記号なら null。
 */
export function halfwidthOf(
  symbol: string,
  next: string,
): { readonly insert: string; readonly consume: number } | null {
  const half = FULLWIDTH[symbol];
  if (half === undefined) return null;
  if (half === "@" || next === "" || next === " ")
    return { insert: half, consume: 0 };
  if (next === "　" || next === "\t") return { insert: `${half} `, consume: 1 };
  return { insert: `${half} `, consume: 0 };
}
