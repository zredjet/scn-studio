// TextMate の文法（syntaxes/scn.tmLanguage.json。T3-16）のテスト：サンプルの全行の行の種類が、Lezer の文法
// （packages/lezer-scn）の期待値と同じこと。文中の記法・式・行IDの scope。

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { lineKinds, scopeOf, tokenize } from "./support.ts";

const STORY = new URL("../../../samples/houkago/story/", import.meta.url);
const LEZER = new URL("../../lezer-scn/test/expected/", import.meta.url);

for (const chapter of ["chapter01", "chapter02", "chapter03"]) {
  test(`サンプル ${chapter}.scn の行の種類が Lezer の文法と同じ`, async () => {
    const source = readFileSync(new URL(`${chapter}.scn`, STORY), "utf8");
    const expected = readFileSync(new URL(`${chapter}.txt`, LEZER), "utf8")
      .split("\n")
      .filter((l) => l !== "")
      .map((l) => l.slice(4).split(" ")[0] ?? "");
    const kinds = await lineKinds(source);
    const lines = source.split("\n");
    const diff = expected
      .map((kind, i) =>
        kind === kinds[i]
          ? null
          : `${i + 1}: ${kind} ≠ ${kinds[i]}  ${lines[i]}`,
      )
      .filter((d) => d !== null);
    assert.deepEqual(diff, []);
  });
}

test("見出し・メタ情報・行ID", async () => {
  const [heading, , meta, , dialogue] = await tokenize(
    "## 放課後の屋上 #s012\n---\nstatus: draft\n---\n灯（？？？）「やっと来た」 ^k3f9\n",
  );
  assert.equal(
    scopeOf(heading ?? [], "放課後の屋上"),
    "entity.name.section.scene.scn",
  );
  assert.equal(scopeOf(heading ?? [], "#s012"), "entity.name.tag.scene-id.scn");
  assert.equal(scopeOf(meta ?? [], "status"), "variable.other.meta-key.scn");
  assert.equal(scopeOf(meta ?? [], "draft"), "string.unquoted.meta-value.scn");
  assert.equal(
    scopeOf(dialogue ?? [], "灯"),
    "entity.name.function.speaker.scn",
  );
  assert.equal(
    scopeOf(dialogue ?? [], "（？？？）"),
    "entity.other.display-name.scn",
  );
  assert.equal(scopeOf(dialogue ?? [], "^k3f9"), "comment.line-id.scn");
});

test("文中の記法", async () => {
  const [line] = await tokenize(
    "　|乱《みだ》れ、灯《あかり》と《《傍点》》{w 0.5}{=湊.名前}{{ ^a1b2",
  );
  const tokens = line ?? [];
  assert.equal(scopeOf(tokens, "乱"), "markup.underline.ruby-base.scn");
  assert.equal(scopeOf(tokens, "みだ"), "string.other.ruby-reading.scn");
  assert.equal(scopeOf(tokens, "灯"), "markup.underline.ruby-base.scn");
  assert.equal(scopeOf(tokens, "傍点"), "markup.bold.emphasis-dots.scn");
  assert.equal(
    scopeOf(tokens, "{w 0.5}"),
    "support.function.inline-command.scn",
  );
  assert.equal(scopeOf(tokens, "湊.名前"), "variable.other.interpolation.scn");
  assert.equal(scopeOf(tokens, "{{"), "constant.character.escape.brace.scn");
  assert.equal(scopeOf(tokens, "^a1b2"), "comment.line-id.scn");
});

test("選択肢・条件・ジャンプ・式", async () => {
  const [option, cond, set, jump] = await tokenize(
    "- 黙ってうなずく if 勇気 >= 2 -> s014 ^k3fe\n@if 灯.好感度 >= 3 and not 雨\n@設定 勇気 += 1\n-> s013a\n",
  );
  assert.equal(scopeOf(option ?? [], "if"), "keyword.control.conditional.scn");
  assert.equal(scopeOf(option ?? [], "勇気"), "variable.other.scn");
  assert.equal(scopeOf(option ?? [], "s014"), "entity.name.tag.scene-ref.scn");
  assert.equal(scopeOf(option ?? [], "^k3fe"), "comment.line-id.scn");
  assert.equal(scopeOf(cond ?? [], "灯.好感度"), "variable.other.scn");
  assert.equal(scopeOf(cond ?? [], ">="), "keyword.operator.comparison.scn");
  assert.equal(scopeOf(cond ?? [], "and"), "keyword.operator.logical.scn");
  assert.equal(scopeOf(cond ?? [], "3"), "constant.numeric.scn");
  assert.equal(scopeOf(set ?? [], "+="), "keyword.operator.assignment.scn");
  assert.equal(scopeOf(jump ?? [], "s013a"), "entity.name.tag.scene-ref.scn");
});

