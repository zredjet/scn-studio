// タイトルバー：作品名、再生（プレビュー。T1-11）とここから再生（T2-4）、作品フォルダを開く・新しく作る、
// 最近開いたもの。

import type { RecentProject } from "../core/types.ts";

interface Props {
  readonly title: string | null;
  readonly recent: readonly RecentProject[];
  readonly onOpen: () => void;
  readonly onCreate: () => void;
  readonly onOpenRecent: (path: string) => void;
  /** 再生（プレビューの画面を開く）。作品フォルダを開いていなければ null。 */
  readonly onPlay: (() => void) | null;
  /** ここから再生（エディタのカーソルの行から）。カーソルが無ければ null。 */
  readonly onPlayHere: (() => void) | null;
}

export function TitleBar({
  title,
  recent,
  onOpen,
  onCreate,
  onOpenRecent,
  onPlay,
  onPlayHere,
}: Props) {
  return (
    <header className="titlebar">
      <span className="app-name">scn-studio</span>
      {title !== null && <span className="project-title">{title}</span>}
      <span className="titlebar-spacer" />
      {onPlay !== null && (
        <button
          type="button"
          className="chrome-button play"
          title="プレビューで再生する"
          onClick={onPlay}
        >
          ▶ 再生
        </button>
      )}
      {onPlay !== null && (
        <button
          type="button"
          className="chrome-button play"
          title="エディタのカーソルの行から再生する（F5）。開始シーンからの最短経路で、変数と舞台をそろえる"
          disabled={onPlayHere === null}
          onClick={onPlayHere ?? undefined}
        >
          ▶ ここから
        </button>
      )}
      {recent.length > 0 && (
        <select
          className="recent-select"
          aria-label="最近開いた作品フォルダ"
          value=""
          onChange={(event) => {
            if (event.target.value !== "") onOpenRecent(event.target.value);
          }}
        >
          <option value="">最近開いたもの</option>
          {recent.map((r) => (
            <option key={r.path} value={r.path}>
              {r.title}（{r.path}）
            </option>
          ))}
        </select>
      )}
      <button type="button" className="chrome-button" onClick={onOpen}>
        開く
      </button>
      <button type="button" className="chrome-button" onClick={onCreate}>
        新しく作る
      </button>
    </header>
  );
}
