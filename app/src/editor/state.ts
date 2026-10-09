// エディタの状態（CodeMirror 6）の作り方。React を使わない部分で、テストから直接使う。
//
// 本文を黙って変えない：Enter で字下げや全角空白を消さない。改行の `\r\n` を `\n` に変えない。
// 改行の種類が混ざった本文だけは、エディタが1種類しか持てないので、多いほうにそろえて読み込み、
// 画面で知らせる（保存するとそろう。D045）。
// 補完と括弧の自動補完は入れない（basicSetup を使わず、拡張を1つずつ選ぶ。.claude/rules/frontend.md）。
// 表示は2つ：執筆表示（writing.ts。記号を畳む）とソース表示（生のテキストと行番号）。Mod-/ で切り替える（D046）。

import {
  defaultKeymap,
  history,
  historyKeymap,
  insertNewline,
} from "@codemirror/commands";
import type { Diagnostic } from "@codemirror/lint";
import { search, searchKeymap } from "@codemirror/search";
import {
  type ChangeSet,
  type Compartment,
  EditorState,
  type Extension,
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
import type { Bracket, IrDiagnostic, ViewMode } from "../core/types.ts";
import { programmatic } from "./flags.ts";
import { lineIdGuard } from "./lineid.ts";
import { halfwidthOf } from "./quickfix.ts";
import { sceneTitles, writingView } from "./writing.ts";

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
      ...(d.code === "W406" ? { actions: W406_ACTIONS } : {}),
    };
  });
}

/**
 * W406（行頭の全角記号）のクイックフィックス（T2-8、D059）：半角にする、地の文のままにする（行頭に `\`）。
 * 変換中は何もしない。
 */
const W406_ACTIONS: Diagnostic["actions"] = [
  {
    name: "半角にする",
    apply(view: EditorView, from: number, to: number) {
      if (view.composing) return;
      const line = view.state.doc.lineAt(to);
      const half = halfwidthOf(
        view.state.sliceDoc(from, to),
        view.state.sliceDoc(to, Math.min(to + 1, line.to)),
      );
      if (half === null) return;
      view.dispatch({
        changes: { from, to: to + half.consume, insert: half.insert },
      });
    },
  },
  {
    name: "地の文のままにする",
    apply(view: EditorView, from: number) {
      if (view.composing) return;
      view.dispatch({ changes: { from, insert: "\\" } });
    },
  },
];

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
  // 補完の窓（T1-8）。選ばれている行は、一覧の選択と同じ色（docs/ui/README.md）。基本テーマより詳細度を上げる。
  "&.cm-editor .cm-tooltip.cm-tooltip-autocomplete": {
    border: "1px solid var(--border)",
    borderRadius: "var(--radius-item)",
    backgroundColor: "var(--paper)",
    overflow: "hidden",
  },
  "&.cm-editor .cm-tooltip.cm-tooltip-autocomplete > ul > li": {
    padding: "4px 10px",
    fontFamily: "var(--font-ui)",
    fontSize: "13px",
    lineHeight: "1.6",
  },
  "&.cm-editor .cm-tooltip.cm-tooltip-autocomplete > ul > li[aria-selected]": {
    backgroundColor: "var(--accent-tint)",
    color: "var(--accent-strong)",
  },
  // 検索と置換の欄（T4-14、U9）。基本テーマより詳細度を上げる。
  "&.cm-editor .cm-panels": {
    backgroundColor: "var(--panel-bg)",
    color: "var(--ink)",
    borderColor: "var(--border)",
    fontFamily: "var(--font-ui)",
    fontSize: "12px",
  },
  "&.cm-editor .cm-panel.cm-search": { padding: "6px 10px" },
  "&.cm-editor .cm-panel.cm-search label": { fontSize: "12px" },
  "&.cm-editor .cm-textfield": {
    border: "1px solid var(--border-control)",
    borderRadius: "var(--radius-control)",
    backgroundColor: "var(--paper)",
    padding: "2px 6px",
    fontSize: "13px",
  },
  "&.cm-editor .cm-button": {
    border: "1px solid var(--border-control)",
    borderRadius: "var(--radius-control)",
    backgroundColor: "var(--paper)",
    backgroundImage: "none",
    color: "var(--ink)",
    fontSize: "12px",
  },
  "&.cm-editor .cm-searchMatch": {
    backgroundColor: "var(--warn-tint)",
    outline: "1px solid var(--warn-border)",
  },
  "&.cm-editor .cm-searchMatch-selected": {
    backgroundColor: "var(--accent-tint)",
  },
  ".cm-completionDetail": {
    marginLeft: "12px",
    fontStyle: "normal",
    fontSize: "12px",
    color: "var(--text-faint)",
  },
  // フォーカス中の選択は、CodeMirror の基本テーマ（クラス5つ）より詳細度を上げないと、基本テーマの色になる。
  ".cm-selectionBackground, &.cm-editor.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground":
    { backgroundColor: "var(--accent-tint)" },
};

