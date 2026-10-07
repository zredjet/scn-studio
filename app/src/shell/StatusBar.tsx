// ステータスバー：今の表示、今のファイル、保存していないファイルの数、問題の数、形式のバージョン、知らせ。

import type { Severity, ViewMode } from "../core/types.ts";

interface Props {
  /** 今の表示（ファイルを開いていなければ null）。 */
  readonly mode: ViewMode | null;
  readonly file: string | null;
  readonly unsaved: number;
  readonly counts: Record<Severity, number>;
  readonly notice: string | null;
}

export function StatusBar({ mode, file, unsaved, counts, notice }: Props) {
  return (
    <footer className="statusbar">
      {mode !== null && (
        <span className="statusbar-mode">
          {mode === "writing" ? "執筆表示" : "ソース表示"}
        </span>
      )}
      <span>{file ?? "ファイル未選択"}</span>
      {unsaved > 0 && <span className="unsaved">未保存 {unsaved}</span>}
      <span className="statusbar-notice" role="status">
        {notice ?? ""}
      </span>
      <span>
        エラー {counts.error}・警告 {counts.warning}・情報 {counts.info}
      </span>
      <span>形式 v0</span>
    </footer>
  );
}
