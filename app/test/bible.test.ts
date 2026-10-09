// 設定DBの画面（bible/fields.ts、T1-10、D049）のテスト：入力欄の選び方、入力を値にする、リストの
// 足し引き、検索、フォームの項目、診断を項目に結びつける。

import assert from "node:assert/strict";
import { test } from "node:test";
import { aiContextText } from "../src/bible/aicontext.ts";
import {
  addItem,
  appended,
  bodyOfText,
  diagnosticsByKey,
  entryName,
  expressionNameProblem,
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
import {
  BODY_KEY,
  fieldKey,
  fieldOrigins,
  isAiDraftEntry,
  originMark,
} from "../src/bible/origin.ts";
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

test("フロントマターの後ろの本文（コアの body_of と同じ読み方）", () => {
  assert.equal(bodyOfText("---\nid: a\n---\n本文\n"), "本文\n");
  assert.equal(bodyOfText("\uFEFF---\r\nid: a\r\n---\r\n本文"), "本文");
  assert.equal(bodyOfText("本文だけ\n"), "本文だけ\n");
  assert.equal(bodyOfText("---\nid: a\n"), "---\nid: a\n");
  assert.equal(bodyOfText("---  \nid: a\n---\t\n"), "");
});

test("立ち絵の差分の新しい名前（＋ 差分を追加）：空、空白、もうある名前は断る", () => {
  assert.equal(expressionNameProblem(" 照れ ", ["笑顔"]), null);
  assert.equal(expressionNameProblem("  ", []), "差分の名前を入れます");
  assert.match(expressionNameProblem("照れ 顔", []) ?? "", /空白/);
  assert.match(expressionNameProblem("照れ\u3000顔", []) ?? "", /空白/);
  assert.equal(expressionNameProblem("笑顔", ["笑顔"]), "笑顔 はもうあります");
});

test("AIに渡す内容の文：部分と字数、渡るシーン、タスクごと", () => {
  assert.equal(aiContextText(null), null);
  const ai = {
    character: true,
    parts: [
      { label: "一人称", chars: 3 },
      { label: "口調メモ", chars: 40 },
      { label: "本文", chars: 60 },
    ],
    tasks: [
      {
        id: "tone_check",
        name: "口調チェック",
        labels: ["一人称", "口調メモ"],
        chars: 43,
      },
    ],
  };
  assert.deepEqual(aiContextText(ai), {
    lead: "一人称・口調メモ・本文（約103字）。登場するシーンでAIタスクを実行すると、この内容が文脈に入ります（シーンを選ばないタスクでは、いつも入ります）。",
    tasks: ["口調チェック：一人称・口調メモ（約43字）"],
  });
  assert.equal(aiContextText({ ...ai, tasks: [] }), null);
  assert.match(
    aiContextText({ ...ai, character: false })?.lead ?? "",
    /名前が出るシーン/,
  );
});

test("項目の出自の印（T4-3）：ai は実線、ai-edited は点線。文でも見せる", () => {
  const map = fieldOrigins([
    { file: "concept.md", key: "logline", origin: "ai" },
    {
      file: "bible/entries/character/sora.md",
      key: BODY_KEY,
      origin: "ai-edited",
    },
    { file: "concept.md", key: "theme", origin: "human" },
  ]);
  assert.equal(map.size, 2);
  assert.equal(map.get(fieldKey("concept.md", "logline")), "ai");
  assert.equal(
    map.get(fieldKey("bible/entries/character/sora.md", "#body")),
    "ai-edited",
  );
  assert.equal(map.get(fieldKey("concept.md", "theme")), undefined);
  assert.equal(originMark("ai")?.text, "AI");
  assert.match(originMark("ai")?.className ?? "", /\bai\b/);
  assert.equal(originMark("ai-edited")?.text, "AI・直した");
  assert.equal(originMark(null), null);
  assert.equal(originMark(undefined), null);
  assert.equal(isAiDraftEntry({ status: "ai-draft" }), true);
  assert.equal(isAiDraftEntry({ status: "確定" }), false);
  assert.equal(isAiDraftEntry(null), false);
});
