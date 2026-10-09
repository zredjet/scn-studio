// フロー図の組み立て（graph/model.ts、T1-9、D048）のテスト。保存した位置と自動の配置、ルートの帯、
// 無いシーン、粒度ごとのノードと辺、入口、凡例の数、開始の札、重なったシーンID。

import assert from "node:assert/strict";
import { test } from "node:test";
import { branchTargetText, routeNames } from "../src/ai/model.ts";
import type {
  GraphEdge,
  GraphGroup,
  SceneGraphData,
  SceneSummary,
  Summary,
} from "../src/core/types.ts";
import {
  buildFlow,
  edgeLabel,
  entriesOf,
  labelPlacement,
  legendCounts,
  NO_ROUTE,
  newStubPosition,
  proposalOverlay,
  READABLE_ZOOM,
  rankOrder,
  readableViewport,
  routeCounts,
  sceneOfDiagnostic,
  startSceneOf,
  structureProblems,
  withLabelSlots,
} from "../src/graph/model.ts";

function scene(
  id: string,
  route: string | null,
  extra: Partial<SceneSummary> = {},
): SceneSummary {
  return {
    id,
    title: `題${id}`,
    file: "story/a.scn",
    span: [0, 0],
    meta: { summary: null, status: null, route, tags: [], extra: {} },
    is_stub: false,
    chars: 100,
    ...extra,
  };
}

function edge(
  from: string,
  to: string,
  extra: Partial<GraphEdge> = {},
): GraphEdge {
  return {
    from,
    to,
    kind: "jump",
    via: "direct",
    label: null,
    cond_text: null,
    to_exists: true,
    to_group: to,
    ...extra,
  };
}

function group(
  id: string,
  route: string | null,
  exits: GraphEdge[],
  extra: Partial<GraphGroup> = {},
): GraphGroup {
  return {
    id,
    title: `題${id}`,
    file: "story/a.scn",
    route,
    status: null,
    is_stub: false,
    scenes: [id],
    exits,
    endings: [],
    reads: [],
    writes: [],
    ...extra,
  };
}

const SUMMARY: Summary = {
  start_scene: "a",
  chapters: [],
  scenes: [
    scene("a", "共通"),
    scene("b", "共通"),
    scene("c", "灯", { is_stub: true }),
  ],
  edges: [],
  diagnostics: [],
};

const GRAPH: SceneGraphData = {
  level: "scene",
  groups: [
    group("a", "共通", [
      edge("a", "b"),
      edge("a", "c", { via: "if", cond_text: "x >= 2" }),
    ]),
    group("b", "共通", [edge("b", "zz", { to_exists: false, to_group: null })]),
    group("c", "灯", [], { is_stub: true, endings: ["end1"] }),
  ],
};

const OPTIONS = { selected: null, route: null, showConditions: true };

test("自動の配置：開始から辺をたどった深さで段を決め、たどり着けないものは続く段に置く", () => {
  const placed = rankOrder(
    ["a", "b", "c", "d", "x"],
    [
      { from: "a", to: "b" },
      { from: "a", to: "c" },
      { from: "b", to: "d" },
      { from: "c", to: "d" },
      { from: "x", to: "missing" },
    ],
    "a",
  );
  assert.deepEqual(
    ["a", "b", "c", "d", "x"].map((id) => {
      const p = placed.get(id);
      return [p?.rank, p?.order];
    }),
    [
      [0, 0],
      [1, 0],
      [1, 1],
      [2, 0],
      [3, 0],
    ],
  );
  // 開始が無ければ最初から。何も無くてもよい。
  assert.equal(
    rankOrder(["a", "b"], [{ from: "a", to: "b" }], null).get("b")?.rank,
    1,
  );
  assert.equal(rankOrder([], [], "a").size, 0);
});

