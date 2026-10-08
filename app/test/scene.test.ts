// カーソルのあるシーンと文字数（shell/scene.ts、T1-8）のテスト。

import assert from "node:assert/strict";
import { test } from "node:test";
import type { SceneSummary } from "../src/core/types.ts";
import { fileChars, formatChars, sceneAt } from "../src/shell/scene.ts";

function scene(
  id: string,
  file: string,
  start: number,
  chars: number,
): SceneSummary {
  return {
    id,
    title: id,
    file,
    span: [start, start + 1],
    meta: { summary: null, status: null, route: null, tags: [], extra: {} },
    is_stub: false,
    chars,
  };
}

test("カーソルのあるシーン：見出しが前にある、いちばん後ろのシーン（位置はUTF-8とエディタで変換する）", () => {
  // 「あ」は UTF-8 で3バイト、エディタでは1つ。2つ目の見出しは、バイト位置 14（エディタの位置 10）。
  const text = "## a #s1\nあ\n## b #s2\n本文\n";
  const second = Buffer.byteLength("## a #s1\nあ\n", "utf8");
  const scenes = [
    scene("s1", "story/a.scn", 0, 1),
    scene("s2", "story/a.scn", second, 2),
    scene("s3", "story/b.scn", 0, 5),
  ];
  assert.equal(sceneAt(scenes, "story/a.scn", text, 0), "s1");
  assert.equal(
    sceneAt(scenes, "story/a.scn", text, "## a #s1\nあ".length),
    "s1",
  );
  assert.equal(
    sceneAt(scenes, "story/a.scn", text, "## a #s1\nあ\n".length),
    "s2",
  );
  assert.equal(sceneAt(scenes, "story/c.scn", "", 0), null);
  assert.equal(fileChars(scenes, "story/a.scn"), 3);
  assert.equal(formatChars(1284), "1,284字");
});
