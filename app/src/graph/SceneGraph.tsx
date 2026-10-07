// フローの画面：シーンをノード、ジャンプと呼び出しを辺にする（xyflow）。座標は自動配置で、保存しない（T1-9）。

import { Background, type Edge, type Node, ReactFlow } from "@xyflow/react";
import { useMemo } from "react";
import type { SceneSummary, Summary } from "../core/types.ts";
import { layoutScenes } from "./layout.ts";

const COLUMN = 220;
const ROW = 84;

interface Props {
  readonly ir: Summary;
  readonly currentFile: string | null;
  readonly onSelect: (scene: SceneSummary) => void;
}

export function SceneGraph({ ir, currentFile, onSelect }: Props) {
  const { nodes, edges } = useMemo(() => {
    const placed = layoutScenes(ir);
    const firstIndex = new Map<string, number>();
    ir.scenes.forEach((scene, index) => {
      if (!firstIndex.has(scene.id)) firstIndex.set(scene.id, index);
    });
    const nodes: Node[] = placed.map(({ index, rank, order }) => {
      const scene = ir.scenes[index];
      const classes = ["scene-node"];
      if (scene?.is_stub) classes.push("stub");
      if (scene?.meta.status === "ai-draft") classes.push("ai-draft");
      if (scene?.file === currentFile) classes.push("current");
      return {
        id: `scene-${index}`,
        position: { x: rank * COLUMN, y: order * ROW },
        data: { label: `${scene?.title ?? ""}\n#${scene?.id ?? ""}` },
        className: classes.join(" "),
      };
    });
    // 存在しないシーンへの辺（E501）は、仮のノードへつなぐ。
    const missing = new Map<string, string>();
    const edges: Edge[] = [];
    ir.edges.forEach((edge, i) => {
      const from = firstIndex.get(edge.from);
      if (from === undefined) return;
      let target: string;
      const to = firstIndex.get(edge.to);
      if (to === undefined) {
        target = missing.get(edge.to) ?? `missing-${edge.to}`;
        if (!missing.has(edge.to)) {
          missing.set(edge.to, target);
          nodes.push({
            id: target,
            position: {
              x: ((placed[from]?.rank ?? 0) + 1) * COLUMN,
              y: -ROW * missing.size,
            },
            data: { label: `${edge.to}（無い）` },
            className: "scene-node missing",
          });
        }
      } else {
        target = `scene-${to}`;
      }
      edges.push({
        id: `edge-${i}`,
        source: `scene-${from}`,
        target,
        label: edge.label ?? edge.cond_text ?? undefined,
        animated: edge.kind === "call",
        className: `edge-${edge.via}`,
      });
    });
    return { nodes, edges };
  }, [ir, currentFile]);

  return (
    <div className="scene-graph">
      <ReactFlow
        nodes={nodes}
        edges={edges}
        fitView
        nodesConnectable={false}
        nodesDraggable={false}
        onNodeClick={(_, node) => {
          const match = /^scene-(\d+)$/.exec(node.id);
          const scene =
            match?.[1] === undefined ? undefined : ir.scenes[Number(match[1])];
          if (scene !== undefined) onSelect(scene);
        }}
      >
        <Background />
      </ReactFlow>
    </div>
  );
}
