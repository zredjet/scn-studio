// シーンのグラフの自動配置。開始シーンから辺をたどった段（深さ）を横に、段の中の順を縦に並べる。
// 簡単な配置で、座標は保存しない（layout.json はT1-9）。

import type { Summary } from "../core/types.ts";

export interface NodePlacement {
  /** シーンの一覧（scenes）の添字。 */
  readonly index: number;
  readonly rank: number;
  readonly order: number;
}

/**
 * 各シーンの段と順を決める。開始シーン（無ければ最初のシーン）から幅優先でたどり、
 * たどり着けないシーンは、ファイルの順に、続く段からたどり直す。
 * シーンIDが重複しているときは、ジャンプ先と同じく先に現れたほうを使う。
 */
export function layoutScenes(
  ir: Pick<Summary, "scenes" | "edges" | "start_scene">,
): NodePlacement[] {
  const firstIndex = new Map<string, number>();
  ir.scenes.forEach((scene, index) => {
    if (!firstIndex.has(scene.id)) firstIndex.set(scene.id, index);
  });
  const next = new Map<number, number[]>();
  for (const edge of ir.edges) {
    const from = firstIndex.get(edge.from);
    const to = firstIndex.get(edge.to);
    if (from === undefined || to === undefined) continue;
    const list = next.get(from) ?? [];
    if (!list.includes(to)) list.push(to);
    next.set(from, list);
  }

  const rank = new Map<number, number>();
  let baseRank = 0;
  const walk = (start: number) => {
    const queue: number[] = [start];
    rank.set(start, baseRank);
    let deepest = baseRank;
    while (queue.length > 0) {
      const current = queue.shift() ?? start;
      const currentRank = rank.get(current) ?? baseRank;
      for (const target of next.get(current) ?? []) {
        if (rank.has(target)) continue;
        rank.set(target, currentRank + 1);
        deepest = Math.max(deepest, currentRank + 1);
        queue.push(target);
      }
    }
    baseRank = deepest + 1;
  };

  const start =
    ir.start_scene === null ? undefined : firstIndex.get(ir.start_scene);
  if (ir.scenes.length > 0) walk(start ?? 0);
  ir.scenes.forEach((_, index) => {
    if (!rank.has(index)) walk(index);
  });

  const counts = new Map<number, number>();
  return ir.scenes.map((_, index) => {
    const r = rank.get(index) ?? 0;
    const order = counts.get(r) ?? 0;
    counts.set(r, order + 1);
    return { index, rank: r, order };
  });
}
