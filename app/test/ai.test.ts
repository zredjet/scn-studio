// AI実行の画面（T3-7、D068）の判断のテスト：タスクの並べ方、実行できない理由、指摘の持ち方と行の印、文。

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  blocker,
  estimateText,
  type FindingRecord,
  findingLines,
  findingOffset,
  findingsForScene,
  groupTasks,
  historyStatus,
  mergeFindings,
  needsScene,
  preparedFor,
  tokensText,
} from "../src/ai/model.ts";
import type { AiFinding, AiTask, AiTaskInfo } from "../src/core/types.ts";

function task(over: Partial<AiTask>): AiTask {
  return {
    id: "tone_check",
    path: "ai/tone_check.yaml",
    name: "口調チェック",
    level: "review",
    model: "local",
    context: [{ kind: "scene" }],
    output: "findings",
    scope: null,
    branch: null,
    checks: [],
    max_iterations: null,
    budget_tokens: null,
    prompt: "p",
    ...over,
  };
}

function info(
  over: Partial<AiTask>,
  more: Partial<AiTaskInfo> = {},
): AiTaskInfo {
  return {
    task: task(over),
    connection: {
      name: "local",
      model: "qwen3",
      cloud: false,
      allowed: true,
      context_tokens: 8192,
    },
    connection_problem: null,
    unsupported: null,
    ...more,
  };
}

function finding(over: Partial<AiFinding>): AiFinding {
  return {
    line: "k3fg",
    scene: "s012",
    severity: "warning",
    message: "硬い",
    file: "story/chapter01.scn",
    span: [10, 20],
    unresolved: false,
    ...over,
  };
}

function record(
  id: string,
  over: Partial<FindingRecord>,
  f: Partial<AiFinding> = {},
): FindingRecord {
  return {
    id,
    run: 1,
    task: "tone_check",
    taskName: "口調チェック",
    target: "s012",
    source: "local · qwen3",
    finding: finding(f),
    ...over,
  };
}

test("段階ごとにまとめる（自律・生成・提案・指摘の順、空の段階は出さない）", () => {
  const groups = groupTasks([
    info({ id: "a", level: "review" }),
    info({ id: "b", level: "auto" }),
    info({ id: "c", level: "review" }),
  ]);
  assert.deepEqual(
    groups.map((g) => [g.label, g.tasks.map((t) => t.task.id)]),
    [
      ["自律", ["b"]],
      ["指摘", ["a", "c"]],
    ],
  );
});

test("シーンを選ぶタスクと、実行できない理由", () => {
  assert.equal(needsScene(task({})), true);
  assert.equal(
    needsScene(task({ context: [{ kind: "previous_summaries" }] })),
    true,
  );
  assert.equal(needsScene(task({ context: [{ kind: "concept" }] })), false);
  assert.equal(blocker(info({})), null);
  assert.equal(
    blocker(info({}, { unsupported: "提案は T3-8" })),
    "提案は T3-8",
  );
  assert.equal(
    blocker(
      info({}, { connection: null, connection_problem: "接続先がありません" }),
    ),
    "接続先がありません",
  );
  assert.match(blocker(info({ prompt: null })) ?? "", /prompt/);
  assert.match(
    blocker(
      info(
        {},
        {
          connection: {
            name: "cloud",
            model: "c",
            cloud: true,
            allowed: false,
            context_tokens: 200000,
          },
        },
      ),
    ) ?? "",
    /許可/,
  );
});

