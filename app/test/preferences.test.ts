// 環境設定の AI の接続先（T3-5、D066）の判断のテスト：使う前に足りないもの、タイトルバーの表示、結果の文。

import assert from "node:assert/strict";
import { test } from "node:test";
import type { AiConnection } from "../src/core/types.ts";
import {
  currentTest,
  indicator,
  type OverrideDraft,
  outputTokensProblem,
  overrideOutputProblem,
  overrideSave,
  parseContextTokens,
  parseOutputTokens,
  readiness,
  replyText,
  structuredChoice,
  structuredValue,
  type TestRecord,
  type TestState,
  testTarget,
  usageText,
} from "../src/preferences/connection.ts";

function connection(over: Partial<AiConnection>): AiConnection {
  return {
    name: "local",
    provider: "openai-compatible",
    base_url: "http://localhost:11434/v1",
    model: "qwen3",
    project_base_url: "http://localhost:11434/v1",
    project_model: "qwen3",
    override_base_url: null,
    override_model: null,
    cloud: false,
    endpoint: "openai-compatible http://localhost:11434/v1",
    allowed: true,
    has_key: false,
    needs_key: false,
    context_tokens: 8192,
    project_context_tokens: null,
    override_context_tokens: null,
    output_tokens: 2048,
    project_output_tokens: null,
    override_output_tokens: null,
    structured_output: false,
    project_structured_output: null,
    override_structured_output: null,
    problem: null,
    url_hint: null,
    is_default: true,
    test_preview: "POST http://localhost:11434/v1/chat/completions",
    ...over,
  };
}

test("使う前に足りないもの：設定の誤り、クラウドの許可、キーの順", () => {
  assert.equal(readiness(connection({})), null);
  assert.equal(
    readiness(connection({ problem: "[ai.local] に model がありません" })),
    "[ai.local] に model がありません",
  );
  const cloud = connection({
    name: "cloud",
    provider: "anthropic",
    cloud: true,
    allowed: false,
    needs_key: true,
  });
  assert.equal(
    readiness(cloud),
    "この端末からクラウドへ送ることを許可していません",
  );
  assert.equal(readiness({ ...cloud, allowed: true }), "キーがありません");
  assert.equal(readiness({ ...cloud, allowed: true, has_key: true }), null);
});

test("タイトルバー：既定の接続先と、試した結果の印", () => {
  const tests = new Map<string, TestRecord>();
  assert.deepEqual(indicator([], tests), {
    text: "AI：未設定",
    state: "unusable",
    detail: "project.toml に [ai.local] などの接続先がありません",
  });
  const list = [
    connection({ name: "cloud", cloud: true, is_default: false }),
    connection({}),
  ];
  const record = (name: string, state: TestState): TestRecord => {
    const c = list.find((x) => x.name === name);
    assert.ok(c);
    return { target: testTarget(c), state };
  };
  const shown = indicator(list, tests);
  assert.equal(shown.text, "AI：qwen3（ローカル）");
  assert.equal(shown.state, "unknown");
  assert.match(shown.detail, /接続先 local（ローカル）/);
  tests.set("local", record("local", { kind: "running" }));
  assert.equal(indicator(list, tests).state, "running");
  tests.set("local", record("local", { kind: "failed", message: "x" }));
  assert.equal(indicator(list, tests).state, "failed");
  tests.set("local", record("local", { kind: "cancelled" }));
  assert.equal(indicator(list, tests).state, "unknown");
  // ほかの接続先の結果は見ない。使えない既定の接続先は、試した結果より先に知らせる。
  tests.set(
    "cloud",
    record("cloud", {
      kind: "ok",
      result: {
        text: "はい",
        usage: { input_tokens: 1, output_tokens: 1, estimated: false },
        stop_reason: "stop",
        thinking_chars: 0,
        elapsed_ms: 10,
      },
    }),
  );
  assert.equal(indicator(list, tests).state, "unknown");
  const broken = [connection({ model: null, problem: "model がありません" })];
  const unusable = indicator(broken, new Map());
  assert.equal(unusable.text, "AI：local（ローカル）");
  assert.equal(unusable.state, "unusable");
  assert.match(unusable.detail, /model がありません/);
});

