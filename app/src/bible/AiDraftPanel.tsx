// 設定DBの画面の「AIで下書き」（T4-5、D080）：足りない設定（未登録の話者、企画書の主要キャラ、参照先の無い参照）
// のエントリを作るか、選んだエントリの空いている項目を書く（タスク `draft_entries`）。書くのはコアで、値のある項目への
// 案は並べて採用・却下させる。

import { type FieldDecision, FieldProposals } from "../ai/FieldProposals.tsx";
import type { TaskRunner } from "../ai/model.ts";
import { QuickRun } from "../ai/QuickRun.tsx";
import type { FieldProposal, FieldsWritten } from "../core/types.ts";

/** 設定DBの画面の AI（T4-5）。 */
export interface BibleAi {
  /** 対象のエントリ（パス。null なら足りない設定をまとめて）で実行するタスク。 */
  readonly runner: (target: string | null) => TaskRunner;
  /** 最後の回の結果。 */
  readonly result: {
    readonly run: number;
    readonly text: string;
    readonly ok: boolean;
    readonly fields: readonly FieldsWritten[];
    /** 書かなかったもの・読まなかったもののわけ。 */
    readonly notes: readonly string[];
  } | null;
  readonly decisions: ReadonlyMap<string, FieldDecision>;
  readonly onAdopt: (
    run: number,
    proposal: FieldProposal,
    written: FieldsWritten,
  ) => Promise<string | null>;
  readonly onReject: (run: number, proposal: FieldProposal) => void;
}

/** 実行のボタン（対象のエントリか、まとめて）。 */
export function AiDraftRun({
  ai,
  target,
}: {
  readonly ai: BibleAi;
  readonly target: string | null;
}) {
  const runner = ai.runner(target);
  if (runner.info === null) return null;
  return (
    <QuickRun
      runner={runner}
      scene={null}
      label={target === null ? "足りない設定をAIで作る" : "空欄をAIで下書き"}
      disabledReason={
        runner.running !== null && runner.running.task !== runner.info.task.id
          ? "ほかのタスクを実行しています"
          : null
      }
    />
  );
}

/** 最後の回の結果（書いたものの文と、値のある項目への案）。`labels` は型の項目の名前（キー → 名前）。 */
export function AiDraftResult({
  ai,
  labels,
}: {
  readonly ai: BibleAi;
  readonly labels: Readonly<Record<string, string>>;
}) {
  const result = ai.result;
  if (result === null) return null;
  return (
    <section className="bible-ai-result" aria-label="AIの下書きの結果">
      <p className={result.ok ? "notice" : "notice notice-warn"}>
        {result.text}
      </p>
      {result.notes.map((n) => (
        <p key={n} className="context-faint ai-prepared-note">
          {n}
        </p>
      ))}
      {result.fields.map((written) => (
        <FieldProposals
          key={written.file}
          written={written}
          run={result.run}
          labels={labels}
          decisions={ai.decisions}
          onAdopt={(p) => ai.onAdopt(result.run, p, written)}
          onReject={(p) => ai.onReject(result.run, p)}
        />
      ))}
    </section>
  );
}
