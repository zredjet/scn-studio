// 左端のバー：画面の切り替え（docs/ui/decisions-v1.md の U04、T4-17）。アイコンと名前を縦に積み、環境設定は線で
// 区切って一番下に離す。プレビューは本文と再生から開く表示なので置かない。検証には件数の札（警告だけなら琥珀、
// エラーがあれば赤）、AI には実行中の点。

import { Icon, type IconName } from "./icons.tsx";
import { IS_MAC, keyForScreen, keyLabel } from "./palette.ts";

export type Screen =
  /** 企画（企画書とアイデアメモ。T2-6）。 */
  | "concept"
  | "text"
  | "flow"
  | "bible"
  | "assets"
  | "variables"
  | "check"
  | "export"
  /** プレビュー（タイトルバーの「再生」から開く。T1-11）。 */
  | "preview"
  /** AI実行（T3-7）。 */
  | "ai"
  /** 環境設定（AIの接続先。T3-5）。 */
  | "preferences";

type RailScreen = Exclude<Screen, "preview">;

const ITEMS: readonly { screen: RailScreen; label: string }[] = [
  { screen: "concept", label: "企画" },
  { screen: "text", label: "本文" },
  { screen: "flow", label: "フロー" },
  { screen: "bible", label: "設定" },
  { screen: "assets", label: "素材" },
  { screen: "variables", label: "変数" },
  { screen: "check", label: "検証" },
  { screen: "ai", label: "AI" },
  { screen: "export", label: "出力" },
];

/** 画面に移るキーの書き方（⌘1 など。T4-14）。 */
function shortcutOf(screen: Screen): string | undefined {
  const keys = keyForScreen(screen);
  return keys === null ? undefined : keyLabel(keys, IS_MAC);
}

interface Props {
  readonly screen: Screen;
  /** 検証の画面に出す件数（エラーと警告）。 */
  readonly counts: { readonly error: number; readonly warning: number };
  /** AI の実行中（AI に点を付ける）。 */
  readonly aiRunning: boolean;
  readonly onSelect: (screen: Screen) => void;
}

export function LeftBar({ screen, counts, aiRunning, onSelect }: Props) {
  const item = (s: RailScreen, label: string) => {
    const problems = counts.error + counts.warning;
    const title = shortcutOf(s);
    return (
      <button
        key={s}
        type="button"
        className="rail-item"
        title={title === undefined ? label : `${label}（${title}）`}
        aria-label={
          s === "check" && problems > 0
            ? `${label}（エラー${counts.error}件・警告${counts.warning}件）`
            : s === "ai" && aiRunning
              ? `${label}（実行中）`
              : undefined
        }
        aria-current={s === screen ? "page" : undefined}
        onClick={() => onSelect(s)}
      >
        <Icon name={s as IconName} size={18} stroke={1.7} />
        {label}
        {s === "check" && problems > 0 && (
          <span
            className={counts.error > 0 ? "count is-error" : "count"}
            aria-hidden="true"
          >
            {problems}
          </span>
        )}
        {s === "ai" && aiRunning && <span className="run" aria-hidden="true" />}
      </button>
    );
  };
  return (
    <nav className="rail" aria-label="画面">
      {ITEMS.map((i) => item(i.screen, i.label))}
      <span className="rail-spacer" />
      <span className="rail-sep" />
      {item("preferences", "環境設定")}
    </nav>
  );
}