test("試した結果の文：トークン数と、見積もりかどうかと、時間", () => {
  const result = {
    text: "はい",
    usage: { input_tokens: 23, output_tokens: 2, estimated: true },
    stop_reason: null,
    thinking_chars: 0,
    elapsed_ms: 1234,
  };
  assert.equal(
    usageText(result),
    "入力 23・出力 2 トークン（文字数からの見積もり） · 1.2秒",
  );
  assert.equal(
    usageText({ ...result, usage: { ...result.usage, estimated: false } }),
    "入力 23・出力 2 トークン · 1.2秒",
  );
});

test("設定が変わった後は、前の設定で試した結果を見せない", () => {
  const before = connection({});
  const tests = new Map<string, TestRecord>([
    [
      "local",
      { target: testTarget(before), state: { kind: "failed", message: "x" } },
    ],
  ]);
  assert.equal(currentTest(before, tests)?.kind, "failed");
  // project.toml か上書きで、モデルや URL が変わった。
  const otherModel = connection({ model: "gemma" });
  assert.equal(currentTest(otherModel, tests), undefined);
  assert.equal(indicator([otherModel], tests).state, "unknown");
  const otherUrl = connection({
    endpoint: "openai-compatible http://localhost:1234/v1",
  });
  assert.equal(currentTest(otherUrl, tests), undefined);
});

test("応答の文：空なら終わった理由を添える", () => {
  const result = {
    text: " はい\n",
    usage: { input_tokens: 1, output_tokens: 1, estimated: false },
    stop_reason: "stop",
    thinking_chars: 0,
    elapsed_ms: 1,
  };
  assert.equal(replyText(result), "応答「はい」");
  // 考えるモデル（T4-2）：除いた考えた中身と、考える分で上限に達してもつながっていること。
  assert.equal(
    replyText({ ...result, thinking_chars: 1200 }),
    "応答「はい」（考えた中身 1,200 字は除きました）",
  );
  assert.match(
    replyText({
      ...result,
      text: "",
      stop_reason: "length",
      thinking_chars: 80,
    }),
    /考えた中身 80 字だけで、答えの前に上限（length）に達しました。つながっています/,
  );
  assert.equal(
    replyText({ ...result, text: "", stop_reason: "length" }),
    "応答（空。終わった理由：length）",
  );
  assert.equal(
    replyText({ ...result, text: "", stop_reason: null }),
    "応答（空）",
  );
});

test("文脈の長さの上書きの欄（T4-1）：空は上書きなし、正の整数、桁区切りと全角", () => {
  assert.equal(parseContextTokens(""), null);
  assert.equal(parseContextTokens("  "), null);
  assert.equal(parseContextTokens("32768"), 32768);
  assert.equal(parseContextTokens("32,768"), 32768);
  assert.equal(parseContextTokens("３２７６８"), 32768);
  assert.equal(parseContextTokens("0"), "invalid");
  assert.equal(parseContextTokens("-1"), "invalid");
  assert.equal(parseContextTokens("8k"), "invalid");
  assert.equal(parseContextTokens("1.5"), "invalid");
});

test("返す分の上限の欄（T4-10）：読み方は文脈の長さと同じ。文脈の長さ以上なら知らせる", () => {
  assert.equal(parseOutputTokens(""), null);
  assert.equal(parseOutputTokens("16,384"), 16384);
  assert.equal(parseOutputTokens("0"), "invalid");
  assert.equal(outputTokensProblem(262144, 16384), null);
  assert.equal(outputTokensProblem(null, 16384), null);
  assert.equal(outputTokensProblem(8192, null), null);
  assert.equal(
    outputTokensProblem(8192, 8192),
    "返す分の上限（8,192）が文脈の長さ（8,192）以上です。文脈に使える分が残らないので、タスクを実行できません",
  );
});

test("構造化出力の欄（T4-2）：project.toml のまま・使う・使わない", () => {
  assert.equal(structuredChoice(null), "project");
  assert.equal(structuredChoice(true), "on");
  assert.equal(structuredChoice(false), "off");
  for (const choice of ["project", "on", "off"] as const) {
    assert.equal(structuredChoice(structuredValue(choice)), choice);
  }
});

const EMPTY: OverrideDraft = {
  url: "",
  model: "",
  tokens: "",
  output: "",
  structured: "project",
};

