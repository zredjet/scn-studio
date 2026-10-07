// タイトルバー：作品名、作品フォルダを開く・新しく作る、最近開いたもの。

import type { RecentProject } from "../core/types.ts";

interface Props {
  readonly title: string | null;
  readonly recent: readonly RecentProject[];
  readonly onOpen: () => void;
  readonly onCreate: () => void;
  readonly onOpenRecent: (path: string) => void;
}

export function TitleBar({
  title,
  recent,
  onOpen,
  onCreate,
  onOpenRecent,
}: Props) {
  return (
    <header className="titlebar">
      <span className="app-name">scn-studio</span>
      {title !== null && <span className="project-title">{title}</span>}
      <span className="titlebar-spacer" />
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