const theme = EditorView.theme(editorThemeSpec);

/** 貼り付けやドロップで入る文字列の改行を、本文の改行（`\r\n` など）にそろえる。 */
export function matchLineBreaks(text: string, lineBreak: string): string {
  return text.replace(/\r\n?|\n/g, lineBreak);
}

/**
 * キーの割り当て。Enter は改行を入れるだけにする。既定の insertNewlineAndIndent は、
 * カーソルの前後の空白（全角空白を含む）を消してしまう。Mod-/ は表示の切り替えに使うので、
 * 既定のコメントの切り替え（toggleComment）は外す（D046）。
 */
export const editorKeymap = [
  { key: "Enter", run: insertNewline },
  ...defaultKeymap.filter((binding) => binding.key !== "Mod-/"),
  ...historyKeymap,
  // 検索と置換（Mod-F など。T4-14、U9）。置き換えは行IDにかからない（lineid.ts）。
  ...searchKeymap,
];

/** 検索と置換の欄の文言（T4-14、U9）。 */
const SEARCH_PHRASES: Readonly<Record<string, string>> = {
  Find: "検索",
  Replace: "置き換え",
  next: "次へ",
  previous: "前へ",
  all: "すべて",
  "match case": "大文字と小文字を区別",
  regexp: "正規表現",
  "by word": "語として",
  replace: "置き換える",
  "replace all": "すべて置き換える",
  close: "閉じる",
  "current match": "今の一致",
  "replaced $ matches": "$ か所を置き換えました",
  "replaced match on line $": "$ 行目を置き換えました",
  "on line": "行目",
  "Go to line": "移る行",
  go: "移る",
};

export { isComposing, programmatic } from "./flags.ts";

export type { ViewMode };

/** 表示ごとの拡張。ソース表示は行番号を出す。執筆表示は writing.ts。 */
export function modeExtension(mode: ViewMode): Extension {
  return mode === "writing"
    ? writingView()
    : [lineNumbers(), highlightActiveLineGutter(), lineIdGuard("source")];
}

/** 表示の設定（エディタが切り替えに使う入れ物と、今の値）。 */
export interface ViewOptions {
  readonly mode: ViewMode;
  readonly modeCompartment: Compartment;
  /** シーンIDと題名（執筆表示で、ジャンプに題名を添える）。 */
  readonly titles: ReadonlyMap<string, string>;
  readonly titlesCompartment: Compartment;
  /** エディタが足す拡張（入力補助、補完、定義へ移る操作など。Editor.tsx）。 */
  readonly extra?: Extension;
}

export interface Hooks {
  /** 本文が変わった。`byUser` は人の編集か（`programmatic` の印の無い変更）。`changes` はその変更。 */
  readonly changed: (byUser: boolean, changes: ChangeSet) => void;
  /** 保存のキー（Ctrl+S／Cmd+S）が押された。 */
  readonly save: () => void;
  /** 変換が始まった。 */
  readonly compositionStarted: () => void;
  /** 変換が終わった。 */
  readonly compositionEnded: () => void;
  /** 表示の切り替えのキー（Ctrl+/／Cmd+/）が押された。 */
  readonly toggleView: () => void;
}

/**
 * エディタの状態を作る。改行の種類が混ざっていれば、多いほうにそろえる。`view` を省くとソース表示。
 * テストからも使う。
 */
export function createState(
  doc: string,
  brackets: readonly Bracket[],
  language: Compartment,
  hooks: Hooks,
  view?: ViewOptions,
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
      view === undefined
        ? modeExtension("source")
        : view.modeCompartment.of(modeExtension(view.mode)),
      view === undefined
        ? []
        : view.titlesCompartment.of(sceneTitles.of(view.titles)),
      view?.extra ?? [],
      history(),
      search({ top: true }),
      EditorState.phrases.of(SEARCH_PHRASES),
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
        {
          key: "Mod-/",
          preventDefault: true,
          run: () => {
            hooks.toggleView();
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
