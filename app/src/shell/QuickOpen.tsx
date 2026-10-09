// シーンのクイックオープン（Mod-P。D047）：IDと題名で絞り、選ぶとそのシーンへ移る。
// コマンドパレットはフェーズ2。入力欄の変換を確定する Enter では選ばない。

import { useEffect, useMemo, useRef, useState } from "react";
import type { SceneSummary } from "../core/types.ts";
import { useOverlay } from "./useOverlay.ts";

interface Props {
  readonly scenes: readonly SceneSummary[];
  readonly onSelect: (scene: SceneSummary) => void;
  readonly onClose: () => void;
}

/** 表示する候補の数。 */
const LIMIT = 50;

/** 絞り込み：IDか題名に、空白で区切った語がすべて含まれる（大文字と小文字は区別しない）。 */
export function filterScenes(
  scenes: readonly SceneSummary[],
  query: string,
): SceneSummary[] {
  const words = query
    .toLowerCase()
    .split(/\s+/)
    .filter((w) => w !== "");
  return scenes.filter((s) => {
    const hay = `${s.id} ${s.title}`.toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}

export function QuickOpen({ scenes, onSelect, onClose }: Props) {
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const found = useMemo(
    () => filterScenes(scenes, query).slice(0, LIMIT),
    [scenes, query],
  );
  useEffect(() => input.current?.focus(), []);
  const overlay = useOverlay<HTMLDivElement>(onClose);
  const pick = (scene: SceneSummary | undefined) => {
    if (scene === undefined) return;
    onSelect(scene);
    onClose();
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
        aria-label="シーンを開く"
        ref={overlay.ref}
        tabIndex={-1}
        onKeyDown={overlay.onKeyDown}
      >
        <input
          ref={input}
          value={query}
          placeholder="シーンのIDか題名"
          aria-label="シーンのIDか題名"
          onChange={(event) => {
            setQuery(event.target.value);
            setIndex(0);
          }}
          onKeyDown={(event) => {
            // 変換を確定する Enter や矢印では動かない。
            if (event.nativeEvent.isComposing) return;
            if (event.key === "ArrowDown") {
              event.preventDefault();
              setIndex((i) => Math.min(i + 1, found.length - 1));
            } else if (event.key === "ArrowUp") {
              event.preventDefault();
              setIndex((i) => Math.max(i - 1, 0));
            } else if (event.key === "Enter") {
              event.preventDefault();
              pick(found[index]);
            }
          }}
        />
        <ul>
          {found.map((s, i) => (
            <li key={s.id}>
              <button
                type="button"
                className={`quick-open-item${i === index ? " selected" : ""}`}
                onClick={() => pick(s)}
              >
                <span className="quick-open-id">{s.id}</span>
                <span>{s.title}</span>
                <span className="quick-open-file">{s.file}</span>
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
