// ステータスバーの中身（docs/ui/decisions-v1.md の U05・U08、T4-17）。左は画面ごとの情報、右は全画面で共通
// （保存の様子・問題の数・形式）。左の中身は、各画面が今持っているデータから作る（今は無いデータの欄は作らない）。

import type { Severity } from "../core/types.ts";
import type { Screen } from "./LeftBar.tsx";

/** 左の1項目。`strong` は画面の名前（太字）、`hideNarrow` は狭い窓で隠すもの。 */
export interface StatusItem {
  readonly text: string;
  readonly title?: string;
  readonly strong?: boolean;
  readonly hideNarrow?: boolean;
}

export type StatusItems = readonly StatusItem[];

/** 画面の名前（左の最初の項目）。本文は表示モードを出すので、ここには無い。 */
export const SCREEN_LABEL: Readonly<Record<Exclude<Screen, "text">, string>> = {
  concept: "企画",
  flow: "フロー",
  bible: "設定",
  assets: "素材",
  variables: "変数",
  check: "検証",
  ai: "AI実行",
  export: "出力",
  preview: "プレビュー",
  preferences: "環境設定",
};

function head(screen: Exclude<Screen, "text">): StatusItem {
  return { text: SCREEN_LABEL[screen], strong: true };
}

/** 保存の様子（U08 の4つの言葉。自動保存はしない：D085・D086）。 */
export type SaveState =
  | { readonly kind: "saved" }
  | { readonly kind: "saving" }
  /** 保存していない本文の章と、ソース表示の書きかけの数。 */
  | { readonly kind: "unsaved"; readonly files: number }
  | { readonly kind: "failed"; readonly message: string };

/** 保存の様子を決める：できなかった・保存中・保存していない・保存済みの順に強い。 */
export function saveState(
  saving: number,
  failed: string | null,
  unsaved: number,
): SaveState {
  if (failed !== null) return { kind: "failed", message: failed };
  if (saving > 0) return { kind: "saving" };
  if (unsaved > 0) return { kind: "unsaved", files: unsaved };
  return { kind: "saved" };
}

export function saveLabel(state: SaveState): string {
  switch (state.kind) {
    case "saved":
      return "保存済み";
    case "saving":
      return "保存中…";
    case "unsaved":
      return "保存していない変更";
    case "failed":
      return "保存できません";
  }
}

/** 保存の様子のツールチップ。 */
export function saveTitle(state: SaveState): string {
  switch (state.kind) {
    case "saved":
      return "本文とソース表示の変更は、すべて保存しています";
    case "saving":
      return "本文を保存しています";
    case "unsaved":
      return `保存していない変更が ${state.files} か所あります（本文は ⌘S、ソース表示は「保存」）`;
    case "failed":
      return `${state.message}（押すと詳しく出します。⌘S でもう一度保存します）`;
  }
}

/** 問題の数のツールチップと読み上げ。 */
export function problemsTitle(counts: Record<Severity, number>): string {
  return `エラー ${counts.error}・警告 ${counts.warning}・情報 ${counts.info}（押すと検証を開く）`;
}

/** 本文：表示モード・シーンと字数・行と列・章の字数・道の字数。 */
export function textStatus(input: {
  readonly mode: "writing" | "source" | null;
  readonly scene: {
    readonly id: string;
    readonly title: string;
    readonly chars: string;
  } | null;
  readonly file: string | null;
  readonly fileChars: string | null;
  readonly route: string | null;
}): StatusItems {
  const items: StatusItem[] = [
    {
      text:
        input.mode === null
          ? "本文"
          : input.mode === "writing"
            ? "執筆表示"
            : "ソース表示",
      strong: true,
    },
  ];
  if (input.scene !== null) {
    const name =
      input.scene.title === ""
        ? input.scene.id
        : `${input.scene.id} ${input.scene.title}`;
    items.push({ text: `${name} · ${input.scene.chars}` });
  }
  if (input.file === null) items.push({ text: "ファイル未選択" });
  else
    items.push({
      text:
        input.fileChars === null
          ? input.file
          : `${input.file} · ${input.fileChars}`,
      hideNarrow: true,
    });
  if (input.route !== null)
    items.push({
      text: input.route,
      title: "道に沿って読む台詞と地の文の字数と、1分に400字で読んだときの目安",
      hideNarrow: true,
    });
  return items;
}

/** 行と列（1から数える。列は文字で数え、サロゲートペアを1字にする）。 */
export function lineColumn(
  text: string,
  offset: number,
): { readonly line: number; readonly column: number } {
  const before = text.slice(0, Math.max(0, Math.min(offset, text.length)));
  const start = before.lastIndexOf("\n") + 1;
  let line = 1;
  for (let i = 0; i < before.length; i += 1) if (before[i] === "\n") line += 1;
  return { line, column: [...before.slice(start)].length + 1 };
}

