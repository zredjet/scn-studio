// 出力の画面のカード（docs/ui/decisions-v1.md の U18、見本 10-export.html。T4-19）：「制作に渡す」と「ほかの形」の
// 2群、収録の台本を先頭。カードの説明は1行で、形式は題の横に小さく。選んだカードの設定は右の欄に出す。

export type ExportKind =
  | "script"
  | "renpy"
  | "orders"
  | "text"
  | "book"
  | "tyrano"
  | "utage"
  | "plugin"
  | "provenance";

export type ExportGroup = "handoff" | "other";

export interface ExportTile {
  readonly id: ExportKind;
  readonly group: ExportGroup;
  readonly title: string;
  /** 形式（題の横に小さく）。 */
  readonly format: string;
  /** 1行の説明。 */
  readonly desc: string;
}

export const GROUP_LABEL: Readonly<Record<ExportGroup, string>> = {
  handoff: "制作に渡す",
  other: "ほかの形",
};

export const EXPORT_TILES: readonly ExportTile[] = [
  {
    id: "script",
    group: "handoff",
    title: "収録の台本",
    format: "CSV・Excel",
    desc: "台詞とボイス名の表。",
  },
  {
    id: "renpy",
    group: "handoff",
    title: "Ren'Py",
    format: ".rpy",
    desc: "そのまま起動できるスクリプト。",
  },
  {
    id: "orders",
    group: "handoff",
    title: "発注リスト",
    format: "CSV",
    desc: "仮素材と未登録の素材の一覧。",
  },
  {
    id: "text",
    group: "handoff",
    title: "通読用テキスト",
    format: ".txt",
    desc: "ルートごとに通して読む形。",
  },
  {
    id: "book",
    group: "other",
    title: "EPUB・縦書きPDF",
    format: ".epub・.html",
    desc: "小説として配る形。",
  },
  {
    id: "tyrano",
    group: "other",
    title: "ティラノスクリプト",
    format: ".ks",
    desc: "ティラノ用のシナリオ。",
  },
  {
    id: "utage",
    group: "other",
    title: "宴",
    format: "Excel",
    desc: "宴のシナリオと設定のシート。",
  },
  {
    id: "plugin",
    group: "other",
    title: "書き出しのプラグイン",
    format: "export/*.yaml",
    desc: "テンプレートかコマンドで書き出す。",
  },
  {
    id: "provenance",
    group: "other",
    title: "AIの出自",
    format: "CSV",
    desc: "AI が書いた行と項目の一覧。",
  },
];

/** 群ごとのカード（並びは表の順）。 */
export function tilesOf(group: ExportGroup): ExportTile[] {
  return EXPORT_TILES.filter((t) => t.group === group);
}

/** カードの下の1行（この窓で最後に書き出した時刻）。 */
export function tileFoot(last: string | undefined): string {
  return last === undefined ? "まだ書き出していません" : `前回 ${last}`;
}
