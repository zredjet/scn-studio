// AI実行の画面（T3-7、D068）の判断のテスト：タスクの並べ方、実行できない理由、指摘の持ち方と行の印、文。

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  entriesWrittenText,
  fieldLabel,
  fieldProposalKey,
  fieldsWrittenText,
  proposalValueText,
} from "../src/ai/fields.ts";
import {
  type AutoRun,
  adoptableIndexes,
  applyAutoEvent,
  autoBranchText,
  autoSceneText,
  autoStageText,
  blocker,
  cloudDestinations,
  connectionOptions,
  driftKey,
  estimateText,
  type FindingRecord,
  fileProposalSet,
  findingLines,
  findingOffset,
  findingsForScene,
  findTask,
  groupTasks,
  historyStatus,
  isSkeleton,
  mergedTags,
  mergeFindings,
  needsScene,
  placeText,
  plotWrittenText,
  preparedFor,
  progressText,
  proposalLetter,
  proposalStatusText,
  proposalsByFile,
  proposalsKey,
  resultKey,
  tokensText,
} from "../src/ai/model.ts";
import type {
  AiFinding,
  AiProposalView,
  AiTask,
  AiTaskInfo,
} from "../src/core/types.ts";

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
      destination: "openai-compatible http://localhost:11434/v1\nqwen3",
    },
    connection_problem: null,
    source: "task",
    override: null,
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
            destination: "anthropic https://api.anthropic.com/v1\nc",
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
    proposals: [],
    model: "m",
    written: null,
    wroteOk: false,
    branches: [],
    skeleton: [],
    ideas: [],
    fields: [],
    created: [],
    notes: [],
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
      output_tokens: 2048,
      limit: 6144,
      sections: [],
      trimmed: [],
      notes: [],
      system: "",
      user: "",
      tokens: 826,
      problem: null,
    }),
    "見積もり 826 / 上限 6,144 トークン（文脈の長さ 8,192、返す分 2,048）",
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
      output_tokens: 2048,
      limit: 6144,
      sections: [],
      trimmed: [],
      notes: [],
      system: "",
      user: "",
      tokens: 1,
      problem: null,
    },
    connection: {
      name: "cloud",
      model: "c",
      cloud: true,
      allowed: true,
      structured: false,
    },
  };
  assert.equal(preparedFor(prepared, "tone_check", "s012"), true);
  assert.equal(preparedFor(prepared, "tone_check", "s013a"), false);
  assert.equal(preparedFor(prepared, "setting_check", "s012"), false);
  assert.equal(preparedFor(null, "tone_check", "s012"), false);
});

function view(
  line: string,
  status: AiProposalView["status"],
  file: string | null = "story/a.scn",
): AiProposalView {
  return {
    proposal: { line, before: "a", after: "b", reason: null },
    file,
    scene: "s1",
    span: [0, 1],
    current: "a",
    status,
    diff: [],
  };
}

test("提案：ファイルのまとまり、様子の文、採用できるもの、章ごとのまとめ", () => {
  const set = fileProposalSet({
    path: "ai/proposals/2026-10-08-tone.md",
    task: null,
    model: null,
    at: null,
    proposals: [],
    problems: [],
  });
  assert.deepEqual(
    [set.id, set.label, set.task, set.model],
    [
      `file:ai/proposals/2026-10-08-tone.md:${proposalsKey([])}`,
      "2026-10-08-tone.md",
      "external:proposals",
      "unknown",
    ],
  );
  // ファイルの中身が外で変わったら、別のまとまり（照らし直し、却下を忘れる）。
  const one = [{ line: "a", before: "x", after: "y", reason: null }];
  assert.notEqual(proposalsKey(one), proposalsKey([]));
  assert.equal(proposalsKey(one), proposalsKey(one.map((p) => ({ ...p }))));
  const views = [
    view("a1", { kind: "ready" }),
    view("a2", { kind: "same" }),
    view("a3", { kind: "blocked", reason: "本文が変わっています" }),
    view("a4", { kind: "ready" }, "story/b.scn"),
    view("a5", { kind: "ready" }),
    view("a6", { kind: "ready" }, null),
  ];
  assert.deepEqual(views.slice(0, 3).map(proposalStatusText), [
    "採用できます",
    "採用済み（今の文と同じ）",
    "本文が変わっています",
  ]);
  assert.deepEqual(adoptableIndexes(views, new Set([4])), [0, 3, 5]);
  const byFile = proposalsByFile(views);
  assert.deepEqual(
    [...byFile].map(([file, list]) => [file, list.map((p) => p.line)]),
    [
      ["story/a.scn", ["a1", "a2", "a3", "a5"]],
      ["story/b.scn", ["a4"]],
    ],
  );
  const ok = {
    kind: "ok" as const,
    run: 1,
    scene: "s012",
    summary: null,
    summaryProblem: null,
    proposals: [{ line: "x", before: "a", after: "b", reason: null }],
    model: "m",
    written: null,
    wroteOk: false,
    branches: [],
    skeleton: [],
    ideas: [],
    fields: [],
    created: [],
    notes: [],
    unread: [],
    tokens: "",
  };
  assert.equal(historyStatus(ok, 0, "diff"), "提案 1件");
  assert.equal(historyStatus({ ...ok, proposals: [] }, 0, "diff"), "提案なし");
});

