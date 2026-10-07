// サンプル作品（samples/houkago/story/*.scn）の全行について、行の種類を確かめる。
// 期待値は test/expected/*.txt。作り直すときは UPDATE_EXPECTED=1 を付けて実行し、差分を読んでから受け入れる。

import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { test } from "node:test";
import {
  errors,
  lineKinds,
  parse,
  renderKinds,
  SAMPLE_BRACKETS,
} from "./support.ts";

const STORY = new URL("../../../samples/houkago/story/", import.meta.url);
const EXPECTED = new URL("./expected/", import.meta.url);

for (const chapter of ["chapter01", "chapter02", "chapter03"]) {
  test(`サンプル ${chapter}.scn の行の種類`, () => {
    const source = readFileSync(new URL(`${chapter}.scn`, STORY), "utf8");
    const tree = parse(source, SAMPLE_BRACKETS);
    assert.deepEqual(errors(tree), [], "構文木にエラーがある");
    const rendered = renderKinds(source, lineKinds(source, tree));
    const expectedPath = new URL(`${chapter}.txt`, EXPECTED);
    if (process.env.UPDATE_EXPECTED === "1")
      writeFileSync(expectedPath, rendered);
    assert.equal(rendered, readFileSync(expectedPath, "utf8"));
  });
}

test("サンプルの括弧の設定が project.toml と一致する", () => {
  const toml = readFileSync(
    new URL("../../../samples/houkago/project.toml", import.meta.url),
    "utf8",
  );
  const section = toml.split("[dialogue.brackets]")[1]?.split(/\n\[/)[0] ?? "";
  const keys = [...section.matchAll(/^"(.)(.)"\s*=/gmu)].map((m) => ({
    open: m[1],
    close: m[2],
  }));
  assert.deepEqual(keys, SAMPLE_BRACKETS);
});
