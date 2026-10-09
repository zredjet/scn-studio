// コマンドパレット（Mod-Shift-P。T2-8、D059）：画面の切り替え、再生、名前の変更、集中モードなどを名前で探して
// 行う。絞り込みは `palette.ts`。入力欄の変換を確定する Enter では選ばない。

import { useEffect, useMemo, useRef, useState } from "react";
import { type Command, filterCommands, IS_MAC, keyLabel } from "./palette.ts";
import { useOverlay } from "./useOverlay.ts";

interface Props {
  readonly commands: readonly Command[];
  readonly onClose: () => void;
}

export function CommandPalette({ commands, onClose }: Props) {
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const found = useMemo(
    () => filterCommands(commands, query),
    [commands, query],
  );
  useEffect(() => input.current?.focus(), []);
  const overlay = useOverlay<HTMLDivElement>(onClose);
  const run = (command: Command | undefined) => {
    if (command === undefined) return;
    onClose();
    command.run();
  };
  return (
    <div className="quick-open-layer">
      <button
        type="button"
        className="quick-open-backdrop"
        aria-label="閉じる"
        tabIndex={-1}
        onClick={onClose}
      />
      <div
        className="quick-open"
        role="dialog"
        aria-label="コマンド"
        ref={overlay.ref}
        tabIndex={-1}
        onKeyDown={overlay.onKeyDown}
      >
        <input
          ref={input}
          value={query}
          placeholder="コマンドの名前"
          aria-label="コマンドの名前"
          onChange={(event) => {
            setQuery(event.target.value);
            setIndex(0);
          }}
          onKeyDown={(event) => {
            if (event.nativeEvent.isComposing || event.keyCode === 229) return;
            if (event.key === "ArrowDown") {
              event.preventDefault();
              setIndex((i) => Math.min(i + 1, found.length - 1));
            } else if (event.key === "ArrowUp") {
              event.preventDefault();
              setIndex((i) => Math.max(i - 1, 0));
            } else if (event.key === "Enter") {
              event.preventDefault();
              run(found[index]);
            }
          }}
        />
        <ul>
          {found.map((c, i) => (
            <li key={c.id}>
              <button
                type="button"
                className={`quick-open-item${i === index ? " selected" : ""}`}
                onClick={() => run(c)}
              >
                <span>{c.label}</span>
                <span className="quick-open-file">
                  {c.keys === undefined ? "" : keyLabel(c.keys, IS_MAC)}
                </span>
              </button>
            </li>
          ))}
          {found.length === 0 && (
            <li className="placeholder">見つかりません。</li>
          )}
        </ul>
      </div>
    </div>
  );
}
