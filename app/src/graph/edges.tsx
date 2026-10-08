// フロー図の辺（T3-1、H3）：角の丸い折れ線（smoothstep）の縦の線を出元のそばに寄せ、条件と選択肢の札を行き先の
// ノードの手前（線の上か下）に置く。同じシーンから出る辺が縦の線を共有しても、札はそれぞれの行き先への横の線の
// 上に乗るので重ならない。札の置き場所の順は `graph/model.ts` の `withLabelSlots`・`labelPlacement`。

import {
  BaseEdge,
  EdgeLabelRenderer,
  type EdgeProps,
  getSmoothStepPath,
  Position,
} from "@xyflow/react";
import { labelPlacement } from "./model.ts";

export interface LabeledEdgeData extends Record<string, unknown> {
  readonly label: string | null;
  readonly slot: number;
  /** 選んだノードから出る辺（札の色を変える）。 */
  readonly out: boolean;
}

/** 出元のそばの縦の線の位置（出元の右の端から）。 */
const TRUNK = 20;
/** 札と矢印の間（矢印の大きさの分）。 */
const ARROW = 14;

export function LabeledEdge({
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourcePosition,
  targetPosition,
  markerEnd,
  style,
  data,
}: EdgeProps) {
  const forward =
    sourcePosition === Position.Right &&
    targetPosition === Position.Left &&
    targetX - sourceX > TRUNK * 3;
  const [path] = getSmoothStepPath({
    sourceX,
    sourceY,
    sourcePosition,
    targetX,
    targetY,
    targetPosition,
    borderRadius: 8,
    ...(forward ? { centerX: sourceX + TRUNK } : {}),
  });
  const d = data as LabeledEdgeData | undefined;
  const label = d?.label ?? null;
  const place = labelPlacement(d?.slot ?? 0);
  return (
    <>
      <BaseEdge path={path} markerEnd={markerEnd} style={style} />
      {label !== null && (
        <EdgeLabelRenderer>
          <div
            className={`flow-edge-label nodrag nopan${d?.out === true ? " out" : ""}`}
            title={label}
            style={{
              transform: `translate(${targetX - ARROW}px, ${targetY + place.shift}px) translate(-100%, ${place.above ? "-100%" : "0"})`,
            }}
          >
            {label}
          </div>
        </EdgeLabelRenderer>
      )}
    </>
  );
}
