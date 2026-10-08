// プレビューの音量とミュート（preview/sound.ts、T3-1、H5）のテスト：覚えた値の読み方、鳴らす大きさ、つまみ。

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  DEFAULT_SOUND,
  effectiveVolume,
  parseSoundPrefs,
  volumePercent,
  withPercent,
} from "../src/preview/sound.ts";

test("覚えた値を読む。無い・壊れている・範囲の外は既定か端にそろえる", () => {
  assert.deepEqual(parseSoundPrefs(null), DEFAULT_SOUND);
  assert.deepEqual(parseSoundPrefs("{壊れた"), DEFAULT_SOUND);
  assert.deepEqual(parseSoundPrefs('{"volume":0.4,"muted":true}'), {
    volume: 0.4,
    muted: true,
  });
  assert.deepEqual(parseSoundPrefs('{"volume":3}'), {
    volume: 1,
    muted: false,
  });
  assert.deepEqual(parseSoundPrefs('{"volume":-1,"muted":"yes"}'), {
    volume: 0,
    muted: false,
  });
  assert.deepEqual(parseSoundPrefs('{"volume":"大"}'), DEFAULT_SOUND);
});

test("鳴らす大きさはミュートなら0。つまみは0〜100", () => {
  assert.equal(effectiveVolume({ volume: 0.6, muted: false }), 0.6);
  assert.equal(effectiveVolume({ volume: 0.6, muted: true }), 0);
  assert.equal(volumePercent({ volume: 0.555, muted: false }), 56);
  assert.deepEqual(withPercent({ volume: 1, muted: false }, 40), {
    volume: 0.4,
    muted: false,
  });
});

test("つまみで音量を上げたらミュートを解き、下げただけなら解かない", () => {
  assert.deepEqual(withPercent({ volume: 0.4, muted: true }, 60), {
    volume: 0.6,
    muted: false,
  });
  assert.deepEqual(withPercent({ volume: 0.4, muted: true }, 20), {
    volume: 0.2,
    muted: true,
  });
});