test("プロットとフローへの支援の文と判断（T3-10）", () => {
  const ok = {
    kind: "ok" as const,
    run: 7,
    scene: "s014",
    summary: null,
    summaryProblem: null,
    proposals: [],
    model: "m",
    written: null,
    wroteOk: false,
    branches: [],
    skeleton: [],
    ideas: [],
    fields: [],
    created: [],
    notes: [],
    unread: [],
    tokens: "",
  };
  assert.equal(historyStatus(ok, 0, "proposals"), "分岐案なし");
  assert.equal(historyStatus(ok, 0, "ideas"), "分類案なし");
  assert.equal(
    historyStatus(
      {
        ...ok,
        ideas: [{ file: "ideas/a.md", status: "保留", tags: [], reason: null }],
      },
      0,
      "ideas",
    ),
    "分類案 1件",
  );
  assert.equal(historyStatus(ok, 0, "draft"), "書き込みなし");
  // 企画書の下書き（T4-4）。
  assert.equal(historyStatus(ok, 0, "concept"), "書き込めませんでした");
  assert.equal(
    historyStatus(
      {
        ...ok,
        wroteOk: true,
        created: ["bible/entries/character/sora.md"],
        fields: [
          {
            file: "bible/entries/character/sora.md",
            text: "",
            written: ["name", "#body"],
            proposals: [],
            notes: [],
            provenance_problem: null,
          },
        ],
      },
      0,
      "entries",
    ),
    "1件作り、2項目を書き、案 0件",
  );
  assert.equal(
    historyStatus(
      {
        ...ok,
        fields: [
          {
            file: "concept.md",
            text: "",
            written: ["theme"],
            proposals: [],
            notes: [],
            provenance_problem: null,
          },
        ],
      },
      0,
      "concept",
    ),
    "1項目を書き、案 0件",
  );
  assert.equal(
    historyStatus({ ...ok, written: "x", wroteOk: true }, 0, "draft"),
    "書き込みました",
  );
  // 書けなかった文（断られた、章を選んでいない）は「書き込めませんでした」。
  assert.equal(
    historyStatus({ ...ok, written: "書きませんでした：x" }, 0, "draft"),
    "書き込めませんでした",
  );
  // ルートの骨格のタスク：生成の段階で本文に書き、今のシーンを文脈に入れないもの。企画書の下書き（T4-4）は違う。
  assert.equal(
    isSkeleton(
      task({
        level: "generate",
        output: "draft",
        context: [{ kind: "concept" }],
      }),
    ),
    true,
  );
  assert.equal(
    isSkeleton(
      task({
        level: "generate",
        output: "concept",
        context: [{ kind: "concept" }],
      }),
    ),
    false,
  );
  assert.equal(
    isSkeleton(task({ level: "generate", context: [{ kind: "scene" }] })),
    false,
  );
  // 使うタスク：名前が合うもの、無ければ形が合う最初のもの。
  const tasks = [
    info({ id: "a", level: "propose", output: "proposals" }),
    info({ id: "branch_ideas", level: "propose", output: "proposals" }),
  ];
  const fits = (t: AiTask) => t.output === "proposals";
  assert.equal(findTask(tasks, "branch_ideas", fits)?.task.id, "branch_ideas");
  assert.equal(findTask(tasks, "none", fits)?.task.id, "a");
  assert.equal(
    findTask(tasks, "none", () => false),
    null,
  );
  assert.equal(findTask(null, "a", fits), null);
  assert.equal(resultKey(7, 2), "7-2");
  assert.deepEqual([0, 1, 2].map(proposalLetter), ["A", "B", "C"]);
  // 分類案のタグは、今のタグの後ろに無いものだけを足す。
  assert.deepEqual(mergedTags(["灯", "屋上"], ["屋上", "伏線"]), [
    "灯",
    "屋上",
    "伏線",
  ]);
  // 書いた結果の文。
  const saved = {
    kind: "saved" as const,
    file: "story/a.scn",
    text: "",
    ids: [],
    verification: [] as string[],
    provenance_problem: null,
    summary: {} as never,
    checks: [],
  };
  assert.equal(
    plotWrittenText(saved, "2シーンを足しました"),
    "2シーンを足しました（story/a.scn）。検証：問題なし",
  );
  assert.equal(
    plotWrittenText(
      { ...saved, verification: ["W503 s070：出口が無い"] },
      "足しました",
    ),
    "足しました（story/a.scn）。検証：1件（W503 s070：出口が無い）",
  );
  assert.equal(
    plotWrittenText({ kind: "refused", reason: "s999 がありません" }, "x"),
    "書きませんでした：s999 がありません",
  );
  assert.equal(
    driftKey({
      scene: "s1",
      title: "",
      file: "",
      current: null,
      proposed: "要約",
      diff: [],
    }),
    "s1\n要約",
  );
});

