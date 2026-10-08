// 企画の画面（T2-6、D057。docs/ui/mockups/01-ideas.html）の判断：アイデアメモの並べ方と絞り込み、タグの
// 読み方、昇格先の見せ方、昇格の欄の初めの値。React と DOM に依存させない（テストする）。

import type { Idea, IdeaTarget } from "../core/types.ts";

/** メモの状態（この順に並べる）。 */
export const STATUSES = ["未整理", "採用", "保留", "没"] as const;

/** 初めは畳んでおく状態（没は消さずに残すが、ふだんは見ない）。 */
export const FOLDED_STATUS = "没";

/** 状態ごとのメモ（`tag` があれば、そのタグを持つメモだけ）。 */
export function groupIdeas(
  ideas: readonly Idea[],
  tag: string | null,
): { readonly status: string; readonly ideas: readonly Idea[] }[] {
  const shown =
    tag === null ? ideas : ideas.filter((idea) => idea.tags.includes(tag));
  return STATUSES.map((status) => ({
    status,
    ideas: shown.filter((idea) => idea.status === status),
  }));
}

/** すべてのタグ（多く使われている順、同じなら先に出た順）。 */
export function allTags(ideas: readonly Idea[]): string[] {
  const counts = new Map<string, number>();
  for (const idea of ideas) {
    for (const tag of idea.tags) counts.set(tag, (counts.get(tag) ?? 0) + 1);
  }
  const order = [...counts.keys()];
  return order.sort(
    (a, b) =>
      (counts.get(b) ?? 0) - (counts.get(a) ?? 0) ||
      order.indexOf(a) - order.indexOf(b),
  );
}

/** 状態ごとの枚数。 */
export function statusCounts(ideas: readonly Idea[]): Record<string, number> {
  const counts: Record<string, number> = {};
  for (const status of STATUSES) counts[status] = 0;
  for (const idea of ideas)
    counts[idea.status] = (counts[idea.status] ?? 0) + 1;
  return counts;
}

/** 昇格先の見せ方（`→ シーン：s012 放課後の屋上` の `→` の後ろ）。 */
export function targetLabel(target: IdeaTarget): string {
  switch (target.kind) {
    case "scene":
      return `シーン：${target.id}${target.title === "" ? "" : ` ${target.title}`}`;
    case "entry":
      return `${target.type_name}：${target.name}`;
    case "missing":
      return `見つからない：${target.value}`;
  }
}

/** 本文の最初の空でない行（カードの見出し、昇格の欄の初めの値）。 */
export function firstLine(body: string): string {
  for (const line of body.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (trimmed !== "") return trimmed;
  }
  return "";
}

/** スタブのシーンの題名の初めの値：本文の最初の文（句点・感嘆符・疑問符の手前）を30字まで。 */
export function defaultTitle(body: string): string {
  const sentence = firstLine(body).split(/[。．！？!?]/u)[0] ?? "";
  return [...sentence.trim()].slice(0, 30).join("");
}

/**
 * 新しいシーンIDの案：今あるIDのうち「英字＋数字」の形でいちばん多い英字を使い、その数字の最大に1を足す
 * （桁は、その中でいちばん長いものにそろえる）。無ければ `s001`。
 */
export function suggestSceneId(ids: readonly string[]): string {
  const groups = new Map<string, { max: number; width: number }>();
  for (const id of ids) {
    const match = /^([A-Za-z_]+)(\d+)$/.exec(id);
    if (match === null) continue;
    const prefix = match[1] ?? "";
    const digits = match[2] ?? "";
    const g = groups.get(prefix) ?? { max: 0, width: 0 };
    groups.set(prefix, {
      max: Math.max(g.max, Number(digits)),
      width: Math.max(g.width, digits.length),
    });
  }
  const counts = new Map<string, number>();
  for (const id of ids) {
    const prefix = /^([A-Za-z_]+)\d+$/.exec(id)?.[1];
    if (prefix !== undefined) counts.set(prefix, (counts.get(prefix) ?? 0) + 1);
  }
  let best: string | null = null;
  for (const [prefix, n] of counts) {
    if (best === null || n > (counts.get(best) ?? 0)) best = prefix;
  }
  if (best === null) return "s001";
  const g = groups.get(best) ?? { max: 0, width: 3 };
  let n = g.max + 1;
  const taken = new Set(ids);
  let id = `${best}${String(n).padStart(g.width, "0")}`;
  while (taken.has(id)) {
    n += 1;
    id = `${best}${String(n).padStart(g.width, "0")}`;
  }
  return id;
}

/** シーンID・エントリのIDの形（英数字と `_`）。 */
export function isValidId(id: string): boolean {
  return /^[A-Za-z0-9_]+$/.test(id);
}
