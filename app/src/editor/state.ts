// エディタの状態（CodeMirror 6）の作り方。React を使わない部分で、テストから直接使う。
//
// 本文を黙って変えない：Enter で字下げや全角空白を消さない。改行の `\r\n` を `\n` に変えない。
// 改行の種類が混ざった本文だけは、エディタが1種類しか持てないので、多いほうにそろえて読み込み、
// 画面で知らせる（保存するとそろう。D045）。
// 補完と括弧の自動補完は入れない（basicSetup を使わず、拡張を1つずつ選ぶ。.claude/rules/frontend.md）。

import {
  defaultKeymap,
  history,
  historyKeymap,
  insertNewline,
} from "@codemirror/commands";
import type { Diagnostic } from "@codemirror/lint";
import {
  Annotation,
  type ChangeSet,
  type Compartment,
  EditorState,
} from "@codemirror/state";
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

/** 改行の数（`\r\n`、`\n` だけ、`\r` だけ）。 */
export function countLineBreaks(text: string): {
  crlf: number;
  lf: number;
  cr: number;
} {
  let crlf = 0;
  let lf = 0;
  let cr = 0;
  for (let i = 0; i < text.length; i++) {
    const code = text.charCodeAt(i);
    if (code === 0x0a) lf += 1;
    else if (code === 0x0d) {
      if (text.charCodeAt(i + 1) === 0x0a) {
        crlf += 1;
        i += 1;
      } else cr += 1;
    }
  }
  return { crlf, lf, cr };
}

/**
 * エディタで使う改行。`\r\n` が `\n` より多ければ `\r\n`（エディタも `\r\n` で行を分け、つなぐ）。
 * 本文を取り出すときは `doc.toString()`（いつも `\n` でつなぐ）ではなく `state.sliceDoc()` を使う。
 */
export function lineBreakOf(text: string): "\r\n" | "\n" {
  const { crlf, lf } = countLineBreaks(text);
  return crlf > lf ? "\r\n" : "\n";
}

/** 改行の種類が混ざっているか（`\r\n` と `\n`、または `\r` だけの改行がある）。 */
export function hasMixedLineBreaks(text: string): boolean {
  const { crlf, lf, cr } = countLineBreaks(text);
  return cr > 0 || (crlf > 0 && lf > 0);
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

/** 人の編集でない変更（保存で付いた行ID、外部の変更の読み込み直し）に付ける印。 */
export const programmatic = Annotation.define<boolean>();

export interface Hooks {
  /** 本文が変わった。`byUser` は人の編集か（`programmatic` の印の無い変更）。`changes` はその変更。 */
  readonly changed: (byUser: boolean, changes: ChangeSet) => void;
  /** 保存のキー（Ctrl+S／Cmd+S）が押された。 */
  readonly save: () => void;
  /** 変換が始まった。 */
  readonly compositionStarted: () => void;
  /** 変換が終わった。 */
  readonly compositionEnded: () => void;
}

/** エディタの状態を作る。改行の種類が混ざっていれば、多いほうにそろえる。テストからも使う。 */
export function createState(
  doc: string,
  brackets: readonly Bracket[],
  language: Compartment,
  hooks: Hooks,
): EditorState {
  const lineBreak = lineBreakOf(doc);
  return EditorState.create({
    doc: matchLineBreaks(doc, lineBreak),
    extensions: [
      lineBreak === "\r\n"
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
      keymap.of([
        {
          key: "Mod-s",
          preventDefault: true,
          run: () => {
            hooks.save();
            return true;
          },
        },
        ...editorKeymap,
      ]),
      language.of(scn({ brackets })),
      scnSyntaxHighlighting,
      theme,
      EditorView.updateListener.of((update) => {
        if (!update.docChanged) return;
        const byUser = update.transactions.some(
          (tr) => tr.docChanged && tr.annotation(programmatic) !== true,
        );
        hooks.changed(byUser, update.changes);
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