test("シーンの粒度：保存した位置を先に使い、無いノードだけ自動で置く。ルートの帯で囲む", () => {
  const flow = buildFlow(GRAPH, SUMMARY, { b: { x: 500, y: 600 } }, OPTIONS);
  const byId = new Map(flow.nodes.map((n) => [n.id, n]));
  assert.deepEqual([byId.get("b")?.x, byId.get("b")?.y], [500, 600]);
  const a = byId.get("a");
  const c = byId.get("c");
  assert.ok(a !== undefined && c !== undefined);
  // 灯の帯は、共通の帯の下に積む。
  assert.ok(c.y > a.y);
  // 帯は中のノードを囲む。
  const band = byId.get("band:共通");
  assert.ok(band !== undefined);
  for (const n of [a, byId.get("b")]) {
    assert.ok(n !== undefined);
    assert.ok(band.x <= n.x && band.y <= n.y);
    assert.ok(band.x + band.width >= n.x + n.width);
    assert.ok(band.y + band.height >= n.y + n.height);
  }
  // 3行目：字数と状態、スタブ。開始とエンディングの印。
  assert.equal(a.data.foot, "100字");
  assert.ok(a.data.start);
  assert.equal(c.data.foot, "スタブ");
  assert.ok(c.data.stub && c.data.ending);
  // 位置を保存するキーは、シーンIDだけ（帯と無いシーンは保存しない）。
  assert.equal(band.key, null);
  assert.equal(a.key, "a");
});

test("シーンの粒度：無いシーンへの辺は仮のノードへつなぎ、条件は札にする", () => {
  const flow = buildFlow(GRAPH, SUMMARY, {}, OPTIONS);
  assert.ok(flow.nodes.some((n) => n.id === "missing:zz" && n.key === null));
  assert.ok(
    flow.edges.some((e) => e.source === "b" && e.target === "missing:zz"),
  );
  assert.equal(flow.edges.find((e) => e.target === "c")?.label, "x ≥ 2");
  const hidden = buildFlow(
    GRAPH,
    SUMMARY,
    {},
    { ...OPTIONS, showConditions: false },
  );
  assert.equal(hidden.edges.find((e) => e.target === "c")?.label, null);
});

test("シーンの粒度：ルートで絞ると、そのルートのシーンだけを出す", () => {
  const flow = buildFlow(GRAPH, SUMMARY, {}, { ...OPTIONS, route: "共通" });
  const ids = flow.nodes.filter((n) => n.type === "scene").map((n) => n.id);
  assert.deepEqual(ids, ["a", "b"]);
  // 見えないシーンへの辺は出さない。
  assert.ok(!flow.edges.some((e) => e.target === "c"));
});

test("章とルートの粒度：まとまりをノードにし、同じ行き先の辺はまとめる", () => {
  const graph: SceneGraphData = {
    level: "route",
    groups: [
      group(
        "共通",
        null,
        [
          edge("a", "c", { to_group: "灯" }),
          edge("b", "c", { to_group: "灯" }),
        ],
        {
          id: "共通",
          scenes: ["a", "b"],
        },
      ),
      group("灯", null, [], { id: "灯", scenes: ["c"], endings: ["end1"] }),
      group("x", null, [], { id: null, scenes: [] }),
    ],
  };
  const flow = buildFlow(graph, SUMMARY, { 灯: { x: 9, y: 9 } }, OPTIONS);
  assert.deepEqual(
    flow.nodes.map((n) => [n.id, n.key]),
    [
      ["g:共通", "共通"],
      ["g:灯", "灯"],
      ["g:", ""],
    ],
  );
  const common = flow.nodes[0];
  assert.equal(common?.data.foot, "2シーン · 200字");
  assert.ok(common?.data.start);
  assert.equal(flow.nodes[2]?.data.title, NO_ROUTE);
  assert.deepEqual([flow.nodes[1]?.x, flow.nodes[1]?.y], [9, 9]);
  assert.deepEqual(
    flow.edges.map((e) => [e.source, e.target, e.label]),
    [["g:共通", "g:灯", "2本"]],
  );
});

