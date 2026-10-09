// AIの接続先の表示の判断（T3-5、D066）：使う前に足りないもの、タイトルバーの表示、試した結果の文。

import type { AiConnection, AiTestResult } from "../core/types.ts";

/** 「接続を試す」の様子。 */
export type TestState =
  | { readonly kind: "running" }
  | { readonly kind: "ok"; readonly result: AiTestResult }
  | { readonly kind: "failed"; readonly message: string }
  | { readonly kind: "cancelled" };

/** 試した結果と、試したときの接続先（行き先とモデル）。 */
export interface TestRecord {
  readonly target: string;
  readonly state: TestState;
}

/** 試した結果が今の接続先のものかを見分ける印（行き先とモデル）。 */
export function testTarget(connection: AiConnection): string {
  return `${connection.endpoint ?? ""}\n${connection.model ?? ""}`;
}

/** 今の接続先で試した結果。設定が変わった後の、前の設定の結果は返さない。 */
export function currentTest(
  connection: AiConnection,
  tests: ReadonlyMap<string, TestRecord>,
): TestState | undefined {
  const record = tests.get(connection.name);
  return record !== undefined && record.target === testTarget(connection)
    ? record.state
    : undefined;
}

export function placeLabel(connection: AiConnection): string {
  return connection.cloud ? "クラウド" : "ローカル";
}

/** 使う前に足りないもの。無ければ null。 */
export function readiness(connection: AiConnection): string | null {
  if (connection.problem !== null) return connection.problem;
  if (connection.cloud && !connection.allowed)
    return "この端末からクラウドへ送ることを許可していません";
  if (connection.needs_key && !connection.has_key) return "キーがありません";
  return null;
}

/** タイトルバーの接続の印。 */
export interface AiIndicator {
  readonly text: string;
  /** 印の形と色：試して応答があった・失敗した・試している・まだ試していない・使えない。 */
  readonly state: "ok" | "failed" | "running" | "unknown" | "unusable";
  /** 詳しい説明（ツールチップと読み上げ）。 */
  readonly detail: string;
}

const STATE_TEXT: Readonly<Record<AiIndicator["state"], string>> = {
  ok: "応答あり",
  failed: "失敗",
  running: "試しています",
  unknown: "まだ試していません",
  unusable: "使えません",
};

/** タイトルバーに出す、既定の接続先。接続先が無ければ「未設定」。 */
export function indicator(
  connections: readonly AiConnection[],
  tests: ReadonlyMap<string, TestRecord>,
): AiIndicator {
  const current = connections.find((c) => c.is_default);
  if (current === undefined)
    return {
      text: "AI：未設定",
      state: "unusable",
      detail: "project.toml に [ai.local] などの接続先がありません",
    };
  const label = current.model ?? current.name;
  const missing = readiness(current);
  const test = currentTest(current, tests);
  const state: AiIndicator["state"] =
    missing !== null
      ? "unusable"
      : test === undefined || test.kind === "cancelled"
        ? "unknown"
        : test.kind;
  const where = current.base_url ?? "URL なし";
  const parts = [
    `接続先 ${current.name}（${placeLabel(current)}）`,
    where,
    STATE_TEXT[state],
  ];
  if (missing !== null) parts.push(missing);
  return {
    text: `AI：${label}（${placeLabel(current)}）`,
    state,
    detail: parts.join(" · "),
  };
}

/** 試した応答の文。空なら、終わった理由を添える（考えるモデルが上限に達したときなど）。 */
export function replyText(result: AiTestResult): string {
  const text = result.text.trim();
  if (text !== "") return `応答「${text}」`;
  return result.stop_reason === null
    ? "応答（空）"
    : `応答（空。終わった理由：${result.stop_reason}）`;
}

/** 試した結果の数（トークン数と時間）。 */
export function usageText(result: AiTestResult): string {
  const { input_tokens, output_tokens, estimated } = result.usage;
  const seconds = (result.elapsed_ms / 1000).toFixed(1);
  return `入力 ${input_tokens}・出力 ${output_tokens} トークン${estimated ? "（文字数からの見積もり）" : ""} · ${seconds}秒`;
}

/**
 * 文脈の長さの上書きの欄（T4-1）を読む。空なら上書きなし（null）、正の整数でなければ `invalid`。
 * 「32,768」のような桁区切りと全角の数字も読む。
 */
export function parseContextTokens(text: string): number | null | "invalid" {
  const plain = text
    .trim()
    .replace(/[０-９]/g, (d) => String.fromCharCode(d.charCodeAt(0) - 0xfee0))
    .replace(/[,，]/g, "");
  if (plain === "") return null;
  if (!/^\d+$/.test(plain)) return "invalid";
  const n = Number(plain);
  return Number.isSafeInteger(n) && n > 0 ? n : "invalid";
}
