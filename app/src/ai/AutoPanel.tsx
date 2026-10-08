// 自律の実行（T3-9、D070。docs/ui/mockups/07-ai.html）：章を選んで、スタブのシーンを上から順に下書きにする
// （展開メモ → 本文 → 点検 → 直す → 書き込む）。進み具合（シーンごとの段、トークン数）、生成したシーンと点検の結果、
// Git のブランチ（差分を見る・取り込む・破棄）を見せる。実行と書き込みはコア（`ai_auto`）。

import { useState } from "react";
import type {
  AiBranches,
  AiTaskInfo,
  AutoResult,
  SceneSummary,
} from "../core/types.ts";
import { type AutoRun, autoBranchText, autoSceneText } from "./model.ts";

interface Props {
  readonly info: AiTaskInfo;
  /** 点検に使うタスク（`checks`。送る先と中身を確かめる）。 */
  readonly checks: readonly AiTaskInfo[];
  readonly scenes: readonly SceneSummary[];
  readonly run: AutoRun | null;
  readonly result: AutoResult | null;
  readonly error: string | null;
  /** ほかの実行（自律を含む）が走っている。 */
  readonly busy: boolean;
  /** 実行できない理由（接続先が無い、クラウドを許可していない）。 */
  readonly blocked: string | null;
  readonly branches: AiBranches | null;
  readonly onRun: (
    info: AiTaskInfo,
    file: string,
    branch: boolean,
    confirmedCloud: boolean,
  ) => void;
  readonly onCancel: () => void;
  readonly onDiff: (
    name: string,
  ) => Promise<{ base: string; stat: string; patch: string }>;
  readonly onMerge: (name: string) => Promise<void>;
  /** 捨てる（人が確かめなければ false）。 */
  readonly onDelete: (name: string) => Promise<boolean>;
}