/** 企画：メモの枚数（状態ごと。`order` の順で、0枚の状態は出さない）・エンディングの数（本文に無いもの）。 */
export function conceptStatus(input: {
  readonly ideas: readonly { readonly status: string }[] | null;
  readonly order: readonly string[];
  readonly endings: number | null;
  readonly missingEndings: number;
}): StatusItems {
  const items: StatusItem[] = [head("concept")];
  if (input.ideas !== null) {
    const ideas = input.ideas;
    const parts = input.order
      .map(
        (status) =>
          [status, ideas.filter((i) => i.status === status).length] as const,
      )
      .filter(([, n]) => n > 0)
      .map(([status, n]) => `${status} ${n}`);
    items.push({
      text:
        parts.length === 0
          ? `メモ ${ideas.length}`
          : `メモ ${ideas.length}（${parts.join("・")}）`,
    });
  }
  if (input.endings !== null)
    items.push({
      text: `エンディング ${input.endings}（本文に無い ${input.missingEndings}）`,
    });
  return items;
}

const LEVEL_LABEL = {
  route: "ルート表示",
  chapter: "章表示",
  scene: "シーン表示",
} as const;

/** フロー：粒度・シーンの数（スタブ・AI下書き）・選んでいるもの。 */
export function flowStatus(input: {
  readonly level: keyof typeof LEVEL_LABEL;
  readonly scenes: number;
  readonly stubs: number;
  readonly aiDrafts: number;
  readonly selected: string | null;
}): StatusItems {
  return [
    head("flow"),
    { text: LEVEL_LABEL[input.level] },
    {
      text: `${input.scenes}シーン（スタブ ${input.stubs}・AI下書き ${input.aiDrafts}）`,
    },
    { text: input.selected ?? "選んでいない", hideNarrow: true },
  ];
}

/** 設定：今の型と数・型とエントリの数・参照切れ。 */
export function bibleStatus(input: {
  readonly type: { readonly name: string; readonly entries: number } | null;
  readonly types: number;
  readonly entries: number;
  readonly broken: number;
}): StatusItems {
  const items: StatusItem[] = [head("bible")];
  if (input.type !== null)
    items.push({ text: `${input.type.name} ${input.type.entries}` });
  items.push({ text: `${input.types}型・${input.entries}エントリ` });
  items.push({ text: `参照切れ ${input.broken}` });
  return items;
}

/** 素材：ファイルあり・仮素材・未登録。 */
export function assetsStatus(input: {
  readonly files: number;
  readonly placeholders: number;
  readonly unregistered: number;
}): StatusItems {
  return [
    head("assets"),
    {
      text: `ファイルあり ${input.files}・仮素材 ${input.placeholders}・未登録 ${input.unregistered}`,
    },
  ];
}

/** 変数：変数の数・書いて読まない数・使っていない数。 */
export function variablesStatus(input: {
  readonly count: number;
  readonly writtenNotRead: number;
  readonly unused: number;
}): StatusItems {
  return [
    head("variables"),
    { text: `${input.count}` },
    { text: `書いて読まない ${input.writtenNotRead}` },
    { text: `使っていない ${input.unused}` },
  ];
}

/** 検証：段階2の様子・件数。 */
export function checkStatus(input: {
  readonly stage2: "none" | "running" | "done" | "stale";
  readonly counts: Record<Severity, number>;
}): StatusItems {
  const stage2 = {
    none: "段階2 まだ",
    running: "段階2 調べています",
    done: "段階2 済み",
    stale: "段階2 本文が変わった後",
  }[input.stage2];
  const { error, warning, info } = input.counts;
  const total = error + warning + info;
  const parts: string[] = [];
  if (error > 0) parts.push(`エラー ${error}`);
  if (warning > 0) parts.push(`警告 ${warning}`);
  if (info > 0) parts.push(`情報 ${info}`);
  return [
    head("check"),
    { text: stage2 },
    { text: parts.length === 0 ? "0件" : `${total}件（${parts.join("・")}）` },
  ];
}

/** AI実行：実行中の数・取り込み待ち・履歴の数。 */
export function aiRunStatus(input: {
  readonly running: number;
  readonly pending: number;
  readonly history: number;
}): StatusItems {
  return [
    head("ai"),
    { text: `実行中 ${input.running}` },
    { text: `取り込み待ち ${input.pending}` },
    { text: `履歴 ${input.history}` },
  ];
}

/** 出力：最後の書き出し（この窓で書き出したもの）。 */
export function exportStatus(last: string | null): StatusItems {
  return [
    head("export"),
    {
      text: last === null ? "まだ書き出していません" : `最後の書き出し ${last}`,
    },
  ];
}

/** プレビュー：位置（シーン・行ID・何行目）・画面とウィンドウの大きさ。 */
export function previewStatus(input: {
  readonly position: string | null;
  readonly screen: string | null;
}): StatusItems {
  const items: StatusItem[] = [head("preview")];
  if (input.position !== null) items.push({ text: input.position });
  if (input.screen !== null)
    items.push({ text: input.screen, hideNarrow: true });
  return items;
}

/** 環境設定：設定の保存先（D086 の「決めた」の3）。 */
export function preferencesStatus(): StatusItems {
  return [
    head("preferences"),
    {
      text: "AI の接続はこの端末の ai.toml と作品の project.toml、表示の好みはこの端末",
      hideNarrow: true,
    },
  ];
}
