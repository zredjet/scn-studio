// フローの画面（構造グラフ。docs/ui/mockups/03-flow.html、T1-9、D048）。
//
// - 粒度（ルート・章・シーン）を切り替える。データはコアの構造グラフ（`scene_graph`）。フローの画面を
//   開いているときだけ取りに行き、本文が変わったら少し待って取り直す。
// - ノードを動かしたら、その位置を `layout.json` に保存する（コアが書く）。保存した位置の無いノードは
//   自動で置く。「自動整列」はその粒度の保存した位置を捨てる。
// - ノードを選ぶと文脈パネルに出口・入口・変数を出し、ダブルクリックか「本文を開く」で本文へ移る。
// - フロー図からの編集（スタブの追加、辺を引く）はフェーズ2、AIの提案ノードはフェーズ3（T3-10）。提案ノードは、
//   選んだシーンの分岐案（AI の結果）を、元のシーンの次の列に点線の仮ノードとして重ねる（位置は保存しない）。

import {
  Background,
  Controls,
  type Edge,
  getViewportForBounds,
  MarkerType,
  type Node,
  ReactFlow,
  useNodesInitialized,
  useNodesState,
  useReactFlow,
  useStore,
} from "@xyflow/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { RunResult, TaskRunner } from "../ai/model.ts";
import { branchTargetText, proposalLetter, resultKey } from "../ai/model.ts";
import {
  addFlowEdge,
  addStub,
  confirm,
  readLayout,
  sceneGraph,
  writeLayout,
} from "../core/api.ts";
import type {
  BranchProposal,
  GraphLevel,
  IrDiagnostic,
  Layout,
  LayoutPoint,
  SceneGraphData,
  SceneSummary,
  StoryWrittenResult,
  Summary,
} from "../core/types.ts";
import { useScreenStatus } from "../shell/StatusBar.tsx";
import { flowStatus } from "../shell/status.ts";
import { LabeledEdge } from "./edges.tsx";
import { EdgeDialog, StubDialog } from "./FlowEditDialogs.tsx";
import { FlowPanel } from "./FlowPanel.tsx";
import {
  buildFlow,
  LEVEL_LABELS,
  legendCounts,
  NO_ROUTE,
  newStubPosition,
  proposalOverlay,
  readableViewport,
  startSceneOf,
} from "./model.ts";
import { nodeTypes } from "./nodes.tsx";

/** 辺の種類（札を行き先の手前に置く辺。T3-1、H3）。 */
const edgeTypes = { labeled: LabeledEdge };

interface Props {
  readonly summary: Summary;
  readonly checks: readonly IrDiagnostic[];
  readonly level: GraphLevel;
  readonly onLevel: (level: GraphLevel) => void;
  /** `layout.json` が外で変わった回数（読み直す）。 */
  readonly layoutVersion: number;
  /** 本文を開く。 */
  readonly onOpen: (scene: SceneSummary) => void;
  /** 本文の章（スタブを足す先）と、保存していない変更のある章（T2-9）。 */
  readonly chapters: readonly string[];
  readonly dirtyChapters: ReadonlySet<string>;
  /** 本文に書き足した（App がエディタと表示をそろえる）。 */
  readonly onStoryWritten: (result: StoryWrittenResult) => void;
  /** AI の分岐案（提案ノード。T3-10）。 */
  readonly ai: FlowAi;
  /** 選んでいるシーン（シーン表示で選んだときだけ。タイトルバーの再生が、そのシーンから始める。T4-17）。 */
  readonly onSelectScene: (id: string | null) => void;
}

/** フロー図の AI の分岐案（T3-10）。 */
export interface FlowAi {
  readonly runner: TaskRunner;
  /** 分岐案のタスクの最後の結果。 */
  readonly result: RunResult | undefined;
  /** 採用・却下した案。 */
  readonly hidden: ReadonlySet<string>;
  readonly onAdopt: (
    proposal: BranchProposal,
    key: string,
    model: string,
  ) => Promise<string>;
  readonly onReject: (key: string) => void;
}

/** 本文が変わってから、構造グラフを取り直すまでの時間（ミリ秒）。 */
const REFRESH_DELAY_MS = 300;
/** ノードを動かしてから、位置を保存するまでの時間（ミリ秒）。 */
const SAVE_DELAY_MS = 400;

const LEVELS: readonly GraphLevel[] = ["route", "chapter", "scene"];

const EMPTY_LAYOUT: Layout = { scene: {}, chapter: {}, route: {} };

/**
 * デザイントークンの色の値を読む。xyflow は矢印の色を SVG の id に入れるので、`var(--…)` のままでは渡さない。
 */
