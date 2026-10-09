// 変数の画面（variables/rows.ts、T1-10、D049）のテスト：型と使用箇所の見せ方、名前空間のキャラクター、
// 絞り込み、シーンごとの読み書き。

import assert from "node:assert/strict";
import { test } from "node:test";
import type { VariableRow } from "../src/core/types.ts";
import {
  characterOf,
  groupRows,
  locationLines,
  locationsByScene,
  matchesVariable,
  namespaceCounts,
  namespaceOf,
  problemsOf,
  rangeText,
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

test("名前空間と、問題で絞る（書いて読まない・使っていない・宣言されていない。T4-18）", () => {
  const row = (key: string, over: Partial<VariableRow> = {}): VariableRow => ({
    ...ROW,
    key,
    reads: [],
    writes: [],
    ...over,
  });
  const rows = [
    row("flag.勇気", { writes: [at("s010", 1)], reads: [at("s012", 2)] }),
    row("akari.好感度", { writes: [at("s011", 3)] }),
    row("akari.転校済み"),
    row("周回", { declared: false, reads: [at("s020", 4)] }),
  ];
  assert.equal(namespaceOf("akari.好感度"), "akari");
  assert.equal(namespaceOf("周回"), "周回");
  assert.deepEqual(problemsOf(rows[0] as VariableRow), []);
  assert.deepEqual(problemsOf(rows[1] as VariableRow), ["written-not-read"]);
  assert.deepEqual(problemsOf(rows[2] as VariableRow), ["unused"]);
  assert.deepEqual(problemsOf(rows[3] as VariableRow), ["undeclared"]);
  assert.deepEqual(namespaceCounts(rows, [{ id: "akari", name: "灯" }]), [
    { namespace: "flag", who: null, count: 1 },
    { namespace: "akari", who: "灯", count: 2 },
    { namespace: "周回", who: null, count: 1 },
  ]);
  const keys = (g: ReturnType<typeof groupRows>) =>
    g.map((x) => [x.namespace, x.rows.map((r) => r.key)]);
  assert.deepEqual(
    keys(groupRows(rows, { namespace: null, problem: null, query: "" })),
    [
      ["flag", ["flag.勇気"]],
      ["akari", ["akari.好感度", "akari.転校済み"]],
      ["周回", ["周回"]],
    ],
  );
  assert.deepEqual(
    keys(groupRows(rows, { namespace: "akari", problem: null, query: "" })),
    [["akari", ["akari.好感度", "akari.転校済み"]]],
  );
  assert.deepEqual(
    keys(groupRows(rows, { namespace: null, problem: "unused", query: "" })),
    [["akari", ["akari.転校済み"]]],
  );
  assert.deepEqual(
    keys(groupRows(rows, { namespace: null, problem: null, query: "勇気" })),
    [["flag", ["flag.勇気"]]],
  );
  assert.equal(rangeText(ROW), "0〜10");
  assert.equal(
    rangeText({ ...ROW, type: "enum", values: ["共通", "灯"] }),
    "共通・灯",
  );
  assert.equal(rangeText({ ...ROW, range: null }), "—");
});

test("書く場所・読む場所の行（バイト位置から行の番号と、その行の文字）", () => {
  const text = "## 屋上 #s1\n灯「やあ」\n  @設定 灯.好感度 += 1\n";
  const at2 = new TextEncoder().encode(
    "## 屋上 #s1\n灯「やあ」\n  @設定 ",
  ).length;
  const lines = locationLines(
    [{ scene: "s1", file: "story/a.scn", span: [at2, at2 + 3] }],
    (file) => (file === "story/a.scn" ? text : undefined),
  );
  assert.deepEqual(lines, [
    {
      scene: "s1",
      file: "story/a.scn",
      offset: at2,
      text: "@設定 灯.好感度 += 1",
      line: 3,
    },
  ]);
  const unknown = locationLines(
    [{ scene: "s1", file: "story/b.scn", span: [0, 1] }],
    () => undefined,
  );
  assert.equal(unknown[0]?.line, null);
});
