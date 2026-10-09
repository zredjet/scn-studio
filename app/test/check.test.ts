// 検証の画面のまとめ方と、自動巡回（検証の段階2。T3-2）の結果の見せ方のテスト。

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  countBySeverity,
  diagnosticPlaces,
  groupDiagnostics,
} from "../src/check/group.ts";
import {
  exampleText,
  quote,
  sortedEndings,
  summaryText,
  valuesText,
} from "../src/check/playthrough.ts";
import type {
  IrDiagnostic,
  Playthrough,
  SceneSummary,
} from "../src/core/types.ts";

function diag(
  file: string,
  start: number,
  code = "W503",
  severity: IrDiagnostic["severity"] = "warning",
): IrDiagnostic {
  return { code, severity, file, span: [start, start], message: code };
}

test("本文のファイルを先に、ほかのファイルをパスの順に並べ、ファイルの中は位置の順", () => {
  const groups = groupDiagnostics(
    [
      diag("variables.yaml", 3, "W605"),
      diag("story/b.scn", 10),
      diag("concept.md", 1, "W701"),
      diag("story/a.scn", 20),
      diag("story/a.scn", 5, "I302", "info"),
    ],
    ["story/a.scn", "story/b.scn"],
  );
  assert.deepEqual(
    groups.map((g) => [g.file, g.story, g.diagnostics.map((d) => d.span[0])]),
    [
      ["story/a.scn", true, [5, 20]],
      ["story/b.scn", true, [10]],
      ["concept.md", false, [1]],
      ["variables.yaml", false, [3]],
    ],
  );
});

test("同じ診断は1つにする", () => {
  const groups = groupDiagnostics(
    [diag("story/a.scn", 1), diag("story/a.scn", 1)],
    ["story/a.scn"],
  );
  assert.equal(groups[0]?.diagnostics.length, 1);
});

test("重大度ごとの件数", () => {
  assert.deepEqual(
    countBySeverity([
      diag("a", 0, "E501", "error"),
      diag("a", 1),
      diag("a", 2),
      diag("a", 3, "I302", "info"),
    ]),
    { error: 1, warning: 2, info: 1 },
  );
});

// ---- 自動巡回の結果の見せ方（T3-2） ----

test("自動巡回：道の例は「」で囲み（台詞の形はそのまま）、多ければ12個まで。着かないエンディングを先に並べる", () => {
  assert.equal(quote("すぐに行く"), "「すぐに行く」");
  assert.equal(quote("「ごめん」"), "「ごめん」");
  assert.equal(exampleText([]), "選択肢を選ばずに");
  assert.equal(exampleText(["a", "「b」"]), "選択肢 「a」 → 「b」");
  assert.equal(
    exampleText(["行く", "〔周回〕", "真のルート"]),
    "選択肢 「行く」 → 〔周回〕 → 「真のルート」",
  );
  const many = Array.from({ length: 14 }, (_, i) => `${i}`);
  assert.ok(exampleText(many).endsWith("「11」 → …"));
  const report: Playthrough = {
    complete: true,
    limit: null,
    states: 3,
    starts: 2,
    scenes: { reached: 11, total: 13 },
    options: { reached: 3, total: 4 },
    endings: [
      {
        id: "akari_end",
        scene: "s040",
        reached: true,
        states: 1,
        example: ["すぐに行く"],
        values: { "flag.勇気": [1], "flag.ルート": ["灯", "紬"] },
        more: ["flag.ルート"],
      },
      {
        id: "bad_end",
        scene: "s061",
        reached: false,
        states: 0,
        example: [],
        values: {},
        more: [],
      },
    ],
    diagnostics: [],
  };
  assert.deepEqual(
    sortedEndings(report).map((e) => e.id),
    ["bad_end", "akari_end"],
  );
  const first = report.endings[0];
  assert.ok(first !== undefined);
  assert.deepEqual(valuesText(first), [
    "flag.勇気 = 1",
    'flag.ルート = "灯" | "紬" | …',
  ]);
  assert.equal(
    summaryText(report),
    "シーン 11/13 · 選べた選択肢 3/4 · 状態 3 · 周回 2通り",
  );
});

function sceneAt(id: string, file: string, start: number): SceneSummary {
  return {
    id,
    title: id,
    file,
    span: [start, start + 1],
    meta: { summary: null, status: null, route: null, tags: [], extra: {} },
    is_stub: false,
    chars: 0,
  };
}

test("診断の位置（T4-13、U17）：行はバイト位置までの改行の数、シーンは前で始まるいちばん後ろのシーン", () => {
  // 「あ」は UTF-8 で3バイト。行の終わりが \r\n でも数える改行は \n。
  const text = "## a #s1\r\nあ\r\n## b #s2\r\n本文\r\n";
  const second = Buffer.byteLength("## a #s1\r\nあ\r\n", "utf8");
  const body = Buffer.byteLength("## a #s1\r\nあ\r\n## b #s2\r\n", "utf8");
  const scenes = [
    sceneAt("s1", "story/a.scn", 0),
    sceneAt("s2", "story/a.scn", second),
    sceneAt("x", "story/other.scn", 0),
  ];
  assert.deepEqual(
    diagnosticPlaces(
      [
        diag("story/a.scn", 0),
        diag("story/a.scn", second - 1),
        diag("story/a.scn", second),
        diag("story/a.scn", body),
      ],
      text,
      scenes,
    ),
    [
      { line: 1, scene: "s1" },
      { line: 2, scene: "s1" },
      { line: 3, scene: "s2" },
      { line: 4, scene: "s2" },
    ],
  );
  // シーンの前の位置（章の見出しなど）と、ほかのファイルのシーンは使わない。
  assert.deepEqual(
    diagnosticPlaces(
      [diag("story/b.scn", Buffer.byteLength("# 章\n", "utf8"))],
      "# 章\n本文\n",
      scenes,
    ),
    [{ line: 2, scene: null }],
  );
});
