// 企画の画面の「AIの問い返し」（T3-10、D071。docs/ui/mockups/01-ideas.html の右端）：企画書とメモの空白・矛盾への
// 質問（指摘のタスク `concept_questions`）と、未整理のメモの分類案（`idea_sort`）。質問は「メモにする」で
// アイデアメモに残せる。分類案の「当てる」はメモの状態とタグを書き換える（直したキーの行だけ。コアが書く）。
// ルートの骨格はAI実行の画面で作る（対象のルートと書く章を選ぶため）。

import { useState } from "react";
import { type FieldDecision, FieldProposals } from "../ai/FieldProposals.tsx";
import {
  connectionBlocker,
  type FindingRecord,
  type TaskRunner,
} from "../ai/model.ts";
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
  /** 結果を閉じる（この画面で見せなくなる。AI実行の画面の履歴には残る。T4-27）。 */
  readonly onDismiss: (run: number) => void;
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
    <aside className="concept-ai ctx-column" aria-label="AI">
      <div className="panel-head">
        <h2 className="h-label">AI</h2>
      </div>
      <section className="concept-ai-task" aria-label="問い返す">
        <h3 className="concept-ai-title">問い返す</h3>
        <p className="t-small t-2">
          企画書とメモの空白・矛盾を、質問で返します
        </p>
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
      </section>
      <section className="concept-ai-task" aria-label="企画書の下書き">
        <h3 className="concept-ai-title">企画書の下書き</h3>
        <p className="t-small t-2">
          メモから、企画書の空いている項目を書きます。書いてある項目には案だけを出します
        </p>
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
            <div className="ai-result-head">
              <p className={draftResult.ok ? "notice" : "notice notice-warn"}>
                {draftResult.text}
              </p>
              <button
                type="button"
                className="btn btn-sm"
                onClick={() => draft.onDismiss(draftResult.run)}
              >
                結果を閉じる
              </button>
            </div>
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
      </section>
      <section className="concept-ai-task" aria-label="メモの分類案">
        <h3 className="concept-ai-title">メモの分類案</h3>
        <p className="t-small t-2">未整理のメモに、状態とタグの案を付けます</p>
        <QuickRun
          runner={sort}
          scene={null}
          label="分類案を出す"
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
      </section>
      {onSkeleton !== null && (
        <section className="concept-ai-task" aria-label="ルートの骨格">
          <h3 className="concept-ai-title">ルートの骨格</h3>
          <p className="t-small t-2">
            エンディングとキャラの葛藤から、スタブを並べます
          </p>
          <button
            type="button"
            className="btn btn-link btn-sm concept-ai-link"
            onClick={onSkeleton}
          >
            AI実行で開く
          </button>
        </section>
      )}
      {[questions, draft.runner, sort].some(
        (r) => r.info !== null && connectionBlocker(r.info) !== null,
      ) && (
        <p className="t-caption t-3 concept-ai-foot">
          鍵の印は、まだ使えない接続先（クラウドへ送ることを許可していない、キーが無いなど）の操作です。環境設定の「AIの接続」で変えられます。
        </p>
      )}
    </aside>
  );
}
