// 表示の好みのテスト（T4-30）：覚えた文字の読み方と、画面の倍率の選び方。

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  DEFAULT_ZOOM,
  parseViewPrefs,
  ZOOM_CHOICES,
  zoomOf,
} from "../src/shell/viewPrefs.ts";

test("倍率の選択肢は 70%〜130% で、既定は 100%", () => {
  assert.deepEqual(ZOOM_CHOICES, [70, 80, 90, 100, 110, 120, 130]);
  assert.equal(DEFAULT_ZOOM, 100);
});

test("倍率：選択肢はそのまま、間はいちばん近いもの、外は端、数でなければ既定", () => {
  for (const z of ZOOM_CHOICES) assert.equal(zoomOf(z), z);
  assert.equal(zoomOf(112), 110);
  assert.equal(zoomOf(118), 120);
  assert.equal(zoomOf(50), 70);
  assert.equal(zoomOf(300), 130);
  assert.equal(zoomOf("120"), DEFAULT_ZOOM);
  assert.equal(zoomOf(Number.NaN), DEFAULT_ZOOM);
  assert.equal(zoomOf(undefined), DEFAULT_ZOOM);
});

test("覚えた文字：前の版（倍率が無い）は 100%、壊れていれば既定", () => {
  assert.deepEqual(
    parseViewPrefs('{"focus":true,"typewriter":false,"gothic":true}'),
    { focus: true, typewriter: false, gothic: true, zoom: 100 },
  );
  assert.deepEqual(
    parseViewPrefs(
      '{"focus":false,"typewriter":true,"gothic":false,"zoom":120}',
    ),
    { focus: false, typewriter: true, gothic: false, zoom: 120 },
  );
  const none = { focus: false, typewriter: false, gothic: false, zoom: 100 };
  assert.deepEqual(parseViewPrefs(null), none);
  assert.deepEqual(parseViewPrefs("{"), none);
  assert.deepEqual(parseViewPrefs("null"), none);
  assert.deepEqual(parseViewPrefs("[1]"), none);
});
