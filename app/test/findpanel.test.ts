// 本文の検索と置換の欄のテスト（T4-20）：一致の数え方と件数の文。

import assert from "node:assert/strict";
import { test } from "node:test";
import { SearchQuery } from "@codemirror/search";
import { EditorSelection, EditorState } from "@codemirror/state";
import {
  countMatches,
  countText,
  findRanges,
  findTest,
  setFindRanges,
} from "../src/editor/findPanel.ts";

const doc = "灯「好感度」\n湊「好感度が」 ^k3f9\n好感度";

test("一致の数と、選んでいる一致の番号", () => {
  const q = new SearchQuery({ search: "好感度" });
  const second = doc.indexOf("好感度", 5);
  const state = EditorState.create({
    doc,
    selection: EditorSelection.single(second, second + 3),
  });
  assert.deepEqual(countMatches(state, q), {
    total: 3,
    current: 2,
    capped: false,
  });
  // 選んでいなければ番号は 0。
  assert.equal(countMatches(EditorState.create({ doc }), q).current, 0);
  // 空・正しくない式は 0件。
  assert.equal(
    countMatches(EditorState.create({ doc }), new SearchQuery({ search: "" }))
      .total,
    0,
  );
  assert.equal(
    countMatches(
      EditorState.create({ doc }),
      new SearchQuery({ search: "(", regexp: true }),
    ).total,
    0,
  );
});

test("問い合わせの test（執筆表示で行IDを除く）を当て、上限で止める", () => {
  const state = EditorState.create({ doc });
  const q = new SearchQuery({
    search: "k",
    test: (_m, s, from) => !s.sliceDoc(from - 1, from).startsWith("^"),
  });
  assert.equal(countMatches(state, q).total, 0);
  const many = EditorState.create({ doc: "a".repeat(20) });
  assert.deepEqual(countMatches(many, new SearchQuery({ search: "a" }), 5), {
    total: 5,
    current: 0,
    capped: true,
  });
});

test("件数の文", () => {
  assert.equal(countText({ total: 4, current: 2, capped: false }), "2 / 4");
  assert.equal(countText({ total: 4, current: 0, capped: false }), "4件");
  assert.equal(countText({ total: 0, current: 0, capped: false }), "0件");
  assert.equal(countText({ total: 1000, current: 0, capped: true }), "1000+件");
});

test("探す範囲と「本文だけ」：今のシーンと本文の範囲に入る一致だけ（T4-22）", () => {
  const text = "## a\n灯「好感度」\n@設定 好感度\n## b\n好感度";
  const second = text.indexOf("## b");
  const quote = text.indexOf("好感度");
  const state = (
    scene: [number, number] | null,
    body: [number, number][] | null,
  ) =>
    EditorState.create({ doc: text, extensions: findRanges }).update({
      effects: setFindRanges.of({ body, scene }),
    }).state;
  const count = (
    s: EditorState,
    options: { scope: "file" | "scene" | "chapters"; bodyOnly: boolean },
  ) =>
    countMatches(
      s,
      new SearchQuery({ search: "好感度", test: findTest(options) }),
    ).total;
  const all = state([0, second], [[quote, quote + 3]]);
  assert.equal(count(all, { scope: "file", bodyOnly: false }), 3);
  assert.equal(count(all, { scope: "chapters", bodyOnly: false }), 3);
  // このシーン：1つ目のシーンの2つ。
  assert.equal(count(all, { scope: "scene", bodyOnly: false }), 2);
  // 本文だけ：台詞の中の1つ（演出の行と、範囲の無い2つ目のシーンは除く）。
  assert.equal(count(all, { scope: "file", bodyOnly: true }), 1);
  assert.equal(count(all, { scope: "scene", bodyOnly: true }), 1);
  // 範囲をまだ読んでいない・シーンが無いときは、当たらない。
  assert.equal(
    count(state(null, null), { scope: "scene", bodyOnly: false }),
    0,
  );
  assert.equal(count(state(null, null), { scope: "file", bodyOnly: true }), 0);
  // 絞らないときは test を付けない。
  assert.equal(findTest({ scope: "file", bodyOnly: false }), undefined);
});

test("範囲は編集に合わせてずれ、入れた分だけ入れ替わる（T4-22）", () => {
  const text = "前置き灯「好感度」";
  const quote = text.indexOf("好感度");
  let s = EditorState.create({ doc: text, extensions: findRanges }).update({
    effects: setFindRanges.of({
      body: [[quote, quote + 3]],
      scene: [0, text.length],
    }),
  }).state;
  // 前に2字足すと、範囲も2つずれる。範囲の終わりに打った字は範囲に入る。
  s = s.update({ changes: { from: 0, insert: "ああ" } }).state;
  assert.deepEqual(s.field(findRanges).body, [[quote + 2, quote + 5]]);
  s = s.update({ changes: { from: quote + 5, insert: "が" } }).state;
  assert.deepEqual(s.field(findRanges).body, [[quote + 2, quote + 6]]);
  assert.deepEqual(s.field(findRanges).scene, [0, text.length + 3]);
  // シーンだけ入れ替えても、本文の範囲は残る。
  s = s.update({ effects: setFindRanges.of({ scene: null }) }).state;
  assert.equal(s.field(findRanges).scene, null);
  assert.deepEqual(s.field(findRanges).body, [[quote + 2, quote + 6]]);
});
