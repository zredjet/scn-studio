// 本文の検索と置換の欄のテスト（T4-20）：一致の数え方と件数の文。

import assert from "node:assert/strict";
import { test } from "node:test";
import { SearchQuery } from "@codemirror/search";
import { EditorSelection, EditorState } from "@codemirror/state";
import { countMatches, countText } from "../src/editor/findPanel.ts";

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
