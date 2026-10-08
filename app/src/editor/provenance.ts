// AIが書いた行の印（T3-4、D065）：出自の記録（`ai/provenance.jsonl`）で今の出自が `ai`・`ai-edited` の行に、
// 行の左に緑青の線を付ける。行IDで引くので、行IDのある行（複数行の台詞は最後の行）だけ。見た目だけを変え、
// 本文は変えない。変換中は作り直さず、前の印を変更に合わせてずらすだけにする。

import {
  type EditorState,
  type Extension,
  Facet,
  RangeSetBuilder,
} from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  type EditorView,
  ViewPlugin,
  type ViewUpdate,
} from "@codemirror/view";
import { isComposing } from "./flags.ts";

/** 行IDから、今の出自（`ai`・`ai-edited`）。 */
export const aiLines = Facet.define<
  ReadonlyMap<string, string>,
  ReadonlyMap<string, string>
>({
  combine: (values) => values[0] ?? new Map(),
});

const LINE_ID = / \^([0-9a-z]{4,12})[ \t]*$/;

/** 行の出自の印の種類（印を付けない行は null）。 */
export function lineOrigin(
  line: string,
  origins: ReadonlyMap<string, string>,
): "ai" | "ai-edited" | null {
  const id = LINE_ID.exec(line)?.[1];
  if (id === undefined) return null;
  const origin = origins.get(id);
  return origin === "ai" || origin === "ai-edited" ? origin : null;
}

const marks = {
  ai: Decoration.line({
    class: "cm-ai-line",
    attributes: { title: "AIが書いた行" },
  }),
  "ai-edited": Decoration.line({
    class: "cm-ai-edited-line",
    attributes: { title: "AIが書いた行を人が直した" },
  }),
};

function build(view: EditorView): DecorationSet {
  const state: EditorState = view.state;
  const origins = state.facet(aiLines);
  const builder = new RangeSetBuilder<Decoration>();
  if (origins.size === 0) return builder.finish();
  for (const { from, to } of view.visibleRanges) {
    let pos = from;
    while (pos <= to) {
      const line = state.doc.lineAt(pos);
      const origin = lineOrigin(line.text, origins);
      if (origin !== null) builder.add(line.from, line.from, marks[origin]);
      pos = line.to + 1;
    }
  }
  return builder.finish();
}

/** AIが書いた行の印を付ける拡張。 */
export function aiLineMarks(): Extension {
  return ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      /** 変換中に作り直しを見送った（変換が終わってから作り直す）。 */
      stale = false;
      constructor(view: EditorView) {
        this.decorations = build(view);
      }
      update(update: ViewUpdate) {
        const changed =
          update.docChanged ||
          update.viewportChanged ||
          update.startState.facet(aiLines) !== update.state.facet(aiLines);
        if (isComposing(update.view)) {
          this.decorations = this.decorations.map(update.changes);
          if (changed) this.stale = true;
          return;
        }
        if (changed || this.stale) {
          this.stale = false;
          this.decorations = build(update.view);
        }
      }
    },
    { decorations: (v) => v.decorations },
  );
}
