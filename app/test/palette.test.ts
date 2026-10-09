// コマンドパレット（⌘K）で何でも探すテスト（T4-20）：種類ごとのまとめ方、件数の上限、`>` でコマンドだけ、
// シーン・設定・素材の行の中身。

import assert from "node:assert/strict";
import { test } from "node:test";
import type { AssetRow, BibleData, SceneSummary } from "../src/core/types.ts";
import {
  assetItems,
  type Command,
  commandItems,
  entryItems,
  FIND_LIMIT,
  findGroups,
  paletteCommands,
  sceneItems,
} from "../src/shell/palette.ts";

function scene(
  id: string,
  title: string,
  route: string | null,
  status: string | null = null,
): SceneSummary {
  return {
    id,
    title,
    file: "story/chapter01.scn",
    span: [0, 1],
    meta: { summary: null, status, route, tags: [], extra: {} },
    is_stub: false,
    chars: 10,
  };
}

const bible: BibleData = {
  character_type: "character",
  types: [
    { id: "foreshadow", name: "伏線", builtin: null, fields: [] },
    { id: "character", name: "キャラクター", builtin: null, fields: [] },
  ],
  files: [
    {
      path: "bible/entries/foreshadow/key.md",
      text: "",
      entry: {
        type_id: "foreshadow",
        id: "key",
        path: "bible/entries/foreshadow/key.md",
        fields: { name: "屋上の鍵" },
        key_spans: {},
        body: "",
      },
    },
    // 読めなかったエントリは探す先に入れない。
    { path: "bible/entries/character/broken.md", text: null, entry: null },
  ],
  expressions: {},
  placeholders: {},
};

function asset(name: string, state: AssetRow["state"]): AssetRow {
  return {
    kind: "background",
    character: null,
    character_name: null,
    name,
    state,
    file: null,
    path: null,
    uses: [],
    scenes: [],
    addable: false,
  };
}

const commands: Command[] = [
  { id: "flow", label: "フローを開く", run: () => {} },
  { id: "rooftop", label: "屋上の見出しへ", keys: "Mod-1", run: () => {} },
];

const items = [
  ...sceneItems([
    scene("s012", "放課後の屋上", "共通"),
    scene("s021", "屋上の約束", "灯", "ai-draft"),
    scene("s010", "登校", "共通"),
  ]),
  ...entryItems(bible),
  ...assetItems([asset("屋上/夕方", "file"), asset("屋上/夜", "placeholder")]),
  ...commandItems(commands, true),
];

test("種類ごとに、シーン・設定・素材・コマンドの順", () => {
  const groups = findGroups(items, "屋上");
  assert.deepEqual(
    groups.map((g) => [g.label, g.items.map((i) => i.label)]),
    [
      ["シーン", ["放課後の屋上", "屋上の約束"]],
      ["設定", ["屋上の鍵"]],
      ["素材", ["屋上/夕方", "屋上/夜"]],
      ["コマンド", ["屋上の見出しへ"]],
    ],
  );
});

test("行の中身：シーンはID・ルートと章・AI下書きの札、設定は型、素材は種類と仮素材の札", () => {
  const [scenes, entries, assets, cmds] = findGroups(items, "屋上");
  const ai = scenes?.items[1];
  assert.equal(ai?.lead, "s021");
  assert.equal(ai?.trail, "灯 · chapter01.scn");
  assert.deepEqual(ai?.tag, { text: "AI下書き", tone: "ai" });
  assert.equal(ai?.mark, "ai");
  assert.equal(entries?.items[0]?.lead, "伏線");
  assert.equal(entries?.items[0]?.trail, "bible/entries/foreshadow");
  assert.equal(assets?.items[1]?.lead, "背景");
  assert.deepEqual(assets?.items[1]?.tag, { text: "仮素材", tone: "ph" });
  assert.equal(cmds?.items[0]?.trail, "⌘1");
});

test("IDでも探せ、種類ごとに上限まで（省いた数を数える）", () => {
  assert.deepEqual(
    findGroups(items, "S010").map((g) => g.items.map((i) => i.lead)),
    [["s010"]],
  );
  const many = sceneItems(
    Array.from({ length: FIND_LIMIT + 2 }, (_, i) =>
      scene(`s${i}`, "雨", null),
    ),
  );
  const [g] = findGroups(many, "雨");
  assert.equal(g?.items.length, FIND_LIMIT);
  assert.equal(g?.more, 2);
});

test("`>` で始めるとコマンドだけ。何も打っていなければコマンドだけ（全部）", () => {
  assert.deepEqual(
    findGroups(items, ">屋上").map((g) => g.label),
    ["コマンド"],
  );
  const empty = findGroups(items, "");
  assert.deepEqual(
    empty.map((g) => [g.label, g.items.length]),
    [["コマンド", 2]],
  );
  assert.equal(findGroups(items, "> ")[0]?.items.length, 2);
});

test("何も打っていないときは、今の画面のコマンドを先に、今いる画面への移動は出さない（T4-21）", () => {
  const run = () => {};
  const list: Command[] = [
    { id: "screen-concept", label: "画面：企画", target: "concept", run },
    { id: "screen-text", label: "画面：本文", target: "text", run },
    { id: "screen-flow", label: "画面：フロー", target: "flow", run },
    { id: "play", label: "再生", run },
    { id: "play-here", label: "ここから再生", screens: ["text"], run },
    { id: "rename", label: "名前を変える", screens: ["text"], run },
  ];
  const ids = (screen: string, query: string) =>
    paletteCommands(list, screen, query).map((c) => c.id);
  assert.deepEqual(ids("text", ""), [
    "play-here",
    "rename",
    "screen-concept",
    "screen-flow",
    "play",
  ]);
  assert.deepEqual(ids("text", ">"), ids("text", ""));
  // ほかの画面では、その画面への移動だけを除く。
  assert.deepEqual(ids("flow", "  "), [
    "screen-concept",
    "screen-text",
    "play",
    "play-here",
    "rename",
  ]);
  // 打ったら、すべてを決まった並びで（今いる画面への移動も探せる）。
  assert.deepEqual(
    ids("text", "本文"),
    list.map((c) => c.id),
  );
});
