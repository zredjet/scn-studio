// 素材カタログの画面（assets/model.ts、T2-3、D054）のテスト：絞り込み、件数、足せる意味名、キャラ×差分の表、
// シーンごとの使用箇所。

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  addableKeys,
  countAssets,
  displayName,
  expressionMatrix,
  filterRows,
  rowId,
  usesByScene,
} from "../src/assets/model.ts";
import type { AssetList, AssetRow } from "../src/core/types.ts";

const at = (scene: string, start: number) => ({
  scene,
  file: "story/a.scn",
  span: [start, start + 1] as const,
});

function row(
  partial: Partial<AssetRow> & Pick<AssetRow, "kind" | "name">,
): AssetRow {
  return {
    character: null,
    character_name: null,
    state: "placeholder",
    file: null,
    path: null,
    uses: [],
    scenes: [],
    addable: false,
    ...partial,
  };
}

const list: AssetList = {
  rows: [
    row({
      kind: "background",
      name: "屋上/夕方",
      state: "file",
      file: "files/a.png",
      path: "/x/a.png",
      uses: [at("s1", 0)],
      scenes: ["s1"],
    }),
    row({
      kind: "background",
      name: "屋上/夜",
      uses: [at("s2", 5), at("s2", 9), at("s3", 1)],
      scenes: ["s2", "s3"],
    }),
    row({
      kind: "background",
      name: "駅",
      state: "unregistered",
      uses: [at("s4", 2)],
      scenes: ["s4"],
      addable: true,
    }),
    row({
      kind: "expression",
      character: "akari",
      character_name: "灯",
      name: "笑顔",
      state: "file",
      uses: [at("s1", 3)],
    }),
    row({
      kind: "expression",
      character: "akari",
      character_name: "灯",
      name: "照れ",
    }),
    row({
      kind: "expression",
      character: "tsumugi",
      character_name: "紬",
      name: "怒り",
      state: "unregistered",
      addable: true,
    }),
    row({
      kind: "expression",
      character: "だれか",
      character_name: "だれか",
      name: "笑顔",
      state: "unregistered",
    }),
    row({ kind: "se", name: "チャイム", state: "unregistered", addable: true }),
  ],
  characters: [
    { id: "akari", name: "灯", registered: true },
    { id: "tsumugi", name: "紬", registered: true },
    { id: "だれか", name: "だれか", registered: false },
  ],
};

test("絞り込み：種類、仮素材と未登録だけ、意味名（立ち絵はキャラの名前も）。未登録を先に", () => {
  const names = (rows: readonly AssetRow[]) => rows.map(displayName);
  assert.deepEqual(
    names(filterRows(list.rows, { tab: "all", missingOnly: false, query: "" })),
    [
      "駅",
      "紬/怒り",
      "だれか/笑顔",
      "チャイム",
      "屋上/夕方",
      "屋上/夜",
      "灯/笑顔",
      "灯/照れ",
    ],
  );
  assert.deepEqual(
    names(
      filterRows(list.rows, {
        tab: "background",
        missingOnly: true,
        query: "",
      }),
    ),
    ["駅", "屋上/夜"],
  );
  assert.deepEqual(
    names(
      filterRows(list.rows, { tab: "all", missingOnly: false, query: "灯" }),
    ),
    ["灯/笑顔", "灯/照れ"],
  );
  assert.deepEqual(
    names(
      filterRows(list.rows, {
        tab: "expression",
        missingOnly: false,
        query: " 笑顔 ",
      }),
    ),
    ["だれか/笑顔", "灯/笑顔"],
  );
});

test("件数：登録（カタログにあるもの）、仮素材、未登録の参照、種類ごと", () => {
  assert.deepEqual(countAssets(list.rows), {
    registered: 4,
    placeholders: 2,
    unregistered: 4,
    byKind: { background: 2, expression: 2, bgm: 0, se: 0 },
  });
});

test("カタログに足せるのは、足せる未登録だけ（キャラクターの無い差分は除く）", () => {
  assert.deepEqual(addableKeys(list.rows), [
    { kind: "background", character: null, name: "駅" },
    { kind: "expression", character: "tsumugi", name: "怒り" },
    { kind: "se", character: null, name: "チャイム" },
  ]);
});

test("キャラ×差分の表：差分は初めて出た順、無い差分は null", () => {
  const matrix = expressionMatrix(list);
  assert.deepEqual(matrix.columns, ["笑顔", "照れ", "怒り"]);
  const cells = matrix.rows.map((r) => [
    r.name,
    r.registered,
    r.cells.map((c) => (c === null ? null : `${c.row.state}:${c.uses}`)),
  ]);
  assert.deepEqual(cells, [
    ["灯", true, ["file:1", "placeholder:0", null]],
    ["紬", true, [null, null, "unregistered:0"]],
    ["だれか", false, ["unregistered:0", null, null]],
  ]);
});

test("使用箇所をシーンごとにまとめ、最初の位置へ移る", () => {
  const night = list.rows[1];
  assert.ok(night !== undefined);
  assert.deepEqual(usesByScene(night), [
    { scene: "s2", file: "story/a.scn", offset: 5, count: 2 },
    { scene: "s3", file: "story/a.scn", offset: 1, count: 1 },
  ]);
});

test("行の見分けは、種類・キャラクター・意味名で重ならない", () => {
  const ids = new Set(list.rows.map(rowId));
  assert.equal(ids.size, list.rows.length);
});
