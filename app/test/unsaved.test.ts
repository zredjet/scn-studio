// 保存していない変更の確認のテスト（T4-20）：並べるもの、保存できるか。

import assert from "node:assert/strict";
import { test } from "node:test";
import { leaveProjectAsk, leaveSourceAsk } from "../src/shell/unsaved.ts";

test("作品を閉じるとき：本文だけなら保存できる", () => {
  const ask = leaveProjectAsk(
    new Set(["story/chapter02.scn", "story/chapter01.scn"]),
    [],
  );
  assert.deepEqual(ask.files, ["story/chapter01.scn", "story/chapter02.scn"]);
  assert.deepEqual(ask.drafts, []);
  assert.equal(ask.canSave, true);
  assert.equal(ask.note, null);
});

test("ソース表示の書きかけがあれば、保存するは出さない", () => {
  const ask = leaveProjectAsk(["story/chapter01.scn"], ["variables", "bible"]);
  assert.deepEqual(ask.drafts, ["設定DBのソース表示", "変数のソース表示"]);
  assert.equal(ask.canSave, false);
  assert.notEqual(ask.note, null);
  // 書きかけだけのときも。
  assert.equal(leaveProjectAsk([], ["concept"]).canSave, false);
});

test("ソース表示から移るとき：捨てるかキャンセルだけ", () => {
  const ask = leaveSourceAsk("bible", "捨てて移りますか？");
  assert.equal(ask.canSave, false);
  assert.deepEqual(ask.files, []);
  assert.deepEqual(ask.drafts, ["設定DBのソース表示"]);
  assert.equal(ask.message, "捨てて移りますか？");
});
