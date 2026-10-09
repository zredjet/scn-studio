// 環境設定の AI の接続先（T3-5、D066）の判断のテスト：使う前に足りないもの、タイトルバーの表示、結果の文。

import assert from "node:assert/strict";
import { test } from "node:test";
import type { AiConnection } from "../src/core/types.ts";
import {
  currentTest,
  indicator,
  parseContextTokens,
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

test("構造化出力の欄（T4-2）：project.toml のまま・使う・使わない", () => {
  assert.equal(structuredChoice(null), "project");
  assert.equal(structuredChoice(true), "on");
  assert.equal(structuredChoice(false), "off");
  for (const choice of ["project", "on", "off"] as const) {
    assert.equal(structuredChoice(structuredValue(choice)), choice);
  }
});