test("辺の札、入口、凡例の数", () => {
  assert.equal(
    edgeLabel(
      edge("a", "b", {
        via: "choice",
        label: "黙ってうなずく",
        cond_text: "勇気 >= 2",
      }),
    ),
    "黙ってうなずく · 勇気 ≥ 2",
  );
  assert.equal(edgeLabel(edge("a", "b", { via: "else" })), "それ以外");
  assert.equal(edgeLabel(edge("a", "b", { kind: "call" })), "呼出");
  assert.equal(edgeLabel(edge("a", "b")), null);
  assert.deepEqual(
    entriesOf(GRAPH, "c").map((e) => e.from),
    ["a"],
  );
  assert.deepEqual(
    legendCounts(
      SUMMARY,
      [{ code: "W503" }, { code: "W504" }, { code: "W504" }],
      GRAPH,
    ),
    { stubs: 1, aiDrafts: 0, endings: 1, deadEnds: 1, unreachable: 2 },
  );
});

test("開始の札：start_scene が無いか、無いシーンを指すときは最初のシーン（章・ルートの粒度も同じ）", () => {
  const noStart = { ...SUMMARY, start_scene: "zz" };
  assert.equal(startSceneOf(noStart), "a");
  assert.equal(startSceneOf({ ...SUMMARY, start_scene: null }), "a");
  assert.equal(
    startSceneOf({ ...SUMMARY, scenes: [], start_scene: null }),
    null,
  );
  const flow = buildFlow(GRAPH, noStart, {}, OPTIONS);
  assert.ok(flow.nodes.find((n) => n.id === "a")?.data.start);
  const chapters: SceneGraphData = {
    level: "chapter",
    groups: [
      group("story/a.scn", null, [], { scenes: ["a", "b"] }),
      group("story/b.scn", null, [], { scenes: ["c"] }),
    ],
  };
  const grouped = buildFlow(chapters, noStart, {}, OPTIONS);
  assert.deepEqual(
    grouped.nodes.map((n) => n.data.start),
    [true, false],
  );
  // ルートの無いまとまり（id が null）には、開始を含まなければ札を付けない。
  const routes: SceneGraphData = {
    level: "route",
    groups: [
      group("共通", null, [], { scenes: ["a", "b"] }),
      group("x", null, [], { id: null, scenes: ["c"] }),
    ],
  };
  assert.deepEqual(
    buildFlow(routes, noStart, {}, OPTIONS).nodes.map((n) => n.data.start),
    [true, false],
  );
});

test("重なったシーンID（E108）は、先に現れたシーンの題名とルートで出す。constructor などのIDも置ける", () => {
  const summary: Summary = {
    ...SUMMARY,
    scenes: [
      scene("a", "共通"),
      scene("a", "灯"),
      scene("constructor", "共通"),
    ],
  };
  const graph: SceneGraphData = {
    level: "scene",
    groups: [
      group("a", null, [], {
        title: null,
        status: null,
        is_stub: null,
        scenes: ["a", "a"],
      }),
      group("constructor", "共通", []),
    ],
  };
  const flow = buildFlow(graph, summary, {}, OPTIONS);
  const a = flow.nodes.find((n) => n.id === "a");
  assert.equal(a?.data.title, "題a");
  assert.ok(flow.nodes.some((n) => n.id === "band:共通"));
  assert.ok(!flow.nodes.some((n) => n.id === `band:${NO_ROUTE}`));
  const c = flow.nodes.find((n) => n.id === "constructor");
  assert.ok(c !== undefined && Number.isFinite(c.x) && Number.isFinite(c.y));
  // ルートで絞っても消えない。
  const filtered = buildFlow(graph, summary, {}, { ...OPTIONS, route: "共通" });
  assert.ok(filtered.nodes.some((n) => n.id === "a"));
});

