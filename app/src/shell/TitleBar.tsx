// タイトルバー（docs/ui/decisions-v1.md の U02・U03・U06、T4-17。見本 00-components.html の 1、12-overlays.html の 1）。
// 自前で描く（D086）：macOS は重ねるタイトルバーで信号機のボタンを残し、左に 72px を空ける。Windows は枠を消し、
// しまう・最大化・閉じるのボタンを右端に置く。空いた所を押して窓を動かす（data-tauri-drag-region）。
// 左：作品メニュー（最近開いた作品・開く・新しく作る・フォルダを表示・この作品の設定）とブランチ／中央：検索と
// コマンド（⌘K）／右：AI の様子（接続先を名前ごとに印の形で並べ、押すと詳しい様子）・再生（分割）・右の欄の開け閉め。

import { type RefObject, useRef, useState } from "react";
import {
  closeThisWindow,
  minimizeThisWindow,
  toggleMaximizeThisWindow,
} from "../core/api.ts";
import type { RecentProject } from "../core/types.ts";
import type { AiStatus, ConnectionStatus } from "../preferences/connection.ts";
import { Icon } from "./icons.tsx";
import { IS_MAC, keyLabel } from "./palette.ts";
import { useOutsideClose, useOverlay } from "./useOverlay.ts";

/** 作品メニューに出す最近開いた作品の数（見本どおり）。 */
const RECENT_SHOWN = 5;

interface Props {
  /** 開いている作品の名前（開いていなければ null）。 */
  readonly title: string | null;
  /** 今のブランチ（Git でなければ null）。 */
  readonly branch: string | null;
  readonly recent: readonly RecentProject[];
  readonly onOpen: () => void;
  readonly onCreate: () => void;
  readonly onOpenRecent: (path: string) => void;
  readonly onReveal: () => void;
  readonly onProjectSettings: () => void;
  /** 検索とコマンド（今はコマンドパレットを開く）。 */
  readonly onSearch: () => void;
  readonly ai: AiStatus | null;
  readonly onOpenAi: () => void;
  readonly onPreferences: () => void;
  /** 再生（作品を開いていなければ null）。 */
  readonly play: {
    /** 押したときの始め方の説明（本文はカーソルの位置、フローは選んだシーン、ほかは最初から）。 */
    readonly title: string;
    readonly run: () => void;
    readonly fromStart: () => void;
    /** このシーンの頭から（フローは選んだシーン、ほかは本文のカーソルのシーン。無ければ null）。 */
    readonly fromScene: {
      readonly scene: string;
      readonly run: () => void;
    } | null;
  } | null;
  /** 右の欄の開け閉め（右の欄の無い画面では押せない）。 */
  readonly panel: {
    readonly shown: boolean;
    readonly available: boolean;
    readonly onToggle: () => void;
  };
}

