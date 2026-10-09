// ほかの画面（フロー図、企画の画面）から1つのAIタスクを実行するボタン（T3-10、D071）。実行はAI実行の画面と
// 同じく App が持つ（`TaskRunner`）。クラウドの接続先では、送る中身（節と見積もり）を見せて「送る」で送る
// （コアは確かめた中身の要約値と同じときだけ送る）。

import { useState } from "react";
import type { AiPrepared } from "../core/types.ts";
import {
  blocker,
  estimateText,
  progressText,
  STRUCTURED_NOTE,
  type TaskRunner,
  trimmedText,
} from "./model.ts";

interface Props {
  readonly runner: TaskRunner;
  /** 対象のシーン（文脈に今のシーンを入れるタスク）。 */
  readonly scene: string | null;
  readonly label: string;
  /** ほかの実行が走っているなど、押せないわけ。 */
  readonly disabledReason?: string | null;
}

export function QuickRun({ runner, scene, label, disabledReason }: Props) {
  const { info, running } = runner;
  const [prepared, setPrepared] = useState<AiPrepared | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [preparing, setPreparing] = useState(false);
  if (info === null) {
    return (
      <p className="context-faint">
        このためのAIタスク（ai/*.yaml）がありません。
      </p>
    );
  }
  const blocked = blocker(info);
  const thisRunning = running !== null && running.task === info.task.id;
  const cloud = info.connection?.cloud === true;
  // 対象が変わった後の見た中身は使わない。
  const shown =
    prepared !== null &&
    prepared.assembled.task === info.task.id &&
    prepared.assembled.scene === scene
      ? prepared
      : null;
  const start = () => {
    setError(null);
    if (!cloud) {
      runner.onRun(info, scene, null);
      return;
    }
    setPreparing(true);
    runner
      .onPrepare(info.task.id, scene)
      .then((p) => setPrepared(p))
      .catch((e: unknown) => setError(String(e)))
      .finally(() => setPreparing(false));
  };
  return (
    <div className="ai-quick">
      <div className="ai-run-actions">
        {thisRunning ? (
          <>
            <span className="ai-test-state" role="status">
              {info.task.name}を実行中 · {progressText(running)}
            </span>
            <button
              type="button"
              className="small-button"
              onClick={runner.onCancel}
            >
              取り消す
            </button>
          </>
        ) : (
          <button
            type="button"
            className="small-button ai-quick-button"
            disabled={
              blocked !== null ||
              running !== null ||
              preparing ||
              shown !== null ||
              (disabledReason ?? null) !== null
            }
            title={blocked ?? disabledReason ?? undefined}
            onClick={start}
          >
            {label}
          </button>
        )}
        <span className="context-faint">
          {info.connection === null
            ? "接続先なし"
            : `${info.connection.name}（${cloud ? "クラウド" : "ローカル"}）`}
        </span>
      </div>
      {blocked !== null && <p className="export-warning">{blocked}</p>}
      {error !== null && <p className="export-warning">{error}</p>}
      {shown !== null && (
        <div className="ai-confirm">
          <p>
            クラウド（{shown.connection.name} · {shown.connection.model}
            ）へ次の中身を送ります：
            {shown.assembled.sections.map((s) => s.title).join("、")}
          </p>
          <p className="context-faint">{estimateText(shown.assembled)}</p>
          {trimmedText(shown.assembled).map((t) => (
            <p key={t} className="context-faint ai-prepared-note">
              {t}
            </p>
          ))}
          {shown.connection.structured && (
            <p className="context-faint ai-prepared-note">{STRUCTURED_NOTE}</p>
          )}
          {shown.assembled.problem !== null && (
            <p className="export-warning">{shown.assembled.problem}</p>
          )}
          <details>
            <summary>指示（system）と依頼（user）</summary>
            <pre className="ai-preview">{shown.assembled.system}</pre>
            <pre className="ai-preview">{shown.assembled.user}</pre>
          </details>
          <div className="ai-actions">
            <button
              type="button"
              className="primary-small"
              disabled={shown.assembled.problem !== null || running !== null}
              onClick={() => {
                setPrepared(null);
                runner.onRun(info, scene, shown.digest);
              }}
            >
              送る
            </button>
            <button
              type="button"
              className="small-button"
              onClick={() => setPrepared(null)}
            >
              やめる
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
