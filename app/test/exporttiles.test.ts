// 出力の画面のカードのテスト（T4-19、U18）：2群に分け、収録の台本を先頭に。カードの下の1行。

import assert from "node:assert/strict";
import { test } from "node:test";
import { EXPORT_TILES, tileFoot, tilesOf } from "../src/export/tiles.ts";

test("制作に渡す（収録の台本が先頭）と、ほかの形の2群", () => {
  assert.deepEqual(
    tilesOf("handoff").map((t) => t.id),
    ["script", "renpy", "orders", "text"],
  );
  assert.deepEqual(
    tilesOf("other").map((t) => t.id),
    ["book", "tyrano", "utage", "plugin", "provenance"],
  );
  // 説明は1行（短く）、形式は空でない。
  for (const t of EXPORT_TILES) {
    assert.ok(t.desc.length <= 20, t.id);
    assert.notEqual(t.format, "");
  }
  assert.equal(
    new Set(EXPORT_TILES.map((t) => t.id)).size,
    EXPORT_TILES.length,
  );
});

test("カードの下の1行（この窓で最後に書き出した時刻）", () => {
  assert.equal(tileFoot(undefined), "まだ書き出していません");
  assert.equal(tileFoot("21:02"), "前回 21:02");
});
