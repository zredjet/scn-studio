// ステータスバー：今の表示、今のシーンとファイルの文字数、保存していないファイルの数、問題の数、
// 形式のバージョン、知らせ。

import type { Severity, ViewMode } from "../core/types.ts";
import { formatChars } from "./scene.ts";

interface Props {
  /** 今の表示（ファイルを開いていなければ null）。 */
  readonly mode: ViewMode | null;
  readonly file: string | null;
  /** 今のシーンと文字数（D047）。 */
  readonly scene: { readonly id: string; readonly chars: number } | null;
  /** 今のファイルの文字数。 */
  readonly fileChars: number | null;
  readonly unsaved: number;
  readonly counts: Record<Severity, number>;
  readonly notice: string | null;
}

export function StatusBar({
  mode,
  file,
  scene,
  fileChars,
  unsaved,
  counts,
  notice,
}: Props) {
  return (
    <footer className="statusbar">
      {mode !== null && (
        <span className="statusbar-mode">
          {mode === "writing" ? "執筆表示" : "ソース表示"}
        </span>
      )}
      {scene !== null && (
        <span>
          {scene.id} · {formatChars(scene.chars)}
        </span>
      )}
      <span>
        {file ?? "ファイル未選択"}
        {file !== null && fileChars !== null && ` · ${formatChars(fileChars)}`}
      </span>
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
