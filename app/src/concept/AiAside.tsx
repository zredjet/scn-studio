// 企画の画面の「AIの問い返し」（T3-10、D071。docs/ui/mockups/01-ideas.html の右端）：企画書とメモの空白・矛盾への
// 質問（指摘のタスク `concept_questions`）と、未整理のメモの分類案（`idea_sort`）。質問は「メモにする」で
// アイデアメモに残せる。分類案の「当てる」はメモの状態とタグを書き換える（直したキーの行だけ。コアが書く）。
// ルートの骨格はAI実行の画面で作る（対象のルートと書く章を選ぶため）。

import { useState } from "react";
import { type FieldDecision, FieldProposals } from "../ai/FieldProposals.tsx";
import type { FindingRecord, TaskRunner } from "../ai/model.ts";
import { QuickRun } from "../ai/QuickRun.tsx";
import type {
  FieldProposal,
  FieldsWritten,
  Idea,
  IdeaSuggestion,
} from "../core/types.ts";

/** 企画書の下書き（T4-4）：タスク、最後の回の結果、案の決め方と採用・却下。 */
export interface ConceptDraftAi {
  readonly runner: TaskRunner;
  readonly result: {
    readonly run: number;
    readonly text: string;
    readonly ok: boolean;
    readonly fields: FieldsWritten | null;
  } | null;
  readonly decisions: ReadonlyMap<string, FieldDecision>;
  readonly onAdopt: (
    run: number,
    proposal: FieldProposal,
    written: FieldsWritten,
  ) => Promise<string | null>;
  readonly onReject: (run: number, proposal: FieldProposal) => void;
}

interface Props {
  readonly questions: TaskRunner;
  readonly sort: TaskRunner;
  /** 問い返しの質問（最後の回）。 */
  readonly findings: readonly FindingRecord[];
  readonly onDismissFinding: (id: string) => void;
  /** 分類案（まだ当てても閉じてもいないもの）。 */
  readonly suggestions: readonly {
    readonly key: string;
    readonly suggestion: IdeaSuggestion;
  }[];
  readonly onHideSuggestion: (key: string) => void;
  readonly ideas: readonly Idea[];
  /** 質問をアイデアメモにする。誤りの文を返す。 */
  readonly onMakeMemo: (text: string) => Promise<string | null>;
  /** 分類案を当てる。誤りの文を返す。 */
  readonly onApply: (suggestion: IdeaSuggestion) => Promise<string | null>;
  /** AI実行の画面で、ルートの骨格のタスクを開く。 */
  readonly onSkeleton: (() => void) | null;
  /** 企画書の下書き（T4-4）。 */
  readonly draft: ConceptDraftAi;
}

export function AiAside({
  questions,
  sort,
  findings,
  onDismissFinding,
  suggestions,
  onHideSuggestion,
  ideas,
  onMakeMemo,
  onApply,
  onSkeleton,
  draft,
}: Props) {
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const act = (work: () => Promise<string | null>, done: () => void) => {
    setBusy(true);
    setProblem(null);
    work()
      .then((p) => {
        if (p === null) done();
        else setProblem(p);
      })
      .catch((e: unknown) => setProblem(String(e)))
      .finally(() => setBusy(false));
  };
  const running = questions.running ?? sort.running ?? draft.runner.running;
  const draftResult = draft.result;
  return (
    <aside className="concept-ai" aria-label="AIの問い返し">
      <div className="concept-ai-head">
        <h2>AIの問い返し</h2>
        <span className="context-faint">
          企画書とメモの空白・矛盾を、質問で返します
        </span>
      </div>
      <QuickRun
        runner={questions}
        scene={null}
        label={findings.length === 0 ? "問い返す" : "もう一度問い返す"}
        disabledReason={
          running !== null && running.task !== questions.info?.task.id
            ? "ほかのタスクを実行しています"
            : null
        }
      />
      {problem !== null && <p className="notice notice-warn">{problem}</p>}
      {findings.map((f) => (
        <div key={f.id} className="concept-ai-card">
          <span>{f.finding.message}</span>
          <div className="ai-actions">
            <button
              type="button"
              className="btn btn-sm"
              disabled={busy}
              onClick={() =>
                act(
                  () => onMakeMemo(f.finding.message),
                  () => onDismissFinding(f.id),
                )
              }
            >
              メモにする
            </button>
            <button
              type="button"
              className="btn btn-sm"
              onClick={() => onDismissFinding(f.id)}
            >
              閉じる
            </button>
          </div>
        </div>
      ))}
      <div className="concept-ai-more">
        <span className="concept-ai-label">企画書の下書き</span>
        <span className="context-faint">
          メモから、企画書の空いている項目を AI
          が書きます。書いてある項目への案は、採用を選べます
        </span>
        <QuickRun
          runner={draft.runner}
          scene={null}
          label="AIで下書き"
          disabledReason={
            running !== null && running.task !== draft.runner.info?.task.id
              ? "ほかのタスクを実行しています"
              : null
          }
        />
        {draftResult !== null && (
          <>
            <p className={draftResult.ok ? "notice" : "notice notice-warn"}>
              {draftResult.text}
            </p>
            {draftResult.fields !== null && (
              <FieldProposals
                written={draftResult.fields}
                run={draftResult.run}
                decisions={draft.decisions}
                onAdopt={(p) =>
                  draftResult.fields === null
                    ? Promise.resolve(null)
                    : draft.onAdopt(draftResult.run, p, draftResult.fields)
                }
                onReject={(p) => draft.onReject(draftResult.run, p)}
              />
            )}
          </>
        )}
      </div>
      <div className="concept-ai-more">
        <span className="concept-ai-label">ほかのAIタスク</span>
        <QuickRun
          runner={sort}
          scene={null}
          label="未整理メモの分類案を出す"
          disabledReason={
            running !== null && running.task !== sort.info?.task.id
              ? "ほかのタスクを実行しています"
              : null
          }
        />
        {suggestions.map(({ key, suggestion: s }) => {
          const idea = ideas.find((i) => i.file === s.file);
          return (
            <div key={key} className="concept-ai-card">
              <span>
                <b>{idea?.body.trim().split("\n")[0] ?? s.file}</b>
                <span className="context-faint">（{s.file}）</span>
              </span>
              <span>
                {s.status !== null &&
                  `状態：${idea?.status ?? "?"} → ${s.status}`}
                {s.status !== null && s.tags.length > 0 && "、"}
                {s.tags.length > 0 && `タグ：＋${s.tags.join("・")}`}
              </span>
              {s.reason !== null && (
                <span className="context-faint">理由：{s.reason}</span>
              )}
              <div className="ai-actions">
                <button
                  type="button"
                  className="btn btn-sm"
                  disabled={busy || idea === undefined}
                  onClick={() =>
                    act(
                      () => onApply(s),
                      () => onHideSuggestion(key),
                    )
                  }
                >
                  当てる
                </button>
                <button
                  type="button"
                  className="btn btn-sm"
                  onClick={() => onHideSuggestion(key)}
                >
                  閉じる
                </button>
              </div>
            </div>
          );
        })}
        {onSkeleton !== null && (
          <button
            type="button"
            className="btn btn-sm concept-ai-link"
            onClick={onSkeleton}
          >
            エンディングからルート骨格を提案（AI実行の画面）
          </button>
        )}
      </div>
    </aside>
  );
}
