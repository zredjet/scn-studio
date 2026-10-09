// フォームでの編集（T2-10、D061）の判断のテスト：変数の宣言の入力欄と値、型定義の項目の型の候補と選択肢、
// 型を作るときの元（テンプレートと複製。T4-9）。

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  freeTypeId,
  needsOptions,
  parseOptions,
  refillTypeForm,
  typeChoices,
  typeSourceDefaults,
  typeSourceOf,
  typeSourceText,
} from "../src/bible/typeform.ts";
import type {
  EntryType,
  TypeTemplate,
  VariableRow,
} from "../src/core/types.ts";
import {
  commitRange,
  declFields,
  declValue,
  rangeShown,
  rangeValue,
} from "../src/variables/form.ts";

const row = (over: Partial<VariableRow>): VariableRow => ({
  key: "flag.x",
  declared: true,
  type: "int",
  values: [],
  default: "0",
  range: null,
  scope: null,
  desc: null,
  span: null,
  reads: [],
  writes: [],
  ...over,
});

test("変数の入力欄は型で変わる", () => {
  assert.deepEqual(
    declFields(row({})).map((f) => f.key),
    ["type", "default", "range_min", "range_max", "scope", "desc"],
  );
  const e = declFields(row({ type: "enum", values: ["甲", "乙"] }));
  assert.deepEqual(
    e.map((f) => f.key),
    ["type", "values", "default", "scope", "desc"],
  );
  assert.deepEqual(e.find((f) => f.key === "default")?.options, ["甲", "乙"]);
  assert.equal(
    declFields(row({ type: "bool" })).find((f) => f.key === "default")?.kind
      .kind,
    "bool",
  );
});

test("一覧の行から入力欄の値を作る", () => {
  assert.equal(declValue(row({ default: "3" }), "default"), 3);
  assert.equal(declValue(row({ default: "x" }), "default"), "x");
  assert.equal(
    declValue(row({ type: "bool", default: "true" }), "default"),
    true,
  );
  assert.equal(declValue(row({ default: null }), "default"), undefined);
  assert.equal(declValue(row({ range: [0, 10] }), "range_max"), 10);
});

test("範囲の片方を直した値", () => {
  assert.deepEqual(rangeValue(row({ range: [0, 10] }), "range_max", 5), [0, 5]);
  assert.equal(rangeValue(row({}), "range_min", 1), undefined);
  assert.equal(
    rangeValue(row({ range: [0, 10] }), "range_min", null),
    undefined,
  );
  assert.equal(rangeValue(row({ range: null }), "range_min", null), null);
  // 範囲の無い変数：先に入れた片方（入れかけ）と組み合わせて、範囲を作れる。
  assert.deepEqual(rangeValue(row({}), "range_max", 10, 0), [0, 10]);
  assert.deepEqual(rangeValue(row({}), "range_min", 2, 8), [2, 8]);
  // 入れかけの値は、宣言の範囲より先に使う。
  assert.deepEqual(
    rangeValue(row({ range: [0, 10] }), "range_min", 1, 5),
    [1, 5],
  );
});

test("範囲の入れかけ：欄に出し、両方そろったら書く。ほかの変数の入れかけは使わない", () => {
  // 範囲の無い変数で最小を入れる：書かずに入れかけにし、欄にはその値を出す。
  const none = row({ key: "flag.x" });
  const first = commitRange(none, null, "range_min", 3);
  assert.equal(first.write, undefined);
  assert.deepEqual(first.draft, { key: "flag.x", range_min: 3 });
  assert.equal(rangeShown(none, first.draft, "range_min"), 3);
  assert.equal(rangeShown(none, first.draft, "range_max"), undefined);
  // 最大を入れると、両方で書き、入れかけは無くなる。
  const second = commitRange(none, first.draft, "range_max", 10);
  assert.deepEqual(second.write, [3, 10]);
  assert.equal(second.draft, null);
  // 範囲のある変数で最小を空にする：書かずに、欄は空のまま（宣言の 0 に戻さない）。
  const ranged = row({ key: "flag.x", range: [0, 10] });
  const cleared = commitRange(ranged, null, "range_min", null);
  assert.equal(cleared.write, undefined);
  assert.equal(rangeShown(ranged, cleared.draft, "range_min"), undefined);
  assert.equal(rangeShown(ranged, cleared.draft, "range_max"), 10);
  // 続けて最大を 20 にしても、最小が空なので書かない（欄は空と 20）。両方空にすると範囲を消す。
  const both = commitRange(ranged, cleared.draft, "range_max", 20);
  assert.equal(both.write, undefined);
  assert.equal(rangeShown(ranged, both.draft, "range_max"), 20);
  assert.equal(
    commitRange(ranged, cleared.draft, "range_max", null).write,
    null,
  );
  // ほかの変数の入れかけは使わない。
  const other = row({ key: "flag.y" });
  assert.equal(
    commitRange(other, first.draft, "range_max", 10).write,
    undefined,
  );
  assert.equal(rangeShown(other, first.draft, "range_min"), undefined);
});