export function TitleBar(props: Props) {
  const { title, branch, ai, play, panel } = props;
  const [menu, setMenu] = useState<"project" | "ai" | "play" | null>(null);
  const projectButton = useRef<HTMLButtonElement>(null);
  const aiButton = useRef<HTMLButtonElement>(null);
  const playButton = useRef<HTMLButtonElement>(null);
  const close = () => setMenu(null);
  const toggle = (which: "project" | "ai" | "play") =>
    setMenu((m) => (m === which ? null : which));
  return (
    <header className="tb" data-tauri-drag-region>
      <div className="tb-l" data-tauri-drag-region>
        {IS_MAC && <span className="tb-traffic" data-tauri-drag-region />}
        <div className="tb-anchor">
          <button
            ref={projectButton}
            type="button"
            className="tb-btn tb-project"
            aria-haspopup="menu"
            aria-expanded={menu === "project"}
            title="作品のメニュー"
            onClick={() => toggle("project")}
          >
            <Icon name="bible" />
            <span className="name">{title ?? "scn-studio"}</span>
            <Icon name="chevron" size={12} stroke={2.2} />
          </button>
          {menu === "project" && (
            <ProjectMenu
              {...props}
              hasProject={title !== null}
              anchor={projectButton}
              onClose={close}
            />
          )}
        </div>
        {branch !== null && (
          <span className="tb-sub" title={`ブランチ ${branch}`}>
            <Icon name="branch" size={14} />
            <span className="mono hide-narrow">{branch}</span>
          </span>
        )}
      </div>
      {title !== null ? (
        <button
          type="button"
          className="tb-search"
          title={`コマンドを探す（${keyLabel("Mod-K", IS_MAC)}）`}
          onClick={props.onSearch}
        >
          <Icon name="search" size={15} />
          <span className="tb-search-text">コマンドを探す</span>
          <kbd>{keyLabel("Mod-K", IS_MAC)}</kbd>
        </button>
      ) : (
        <span data-tauri-drag-region />
      )}
      <div className="tb-r" data-tauri-drag-region>
        {ai !== null && (
          <div className="tb-anchor">
            <button
              ref={aiButton}
              type="button"
              className="tb-btn tb-ai"
              aria-haspopup="dialog"
              aria-expanded={menu === "ai"}
              title={ai.title}
              aria-label={ai.title}
              onClick={() => toggle("ai")}
            >
              <Icon name="ai" size={15} stroke={1.7} />
              {ai.running > 0 && (
                <span className="who tb-running">
                  <span className="spin" aria-hidden="true" />
                  実行中 {ai.running}
                </span>
              )}
              {ai.connections.length === 0 ? (
                <span className="who">
                  <ConnectionMark mark="off" />
                  未設定
                </span>
              ) : (
                ai.connections.map((c) => (
                  <span key={c.name} className="who">
                    <ConnectionMark mark={c.mark} />
                    {c.name}
                  </span>
                ))
              )}
              <Icon name="chevron" size={12} stroke={2.2} />
            </button>
            {menu === "ai" && (
              <AiPopover
                ai={ai}
                anchor={aiButton}
                onClose={close}
                onOpenAi={() => {
                  close();
                  props.onOpenAi();
                }}
                onPreferences={() => {
                  close();
                  props.onPreferences();
                }}
              />
            )}
          </div>
        )}
        {play !== null && (
          <>
            <span className="tb-sep" />
            <div className="tb-anchor">
              <div className="split">
                <button
                  type="button"
                  className="btn btn-primary btn-sm"
                  title={`${play.title}（F5 はカーソルの位置から）`}
                  onClick={play.run}
                >
                  <Icon name="play" size={12} />
                  再生
                </button>
                <button
                  ref={playButton}
                  type="button"
                  className="btn btn-primary btn-sm"
                  aria-label="再生の始め方を選ぶ"
                  aria-haspopup="menu"
                  aria-expanded={menu === "play"}
                  onClick={() => toggle("play")}
                >
                  <Icon name="chevron" size={12} stroke={2.2} />
                </button>
              </div>
              {menu === "play" && (
                <PlayMenu
                  anchor={playButton}
                  onClose={close}
                  fromStart={() => {
                    close();
                    play.fromStart();
                  }}
                  fromScene={
                    play.fromScene === null
                      ? null
                      : {
                          scene: play.fromScene.scene,
                          run: () => {
                            close();
                            play.fromScene?.run();
                          },
                        }
                  }
                />
              )}
            </div>
          </>
        )}
        {title !== null && (
          <button
            type="button"
            className="tb-btn tb-icon tb-panel-toggle"
            aria-label="右の欄"
            aria-pressed={panel.shown}
            disabled={!panel.available}
            title={
              panel.available
                ? `右の欄を開け閉め（${keyLabel("Alt-Mod-B", IS_MAC)}）`
                : "この画面には右の欄がありません"
            }
            onClick={panel.onToggle}
          >
            <Icon name="panel" stroke={1.7} />
          </button>
        )}
        {!IS_MAC && <WindowControls />}
      </div>
    </header>
  );
}

function ConnectionMark({ mark }: { readonly mark: ConnectionStatus["mark"] }) {
  if (mark === "lock") return <Icon name="lock" size={12} stroke={2} />;
  return (
    <span className={mark === "on" ? "gl-on" : "gl-off"} aria-hidden="true" />
  );
}

interface MenuProps {
  readonly anchor: RefObject<HTMLButtonElement | null>;
  readonly onClose: () => void;
}

function ProjectMenu({
  recent,
  hasProject,
  anchor,
  onClose,
  onOpen,
  onCreate,
  onOpenRecent,
  onReveal,
  onProjectSettings,
}: Props & MenuProps & { readonly hasProject: boolean }) {
  const { ref, onKeyDown } = useOverlay<HTMLDivElement>(onClose);
  useOutsideClose(ref, anchor, onClose);
  const run = (action: () => void) => () => {
    onClose();
    action();
  };
  return (
    <div
      ref={ref}
      className="popover tb-pop tb-pop-left"
      role="menu"
      aria-label="作品のメニュー"
      tabIndex={-1}
      onKeyDown={onKeyDown}
    >
      <div className="menu">
        {recent.length > 0 && (
          <>
            <div className="menu-label">最近開いた作品</div>
            {recent.slice(0, RECENT_SHOWN).map((r) => (
              <button
                key={r.path}
                type="button"
                role="menuitem"
                className="menu-item menu-item-2"
                title={r.path}
                onClick={run(() => onOpenRecent(r.path))}
              >
                <span className="ellipsis">{r.title}</span>
                <span className="menu-sub ellipsis">{r.path}</span>
              </button>
            ))}
            <div className="menu-sep" />
          </>
        )}
        <button
          type="button"
          role="menuitem"
          className="menu-item"
          onClick={run(onOpen)}
        >
          開く…<kbd>{keyLabel("Mod-O", IS_MAC)}</kbd>
        </button>
        <button
          type="button"
          role="menuitem"
          className="menu-item"
          onClick={run(onCreate)}
        >
          新しく作る…<kbd>{keyLabel("Mod-N", IS_MAC)}</kbd>
        </button>
        {hasProject && (
          <>
            <div className="menu-sep" />
            <button
              type="button"
              role="menuitem"
              className="menu-item"
              onClick={run(onReveal)}
            >
              フォルダを表示
            </button>
            <button
              type="button"
              role="menuitem"
              className="menu-item"
              onClick={run(onProjectSettings)}
            >
              この作品の設定
            </button>
          </>
        )}
      </div>
    </div>
  );
}

