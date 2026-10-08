// 変数の画面（variables/rows.ts、T1-10、D049）のテスト：型と使用箇所の見せ方、名前空間のキャラクター、
// 絞り込み、シーンごとの読み書き。

import assert from "node:assert/strict";
import { test } from "node:test";
import type { VariableRow } from "../src/core/types.ts";
import {
  characterOf,
  locationsByScene,
  matchesVariable,
  typeText,
  usageText,
} from "../src/variables/rows.ts";

const at = (scene: string, start: number) => ({
  scene,
  file: "story/a.scn",
  span: [start, start + 1] as const,
});

const ROW: VariableRow = {
  key: "akari.好感度",
  declared: true,
  type: "int",
  values: [],
  default: "0",
  range: [0, 10],
  scope: "save",
  desc: "灯の好感度",
  span: [10, 20],
  reads: [at("s010", 50), at("s020", 5)],
  writes: [at("s010", 40), at("s010", 30)],
};

test("型と使用箇所の見せ方", () => {
  assert.equal(typeText(ROW), "int 0〜10");
  assert.equal(
    typeText({ ...ROW, type: "enum", values: ["共通", "灯"], range: null }),
    "enum（共通・灯）",
  );
  assert.equal(typeText({ ...ROW, type: null, declared: false }), "宣言なし");
  assert.equal(usageText(ROW), "書き 2 · 読み 2");
});

test("名前空間のキャラクターと絞り込み", () => {
  const characters = [{ id: "akari", name: "灯" }];
  assert.equal(characterOf(ROW.key, characters), "灯");
  assert.equal(characterOf("flag.勇気", characters), null);
  assert.ok(matchesVariable(ROW, "好感"));
  assert.ok(matchesVariable(ROW, "AKARI 灯の"));
  assert.ok(!matchesVariable(ROW, "勇気"));
  assert.ok(matchesVariable(ROW, ""));
});

test("読み書きはシーンごとにまとめ、最初の位置へ移る", () => {
  assert.deepEqual(locationsByScene(ROW), [
    { scene: "s010", file: "story/a.scn", offset: 30, writes: 2, reads: 1 },
    { scene: "s020", file: "story/a.scn", offset: 5, writes: 0, reads: 1 },
  ]);
});
