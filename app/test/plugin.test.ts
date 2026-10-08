// 書き出しのプラグインと PDF の組版の画面の判断（export/plugin.ts、T3-14、D075）のテスト：実行の前の確かめ方、
// 結果の見せ方。

import assert from "node:assert/strict";
import { test } from "node:test";
import type { PluginRun } from "../src/core/types.ts";
import {
  approvalMessage,
  outputTail,
  pluginKindLabel,
  pluginRunText,
  runApproved,
} from "../src/export/plugin.ts";

test("確かめが要らなければ、1回だけ呼ぶ", async () => {
  const calls: (string | null)[] = [];
  const result = await runApproved(
    async (approved) => {
      calls.push(approved);
      return { path: "export/a/a.txt" };
    },
    async () => {
      throw new Error("聞かない");
    },
  );
  assert.deepEqual(calls, [null]);
  assert.equal(result?.path, "export/a/a.txt");
});

test("確かめが要れば、コマンドを見せて聞き、はいなら見せたコマンドを添えて呼び直す", async () => {
  const calls: (string | null)[] = [];
  const asked: string[] = [];
  const run = async (approved: string | null): Promise<PluginRun> => {
    calls.push(approved);
    return approved === "python tools/x.py"
      ? { path: "export/list", stdout: "書きました" }
      : { needs_approval: "python tools/x.py", profile: "export/list.yaml" };
  };
  const result = await runApproved(run, async (message) => {
    asked.push(message);
    return true;
  });
  assert.deepEqual(calls, [null, "python tools/x.py"]);
  assert.equal(result?.stdout, "書きました");
  assert.match(asked[0] ?? "", /export\/list\.yaml/);
  assert.match(asked[0] ?? "", /python tools\/x\.py/);
});

test("見せている間にコマンドが変われば、新しいコマンドを見せて聞き直す", async () => {
  const calls: (string | null)[] = [];
  const asked: string[] = [];
  let current = "python old.py";
  const run = async (approved: string | null): Promise<PluginRun> => {
    calls.push(approved);
    if (approved === current) return { path: "export/list" };
    const shown = current;
    // 1回目に見せた後で、プロファイルが書き換わった。
    current = "python new.py";
    return { needs_approval: shown, profile: "export/list.yaml" };
  };
  const result = await runApproved(run, async (message) => {
    asked.push(message);
    return true;
  });
  assert.deepEqual(calls, [null, "python old.py", "python new.py"]);
  assert.equal(asked.length, 2);
  assert.match(asked[1] ?? "", /python new\.py/);
  assert.equal(result?.path, "export/list");
});

test("断れば実行しない", async () => {
  const calls: (string | null)[] = [];
  const result = await runApproved(
    async (approved) => {
      calls.push(approved);
      return { needs_approval: "rm -rf x", profile: "export/x.yaml" };
    },
    async () => false,
  );
  assert.deepEqual(calls, [null]);
  assert.equal(result, null);
});

test("知らせと出力の見せ方", () => {
  assert.equal(
    pluginRunText({ path: "export/a/a.md" }),
    "export/a/a.md に書き出しました",
  );
  assert.equal(
    pluginRunText({ path: "export/pdf/通し.html", pdf: "export/pdf/通し.pdf" }),
    "export/pdf/通し.html に書き出し、export/pdf/通し.pdf を組みました",
  );
  assert.equal(pluginRunText({ path: null, error: "失敗しました" }), null);
  assert.equal(pluginRunText({ needs_approval: "x" }), null);
  assert.equal(outputTail("abc", 5), "abc");
  assert.equal(outputTail("abcdef", 3), "…def");
  assert.equal(pluginKindLabel("command"), "外部コマンド");
  assert.match(
    approvalMessage("export/book.yaml", "npx vivliostyle"),
    /次からは確かめません/,
  );
});
