// 環境設定（左端のバーの「環境設定」。T3-5、D066）：頁を左の一覧で選ぶ（見本 11。T4-25）。AIの接続先は作品
// フォルダの project.toml の [ai.<名前>] に書き、URL・モデル・文脈の長さ（T4-1）・返す分の上限（T4-10）の上書き・
// クラウドへの送信の許可・キーは、この端末だけの設定にする。キーは入力をコアへ渡すだけで、画面には戻ってこない。
// ほかの頁（文字と表示、キーの割り当て、作品の情報、メッセージウィンドウ、エディタ）は `SettingsPages.tsx`。

import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { isComposingKey } from "../bible/fields.ts";
import {
  aiCliCheck,
  aiModels,
  confirm,
  projectSettings,
  setProjectSettings,
} from "../core/api.ts";
import type {
  AiConnection,
  AiConnections,
  AiVia,
  ProjectSettings,
  ProjectSettingsWritten,
  SceneSummary,
  SettingEdit,
} from "../core/types.ts";
import {
  type CliCheck,
  canListModels,
  cliCheckText,
  currentModelList,
  currentTest,
  type ModelList,
  modelListBusy,
  modelListOf,
  modelListText,
  type OverrideDraft,
  type OverrideValues,
  overrideOutputProblem,
  overrideSave,
  parseContextTokens,
  parseOutputTokens,
  placeLabel,
  readiness,
  replyText,
  type StructuredChoice,
  structuredChoice,
  type TestRecord,
  type TestState,
  usageText,
} from "./connection.ts";
import {
  DisplayPage,
  KeysPage,
  ProjectPage,
  type ViewPrefsValue,
} from "./SettingsPages.tsx";

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
  /** この端末での呼び方と CLI の場所（Messages API の接続先だけ。T4-26）。 */
  readonly onVia: (
    name: string,
    via: AiVia,
    cli: string | null,
  ) => Promise<void>;
  readonly onTest: (name: string) => void;
  readonly onCancel: (name: string) => void;
  /** 表示の好み（文字と表示の頁。T4-25）。 */
  readonly view: ViewPrefsValue;
  readonly onView: (next: Partial<ViewPrefsValue>) => void;
  /** シーン（開始シーンの選択肢）。 */
  readonly scenes: readonly SceneSummary[];
  /** 設定ファイルが外で変わった回数（project.toml を読み直す）。 */
  readonly settingsVersion: number;
  /** project.toml を書いた後（作品名・括弧・エディタの設定・解析の結果を入れる）。 */
  readonly onProjectWritten: (result: ProjectSettingsWritten) => void;
}

/** 頁（見本 11 の一覧のうち、今ある設定の頁）。 */
type Page = "display" | "ai" | "keys" | "project" | "window" | "editor";

const APP_PAGES: readonly (readonly [Page, string])[] = [
  ["display", "文字と表示"],
  ["ai", "AIの接続"],
  ["keys", "キーの割り当て"],
];
const PROJECT_PAGES: readonly (readonly [Page, string])[] = [
  ["project", "作品の情報"],
  ["window", "メッセージウィンドウ"],
  ["editor", "エディタ"],
];
const PAGE_NOTES: Readonly<Record<Page, string>> = {
  display: "すぐに使われます",
  ai: "変えるとすぐに使われます",
  keys: "見るだけ",
  project: "project.toml",
  window: "project.toml",
  editor: "project.toml",
};

