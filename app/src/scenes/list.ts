// 本文の画面のシーン一覧の判断（docs/ui/decisions-v1.md の U14、T4-18）：状態の印、絞り込み、章の名前、
// アウトラインの行番号、シーンの範囲。React と DOM に依存させない（テストする）。

import type {
  ChapterTitle,
  IrDiagnostic,
  OutlineItem,
  SceneSummary,
} from "../core/types.ts";

/** シーンの状態の印（形で分ける。U11）：執筆中 ●藍・下書き ○・完成 ●灰・スタブ 点線の○・AI下書き「AI」。 */
export type SceneMark = "writing" | "draft" | "done" | "stub" | "ai";

export const MARK_LABEL: Readonly<Record<SceneMark, string>> = {
  writing: "執筆中",
  draft: "下書き",
  done: "完成",
  stub: "スタブ",
  ai: "AI下書き",
};

/** 状態の印（`status` と、本文が無いか）。本文の無いシーンはスタブ、`status` の無いシーンは下書き。 */
export function markOf(status: string | null, isStub: boolean): SceneMark {
  if (status === "ai-draft") return "ai";
  if (isStub) return "stub";
  if (status === "writing") return "writing";
  if (status === "done") return "done";
  return "draft";
}

/** シーンの状態の印。 */
export function sceneMark(scene: SceneSummary): SceneMark {
  return markOf(scene.meta.status, scene.is_stub);
}

/** 絞り込み：ID か名前に、空白で区切った語がすべて含まれる（大文字と小文字は区別しない）。 */
export function filterScenes(
  scenes: readonly SceneSummary[],
  query: string,
): SceneSummary[] {
  const words = query
    .toLowerCase()
    .split(/[\s　]+/)
    .filter((w) => w !== "");
  if (words.length === 0) return [...scenes];
  return scenes.filter((s) => {
    const hay = `${s.id} ${s.title}`.toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}

/** 章の名前：見出し（`# 第1章 放課後`）。無ければファイル名（`story/` と `.scn` を除く）。 */
export function chapterName(
  file: string,
  chapters: readonly ChapterTitle[],
): string {
  const title = chapters.find((c) => c.file === file)?.title ?? null;
  return title ?? file.replace(/^story\//, "").replace(/\.scn$/, "");
}

export const OUTLINE_LABEL: Readonly<Record<OutlineItem["kind"], string>> = {
  direction: "演出",
  choice: "選択",
  branch: "分岐",
  exit: "出口",
};

export interface OutlineRow extends OutlineItem {
  readonly label: string;
  /** 1から数える行の番号。 */
  readonly line: number;
}

/** アウトラインに行の番号を付ける（`start` は UTF-8 のバイト位置）。 */
export function outlineRows(
  outline: readonly OutlineItem[],
  text: string,
): OutlineRow[] {
  const bytes = new TextEncoder().encode(text);
  return outline.map((item) => {
    let line = 1;
    const end = Math.min(item.start, bytes.length);
    for (let i = 0; i < end; i += 1) if (bytes[i] === 0x0a) line += 1;
    return { ...item, label: OUTLINE_LABEL[item.kind], line };
  });
}

/** シーンの範囲（見出しの行の頭から、同じ章の次のシーンの見出しまで。バイト位置）。無ければ null。 */
export function sceneRange(
  scenes: readonly SceneSummary[],
  id: string,
): {
  readonly file: string;
  readonly start: number;
  readonly end: number;
} | null {
  const scene = scenes.find((s) => s.id === id);
  if (scene === undefined) return null;
  const next = scenes
    .filter((s) => s.file === scene.file && s.span[0] > scene.span[0])
    .reduce<number>(
      (min, s) => Math.min(min, s.span[0]),
      Number.POSITIVE_INFINITY,
    );
  return { file: scene.file, start: scene.span[0], end: next };
}

/** シーンの中の診断（範囲の外のものは除く）。 */
export function diagnosticsInScene(
  diagnostics: readonly IrDiagnostic[],
  range: {
    readonly file: string;
    readonly start: number;
    readonly end: number;
  },
): IrDiagnostic[] {
  return diagnostics.filter(
    (d) =>
      d.file === range.file &&
      d.span[0] >= range.start &&
      d.span[0] < range.end,
  );
}

/** 右の欄の問題：シーンにいればそのシーンの中の診断、いなければ今のファイルの診断。 */
export function sceneDiagnostics(
  diagnostics: readonly IrDiagnostic[],
  scenes: readonly SceneSummary[],
  file: string | null,
  scene: string | null,
): IrDiagnostic[] {
  const range = scene === null ? null : sceneRange(scenes, scene);
  if (range !== null && range.file === file)
    return diagnosticsInScene(diagnostics, range);
  return diagnostics.filter((d) => d.file === file);
}
