// 企画の画面の判断（concept/ideas.ts、T2-6、D057）のテスト：状態ごとの並べ方と絞り込み、タグの順、枚数、
// 昇格先の見せ方、昇格の欄の初めの値。

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  allTags,
  defaultTitle,
  firstLine,
  groupIdeas,
  isValidId,
  statusCounts,
  suggestSceneId,
  targetLabel,
} from "../src/concept/ideas.ts";
import type { Idea } from "../src/core/types.ts";

const idea = (
  file: string,
  status: string,
  tags: string[],
  body = "",
): Idea => ({
  file,
  text: "",
  status,
  tags,
  promoted_to: null,
  target: null,
  body,
  problems: [],
});

const ideas = [
  idea("ideas/a.md", "未整理", ["灯", "屋上"]),
  idea("ideas/b.md", "採用", ["紬"]),
  idea("ideas/c.md", "没", ["灯"]),
  idea("ideas/d.md", "未整理", []),
];

test("状態の順に並べ、タグで絞り込む", () => {
  assert.deepEqual(
    groupIdeas(ideas, null).map((g) => [g.status, g.ideas.map((i) => i.file)]),
    [
      ["未整理", ["ideas/a.md", "ideas/d.md"]],
      ["採用", ["ideas/b.md"]],
      ["保留", []],
      ["没", ["ideas/c.md"]],
    ],
  );
  assert.deepEqual(
    groupIdeas(ideas, "灯").flatMap((g) => g.ideas.map((i) => i.file)),
    ["ideas/a.md", "ideas/c.md"],
  );
});

test("タグは多い順、同じなら先に出た順。枚数は状態ごと", () => {
  assert.deepEqual(allTags(ideas), ["灯", "屋上", "紬"]);
  assert.deepEqual(statusCounts(ideas), {
    未整理: 2,
    採用: 1,
    保留: 0,
    没: 1,
  });
});

test("昇格先の見せ方", () => {
  assert.equal(
    targetLabel({
      kind: "scene",
      id: "s012",
      title: "放課後の屋上",
      file: "story/chapter01.scn",
      span: [0, 1],
    }),
    "シーン：s012 放課後の屋上",
  );
  assert.equal(
    targetLabel({
      kind: "entry",
      type_id: "foreshadow",
      type_name: "伏線",
      id: "hairpin",
      name: "髪留め",
      path: "bible/entries/foreshadow/hairpin.md",
    }),
    "伏線：髪留め",
  );
  assert.equal(
    targetLabel({ kind: "missing", value: "scene/s999" }),
    "見つからない：scene/s999",
  );
});

test("昇格の欄の初めの値：題名は最初の文（30字まで）、シーンIDは続きの番号", () => {
  assert.equal(firstLine("\n  一行目 \n二行目"), "一行目");
  assert.equal(
    defaultTitle("放課後の屋上で、灯が転校を打ち明けかける。\n"),
    "放課後の屋上で、灯が転校を打ち明けかける",
  );
  assert.equal(
    defaultTitle(
      "屋上の鍵を持っているのは灯だけ。主人公はなぜ屋上に入れるのか。",
    ),
    "屋上の鍵を持っているのは灯だけ",
  );
  assert.equal(defaultTitle("知っている？ 本当に"), "知っている");
  assert.equal([...defaultTitle("あ".repeat(40))].length, 30);
  assert.equal(suggestSceneId(["s012", "s040", "s061", "intro"]), "s062");
  assert.equal(suggestSceneId(["s01", "s02", "x9", "s03"]), "s04");
  assert.equal(suggestSceneId(["s1", "s2", "s3"]), "s4");
  assert.equal(suggestSceneId([]), "s001");
  assert.equal(suggestSceneId(["s009", "s0010"]), "s0011");
  assert.ok(isValidId("s_01"));
  assert.ok(!isValidId("s-1"));
  assert.ok(!isValidId(""));
});
