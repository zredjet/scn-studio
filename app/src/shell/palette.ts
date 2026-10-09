// コマンドパレット（Mod-Shift-P・⌘K。T2-8、D059）の判断：コマンドの絞り込みと、シーン・設定DBのエントリ・素材を
// 種類ごとに探すこと（T4-20、見本 12-overlays.html の 2）。React と DOM に依存させない（テストする）。

import { displayName, KIND_LABEL, rowId } from "../assets/model.ts";
import { entryName } from "../bible/fields.ts";
import type { AssetRow, BibleData, SceneSummary } from "../core/types.ts";
import { MARK_LABEL, type SceneMark, sceneMark } from "../scenes/list.ts";

/** パレットのコマンド。 */
export interface Command {
  readonly id: string;
  /** 見せる名前。 */
  readonly label: string;
  /** 絞り込みに使う、ほかの言い方（英語の名前など）。 */
  readonly keywords?: string;
  /** キー（見せるだけ）。 */
  readonly keys?: string;
  readonly run: () => void;
}

/** 絞り込み：名前かほかの言い方に、空白で区切った語がすべて含まれる（大文字と小文字は区別しない）。 */
export function filterCommands(
  commands: readonly Command[],
  query: string,
): Command[] {
  const words = query
    .toLowerCase()
    .split(/[\s　]+/)
    .filter((w) => w !== "");
  return commands.filter((c) => {
    const hay = `${c.label} ${c.keywords ?? ""}`.toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}

/** この端末が macOS か（キーの書き方を ⌘ にする）。 */
export const IS_MAC =
  typeof navigator !== "undefined" && /Mac/.test(navigator.userAgent);

/**
 * キーの書き方を OS に合わせる（T4-14、U11）。CodeMirror の書き方（`Mod-Shift-P`）を、macOS では `⌘⇧P`、ほかでは
 * `Ctrl+Shift+P` にする。`F5` などの修飾の無いキーはそのまま。
 */
export function keyLabel(keys: string, mac: boolean): string {
  const parts = keys.split("-");
  const key = parts.pop() ?? "";
  const name = key.length === 1 ? key.toUpperCase() : key;
  const mods = parts.map((m) => {
    if (m === "Mod") return mac ? "⌘" : "Ctrl";
    if (m === "Shift") return mac ? "⇧" : "Shift";
    if (m === "Alt") return mac ? "⌥" : "Alt";
    if (m === "Ctrl") return mac ? "⌃" : "Ctrl";
    return m;
  });
  return mac ? `${mods.join("")}${name}` : [...mods, name].join("+");
}

/** 画面を切り替えるキー（`Mod-` に続けて押す。左端のバーの順に 1〜9、環境設定は `,`。T4-14、U10）。 */
export const SCREEN_KEYS = [
  ["1", "concept"],
  ["2", "text"],
  ["3", "flow"],
  ["4", "bible"],
  ["5", "assets"],
  ["6", "variables"],
  ["7", "check"],
  ["8", "ai"],
  ["9", "export"],
  [",", "preferences"],
] as const;

/** 画面の名前（`SCREEN_KEYS` の2つ目）。 */
export type ScreenKeyTarget = (typeof SCREEN_KEYS)[number][1];

/** `Mod-` と一緒に押したキーで移る画面。割り当てが無ければ null。 */
export function screenForKey(key: string): ScreenKeyTarget | null {
  return SCREEN_KEYS.find(([k]) => k === key)?.[1] ?? null;
}

/** 画面に移るキー（`Mod-1` など）。割り当てが無ければ null。 */
export function keyForScreen(screen: string): string | null {
  const found = SCREEN_KEYS.find(([, s]) => s === screen);
  return found === undefined ? null : `Mod-${found[0]}`;
}

// ---- 何でも探す（⌘K。T4-20） ----

/** 探す窓の項目の種類（並べる順）。 */
export type FindKind = "scene" | "entry" | "asset" | "command";

export const FIND_ORDER: readonly FindKind[] = [
  "scene",
  "entry",
  "asset",
  "command",
];

export const FIND_LABEL: Readonly<Record<FindKind, string>> = {
  scene: "シーン",
  entry: "設定",
  asset: "素材",
  command: "コマンド",
};

/** 1つの種類で見せる数（コマンドだけを探すときと、何も打っていないときのコマンドは全部）。 */
export const FIND_LIMIT = 5;

/** 名前の後ろの札（AI下書き・仮素材・未登録）。 */
export interface FindTag {
  readonly text: string;
  readonly tone: "ai" | "ph" | "warn";
}

/** 探す窓の1行。開き方（`target`）は App が決める。 */
export interface FindItem {
  readonly kind: FindKind;
  readonly key: string;
  readonly label: string;
  /** 左の小さな文（シーンID・型の名前・素材の種類）。 */
  readonly lead: string;
  /** 右に寄せる控えめな文（ルートと章・ファイル・使用回数・キー）。 */
  readonly trail: string;
  readonly tag: FindTag | null;
  /** シーンの状態の印（シーンだけ）。 */
  readonly mark: SceneMark | null;
  /** 絞り込みに使う文（小文字）。 */
  readonly hay: string;
  readonly target: FindTarget;
}

export type FindTarget =
  | { readonly kind: "scene"; readonly scene: SceneSummary }
  | { readonly kind: "entry"; readonly path: string }
  | { readonly kind: "asset"; readonly id: string }
  | { readonly kind: "command"; readonly command: Command };

export interface FindGroup {
  readonly kind: FindKind;
  readonly label: string;
  readonly items: readonly FindItem[];
  /** 見せずに省いた数。 */
  readonly more: number;
}

function wordsOf(query: string): string[] {
  return query
    .toLowerCase()
    .split(/[\s　]+/)
    .filter((w) => w !== "");
}

function chapterFile(file: string): string {
  return file.replace(/^story\//, "");
}

export function sceneItems(scenes: readonly SceneSummary[]): FindItem[] {
  return scenes.map((s) => {
    const mark = sceneMark(s);
    return {
      kind: "scene",
      key: `scene:${s.id}`,
      label: s.title,
      lead: s.id,
      trail: [s.meta.route, chapterFile(s.file)]
        .filter((x) => x !== null && x !== "")
        .join(" · "),
      tag: mark === "ai" ? { text: MARK_LABEL.ai, tone: "ai" } : null,
      mark,
      hay: `${s.id} ${s.title} ${s.meta.route ?? ""}`.toLowerCase(),
      target: { kind: "scene", scene: s },
    };
  });
}

export function entryItems(bible: BibleData): FindItem[] {
  const typeName = new Map(bible.types.map((t) => [t.id, t.name]));
  return bible.files.flatMap((f) => {
    if (f.entry === null) return [];
    const name = entryName(f.entry);
    const type = typeName.get(f.entry.type_id) ?? f.entry.type_id;
    return [
      {
        kind: "entry",
        key: `entry:${f.path}`,
        label: name,
        lead: type,
        trail: f.path.replace(/\/[^/]*$/, ""),
        tag: null,
        mark: null,
        hay: `${name} ${f.entry.id} ${type}`.toLowerCase(),
        target: { kind: "entry", path: f.path },
      } satisfies FindItem,
    ];
  });
}

export function assetItems(rows: readonly AssetRow[]): FindItem[] {
  return rows.map((r) => {
    const name = displayName(r);
    return {
      kind: "asset",
      key: `asset:${rowId(r)}`,
      label: name,
      lead: KIND_LABEL[r.kind],
      trail: `使用 ${r.uses.length}`,
      tag:
        r.state === "placeholder"
          ? { text: "仮素材", tone: "ph" }
          : r.state === "unregistered"
            ? { text: "未登録", tone: "warn" }
            : null,
      mark: null,
      hay: `${name} ${r.name} ${KIND_LABEL[r.kind]}`.toLowerCase(),
      target: { kind: "asset", id: rowId(r) },
    };
  });
}

export function commandItems(
  commands: readonly Command[],
  mac: boolean,
): FindItem[] {
  return commands.map((c) => ({
    kind: "command",
    key: `command:${c.id}`,
    label: c.label,
    lead: "",
    trail: c.keys === undefined ? "" : keyLabel(c.keys, mac),
    tag: null,
    mark: null,
    hay: `${c.label} ${c.keywords ?? ""}`.toLowerCase(),
    target: { kind: "command", command: c },
  }));
}

/**
 * 種類ごとに探す（見本 12 の 2）。空白で区切った語がすべて含まれるものを、シーン・設定・素材・コマンドの順に、
 * 種類ごとに `FIND_LIMIT` 件まで。`>` で始めるとコマンドだけ（全部）。何も打っていなければコマンドだけ（全部）。
 */
export function findGroups(
  items: readonly FindItem[],
  query: string,
): FindGroup[] {
  const trimmed = query.trimStart();
  const commandsOnly = trimmed.startsWith(">");
  const words = wordsOf(commandsOnly ? trimmed.slice(1) : query);
  const everything = !commandsOnly && words.length > 0;
  return FIND_ORDER.flatMap((kind) => {
    if (kind !== "command" && !everything) return [];
    const hits = items.filter(
      (i) => i.kind === kind && words.every((w) => i.hay.includes(w)),
    );
    if (hits.length === 0) return [];
    const shown = everything ? hits.slice(0, FIND_LIMIT) : hits;
    return [
      {
        kind,
        label: FIND_LABEL[kind],
        items: shown,
        more: hits.length - shown.length,
      },
    ];
  });
}
