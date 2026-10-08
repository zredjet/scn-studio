// 式を読みやすく見せる（見た目だけ。`>=` を `≥` など）。執筆表示の札とフロー図の辺の札で使う。

export function prettyExpr(text: string): string {
  return text
    .replace(/>=/g, "≥")
    .replace(/<=/g, "≤")
    .replace(/!=/g, "≠")
    .replace(/\s+/g, " ")
    .trim();
}
