// シーンを開く（⌘P。D047。見本 12-overlays.html の 2、T4-20）：IDと名前で絞り、状態の印・ID・名前・ルートを並べる。
// 選ぶとそのシーンへ移る。入力欄の変換を確定する Enter や矢印では動かない。

import { useEffect, useMemo, useRef, useState } from "react";
import type { SceneSummary } from "../core/types.ts";
import { filterScenes } from "../scenes/list.ts";
import { FinderRow } from "./CommandPalette.tsx";
import { Icon } from "./icons.tsx";
import { Finder } from "./Overlay.tsx";
import { sceneItems } from "./palette.ts";

interface Props {
  readonly scenes: readonly SceneSummary[];
  readonly onSelect: (scene: SceneSummary) => void;
  readonly onClose: () => void;
}

/** 表示する候補の数。 */
const LIMIT = 50;

export function QuickOpen({ scenes, onSelect, onClose }: Props) {
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const found = useMemo(
    () =>
      sceneItems(filterScenes(scenes, query).slice(0, LIMIT)).map((item) => ({
        // ⌘P の右はルートだけ（見本 12）。
        ...item,
        trail:
          item.target.kind === "scene"
            ? (item.target.scene.meta.route ?? "")
            : "",
      })),
    [scenes, query],
  );
  useEffect(() => input.current?.focus(), []);
  const current = Math.min(index, Math.max(found.length - 1, 0));
  const pick = (i: number) => {
    const target = found[i]?.target;
    if (target?.kind !== "scene") return;
    onSelect(target.scene);
    onClose();
  };
  return (
    <Finder label="シーンを開く" onClose={onClose}>
      <div className="finder-search">
        <label className="search">
          <Icon name="search" size={15} />
          <input
            ref={input}
            type="search"
            value={query}
            placeholder="シーンのIDか名前"
            aria-label="シーンを開く"
            onChange={(event) => {
              setQuery(event.target.value);
              setIndex(0);
            }}
            onKeyDown={(event) => {
              // 変換を確定する Enter や矢印では動かない。
              if (event.nativeEvent.isComposing || event.keyCode === 229)
                return;
              if (event.key === "ArrowDown") {
                event.preventDefault();
                setIndex(Math.min(current + 1, found.length - 1));
              } else if (event.key === "ArrowUp") {
                event.preventDefault();
                setIndex(Math.max(current - 1, 0));
              } else if (event.key === "Enter") {
                event.preventDefault();
                pick(current);
              }
            }}
          />
        </label>
      </div>
      <div className="menu finder-list">
        {found.map((item, i) => (
          <FinderRow
            key={item.key}
            item={{ ...item, tag: null }}
            active={i === current}
            onClick={() => pick(i)}
          />
        ))}
        {found.length === 0 && (
          <p className="empty finder-empty">見つかりません。</p>
        )}
      </div>
      <div className="finder-foot t-caption t-3">
        <span>IDと名前の両方で探す</span>
        <span className="grow" />
        <span>↑↓ 選ぶ · Enter 開く · Esc 閉じる</span>
      </div>
    </Finder>
  );
}
