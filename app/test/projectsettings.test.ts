// 環境設定の「この作品」の頁（T4-25、N6）：欄の値から project.toml の直し方を決める。

import assert from "node:assert/strict";
import { test } from "node:test";
import type { ProjectSettings } from "../src/core/types.ts";
import {
  bracketAddPlan,
  bracketRemovePlan,
  bracketStylePlan,
  editorPlan,
  screenPlan,
  startScenePlan,
  titlePlan,
  windowPlan,
  windowTogglePlan,
} from "../src/preferences/project.ts";

const settings: ProjectSettings = {
  title: "放課後の灯",
  start_scene: "s010",
  brackets: [
    { pair: "「」", style: "normal" },
    { pair: "『』", style: "thought" },
  ],
  screen: { width: 1280, height: 720 },
  message_window: { lines: 3, chars_per_line: 24 },
  editor: { normalize_ellipsis: true, indent_narration: true },
  problems: [],
};

test("作品の情報：変わっていなければ書かない。空は消す", () => {
  assert.deepEqual(titlePlan(settings, " 放課後の灯 "), { kind: "none" });
  assert.deepEqual(titlePlan(settings, "放課後の灯 改"), {
    kind: "save",
    edits: [{ path: ["title"], value: "放課後の灯 改" }],
  });
  assert.deepEqual(titlePlan(settings, ""), {
    kind: "save",
    edits: [{ path: ["title"], value: null }],
  });
  assert.deepEqual(startScenePlan(settings, "s010"), { kind: "none" });
  assert.deepEqual(startScenePlan(settings, ""), {
    kind: "save",
    edits: [{ path: ["start_scene"], value: null }],
  });
});

test("画面とメッセージウィンドウ：両方をそろえて書く。片方だけ・正の整数でないものは書かない", () => {
  assert.deepEqual(screenPlan(settings, "1,280", "７２０"), { kind: "none" });
  assert.deepEqual(screenPlan(settings, "1920", "1080"), {
    kind: "save",
    edits: [
      { path: ["screen", "width"], value: 1920 },
      { path: ["screen", "height"], value: 1080 },
    ],
  });
  assert.equal(screenPlan(settings, "1920", "").kind, "invalid");
  assert.equal(screenPlan(settings, "0", "720").kind, "invalid");
  assert.deepEqual(screenPlan(settings, "", ""), {
    kind: "save",
    edits: [{ path: ["screen"], value: null }],
  });
  assert.deepEqual(screenPlan({ ...settings, screen: null }, "", ""), {
    kind: "none",
  });

  assert.deepEqual(windowPlan(settings, "4", "24"), {
    kind: "save",
    edits: [
      { path: ["message_window", "lines"], value: 4 },
      { path: ["message_window", "chars_per_line"], value: 24 },
    ],
  });
  assert.equal(windowPlan(settings, "", "24").kind, "invalid");
  assert.deepEqual(windowTogglePlan(settings, true), { kind: "none" });
  assert.deepEqual(windowTogglePlan(settings, false), {
    kind: "save",
    edits: [{ path: ["message_window"], value: null }],
  });
  assert.deepEqual(
    windowTogglePlan({ ...settings, message_window: null }, true),
    {
      kind: "save",
      edits: [
        { path: ["message_window", "lines"], value: 3 },
        { path: ["message_window", "chars_per_line"], value: 24 },
      ],
    },
  );
});

test("エディタと台詞の括弧", () => {
  assert.deepEqual(editorPlan(settings, "indent_narration", true), {
    kind: "none",
  });
  assert.deepEqual(editorPlan(settings, "indent_narration", false), {
    kind: "save",
    edits: [{ path: ["editor", "indent_narration"], value: false }],
  });
  assert.deepEqual(bracketStylePlan(settings, "『』", " thought "), {
    kind: "none",
  });
  assert.deepEqual(bracketStylePlan(settings, "『』", "whisper"), {
    kind: "save",
    edits: [{ path: ["dialogue", "brackets", "『』"], value: "whisper" }],
  });
  assert.equal(bracketStylePlan(settings, "『』", "").kind, "invalid");
  assert.deepEqual(bracketAddPlan(settings, "（）", "aside"), {
    kind: "save",
    edits: [{ path: ["dialogue", "brackets", "（）"], value: "aside" }],
  });
  assert.equal(bracketAddPlan(settings, "（", "aside").kind, "invalid");
  assert.equal(bracketAddPlan(settings, "「」", "aside").kind, "invalid");
  assert.deepEqual(bracketRemovePlan(settings, "『』"), {
    kind: "save",
    edits: [{ path: ["dialogue", "brackets", "『』"], value: null }],
  });
  const one = { ...settings, brackets: [{ pair: "「」", style: "normal" }] };
  assert.equal(bracketRemovePlan(one, "「」").kind, "invalid");
});
