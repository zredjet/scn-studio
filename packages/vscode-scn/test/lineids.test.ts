// 行IDの見つけ方と見せ方（src/lineids.ts。T3-16）のテスト。

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { lineIdRanges, modeOf, nextMode } from "../src/lineids.ts";

test("行末の ^xxxx を、前の空白ごと見つける（UTF-16 の位置）", () => {
  const text =
    "灯「😀やっと来た」 ^k3f9\n　地の文。  ^a1b2  \r\n行IDの無い行\n";
  assert.deepEqual(lineIdRanges(text), [
    // 😀 は UTF-16 で2単位。
    { line: 0, gap: 10, start: 11, end: 16 },
    { line: 1, gap: 5, start: 7, end: 12 },
  ]);
});

test("空白の無い ^ や、^ の後ろが英数字でないものは行IDではない", () => {
  assert.deepEqual(lineIdRanges("顔文字^_^\n値^abc\n ^k3f9\n"), []);
});

test("コメントと @raw の中は見ない。字下げの無い ## でも @raw を閉じる", () => {
  const text = [
    "// - 案 ^abcd",
    "@raw renpy",
    "  $ x = 1 ^efgh",
    "@end",
    "地の文。 ^ijkl",
    "@raw renpy",
    "## 次の場面 #s2 ^mnop",
    "地の文。 ^qrst",
  ].join("\n");
  assert.deepEqual(
    lineIdRanges(text).map((r) => r.line),
    [4, 6, 7],
  );
});

test("全角の ＠raw は生コードではない。全角の空白で挟んだ @end でも閉じる", () => {
  const text = [
    "＠raw renpy",
    "地の文。 ^abcd",
    "@raw renpy",
    "  x ^efgh",
    "　@end　",
    "地の文。 ^ijkl",
  ].join("\n");
  assert.deepEqual(
    lineIdRanges(text).map((r) => r.line),
    [1, 5],
  );
});

test("サンプルの行IDをすべて見つける", () => {
  const text = readFileSync(
    new URL("../../../samples/houkago/story/chapter01.scn", import.meta.url),
    "utf8",
  );
  const ids = lineIdRanges(text).map((r) => {
    const line = text.split("\n")[r.line] ?? "";
    return line.slice(r.start, r.end);
  });
  assert.ok(ids.includes("^k3f9"));
  assert.ok(
    ids.every((id) => /^\^[0-9a-z]+$/.test(id)),
    ids.join(" "),
  );
});

test("見せ方の切り替え：薄く → 隠す → そのまま → 薄く。知らない値は薄く", () => {
  assert.equal(nextMode("dim"), "hide");
  assert.equal(nextMode("hide"), "show");
  assert.equal(nextMode("show"), "dim");
  assert.equal(modeOf("hide"), "hide");
  assert.equal(modeOf("消す"), "dim");
  assert.equal(modeOf(undefined), "dim");
});