test("生コードの中は字句に分けない。全角の ＠ と \\ で始まる行は地の文", async () => {
  const kinds = await lineKinds(
    "@raw renpy\n  $ x = 1 ^abcd\n## 見出しではない\n@end\n＠背景 屋上\n\\@背景 屋上\n",
  );
  assert.deepEqual(kinds.slice(0, 6), [
    "RawStart",
    "RawLine",
    "SceneHeading",
    "Directive",
    "Narration",
    "Narration",
  ]);
});

/** 壊れた入力や小説の体裁の行でも、行の種類が Lezer と同じ（期待値は Lezer の lineKinds で確かめたもの）。 */
const LEZER_CASES: readonly [string, readonly string[]][] = [
  // 本文の `---` は地の文（見出しの直後だけがメタ情報）。
  [
    "地の文。\n---\n灯「こんにちは」\n@背景 屋上\n-> s2\n",
    ["Narration", "Narration", "Dialogue", "Directive", "Jump"],
  ],
  [
    "## 見出し #s1\n\n---\n灯「やあ」\n",
    ["SceneHeading", "Blank", "Narration", "Dialogue"],
  ],
  // 閉じていないメタ情報は、`キー: 値` でない行で終わる（E102 の回復）。
  [
    "## 見出し #s1\n---\nsummary: x\n\n灯「やあ」\n@背景 屋上\n",
    [
      "SceneHeading",
      "MetaDelimiter",
      "MetaEntry",
      "Blank",
      "Dialogue",
      "Directive",
    ],
  ],
  // 閉じ括弧の後ろに文が続けば地の文。インライン命令と行IDだけなら台詞。
  [
    "彼は「行こう」と言った。\n灯「またね」{p}\n灯「またね」 ^abcd\n",
    ["Narration", "Dialogue", "Dialogue"],
  ],
  // 傍点は自動ルビではない。
  ["今日《《こそ》》は。\n", ["Narration"]],
  // `->` で始まればジャンプ。`@raw-x` は生コードではない。`-` の後ろがタブなら地の文。話者に半角の () を含められる。
  [
    "-> s1 extra\n-> 帰り道\n@raw-x\n灯「やあ」\n-\tfoo\nA(B)「やあ」\n",
    ["Jump", "Jump", "Directive", "Dialogue", "Narration", "Dialogue"],
  ],
  // 全角の空白で挟んだ `@end` でも生コードを閉じる。
  [
    "@raw renpy\n  x\n　@end　\n灯「やあ」\n",
    ["RawStart", "RawLine", "RawEnd", "Dialogue"],
  ],
  // 複数行の台詞の中の、同じ種類の括弧の入れ子。
  [
    "灯「彼は\n「行く」\nと言った」\n@背景 屋上\n",
    ["Dialogue", "Dialogue", "Dialogue", "Directive"],
  ],
  // 違う種類の閉じ括弧では閉じない（命令の行で終える）。
  ["灯「やあ』\n@背景 屋上\n", ["Dialogue", "Directive"]],
  // 最初の閉じ括弧の後ろに文が続く行は地の文（行末が閉じ括弧でも）。後ろの行を飲み込まない。
  [
    "灯は「またね」と手を振った。「明日ね」\n地の文A\n灯「やあ」\n地の文B\n",
    ["Narration", "Narration", "Dialogue", "Narration"],
  ],
  ["灯「a」b」\n地の文\n", ["Narration", "Narration"]],
];

test("複数行の台詞の閉じ括弧の後ろに文が続いても、次の行で地の文に戻る", async () => {
  // Lezer は1・2行目も地の文にする（先を読む）。TextMate は行をまたいで先を読めないので、1・2行目は台詞の色になる。
  const kinds = await lineKinds(
    "灯「あのね、\n実は」と言いかけた。\n地の文C\n灯「やあ」\n",
  );
  assert.deepEqual(kinds.slice(2, 4), ["Narration", "Dialogue"]);
});

for (const [source, expected] of LEZER_CASES) {
  test(`行の種類が Lezer と同じ：${JSON.stringify(source.split("\n")[0])}…`, async () => {
    const kinds = await lineKinds(source);
    assert.deepEqual(kinds.slice(0, expected.length), expected);
  });
}

test("傍点・台詞の後ろのインライン命令・ジャンプの誤り", async () => {
  const [emphasis, after, jump] = await tokenize(
    "今日《《こそ》》は。\n灯「またね」{p}\n-> 帰り道\n",
  );
  assert.equal(
    scopeOf(emphasis ?? [], "こそ"),
    "markup.bold.emphasis-dots.scn",
  );
  assert.equal(
    scopeOf(after ?? [], "{p}"),
    "support.function.inline-command.scn",
  );
  assert.equal(scopeOf(jump ?? [], "帰り道"), "invalid.illegal.scene-ref.scn");
});
