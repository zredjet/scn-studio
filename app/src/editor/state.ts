// エディタの状態（CodeMirror 6）の作り方。React を使わない部分で、テストから直接使う。
//
// 本文を黙って変えない：Enter で字下げや全角空白を消さない。改行の `\r\n` を `\n` に変えない。
// 補完と括弧の自動補完は入れない（basicSetup を使わず、拡張を1つずつ選ぶ。.claude/rules/frontend.md）。

import {
  defaultKeymap,
  history,
  historyKeymap,
  insertNewline,
} from "@codemirror/commands";
import type { Diagnostic } from "@codemirror/lint";
import { type Compartment, EditorState } from "@codemirror/state";
import {
  drawSelection,
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  keymap,
  lineNumbers,
} from "@codemirror/view";
import { scn, scnSyntaxHighlighting } from "@scn-studio/lezer-scn";
import { PositionMap } from "../core/position.ts";
import type { Bracket, IrDiagnostic } from "../core/types.ts";

/**
 * 改行がすべて `\r\n` の本文か。そのときは、エディタも `\r\n` で行を分け、つなぐ。
 * 本文を取り出すときは `doc.toString()`（いつも `\n` でつなぐ）ではなく `state.sliceDoc()` を使う。
 */
export function usesCrlf(text: string): boolean {
  return text.includes("\r\n") && !/(^|[^\r])\n/.test(text);
}

/** コアの診断を、CodeMirrorの診断にする（UTF-8のバイト位置から、エディタの位置へ）。 */
export function toEditorDiagnostics(
  text: string,
  diagnostics: readonly IrDiagnostic[],
): Diagnostic[] {
  const map = new PositionMap(text);
  return diagnostics.map((d) => {
    const from = map.byteToUtf16(d.span[0]);
    const to = Math.max(from, map.byteToUtf16(d.span[1]));
    return {
      from,
      to,
      severity: d.severity,
      message: `${d.code} ${d.message}`,
    };
  });
}

/** 変換中か。`composing` は最初の変更が入るまで false なので、変換の始まりも見る。 */
export function isComposing(view: EditorView): boolean {
  return view.composing || view.compositionStarted;
}

/** エディタの見た目。テストで中身を確かめるので、書き出す。 */
export const editorThemeSpec = {
  "&": {
    height: "100%",
    backgroundColor: "var(--panel-bg-2)",
    color: "var(--ink)",
  },
  ".cm-scroller": {
    fontFamily: "var(--font-mono)",
    fontSize: "13.5px",
    lineHeight: "1.9",
  },
  ".cm-content": { padding: "16px 0" },
  ".cm-gutters": {
    backgroundColor: "var(--panel-bg-2)",
    color: "var(--line-number)",
    border: "none",
  },
  // 選択の色（drawSelection）は本文の後ろの層に描かれるので、本文の行の背景は透けさせる。
  // 不透明にすると、1行の中の選択（Shift+←など）が隠れて見えなくなる。
  ".cm-activeLine": {
    backgroundColor:
      "color-mix(in srgb, var(--accent-tint-2) 60%, transparent)",
  },
  ".cm-activeLineGutter": { backgroundColor: "var(--accent-tint-2)" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground": {
    backgroundColor: "var(--accent-tint)",
  },
};

const theme = EditorView.theme(editorThemeSpec);

/** 貼り付けやドロップで入る文字列の改行を、本文の改行（`\r\n` など）にそろえる。 */
export function matchLineBreaks(text: string, lineBreak: string): string {
  return text.replace(/\r\n?|\n/g, lineBreak);
}

/**
 * キーの割り当て。Enter は改行を入れるだけにする。既定の insertNewlineAndIndent は、
 * カーソルの前後の空白（全角空白を含む）を消してしまう。
 */
export const editorKeymap = [
  { key: "Enter", run: insertNewline },
  ...defaultKeymap,
  ...historyKeymap,
];

export interface Hooks {
  /** 本文が変わった。 */
  readonly changed: () => void;
  /** 変換が始まった。 */
  readonly compositionStarted: () => void;
  /** 変換が終わった。 */
  readonly compositionEnded: () => void;
}

/** エディタの状態を作る。テストからも使う。 */
export function createState(
  doc: string,
  brackets: readonly Bracket[],
  language: Compartment,
  hooks: Hooks,
): EditorState {
  return EditorState.create({
    doc,
    extensions: [
      usesCrlf(doc)
        ? [
            EditorState.lineSeparator.of("\r\n"),
            // \r\n で行を分けるので、貼り付けた \n も \r\n にそろえる（行の中に \n を入れない）。
            EditorView.clipboardInputFilter.of((text, state) =>
              matchLineBreaks(text, state.lineBreak),
            ),
          ]
        : [],
      lineNumbers(),
      highlightActiveLineGutter(),
      history(),
      drawSelection(),
      highlightActiveLine(),
      EditorView.lineWrapping,
      keymap.of(editorKeymap),
      language.of(scn({ brackets })),
      scnSyntaxHighlighting,
      theme,
      EditorView.updateListener.of((update) => {
        if (update.docChanged) hooks.changed();
      }),
      EditorView.domEventHandlers({
        compositionstart: () => {
          hooks.compositionStarted();
        },
        compositionend: () => {
          hooks.compositionEnded();
        },
      }),
    ],
  });
}
