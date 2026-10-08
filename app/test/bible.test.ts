// 設定DBの画面（bible/fields.ts、T1-10、D049）のテスト：入力欄の選び方、入力を値にする、リストの
// 足し引き、検索、フォームの項目、診断を項目に結びつける。

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  addItem,
  appended,
  diagnosticsByKey,
  entryName,
  formFields,
  initialOf,
  isComposingKey,
  isDate,
  kindLabel,
  listItems,
  matchesQuery,
  parseInput,
  removeItem,
  sameValue,
  toggled,
  typeOfPath,
  widgetOf,
  withoutItem,
} from "../src/bible/fields.ts";
import type {
  Entry,
  EntryFile,
  EntryType,
  FieldKind,
  IrDiagnostic,
} from "../src/core/types.ts";

const kind = (k: string): FieldKind => {
  if (k.startsWith("list<"))
    return { kind: "list", item: kind(k.slice(5, -1)) };
  if (k.startsWith("ref<")) return { kind: "ref", target: k.slice(4, -1) };
  return { kind: k as "text" };
};

const AKARI: Entry = {
  type_id: "character",
  id: "akari",
  path: "bible/entries/character/akari.md",
  fields: {
    id: "akari",
    name: "灯",
    aliases: ["あかり", "灯ちゃん"],
    org: "kitakaze_high",
    memo: "型に無い",
  },
  key_spans: { id: [4, 6], name: [15, 19], aliases: [26, 33], org: [60, 63] },
  body: "本文\n",
};

const CHARACTER: EntryType = {
  id: "character",
  name: "キャラクター",
  builtin: "character",
  fields: [
    {
      key: "aliases",
      label: "呼び名",
      type: "list<text>",
      kind: kind("list<text>"),
      options: [],
    },
    {
      key: "org",
      label: "所属",
      type: "ref<organization>",
      kind: kind("ref<organization>"),
      options: [],
    },
  ],
};

test("型の見せ方：参照は型の名前で出す", () => {
  const names = new Map([["organization", "組織"]]);
  assert.equal(kindLabel(kind("ref<organization>"), names), "ref<組織>");
  assert.equal(kindLabel(kind("list<ref<scene>>"), names), "list<ref<シーン>>");
  assert.equal(kindLabel(kind("ref<animal>"), names), "ref<animal>");
  assert.equal(kindLabel(kind("longtext"), names), "longtext");
});

test("入力欄の選び方：型に合わない値の形は読むだけにする", () => {
  assert.equal(widgetOf(kind("text"), "灯"), "text");
  assert.equal(widgetOf(kind("text"), 12), "text");
  assert.equal(widgetOf(kind("text"), ["a"]), "readonly");
  assert.equal(widgetOf(kind("number"), undefined), "number");
  assert.equal(widgetOf(kind("number"), "百"), "readonly");
  assert.equal(widgetOf(kind("bool"), null), "bool");
  assert.equal(widgetOf(kind("list<text>"), ["a", 1]), "list");
  assert.equal(widgetOf(kind("list<text>"), "a"), "readonly");
  assert.equal(widgetOf(kind("list<ref<scene>>"), []), "list-ref");
  assert.equal(widgetOf(kind("list<list<text>>"), []), "readonly");
  assert.equal(widgetOf(kind("ref<organization>"), "x"), "ref");
  assert.equal(widgetOf(kind("multiselect"), ["学校"]), "multiselect");
  assert.equal(widgetOf(kind("multiselect"), "学校"), "readonly");
});

test("入力を値にする：空は文字列なら空の文字列、ほかは null。数と日付は確かめる", () => {
  assert.deepEqual(parseInput(kind("text"), " わたし "), {
    ok: true,
    value: " わたし ",
  });
  assert.deepEqual(parseInput(kind("text"), ""), { ok: true, value: "" });
  assert.deepEqual(parseInput(kind("number"), " 12.5 "), {
    ok: true,
    value: 12.5,
  });
  assert.deepEqual(parseInput(kind("number"), "１２"), { ok: true, value: 12 });
  assert.deepEqual(parseInput(kind("number"), ""), { ok: true, value: null });
  assert.equal(parseInput(kind("number"), "十二").ok, false);
  assert.deepEqual(parseInput(kind("date"), "2024-02-29"), {
    ok: true,
    value: "2024-02-29",
  });
  assert.equal(parseInput(kind("date"), "2023-02-29").ok, false);
  assert.equal(isDate("2024-13-01"), false);
  assert.equal(isDate("2024-1-01"), false);
});