test("スタブを足したときのノードの位置（T2-9）", () => {
  assert.deepEqual(newStubPosition([], null), { x: 0, y: 0 });
  assert.deepEqual(
    newStubPosition(
      [
        { x: 0, y: 0 },
        { x: 300, y: 80 },
      ],
      null,
    ),
    { x: 492, y: 80 },
  );
  assert.deepEqual(newStubPosition([{ x: 0, y: 0 }], { x: 10, y: 20 }), {
    x: 202,
    y: 116,
  });
});

test("提案ノード（T3-10）：元のシーンの次の列に縦に並べ、元から辺を引く", () => {
  const items = ["A", "B", "C"].map((letter, i) => ({
    key: `7-${i}`,
    letter,
    label: `案${letter}`,
    target: "s013a に合流",
    problem: i === 2,
  }));
  const { nodes, edges } = proposalOverlay({ x: 100, y: 200 }, "s014", items);
  assert.deepEqual(
    nodes.map((n) => [n.id, n.x, n.y]),
    [
      ["proposal:7-0", 340, 116],
      ["proposal:7-1", 340, 200],
      ["proposal:7-2", 340, 284],
    ],
  );
  assert.deepEqual(nodes[0]?.data, {
    head: "AI提案 A",
    title: "案A",
    foot: "s013a に合流",
    problem: false,
    key: "s014",
  });
  assert.equal(nodes[2]?.data.problem, true);
  assert.deepEqual(
    edges.map((e) => [e.source, e.target]),
    [
      ["s014", "proposal:7-0"],
      ["s014", "proposal:7-1"],
      ["s014", "proposal:7-2"],
    ],
  );
  assert.deepEqual(proposalOverlay({ x: 0, y: 0 }, "s1", []), {
    nodes: [],
    edges: [],
  });
});

test("提案ノードの行き先の文と、ルートの名前の候補（T3-10）", () => {
  const scenes = [
    scene("s013a", "灯", { title: "打ち明ける" }),
    scene("s020", "紬"),
  ];
  const base = {
    name: "案",
    from: "s014",
    label: "追いかける",
    effects: [],
    reason: null,
    problems: [],
  };
  assert.equal(
    branchTargetText({ ...base, target: "s013a", stubs: [] }, scenes),
    "s013a「打ち明ける」に合流",
  );
  assert.equal(
    branchTargetText(
      {
        ...base,
        target: "s015",
        stubs: [{ id: "s015", title: "紬と会う", summary: null }],
      },
      scenes,
    ),
    "新規 s015 · 紬と会う",
  );
  assert.equal(
    branchTargetText({ ...base, target: "s999", stubs: [] }, scenes),
    "s999（無いシーン）",
  );
  assert.deepEqual(
    routeNames([...scenes, scene("s021", "灯"), scene("s001", null)]),
    ["灯", "紬"],
  );
});

