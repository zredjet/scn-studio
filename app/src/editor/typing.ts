// 打った後の入力補助（T1-8、D047）：括弧の補い、三点リーダとダッシュの正規化、段落頭の字下げ、
// 補完の窓を開く。
//
// どれも、変換を確定した後か、変換を使わずに打った後、または Enter のときにだけ動く。変換中は何もしない
// （.claude/rules/frontend.md）。正規化と字下げは `project.toml` の `[editor]` で入れたときだけ動く
// （無ければ切。本文を黙って変えない）。

import { startCompletion } from "@codemirror/autocomplete";
import { syntaxTree } from "@codemirror/language";
import type {
  ChangeSpec,
  EditorState,
  Extension,
  TransactionSpec,
} from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import type { Bracket, EditorSettings } from "../core/types.ts";
import { assistEnv } from "./complete.ts";
import { isComposing } from "./flags.ts";

/** 入力補助の設定。 */
export interface TypingOptions {
  readonly brackets: readonly Bracket[];
  readonly editor: EditorSettings;
}

/**
 * 確定した文字の正規化（`normalize_ellipsis`）：`...`・`・・・`・単独の `…` を `……` に、`--`・`－－` を
 * `――` にする。`prev` は確定した文字の直前の1文字（`…` なら、続けて打っているので単独とみなさない）。
 */
export function normalizeInserted(text: string, prev: string): string {
  const replaced = text
    .replace(/\.\.\.|・・・/g, "……")
    .replace(/--|－－/g, "――");
  return replaced.replace(/…+/g, (run, offset: number) =>
    run.length === 1 && !(offset === 0 && prev === "…") ? "……" : run,
  );
}

/** 1文字ずつ打って、直前の文字と合わせてできる並び（変換を使わずに打ったとき）。 */
const TYPED_RUNS: readonly (readonly [string, string])[] = [
  ["...", "……"],
  ["・・・", "……"],
  ["--", "――"],
  ["－－", "――"],
];

/**
 * 括弧を補う：確定した文字（`inserted`）に、閉じていない開き括弧があれば、閉じ括弧を返す（入れ子なら
 * 内側から）。カーソルの後ろがもうその閉じ括弧で始まっていれば足さない。
 */
export function closingFor(
  inserted: string,
  after: string,
  brackets: readonly Bracket[],
): string {
  const stack: string[] = [];
  for (const ch of inserted) {
    const open = brackets.find((b) => b.open === ch);
    if (open !== undefined) {
      stack.push(open.close);
      continue;
    }
    if (stack[stack.length - 1] === ch) stack.pop();
  }
  const closes = stack.reverse().join("");
  return closes !== "" && after.startsWith(closes) ? "" : closes;
}

/**
 * 閉じ括弧を打ったら、すぐ後ろの同じ閉じ括弧（補ったもの）に重ねる（2つにしない）。確定した文字の末尾の、
 * 確定した文字の中に対応する開き括弧の無い閉じ括弧（`やあ」` の `」`）が、後ろの文字の頭と同じなら、
 * 後ろから消す文字数を返す。
 */
export function overtypeLength(
  inserted: string,
  after: string,
  brackets: readonly Bracket[],
): number {
  const chars = Array.from(inserted);
  const stack: string[] = [];
  const unmatched = chars.map((ch) => {
    const open = brackets.find((b) => b.open === ch);
    if (open !== undefined) {
      stack.push(open.close);
      return false;
    }
    if (stack[stack.length - 1] === ch) {
      stack.pop();
      return false;
    }
    return brackets.some((b) => b.close === ch);
  });
  let n = chars.length;
  while (n > 0 && unmatched[n - 1] === true) n -= 1;
  const suffix = chars.slice(n).join("");
  return suffix !== "" && after.startsWith(suffix) ? suffix.length : 0;
}

/**
 * 括弧を補ってよいか：カーソルの後ろが、空・空白・閉じ括弧・行IDのとき（文の途中では補わない。
 * CodeMirror の closeBrackets と同じ考え）。
 */
function closableBefore(after: string, brackets: readonly Bracket[]): boolean {
  return (
    after.trim() === "" ||
    brackets.some((b) => after.startsWith(b.close)) ||
    /^ +\^[0-9A-Za-z]+[ \t]*$/.test(after)
  );
}

/**
 * 正規化してよい位置か：本文（地の文、台詞、選択肢の表示文、`@選択` の問いかけ文）だけ。メタ情報、
 * `@raw` の中身、コメント、ほかのディレクティブ、見出しでは動かさない。行の中身が `-` だけ（メタ情報の
 * 区切り `---` を打っている途中）のときも動かさない（D047）。
 */
export function normalizableAt(state: EditorState, pos: number): boolean {
  const line = state.doc.lineAt(pos);
  if (/^[ \t]*[-－]+[ \t]*$/.test(line.text)) return false;
  const node = syntaxTree(state).topNode.childAfter(line.from);
  if (node === null || node.from > line.to) return false;
  switch (node.name) {
    case "Narration":
    case "Dialogue":
    case "ChoiceOption":
      return true;
    case "Directive": {
      const name = node.getChild("DirectiveName");
      if (name === null) return false;
      const text = state.sliceDoc(name.from, name.to).trim();
      return (text === "@選択" || text === "@choice") && pos > name.to;
    }
    default:
      return false;
  }
}

/**
 * 確定した文字（`[from, to)`）の後の入力補助（正規化と括弧の補い）を、1つの変更にする。変換中なら、
 * 何もしない（null）。`complete` は、補完の窓を開いてよいか。テストから直接使う。
 */
