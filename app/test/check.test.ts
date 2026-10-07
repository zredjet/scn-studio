// 検証の画面のまとめ方のテスト。

import assert from "node:assert/strict";
import { test } from "node:test";
import { countBySeverity, groupDiagnostics } from "../src/check/group.ts";
import type { IrDiagnostic } from "../src/core/types.ts";

function diag(
  file: string,
  start: number,
  code = "W503",
  severity: IrDiagnostic["severity"] = "warning",
): IrDiagnostic {
  return { code, severity, file, span: [start, start], message: code };
}

test("本文のファイルを先に、ほかのファイルをパスの順に並べ、ファイルの中は位置の順", () => {
  const groups = groupDiagnostics(
    [
      diag("variables.yaml", 3, "W605"),
      diag("story/b.scn", 10),
      diag("concept.md", 1, "W701"),
      diag("story/a.scn", 20),
      diag("story/a.scn", 5, "I302", "info"),
    ],
    ["story/a.scn", "story/b.scn"],
  );
  assert.deepEqual(
    groups.map((g) => [g.file, g.story, g.diagnostics.map((d) => d.span[0])]),
    [
      ["story/a.scn", true, [5, 20]],
      ["story/b.scn", true, [10]],
      ["concept.md", false, [1]],
      ["variables.yaml", false, [3]],
    ],
  );
});

test("同じ診断は1つにする", () => {
  const groups = groupDiagnostics(
    [diag("story/a.scn", 1), diag("story/a.scn", 1)],
    ["story/a.scn"],
  );
  assert.equal(groups[0]?.diagnostics.length, 1);
});

test("重大度ごとの件数", () => {
  assert.deepEqual(
    countBySeverity([
      diag("a", 0, "E501", "error"),
      diag("a", 1),
      diag("a", 2),
      diag("a", 3, "I302", "info"),
    ]),
    { error: 1, warning: 2, info: 1 },
  );
});
