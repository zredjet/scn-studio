// 本文の作業領域の見出しの帯（docs/ui/decisions-v1.md の U13、見本 02-editor.html。T4-18）：章とシーンの道しるべ、
// 表示の切り替え（執筆表示｜ソース表示）、検索と置換、表示のメニュー（集中モード・タイプライタースクロール・本文の書体）。

import { type RefObject, useRef, useState } from "react";
import type { ViewMode } from "../core/types.ts";
import { Icon } from "../shell/icons.tsx";
import { IS_MAC, keyLabel } from "../shell/palette.ts";
import { useOutsideClose, useOverlay } from "../shell/useOverlay.ts";

interface Props {
  readonly file: string;
  /** カーソルのあるシーン（無ければ null）。 */
  readonly scene: { readonly id: string; readonly title: string } | null;
  readonly mode: ViewMode;
  readonly onSelect: (mode: ViewMode) => void;
  /** 検索と置換の欄を開く。 */
  readonly onSearch: () => void;
  /** 表示の切り替え（今の状態と、切り替える）。 */
  readonly view: {
    readonly focus: boolean;
    readonly typewriter: boolean;
    readonly gothic: boolean;
    readonly onFocus: () => void;
    readonly onTypewriter: () => void;
    readonly onGothic: () => void;
  };
}

const MODES: readonly { readonly mode: ViewMode; readonly label: string }[] = [
  { mode: "writing", label: "執筆表示" },
  { mode: "source", label: "ソース表示" },
];

export function ModeBar({
  file,
  scene,
  mode,
  onSelect,
  onSearch,
  view,
}: Props) {
  const [menu, setMenu] = useState(false);
  const more = useRef<HTMLButtonElement>(null);
  return (
    <div className="mode-bar work-head">
      <nav className="crumbs" aria-label="今の場所">
        <span className="crumb-file">{file.replace(/^story\//, "")}</span>
        {scene !== null && (
          <>
            <span className="crumb-sep" aria-hidden="true">
              ›
            </span>
            <span className="crumb-scene">
              <span className="mono">{scene.id}</span> {scene.title}
            </span>
          </>
        )}
      </nav>
      <fieldset className="mode-switch" aria-label="表示">
        {MODES.map((m) => (
          <button
            key={m.mode}
            type="button"
            aria-pressed={mode === m.mode}
            onClick={() => onSelect(m.mode)}
          >
            {m.label}
          </button>
        ))}
      </fieldset>
      <kbd className="mode-shortcut" title="表示を切り替えるキー">
        {keyLabel("Mod-/", IS_MAC)}
      </kbd>
      <button
        type="button"
        className="btn btn-icon"
        aria-label="検索と置換"
        title={`検索と置換（${keyLabel("Mod-F", IS_MAC)}）`}
        onClick={onSearch}
      >
        <Icon name="search" size={16} />
      </button>
      <span className="chapter-menu-anchor">
        <button
          ref={more}
          type="button"
          className="btn btn-icon"
          aria-label="表示のメニュー"
          aria-haspopup="menu"
          aria-expanded={menu}
          onClick={() => setMenu((m) => !m)}
        >
          …
        </button>
        {menu && (
          <ViewMenu anchor={more} view={view} onClose={() => setMenu(false)} />
        )}
      </span>
    </div>
  );
}

function ViewMenu({
  anchor,
  view,
  onClose,
}: {
  readonly anchor: RefObject<HTMLButtonElement | null>;
  readonly view: Props["view"];
  readonly onClose: () => void;
}) {
  const { ref, onKeyDown } = useOverlay<HTMLDivElement>(onClose);
  useOutsideClose(ref, anchor, onClose);
  const item = (label: string, on: boolean, run: () => void, keys?: string) => (
    <button
      type="button"
      role="menuitemcheckbox"
      aria-checked={on}
      className="menu-item"
      onClick={() => {
        onClose();
        run();
      }}
    >
      <span className="menu-check" aria-hidden="true">
        {on ? "✓" : ""}
      </span>
      {label}
      {keys !== undefined && <kbd>{keyLabel(keys, IS_MAC)}</kbd>}
    </button>
  );
  return (
    <div
      ref={ref}
      className="popover chapter-menu view-menu"
      role="menu"
      aria-label="表示のメニュー"
      tabIndex={-1}
      onKeyDown={onKeyDown}
    >
      <div className="menu">
        {item("集中モード", view.focus, view.onFocus, "Mod-Shift-F")}
        {item("タイプライタースクロール", view.typewriter, view.onTypewriter)}
        {item("本文をゴシックにする", view.gothic, view.onGothic)}
      </div>
    </div>
  );
}
