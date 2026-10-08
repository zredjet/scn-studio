// AIの指摘の行の印（T3-7、D068）：指摘のある行（行末の行IDで引く）に、緑青の薄い地と点の下線を付け、
// ツールチップに指摘を出す。見た目だけを変え、本文は変えない。変換中は作り直さず、前の印をずらすだけにする。

import { type Extension, Facet, RangeSetBuilder } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  type EditorView,
  ViewPlugin,
  type ViewUpdate,
} from "@codemirror/view";
import { isComposing } from "./flags.ts";
import { trailingLineId } from "./provenance.ts";

/** 行IDから、その行の指摘の文。 */
export const aiFindingLines = Facet.define<
  ReadonlyMap<string, string>,
  ReadonlyMap<string, string>
>({
  combine: (values) => values[0] ?? new Map(),
});

function build(view: EditorView): DecorationSet {
  const findings = view.state.facet(aiFindingLines);
  const builder = new RangeSetBuilder<Decoration>();
  if (findings.size === 0) return builder.finish();
  for (const { from, to } of view.visibleRanges) {
    let pos = from;
    while (pos <= to) {
      const line = view.state.doc.lineAt(pos);
      const id = trailingLineId(line.text);
      const message = id === undefined ? undefined : findings.get(id);
      if (message !== undefined)
        builder.add(
          line.from,
          line.from,
          Decoration.line({
            class: "cm-ai-finding-line",
            attributes: { title: `AIの指摘\n${message}` },
          }),
        );
      pos = line.to + 1;
    }
  }
  return builder.finish();
}

/** AIの指摘の行の印を付ける拡張。 */
export function aiFindingMarks(): Extension {
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
          update.startState.facet(aiFindingLines) !==
            update.state.facet(aiFindingLines);
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
