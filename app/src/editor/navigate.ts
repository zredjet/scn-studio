// 定義へ移る（T1-8、D047）：F12 と Mod-クリック。シーンID（`-> s013`、選択肢の `->`、`@呼出` の引数）から
// そのシーンへ、話者名から文脈パネルのそのキャラクターのカードへ（設定DBの画面ができるまで。T1-10）。
// どこへ移るかは Lezer の構文木で決める（見た目のための文法。移る先の解決はアプリがシーンの一覧で行う）。

import { syntaxTree } from "@codemirror/language";
import type { EditorState } from "@codemirror/state";
import type { SyntaxNode } from "@lezer/common";

export type Definition =
  | { readonly kind: "scene"; readonly id: string }
  | { readonly kind: "character"; readonly name: string };

function fromNode(state: EditorState, node: SyntaxNode): Definition | null {
  const text = state.sliceDoc(node.from, node.to).trim();
  switch (node.name) {
    case "SceneRef":
      return text === "" ? null : { kind: "scene", id: text };
    case "Speaker":
      return text === "" ? null : { kind: "character", name: text };
    case "Argument": {
      const directive = node.parent;
      const name = directive?.getChild("DirectiveName");
      const command =
        name === null || name === undefined
          ? ""
          : state.sliceDoc(name.from, name.to).trim();
      return command === "@呼出" || command === "@call"
        ? { kind: "scene", id: text }
        : null;
    }
    default:
      return null;
  }
}

/** その位置の定義先。無ければ null。テストから直接使う。 */
export function definitionAt(
  state: EditorState,
  pos: number,
): Definition | null {
  const tree = syntaxTree(state);
  for (const side of [1, -1] as const) {
    for (
      let node: SyntaxNode | null = tree.resolveInner(pos, side);
      node !== null;
      node = node.parent
    ) {
      const found = fromNode(state, node);
      if (found !== null) return found;
    }
  }
  return null;
}
