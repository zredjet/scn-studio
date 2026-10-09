// AIの接続先の表示の判断（T3-5、D066）：使う前に足りないもの、タイトルバーの AI の様子（T4-17、U03）、試した結果の文。

import type { AiConnection, AiModels, AiTestResult } from "../core/types.ts";

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

/** 接続先の印の形（色だけで区別しない。U03）：● 接続中・○ つながらない／まだ・鍵 送ることを未許可。 */
export type ConnectionMark = "on" | "off" | "lock";

/** タイトルバーの AI の様子の、接続先1つ分（U03、D086：接続先は名前ごとに並べる）。 */
export interface ConnectionStatus {
  readonly name: string;
  /** ローカルかクラウドか。 */
  readonly place: string;
  readonly mark: ConnectionMark;
  /** 様子の言葉。 */
  readonly state: string;
  /** モデル・行き先・応答の速さ。 */
  readonly detail: string;
  /** 使う前に足りないもの（無ければ null）。 */
  readonly missing: string | null;
  readonly isDefault: boolean;
}

/** タイトルバーの AI の様子。 */
export interface AiStatus {
  readonly connections: readonly ConnectionStatus[];
  /** 実行中の数（AI実行と自律）。 */
  readonly running: number;
  /** ボタンのツールチップと読み上げ。 */
  readonly title: string;
}

/** 接続先1つの様子。使う前に足りないものを、試した結果より先に知らせる。 */
export function connectionStatus(
  c: AiConnection,
  tests: ReadonlyMap<string, TestRecord>,
): ConnectionStatus {
  const missing = readiness(c);
  const test = currentTest(c, tests);
  const [mark, state]: [ConnectionMark, string] =
    c.problem !== null
      ? ["off", "使えません"]
      : c.cloud && !c.allowed
        ? ["lock", "送ることを未許可"]
        : missing !== null
          ? ["off", missing]
          : test === undefined || test.kind === "cancelled"
            ? ["off", "まだ試していません"]
            : test.kind === "running"
              ? ["off", "試しています"]
              : test.kind === "failed"
                ? ["off", "つながらない"]
                : ["on", "接続中"];
  const parts = [c.model ?? "モデル未設定", c.base_url ?? "URL なし"];
  if (test?.kind === "ok")
    parts.push(`応答 ${(test.result.elapsed_ms / 1000).toFixed(2)}秒`);
  if (c.problem !== null) parts.push(c.problem);
  return {
    name: c.name,
    place: placeLabel(c),
    mark,
    state,
    detail: parts.join(" · "),
    missing,
    isDefault: c.is_default,
  };
}

/** タイトルバーの AI の様子（接続先は既定を先に、あとは書いた順）。 */
export function aiStatus(
  connections: readonly AiConnection[],
  tests: ReadonlyMap<string, TestRecord>,
  running: number,
): AiStatus {
  const list = [
    ...connections.filter((c) => c.is_default),
    ...connections.filter((c) => !c.is_default),
  ].map((c) => connectionStatus(c, tests));
  const parts =
    list.length === 0
      ? ["接続先がありません（project.toml の [ai.<名前>]）"]
      : list.map((c) => `${c.name} ${c.state}`);
  const title = `AIの接続：${parts.join("・")}${running > 0 ? `。実行中 ${running}` : ""}`;
  return { connections: list, running, title };
}

/**
 * 試した応答の文。空なら、終わった理由を添える。考えるモデル（T4-2）は、考えた中身を除いたことと、考える分で
 * 上限に達しても、つながっていることを添える。
 */
export function replyText(result: AiTestResult): string {
  const text = result.text.trim();
  const thought = result.thinking_chars.toLocaleString("ja-JP");
  if (text !== "")
    return result.thinking_chars > 0
      ? `応答「${text}」（考えた中身 ${thought} 字は除きました）`
      : `応答「${text}」`;
  if (result.thinking_chars > 0)
    return `応答（考えた中身 ${thought} 字だけで、答えの前に上限（${result.stop_reason ?? "なし"}）に達しました。つながっています。作品のタスクでは、文脈の長さを十分に取ります）`;
  return result.stop_reason === null
    ? "応答（空）"
    : `応答（空。終わった理由：${result.stop_reason}）`;
}

/** 構造化出力の欄（T4-2）：project.toml のまま（null）・使う・使わない。 */
export type StructuredChoice = "project" | "on" | "off";

export function structuredChoice(value: boolean | null): StructuredChoice {
  return value === null ? "project" : value ? "on" : "off";
}

