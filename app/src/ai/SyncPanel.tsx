// 同期（T3-10、D071）：本文から作り直したシーン要約（`.cache/summaries.json`）と、メタ情報の summary（プロット
// カード）のずれを、差分（文字ごと）で見せる。「summary に書く」で summary の1行だけを書き換えて保存する（コアの
// `set_scene_summary`）。章のシーンの要約をまとめて作り直せる（コアの `ai_summarize`）。

import { useState } from "react";
import type {
  AiTaskInfo,
  SummarizeResult,
  SummaryDrift,
} from "../core/types.ts";
import { blocker, driftKey, type SummarizeRun } from "./model.ts";

interface Props {
  readonly info: AiTaskInfo;
  readonly chapters: readonly string[];
  readonly drift: readonly SummaryDrift[] | null;
  /** 章ごとの、要約を作り直すシーンの数。 */
  readonly pending: Readonly<Record<string, number>>;
  readonly driftError: string | null;
  /** 却下したずれ（シーンIDと案）。 */
  readonly hidden: ReadonlySet<string>;
  readonly running: SummarizeRun | null;
  /** ほかの実行が走っている。 */
  readonly busy: boolean;
  readonly result: SummarizeResult | null;
  readonly error: string | null;
  readonly onSummarize: (file: string, confirmedCloud: boolean) => void;
  readonly onCancel: () => void;
  /** summary に書く。誤りの文を返す（書けたら null）。 */
  readonly onAdopt: (drift: SummaryDrift) => Promise<string | null>;
  readonly onReject: (key: string) => void;
}

export function SyncPanel({
  info,
  chapters,
  drift,
  pending,
  driftError,
  hidden,
  running,
  busy,
  result,
  error,
  onSummarize,
  onCancel,
  onAdopt,
  onReject,
}: Props) {
  const [file, setFile] = useState(chapters[0] ?? "");
  const [confirmed, setConfirmed] = useState(false);
  const [problems, setProblems] = useState<ReadonlyMap<string, string>>(
    new Map(),
  );
  const [writing, setWriting] = useState(false);
  const cloud = info.connection?.cloud === true;
  const blocked = blocker(info);
  const items = (drift ?? []).filter((d) => !hidden.has(driftKey(d)));
  /** 選んだ章の、要約を作り直すシーンの数（送る回数）。 */
  const count = pending[file] ?? 0;
  return (
    <section className="ai-sync" aria-label="同期">
      <h2 className="context-heading">同期：要約のずれ</h2>
      <p className="context-faint ai-scope-note">
        本文から作り直したシーン要約と、メタ情報の
        summary（フロー図のカード）が違うシーンです。「summary
        に書く」で、そのシーンの summary の1行だけを書き換えて保存します。
      </p>
      <div className="ai-auto-settings">
        <label className="ai-scene-select">
          <span>章の要約をまとめて作り直す</span>
          <select
            value={file}
            disabled={running !== null}
            onChange={(e) => {
              setFile(e.target.value);
              // 確かめたのは、選んでいた章の送る回数。
              setConfirmed(false);
            }}
          >
            {chapters.map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </select>
        </label>
        {cloud && (
          <label className="ai-allow">
            <input
              type="checkbox"
              checked={confirmed}
              disabled={running !== null}
              onChange={(e) => setConfirmed(e.target.checked)}
            />
            <span>
              クラウド（{info.connection?.name} · {info.connection?.model}
              ）へ、{file} の要約を作り直す {count}{" "}
              シーンについて、シーンごとに文脈（
              {info.task.context.map((c) => c.kind).join("、")}
              ）を送る（{count}回）ことを確かめました
            </span>
          </label>
        )}
        <div className="ai-run-actions">
          {running !== null ? (
            <>
              <span className="ai-test-state" role="status">
                要約を作り直しています（{running.done}/{running.total}
                {running.scene !== null ? ` · ${running.scene}` : ""}）
              </span>
              <button type="button" className="small-button" onClick={onCancel}>
                取り消す
              </button>
            </>
          ) : (
            <button
              type="button"
              className="small-button"
              disabled={
                blocked !== null ||
                busy ||
                file === "" ||
                count === 0 ||
                (cloud && !confirmed)
              }
              onClick={() => {
                onSummarize(file, cloud && confirmed);
                setConfirmed(false);
              }}
            >
              {count === 0
                ? "作り直すシーンはありません"
                : `作り直す（要約の無いシーンと本文が変わったシーン：${count}）`}
            </button>
          )}
        </div>
        {error !== null && <p className="export-warning">{error}</p>}
        {result !== null && running === null && (
          <p
            className={
              result.stopped === null ? "export-info" : "export-warning"
            }
          >
            {result.total === 0
              ? "作り直すシーンはありませんでした。"
              : `${result.done.length}/${result.total} シーンの要約を作り直しました。`}
            {result.stopped !== null && ` ${result.stopped}`}
            {result.problems.map((p) => ` ${p}`).join("")}
          </p>
        )}
      </div>
      {driftError !== null && <p className="export-warning">{driftError}</p>}
      {drift !== null && items.length === 0 && (
        <p className="context-faint">ずれはありません。</p>
      )}
      {items.map((d) => {
        const key = driftKey(d);
        return (
          <article key={key} className="ai-proposal">
            <div className="ai-proposal-head">
              <span className="context-mono">{d.scene}</span>
              <span>{d.title}</span>
              <span className="context-faint">{d.file}</span>
            </div>
            <p className="ai-proposal-line before">
              <span className="ai-proposal-label">今</span>
              {d.current === null && (
                <span className="context-faint">（summary なし）</span>
              )}
              {d.diff.map((part, n) =>
                part.kind === "insert" ? null : part.kind === "delete" ? (
                  // biome-ignore lint/suspicious/noArrayIndexKey: 差分の一続きは並びの順だけで見分ける
                  <del key={n}>{part.text}</del>
                ) : (
                  // biome-ignore lint/suspicious/noArrayIndexKey: 差分の一続きは並びの順だけで見分ける
                  <span key={n}>{part.text}</span>
                ),
              )}
            </p>
            <p className="ai-proposal-line after">
              <span className="ai-proposal-label">案</span>
              {d.diff.map((part, n) =>
                part.kind === "delete" ? null : part.kind === "insert" ? (
                  // biome-ignore lint/suspicious/noArrayIndexKey: 差分の一続きは並びの順だけで見分ける
                  <ins key={n}>{part.text}</ins>
                ) : (
                  // biome-ignore lint/suspicious/noArrayIndexKey: 差分の一続きは並びの順だけで見分ける
                  <span key={n}>{part.text}</span>
                ),
              )}
            </p>
            {problems.get(key) !== undefined && (
              <p className="export-warning">{problems.get(key)}</p>
            )}
            <div className="ai-finding-actions">
              <button
                type="button"
                className="small-button"
                disabled={writing || running !== null}
                onClick={() => {
                  setWriting(true);
                  onAdopt(d)
                    .then((problem) =>
                      setProblems((m) => {
                        const next = new Map(m);
                        if (problem === null) next.delete(key);
                        else next.set(key, problem);
                        return next;
                      }),
                    )
                    .finally(() => setWriting(false));
                }}
              >
                summary に書く
              </button>
              <button
                type="button"
                className="small-button"
                disabled={writing}
                onClick={() => onReject(key)}
              >
                却下
              </button>
            </div>
          </article>
        );
      })}
    </section>
  );
}
