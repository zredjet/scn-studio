// 書き込まれる行の見せ方（名前の変更・スタブを足す・辺を足す。見本 12-overlays.html の 3、T4-20）。
// 行と場所はコアが求め（`rename_preview` の `places`、`stub_preview`・`flow_edge_preview`）、ここは文にするだけ。
// React と DOM に依存させない（テストする）。

import type { RenamePreview, WritePreview } from "../core/types.ts";

/** 行の番号を並べるときの数（それより多ければ「ほか N行」）。 */
export const LINES_SHOWN = 8;

/** 章のファイルの見せ方（`story/` を除く）。 */
export function storyFile(file: string): string {
  return file.replace(/^story\//, "");
}

/** 行の番号の並び（`41・43・71・77行`、多ければ `…・N行 ほか M行`）。 */
export function linesText(lines: readonly number[]): string {
  if (lines.length === 0) return "";
  const shown = lines.slice(0, LINES_SHOWN).join("・");
  const rest = lines.length - LINES_SHOWN;
  return rest > 0 ? `${shown}行 ほか ${rest}行` : `${shown}行`;
}

/** 名前の変更の変わる場所の1行。 */
export interface PlaceRow {
  readonly file: string;
  /** 何が変わるか（行の番号・設定・ファイル名）。 */
  readonly what: string;
  /** 変わる数（ファイル名の変更と設定は1）。 */
  readonly count: number;
}

/** 名前の変更の変わる場所（本文は章ごとの行、設定はファイル、ファイル名の変更は前と後）。 */
export function renamePlaces(preview: RenamePreview): PlaceRow[] {
  return [
    ...preview.places.map((p) => ({
      file: storyFile(p.file),
      what: linesText(p.lines),
      count: p.edits,
    })),
    ...preview.settings.map((file) => ({ file, what: "設定", count: 1 })),
    ...preview.moves.map((m) => ({
      file: m.from,
      what: `ファイル名 → ${m.to.split("/").pop() ?? m.to}`,
      count: 1,
    })),
  ];
}

/** 変わる場所の数（本文の置き換え、設定のファイル、ファイル名の変更）。 */
export function renameCount(preview: RenamePreview): number {
  return renamePlaces(preview).reduce((n, r) => n + r.count, 0);
}

/** 書き込む場所（`chapter01.scn 96行目から`）。 */
export function writtenAt(preview: WritePreview): string {
  const file = storyFile(preview.file);
  return preview.lines.length > 1
    ? `${file} ${preview.line}行目から`
    : `${file} ${preview.line}行目`;
}
