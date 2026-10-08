// マージの画面の判断（editor/merge.ts、T2-7、D058）のテスト：まとまった部分の畳み方、選び方の既定、
// 印の残り。

import assert from "node:assert/strict";
import { test } from "node:test";
import type { MergeView } from "../src/core/types.ts";
import {
  contextOf,
  defaultChoices,
  hasMergeMarkers,
  linesOf,
  rememberBase,
} from "../src/editor/merge.ts";

const view: MergeView = {
  hunks: [
    { kind: "merged", text: "a\nb\nc\nd\ne\nf\ng\n" },
    { kind: "conflict", base: "x\n", mine: "y\n", theirs: "z\n" },
    { kind: "merged", text: "h\n" },
    { kind: "conflict", base: "p\n", mine: "q\n", theirs: "" },
  ],
  conflicts: 2,
  token: "0123456789abcdef",
};

test("選び方の既定は「両方」（ぶつかりの数だけ）", () => {
  assert.deepEqual(defaultChoices(view), ["both", "both"]);
  assert.deepEqual(defaultChoices({ hunks: [], conflicts: 0, token: "0" }), []);
});

test("まとまった部分は、ぶつかりの前後の数行だけ見せ、間は数で畳む", () => {
  // 最初の部分：前にぶつかりが無いので、後ろの2行だけ。
  assert.deepEqual(contextOf("a\nb\nc\nd\ne\nf\ng\n", false, true, 2), {
    head: [],
    omitted: 5,
    tail: ["f", "g"],
  });
  // 間の部分：前後の2行ずつ。短ければ畳まない。
  assert.deepEqual(contextOf("h\n", true, true, 2), {
    head: ["h"],
    omitted: 0,
    tail: [],
  });
  assert.deepEqual(contextOf("1\n2\n3\n4\n5\n6\n", true, true, 2), {
    head: ["1", "2"],
    omitted: 2,
    tail: ["5", "6"],
  });
  // 最後の部分：前の2行だけ。
  assert.deepEqual(contextOf("1\n2\n3\n", true, false, 2), {
    head: ["1", "2"],
    omitted: 1,
    tail: [],
  });
});

test("行に分ける（終わりの改行は行を作らない。CRLF も）", () => {
  assert.deepEqual(linesOf("a\r\nb\n"), ["a", "b"]);
  assert.deepEqual(linesOf(""), []);
  assert.deepEqual(linesOf("a"), ["a"]);
  assert.deepEqual(linesOf("\n"), [""]);
});

test("ソース表示の印が残っているか", () => {
  assert.ok(
    hasMergeMarkers("a\n<<<<<<< 自分の変更\nb\n=======\nc\n>>>>>>> 外の変更\n"),
  );
  assert.ok(hasMergeMarkers("a\r\n>>>>>>> 外の変更"));
  assert.ok(!hasMergeMarkers("a\n<<<<<<< 別のもの\n"));
  assert.ok(!hasMergeMarkers("a\nb\n"));
});

test("マージの元は、最初に覚えたものを残す（読み込み直しが続いても上書きしない）", () => {
  const bases = new Map<string, string>();
  rememberBase(bases, "a.scn", undefined);
  assert.equal(bases.has("a.scn"), false);
  rememberBase(bases, "a.scn", "A");
  rememberBase(bases, "a.scn", "B");
  assert.equal(bases.get("a.scn"), "A");
  bases.delete("a.scn");
  rememberBase(bases, "a.scn", "C");
  assert.equal(bases.get("a.scn"), "C");
});
