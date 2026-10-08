// エディタの残り（T2-8、D059）の判断のテスト：W406 のクイックフィックス、コマンドパレットの絞り込み、
// ステータスバーの道の文字数。

import assert from "node:assert/strict";
import { test } from "node:test";
import { halfwidthOf } from "../src/editor/quickfix.ts";
import { type Command, filterCommands } from "../src/shell/palette.ts";
import { routeStatText, statOfRoute } from "../src/shell/scene.ts";

test("W406：全角の記号を半角に（見出しとジャンプの後ろは半角空白にする）", () => {
  assert.deepEqual(halfwidthOf("＠", "背"), { insert: "@", consume: 0 });
  assert.deepEqual(halfwidthOf("－＞", " "), { insert: "->", consume: 0 });
  assert.deepEqual(halfwidthOf("ー＞", "s"), { insert: "-> ", consume: 0 });
  assert.deepEqual(halfwidthOf("＞", "s"), { insert: "-> ", consume: 0 });
  assert.deepEqual(halfwidthOf("＃＃", "屋"), { insert: "## ", consume: 0 });
  // 全角空白とタブは半角空白に置き換える（`##　` は見出しにならない）。
  assert.deepEqual(halfwidthOf("＃＃", "\u3000"), {
    insert: "## ",
    consume: 1,
  });
  assert.deepEqual(halfwidthOf("＃＃", "\t"), { insert: "## ", consume: 1 });
  assert.deepEqual(halfwidthOf("＃＃", ""), { insert: "##", consume: 0 });
  assert.equal(halfwidthOf("！", "x"), null);
});

test("コマンドの絞り込み：名前かほかの言い方に、語がすべて含まれる", () => {
  const command = (id: string, label: string, keywords?: string): Command => ({
    id,
    label,
    keywords,
    run: () => {},
  });
  const commands = [
    command("focus", "集中モード", "focus zen"),
    command("rename", "名前を変える", "rename F2"),
    command("preview", "再生"),
  ];
  assert.deepEqual(
    filterCommands(commands, "").map((c) => c.id),
    ["focus", "rename", "preview"],
  );
  assert.deepEqual(
    filterCommands(commands, "ZEN").map((c) => c.id),
    ["focus"],
  );
  assert.deepEqual(
    filterCommands(commands, "名前　変える").map((c) => c.id),
    ["rename"],
  );
  assert.deepEqual(filterCommands(commands, "なし"), []);
});

test("ステータスバーの道の文字数と、読み終えるまでの目安", () => {
  assert.equal(
    routeStatText({ label: "灯ルート", chars: 12345, minutes: 31 }),
    "灯ルート 12,345字・約31分",
  );
  assert.equal(
    routeStatText({ label: "共通のみ", chars: 30000, minutes: 75 }),
    "共通のみ 30,000字・約1時間15分",
  );
  const stats = [
    { label: "共通のみ", route: "共通" },
    { label: "灯ルート", route: "灯" },
  ];
  assert.equal(statOfRoute(stats, "灯")?.label, "灯ルート");
  assert.equal(statOfRoute(stats, "知らない")?.label, "共通のみ");
  assert.equal(statOfRoute(stats, null)?.label, "共通のみ");
  assert.equal(statOfRoute([], "灯"), null);
});