test("自律の進み具合：計画、段、終わったシーン、トークン数", () => {
  let run: AutoRun = {
    run: 1,
    task: "draft_chapter",
    file: "story/ch.scn",
    planned: [],
    current: null,
    done: [],
    tokens: 0,
    budget: 1000,
    stage: null,
    stagesDone: [],
  };
  run = applyAutoEvent(run, { kind: "planned", scenes: ["s1", "s2"] });
  assert.deepEqual(run.planned, ["s1", "s2"]);
  assert.equal(autoSceneText(run, "s1"), "待機");
  run = applyAutoEvent(run, {
    kind: "step",
    scene: "s1",
    step: "fix",
    iteration: 2,
  });
  assert.equal(autoSceneText(run, "s1"), "直し中（2回目）");
  run = applyAutoEvent(run, { kind: "tokens", used: 220 });
  assert.equal(run.tokens, 220);
  const scene = {
    scene: "s1",
    title: "a",
    status: "written" as const,
    chars: 10,
    iterations: 1,
    issues: [],
    message: null,
    ids: ["aaaa"],
  };
  run = applyAutoEvent(run, { kind: "scene_done", scene });
  assert.equal(run.current, null);
  assert.equal(
    autoSceneText(run, "s1"),
    "書き込みました（1回直して 問題なし）",
  );
  run = applyAutoEvent(run, {
    kind: "scene_done",
    scene: { ...scene, scene: "s2", issues: ["W202 x"], iterations: 3 },
  });
  assert.equal(
    autoSceneText(run, "s2"),
    "書き込みました（点検が 1件残っています）",
  );
  run = applyAutoEvent(run, {
    kind: "scene_done",
    scene: {
      ...scene,
      scene: "s3",
      status: "skipped",
      message: "もうスタブではないので書き込みません",
    },
  });
  assert.equal(
    autoSceneText(run, "s3"),
    "もうスタブではないので書き込みません",
  );
});