test("指摘：同じタスクと対象のやり直しは置き換え、シーンごと・行ごとに引く", () => {
  const old = [
    record("1-0", { run: 1 }),
    record("1-1", { run: 1 }, { line: "k3f9", message: "一人称" }),
    record("2-0", {
      run: 2,
      task: "setting_check",
      taskName: "設定矛盾チェック",
    }),
  ];
  const fresh = [record("3-0", { run: 3 }, { message: "やり直し" })];
  const merged = mergeFindings(old, fresh, "tone_check", "s012");
  assert.deepEqual(
    merged.map((r) => r.id),
    ["2-0", "3-0"],
  );
  // ほかの対象のシーンの指摘は残す。
  assert.equal(mergeFindings(old, fresh, "tone_check", "s013a").length, 4);
  assert.deepEqual(
    findingsForScene(
      [...merged, record("4-0", {}, { scene: "s020" })],
      "s012",
    ).map((r) => r.id),
    ["2-0", "3-0"],
  );
  assert.deepEqual(findingsForScene(merged, null), []);
  const lines = findingLines([
    ...merged,
    record("5-0", {}, { line: "zzzz", unresolved: true }),
    record("6-0", {}, { line: null }),
  ]);
  assert.deepEqual(
    [...lines],
    [["k3fg", "設定矛盾チェック：硬い\n口調チェック：やり直し"]],
  );
});

test("履歴と数の文", () => {
  const ok = {
    kind: "ok" as const,
    run: 1,
    scene: "s012",
    summary: null,
    summaryProblem: null,
    unread: [],
    tokens: "",
  };
  assert.equal(historyStatus(ok, 3, "findings"), "指摘 3件");
  assert.equal(historyStatus(ok, 0, "findings"), "指摘なし");
  assert.equal(historyStatus(ok, 0, "summary"), "要約を読めません");
  assert.equal(
    historyStatus({ ...ok, summary: "x" }, 0, "summary"),
    "要約を作りました",
  );
  assert.equal(
    historyStatus(
      { kind: "failed", run: 1, scene: null, message: "x" },
      0,
      "findings",
    ),
    "失敗",
  );
  assert.equal(
    historyStatus({ kind: "cancelled", run: 1, scene: null }, 0, "findings"),
    "取り消し",
  );
  assert.equal(
    tokensText({ input_tokens: 1200, output_tokens: 40, estimated: true }),
    "入力 1,200・出力 40 トークン（文字数からの見積もり）",
  );
  assert.equal(
    estimateText({
      task: "t",
      scene: null,
      context_tokens: 8192,
      limit: 6144,
      sections: [],
      trimmed: [],
      notes: [],
      system: "",
      user: "",
      tokens: 826,
      problem: null,
    }),
    "見積もり 826 / 上限 6,144 トークン（文脈の長さ 8,192）",
  );
});

test("指摘の行へ移る位置：行IDで今の本文を探す（実行の後に行が動いても）。無ければ実行したときの位置", () => {
  const before = "## a #s1\n灯「やあ」 ^k3fg\n";
  const span: [number, number] = [
    new TextEncoder().encode("## a #s1\n").length,
    0,
  ];
  const moved = "## a #s1\n足した行。\n灯「やあ」 ^k3fg\n";
  const f = finding({ line: "k3fg", span });
  assert.equal(findingOffset(before, f), "## a #s1\n".length);
  assert.equal(findingOffset(moved, f), "## a #s1\n足した行。\n".length);
  // 行IDが消えた、本文に無い行ID：実行したときの位置へ。
  assert.equal(findingOffset("## a #s1\n灯「やあ」\n", f), "## a #s1\n".length);
  assert.equal(
    findingOffset(moved, finding({ line: "k3fg", span, unresolved: true })),
    "## a #s1\n".length,
  );
  assert.equal(findingOffset(moved, finding({ line: null, span: null })), null);
});

test("見た送る中身は、同じタスクと対象のときだけ使う", () => {
  const prepared = {
    digest: "d",
    assembled: {
      task: "tone_check",
      scene: "s012",
      context_tokens: 8192,
      limit: 6144,
      sections: [],
      trimmed: [],
      notes: [],
      system: "",
      user: "",
      tokens: 1,
      problem: null,
    },
    connection: { name: "cloud", model: "c", cloud: true, allowed: true },
  };
  assert.equal(preparedFor(prepared, "tone_check", "s012"), true);
  assert.equal(preparedFor(prepared, "tone_check", "s013a"), false);
  assert.equal(preparedFor(prepared, "setting_check", "s012"), false);
  assert.equal(preparedFor(null, "tone_check", "s012"), false);
});
