// 検証の画面の判断のテスト（T4-19、U19）：直し方の表が仕様書の診断の一覧を覆っていること、重さで絞る、
// 種類ごと・ファイルごと・シーンごとのまとめ方、行き先、該当の行。

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import {
  CATEGORIES,
  CODE_INFO,
  codeInfo,
  contextLines,
  destinationOf,
  filterBySeverity,
  groupProblems,
  sceneOf,
} from "../src/check/fixes.ts";
import type { IrDiagnostic, SceneSummary } from "../src/core/types.ts";

function diag(
  code: string,
  file: string,
  at: number,
  severity: IrDiagnostic["severity"] = "warning",
): IrDiagnostic {
  return { code, severity, file, span: [at, at + 1], message: code };
}

function scene(id: string, file: string, at: number): SceneSummary {
  return {
    id,
    title: `題${id}`,
    file,
    span: [at, at + 4],
    meta: { summary: null, status: null, route: null, tags: [], extra: {} },
    is_stub: false,
    chars: 0,
  };
}

test("直し方の表は、仕様書の診断の一覧のコードをすべて覆う（種類・名前・説明・直し方がある）", () => {
  const spec = readFileSync(
    new URL("../../docs/spec/scn-format-v0.md", import.meta.url),
    "utf8",
  );
  const codes = [...spec.matchAll(/^\| ([EWI]\d{3}) \|/gm)].map((m) => m[1]);
  assert.ok(codes.length >= 50);
  for (const code of codes) {
    const info = CODE_INFO[code as string];
    assert.ok(info !== undefined, `${code} が表に無い`);
    assert.ok(CATEGORIES.includes(info.category));
    assert.notEqual(info.title, "");
    assert.notEqual(info.about, "");
    assert.ok(info.fixes.length > 0, `${code} に直し方が無い`);
  }
  // 表に無いコードは、名前をコードにして書式に入れる。
  assert.deepEqual(codeInfo("X999"), {
    category: "書式",
    title: "X999",
    about: "",
    fixes: [],
  });
});

test("重さで絞る", () => {
  const list = [
    diag("E501", "story/a.scn", 1, "error"),
    diag("W503", "story/a.scn", 2),
    diag("I302", "story/a.scn", 3, "info"),
  ];
  assert.equal(filterBySeverity(list, null).length, 3);
  assert.deepEqual(
    filterBySeverity(list, "warning").map((d) => d.code),
    ["W503"],
  );
});

test("種類ごと・ファイルごと・シーンごとにまとめる（同じ診断は1つ）", () => {
  const files = ["story/a.scn", "story/b.scn"];
  const scenes = [
    scene("s1", "story/a.scn", 10),
    scene("s2", "story/a.scn", 100),
    scene("t1", "story/b.scn", 0),
  ];
  const list = [
    diag("I302", "story/a.scn", 120, "info"),
    diag("W605", "variables.yaml", 0),
    diag("W503", "story/a.scn", 50),
    diag("W503", "story/a.scn", 50),
    diag("W701", "concept.md", 0),
    diag("W203", "story/b.scn", 5),
  ];
  const view = (g: ReturnType<typeof groupProblems>) =>
    g.map((x) => [x.label, x.items.map((d) => d.code)]);
  assert.deepEqual(view(groupProblems(list, "category", files, scenes)), [
    ["構造", ["W503", "W701"]],
    ["変数", ["W605"]],
    ["表示", ["W203"]],
    ["行ID", ["I302"]],
  ]);
  assert.deepEqual(view(groupProblems(list, "file", files, scenes)), [
    ["story/a.scn", ["W503", "I302"]],
    ["story/b.scn", ["W203"]],
    ["concept.md", ["W701"]],
    ["variables.yaml", ["W605"]],
  ]);
  assert.deepEqual(view(groupProblems(list, "scene", files, scenes)), [
    ["s1 題s1", ["W503"]],
    ["s2 題s2", ["I302"]],
    ["t1 題t1", ["W203"]],
    ["concept.md", ["W701"]],
    ["variables.yaml", ["W605"]],
  ]);
  assert.equal(sceneOf(scenes, diag("W101", "story/a.scn", 5)), null);
  // 本文でないファイルのまとまりは、重さではなく名前の順。
  const heavy = [diag("E801", "variables.yaml", 0, "error"), ...list];
  assert.deepEqual(
    view(groupProblems(heavy, "file", files, scenes)).map(([label]) => label),
    ["story/a.scn", "story/b.scn", "concept.md", "variables.yaml"],
  );
});

test("行き先と、該当の行と前後1行", () => {
  const files = ["story/a.scn"];
  assert.equal(
    destinationOf(diag("W503", "story/a.scn", 0), files),
    "本文で開く",
  );
  assert.equal(
    destinationOf(diag("W806", "bible/entries/x/a.md", 0), files),
    "設定で見る",
  );
  assert.equal(
    destinationOf(diag("W605", "variables.yaml", 0), files),
    "変数で見る",
  );
  assert.equal(
    destinationOf(diag("W701", "concept.md", 0), files),
    "企画を開く",
  );
  assert.equal(destinationOf(diag("E803", "project.toml", 0), files), null);
  const text = "一行目\n二行目\n三行目\n";
  const at = new TextEncoder().encode("一行目\n").length;
  assert.deepEqual(contextLines(text, at), [
    { line: 1, text: "一行目", at: false },
    { line: 2, text: "二行目", at: true },
    { line: 3, text: "三行目", at: false },
  ]);
  assert.deepEqual(contextLines(undefined, 0), []);
});
