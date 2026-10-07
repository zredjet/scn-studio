// 執筆表示で行IDを守る扱い（lineid.ts）のテスト。仕様書9節：分割したら前半が元のIDを引き継ぎ、
// 結合したら先頭の行のIDを残す。内容を編集してもIDは変えない。台詞の括弧の中での改行は分割ではなく、
// 行IDは閉じ括弧のある行に残す（6節、D046）。

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  deleteCharBackward,
  history,
  insertNewline,
  undo,
} from "@codemirror/commands";
import { ensureSyntaxTree } from "@codemirror/language";
import {
  EditorSelection,
  EditorState,
  type Transaction,
} from "@codemirror/state";
import type { Command, EditorView } from "@codemirror/view";
import { scn } from "@scn-studio/lezer-scn";
import { deleteForwardAtLineId, lineIdGuard } from "../src/editor/lineid.ts";
import { programmatic } from "../src/editor/state.ts";

const DOC = "灯「やあ」 ^aaaa\n地の文 ^bbbb\n";
/** 1行目の中身の終わり（行IDの手前）。 */
const END1 = "灯「やあ」".length;
/** 2行目の頭。 */
const LINE2 = "灯「やあ」 ^aaaa\n".length;

function create(doc: string, anchor: number, head = anchor): EditorState {
  const state = EditorState.create({
    doc,
    selection: EditorSelection.single(anchor, head),
    extensions: [scn(), history(), lineIdGuard()],
  });
  ensureSyntaxTree(state, state.doc.length, 5000);
  return state;
}

/** 命令を実行した後の状態（命令が使うのは state と dispatch だけ）。 */
function run(state: EditorState, command: Command): EditorState {
  let result = state;
  const target = {
    state,
    dispatch: (tr: Transaction) => {
      result = tr.state;
    },
  };
  command(target as unknown as EditorView);
  return result;
}

test("行IDの手前で Enter：元の行が行IDを引き継ぎ、新しい行は行IDなし", () => {
  const after = run(create(DOC, END1), insertNewline);
  assert.equal(after.sliceDoc(), "灯「やあ」 ^aaaa\n\n地の文 ^bbbb\n");
  assert.equal(after.selection.main.head, LINE2);
});

test("行の途中で Enter：前半が行IDを引き継ぐ", () => {
  const after = run(create(DOC, LINE2 + "地の".length), insertNewline);
  assert.equal(after.sliceDoc(), "灯「やあ」 ^aaaa\n地の ^bbbb\n文\n");
});

test("台詞の括弧の中で Enter：複数行の台詞になり、行IDは閉じ括弧の行に残る", () => {
  const doc = "灯「やあ、元気？」 ^aaaa\n";
  const after = run(create(doc, "灯「やあ、".length), insertNewline);
  assert.equal(after.sliceDoc(), "灯「やあ、\n元気？」 ^aaaa\n");
  // 複数行の台詞の最後の行の途中でも同じ。
  const multi = "灯「一行目\n二行目です」 ^aaaa\n";
  const split = run(create(multi, multi.indexOf("です")), insertNewline);
  assert.equal(split.sliceDoc(), "灯「一行目\n二行目\nです」 ^aaaa\n");
  // 閉じ括弧の後ろ（行IDの手前）なら、ふつうの行末と同じ。
  const end = run(create(doc, "灯「やあ、元気？」".length), insertNewline);
  assert.equal(end.sliceDoc(), "灯「やあ、元気？」 ^aaaa\n\n");
});

test("複数行の台詞の途中の行（閉じ括弧が下の行）を編集しても、行IDを動かさない", () => {
  // 途中の行の行ID（W303）は、そのままにする（閉じ括弧の行の行IDと2つ並べない）。
  const doc = "灯「やあ ^aaaa\n元気？」 ^bbbb\n";
  const at = "灯「".length;
  const typed = create(doc, at).update({
    changes: { from: at, insert: "ね" },
    selection: { anchor: at + 1 },
    userEvent: "input.type",
  }).state;
  assert.equal(typed.sliceDoc(), "灯「ねやあ ^aaaa\n元気？」 ^bbbb\n");
  // 閉じ括弧の行に行IDが無くても、編集した行の外へ移さない。
  const noId = "灯「やあ ^aaaa\n元気？」\n";
  const edited = create(noId, at).update({
    changes: { from: at, insert: "ね" },
    selection: { anchor: at + 1 },
    userEvent: "input.type",
  }).state;
  assert.equal(edited.sliceDoc(), "灯「ねやあ ^aaaa\n元気？」\n");
});

test("台詞の行は、どこで分割しても閉じ括弧のある行が行IDを引き継ぐ", () => {
  const doc = "灯「やあ」 ^aaaa\n";
  const after = run(create(doc, "灯".length), insertNewline);
  assert.equal(after.sliceDoc(), "灯\n「やあ」 ^aaaa\n");
});

