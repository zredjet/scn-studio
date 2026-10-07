// エディタの上の帯：今のファイルと、表示の切り替え（執筆表示｜ソース表示。docs/ui/mockups/02-editor.html）。

import type { ViewMode } from "../core/types.ts";

interface Props {
  readonly file: string;
  readonly mode: ViewMode;
  readonly onSelect: (mode: ViewMode) => void;
}

/** 切り替えのキーの書き方（macOS は ⌘、ほかは Ctrl）。 */
const SHORTCUT =
  typeof navigator !== "undefined" && /Mac/.test(navigator.userAgent)
    ? "⌘ /"
    : "Ctrl /";

const MODES: readonly { readonly mode: ViewMode; readonly label: string }[] = [
  { mode: "writing", label: "執筆表示" },
  { mode: "source", label: "ソース表示" },
];

export function ModeBar({ file, mode, onSelect }: Props) {
  return (
    <div className="mode-bar">
      <span className="mode-bar-file">{file}</span>
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
      <span className="mode-shortcut">{SHORTCUT}</span>
    </div>
  );
}
