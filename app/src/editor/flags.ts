// エディタの印と判定（state.ts・writing.ts・lineid.ts で使う。互いを読み込まないよう、ここに分けた）。

import { Annotation } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";

/** 人の編集でない変更（保存で付いた行ID、外部の変更の読み込み直し）に付ける印。 */
export const programmatic = Annotation.define<boolean>();

/** 変換中か。`composing` は最初の変更が入るまで false なので、変換の始まりも見る。 */
export function isComposing(view: EditorView): boolean {
  return view.composing || view.compositionStarted;
}
