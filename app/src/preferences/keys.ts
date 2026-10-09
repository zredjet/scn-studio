// 環境設定の「キーの割り当て」の頁（T4-25、N6）：今あるキーの一覧（見るだけ。変えられない）。キーの書き方は
// `shell/palette.ts` の `keyLabel` で OS に合わせる。キーを足したら、ここにも足す。

import { SCREEN_KEYS } from "../shell/palette.ts";

export interface KeyRow {
  /** キー（`Mod-K` の書き方）。 */
  readonly keys: string;
  readonly label: string;
}

export interface KeyGroup {
  readonly label: string;
  readonly rows: readonly KeyRow[];
}

/** 画面の名前（左端のバーと同じ）。 */
const SCREEN_NAMES: Readonly<Record<string, string>> = {
  concept: "企画",
  text: "本文",
  flow: "フロー",
  bible: "設定",
  assets: "素材",
  variables: "変数",
  check: "検証",
  ai: "AI実行",
  export: "出力",
  preferences: "環境設定",
};

/** キーの一覧（全体・画面を移る・本文）。 */
export function keyGroups(): readonly KeyGroup[] {
  return [
    {
      label: "全体",
      rows: [
        {
          keys: "Mod-K",
          label: "検索とコマンド（シーン・設定・素材・コマンドを探す）",
        },
        { keys: "Mod-Shift-P", label: "検索とコマンド" },
        { keys: "Mod-P", label: "シーンを開く" },
        { keys: "F5", label: "ここから再生" },
        { keys: "Alt-Mod-B", label: "右の欄を開く・閉じる" },
        { keys: "Mod-O", label: "作品を開く" },
        { keys: "Mod-N", label: "新しい作品を作る" },
        { keys: "Esc", label: "重ね表示（ダイアログ・探す窓）を閉じる" },
      ],
    },
    {
      label: "画面を移る",
      rows: SCREEN_KEYS.map(([key, screen]) => ({
        keys: `Mod-${key}`,
        label: SCREEN_NAMES[screen] ?? screen,
      })),
    },
    {
      label: "本文",
      rows: [
        { keys: "Mod-S", label: "保存" },
        { keys: "Mod-/", label: "執筆表示とソース表示を切り替える" },
        { keys: "Mod-F", label: "検索と置換" },
        { keys: "Mod-G", label: "次を探す" },
        { keys: "Mod-Shift-G", label: "前を探す" },
        { keys: "F2", label: "名前を変える（カーソルのところ）" },
        { keys: "F12", label: "定義へ移る" },
        { keys: "Mod-Shift-F", label: "集中モード" },
        { keys: "Mod-Z", label: "取り消す" },
        { keys: "Mod-Shift-Z", label: "やり直す" },
      ],
    },
  ];
}
