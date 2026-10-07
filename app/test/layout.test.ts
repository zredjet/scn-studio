// シーンのグラフの自動配置のテスト。

import assert from "node:assert/strict";
import { test } from "node:test";
import type { Ir, IrEdge, IrScene } from "../src/core/types.ts";
import { layoutScenes } from "../src/graph/layout.ts";

function scene(id: string): IrScene {
  return {
    id,
    title: id,
    file: "story/a.scn",
    span: [0, 0],
    meta: { summary: null, status: null, route: null, tags: [], extra: {} },
    is_stub: false,
    ops: [],
  };
}

function edge(from: string, to: string): IrEdge {
  return {
    from,
    to,
    kind: "jump",
    via: "direct",
    label: null,
    cond_text: null,
    span: [0, 0],
  };
}

function ir(
  ids: string[],
  edges: [string, string][],
  start: string | null,
): Ir {
  return {
    ir_version: 1,
    start_scene: start,
    scenes: ids.map(scene),
    edges: edges.map(([from, to]) => edge(from, to)),
    diagnostics: [],
  };
}

test("開始シーンからの深さで段を決め、段の中は順に並べる", () => {
  const placed = layoutScenes(
    ir(
      ["a", "b", "c", "d"],
      [
        ["a", "b"],
        ["a", "c"],
        ["b", "d"],
        ["c", "d"],
      ],
      "a",
    ),
  );
  assert.deepEqual(
    placed.map((p) => [p.rank, p.order]),
    [
      [0, 0],
      [1, 0],
      [1, 1],
      [2, 0],
    ],
  );
});

test("たどり着けないシーンは、続く段に置く。存在しない飛び先と重複したIDでも止まらない", () => {
  const placed = layoutScenes(
    ir(
      ["s1", "s2", "x", "s1"],
      [
        ["s1", "s2"],
        ["s2", "missing"],
        ["x", "s1"],
      ],
      "s1",
    ),
  );
  assert.deepEqual(
    placed.map((p) => p.rank),
    [0, 1, 2, 3],
  );
});

test("開始シーンが無ければ、最初のシーンから始める。シーンが無くてもよい", () => {
  assert.deepEqual(
    layoutScenes(ir(["a", "b"], [["a", "b"]], null)).map((p) => p.rank),
    [0, 1],
  );
  assert.deepEqual(layoutScenes(ir([], [], "a")), []);
});
