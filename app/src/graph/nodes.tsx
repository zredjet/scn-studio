// フロー図のノード（xyflow の自前のノード。docs/ui/mockups/03-flow.html）。提案ノードは T3-10。
// シーン：3行（ID、題名、字数と状態）。スタブは灰色の点線、AI下書きは緑青の実線（色だけで区別しない）。
// まとまり（章・ルート）：3行（ファイル名か「ルート」、題名、シーンの数と字数）。帯：ルートの見出しと枠。

import { Handle, type NodeProps, Position } from "@xyflow/react";
import type { FlowNodeData, ProposalNodeData } from "./model.ts";

function classes(data: FlowNodeData, base: string): string {
  const list = [base];
  if (data.stub) list.push("stub");
  if (data.aiDraft) list.push("ai-draft");
  if (data.selected) list.push("selected");
  return list.join(" ");
}

function Marks({ data }: { readonly data: FlowNodeData }) {
  if (!data.start && !data.ending) return null;
  return (
    <span className="flow-marks">
      {data.start && <span className="flow-mark start">開始</span>}
      {data.ending && <span className="flow-mark ending">エンディング</span>}
    </span>
  );
}

export function SceneNode({ data }: NodeProps) {
  const d = data as unknown as FlowNodeData;
  return (
    <div
      className={classes(d, "flow-node")}
      aria-current={d.selected ? "true" : undefined}
    >
      <Handle type="target" position={Position.Left} />
      <span className="flow-node-head">
        {d.head}
        <Marks data={d} />
      </span>
      <b className="flow-node-title">{d.title}</b>
      <span className="flow-node-foot">{d.foot}</span>
      <Handle type="source" position={Position.Right} />
    </div>
  );
}

export function GroupNode({ data }: NodeProps) {
  const d = data as unknown as FlowNodeData;
  return (
    <div className={classes(d, "flow-node flow-group")}>
      <Handle type="target" position={Position.Left} />
      <span className="flow-node-head">
        {d.head}
        <Marks data={d} />
      </span>
      <b className="flow-node-title">{d.title}</b>
      <span className="flow-node-foot">{d.foot}</span>
      <Handle type="source" position={Position.Right} />
    </div>
  );
}

export function BandNode({ data }: NodeProps) {
  const d = data as unknown as FlowNodeData;
  return (
    <div className="flow-band">
      <span className="flow-band-title">{d.title}</span>
    </div>
  );
}

export function MissingNode({ data }: NodeProps) {
  const d = data as unknown as FlowNodeData;
  return (
    <div className="flow-node missing">
      <Handle type="target" position={Position.Left} />
      <span className="flow-node-head">{d.head}</span>
      <b className="flow-node-title">{d.title}</b>
    </div>
  );
}

/** 提案ノード（T3-10）：AI の分岐案。緑青の点線（色だけで区別しない）。 */
export function ProposalNode({ data }: NodeProps) {
  const d = data as unknown as ProposalNodeData;
  return (
    <div
      className={
        d.problem ? "flow-node proposal problem" : "flow-node proposal"
      }
    >
      <Handle type="target" position={Position.Left} />
      <span className="flow-node-head">{d.head}</span>
      <b className="flow-node-title">{d.title}</b>
      <span className="flow-node-foot">{d.foot}</span>
    </div>
  );
}

export const nodeTypes = {
  scene: SceneNode,
  cluster: GroupNode,
  band: BandNode,
  missing: MissingNode,
  proposal: ProposalNode,
};