export function AutoPanel({
  info,
  checks,
  scenes,
  run,
  result,
  error,
  busy,
  blocked,
  branches,
  onRun,
  onCancel,
  onDiff,
  onMerge,
  onDelete,
}: Props) {
  // スタブのある章（スタブの数）。
  const chapters = [...new Set(scenes.map((s) => s.file))].map((file) => ({
    file,
    stubs: scenes.filter((s) => s.file === file && s.is_stub).length,
  }));
  const [file, setFile] = useState<string>(
    chapters.find((c) => c.stubs > 0)?.file ?? chapters[0]?.file ?? "",
  );
  const repo = branches?.repo === true;
  const [branch, setBranch] = useState(true);
  const [confirmed, setConfirmed] = useState(false);
  // クラウドへ送るタスク（自律と、点検のタスク）。
  const cloudTasks = [info, ...checks].filter(
    (t) => t.connection?.cloud === true,
  );
  const cloud = cloudTasks.length > 0;
  const stubs = chapters.find((c) => c.file === file)?.stubs ?? 0;
  const titles = new Map(scenes.map((s) => [s.id, s.title]));
  const running = run !== null && run.task === info.task.id;
  return (
    <section className="ai-auto" aria-label="自律の実行">
      <div className="ai-auto-settings">
        <label className="ai-scene-select">
          <span>対象の章</span>
          <select
            value={file}
            disabled={running}
            onChange={(e) => setFile(e.target.value)}
          >
            {chapters.map((c) => (
              <option key={c.file} value={c.file}>
                {c.file}（スタブ {c.stubs}）
              </option>
            ))}
          </select>
        </label>
        <label className="ai-allow">
          <input
            type="checkbox"
            checked={repo && branch}
            disabled={!repo || running}
            onChange={(e) => setBranch(e.target.checked)}
          />
          <span>
            Git のブランチで実行し、確かめてから取り込む
            {!repo &&
              "（作品フォルダが Git の作業ツリーではないので、作品フォルダに直接書きます）"}
          </span>
        </label>
        {cloud && (
          <label className="ai-allow">
            <input
              type="checkbox"
              checked={confirmed}
              disabled={running}
              onChange={(e) => setConfirmed(e.target.checked)}
            />
            <span>
              クラウドへ、スタブのシーンごとに次の文脈を、展開メモ・本文・点検・直しのたびに送ることを確かめました：
              {cloudTasks.map((t) => (
                <span key={t.task.id} className="ai-auto-cloud-task">
                  {t.task.name}（{t.connection?.name} · {t.connection?.model}
                  ）：
                  {t.task.context.map((c) => c.kind).join("、")}
                  {t.task.id === info.task.id
                    ? "と今のシーン、前のシーンまでの要約"
                    : "と下書きを入れた今のシーン"}
                </span>
              ))}
            </span>
          </label>
        )}
        <p className="ai-scope-note">
          書き込む範囲：スタブのシーン（台詞も地の文も無いシーン）だけ。人が書いた行は変更しません。書き込んだシーンは
          status: ai-draft
          になります。保存していない変更とコミットしていない変更があれば始めません。
        </p>
        <div className="ai-run-actions">
          {running ? (
            <button type="button" className="small-button" onClick={onCancel}>
              取り消す（次の段の合間で止まります）
            </button>
          ) : (
            <button
              type="button"
              className="primary-small ai-run-button"
              disabled={
                busy || blocked !== null || stubs === 0 || (cloud && !confirmed)
              }
              onClick={() => {
                onRun(info, file, repo && branch, cloud && confirmed);
                // 確かめたのはこの回だけ。
                setConfirmed(false);
              }}
            >
              自律で実行
            </button>
          )}
          {stubs === 0 && !running && (
            <span className="context-faint">この章にスタブはありません</span>
          )}
        </div>
      </div>
      {error !== null && <p className="export-warning">{error}</p>}
      {running && run !== null && (
        <div className="ai-auto-progress" role="status">
          <p className="context-faint">
            トークン {run.tokens.toLocaleString("ja-JP")}
            {run.budget !== null
              ? ` / ${run.budget.toLocaleString("ja-JP")}`
              : ""}
          </p>
          {run.budget !== null && (
            <div
              className="ai-progressbar"
              role="progressbar"
              aria-label="トークン使用量"
              aria-valuemin={0}
              aria-valuemax={run.budget}
              aria-valuenow={run.tokens}
            >
              <div
                style={{
                  width: `${Math.min(100, (run.tokens / run.budget) * 100)}%`,
                }}
              />
            </div>
          )}
          <ol className="ai-auto-scenes">
            {run.planned.map((scene) => (
              <li key={scene}>
                <span className="context-mono">{scene}</span>{" "}
                {titles.get(scene) ?? ""}
                <span className="context-faint">
                  {" "}
                  · {autoSceneText(run, scene)}
                </span>
              </li>
            ))}
          </ol>
        </div>
      )}
      {result !== null && result.outcome.task === info.task.id && !running && (
        <div className="ai-auto-result">
          <h2 className="context-heading">生成したシーン</h2>
          {result.outcome.stopped !== null && (
            <p className="export-warning">
              途中で止めました：{result.outcome.stopped}
            </p>
          )}
          <table className="ai-auto-table">
            <thead>
              <tr>
                <th scope="col">シーン</th>
                <th scope="col">文字数</th>
                <th scope="col">状態</th>
                <th scope="col">点検</th>
              </tr>
            </thead>
            <tbody>
              {result.outcome.scenes.map((s) => (
                <tr key={s.scene}>
                  <td>
                    <span className="context-mono">{s.scene}</span> {s.title}
                  </td>
                  <td>
                    {s.status === "written"
                      ? s.chars.toLocaleString("ja-JP")
                      : "—"}
                  </td>
                  <td>
                    {s.status === "written"
                      ? "AI下書き"
                      : s.status === "failed"
                        ? "失敗"
                        : "書き込まず"}
                  </td>
                  <td>
                    {s.status !== "written"
                      ? (s.message ?? "")
                      : s.issues.length === 0
                        ? s.iterations === 0
                          ? "問題なし"
                          : `${s.iterations}回直して 問題なし`
                        : `残り ${s.issues.length}件：${s.issues.join("／")}`}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="context-faint">
            トークン {result.outcome.tokens.toLocaleString("ja-JP")}
            {result.outcome.budget !== null
              ? ` / ${result.outcome.budget.toLocaleString("ja-JP")}`
              : ""}
          </p>
          {result.branch !== null && (
            <p
              className={
                result.branch.problem === null
                  ? "export-info"
                  : "export-warning"
              }
            >
              {autoBranchText(result.branch)}
            </p>
          )}
        </div>
      )}
      <BranchList
        branches={branches}
        onDiff={onDiff}
        onMerge={onMerge}
        onDelete={onDelete}
      />
    </section>
  );
}

function BranchList({
  branches,
  onDiff,
  onMerge,
  onDelete,
}: {
  readonly branches: AiBranches | null;
  readonly onDiff: Props["onDiff"];
  readonly onMerge: Props["onMerge"];
  readonly onDelete: Props["onDelete"];
}) {
  const [diff, setDiff] = useState<{
    name: string;
    base: string;
    stat: string;
    patch: string;
  } | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  if (branches === null || !branches.repo || branches.branches.length === 0)
    return null;
  const act = (work: () => Promise<boolean | undefined>, done: string) => {
    setBusy(true);
    work()
      .then((ok) => {
        if (ok === false) return;
        setMessage(done);
        setDiff(null);
      })
      .catch((e: unknown) => setMessage(String(e)))
      .finally(() => setBusy(false));
  };
  return (
    <div className="ai-branches">
      <h2 className="context-heading">AIのブランチ</h2>
      <ul className="ai-branch-list">
        {branches.branches.map((name) => (
          <li key={name}>
            <code>{name}</code>
            <button
              type="button"
              className="small-button"
              disabled={busy}
              onClick={() => {
                setBusy(true);
                onDiff(name)
                  .then((d) => setDiff({ name, ...d }))
                  .catch((e: unknown) => setMessage(String(e)))
                  .finally(() => setBusy(false));
              }}
            >
              差分を見る
            </button>
            <button
              type="button"
              className="small-button"
              disabled={busy || diff?.name !== name}
              title="差分を見てから取り込みます"
              onClick={() =>
                act(
                  () => onMerge(name).then(() => true),
                  `${name} を ${branches.current ?? "今のブランチ"} に取り込みました`,
                )
              }
            >
              {branches.current ?? "今のブランチ"}に取り込む
            </button>
            <button
              type="button"
              className="small-button danger"
              disabled={busy}
              onClick={() => act(() => onDelete(name), `${name} を捨てました`)}
            >
              破棄
            </button>
          </li>
        ))}
      </ul>
      {message !== null && <p className="export-notice">{message}</p>}
      {diff !== null && (
        <div className="ai-branch-diff">
          <p className="context-faint">
            {diff.base} との差分：<code>{diff.name}</code>
          </p>
          <pre className="ai-preview">{diff.stat || "（差分なし）"}</pre>
          <details>
            <summary>差分の本文</summary>
            <pre className="ai-preview">{diff.patch}</pre>
          </details>
        </div>
      )}
    </div>
  );
}