export function afterCommit(
  state: EditorState,
  from: number,
  to: number,
  options: TypingOptions,
  composing: boolean,
): { spec: TransactionSpec | null; complete: boolean } | null {
  if (composing || from > to) return null;
  const inserted = state.sliceDoc(from, to);
  const prev = from > 0 ? state.sliceDoc(from - 1, from) : "";
  const line = state.doc.lineAt(to);
  const after = state.sliceDoc(to, line.to);
  const normalize =
    options.editor.normalize_ellipsis && normalizableAt(state, to);
  let start = from;
  let text = normalize ? normalizeInserted(inserted, prev) : inserted;
  // 1文字打って、直前の文字と合わせて `...` や `--` になったら、それも置き換える。
  if (normalize && inserted.length === 1) {
    for (const [run, replaced] of TYPED_RUNS) {
      const begin = to - run.length;
      if (begin >= line.from && state.sliceDoc(begin, to) === run) {
        start = begin;
        text = replaced;
        break;
      }
    }
  }
  const changes: ChangeSpec[] = [];
  if (start !== from || text !== inserted) {
    changes.push({ from: start, to, insert: text });
  }
  const anchor = start + text.length;
  const overtype = overtypeLength(inserted, after, options.brackets);
  if (overtype > 0) {
    changes.push({ from: to, to: to + overtype });
  } else if (closableBefore(after, options.brackets)) {
    const closes = closingFor(text, after, options.brackets);
    if (closes !== "") changes.push({ from: to, insert: closes });
  }
  const spec: TransactionSpec | null =
    changes.length === 0
      ? null
      : { changes, selection: { anchor }, userEvent: "input.assist" };
  return { spec, complete: true };
}

/**
 * Enter で行を離れるとき、その行が地の文で、段落頭に全角空白が無ければ足す変更（`indent_narration`）。
 * 足さないなら null。行頭の半角空白とタブ（整形用）の後ろに足す。`\` で始まる行には足さない
 * （`\` が行頭でなくなると、エスケープでなくなるため）。カーソルが中身の頭より前なら足さない
 * （行の頭の Enter は、上に行を足す操作なので）。地の文かどうかは Lezer の構文木で見る（控えめに判断し、
 * 台詞の形の行には足さない）。
 */
export function narrationIndent(
  state: EditorState,
  pos: number,
): ChangeSpec | null {
  const line = state.doc.lineAt(pos);
  const node = syntaxTree(state).topNode.childAfter(line.from);
  if (node === null || node.name !== "Narration" || node.from < line.from) {
    return null;
  }
  if (node.from > line.to) return null;
  const indent = /^[ \t]*/.exec(line.text)?.[0].length ?? 0;
  const rest = line.text.slice(indent);
  if (rest === "" || /^[　\\]/.test(rest)) return null;
  if (pos <= line.from + indent) return null;
  return { from: line.from + indent, insert: "　" };
}

/** Enter：地の文の段落頭に全角空白を足してから改行する（設定で入れたときだけ）。変換中は何もしない。 */
export function enterWithIndent(options: () => TypingOptions) {
  return (view: EditorView): boolean => {
    if (isComposing(view) || !options().editor.indent_narration) return false;
    const { state } = view;
    const range = state.selection.main;
    if (state.selection.ranges.length !== 1 || !range.empty) return false;
    const indent = narrationIndent(state, range.head);
    if (indent === null) return false;
    view.dispatch({
      changes: [indent, { from: range.head, insert: state.lineBreak }],
      selection: { anchor: range.head + 2 },
      userEvent: "input",
      scrollIntoView: true,
    });
    return true;
  };
}

/** 開き括弧と閉じ括弧の間（中が空）で Backspace を押したら、両方を消す。変換中は何もしない。 */
export function deleteBracketPair(options: () => TypingOptions) {
  return (view: EditorView): boolean => {
    if (isComposing(view)) return false;
    const { state } = view;
    const range = state.selection.main;
    if (state.selection.ranges.length !== 1 || !range.empty) return false;
    const pos = range.head;
    const before = pos > 0 ? state.sliceDoc(pos - 1, pos) : "";
    const after = state.sliceDoc(pos, pos + 1);
    const pair = options().brackets.some(
      (b) => b.open === before && b.close === after,
    );
    if (!pair) return false;
    view.dispatch({
      changes: { from: pos - 1, to: pos + 1 },
      userEvent: "delete.backward",
      scrollIntoView: true,
    });
    return true;
  };
}

/** 確定の後の入力補助を行い、補完の窓を開く。変換中は何もしない。 */
export function runAfterCommit(
  view: EditorView,
  from: number,
  to: number,
  options: TypingOptions,
): void {
  const result = afterCommit(view.state, from, to, options, isComposing(view));
  if (result === null) return;
  if (result.spec !== null) view.dispatch(result.spec);
  if (result.complete && view.state.facet(assistEnv).data !== null) {
    startCompletion(view);
  }
}

/**
 * 変換を使わずに打った文字を受け取り、入力補助を行う（変換中は CodeMirror に任せる）。Enter と Backspace の
 * 割り当てもここに置く。
 */
export function typingAssist(options: () => TypingOptions): Extension {
  return [
    EditorView.inputHandler.of((view, _from, _to, text, insert) => {
      if (isComposing(view)) return false;
      view.dispatch(insert());
      // 入れた文字は、カーソルのすぐ前にある。
      const head = view.state.selection.main.head;
      runAfterCommit(view, Math.max(0, head - text.length), head, options());
      return true;
    }),
    keymap.of([
      { key: "Enter", run: enterWithIndent(options) },
      { key: "Backspace", run: deleteBracketPair(options) },
    ]),
  ];
}
