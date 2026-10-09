// 環境設定（左端のバーの「環境設定」。T3-5、D066）：AIの接続先。接続先は作品フォルダの project.toml の
// [ai.<名前>] に書き、URL・モデル・文脈の長さ（T4-1）・返す分の上限（T4-10）の上書き・クラウドへの送信の許可・
// キーは、この端末だけの設定にする。
// キーは入力をコアへ渡すだけで、画面には戻ってこない。

import { useState } from "react";
import type { AiConnection, AiConnections } from "../core/types.ts";
import {
  currentTest,
  outputTokensProblem,
  parseContextTokens,
  parseOutputTokens,
  placeLabel,
  readiness,
  replyText,
  type StructuredChoice,
  structuredChoice,
  structuredValue,
  type TestRecord,
  type TestState,
  usageText,
} from "./connection.ts";

interface Props {
  readonly data: AiConnections | null;
  readonly error: string | null;
  readonly tests: ReadonlyMap<string, TestRecord>;
  readonly onOverride: (
    name: string,
    baseUrl: string | null,
    model: string | null,
    contextTokens: number | null,
    outputTokens: number | null,
    structuredOutput: boolean | null,
  ) => Promise<void>;
  readonly onAllow: (endpoint: string, allowed: boolean) => Promise<void>;
  readonly onSaveKey: (endpoint: string, key: string) => Promise<void>;
  readonly onDeleteKey: (endpoint: string) => Promise<void>;
  readonly onTest: (name: string) => void;
  readonly onCancel: (name: string) => void;
}

export function PreferencesScreen({
  data,
  error,
  tests,
  onOverride,
  onAllow,
  onSaveKey,
  onDeleteKey,
  onTest,
  onCancel,
}: Props) {
  // 見せている（今の設定の）回だけを数える。前の設定の回が残っていても、ほかを押せなくしない。
  const running = (data?.connections ?? []).some(
    (c) => currentTest(c, tests)?.kind === "running",
  );
  return (
    <div className="preferences-body">
      <header className="export-head">
        <h1>環境設定</h1>
      </header>
      <section className="export-card" aria-label="AIの接続先">
        <div>
          <h2>AIの接続先</h2>
          <p className="export-note">
            接続先は、作品フォルダの project.toml の [ai.&lt;名前&gt;]
            に書きます（チームで共有します。AIタスクの model
            はこの名前を指します）。URL
            とモデルと文脈の長さの上書き、クラウドへ送ることの許可は、この端末だけの設定です。キーは
            OS のキーチェーンに置き、作品フォルダには書きません。既定の接続先は
            [ai.local]
            です。タスクごとの接続先は、この端末で、AI実行の画面で選べます。
          </p>
        </div>
        {error !== null && <p className="export-warning">{error}</p>}
        {data?.settings_problem && (
          <p className="export-warning">
            {data.settings_problem}。既定の設定で読みました。
          </p>
        )}
        {data?.key_problem && (
          <p className="export-warning">{data.key_problem}</p>
        )}
        {data !== null && data.connections.length === 0 && (
          <div className="export-info">
            <p className="ai-empty">
              接続先がありません。project.toml に次のように書きます（Ollama
              の例）。
            </p>
            <pre className="ai-preview">
              {`[ai.local]
provider = "openai-compatible"
base_url = "http://localhost:11434/v1"
model = "qwen3:8b"`}
            </pre>
          </div>
        )}
        {data?.connections.map((c) => (
          <ConnectionCard
            key={c.name}
            connection={c}
            test={currentTest(c, tests)}
            otherRunning={running && currentTest(c, tests)?.kind !== "running"}
            onOverride={onOverride}
            onAllow={onAllow}
            onSaveKey={onSaveKey}
            onDeleteKey={onDeleteKey}
            onTest={onTest}
            onCancel={onCancel}
          />
        ))}
      </section>
    </div>
  );
}