test("リストの足し引き", () => {
  assert.deepEqual(addItem(kind("list<text>"), ["あかり"], " 灯ちゃん "), {
    ok: true,
    value: ["あかり", "灯ちゃん"],
  });
  assert.deepEqual(addItem(kind("list<text>"), undefined, "あ"), {
    ok: true,
    value: ["あ"],
  });
  assert.deepEqual(addItem(kind("list<number>"), [1], "2"), {
    ok: true,
    value: [1, 2],
  });
  assert.equal(addItem(kind("list<number>"), [1], "x").ok, false);
  assert.deepEqual(addItem(kind("list<text>"), ["a"], "  "), {
    ok: true,
    value: ["a"],
  });
  assert.deepEqual(removeItem(["a", "b", "c"], 1), ["a", "c"]);
  assert.deepEqual(listItems(["a", 1, true, null]), ["a", "1", "true"]);
  assert.ok(sameValue(["a"], ["a"]));
  assert.ok(!sameValue("1", 1));
  assert.ok(sameValue(undefined, null));
});

test("名前、頭文字、検索", () => {
  const file: EntryFile = { path: AKARI.path, text: "", entry: AKARI };
  assert.equal(entryName(AKARI), "灯");
  assert.equal(initialOf("灯"), "灯");
  assert.equal(initialOf("😀ab"), "😀");
  assert.equal(initialOf(""), "？");
  assert.ok(matchesQuery(file, "あかり"));
  assert.ok(matchesQuery(file, "AKARI"));
  assert.ok(matchesQuery(file, "灯 ちゃん"));
  assert.ok(!matchesQuery(file, "紬"));
  assert.ok(matchesQuery(file, "  "));
  const broken: EntryFile = {
    path: "bible/entries/character/broken.md",
    text: "---\n",
    entry: null,
  };
  assert.ok(matchesQuery(broken, "broken"));
  assert.equal(typeOfPath(broken.path), "character");
});

test("フォームの項目：名前を先頭に足し、型に無い項目は読むだけで出す", () => {
  const { fields, extra } = formFields(CHARACTER, AKARI);
  assert.deepEqual(
    fields.map((f) => f.key),
    ["name", "aliases", "org"],
  );
  assert.deepEqual(extra, ["memo"]);
  // 型の定義が無くても、名前は出す。
  const none = formFields(undefined, AKARI);
  assert.deepEqual(
    none.fields.map((f) => f.key),
    ["name"],
  );
  assert.deepEqual(none.extra, ["aliases", "org", "memo"]);
});

test("診断を項目に結びつける：キーの位置が同じもの", () => {
  const d = (span: [number, number], code = "W805"): IrDiagnostic => ({
    code,
    severity: "warning",
    file: AKARI.path,
    span,
    message: "m",
  });
  const byKey = diagnosticsByKey(AKARI, [d([60, 63], "W806"), d([0, 0])]);
  assert.deepEqual(
    byKey.get("org")?.map((x) => x.code),
    ["W806"],
  );
  assert.equal(byKey.get("")?.length, 1);
  assert.equal(diagnosticsByKey(null, [d([60, 63])]).get("")?.length, 1);
});

test("書く時点の値に対する足し引き（続けて押しても取りこぼさない）", () => {
  // 1つ目の×で a が消えた後の値に、2つ目の×（c）を当てる。
  assert.deepEqual(withoutItem(withoutItem(["a", "b", "c"], "a"), "c"), ["b"]);
  assert.deepEqual(withoutItem(["a", "a"], "a"), ["a"]);
  assert.deepEqual(withoutItem(undefined, "a"), []);
  assert.deepEqual(toggled(["学校"], "部活", true), ["学校", "部活"]);
  assert.deepEqual(toggled(["学校", "部活"], "学校", false), ["部活"]);
  assert.deepEqual(toggled(["学校"], "学校", true), ["学校"]);
  assert.deepEqual(appended(["s001"], "s002"), ["s001", "s002"]);
  assert.deepEqual(appended(["s001"], "s001"), ["s001"]);
});

test("変換中のキー操作（確定の Enter、取り消しの Escape）を見分ける", () => {
  assert.ok(
    isComposingKey({ nativeEvent: { isComposing: true }, keyCode: 13 }),
  );
  assert.ok(
    isComposingKey({ nativeEvent: { isComposing: false }, keyCode: 229 }),
  );
  assert.ok(
    !isComposingKey({ nativeEvent: { isComposing: false }, keyCode: 27 }),
  );
});
