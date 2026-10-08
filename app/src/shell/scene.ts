// カーソルのあるシーンと、文字数のまとめ（ステータスバーと文脈パネル。T1-8、D047）。

import { PositionMap } from "../core/position.ts";
import type { SceneSummary } from "../core/types.ts";

/**
 * エディタの位置（`offset`）があるシーン：そのファイルのシーンのうち、見出しが位置より前で、いちばん後ろの
 * もの。見出しより前なら null。`text` はそのファイルの本文。
 */
export function sceneAt(
  scenes: readonly SceneSummary[],
  file: string,
  text: string,
  offset: number,
): string | null {
  const map = new PositionMap(text);
  let found: { id: string; start: number } | null = null;
  for (const scene of scenes) {
    if (scene.file !== file) continue;
    const start = map.byteToUtf16(scene.span[0]);
    if (start <= offset && (found === null || start >= found.start)) {
      found = { id: scene.id, start };
    }
  }
  return found?.id ?? null;
}

/** ファイルの文字数（シーンの文字数の合計）。 */
export function fileChars(
  scenes: readonly SceneSummary[],
  file: string,
): number {
  return scenes
    .filter((s) => s.file === file)
    .reduce((sum, s) => sum + s.chars, 0);
}

/** 字数の書き方（3桁ごとにカンマ）。 */
export function formatChars(n: number): string {
  return `${n.toLocaleString("en-US")}字`;
}

/** 道の文字数と、読み終えるまでの目安（ステータスバー。T2-8）。`灯ルート 12,345字・約31分`。 */
export function routeStatText(stat: {
  readonly label: string;
  readonly chars: number;
  readonly minutes: number;
}): string {
  const time =
    stat.minutes < 60
      ? `約${stat.minutes}分`
      : `約${Math.floor(stat.minutes / 60)}時間${stat.minutes % 60}分`;
  return `${stat.label} ${formatChars(stat.chars)}・${time}`;
}

/** 今のシーンのルートの道（無ければ共通。どれも無ければ null）。 */
export function statOfRoute<
  T extends { readonly route: string | null; readonly label: string },
>(stats: readonly T[], route: string | null): T | null {
  return (
    (route === null ? undefined : stats.find((s) => s.route === route)) ??
    stats[0] ??
    null
  );
}
