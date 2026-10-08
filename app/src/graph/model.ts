// フロー図のノードと辺を組み立てる（T1-9、D048）。xyflow に渡す前の、テストできる部分。
//
// - シーンの粒度：シーンをノードにし、ルート（メタ情報の `route`）ごとの帯で囲む（章とルートは入れ子に
//   ならないので、章はノードの中に書かない。章はファイルの一覧と、章の粒度で見る）。
// - 章・ルートの粒度：まとまりをノードにし、まとまりの外へ出る辺だけをつなぐ（コアの `graph`）。
// - 位置は、保存した位置（`layout.json`）を先に使い、無いノードだけ自動で置く。自動の配置は、開始の
//   ノードから辺をたどった深さを横に、深さの中の順を縦に並べる（シーンの粒度では、ルートの帯ごとに縦へ積む）。

import { prettyExpr } from "../core/expr.ts";
import type {
  GraphEdge,
  GraphGroup,
  GraphLevel,
  LayoutPoint,
  SceneGraphData,
  SceneSummary,
  Summary,
} from "../core/types.ts";

/** ノードの大きさ（docs/ui/mockups/03-flow.html）。 */
export const NODE_WIDTH = 144;
export const NODE_HEIGHT = 64;
const GROUP_WIDTH = 200;
/** 自動の配置の、段の間と行の間。 */
const COLUMN = 192;
const ROW = 84;
/** 帯の見出しの高さと、帯の内側の余白。 */
const BAND_HEADER = 36;
const BAND_PAD = 16;
const BAND_GAP = 24;

/** 状態の日本語（docs/glossary.md）。 */
const STATUS_LABELS: Readonly<Record<string, string>> = {
  draft: "下書き",
  writing: "執筆中",
  done: "完成",
  "ai-draft": "AI下書き",
};

/** ルートの無いシーンのまとまりの名前（帯と、ルートの粒度のノード）。 */
export const NO_ROUTE = "ルート未設定";

export interface FlowNode {
  readonly id: string;
  readonly type: "scene" | "cluster" | "band" | "missing";
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
  /** 位置を保存するときのキー（帯と無いシーンは保存しない）。 */
  readonly key: string | null;
  readonly data: FlowNodeData;
}

export interface FlowNodeData {
  /** 1行目（ID、ファイル名）。 */
  readonly head: string;
  /** 2行目（題名）。 */
  readonly title: string;
  /** 3行目（字数と状態など）。 */
  readonly foot: string;
  readonly stub: boolean;
  readonly aiDraft: boolean;
  readonly start: boolean;
  readonly ending: boolean;
  readonly selected: boolean;
}

export interface FlowEdge {
  readonly id: string;
  readonly source: string;
  readonly target: string;
  readonly label: string | null;
  /** 選んだノードから出る辺。 */
  readonly out: boolean;
  readonly call: boolean;
}

export interface FlowOptions {
  /** 選んだノード（シーンID、章のファイル、ルートの名前）。 */
  readonly selected: string | null;
  /** ルートで絞る（null ならすべて）。 */
  readonly route: string | null;
  /** 条件と選択肢の札を出すか。 */
  readonly showConditions: boolean;
}

/** 字数の書き方。 */
function chars(n: number): string {
  return `${n.toLocaleString("en-US")}字`;
}

/**
 * 自動の配置：開始のノード（無ければ最初のノード）から辺をたどった深さ（段）と、段の中の順。
 * たどり着けないノードは、並びの順に、続く段からたどり直す。
 */
export function rankOrder(
  ids: readonly string[],
  edges: readonly { readonly from: string; readonly to: string }[],
  start: string | null,
): Map<string, { rank: number; order: number }> {
  const known = new Set(ids);
  const next = new Map<string, string[]>();
  for (const e of edges) {
    if (!known.has(e.from) || !known.has(e.to)) continue;
    const list = next.get(e.from) ?? [];
    if (!list.includes(e.to)) list.push(e.to);
    next.set(e.from, list);
  }
  const rank = new Map<string, number>();
  let base = 0;
  const walk = (from: string) => {
    const queue = [from];
    rank.set(from, base);
    let deepest = base;
    while (queue.length > 0) {
      const current = queue.shift() ?? from;
      const r = rank.get(current) ?? base;
      for (const to of next.get(current) ?? []) {
        if (rank.has(to)) continue;
        rank.set(to, r + 1);
        deepest = Math.max(deepest, r + 1);
        queue.push(to);
      }
    }
    base = deepest + 1;
  };
  const first = start !== null && known.has(start) ? start : ids[0];
  if (first !== undefined) walk(first);
  for (const id of ids) if (!rank.has(id)) walk(id);
  const counts = new Map<number, number>();
  const result = new Map<string, { rank: number; order: number }>();
  for (const id of ids) {
    const r = rank.get(id) ?? 0;
    const order = counts.get(r) ?? 0;
    counts.set(r, order + 1);
    result.set(id, { rank: r, order });
  }
  return result;
}