test("上書きの欄を離れたときに保存するか（T4-12、Z1）：変わっていなければ保存しない", () => {
  assert.deepEqual(overrideSave(connection({}), EMPTY), { kind: "unchanged" });
  const overridden = connection({
    override_base_url: "http://127.0.0.1:1234/v1",
    override_context_tokens: 32768,
    override_structured_output: true,
  });
  assert.deepEqual(
    overrideSave(overridden, {
      ...EMPTY,
      url: "  http://127.0.0.1:1234/v1 ",
      tokens: "32,768",
      structured: "on",
    }),
    { kind: "unchanged" },
  );
});

test("上書きの欄を離れたときに保存するか：保存する値（空は上書きなし、前後の空白を除く）", () => {
  assert.deepEqual(
    overrideSave(connection({}), {
      url: " http://127.0.0.1:1234/v1 ",
      model: "qwen3-32b",
      tokens: "２６２１４４",
      output: "16384",
      structured: "off",
    }),
    {
      kind: "save",
      values: {
        base_url: "http://127.0.0.1:1234/v1",
        model: "qwen3-32b",
        context_tokens: 262144,
        output_tokens: 16384,
        structured_output: false,
      },
    },
  );
  // 上書きをやめた欄（空にした）は null で保存する。
  assert.deepEqual(overrideSave(connection({ override_model: "old" }), EMPTY), {
    kind: "save",
    values: {
      base_url: null,
      model: null,
      context_tokens: null,
      output_tokens: null,
      structured_output: null,
    },
  });
});

test("上書きの欄を離れたときに保存するか：正しくない欄、返す分の上限が文脈の長さ以上なら保存しない", () => {
  assert.deepEqual(overrideSave(connection({}), { ...EMPTY, tokens: "abc" }), {
    kind: "invalid",
  });
  assert.deepEqual(overrideSave(connection({}), { ...EMPTY, output: "0" }), {
    kind: "invalid",
  });
  // 欄の文脈の長さと比べる。
  assert.deepEqual(
    overrideSave(connection({}), { ...EMPTY, tokens: "8192", output: "8192" }),
    { kind: "invalid" },
  );
  // 文脈の長さの欄が空なら、project.toml の値と比べる。
  assert.deepEqual(
    overrideSave(connection({ project_context_tokens: 4096 }), {
      ...EMPTY,
      output: "8192",
    }),
    { kind: "invalid" },
  );
});

test("返す分の上限の知らせ：保存したときに効く値で比べ、どちらの欄も空なら比べない", () => {
  // どちらの欄も空なら、project.toml だけの食い違いでもほかの上書きを止めない。
  assert.equal(
    overrideOutputProblem(
      connection({ project_context_tokens: 4096, project_output_tokens: 8192 }),
      { tokens: "", output: "" },
    ),
    null,
  );
  // 文脈の長さの欄だけ書いたら、project.toml の返す分と比べる。
  assert.notEqual(
    overrideOutputProblem(connection({ project_output_tokens: 8192 }), {
      tokens: "4096",
      output: "",
    }),
    null,
  );
  // 上書きしていなければ今の文脈の長さと比べ、上書きしていれば比べない（欄を空にすると今の値は変わるため）。
  assert.notEqual(
    overrideOutputProblem(connection({ context_tokens: 8192 }), {
      tokens: "",
      output: "8192",
    }),
    null,
  );
  assert.equal(
    overrideOutputProblem(
      connection({ context_tokens: 8192, override_context_tokens: 8192 }),
      { tokens: "", output: "8192" },
    ),
    null,
  );
});

test("上書きの欄を離れたときに保存するか：最後に保存を頼んだ値があれば、それと比べる", () => {
  const c = connection({});
  const draft: OverrideDraft = { ...EMPTY, model: "qwen3-32b" };
  const first = overrideSave(c, draft);
  assert.equal(first.kind, "save");
  const sent = first.kind === "save" ? first.values : null;
  // 接続先を取り直す前に同じ値で離れても、2回は書かない。
  assert.deepEqual(overrideSave(c, draft, sent), { kind: "unchanged" });
  // 取り直す前に前の値（空）へ打ち直したら、書く。
  assert.deepEqual(overrideSave(c, EMPTY, sent), {
    kind: "save",
    values: {
      base_url: null,
      model: null,
      context_tokens: null,
      output_tokens: null,
      structured_output: null,
    },
  });
});
