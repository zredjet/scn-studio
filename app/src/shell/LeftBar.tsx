// 左端のバー：画面の切り替え（docs/ui/README.md の画面の骨格）。フェーズ1で作る画面だけを出し、
// まだ作っていない画面は押せなくする。

export type Screen =
  | "text"
  | "flow"
  | "bible"
  | "variables"
  | "check"
  | "export";

const ITEMS: readonly { screen: Screen; label: string; later?: string }[] = [
  { screen: "text", label: "本文" },
  { screen: "flow", label: "フロー" },
  { screen: "bible", label: "設定", later: "T1-10で作ります" },
  { screen: "variables", label: "変数", later: "T1-10で作ります" },
  { screen: "check", label: "検証" },
  { screen: "export", label: "出力", later: "T1-12で作ります" },
];

interface Props {
  readonly screen: Screen;
  /** 検証の画面に出す件数（エラーと警告）。 */
  readonly problems: number;
  readonly onSelect: (screen: Screen) => void;
}

export function LeftBar({ screen, problems, onSelect }: Props) {
  return (
    <nav className="left-bar" aria-label="画面">
      {ITEMS.map((item) => (
        <button
          key={item.screen}
          type="button"
          className={item.screen === screen ? "left-item current" : "left-item"}
          disabled={item.later !== undefined}
          title={item.later}
          aria-current={item.screen === screen ? "page" : undefined}
          onClick={() => onSelect(item.screen)}
        >
          <span>{item.label}</span>
          {item.screen === "check" && problems > 0 && (
            <span className="badge">{problems}</span>
          )}
        </button>
      ))}
    </nav>
  );
}