test("自律をブランチで実行した後の文", () => {
  const branch = {
    name: "ai/draft_chapter-20261008-1200",
    base: "main",
    committed: true as boolean | null,
    kept: true,
    problem: null as string | null,
  };
  assert.match(autoBranchText(branch), /コミットし、main に戻りました/);
  assert.match(
    autoBranchText({ ...branch, committed: false, kept: false }),
    /空のブランチ .* を消して main に戻りました/,
  );
  assert.equal(
    autoBranchText({
      ...branch,
      committed: null,
      problem: "コミットできなかったので、ブランチ ai/x にとどまりました（e）",
    }),
    "ブランチ ai/draft_chapter-20261008-1200 に書きました。コミットできなかったので、ブランチ ai/x にとどまりました（e）",
  );
  assert.equal(
    autoBranchText({ ...branch, problem: "main に戻れませんでした（e）" }),
    "ブランチ ai/draft_chapter-20261008-1200 にコミットしました。main に戻れませんでした（e）",
  );
});

test("タスクの接続先の選択肢（T4-1）：定義のまま、作品の接続先、作品に無い選んだ接続先", () => {
  const connections = [
    { name: "local", cloud: false },
    { name: "cloud", cloud: true },
    { name: "broken", cloud: null },
  ];
  const cloudTask = info({ id: "concept_questions", model: "cloud" });
  assert.deepEqual(connectionOptions(cloudTask, connections), [
    { value: "", label: "定義のまま（cloud）" },
    { value: "local", label: "local（ローカル）" },
    { value: "cloud", label: "cloud（クラウド）" },
    { value: "broken", label: "broken（設定に誤り）" },
  ]);
  assert.equal(
    connectionOptions(info({ model: null }), [])[0]?.label,
    "定義のまま（既定の接続先）",
  );
  const gone = info({}, { source: "machine", override: "gone" });
  assert.deepEqual(connectionOptions(gone, connections).at(-1), {
    value: "gone",
    label: "gone（作品に無い）",
  });
  // 一覧の行き先：この端末で選んだものには「端末」を添える。
  assert.equal(placeText(info({})), "ローカル");
  assert.equal(
    placeText(info({}, { source: "machine", override: "local" })),
    "ローカル · 端末",
  );
  assert.equal(placeText(info({}, { connection: null })), "接続先なし");
});

test("クラウドへ送ることを確かめる送り先（T4-1）：クラウドのタスクの行き先とモデルを、重ねずに並べる", () => {
  const cloud = (name: string, destination: string) =>
    info(
      { id: name },
      {
        connection: {
          name: "cloud",
          model: "m",
          destination,
          cloud: true,
          allowed: true,
          context_tokens: 200000,
        },
      },
    );
  assert.deepEqual(
    cloudDestinations([
      info({}),
      cloud("a", "anthropic https://api.anthropic.com/v1\nb"),
      cloud("b", "anthropic https://api.anthropic.com/v1\na"),
      cloud("c", "anthropic https://api.anthropic.com/v1\nb"),
      info({}, { connection: null }),
    ]),
    [
      "anthropic https://api.anthropic.com/v1\na",
      "anthropic https://api.anthropic.com/v1\nb",
    ],
  );
  assert.deepEqual(cloudDestinations([info({})]), []);
});

test("実行中の進み具合の文（T4-2）：考えている間は「考えています」", () => {
  const state = {
    run: 1,
    task: "t",
    scene: null,
    chars: 1234,
    thinking: false,
  };
  assert.equal(progressText(state), "受け取った文字 1,234");
  assert.equal(
    progressText({ ...state, thinking: true }),
    "考えています · 受け取った文字 1,234",
  );
});