test("型定義の項目の型の候補と、選択肢", () => {
  const types: EntryType[] = [
    { id: "place", name: "場所", builtin: null, fields: [] },
  ];
  const choices = typeChoices(types);
  assert.ok(
    choices.includes("ref<place>") && choices.includes("list<ref<scene>>"),
  );
  assert.ok(typeChoices(types, "list<ref<item>>").includes("list<ref<item>>"));
  assert.deepEqual(parseOptions(" 甲、乙,甲\n 丙 ,"), ["甲", "乙", "丙"]);
  assert.ok(needsOptions("select") && !needsOptions("text"));
});

test("型を作るときの元：選択肢の値、入れておく名前とID、説明", () => {
  assert.deepEqual(typeSourceOf("empty"), { kind: "empty" });
  assert.deepEqual(typeSourceOf("template:place"), {
    kind: "template",
    id: "place",
  });
  assert.deepEqual(typeSourceOf("type:character"), {
    kind: "type",
    id: "character",
  });
  assert.equal(freeTypeId("place", []), "place");
  assert.equal(freeTypeId("place", ["place", "place_2"]), "place_3");
  assert.equal(freeTypeId("場所", []), "");

  const place: TypeTemplate = {
    id: "place",
    name: "場所",
    description: "舞台になる場所",
    labels: ["呼び名", "種類"],
    placed: true,
  };
  const templates: TypeTemplate[] = [place];
  const types: EntryType[] = [
    {
      id: "character",
      name: "キャラクター",
      builtin: "character",
      fields: [
        {
          key: "speech",
          label: "口調メモ",
          type: "longtext",
          kind: { kind: "longtext" },
          options: [],
        },
      ],
    },
    { id: "place", name: "場所", builtin: null, fields: [] },
  ];
  const taken = ["character", "place"];
  // テンプレート：名前とID（使われていれば番号を付ける）。
  assert.deepEqual(
    typeSourceDefaults(typeSourceOf("template:place"), templates, types, taken),
    { name: "場所2", id: "place_2" },
  );
  // 複製：「〇〇のコピー」と <ID>_copy。
  assert.deepEqual(
    typeSourceDefaults(typeSourceOf("type:character"), templates, types, taken),
    { name: "キャラクターのコピー", id: "character_copy" },
  );
  assert.deepEqual(
    typeSourceDefaults({ kind: "empty" }, templates, types, taken),
    { name: "", id: "" },
  );
  assert.deepEqual(
    typeSourceDefaults(typeSourceOf("template:none"), templates, types, taken),
    { name: "", id: "" },
  );
  assert.equal(
    typeSourceText(typeSourceOf("template:place"), templates, types),
    "舞台になる場所（項目 2：呼び名・種類）",
  );
  assert.equal(
    typeSourceText(typeSourceOf("type:place"), templates, types),
    "場所 の項目を写した新しい型を作ります。元の型とはつながりません（項目なし）",
  );
  assert.equal(typeSourceText({ kind: "empty" }, templates, types), null);
  // 項目が多ければ、初めの8つと数だけ。
  const many: TypeTemplate = {
    ...place,
    id: "many",
    labels: ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"],
  };
  assert.equal(
    typeSourceText(typeSourceOf("template:many"), [many], types),
    "舞台になる場所（項目 10：a・b・c・d・e・f・g・h ほか 2）",
  );
});

test("元を選び直したとき：入れたままの欄は入れ替え、人が書き換えた欄は残す", () => {
  const filled = { name: "場所", id: "place" };
  const next = { name: "キャラクターのコピー", id: "character_copy" };
  // 入れたまま：両方入れ替わる。
  assert.deepEqual(refillTypeForm(filled, filled, next), next);
  // 人が書き換えた名前とIDは残る。
  assert.deepEqual(
    refillTypeForm({ name: "舞台", id: "stage" }, filled, next),
    { name: "舞台", id: "stage" },
  );
  // 元を選ぶ前に名前だけ書いた：IDだけが入る。
  assert.deepEqual(
    refillTypeForm(
      { name: "脇役", id: "" },
      { name: "", id: "" },
      { name: "キャラクター", id: "character_2" },
    ),
    { name: "脇役", id: "character_2" },
  );
});