test("行頭の字下げの後ろで Enter：中身のある行が行IDを引き継ぐ（全角空白は本文）", () => {
  const block = "@選択 q ^c001\n- a ^c002\n  灯「やあ」 ^c003\n@end\n";
  const at = block.indexOf("灯");
  const after = run(create(block, at), insertNewline);
  assert.equal(
    after.sliceDoc(),
    "@選択 q ^c001\n- a ^c002\n  \n灯「やあ」 ^c003\n@end\n",
  );
  const narration = "　地の文です ^aaaa\n";
  const split = run(create(narration, 1), insertNewline);
  assert.equal(split.sliceDoc(), "　\n地の文です ^aaaa\n");
});

test("行の頭で Enter：行IDは中身と一緒に下の行に残る", () => {
  const after = run(create(DOC, LINE2), insertNewline);
  assert.equal(after.sliceDoc(), "灯「やあ」 ^aaaa\n\n地の文 ^bbbb\n");
});

test("行の頭で Backspace：先頭の行の行IDを残して結合する", () => {
  const after = run(create(DOC, LINE2), deleteCharBackward);
  assert.equal(after.sliceDoc(), "灯「やあ」地の文 ^aaaa\n");
  assert.equal(after.selection.main.head, END1);
});

test("行IDの手前で Delete：次の行と結合する（行IDは消さない）", () => {
  const after = run(create(DOC, END1), deleteForwardAtLineId);
  assert.equal(after.sliceDoc(), "灯「やあ」地の文 ^aaaa\n");
});

test("行IDの手前に打った文字は、行IDの前に入る", () => {
  const state = create(DOC, END1);
  const after = state.update({
    changes: { from: END1, insert: "ね" },
    selection: { anchor: END1 + 1 },
    userEvent: "input.type",
  }).state;
  assert.equal(after.sliceDoc(), "灯「やあ」ね ^aaaa\n地の文 ^bbbb\n");
});

test("行を丸ごと消すと行IDも消え、中身だけを消すと空の行に行IDを残さない", () => {
  const whole = create(DOC, 0, LINE2);
  const deleted = whole.update({
    changes: { from: 0, to: LINE2 },
    userEvent: "delete.selection",
  }).state;
  assert.equal(deleted.sliceDoc(), "地の文 ^bbbb\n");
  const content = create(DOC, 0, END1);
  const cleared = content.update({
    changes: { from: 0, to: END1 },
    userEvent: "delete.selection",
  }).state;
  assert.equal(cleared.sliceDoc(), "\n地の文 ^bbbb\n");
});

test("行をまたいで消すと、先頭の行の行IDが残る", () => {
  const from = "灯「や".length;
  const to = LINE2 + "地の".length;
  const after = create(DOC, from, to).update({
    changes: { from, to },
    userEvent: "delete.selection",
  }).state;
  assert.equal(after.sliceDoc(), "灯「や文 ^aaaa\n");
});

test("行IDの手前に複数行を貼り付けると、元の行が行IDを引き継ぐ", () => {
  const after = create(DOC, END1).update({
    changes: { from: END1, insert: "X\nY" },
    userEvent: "input.paste",
  }).state;
  assert.equal(after.sliceDoc(), "灯「やあ」X ^aaaa\nY\n地の文 ^bbbb\n");
});

test("取り消すと、行IDの置き直しも一緒に戻る", () => {
  const split = run(create(DOC, "灯「や".length), insertNewline);
  const back = run(split, undo);
  assert.equal(back.sliceDoc(), DOC);
});

test("プログラムによる置き換えと変換中の入力には、手を出さない", () => {
  const state = create(DOC, END1);
  const replaced = state.update({
    changes: { from: END1, insert: "\n" },
    annotations: programmatic.of(true),
  }).state;
  assert.equal(replaced.sliceDoc(), "灯「やあ」\n ^aaaa\n地の文 ^bbbb\n");
  const composed = state.update({
    changes: { from: END1, insert: "かな" },
    userEvent: "input.type.compose",
  }).state;
  assert.equal(composed.sliceDoc(), "灯「やあ」かな ^aaaa\n地の文 ^bbbb\n");
});

test("カーソルは行IDに入らない。行IDの手前から → で次の行の頭へ移る", () => {
  const state = create(DOC, 0);
  // 行の終わりをクリックしても、行IDの手前に止まる。
  const clicked = state.update({
    selection: { anchor: LINE2 - 1 },
    userEvent: "select.pointer",
  }).state;
  assert.equal(clicked.selection.main.head, END1);
  // 行IDの手前で → を押すと、次の行の頭へ。
  const right = clicked.update({
    selection: { anchor: END1 + 1 },
    userEvent: "select",
  }).state;
  assert.equal(right.selection.main.head, LINE2);
  // 次の行の頭で ← を押すと、行IDの手前へ。
  const left = right.update({
    selection: { anchor: LINE2 - 1 },
    userEvent: "select",
  }).state;
  assert.equal(left.selection.main.head, END1);
  // 行末まで選んでも、行IDは選ばない。
  const selected = state.update({
    selection: { anchor: 0, head: LINE2 - 1 },
    userEvent: "select",
  }).state;
  assert.deepEqual(
    [selected.selection.main.anchor, selected.selection.main.head],
    [0, END1],
  );
});
