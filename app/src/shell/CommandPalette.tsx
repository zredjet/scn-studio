// コマンドパレット（⌘K・Mod-Shift-P。T2-8、D059。見本 12-overlays.html の 2、T4-20）：コマンドに加えて、シーン・
// 設定DBのエントリ・素材を種類ごとに探して開く。`>` で始めるとコマンドだけ。探し方は `palette.ts`。
// 入力欄の変換を確定する Enter や矢印では動かない。

import { useEffect, useMemo, useRef, useState } from "react";
import { assetList, readBible } from "../core/api.ts";
import type { SceneSummary } from "../core/types.ts";
import { SceneMarkView } from "../scenes/SceneMark.tsx";
import { Icon } from "./icons.tsx";
import { Finder } from "./Overlay.tsx";
import {
  assetItems,
  type Command,
  commandItems,
  entryItems,
  type FindItem,
  type FindTarget,
  findGroups,
  IS_MAC,
  keyLabel,
  sceneItems,
} from "./palette.ts";

interface Props {
  readonly commands: readonly Command[];
  readonly scenes: readonly SceneSummary[];
  /** 選んだものを開く（コマンドは実行する）。 */
  readonly onOpen: (target: FindTarget) => void;
  readonly onClose: () => void;
}

const TAG_CLASS = { ai: "tag tag-ai", ph: "tag tag-ph", warn: "tag tag-warn" };

export function CommandPalette({ commands, scenes, onOpen, onClose }: Props) {
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const [loaded, setLoaded] = useState<readonly FindItem[]>([]);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => input.current?.focus(), []);
  // 設定DBのエントリと素材は、開いたときに読む（読めなければ、探す先に入れない）。
  useEffect(() => {
    let live = true;
    void Promise.allSettled([readBible(), assetList()]).then(
      ([bible, assets]) => {
        if (!live) return;
        setLoaded([
          ...(bible.status === "fulfilled" ? entryItems(bible.value) : []),
          ...(assets.status === "fulfilled"
            ? assetItems(assets.value.rows)
            : []),
        ]);
      },
    );
    return () => {
      live = false;
    };
  }, []);
  const groups = useMemo(
    () =>
      findGroups(
        [...sceneItems(scenes), ...loaded, ...commandItems(commands, IS_MAC)],
        query,
      ),
    [scenes, loaded, commands, query],
  );
  const flat = groups.flatMap((g) => g.items);
  const current = Math.min(index, Math.max(flat.length - 1, 0));
  const open = (item: FindItem | undefined) => {
    if (item === undefined) return;
    onClose();
    onOpen(item.target);
  };
  return (
    <Finder label="検索とコマンド" onClose={onClose}>
      <div className="finder-search">
        <label className="search">
          <Icon name="search" size={15} />
          <input
            ref={input}
            type="search"
            value={query}
            placeholder="シーン・設定・素材・コマンドを探す"
            aria-label="検索とコマンド"
            onChange={(event) => {
              setQuery(event.target.value);
              setIndex(0);
            }}
            onKeyDown={(event) => {
              if (event.nativeEvent.isComposing || event.keyCode === 229)
                return;
              if (event.key === "ArrowDown") {
                event.preventDefault();
                setIndex(Math.min(current + 1, flat.length - 1));
              } else if (event.key === "ArrowUp") {
                event.preventDefault();
                setIndex(Math.max(current - 1, 0));
              } else if (event.key === "Enter") {
                event.preventDefault();
                open(flat[current]);
              }
            }}
          />
        </label>
      </div>
      <div className="menu finder-list">
        {groups.map((g) => (
          <div key={g.kind} className="finder-group">
            <div className="menu-label">
              {g.label}
              {g.more > 0 && (
                <span className="finder-more">ほか {g.more}件</span>
              )}
            </div>
            {g.items.map((item) => {
              const i = flat.indexOf(item);
              return (
                <FinderRow
                  key={item.key}
                  item={item}
                  active={i === current}
                  onClick={() => open(item)}
                />
              );
            })}
          </div>
        ))}
        {flat.length === 0 && (
          <p className="empty finder-empty">見つかりません。</p>
        )}
      </div>
      <div className="finder-foot t-caption t-3">
        <span>
          <span className="mono">&gt;</span> で始めるとコマンドだけ
        </span>
        <span>{keyLabel("Mod-p", IS_MAC)} はシーンだけ</span>
        <span className="grow" />
        <span>↑↓ 選ぶ · Enter 開く · Esc 閉じる</span>
      </div>
    </Finder>
  );
}

/** 探す窓の1行（⌘K と ⌘P で同じ形）。 */
export function FinderRow({
  item,
  active,
  onClick,
}: {
  readonly item: FindItem;
  readonly active: boolean;
  readonly onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={
        active ? "menu-item finder-row is-active" : "menu-item finder-row"
      }
      aria-current={active ? "true" : undefined}
      onClick={onClick}
    >
      {item.mark !== null && <SceneMarkView mark={item.mark} />}
      {item.lead !== "" && (
        <span
          className={
            item.kind === "scene"
              ? "finder-lead mono t-caption"
              : "finder-lead t-caption"
          }
        >
          {item.lead}
        </span>
      )}
      <span className="finder-label ellipsis">{item.label}</span>
      {item.tag !== null && (
        <span className={TAG_CLASS[item.tag.tone]}>{item.tag.text}</span>
      )}
      {item.trail !== "" && (
        <span className="finder-trail t-caption t-3">{item.trail}</span>
      )}
    </button>
  );
}
