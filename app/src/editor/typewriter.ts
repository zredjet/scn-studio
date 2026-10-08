// タイプライタースクロール（T2-8、D059）：打つ・動かすたびに、カーソルの行を画面の縦の真ん中に置く。
// 変換中は動かさない（.claude/rules/frontend.md）。見た目だけを変え、本文は変えない。

import type { Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";

/** 人の操作（入力、削除、カーソルの移動、選択）で、カーソルが動いたか。 */
const USER_EVENTS = ["input", "delete", "select", "move", "undo", "redo"];

/** カーソルの行を縦の真ん中に寄せる（次の描画で。変換中なら寄せない）。 */
function center(view: EditorView): void {
  requestAnimationFrame(() => {
    if (view.composing) return;
    view.dispatch({
      effects: EditorView.scrollIntoView(view.state.selection.main.head, {
        y: "center",
      }),
    });
  });
}

export function typewriterScroll(): Extension {
  return [
    // 確定の直後の更新は、まだ変換中の扱いで来ることがあるので、変換が終わったときにも寄せる。
    EditorView.domEventHandlers({
      compositionend: (_event, view) => {
        center(view);
        return false;
      },
    }),
    EditorView.updateListener.of((update) => {
      if (!update.selectionSet && !update.docChanged) return;
      const view = update.view;
      if (view.composing) return;
      const byUser = update.transactions.some((tr) =>
        USER_EVENTS.some((e) => tr.isUserEvent(e)),
      );
      if (!byUser) return;
      // 更新の途中では dispatch できないので、次の描画で寄せる。
      center(view);
    }),
  ];
}