test("辺の札（T3-1、H3）：札を出すときは段の間を広げ、同じ行き先に入る札は上下に振り分ける", () => {
  const withLabels = buildFlow(GRAPH, SUMMARY, {}, OPTIONS);
  const without = buildFlow(
    GRAPH,
    SUMMARY,
    {},
    {
      ...OPTIONS,
      showConditions: false,
    },
  );
  const x = (flow: typeof withLabels, id: string) =>
    flow.nodes.find((n) => n.id === id)?.x ?? Number.NaN;
  // a（段0）と b（段1）の間：札を出すときは札が入るだけ空ける。
  assert.ok(
    x(withLabels, "b") - x(withLabels, "a") > x(without, "b") - x(without, "a"),
  );
  assert.equal(x(without, "b") - x(without, "a"), 192);
  assert.equal(x(withLabels, "b") - x(withLabels, "a"), 144 + 176);

  const edges = withLabelSlots([
    {
      id: "1",
      source: "a",
      target: "t",
      label: "行く",
      labelSlot: 0,
      out: false,
      call: false,
    },
    {
      id: "2",
      source: "b",
      target: "t",
      label: null,
      labelSlot: 0,
      out: false,
      call: false,
    },
    {
      id: "3",
      source: "c",
      target: "t",
      label: "x ≥ 2",
      labelSlot: 0,
      out: false,
      call: false,
    },
    {
      id: "4",
      source: "a",
      target: "u",
      label: "それ以外",
      labelSlot: 0,
      out: false,
      call: false,
    },
    {
      id: "5",
      source: "d",
      target: "t",
      label: "戻る",
      labelSlot: 0,
      out: false,
      call: false,
    },
  ]);
  assert.deepEqual(
    edges.map((e) => e.labelSlot),
    [0, 0, 1, 0, 2],
  );
  // 順 0 は線の上、1 は線の下、2 はさらに上。
  assert.deepEqual(labelPlacement(0), { above: true, shift: -3 });
  assert.deepEqual(labelPlacement(1), { above: false, shift: 3 });
  assert.deepEqual(labelPlacement(2), { above: true, shift: -23 });
  // 組み立てた辺にも振ってある。
  assert.ok(withLabels.edges.every((e) => e.labelSlot === 0));
});

test("フローの初めの表示（T4-13、U6）：収めた縮尺で読めればそのまま、小さすぎれば読める縮尺で開始のあたり", () => {
  const bounds = { x: -100, y: 50, width: 4000, height: 600 };
  const pane = { width: 1000, height: 700 };
  assert.equal(
    readableViewport({ x: 0, y: 0, zoom: 0.9 }, bounds, null, pane),
    null,
  );
  // 開始のノードが無ければ、図の左上を窓の左上（余白24）に。
  assert.deepEqual(
    readableViewport({ x: 0, y: 0, zoom: 0.3 }, bounds, null, pane),
    {
      x: 24 + 100 * READABLE_ZOOM,
      y: 24 - 50 * READABLE_ZOOM,
      zoom: READABLE_ZOOM,
    },
  );
  // 開始のノードがあれば、その縦の真ん中を窓の縦の真ん中に。
  const start = { x: -100, y: 300, width: 144, height: 64 };
  assert.deepEqual(
    readableViewport({ x: 0, y: 0, zoom: 0.3 }, bounds, start, pane, 0.8),
    { x: 24 + 100 * 0.8, y: 350 - 332 * 0.8, zoom: 0.8 },
  );
});

test("フローの概要：構造の問題・問題のシーン・ルートごとの数（T4-18、U15）", () => {
  const diag = (code: string, file: string, at: number) => ({
    code,
    severity: "warning",
    file,
    span: [at, at + 1] as [number, number],
    message: code,
  });
  const checks = [
    diag("W503", "story/a.scn", 120),
    diag("W203", "story/a.scn", 10),
    diag("W701", "concept.md", 0),
    diag("W504", "story/a.scn", 5),
  ];
  assert.deepEqual(
    structureProblems(checks).map((d) => d.code),
    ["W503", "W701", "W504"],
  );
  const scenes = [
    scene("a", "共通", { span: [0, 4] }),
    scene("b", "共通", { span: [100, 104] }),
    scene("c", "灯", { file: "story/b.scn", span: [0, 4] }),
    scene("d", null, { span: [200, 204] }),
  ];
  assert.equal(
    sceneOfDiagnostic(scenes, diag("W503", "story/a.scn", 120))?.id,
    "b",
  );
  assert.equal(
    sceneOfDiagnostic(scenes, diag("W504", "story/a.scn", 0))?.id,
    "a",
  );
  assert.equal(sceneOfDiagnostic(scenes, diag("W701", "concept.md", 0)), null);
  assert.deepEqual(routeCounts(scenes), [
    { route: "共通", count: 2 },
    { route: "灯", count: 1 },
    { route: NO_ROUTE, count: 1 },
  ]);
});
