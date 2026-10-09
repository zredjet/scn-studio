// 提案ノードのカード（T3-10、D071。docs/ui/mockups/03-flow.html の「AIの提案」）：分岐案ごとに、足す選択肢と
// 行き先（今あるシーンか、新しいスタブ）、`@設定`、理由。採用はコア（`adopt_branch_proposal`）がスタブと選択肢を
// 書き足して保存し、出自を記録して検証する。却下は画面の中だけ（ファイルは書き換えない）。

import { useState } from "react";
import type { BranchProposal, SceneSummary } from "../core/types.ts";
import {
  branchTargetText,
  proposalLetter,
  type RunResult,
  resultKey,
} from "./model.ts";

interface Props {
  /** 分岐案のタスクの最後の結果。 */
  readonly result: RunResult | undefined;
  readonly scenes: readonly SceneSummary[];
  /** 採用・却下した案（`resultKey`）。 */
  readonly hidden: ReadonlySet<string>;
  /** 採用する。結果の文を返す。 */
  readonly onAdopt: (
    proposal: BranchProposal,
    key: string,
    model: string,
  ) => Promise<string>;
  readonly onReject: (key: string) => void;
}

export function BranchProposals({
  result,
  scenes,
  hidden,
  onAdopt,
  onReject,
}: Props) {
  const [messages, setMessages] = useState<ReadonlyMap<string, string>>(
    new Map(),
  );
  const [busy, setBusy] = useState(false);
  if (result === undefined || result.kind !== "ok") return null;
  const items = result.branches
    .map((p, i) => ({ p, i, key: resultKey(result.run, i) }))
    .filter(({ key }) => !hidden.has(key));
  const done = [...messages.entries()].filter(([key]) => hidden.has(key));
  if (items.length === 0 && done.length === 0) {
    return result.branches.length === 0 ? (
      <p className="context-faint">分岐案はありませんでした。</p>
    ) : null;
  }
  return (
    <div className="ai-branches-list">
      {done.map(([key, message]) => (
        <p key={key} className="notice">
          {message}
        </p>
      ))}
      {items.map(({ p, i, key }) => (
        <article key={key} className="ai-branch-card">
          <header className="ai-branch-head">
            <span className="ai-branch-letter">AI提案 {proposalLetter(i)}</span>
            <b>{p.name}</b>
          </header>
          <p className="ai-branch-effect">
            選択肢「{p.label}」→ {branchTargetText(p, scenes)}
          </p>
          {p.effects.map((e) => (
            <p key={e} className="ai-branch-effect context-mono">
              {e}
            </p>
          ))}
          {p.stubs.map((s) => (
            <p key={s.id} className="ai-branch-effect">
              新規スタブ {s.id}「{s.title}」
              {s.summary !== null && `：${s.summary}`}
            </p>
          ))}
          {p.reason !== null && (
            <p className="context-faint">理由：{p.reason}</p>
          )}
          {p.problems.map((problem) => (
            <p key={problem} className="notice notice-warn">
              {problem}
            </p>
          ))}
          {messages.get(key) !== undefined && (
            <p className="notice notice-warn">{messages.get(key)}</p>
          )}
          <div className="ai-actions">
            <button
              type="button"
              className="btn btn-primary"
              disabled={busy || p.problems.length > 0}
              onClick={() => {
                setBusy(true);
                onAdopt(p, key, result.model)
                  .then((message) =>
                    setMessages((m) => new Map(m).set(key, message)),
                  )
                  .finally(() => setBusy(false));
              }}
            >
              採用
            </button>
            <button
              type="button"
              className="btn btn-sm"
              disabled={busy}
              onClick={() => onReject(key)}
            >
              却下
            </button>
          </div>
        </article>
      ))}
      <p className="context-faint">
        採用すると、スタブと選択肢を本文に書き足し（人の行は変えません）、検証して、出自を記録します。
      </p>
    </div>
  );
}
