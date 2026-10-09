// 本文の画面のシーン一覧の判断のテスト（T4-18、U14）：状態の印、絞り込み、章の名前、アウトラインの行番号、
// シーンの範囲と右の欄の問題。

import assert from "node:assert/strict";
import { test } from "node:test";
import type { IrDiagnostic, SceneSummary } from "../src/core/types.ts";
import {
  chapterName,
  diagnosticsInScene,
  filterScenes,
  outlineRows,
  sceneDiagnostics,
  sceneMark,
  sceneRange,
} from "../src/scenes/list.ts";

function scene(
  id: string,
  title: string,
  over: Partial<SceneSummary> & { status?: string | null } = {},
): SceneSummary {
  const { status = null, ...rest } = over;
  return {
    id,
    title,
    file: "story/a.scn",
    span: [0, 10],
    meta: { summary: null, status, route: null, tags: [], extra: {} },
    is_stub: false,
    chars: 0,
    ...rest,
  };
}

test("状態の印：AI下書き・スタブ・執筆中・完成、ほかは下書き", () => {
  assert.equal(sceneMark(scene("a", "a", { status: "ai-draft" })), "ai");
  assert.equal(
    sceneMark(scene("a", "a", { status: "ai-draft", is_stub: true })),
    "ai",
  );
  assert.equal(sceneMark(scene("a", "a", { is_stub: true })), "stub");
  assert.equal(sceneMark(scene("a", "a", { status: "writing" })), "writing");
  assert.equal(sceneMark(scene("a", "a", { status: "done" })), "done");
  assert.equal(sceneMark(scene("a", "a", { status: "draft" })), "draft");
  assert.equal(sceneMark(scene("a", "a")), "draft");
});

test("絞り込み：ID か名前に、空白で区切った語がすべて含まれる", () => {
  const list = [
    scene("s010", "登校"),
    scene("s012", "放課後の屋上"),
    scene("S021", "屋上の約束"),
  ];
  const ids = (q: string) => filterScenes(list, q).map((s) => s.id);
  assert.deepEqual(ids(""), ["s010", "s012", "S021"]);
  assert.deepEqual(ids("屋上"), ["s012", "S021"]);
  assert.deepEqual(ids("s02"), ["S021"]);
  assert.deepEqual(ids("屋上　約束"), ["S021"]);
  assert.deepEqual(ids("無い"), []);
});

test("章の名前：見出しがあれば見出し、無ければファイル名", () => {
  const chapters = [
    { file: "story/chapter01.scn", title: "第1章 放課後" },
    { file: "story/part/b.scn", title: null },
  ];
  assert.equal(chapterName("story/chapter01.scn", chapters), "第1章 放課後");
  assert.equal(chapterName("story/part/b.scn", chapters), "part/b");
  assert.equal(chapterName("story/new.scn", chapters), "new");
});

test("アウトラインの行番号は、バイト位置から数える（全角を含む）", () => {
  const text = "## 屋上 #s1\n@背景 屋上\n灯「やあ」\n@選択 どう\n";
  const bytes = (s: string) => new TextEncoder().encode(s).length;
  const choice = bytes("## 屋上 #s1\n@背景 屋上\n灯「やあ」\n");
  const rows = outlineRows(
    [
      { kind: "direction", text: "背景", start: bytes("## 屋上 #s1\n") },
      { kind: "choice", text: "どう（1つ）", start: choice },
    ],
    text,
  );
  assert.deepEqual(
    rows.map((r) => [r.label, r.text, r.line]),
    [
      ["演出", "背景", 2],
      ["選択", "どう（1つ）", 4],
    ],
  );
});

test("シーンの範囲と、右の欄の問題（シーンの中だけ、シーンの外ならファイル全体）", () => {
  const scenes = [
    scene("s1", "一", { span: [10, 20] }),
    scene("s2", "二", { span: [100, 110] }),
    scene("t1", "別", { file: "story/b.scn", span: [0, 5] }),
  ];
  assert.deepEqual(sceneRange(scenes, "s1"), {
    file: "story/a.scn",
    start: 10,
    end: 100,
  });
  assert.deepEqual(sceneRange(scenes, "s2"), {
    file: "story/a.scn",
    start: 100,
    end: Number.POSITIVE_INFINITY,
  });
  assert.equal(sceneRange(scenes, "nope"), null);
  const diag = (file: string, at: number): IrDiagnostic => ({
    code: "W203",
    severity: "warning",
    file,
    span: [at, at + 1],
    message: "",
  });
  const all = [
    diag("story/a.scn", 5),
    diag("story/a.scn", 50),
    diag("story/a.scn", 150),
    diag("story/b.scn", 1),
  ];
  const range = sceneRange(scenes, "s1");
  assert.ok(range);
  assert.deepEqual(
    diagnosticsInScene(all, range).map((d) => d.span[0]),
    [50],
  );
  assert.deepEqual(
    sceneDiagnostics(all, scenes, "story/a.scn", "s2").map((d) => d.span[0]),
    [150],
  );
  // シーンが無い（見出しより前）か、別の章のシーンなら、今のファイルの問題すべて。
  assert.deepEqual(
    sceneDiagnostics(all, scenes, "story/a.scn", null).map((d) => d.span[0]),
    [5, 50, 150],
  );
  assert.deepEqual(
    sceneDiagnostics(all, scenes, "story/a.scn", "t1").map((d) => d.span[0]),
    [5, 50, 150],
  );
});
