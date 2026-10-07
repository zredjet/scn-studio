// エディタの本文の扱いのテスト（CodeMirrorの状態を使う。画面は出さない）。

import assert from "node:assert/strict";
import { test } from "node:test";
import { insertNewline } from "@codemirror/commands";
import { Compartment, EditorSelection, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { PositionMap } from "../src/core/position.ts";
import {
  countLineBreaks,
  createState,
  editorKeymap,
  editorThemeSpec,
  hasMixedLineBreaks,
  lineBreakOf,
  matchLineBreaks,
  toEditorDiagnostics,
} from "../src/editor/state.ts";

const BRACKETS = [{ open: "「", close: "」" }];
const HOOKS = {
  changed: () => {},
  save: () => {},
  compositionStarted: () => {},
  compositionEnded: () => {},
  toggleView: () => {},
};

function state(doc: string): EditorState {
  return createState(doc, BRACKETS, new Compartment(), HOOKS);
}

/** Enter に割り当てた命令を、カーソルの位置で実行する。 */
function pressEnter(doc: string, cursor: number): string {
  let current = state(doc);
  current = current.update({ selection: EditorSelection.cursor(cursor) }).state;
  let result = current;
  insertNewline({
    state: current,
    dispatch: (tr) => {
      result = tr.state;
    },
  });
  return result.sliceDoc();
}

test("Enter で、字下げの全角空白を消さない", () => {
  assert.equal(pressEnter("　本文です。", 0), "\n　本文です。");
  assert.equal(pressEnter("　", 1), "　\n");
  assert.equal(pressEnter("  灯「やあ」", 2), "  \n灯「やあ」");
});

test("改行が CRLF の本文は、そのまま保つ。診断の位置も合う", () => {
  const text = "## a #s1\r\n灯「やあ」 ^k3f9\r\n😀\r\n";
  assert.equal(lineBreakOf(text), "\r\n");
  assert.ok(!hasMixedLineBreaks(text));
  const s = state(text);
  assert.equal(s.sliceDoc(), text);
  const map = new PositionMap(text);
  assert.equal(map.utf16Length, s.doc.length);
  const start = Buffer.byteLength(text.slice(0, text.indexOf("^k3f9")), "utf8");
  const [diagnostic] = toEditorDiagnostics(text, [
    {
      code: "W303",
      severity: "warning",
      file: "a.scn",
      span: [start, start + 5],
      message: "",
    },
  ]);
  assert.equal(s.sliceDoc(diagnostic?.from, diagnostic?.to), "^k3f9");
  // 各行の先頭の位置は、本文のバイト位置と行き来できる。
  for (let n = 1; n <= s.doc.lines; n++) {
    const line = s.doc.line(n);
    const byte = map.utf16ToByte(line.from);
    assert.equal(map.byteToUtf16(byte), line.from);
    assert.ok(
      Buffer.from(text, "utf8")
        .subarray(byte)
        .toString("utf8")
        .startsWith(line.text),
    );
  }
});

test("改行が LF の本文と、混ざった本文", () => {
  assert.equal(lineBreakOf("a\nb\n"), "\n");
  assert.equal(lineBreakOf("a"), "\n");
  assert.deepEqual(countLineBreaks("a\r\nb\nc\rd\r\n"), {
    crlf: 2,
    lf: 1,
    cr: 1,
  });
  assert.ok(!hasMixedLineBreaks("a\nb\n"));
  assert.ok(hasMixedLineBreaks("a\r\nb\n"));
  assert.ok(hasMixedLineBreaks("a\rb"));
  const lf = state("灯「a」\n😀\n");
  assert.equal(lf.sliceDoc(), "灯「a」\n😀\n");
  assert.equal(new PositionMap("灯「a」\n😀\n").utf16Length, lf.doc.length);
  // 混ざった本文でも、エディタの位置の数え方（\r\n を1つ）は CodeMirror と同じ。
  const mixed = "a\r\nb\nc";
  assert.equal(
    new PositionMap(mixed).utf16Length,
    EditorState.create({ doc: mixed }).doc.length,
  );
});

test("改行の種類が混ざった本文は、多いほうにそろえて読み込む。位置は変わらない", () => {
  // \r\n が多い：\r\n にそろえる（Windowsで、CRLFのファイルにLFの行を足した場合）。
  const mostlyCrlf = "## a #s1\r\n灯「a」\r\n足した行\n😀\r\n";
  assert.equal(lineBreakOf(mostlyCrlf), "\r\n");
  const crlf = state(mostlyCrlf);
  assert.equal(crlf.lineBreak, "\r\n");
  assert.equal(crlf.sliceDoc(), "## a #s1\r\n灯「a」\r\n足した行\r\n😀\r\n");
  assert.equal(crlf.doc.lines, 5);
  // \n が多いか同じ数：\n にそろえる。\r だけの改行も \n にする。
  const mostlyLf = "a\nb\r\nc\rd\n";
  const lf = state(mostlyLf);
  assert.equal(lf.lineBreak, "\n");
  assert.equal(lf.sliceDoc(), "a\nb\nc\nd\n");
  // エディタの位置（改行は1つ）は、そろえる前の本文の位置と同じなので、診断の位置もそのまま使える。
  for (const text of [mostlyCrlf, mostlyLf]) {
    assert.equal(new PositionMap(text).utf16Length, state(text).doc.length);
  }
});

test("Enter の割り当ては、改行を入れるだけの insertNewline", () => {
  const enter = editorKeymap.find((binding) => binding.key === "Enter");
  assert.equal(enter?.run, insertNewline);
});

test("CRLF の本文に貼り付けた LF の文字列は、CRLF にそろえる", () => {
  assert.equal(matchLineBreaks("x\ny\r\nz\rw", "\r\n"), "x\r\ny\r\nz\r\nw");
  const crlf = state("a\r\nb\r\n");
  const filters = crlf.facet(EditorView.clipboardInputFilter);
  assert.equal(filters.length, 1);
  const pasted = filters.reduce((text, filter) => filter(text, crlf), "x\ny");
  const inserted = crlf.update({
    changes: { from: 0, insert: crlf.toText(pasted) },
  }).state;
  assert.equal(inserted.sliceDoc(), "x\r\nya\r\nb\r\n");
  assert.equal(inserted.doc.lines, 4);
  // LF の本文には、フィルタを付けない。
  assert.equal(
    state("a\nb\n").facet(EditorView.clipboardInputFilter).length,
    0,
  );
});

test("本文の行の背景は透けさせる（不透明だと、1行の中の選択の色が隠れる）", () => {
  const activeLine = editorThemeSpec[".cm-activeLine"];
  assert.match(activeLine.backgroundColor, /transparent/);
});