export function structuredValue(choice: StructuredChoice): boolean | null {
  return choice === "project" ? null : choice === "on";
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

/** 返す分の上限の上書きの欄（T4-10）を読む。読み方は文脈の長さの欄と同じ。 */
export const parseOutputTokens = parseContextTokens;

/**
 * 返す分の上限が、文脈の長さ以上で文脈に使える分が残らないときの知らせ（T4-10）。どちらかが分からなければ
 * `null`。
 */
export function outputTokensProblem(
  contextTokens: number | null,
  outputTokens: number | null,
): string | null {
  if (contextTokens === null || outputTokens === null) return null;
  return outputTokens >= contextTokens
    ? `返す分の上限（${outputTokens.toLocaleString("ja-JP")}）が文脈の長さ（${contextTokens.toLocaleString("ja-JP")}）以上です。文脈に使える分が残らないので、タスクを実行できません`
    : null;
}

/** 「この端末での上書き」の欄の今の中身（T4-12）。 */
export interface OverrideDraft {
  readonly url: string;
  readonly model: string;
  readonly tokens: string;
  readonly output: string;
  readonly structured: StructuredChoice;
}

/** 上書きの欄の中身から、保存するときの値。 */
export interface OverrideValues {
  readonly base_url: string | null;
  readonly model: string | null;
  readonly context_tokens: number | null;
  readonly output_tokens: number | null;
  readonly structured_output: boolean | null;
}

/**
 * 返す分の上限が文脈の長さ以上なら、その知らせ（T4-10、R221）。保存したときに効く値で比べる（欄が空なら
 * project.toml の値。文脈の長さがどちらにも無ければ、今上書きしていないときだけ今の値。分からなければ比べない。
 * 返す分の既定は文脈の長さから決まるので、返す分を書いたときだけ比べる）。どちらの欄も空なら比べない
 * （project.toml だけの食い違いで、ほかの上書きを止めない。実行はコアが断る）。
 */
export function overrideOutputProblem(
  connection: AiConnection,
  draft: Pick<OverrideDraft, "tokens" | "output">,
): string | null {
  if (draft.tokens.trim() === "" && draft.output.trim() === "") return null;
  const tokens = parseContextTokens(draft.tokens);
  const output = parseOutputTokens(draft.output);
  return outputTokensProblem(
    tokens === "invalid"
      ? null
      : (tokens ??
          connection.project_context_tokens ??
          (connection.override_context_tokens === null
            ? connection.context_tokens
            : null)),
    output === "invalid" ? null : (output ?? connection.project_output_tokens),
  );
}

/**
 * 上書きの欄を離れたときに保存するか（T4-12、Z1）。今の上書きと同じなら `unchanged`、欄が正しくないか返す分の
 * 上限が文脈の長さ以上なら `invalid`（保存しない）、それ以外は保存する値。`sent`（最後に保存を頼んだ値）が
 * あれば、接続先の今の上書きではなくそれと比べる（保存した後、接続先を取り直す前に、同じ値を2回書いたり、
 * 打ち直した前の値を書きそびれたりしないように）。
 */
export function overrideSave(
  connection: AiConnection,
  draft: OverrideDraft,
  sent: OverrideValues | null = null,
):
  | { readonly kind: "unchanged" }
  | { readonly kind: "invalid" }
  | { readonly kind: "save"; readonly values: OverrideValues } {
  const tokens = parseContextTokens(draft.tokens);
  const output = parseOutputTokens(draft.output);
  if (
    tokens === "invalid" ||
    output === "invalid" ||
    overrideOutputProblem(connection, draft) !== null
  ) {
    return { kind: "invalid" };
  }
  const values: OverrideValues = {
    base_url: draft.url.trim() === "" ? null : draft.url.trim(),
    model: draft.model.trim() === "" ? null : draft.model.trim(),
    context_tokens: tokens,
    output_tokens: output,
    structured_output: structuredValue(draft.structured),
  };
  const base: OverrideValues = sent ?? {
    base_url: connection.override_base_url,
    model: connection.override_model,
    context_tokens: connection.override_context_tokens,
    output_tokens: connection.override_output_tokens,
    structured_output: connection.override_structured_output,
  };
  const same =
    values.base_url === base.base_url &&
    values.model === base.model &&
    values.context_tokens === base.context_tokens &&
    values.output_tokens === base.output_tokens &&
    values.structured_output === base.structured_output;
  return same ? { kind: "unchanged" } : { kind: "save", values };
}

/** 接続先に入っているモデルの一覧を読んだ様子（T4-23、N4）。`base` はコアが頼んだ URL（読んでいる間と、コアが
 * 解決できなかった失敗は、押したときの URL）。 */
export type ModelList =
  | { readonly kind: "loading"; readonly base: string | null }
  | {
      readonly kind: "done";
      readonly base: string;
      readonly names: readonly string[];
    }
  | {
      readonly kind: "failed";
      readonly base: string | null;
      readonly message: string;
    };

/** 一覧を読んでいて、読み直せないか（URL が変わったら、読み直せる。古い返事は捨てるので）。 */
export function modelListBusy(
  c: AiConnection,
  list: ModelList | null,
): boolean {
  return list?.kind === "loading" && list.base === c.base_url;
}

/** コアの返事を一覧の様子にする。 */
export function modelListOf(reply: AiModels): ModelList {
  return reply.names !== null
    ? { kind: "done", base: reply.base_url, names: reply.names }
    : { kind: "failed", base: reply.base_url, message: reply.error ?? "" };
}

/** モデルの一覧を読める接続先か（ローカルの OpenAI 互換だけ。クラウドへは頼まない。コアと同じ。model が無くても読める）。 */
export function canListModels(c: AiConnection): boolean {
  return !c.cloud && c.provider === "openai-compatible" && c.base_url !== null;
}

/** 今の URL の一覧（頼んだ URL が今の URL と違う一覧と失敗は見せない。読んでいる間は見せる。押したときの URL を
 * 保存してから頼むことがあるため）。 */
export function currentModelList(
  c: AiConnection,
  list: ModelList | null,
): ModelList | null {
  if (list === null) return null;
  if (list.kind === "loading") return list;
  return list.base === c.base_url ? list : null;
}

/** 一覧の知らせ（読んでいる・何個から選べる・無い・読めない）。 */
export function modelListText(list: ModelList): string {
  switch (list.kind) {
    case "loading":
      return "モデルの一覧を読んでいます";
    case "done":
      return list.names.length === 0
        ? "接続先にモデルがありません（Ollama は pull、LM Studio はダウンロードしてから読み直します）"
        : `この接続先に入っている ${list.names.length} 個から選べます`;
    case "failed":
      return `モデルの一覧を読めません：${list.message}`;
  }
}
