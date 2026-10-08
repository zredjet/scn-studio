// プレビュー中の行の印（T1-11、D050）：エディタの1行に背景の印を付ける。カーソルもフォーカスも動かさない
// （「エディタと同期」）。本文を直すと、印は変更に合わせてずれる。

import {
  type EditorState,
  type Extension,
  StateEffect,
  StateField,
} from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView } from "@codemirror/view";

/** 印を付ける行の位置（null で消す）。 */
export const setPreviewLine = StateEffect.define<number | null>();

const mark = Decoration.line({ class: "cm-preview-line" });

const previewLineField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(set, tr) {
    let next = set.map(tr.changes);
    for (const effect of tr.effects) {
      if (!effect.is(setPreviewLine)) continue;
      if (effect.value === null) {
        next = Decoration.none;
      } else {
        const pos = Math.min(Math.max(0, effect.value), tr.state.doc.length);
        next = Decoration.set([mark.range(tr.state.doc.lineAt(pos).from)]);
      }
    }
    return next;
  },
  provide: (field) => EditorView.decorations.from(field),
});

/** 印の拡張。 */
export function previewLine(): Extension {
  return previewLineField;
}

/** 印の付いた行の行頭（無ければ null）。テストに使う。 */
export function previewLineAt(state: EditorState): number | null {
  const set = state.field(previewLineField, false);
  if (set === undefined) return null;
  let found: number | null = null;
  set.between(0, state.doc.length, (from) => {
    found = from;
    return false;
  });
  return found;
}
