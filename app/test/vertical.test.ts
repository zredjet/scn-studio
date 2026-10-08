// 縦書きビューの組み方（preview/vertical.ts、T2-5、D056）のテスト：縦中横、折り返し（ルビと縦中横は分けない、
// 半角は0.5字）、段落（小説の形、注記）、ページ、原稿用紙換算。

import assert from "node:assert/strict";
import { test } from "node:test";
import type { ReadingItem } from "../src/core/types.ts";
import {
  clampSize,
  manuscriptSheets,
  pageOfParagraph,
  paginate,
  paragraphsOf,
  tokensOf,
  wrap,
} from "../src/preview/vertical.ts";

const t = (text: string) => ({ kind: "text" as const, text });

test("縦中横はちょうど2桁の半角数字だけ。傍点は1字ずつ。改行で列を分ける", () => {
  assert.deepEqual(tokensOf([t("16時と3時と123番")]), [
    [
      { kind: "tcy", text: "16" },
      { kind: "text", text: "時と3時と123番" },
    ],
  ]);
  assert.deepEqual(
    tokensOf([
      { kind: "emphasis", text: "強調" },
      { kind: "line_break" },
      t("次"),
    ]),
    [
      [
        { kind: "dots", text: "強" },
        { kind: "dots", text: "調" },
      ],
      [{ kind: "text", text: "次" }],
    ],
  );
});

test("折り返し：全角1・半角0.5字。ルビと縦中横は途中で分けない", () => {
  const lines = wrap(
    [t("あいう"), { kind: "ruby", base: "漢字", reading: "かんじ" }, t("ab")],
    4,
  );
  assert.deepEqual(lines, [
    [t("あいう")],
    [{ kind: "ruby", base: "漢字", reading: "かんじ" }, t("ab")],
  ]);
  assert.equal(wrap([t("あいうえおか")], 3).length, 2);
  assert.equal(wrap([t("abcdef")], 3).length, 1);
  assert.deepEqual(wrap([], 3), [[]]);
});

const items: ReadingItem[] = [
  {
    kind: "scene",
    id: "s1",
    title: "屋上",
    route: "共通",
    file: "a.scn",
    span: [0, 5],
  },
  {
    kind: "say",
    file: "a.scn",
    span: [6, 9],
    line_id: "aaaa",
    speaker: "akari",
    style: "normal",
    text: [t("やあ")],
  },
  {
    kind: "say",
    file: "a.scn",
    span: [10, 12],
    line_id: null,
    speaker: "akari",
    style: "thought",
    text: [t("心")],
  },
  { kind: "break" },
  {
    kind: "narrate",
    file: "a.scn",
    span: [13, 20],
    line_id: null,
    text: [t("　地の文")],
  },
  {
    kind: "choice",
    file: "a.scn",
    span: [21, 25],
    prompt: null,
    options: [{ text: [t("行く")], enabled: true }],
    chosen: 0,
  },
  {
    kind: "scene",
    id: "s2",
    title: "",
    route: null,
    file: "a.scn",
    span: [26, 30],
  },
  { kind: "ending", id: "end" },
];
const brackets = [
  { open: "「", close: "」", style: "normal" },
  { open: "『", close: "』", style: "thought" },
];
const plain = (p: {
  tokens: readonly { kind: string; text?: string; base?: string }[];
}) => p.tokens.map((x) => x.text ?? x.base ?? "").join("");

test("段落：見出し、括弧つきの台詞（話者名は出さない）、空行、注記", () => {
  const ps = paragraphsOf(items, brackets, true);
  assert.deepEqual(
    ps.map((p) => `${p.kind}:${plain(p)}`),
    [
      "heading:屋上",
      "body:「やあ」",
      "body:『心』",
      "blank:",
      "body:　地の文",
      "note:〔選択：行く〕",
      "blank:",
      "heading:s2",
      "note:〔エンディング：end〕",
    ],
  );
  const bare = paragraphsOf(items, brackets, false);
  assert.ok(bare.every((p) => p.kind !== "note"));
});

test("ページ：行数で分け、ページの初めの空行は飛ばす。原稿用紙は本文だけ", () => {
  const ps = paragraphsOf(items, brackets, true);
  const pages = paginate(ps, 12, 3);
  // ページの途中の空行は残し、ページの初めの空行（2ページ目の頭）は飛ばす。
  assert.deepEqual(
    pages.map((p) => p.map((l) => l.kind)),
    [
      ["heading", "body", "body"],
      ["body", "note", "blank"],
      ["heading", "note"],
    ],
  );
  // 字数を超える行は折り返す（「〔エンディング：end〕」は10.5字）。
  assert.equal(paginate(ps, 10, 3)[2]?.length, 3);
  // 見出しは字下げ（3字）の分を引いて折り返す（「屋上」2字は、4字の行で2行になる）。
  assert.deepEqual(
    paginate(ps, 4, 20)[0]
      ?.filter((l) => l.kind === "heading" && l.paragraph === 0)
      .map((l) => l.tokens.length),
    [1, 1],
  );
  // 見出しの段落の番号で移る（s2 の見出しは7番目の段落）。
  assert.equal(ps[7]?.scene, "s2");
  assert.equal(pageOfParagraph(pages, 7), 2);
  assert.equal(pageOfParagraph(pages, 99), null);
  assert.equal(manuscriptSheets(ps), 1);
  const long = paragraphsOf(
    [
      {
        kind: "narrate",
        file: "a",
        span: [0, 1],
        line_id: null,
        text: [t("あ".repeat(401))],
      },
    ],
    brackets,
    false,
  );
  assert.equal(manuscriptSheets(long), 2);
});

test("段落は台詞と地の文の行IDを持つ（エディタで見せる）", () => {
  const ps = paragraphsOf(items, brackets, true);
  assert.deepEqual(
    ps.map((p) => p.lineId),
    [null, "aaaa", null, null, null, null, null, null, null],
  );
});

test("字数・行数の入力は範囲に収め、数でなければ既定の値", () => {
  assert.equal(clampSize("5", 10, 60, 40), 10);
  assert.equal(clampSize("99", 10, 60, 40), 60);
  assert.equal(clampSize("25", 10, 60, 40), 25);
  assert.equal(clampSize("", 10, 60, 40), 40);
  assert.equal(clampSize("abc", 10, 60, 40), 40);
  assert.equal(clampSize("12.7", 4, 40, 16), 12);
});
