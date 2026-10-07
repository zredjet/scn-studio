// 作品フォルダを開いていないときの画面：開く、新しく作る、最近開いたもの。

import type { RecentProject } from "../core/types.ts";

interface Props {
  readonly recent: readonly RecentProject[];
  readonly onOpen: () => void;
  readonly onCreate: () => void;
  readonly onOpenRecent: (path: string) => void;
}

export function Welcome({ recent, onOpen, onCreate, onOpenRecent }: Props) {
  return (
    <section className="welcome" aria-label="はじめに">
      <h1>作品フォルダを開く</h1>
      <div className="welcome-actions">
        <button type="button" className="primary-button" onClick={onOpen}>
          作品フォルダを開く
        </button>
        <button type="button" className="secondary-button" onClick={onCreate}>
          新しく作る
        </button>
      </div>
      {recent.length > 0 && (
        <>
          <h2 className="panel-heading">最近開いたもの</h2>
          <ul className="recent-list">
            {recent.map((r) => (
              <li key={r.path}>
                <button
                  type="button"
                  className="recent-item"
                  onClick={() => onOpenRecent(r.path)}
                >
                  <span className="recent-title">{r.title}</span>
                  <span className="recent-path">{r.path}</span>
                </button>
              </li>
            ))}
          </ul>
        </>
      )}
    </section>
  );
}