function ConnectionCard({
  connection: c,
  test,
  otherRunning,
  onOverride,
  onAllow,
  onSaveKey,
  onDeleteKey,
  onTest,
  onCancel,
}: {
  readonly connection: AiConnection;
  readonly test: TestState | undefined;
  readonly otherRunning: boolean;
  readonly onOverride: Props["onOverride"];
  readonly onAllow: Props["onAllow"];
  readonly onSaveKey: Props["onSaveKey"];
  readonly onDeleteKey: Props["onDeleteKey"];
  readonly onTest: Props["onTest"];
  readonly onCancel: Props["onCancel"];
}) {
  const [url, setUrl] = useState(c.override_base_url ?? "");
  const [model, setModel] = useState(c.override_model ?? "");
  const [tokens, setTokens] = useState(
    c.override_context_tokens === null ? "" : String(c.override_context_tokens),
  );
  const parsedTokens = parseContextTokens(tokens);
  const [output, setOutput] = useState(
    c.override_output_tokens === null ? "" : String(c.override_output_tokens),
  );
  const parsedOutput = parseOutputTokens(output);
  // 保存したときに効く文脈の長さと返す分の上限で比べる（欄が空なら project.toml の値。どちらも無ければ、今
  // 上書きしていないときだけ今の値。分からなければ比べない。返す分の既定は文脈の長さから決まるので、返す分を書いた
  // ときだけ比べる）。どちらの欄も空なら比べない（project.toml だけの食い違いで、ほかの上書きを止めない。実行は
  // コアが断る）。
  const outputProblem =
    tokens.trim() === "" && output.trim() === ""
      ? null
      : outputTokensProblem(
          parsedTokens === "invalid"
            ? null
            : (parsedTokens ??
                c.project_context_tokens ??
                (c.override_context_tokens === null ? c.context_tokens : null)),
          parsedOutput === "invalid"
            ? null
            : (parsedOutput ?? c.project_output_tokens),
        );
  const [structured, setStructured] = useState<StructuredChoice>(
    structuredChoice(c.override_structured_output),
  );
  // 構造化出力は OpenAI 互換だけ（T4-2）。
  const canStructure = c.provider === "openai-compatible";
  const [key, setKey] = useState("");
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const missing = readiness(c);
  const act = (work: () => Promise<void>, done: string) => {
    setBusy(true);
    work()
      .then(() => setMessage(done))
      .catch((e: unknown) => setMessage(String(e)))
      .finally(() => setBusy(false));
  };
  const overridden =
    c.override_base_url !== null ||
    c.override_model !== null ||
    c.override_context_tokens !== null ||
    c.override_output_tokens !== null ||
    c.override_structured_output !== null;
  const changed =
    url.trim() !== (c.override_base_url ?? "") ||
    model.trim() !== (c.override_model ?? "") ||
    parsedTokens !== c.override_context_tokens ||
    parsedOutput !== c.override_output_tokens ||
    structuredValue(structured) !== c.override_structured_output;
  const startTest = () => {
    setConfirming(false);
    setMessage(null);
    onTest(c.name);
  };
  return (
    <article
      className={c.cloud ? "ai-connection cloud" : "ai-connection"}
      aria-label={`接続先 ${c.name}`}
    >
      <div className="ai-connection-head">
        <h3>{c.name}</h3>
        {c.endpoint !== null && (
          <span className="ai-badge">{placeLabel(c)}</span>
        )}
        {c.is_default && <span className="ai-badge default">既定</span>}
        <span className="ai-provider">{c.provider}</span>
      </div>
      <dl className="ai-fields">
        <dt>URL</dt>
        <dd>
          <code>{c.base_url ?? "（なし）"}</code>
          {c.override_base_url !== null && (
            <span className="ai-note">
              この端末で上書き（project.toml：{c.project_base_url ?? "なし"}）
            </span>
          )}
          {c.url_hint !== null && (
            <span className="ai-url-hint">注意：{c.url_hint}</span>
          )}
        </dd>
        <dt>モデル</dt>
        <dd>
          <code>{c.model ?? "（なし）"}</code>
          {c.override_model !== null && (
            <span className="ai-note">
              この端末で上書き（project.toml：{c.project_model ?? "なし"}）
            </span>
          )}
        </dd>
        {canStructure && (
          <>
            <dt>構造化出力</dt>
            <dd>
              <code>{c.structured_output === true ? "使う" : "使わない"}</code>
              {c.override_structured_output !== null && (
                <span className="ai-note">
                  この端末で上書き（project.toml：
                  {c.project_structured_output === null
                    ? "なし"
                    : c.project_structured_output
                      ? "使う"
                      : "使わない"}
                  ）
                </span>
              )}
            </dd>
          </>
        )}
        <dt>文脈の長さ</dt>
        <dd>
          <code>
            {c.context_tokens === null
              ? "（なし）"
              : `${c.context_tokens.toLocaleString("ja-JP")} トークン`}
          </code>
          {c.override_context_tokens !== null && (
            <span className="ai-note">
              この端末で上書き（project.toml：
              {c.project_context_tokens?.toLocaleString("ja-JP") ?? "なし"}）
            </span>
          )}
        </dd>
        <dt>返す分の上限</dt>
        <dd>
          <code>
            {c.output_tokens === null
              ? "（なし）"
              : `${c.output_tokens.toLocaleString("ja-JP")} トークン`}
          </code>
          {c.override_output_tokens !== null ? (
            <span className="ai-note">
              この端末で上書き（project.toml：
              {c.project_output_tokens?.toLocaleString("ja-JP") ?? "なし"}）
            </span>
          ) : (
            c.project_output_tokens === null && (
              <span className="ai-note">
                既定（文脈の長さの4分の1、多くて4,096）
              </span>
            )
          )}
        </dd>
      </dl>
      {missing !== null && <p className="export-warning">{missing}</p>}
      <fieldset className="ai-override">
        <legend>この端末での上書き</legend>
        <label>
          <span>URL</span>
          <input
            type="text"
            value={url}
            placeholder={c.project_base_url ?? c.base_url ?? ""}
            spellCheck={false}
            onChange={(e) => setUrl(e.target.value)}
          />
        </label>
        <label>
          <span>モデル</span>
          <input
            type="text"
            value={model}
            placeholder={c.project_model ?? ""}
            spellCheck={false}
            onChange={(e) => setModel(e.target.value)}
          />
        </label>
        <label>
          <span>文脈の長さ（トークン）</span>
          <input
            type="text"
            inputMode="numeric"
            value={tokens}
            placeholder={
              c.project_context_tokens === null
                ? "既定"
                : String(c.project_context_tokens)
            }
            spellCheck={false}
            aria-invalid={parsedTokens === "invalid"}
            onChange={(e) => setTokens(e.target.value)}
          />
        </label>
        <label>
          <span>返す分の上限（トークン。考えるモデルは考える分も含む）</span>
          <input
            type="text"
            inputMode="numeric"
            value={output}
            placeholder={
              c.project_output_tokens === null
                ? "既定"
                : String(c.project_output_tokens)
            }
            spellCheck={false}
            aria-invalid={parsedOutput === "invalid" || outputProblem !== null}
            onChange={(e) => setOutput(e.target.value)}
          />
        </label>
        {canStructure && (
          <label>
            <span>構造化出力（JSON の形を指定）</span>
            <select
              value={structured}
              onChange={(e) =>
                setStructured(e.target.value as StructuredChoice)
              }
            >
              <option value="project">project.toml のまま</option>
              <option value="on">使う</option>
              <option value="off">使わない</option>
            </select>
          </label>
        )}
        {parsedTokens === "invalid" && (
          <span className="ai-field-problem">
            文脈の長さは正の整数で書きます（LM Studio
            などで読み込んだときの長さ）
          </span>
        )}
        {parsedOutput === "invalid" && (
          <span className="ai-field-problem">
            返す分の上限は正の整数で書きます（考えるモデルでは 16384 など）
          </span>
        )}
        {outputProblem !== null && (
          <span className="ai-field-problem">{outputProblem}</span>
        )}
        <div className="ai-actions">
          <button
            type="button"
            className="small-button"
            disabled={
              busy ||
              !changed ||
              parsedTokens === "invalid" ||
              parsedOutput === "invalid" ||
              outputProblem !== null
            }
            onClick={() =>
              act(
                () =>
                  onOverride(
                    c.name,
                    url.trim() === "" ? null : url.trim(),
                    model.trim() === "" ? null : model.trim(),
                    parsedTokens === "invalid" ? null : parsedTokens,
                    parsedOutput === "invalid" ? null : parsedOutput,
                    structuredValue(structured),
                  ),
                "上書きを保存しました",
              )
            }
          >
            上書きを保存
          </button>
          {overridden && (
            <button
              type="button"
              className="small-button"
              disabled={busy}
              onClick={() => {
                setUrl("");
                setModel("");
                setTokens("");
                setOutput("");
                setStructured("project");
                act(
                  () => onOverride(c.name, null, null, null, null, null),
                  "上書きをやめました",
                );
              }}
            >
              上書きをやめる
            </button>
          )}
        </div>
      </fieldset>
      {c.endpoint !== null && (
        <fieldset className="ai-key">
          <legend>
            キー
            {c.needs_key
              ? ""
              : c.cloud
                ? "（クラウドの多くは要ります）"
                : "（任意。ローカルのサーバーでは多くは不要）"}
          </legend>
          <p className="ai-key-state">
            {c.has_key ? "キーチェーンに保存してあります" : "ありません"}
          </p>
          <label>
            <span>新しいキー</span>
            <input
              type="password"
              value={key}
              autoComplete="off"
              spellCheck={false}
              onChange={(e) => setKey(e.target.value)}
            />
          </label>
          <div className="ai-actions">
            <button
              type="button"
              className="small-button"
              disabled={busy || key.trim() === ""}
              onClick={() => {
                const endpoint = c.endpoint;
                if (endpoint === null) return;
                const value = key;
                setKey("");
                act(
                  () => onSaveKey(endpoint, value),
                  "キーをキーチェーンに保存しました",
                );
              }}
            >
              キーを保存
            </button>
            {c.has_key && (
              <button
                type="button"
                className="small-button danger"
                disabled={busy}
                onClick={() => {
                  const endpoint = c.endpoint;
                  if (endpoint === null) return;
                  act(() => onDeleteKey(endpoint), "キーを消しました");
                }}
              >
                キーを消す
              </button>
            )}
          </div>
        </fieldset>
      )}
      {c.cloud && c.endpoint !== null && (
        <label className="ai-allow">
          <input
            type="checkbox"
            checked={c.allowed}
            disabled={busy}
            onChange={(e) => {
              const endpoint = c.endpoint;
              if (endpoint === null) return;
              const allowed = e.target.checked;
              act(
                () => onAllow(endpoint, allowed),
                allowed
                  ? "この行き先へ送ることを許可しました"
                  : "許可をやめました",
              );
            }}
          />
          <span>
            この端末から、この行き先（<code>{c.endpoint}</code>
            ）へ作品の中身を送ることを許可する
          </span>
        </label>
      )}
      <div className="ai-test">
        {test?.kind === "running" ? (
          <>
            <span className="ai-test-state">試しています…</span>
            <button
              type="button"
              className="small-button"
              onClick={() => onCancel(c.name)}
            >
              取り消す
            </button>
          </>
        ) : (
          <button
            type="button"
            className="primary-small"
            disabled={busy || missing !== null || otherRunning || confirming}
            onClick={() => (c.cloud ? setConfirming(true) : startTest())}
          >
            接続を試す
          </button>
        )}
        {test?.kind === "ok" && (
          <span className="ai-test-state ok">
            {replyText(test.result)} · {usageText(test.result)}
          </span>
        )}
        {test?.kind === "failed" && (
          <span className="ai-test-state failed">{test.message}</span>
        )}
        {test?.kind === "cancelled" && (
          <span className="ai-test-state">取り消しました</span>
        )}
      </div>
      {confirming && c.test_preview !== null && (
        <section className="ai-confirm" aria-label="送る中身の確認">
          <p>
            クラウドへ次の中身を送ります（作品の中身は入りません。キーは表示しません）。
          </p>
          <pre className="ai-preview">{c.test_preview}</pre>
          <div className="ai-actions">
            <button type="button" className="primary-small" onClick={startTest}>
              送る
            </button>
            <button
              type="button"
              className="small-button"
              onClick={() => setConfirming(false)}
            >
              やめる
            </button>
          </div>
        </section>
      )}
      {!confirming && c.test_preview !== null && !c.cloud && (
        <details className="ai-preview-details">
          <summary>「接続を試す」で送る中身</summary>
          <pre className="ai-preview">{c.test_preview}</pre>
        </details>
      )}
      {message !== null && <p className="export-notice">{message}</p>}
    </article>
  );
}