function tokenColor(name: string): string {
  return getComputedStyle(document.documentElement)
    .getPropertyValue(name)
    .trim();
}

/**
 * グラフを読み込んだときと、粒度を変えたときに1回、全体が収まるように表示し直す。ノードの大きさを
 * 測り終えてから行う（測る前だと、はみ出す）。収めると節の文字が読めないほど小さくなるときは、読める縮尺で
 * 開始のノードのあたりを見せる（`start` は開始のノードのID。T4-13、U6）。
 */
function FitOnLoad({
  level,
  start,
  ids,
}: {
  readonly level: GraphLevel;
  readonly start: string | null;
  /** この粒度のノードのID（粒度を変えた直後は、前の粒度のノードが残っているので、届くまで待つ）。 */
  readonly ids: ReadonlySet<string>;
}) {
  const initialized = useNodesInitialized();
  const flow = useReactFlow();
  const width = useStore((s) => s.width);
  const height = useStore((s) => s.height);
  const nodes = useStore((s) => s.nodes);
  const fitted = useRef<GraphLevel | null>(null);
  useEffect(() => {
    if (!initialized || fitted.current === level) return;
    // この粒度のノードがそろい、大きさを測り終えてから求める。
    if (
      nodes.length === 0 ||
      nodes.length !== ids.size ||
      nodes.some((n) => {
        const internal = flow.getInternalNode(n.id);
        return (
          !ids.has(n.id) ||
          internal === undefined ||
          !internal.measured.width ||
          !internal.measured.height
        );
      })
    ) {
      return;
    }
    fitted.current = level;
    // 収めた表示を先に求め（fitView と同じ式）、小さすぎるなら読める縮尺にする。
    const bounds = flow.getNodesBounds(nodes);
    const fit = getViewportForBounds(bounds, width, height, 0.2, 1, 0.08);
    const node = start === null ? undefined : flow.getInternalNode(start);
    const view = readableViewport(
      fit,
      bounds,
      node === undefined
        ? null
        : {
            ...node.internals.positionAbsolute,
            width: node.measured.width ?? 0,
            height: node.measured.height ?? 0,
          },
      { width, height },
    );
    void flow.setViewport(view ?? fit);
  }, [initialized, level, flow, start, width, height, nodes, ids]);
  return null;
}