/** 辺の札（条件と選択肢）。 */
export function edgeLabel(edge: GraphEdge): string | null {
  const cond = edge.cond_text === null ? null : prettyExpr(edge.cond_text);
  let text: string | null;
  switch (edge.via) {
    case "choice": {
      const option =
        edge.label === null
          ? "選択肢"
          : edge.label.length > 12
            ? `${edge.label.slice(0, 12)}…`
            : edge.label;
      text = cond === null ? option : `${option} · ${cond}`;
      break;
    }
    case "if":
      text = cond;
      break;
    case "else":
      text = "それ以外";
      break;
    default:
      text = null;
  }
  if (edge.kind === "call") return text === null ? "呼出" : `呼出 · ${text}`;
  return text;
}

/** シーンへ入る辺（入口）。 */
export function entriesOf(graph: SceneGraphData, id: string): GraphEdge[] {
  return graph.groups.flatMap((g) => g.exits.filter((e) => e.to === id));
}

/** シーンの粒度のノードの3行目。 */
function sceneFoot(group: GraphGroup, scene: SceneSummary | undefined): string {
  const n = chars(scene?.chars ?? 0);
  if (group.is_stub === true) return "スタブ";
  if (group.status === "ai-draft") return `AI下書き · ${n}`;
  const status =
    group.status === null
      ? null
      : (STATUS_LABELS[group.status] ?? group.status);
  return status === null ? n : `${n} · ${status}`;
}

/** 保存した位置（シーンIDに `constructor` などを使えるので、自分のキーだけを見る）。 */
function savedAt(
  saved: Readonly<Record<string, LayoutPoint>>,
  key: string,
): LayoutPoint | undefined {
  return Object.hasOwn(saved, key) ? saved[key] : undefined;
}

/** シーンIDごとの最初のシーン（シーンIDが重なっていれば、先に現れたほう）。 */
function firstScenes(summary: Summary): Map<string, SceneSummary> {
  const scenes = new Map<string, SceneSummary>();
  for (const s of summary.scenes) if (!scenes.has(s.id)) scenes.set(s.id, s);
  return scenes;
}

/**
 * 開始のシーン：`start_scene` がシーンの一覧にあればそれ、無ければ（書いていない、無いシーンを指す）
 * 最初のシーン（仕様書16節の到達性と同じ）。
 */
export function startSceneOf(summary: Summary): string | null {
  const start = summary.start_scene;
  if (start !== null && summary.scenes.some((s) => s.id === start)) {
    return start;
  }
  return summary.scenes[0]?.id ?? null;
}

