// 本文の置き換え（外部の変更の読み込み直し、保存で付いた行ID）のテスト。

import assert from "node:assert/strict";
import { test } from "node:test";
import { ChangeSet, EditorState } from "@codemirror/state";
import {
  appendedAtLineEnds,
  replacement,
  savedChanges,
} from "../src/editor/replace.ts";

/** エディタの状態に置き換えを当て、本文とカーソルを返す。 */
function apply(before: string, after: string, cursor: number) {
  const crlf = before.includes("\r\n");
  let state = EditorState.create({
    doc: before,
    selection: { anchor: cursor },
    extensions: crlf ? EditorState.lineSeparator.of("\r\n") : [],
  });
  const change = replacement(before, after);
  if (change !== null) state = state.update({ changes: change }).state;
  return { text: state.sliceDoc(), cursor: state.selection.main.head };
}

test("同じなら置き換えない", () => {
  assert.equal(replacement("あ", "あ"), null);
});

test("行末に足した行IDだけを置き換え、カーソルは動かさない", () => {
  const before = "## a #s1\n灯「台詞」\n地の文\n";
  const after = "## a #s1\n灯「台詞」 ^k3f9a1\n地の文\n";
  assert.deepEqual(replacement(before, after), {
    from: 14,
    to: 14,
    insert: " ^k3f9a1",
  });
  const result = apply(before, after, 3);
  assert.equal(result.text, after);
  assert.equal(result.cursor, 3);
});

test("CRLF の本文では、\\r\\n を1つに数えた位置で置き換える", () => {
  const before = "## a #s1\r\n一\r\n二\r\n";
  const after = "## a #s1\r\n一\r\n二 ^aaaa\r\n";
  const change = replacement(before, after);
  assert.deepEqual(change, { from: 12, to: 12, insert: " ^aaaa" });
  assert.equal(apply(before, after, 0).text, after);
});

test("サロゲートペア（絵文字）と \\r\\n を割らない", () => {
  const before = "😀😀";
  const after = "😀😃";
  const change = replacement(before, after);
  assert.ok(change !== null);
  assert.equal(change.insert, "😃");
  assert.equal(apply(before, after, 0).text, after);
  const crlfBefore = "一\r\n二";
  const crlfAfter = "一\r\n\r\n二";
  assert.equal(apply(crlfBefore, crlfAfter, 0).text, crlfAfter);
});

test("外部の変更：前後が同じ部分のカーソルは保つ", () => {
  const before = "一行目\n二行目\n三行目\n";
  const after = "一行目\n書き換えた二行目\n三行目\n";
  const result = apply(before, after, 1);
  assert.equal(result.text, after);
  assert.equal(result.cursor, 1);
});

interface Edit {
  readonly from: number;
  readonly to?: number;
  readonly insert: string;
}

/**
 * 保存を頼んだ本文 `base` に、保存を待つ間の編集 `edits`（`base` のエディタの位置）を当て、
 * 保存の結果 `saved` の行IDを入れた本文。
 */
function merge(base: string, saved: string, edits: readonly Edit[]): string {
  const lineBreak = base.includes("\r\n") ? "\r\n" : "\n";
  const extensions =
    lineBreak === "\r\n" ? EditorState.lineSeparator.of("\r\n") : [];
  let state = EditorState.create({ doc: base, extensions });
  const since = ChangeSet.of(edits, state.doc.length, lineBreak);
  state = state.update({ changes: since }).state;
  const changes = savedChanges(base, saved, since, state.doc);
  return state.update({ changes }).state.sliceDoc();
}

const BASE = "## a #s1\n灯「やあ」\n地の文\n";
const SAVED = "## a #s1\n灯「やあ」 ^k3f9\n地の文 ^k3fa\n";
/** 「灯「やあ」」の行末。 */
const END = "## a #s1\n灯「やあ」".length;

test("行末に足しただけの違いを見つける", () => {
  assert.deepEqual(appendedAtLineEnds(BASE, SAVED), [
    { at: END, insert: " ^k3f9" },
    { at: "## a #s1\n灯「やあ」\n地の文".length, insert: " ^k3fa" },
  ]);
  // CRLF でも、エディタの位置（\r\n を1つ）で返す。
  assert.deepEqual(appendedAtLineEnds("a\r\nb\r\n", "a\r\nb ^x\r\n"), [
    { at: 3, insert: " ^x" },
  ]);
  assert.equal(appendedAtLineEnds("a\nb\n", "a\nc\n"), null);
  assert.equal(appendedAtLineEnds("a\n", "a\nb\n"), null);
});

test("保存を待つ間に編集が無ければ、書いた本文になる", () => {
  assert.equal(merge(BASE, SAVED, []), SAVED);
});

test("保存を待つ間に行末に打った文字は残し、行IDはその後ろに付ける", () => {
  assert.equal(
    merge(BASE, SAVED, [{ from: END, insert: "ね" }]),
    "## a #s1\n灯「やあ」ね ^k3f9\n地の文 ^k3fa\n",
  );
});

test("保存を待つ間に改行して足した行には、行IDを付けない", () => {
  assert.equal(
    merge(BASE, SAVED, [{ from: END, insert: "\n次の行" }]),
    "## a #s1\n灯「やあ」 ^k3f9\n次の行\n地の文 ^k3fa\n",
  );
});

test("前の行を編集して位置がずれても、行IDは元の行に付く", () => {
  assert.equal(
    merge(BASE, SAVED, [
      { from: 0, to: 2, insert: "##" },
      { from: 9, insert: "前に足した行\n" },
    ]),
    "## a #s1\n前に足した行\n灯「やあ」 ^k3f9\n地の文 ^k3fa\n",
  );
});

test("保存を待つ間に消した行と、空にした行には、行IDを付けない", () => {
  const line = "地の文\n";
  const start = BASE.indexOf(line);
  assert.equal(
    merge(BASE, SAVED, [{ from: start, to: start + line.length, insert: "" }]),
    "## a #s1\n灯「やあ」 ^k3f9\n",
  );
  assert.equal(
    merge(BASE, SAVED, [{ from: start, to: start + 3, insert: "" }]),
    "## a #s1\n灯「やあ」 ^k3f9\n\n",
  );
});

test("保存を待つ間に行IDのある行と結合したら、行IDを2つ並べない", () => {
  const base = "## a #s1\n一行目\n二行目 ^bbbb\n";
  const saved = "## a #s1\n一行目 ^aaaa\n二行目 ^bbbb\n";
  const end = "## a #s1\n一行目".length;
  assert.equal(
    merge(base, saved, [{ from: end, to: end + 1, insert: "" }]),
    "## a #s1\n一行目二行目 ^bbbb\n",
  );
});

test("CRLF の本文でも、行IDを行末に付ける", () => {
  const base = "## a #s1\r\n一\r\n";
  const saved = "## a #s1\r\n一 ^aaaa\r\n";
  assert.equal(merge(base, saved, []), saved);
  assert.equal(
    merge(base, saved, [{ from: 10, insert: "二" }]),
    "## a #s1\r\n一二 ^aaaa\r\n",
  );
});

test("行末に足しただけでない結果は、編集が無いときだけ置き換える", () => {
  assert.equal(merge("a\n", "b\n", []), "b\n");
  assert.equal(merge("a\n", "b\n", [{ from: 1, insert: "x" }]), "ax\n");
});