export function SceneGraph({
  summary,
  checks,
  level,
  onLevel,
  layoutVersion,
  onOpen,
  chapters,
  dirtyChapters,
  onStoryWritten,
  ai,
  onSelectScene,
}: Props) {
  /** スタブを足す欄を開いているか、書き足す辺（T2-9）。 */
  const [stubOpen, setStubOpen] = useState(false);
  const [edgeDraft, setEdgeDraft] = useState<{
    readonly from: string;
    readonly to: string;
  } | null>(null);
  const [graph, setGraph] = useState<SceneGraphData | null>(null);
  const [layout, setLayout] = useState<Layout>(EMPTY_LAYOUT);
  const [problem, setProblem] = useState<string | null>(null);
  /** まだ保存していない、動かした位置（粒度ごと）。 */
  const [moved, setMoved] = useState<
    Record<GraphLevel, Record<string, LayoutPoint>>
  >({
    scene: {},
    chapter: {},
    route: {},
  });
  const [selected, setSelected] = useState<string | null>(null);
  const [route, setRoute] = useState<string | null>(null);
  const [showConditions, setShowConditions] = useState(true);
  const [error, setError] = useState<string | null>(null);
  /** まだ書いていない位置と、その粒度。 */
  const pending = useRef<{
    level: GraphLevel;
    positions: Record<string, [number, number]>;
  }>({ level, positions: {} });
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // 構造グラフを取る（粒度が変わったとき、本文が変わったとき）。
  useEffect(() => {
    // 本文が変わったら（summary が新しくなったら）取り直す。
    void summary;
    const timer = setTimeout(() => {
      sceneGraph(level)
        .then((g) => {
          setGraph(g);
          setError(null);
        })
        .catch((e: unknown) => setError(String(e)));
    }, REFRESH_DELAY_MS);
    return () => clearTimeout(timer);
  }, [level, summary]);

  // 配置を読む（開いたとき、外で変わったとき）。まだ書いていない位置は残す（読み直しで戻さない）。
  useEffect(() => {
    void layoutVersion;
    readLayout()
      .then((loaded) => {
        setLayout(loaded.layout);
        setProblem(loaded.problem);
        const waiting = pending.current;
        const keep = (lv: GraphLevel, m: Record<string, LayoutPoint>) =>
          lv !== waiting.level
            ? {}
            : Object.fromEntries(
                Object.entries(m).filter(([key]) =>
                  Object.hasOwn(waiting.positions, key),
                ),
              );
        setMoved((m) => ({
          scene: keep("scene", m.scene),
          chapter: keep("chapter", m.chapter),
          route: keep("route", m.route),
        }));
      })
      .catch((e: unknown) => setError(String(e)));
  }, [layoutVersion]);

  // 粒度を変えたら、選択を外す（ルートの絞り込みは、シーンの粒度に戻ったときのために残す）。
  useEffect(() => {
    void level;
    setSelected(null);
  }, [level]);

  const routes = useMemo(() => {
    const list: string[] = [];
    for (const s of summary.scenes) {
      const r = s.meta.route ?? NO_ROUTE;
      if (!list.includes(r)) list.push(r);
    }
    return list;
  }, [summary.scenes]);

  const flow = useMemo(() => {
    if (graph === null || graph.level !== level) return null;
    const saved = { ...layout[level], ...moved[level] };
    return buildFlow(graph, summary, saved, {
      selected,
      route: level === "scene" ? route : null,
      showConditions,
    });
  }, [graph, level, layout, moved, summary, selected, route, showConditions]);

  // 提案ノード（シーンの粒度で、分岐案の元のシーンがあるとき）。
  const overlay = useMemo(() => {
    const result = ai.result;
    if (level !== "scene" || result?.kind !== "ok" || result.scene === null)
      return null;
    const from = flow?.nodes.find((n) => n.id === result.scene);
    if (from === undefined) return null;
    const items = result.branches
      .map((p, i) => ({ p, i, key: resultKey(result.run, i) }))
      .filter(({ key }) => !ai.hidden.has(key))
      .map(({ p, i, key }) => ({
        key,
        letter: proposalLetter(i),
        label: p.label === "" ? p.name : p.label,
        target: branchTargetText(p, summary.scenes),
        problem: p.problems.length > 0,
      }));
    return items.length === 0 ? null : proposalOverlay(from, from.id, items);
  }, [ai.result, ai.hidden, flow, level, summary.scenes]);

  const built: Node[] = useMemo(
    () => [
      ...(flow?.nodes ?? []).map((n) => ({
        id: n.id,
        type: n.type,
        position: { x: n.x, y: n.y },
        data: { ...n.data, key: n.key },
        draggable: n.key !== null,
        selectable: n.type !== "band",
        style:
          n.type === "band" ? { width: n.width, height: n.height } : undefined,
        zIndex: n.type === "band" ? -1 : undefined,
      })),
      ...(overlay?.nodes ?? []).map((n) => ({
        id: n.id,
        type: "proposal",
        position: { x: n.x, y: n.y },
        data: { ...n.data },
        draggable: false,
        connectable: false,
      })),
    ],
    [flow, overlay],
  );
  const [nodes, setNodes, onNodesChange] = useNodesState<Node>(built);
  useEffect(() => setNodes(built), [built, setNodes]);
  const builtIds = useMemo(() => new Set(built.map((n) => n.id)), [built]);

  const edges: Edge[] = useMemo(
    () => [
      ...(flow?.edges ?? []).map((e) => ({
        id: e.id,
        source: e.source,
        target: e.target,
        type: "labeled",
        data: { label: e.label, slot: e.labelSlot, out: e.out },
        className: `flow-edge${e.out ? " out" : ""}${e.call ? " call" : ""}`,
        markerEnd: {
          type: MarkerType.ArrowClosed,
          color: tokenColor(e.out ? "--accent" : "--stub-line"),
          width: 16,
          height: 16,
        },
      })),
      ...(overlay?.edges ?? []).map((e) => ({
        id: e.id,
        source: e.source,
        target: e.target,
        type: "smoothstep",
        className: "flow-edge proposal",
        markerEnd: {
          type: MarkerType.ArrowClosed,
          color: tokenColor("--ai-line"),
          width: 16,
          height: 16,
        },
      })),
    ],
    [flow, overlay],
  );

  /** まだ書いていない位置を、すぐに書く。 */
  const flush = useCallback(() => {
    if (saveTimer.current !== null) clearTimeout(saveTimer.current);
    saveTimer.current = null;
    const { level: target, positions } = pending.current;
    pending.current = { level: target, positions: {} };
    if (Object.keys(positions).length === 0) return;
    writeLayout(target, positions, false)
      .then(() => setError(null))
      .catch((e: unknown) => setError(`配置を保存できません：${String(e)}`));
  }, []);

  /** 動かした位置を、少し待ってまとめて保存する。 */
  const save = useCallback(
    (moves: readonly { key: string; x: number; y: number }[]) => {
      if (moves.length === 0) return;
      // 別の粒度の書いていない位置は、先に書く（粒度を混ぜない）。
      if (pending.current.level !== level) flush();
      pending.current.level = level;
      for (const { key, x, y } of moves) {
        pending.current.positions[key] = [x, y];
      }
      setMoved((m) => {
        const next = { ...m[level] };
        for (const { key, x, y } of moves) next[key] = { x, y };
        return { ...m, [level]: next };
      });
      if (saveTimer.current !== null) clearTimeout(saveTimer.current);
      saveTimer.current = setTimeout(flush, SAVE_DELAY_MS);
    },
    [level, flush],
  );

  const arrange = useCallback(() => {
    // この粒度の書いていない位置は捨てる（整列の後に戻ってこないように）。別の粒度のものは先に書く。
    if (pending.current.level !== level) flush();
    if (saveTimer.current !== null) clearTimeout(saveTimer.current);
    saveTimer.current = null;
    pending.current = { level, positions: {} };
    setMoved((m) => ({ ...m, [level]: {} }));
    setLayout((l) => ({ ...l, [level]: {} }));
    writeLayout(level, {}, true)
      .then(() => {
        setError(null);
        setProblem(null);
      })
      .catch((e: unknown) => setError(`配置を保存できません：${String(e)}`));
  }, [level, flush]);

  const counts = legendCounts(
    summary,
    checks,
    level === "scene" ? graph : null,
  );
  const scenesById = useMemo(
    () => new Map(summary.scenes.map((s) => [s.id, s])),
    [summary.scenes],
  );
  const selectedScene =
    level === "scene" && selected !== null
      ? (scenesById.get(selected) ?? null)
      : null;
  const selectedSceneId = selectedScene?.id ?? null;
  useEffect(
    () => onSelectScene(selectedSceneId),
    [onSelectScene, selectedSceneId],
  );
  useScreenStatus(
    "flow",
    flowStatus({
      level,
      scenes: summary.scenes.length,
      stubs: counts.stubs,
      aiDrafts: counts.aiDrafts,
      selected:
        selectedScene !== null
          ? `${selectedScene.id} ${selectedScene.title}`
          : selected,
    }),
  );

  return (
    <div className="flow-screen-body">
      <div className="flow-main">
        <div className="flow-toolbar">
          <fieldset className="mode-switch" aria-label="表示の粒度">
            {LEVELS.map((l) => (
              <button
                key={l}
                type="button"
                aria-pressed={level === l}
                onClick={() => onLevel(l)}
              >
                {LEVEL_LABELS[l]}
              </button>
            ))}
          </fieldset>
          {level === "scene" && (
            <label className="flow-filter">
              ルート
              <select
                value={route ?? ""}
                onChange={(event) =>
                  setRoute(
                    event.target.value === "" ? null : event.target.value,
                  )
                }
              >
                <option value="">すべて</option>
                {routes.map((r) => (
                  <option key={r} value={r}>
                    {r}
                  </option>
                ))}
              </select>
            </label>
          )}
          <label className="flow-filter">
            <input
              type="checkbox"
              checked={showConditions}
              onChange={(event) => setShowConditions(event.target.checked)}
            />
            条件を表示
          </label>
          <span className="flow-spacer" />
          {problem !== null && (
            <span className="flow-problem" role="status">
              {problem}
            </span>
          )}
          {error !== null && (
            <span className="flow-problem" role="alert">
              {error}
            </span>
          )}
          {level === "scene" && (
            <button
              type="button"
              className="btn btn-sm"
              onClick={() => setStubOpen(true)}
            >
              ＋ スタブ
            </button>
          )}
          <button
            type="button"
            className="btn btn-sm"
            onClick={() => {
              // 人が動かした位置があれば、捨ててよいかを聞く（T4-12、U4）。
              const manual =
                Object.keys(layout[level]).length > 0 ||
                Object.keys(moved[level]).length > 0;
              if (!manual) {
                arrange();
                return;
              }
              void confirm(
                "この粒度で動かした位置を捨てて、自動で並べ直しますか？",
                "自動整列",
              ).then((ok) => {
                if (ok) arrange();
              });
            }}
          >
            自動整列
          </button>
        </div>
        <div className="scene-graph">
          <ReactFlow
            nodes={nodes}
            edges={edges}
            nodeTypes={nodeTypes}
            edgeTypes={edgeTypes}
            onNodesChange={onNodesChange}
            // 初めの表示は FitOnLoad が決める（収めると読めないときは読める縮尺にするため。T4-13）。
            minZoom={0.2}
            // シーンの粒度では、ノードの右の点から別のノードへ引くと、辺を書き足す（T2-9）。
            nodesConnectable={level === "scene"}
            // 無いシーンの仮のノードと、IDの無いシーン（`_anon_…`）にはつながない（IDで書けないため）。
            isValidConnection={(c) =>
              [c.source, c.target].every(
                (id) =>
                  id !== null &&
                  !id.startsWith("missing:") &&
                  !id.startsWith("proposal:") &&
                  !id.startsWith("_anon_"),
              )
            }
            onConnect={(c) => {
              if (c.source !== null && c.target !== null)
                setEdgeDraft({ from: c.source, to: c.target });
            }}
            // ノードを消すキー、範囲選択、複数選択は使わない（本文を消さないため）。
            deleteKeyCode={null}
            selectionKeyCode={null}
            multiSelectionKeyCode={null}
            onNodeClick={(_, node) => {
              // 帯（キーが無い）の中を押したら、何も選ばない。
              setSelected((node.data as { key: string | null }).key);
            }}
            onNodeDoubleClick={(_, node) => {
              if (level !== "scene") return;
              const scene = scenesById.get(node.id);
              if (scene !== undefined) onOpen(scene);
            }}
            onNodeDragStop={(_, _node, dragged) => {
              const moves: { key: string; x: number; y: number }[] = [];
              for (const n of dragged) {
                const key = (n.data as { key: string | null }).key;
                if (key !== null) {
                  moves.push({ key, x: n.position.x, y: n.position.y });
                }
              }
              save(moves);
            }}
            onPaneClick={() => setSelected(null)}
          >
            <Background />
            <Controls showInteractive={false} />
            <FitOnLoad
              level={level}
              start={level === "scene" ? startSceneOf(summary) : null}
              ids={builtIds}
            />
          </ReactFlow>
        </div>
        <footer className="flow-legend">
          <strong>凡例</strong>
          <span className="legend-item">
            <span className="legend-line" />辺
          </span>
          <span className="legend-item">
            <span className="legend-line out" />
            選択中の出口
          </span>
          <span className="legend-item">
            <span className="legend-line call" />
            呼び出し
          </span>
          <span className="legend-item">
            <span className="legend-box ai" />
            AI下書き
          </span>
          <span className="legend-item">
            <span className="legend-box stub" />
            スタブ（本文なし）
          </span>
          {overlay !== null && (
            <span className="legend-item">
              <span className="legend-box proposal" />
              AI提案（未採用）
            </span>
          )}
          <span className="legend-counts">
            到達できない {counts.unreachable} · 行き止まり {counts.deadEnds} ·
            スタブ {counts.stubs} · AI下書き {counts.aiDrafts}
            {level === "scene" && ` · エンディング ${counts.endings}`}
          </span>
        </footer>
      </div>
      {stubOpen && (
        <StubDialog
          chapters={chapters}
          dirtyChapters={dirtyChapters}
          defaultChapter={
            selected === null ? null : (scenesById.get(selected)?.file ?? null)
          }
          sceneIds={summary.scenes.map((s) => s.id)}
          onSubmit={async (file, id, title, text) => {
            try {
              const result = await addStub(
                file,
                id,
                title,
                text === "" ? null : text,
              );
              const from =
                selected === null
                  ? null
                  : (nodes.find((n) => n.id === selected)?.position ?? null);
              const at = newStubPosition(
                nodes.filter((n) => n.type === "scene").map((n) => n.position),
                from,
              );
              save([{ key: id, x: at.x, y: at.y }]);
              onStoryWritten(result);
              setSelected(id);
              return null;
            } catch (e) {
              return String(e);
            }
          }}
          onClose={() => setStubOpen(false)}
        />
      )}
      {edgeDraft !== null && (
        <EdgeDialog
          from={edgeDraft.from}
          to={edgeDraft.to}
          onSubmit={async (kind, label) => {
            const { from, to } = edgeDraft;
            try {
              const result = await addFlowEdge(
                kind === "jump"
                  ? { kind, from, to }
                  : { kind, from, to, label },
              );
              onStoryWritten(result);
              return null;
            } catch (e) {
              return String(e);
            }
          }}
          onClose={() => setEdgeDraft(null)}
        />
      )}
      {graph !== null && graph.level === level && (
        <FlowPanel
          graph={graph}
          scenes={summary.scenes}
          selected={selected}
          onOpen={onOpen}
          ai={ai}
        />
      )}
    </div>
  );
}