/** シーンの粒度：シーンのノード、ルートの帯、無いシーンの仮のノード、辺。 */
function sceneFlow(
  graph: SceneGraphData,
  summary: Summary,
  saved: Readonly<Record<string, LayoutPoint>>,
  options: FlowOptions,
): { nodes: FlowNode[]; edges: FlowEdge[] } {
  // シーンIDが重なっていれば（E108）、コアはそれらを1つのまとまりにし、題名・ルート・状態を空にする。
  // 先に現れたシーンの値で補う（文脈パネルと同じ）。
  const scenes = firstScenes(summary);
  const seen = new Set<string>();
  const groups = graph.groups
    .filter((g) => {
      if (g.id === null || seen.has(g.id)) return false;
      seen.add(g.id);
      return true;
    })
    .map((g): GraphGroup => {
      const s = scenes.get(g.id ?? "");
      return {
        ...g,
        title: g.title ?? s?.title ?? null,
        route: g.route ?? s?.meta.route ?? null,
        status: g.status ?? s?.meta.status ?? null,
        is_stub: g.is_stub ?? s?.is_stub ?? null,
      };
    })
    .filter(
      (g) => options.route === null || (g.route ?? NO_ROUTE) === options.route,
    );
  const ids = groups.map((g) => g.id ?? "");
  const visible = new Set(ids);
  const allEdges = groups.flatMap((g) => g.exits);
  const startId = startSceneOf(summary);
  const placed = rankOrder(ids, allEdges, startId);
  const endings = new Set(
    groups.filter((g) => g.endings.length > 0).map((g) => g.id),
  );

  // ルートの帯ごとに、自動の配置を縦へ積む。
  const bands: string[] = [];
  for (const g of groups) {
    const route = g.route ?? NO_ROUTE;
    if (!bands.includes(route)) bands.push(route);
  }
  const auto = new Map<string, { x: number; y: number }>();
  let top = 0;
  for (const band of bands) {
    const members = groups.filter((g) => (g.route ?? NO_ROUTE) === band);
    const rows = new Map<number, number>();
    let maxRows = 1;
    for (const g of members) {
      const p = placed.get(g.id ?? "");
      if (p === undefined) continue;
      const row = rows.get(p.rank) ?? 0;
      rows.set(p.rank, row + 1);
      maxRows = Math.max(maxRows, row + 1);
      auto.set(g.id ?? "", {
        x: BAND_PAD + p.rank * COLUMN,
        y: top + BAND_HEADER + row * ROW,
      });
    }
    top += BAND_HEADER + maxRows * ROW + BAND_GAP;
  }

  const nodes: FlowNode[] = [];
  for (const g of groups) {
    const id = g.id ?? "";
    const position = savedAt(saved, id) ?? auto.get(id) ?? { x: 0, y: 0 };
    nodes.push({
      id,
      type: "scene",
      x: position.x,
      y: position.y,
      width: NODE_WIDTH,
      height: NODE_HEIGHT,
      key: id,
      data: {
        head: id,
        title: g.title ?? "",
        foot: sceneFoot(g, scenes.get(id)),
        stub: g.is_stub === true,
        aiDraft: g.status === "ai-draft",
        start: id === startId,
        ending: endings.has(id),
        selected: id === options.selected,
      },
    });
  }

  // 帯は、中のノードを囲む（人が動かしたノードにもついていく）。
  const bandOf = new Map(groups.map((g) => [g.id ?? "", g.route ?? NO_ROUTE]));
  const bandNodes: FlowNode[] = bands.map((band) => {
    const members = nodes.filter((n) => bandOf.get(n.id) === band);
    const minX = Math.min(...members.map((n) => n.x)) - BAND_PAD;
    const minY = Math.min(...members.map((n) => n.y)) - BAND_HEADER;
    const maxX = Math.max(...members.map((n) => n.x + n.width)) + BAND_PAD;
    const maxY = Math.max(...members.map((n) => n.y + n.height)) + BAND_PAD;
    return {
      id: `band:${band}`,
      type: "band",
      x: minX,
      y: minY,
      width: maxX - minX,
      height: maxY - minY,
      key: null,
      data: {
        head: "",
        title: band,
        foot: "",
        stub: false,
        aiDraft: false,
        start: false,
        ending: false,
        selected: false,
      },
    };
  });

  // 辺。無いシーンへの辺（E501）は、仮のノードへつなぐ。
  const edges: FlowEdge[] = [];
  const missing = new Map<string, FlowNode>();
  allEdges.forEach((e, i) => {
    let target = e.to;
    if (!e.to_exists || !visible.has(e.to)) {
      if (e.to_exists) return; // ルートで絞って見えないシーン。
      target = `missing:${e.to}`;
      if (!missing.has(target)) {
        const from = nodes.find((n) => n.id === e.from);
        missing.set(target, {
          id: target,
          type: "missing",
          x: (from?.x ?? 0) + COLUMN,
          y: (from?.y ?? 0) + ROW / 2,
          width: NODE_WIDTH,
          height: NODE_HEIGHT,
          key: null,
          data: {
            head: e.to,
            title: "無いシーン",
            foot: "",
            stub: false,
            aiDraft: false,
            start: false,
            ending: false,
            selected: false,
          },
        });
      }
    }
    edges.push({
      id: `e${i}`,
      source: e.from,
      target,
      label: options.showConditions ? edgeLabel(e) : null,
      out: e.from === options.selected,
      call: e.kind === "call",
    });
  });
  return { nodes: [...bandNodes, ...nodes, ...missing.values()], edges };
}