test("企画書の下書きの見せ方（T4-4）：項目の名前、案の値の文、書いた結果の文", () => {
  assert.equal(fieldLabel("logline"), "ログライン");
  assert.equal(fieldLabel("#body"), "本文");
  assert.equal(fieldLabel("unknown"), "unknown");
  assert.equal(proposalValueText("一文"), "一文");
  assert.equal(proposalValueText(""), "（空）");
  assert.equal(proposalValueText(null), "（空）");
  assert.equal(proposalValueText([]), "（空）");
  assert.equal(proposalValueText(["akari", "minato"]), "akari、minato");
  assert.equal(
    proposalValueText({ chars: 120000, routes: 2 }),
    "字数 120,000・ルート 2",
  );
  assert.equal(
    proposalValueText([
      { id: "akari_end", name: "灯エンド", intent: "見送る" },
      { id: "bad" },
    ]),
    "akari_end（灯エンド：見送る）、bad",
  );
  const written = {
    file: "concept.md",
    text: "",
    written: ["theme", "#body"],
    proposals: [
      {
        file: "concept.md",
        key: "logline",
        current: "人",
        proposed: "AI",
      },
    ],
    notes: [],
    provenance_problem: null,
  };
  assert.equal(
    fieldsWrittenText(written),
    "concept.md の空いている項目に書きました：テーマ、本文。値のある項目への案 1 件（採用を選べます）",
  );
  assert.equal(
    fieldsWrittenText({ ...written, written: [], proposals: [] }),
    "concept.md に書く項目はありませんでした",
  );
  assert.equal(
    fieldsWrittenText({
      ...written,
      written: [],
      proposals: [],
      notes: ["x"],
    }),
    "concept.md に書いた項目はありません（読まなかった・書けなかったわけは下に）",
  );
  const p = written.proposals[0];
  assert.ok(p !== undefined);
  assert.notEqual(fieldProposalKey(1, p), fieldProposalKey(2, p));
});

test("設定の下書きの結果の文（T4-5）：作ったエントリ、空欄に書いたエントリ、案、書かなかったもの", () => {
  const file = (path: string, written: string[], proposals = 0) => ({
    file: path,
    text: "",
    written,
    proposals: Array.from({ length: proposals }, () => ({
      file: path,
      key: "first_person",
      current: "わたし",
      proposed: "あたし",
    })),
    notes: [],
    provenance_problem: null,
  });
  assert.equal(
    entriesWrittenText({
      created: ["bible/entries/character/tenin.md"],
      files: [
        file("bible/entries/character/tenin.md", ["name", "#body"]),
        file("bible/entries/character/akari.md", ["age", "#body"], 1),
      ],
      notes: ["x"],
      provenance_problem: null,
    }),
    "設定DBにエントリを作りました（status: ai-draft）：character/tenin。空いている項目に書きました：character/akari（age、本文）。値のある項目への案 1 件（採用を選べます）。書かなかったもの 1 件（わけは下に）",
  );
  assert.equal(
    entriesWrittenText({
      created: [],
      files: [],
      notes: [],
      provenance_problem: null,
    }),
    "書くものはありませんでした",
  );
  assert.equal(fieldLabel("name", {}), "名前");
});

test("自律の企画と設定の段（T4-6）：進み具合と結果の文", () => {
  let run: AutoRun = {
    run: 1,
    task: "draft_chapter",
    file: "story/ch.scn",
    planned: [],
    current: null,
    done: [],
    tokens: 0,
    budget: null,
    stage: null,
    stagesDone: [],
  };
  run = applyAutoEvent(run, { kind: "stage", stage: "concept" });
  assert.equal(run.stage, "concept");
  const stage = {
    stage: "concept" as const,
    task: "draft_concept",
    model: "m",
    status: "written" as const,
    message: null,
    created: [],
    files: [
      {
        file: "concept.md",
        text: "",
        written: ["theme", "tone"],
        proposals: [
          { file: "concept.md", key: "logline", current: "a", proposed: "b" },
        ],
        notes: [],
        provenance_problem: null,
      },
    ],
    notes: [],
    issues: ["W701 x"],
  };
  run = applyAutoEvent(run, { kind: "stage_done", stage });
  assert.equal(run.stage, null);
  assert.equal(run.stagesDone.length, 1);
  assert.equal(
    autoStageText(stage),
    "concept.md の空いている項目に書きました：テーマ、トーン。値のある項目への案 1 件（採用を選べます）（検証 1件）",
  );
  assert.equal(
    autoStageText({
      ...stage,
      stage: "entries",
      created: ["bible/entries/character/x.md"],
      files: [
        {
          file: "bible/entries/character/x.md",
          text: "",
          written: ["name"],
          proposals: [],
          notes: [],
          provenance_problem: null,
        },
      ],
      issues: [],
    }),
    "設定DBにエントリを作りました（status: ai-draft）：character/x（検証 問題なし）",
  );
  assert.equal(
    autoStageText({ ...stage, status: "skipped", message: "無い" }),
    "無い",
  );
});