function AiPopover({
  ai,
  anchor,
  onClose,
  onOpenAi,
  onPreferences,
}: MenuProps & {
  readonly ai: AiStatus;
  readonly onOpenAi: () => void;
  readonly onPreferences: () => void;
}) {
  const { ref, onKeyDown } = useOverlay<HTMLDivElement>(onClose);
  useOutsideClose(ref, anchor, onClose);
  return (
    <div
      ref={ref}
      className="popover tb-pop tb-pop-right"
      role="dialog"
      aria-label="AIの様子"
      tabIndex={-1}
      onKeyDown={onKeyDown}
    >
      {ai.connections.length === 0 ? (
        <p className="ai-pop-row empty">
          接続先がありません。project.toml に [ai.local] などを書くと使えます。
        </p>
      ) : (
        ai.connections.map((c) => (
          <div key={c.name} className="ai-pop-row">
            <span className={`ai-pop-mark ${c.mark}`}>
              {c.mark === "lock" ? (
                <Icon name="lock" size={13} stroke={2} />
              ) : (
                <span className={c.mark === "on" ? "dot-on" : "dot-off"} />
              )}
            </span>
            <div className="ai-pop-body">
              <div className="ai-pop-name">
                <strong>{c.name}</strong>
                <span className="tag">{c.place}</span>
                {c.isDefault && <span className="tag tag-accent">既定</span>}
                <span className="ai-pop-state">{c.state}</span>
              </div>
              <div className="t-small t-3 ai-pop-detail">{c.detail}</div>
            </div>
            {c.mark === "lock" && (
              <button
                type="button"
                className="btn btn-link btn-sm"
                title="環境設定で、この端末から送ることを許可します"
                onClick={onPreferences}
              >
                許可する…
              </button>
            )}
          </div>
        ))
      )}
      <div className="ai-pop-foot">
        <span className="t-small t-2">実行中 {ai.running}</span>
        <span className="grow" />
        <button type="button" className="btn btn-sm" onClick={onOpenAi}>
          AI実行を開く
        </button>
        <button type="button" className="btn btn-sm" onClick={onPreferences}>
          環境設定
        </button>
      </div>
    </div>
  );
}

function PlayMenu({
  anchor,
  onClose,
  fromStart,
  fromScene,
}: MenuProps & {
  readonly fromStart: () => void;
  readonly fromScene: {
    readonly scene: string;
    readonly run: () => void;
  } | null;
}) {
  const { ref, onKeyDown } = useOverlay<HTMLDivElement>(onClose);
  useOutsideClose(ref, anchor, onClose);
  return (
    <div
      ref={ref}
      className="popover tb-pop tb-pop-right tb-pop-narrow"
      role="menu"
      aria-label="再生の始め方"
      tabIndex={-1}
      onKeyDown={onKeyDown}
    >
      <div className="menu">
        <button
          type="button"
          role="menuitem"
          className="menu-item"
          onClick={fromStart}
        >
          最初から
        </button>
        <button
          type="button"
          role="menuitem"
          className="menu-item"
          disabled={fromScene === null}
          title={
            fromScene === null
              ? "本文でカーソルをシーンに置くか、フローでシーンを選ぶと使えます"
              : `${fromScene.scene} の頭から（開始シーンからの最短経路で、変数と舞台をそろえる）`
          }
          onClick={fromScene?.run}
        >
          このシーンから
          {fromScene !== null && <kbd>{fromScene.scene}</kbd>}
        </button>
      </div>
    </div>
  );
}

/** Windows の窓の操作（枠を消したので自前で置く。D086）。 */
function WindowControls() {
  return (
    <div className="tb-win">
      <button
        type="button"
        className="tb-win-btn"
        aria-label="しまう"
        onClick={() => void minimizeThisWindow()}
      >
        <Icon name="minimize" size={14} stroke={1.4} />
      </button>
      <button
        type="button"
        className="tb-win-btn"
        aria-label="最大化と元に戻す"
        onClick={() => void toggleMaximizeThisWindow()}
      >
        <Icon name="maximize" size={12} stroke={1.4} />
      </button>
      <button
        type="button"
        className="tb-win-btn close"
        aria-label="閉じる"
        onClick={() => void closeThisWindow()}
      >
        <Icon name="close" size={14} stroke={1.4} />
      </button>
    </div>
  );
}
