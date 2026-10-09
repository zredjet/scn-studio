// 書き込まれる行の見せ方のテスト（T4-20）：行の番号の並べ方、名前の変更の変わる場所、書き込む場所。

import assert from "node:assert/strict";
import { test } from "node:test";
import type { RenamePreview } from "../src/core/types.ts";
import {
  LINES_SHOWN,
  linesText,
  renameCount,
  renamePlaces,
  writtenAt,
} from "../src/shell/writes.ts";

test("行の番号は・で並べ、多ければ残りの数", () => {
  assert.equal(linesText([]), "");
  assert.equal(linesText([41, 43, 71, 77]), "41・43・71・77行");
  const many = Array.from({ length: LINES_SHOWN + 3 }, (_, i) => i + 1);
  assert.equal(linesText(many), "1・2・3・4・5・6・7・8行 ほか 3行");
});

test("名前の変更の変わる場所：本文は章ごとの行、設定、ファイル名", () => {
  const preview: RenamePreview = {
    story_edits: 5,
    story: ["story/chapter01.scn", "story/chapter02.scn"],
    places: [
      { file: "story/chapter01.scn", lines: [41, 43, 71], edits: 4 },
      { file: "story/chapter02.scn", lines: [9], edits: 1 },
    ],
    settings: ["variables.yaml"],
    moves: [
      {
        from: "bible/entries/character/akari.md",
        to: "bible/entries/character/hikari.md",
      },
    ],
    notes: [],
  };
  assert.deepEqual(renamePlaces(preview), [
    { file: "chapter01.scn", what: "41・43・71行", count: 4 },
    { file: "chapter02.scn", what: "9行", count: 1 },
    { file: "variables.yaml", what: "設定", count: 1 },
    {
      file: "bible/entries/character/akari.md",
      what: "ファイル名 → hikari.md",
      count: 1,
    },
  ]);
  assert.equal(renameCount(preview), 7);
});

test("書き込む場所", () => {
  assert.equal(
    writtenAt({ file: "story/chapter01.scn", line: 96, lines: ["-> s022"] }),
    "chapter01.scn 96行目",
  );
  assert.equal(
    writtenAt({
      file: "story/chapter01.scn",
      line: 120,
      lines: ["## 放課後の図書室 #s015", "---", "summary: …", "---"],
    }),
    "chapter01.scn 120行目から",
  );
});