/** 章・ルートの粒度：まとまりのノードと、まとまりの外へ出る辺（同じ行き先はまとめる）。 */
function groupFlow(
  graph: SceneGraphData,
  summary: Summary,
  saved: Readonly<Record<string, LayoutPoint>>,
  options: FlowOptions,
): { nodes: FlowNode[]; edges: FlowEdge[] } {
  const keyOf = (id: string | null) => id ?? "";
  const ids = graph.groups.map((g) => keyOf(g.id));
  const pairs = graph.groups.flatMap((g) =>
    g.exits
      .filter((e) => e.to_exists)
      .map((e) => ({ from: keyOf(g.id), to: keyOf(e.to_group), edge: e })),
  );
  const startId = startSceneOf(summary);
  const startGroup =
    startId === null
      ? undefined
      : graph.groups.find((g) => g.scenes.includes(startId));
  const placed = rankOrder(
    ids,
    pairs,
    startGroup === undefined ? null : keyOf(startGroup.id),
  );
  const byScene = new Map(summary.scenes.map((s) => [s.id, s]));
  const nodes: FlowNode[] = graph.groups.map((g) => {
    const key = keyOf(g.id);
    const p = placed.get(key) ?? { rank: 0, order: 0 };
    const position = savedAt(saved, key) ?? {
      x: p.rank * (GROUP_WIDTH + 48),
      y: p.order * ROW,
    };
    const total = g.scenes.reduce(
      (n, id) => n + (byScene.get(id)?.chars ?? 0),
      0,
    );
    const stubs = g.scenes.filter(
      (id) => byScene.get(id)?.is_stub === true,
    ).length;
    const head =
      graph.level === "chapter"
        ? (g.id?.replace(/^story\//, "") ?? "")
        : "ルート";
    const title =
      graph.level === "chapter"
        ? (summary.scenes.find((s) => s.id === g.scenes[0])?.title ?? "")
        : (g.id ?? NO_ROUTE);
    return {
      id: `g:${key}`,
      type: "cluster",
      x: position.x,
      y: position.y,
      width: GROUP_WIDTH,
      height: NODE_HEIGHT,
      key,
      data: {
        head,
        title,
        foot: `${g.scenes.length}シーン · ${chars(total)}${stubs > 0 ? ` · スタブ ${stubs}` : ""}`,
        stub: false,
        aiDraft: false,
        start: g === startGroup,
        ending: g.endings.length > 0,
        selected: key === options.selected,
      },
    };
  });
  const merged = new Map<
    string,
    { from: string; to: string; edges: GraphEdge[] }
  >();
  for (const p of pairs) {
    const id = `${p.from}->${p.to}`;
    const entry = merged.get(id) ?? { from: p.from, to: p.to, edges: [] };
    entry.edges.push(p.edge);
    merged.set(id, entry);
  }
  const edges: FlowEdge[] = [...merged.values()].map((m, i) => {
    const first = m.edges[0];
    const label =
      !options.showConditions || first === undefined
        ? null
        : m.edges.length > 1
          ? `${m.edges.length}本`
          : `${first.from} → ${first.to}`;
    return {
      id: `g${i}`,
      source: `g:${m.from}`,
      target: `g:${m.to}`,
      label,
      out: m.from === options.selected,
      call: m.edges.every((e) => e.kind === "call"),
    };
  });
  return { nodes, edges };
}

/** フロー図のノードと辺。 */
export function buildFlow(
  graph: SceneGraphData,
  summary: Summary,
  saved: Readonly<Record<string, LayoutPoint>>,
  options: FlowOptions,
): { nodes: FlowNode[]; edges: FlowEdge[] } {
  return graph.level === "scene"
    ? sceneFlow(graph, summary, saved, options)
    : groupFlow(graph, summary, saved, options);
}

/** 凡例の数。 */
export interface LegendCounts {
  readonly stubs: number;
  readonly aiDrafts: number;
  readonly endings: number;
  readonly deadEnds: number;
  readonly unreachable: number;
}

export function legendCounts(
  summary: Summary,
  checks: readonly { readonly code: string }[],
  graph: SceneGraphData | null,
): LegendCounts {
  return {
    stubs: summary.scenes.filter((s) => s.is_stub).length,
    aiDrafts: summary.scenes.filter((s) => s.meta.status === "ai-draft").length,
    endings: graph?.groups.reduce((n, g) => n + g.endings.length, 0) ?? 0,
    deadEnds: checks.filter((d) => d.code === "W503").length,
    unreachable: checks.filter((d) => d.code === "W504").length,
  };
}

/** 粒度の日本語。 */
export const LEVEL_LABELS: Readonly<Record<GraphLevel, string>> = {
  route: "ルート",
  chapter: "章",
  scene: "シーン",
};

/**
 * スタブを足したときのノードの位置（T2-9）：選んだシーンの次の列の、少し下（同じ行のノードと重ねにくくする）。
 * 選んでいなければ、いちばん右のノードの次の列。
 */
export function newStubPosition(
  points: readonly { readonly x: number; readonly y: number }[],
  from: { readonly x: number; readonly y: number } | null,
): { x: number; y: number } {
  const column = 192;
  const drop = 96;
  if (from !== null) return { x: from.x + column, y: from.y + drop };
  if (points.length === 0) return { x: 0, y: 0 };
  const right = points.reduce((a, b) => (b.x > a.x ? b : a));
  return { x: right.x + column, y: right.y };
}