export function PreferencesScreen({
  data,
  error,
  tests,
  onOverride,
  onAllow,
  onSaveKey,
  onDeleteKey,
  onVia,
  onTest,
  onCancel,
  view,
  onView,
  scenes,
  settingsVersion,
  onProjectWritten,
}: Props) {
  const [page, setPage] = useState<Page>("ai");
  // project.toml の値（開いたとき・外で変わったときに読み直し、書いたら書いた後の値にする。T4-25）。
  const [project, setProject] = useState<ProjectSettings | null>(null);
  const [projectError, setProjectError] = useState<string | null>(null);
  useEffect(() => {
    void settingsVersion;
    let live = true;
    projectSettings()
      .then((s) => {
        if (!live) return;
        setProject(s);
        setProjectError(null);
      })
      .catch((e: unknown) => {
        if (live) setProjectError(String(e));
      });
    return () => {
      live = false;
    };
  }, [settingsVersion]);
  const editProject = async (
    edits: readonly SettingEdit[],
  ): Promise<string | null> => {
    const result = await setProjectSettings(edits);
    setProject(result.settings);
    onProjectWritten(result);
    return result.edit.kind === "unsupported" ? result.edit.message : null;
  };
  // 見せている（今の設定の）回だけを数える。前の設定の回が残っていても、ほかを押せなくしない。
  const running = (data?.connections ?? []).some(
    (c) => currentTest(c, tests)?.kind === "running",
  );
  const title =
    [...APP_PAGES, ...PROJECT_PAGES].find(([p]) => p === page)?.[1] ?? "";
  const pageButton = ([p, label]: readonly [Page, string]) => (
    <button
      key={p}
      type="button"
      className={p === page ? "lrow is-selected" : "lrow"}
      aria-current={p === page ? "page" : undefined}
      onClick={() => setPage(p)}
    >
      <span className="name">{label}</span>
    </button>
  );
  return (
    <div className="preferences-body">
      <nav className="preferences-list list-column" aria-label="設定の頁">
        <div className="panel-head">
          <h2 className="h-label">このアプリ（すべての作品）</h2>
        </div>
        {APP_PAGES.map(pageButton)}
        <div className="panel-head prefs-group">
          <h2 className="h-label">この作品（project.toml）</h2>
        </div>
        {PROJECT_PAGES.map(pageButton)}
      </nav>
      <main className="preferences-main" aria-label={title}>
        <header className="work-head">
          <h1 className="h1">{title}</h1>
          <span className="t-small t-3">{PAGE_NOTES[page]}</span>
        </header>
        {page === "display" && (
          <section className="preferences-scroll" aria-label={title}>
            <DisplayPage view={view} onView={onView} />
          </section>
        )}
        {page === "keys" && (
          <section className="preferences-scroll" aria-label={title}>
            <KeysPage />
          </section>
        )}
        {(page === "project" || page === "window" || page === "editor") && (
          <section className="preferences-scroll" aria-label={title}>
            <ProjectPage
              key={page}
              kind={page}
              settings={project}
              error={projectError}
              scenes={scenes}
              onEdit={editProject}
            />
          </section>
        )}
        <section
          className="preferences-scroll"
          aria-label="AIの接続先"
          hidden={page !== "ai"}
        >
          <div>
            <p className="export-note prefs-lead">
              接続先は、作品フォルダの project.toml の [ai.&lt;名前&gt;]
              に書きます（チームで共有します。AIタスクの model
              はこの名前を指します）。URL
              とモデルと文脈の長さの上書き、クラウドへ送ることの許可は、この端末だけの設定です。キーは
              OS
              のキーチェーンに置き、作品フォルダには書きません。既定の接続先は
              [ai.local]
              です。タスクごとの接続先は、この端末で、AI実行の画面で選べます。
            </p>
          </div>
          {error !== null && <p className="notice notice-warn">{error}</p>}
          {data?.settings_problem && (
            <p className="notice notice-warn">
              {data.settings_problem}。既定の設定で読みました。
            </p>
          )}
          {data?.key_problem && (
            <p className="notice notice-warn">{data.key_problem}</p>
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
              otherRunning={
                running && currentTest(c, tests)?.kind !== "running"
              }
              onOverride={onOverride}
              onAllow={onAllow}
              onSaveKey={onSaveKey}
              onDeleteKey={onDeleteKey}
              onVia={onVia}
              onTest={onTest}
              onCancel={onCancel}
            />
          ))}
        </section>
      </main>
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
  onVia,
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
  readonly onVia: Props["onVia"];
  readonly onTest: Props["onTest"];
  readonly onCancel: Props["onCancel"];
}) {
  // Claude Code の CLI の場所と、確かめた様子（T4-26）。
  const [cli, setCli] = useState(c.cli ?? "");
  const [cliCheck, setCliCheck] = useState<CliCheck | null>(null);
  const viaClaude = c.via === "claude-code";
  const changeVia = (via: AiVia) => {
    if (via === c.via) return;
    setCliCheck(null);
    onVia(c.name, via, via === "claude-code" ? cli.trim() || null : null)
      .then(() =>
        setMessage(
          via === "claude-code"
            ? "この端末では Claude Code の CLI で呼びます"
            : "この端末では API キーで呼びます",
        ),
      )
      .catch((e: unknown) => setMessage(String(e)));
  };
  const commitCli = () => {
    const value = cli.trim();
    if (value === (c.cli ?? "")) return;
    setCliCheck(null);
    onVia(c.name, "claude-code", value === "" ? null : value)
      .then(() => setMessage("CLI の場所を保存しました"))
      .catch((e: unknown) => setMessage(String(e)));
  };
  const checkCli = () => {
    setCliCheck({ kind: "checking" });
    aiCliCheck(cli.trim() === "" ? null : cli.trim())
      .then((r) => setCliCheck({ kind: "found", ...r }))
      .catch((e: unknown) =>
        setCliCheck({ kind: "failed", message: String(e) }),
      );
  };
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
  const [structured, setStructured] = useState<StructuredChoice>(
    structuredChoice(c.override_structured_output),
  );
  const draft: OverrideDraft = { url, model, tokens, output, structured };
  const outputProblem = overrideOutputProblem(c, draft);
  // 構造化出力は OpenAI 互換だけ（T4-2）。
  const canStructure = c.provider === "openai-compatible";
  const [key, setKey] = useState("");
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  /** 最後に保存を頼んだ上書きの値（接続先を取り直す前の比べ先）。 */
  const sent = useRef<OverrideValues | null>(null);
  // 接続先に入っているモデルの一覧（T4-23、N4）。URL が変わったら前の一覧は見せない。
  // 一覧は、コアが頼んだ URL のものとして持つ（押したときに URL の欄から外れて上書きを保存しても、合う）。最後に
  // 頼んだものの返事だけを使う。
  const [modelList, setModelList] = useState<ModelList | null>(null);
  const modelRequest = useRef(0);
  const models = currentModelList(c, modelList);
  const loadModels = () => {
    const request = ++modelRequest.current;
    const base = c.base_url;
    setModelList({ kind: "loading", base });
    aiModels(c.name)
      .then((reply) => {
        if (request === modelRequest.current) setModelList(modelListOf(reply));
      })
      .catch((e: unknown) => {
        if (request === modelRequest.current)
          setModelList({ kind: "failed", base, message: String(e) });
      });
  };

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
  // 欄を離れたとき（変換中でない Enter、選んだとき）に保存する（T4-12、Z1）。正しくない欄があれば保存せずに
  // そう知らせる。ほかのボタン（接続を試す、上書きをやめる）を止めないよう、`busy` は立てない。
  const commit = (next: OverrideDraft = draft) => {
    const decision = overrideSave(c, next, sent.current);
    if (decision.kind === "invalid") {
      setMessage("正しくない欄があるので、上書きを保存していません");
      return;
    }
    if (decision.kind !== "save") return;
    const v = decision.values;
    sent.current = v;
    onOverride(
      c.name,
      v.base_url,
      v.model,
      v.context_tokens,
      v.output_tokens,
      v.structured_output,
    )
      .then(() => setMessage("上書きを保存しました"))
      .catch((e: unknown) => {
        sent.current = null;
        setMessage(String(e));
      });
  };
  const commitOnEnter = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== "Enter" || isComposingKey(event)) return;
    event.preventDefault();
    commit();
  };
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
          <span className="tag tag-ai">{placeLabel(c)}</span>
        )}
        {c.is_default && <span className="tag tag-accent">既定</span>}
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
      {missing !== null && <p className="notice notice-warn">{missing}</p>}
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
            onBlur={() => commit()}
            onKeyDown={commitOnEnter}
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
            onBlur={() => commit()}
            onKeyDown={commitOnEnter}
          />
        </label>
        {canListModels(c) && (
          <button
            type="button"
            className="btn btn-sm"
            disabled={modelListBusy(c, models)}
            onClick={loadModels}
          >
            {models === null ? "一覧を読む" : "一覧を読み直す"}
          </button>
        )}
        {models?.kind === "done" && models.names.length > 0 && (
          <label>
            <span>入っているモデル</span>
            <select
              value={models.names.includes(model) ? model : ""}
              onChange={(e) => {
                const next = e.target.value;
                if (next === "") return;
                setModel(next);
                commit({ ...draft, model: next });
              }}
            >
              <option value="">一覧から選ぶ</option>
              {models.names.map((name) => (
                <option key={name} value={name}>
                  {name}
                </option>
              ))}
            </select>
          </label>
        )}
        {models !== null && (
          <span
            className={
              models.kind === "failed" ? "ai-field-problem" : "ai-field-note"
            }
            role={models.kind === "failed" ? "alert" : undefined}
          >
            {modelListText(models)}
          </span>
        )}
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
            onBlur={() => commit()}
            onKeyDown={commitOnEnter}
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
            onBlur={() => commit()}
            onKeyDown={commitOnEnter}
          />
        </label>
        {canStructure && (
          <label>
            <span>構造化出力（JSON の形を指定）</span>
            <select
              value={structured}
              onChange={(e) => {
                const next = e.target.value as StructuredChoice;
                setStructured(next);
                commit({ ...draft, structured: next });
              }}
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
        <span className="ai-field-note">欄を離れると保存します</span>
        {overridden && (
          <div className="ai-actions">
            <button
              type="button"
              className="btn btn-sm"
              disabled={busy}
              onClick={() => {
                setUrl("");
                setModel("");
                setTokens("");
                setOutput("");
                setStructured("project");
                sent.current = {
                  base_url: null,
                  model: null,
                  context_tokens: null,
                  output_tokens: null,
                  structured_output: null,
                };
                act(
                  () => onOverride(c.name, null, null, null, null, null),
                  "上書きをやめました",
                );
              }}
            >
              上書きをやめる
            </button>
          </div>
        )}
      </fieldset>
      {c.claude_code && (
        <fieldset className="ai-override ai-via">
          <legend>使い方（この端末）</legend>
          <div className="seg">
            <button
              type="button"
              aria-pressed={!viaClaude}
              onClick={() => changeVia("api")}
            >
              APIキー
            </button>
            <button
              type="button"
              aria-pressed={viaClaude}
              onClick={() => changeVia("claude-code")}
            >
              Claude Code（CLI）
            </button>
          </div>
          {viaClaude && (
            <>
              <label>
                <span>CLI の場所</span>
                <input
                  type="text"
                  className="mono"
                  value={cli}
                  placeholder="自動で探す（claude）"
                  spellCheck={false}
                  onChange={(e) => setCli(e.target.value)}
                  onBlur={commitCli}
                  onKeyDown={(event) => {
                    if (event.key !== "Enter" || isComposingKey(event)) return;
                    event.preventDefault();
                    commitCli();
                  }}
                />
              </label>
              <div className="ai-actions">
                <button
                  type="button"
                  className="btn btn-sm"
                  disabled={cliCheck?.kind === "checking"}
                  onClick={checkCli}
                >
                  CLI を確かめる
                </button>
              </div>
              {cliCheck !== null && (
                <span
                  className={
                    cliCheck.kind === "failed"
                      ? "ai-field-problem"
                      : "ai-field-note"
                  }
                >
                  {cliCheckText(cliCheck)}
                </span>
              )}
            </>
          )}
          <span className="ai-field-note">
            {viaClaude
              ? "ログイン済みの Claude Code の CLI（claude -p）で呼びます。キーをこのアプリに保存しません。ツールは使わせません。送ることの許可と、送る中身の確かめは同じです"
              : "キーを OS のキーチェーンに保存して、API で呼びます"}
          </span>
        </fieldset>
      )}
      {c.endpoint !== null && !viaClaude && (
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
              className="btn btn-sm"
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
                className="btn btn-sm btn-danger"
                disabled={busy}
                onClick={() => {
                  const endpoint = c.endpoint;
                  if (endpoint === null) return;
                  // 消したキーは戻せないので、聞いてから消す（T4-12、U4）。
                  void confirm(
                    `接続先 ${c.name} のキーを、この端末のキーチェーンから消しますか？`,
                    "キーを消す",
                  ).then((ok) => {
                    if (ok)
                      act(() => onDeleteKey(endpoint), "キーを消しました");
                  });
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
              className="btn btn-sm"
              onClick={() => onCancel(c.name)}
            >
              取り消す
            </button>
          </>
        ) : (
          <button
            type="button"
            className="btn btn-primary"
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
            <button
              type="button"
              className="btn btn-primary"
              onClick={startTest}
            >
              送る
            </button>
            <button
              type="button"
              className="btn btn-sm"
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
      {message !== null && <p className="notice">{message}</p>}
    </article>
  );
}
