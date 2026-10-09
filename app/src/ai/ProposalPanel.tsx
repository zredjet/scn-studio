// 提案と行ごとの採用（T3-8、D069）：提案を今の本文と照らし（コアの `view_proposals`）、行ごとに文字の差分を見せて、
// 採用・却下する。採用はコア（`adopt_proposals`）が、採用できる行だけを字下げと行IDを残して置き換え、保存して、
// 出自を記録する。人の行を直接書き換えないために、採用はこの画面のボタンからだけ行う。

import { useEffect, useRef, useState } from "react";
import { viewProposals } from "../core/api.ts";
import type { AiProposalView } from "../core/types.ts";
import { AiMark } from "../shell/AiMark.tsx";
import {
  adoptableIndexes,
  type ProposalSet,
  proposalStatusText,
} from "./model.ts";

interface Props {
  readonly set: ProposalSet;
  /** 本文が変わった回数（照らし直す）。 */
  readonly version: number;
  /** 採用する（ファイルごとにまとめて保存する）。結果の文を返す。 */
  readonly onAdopt: (
    set: ProposalSet,
    views: readonly AiProposalView[],
  ) => Promise<string>;
  readonly onGoTo: (view: AiProposalView) => void;
  /** 却下した提案（まとまりの中の並びの番号）。画面を切り替えても忘れないよう、App が持つ。 */
  readonly rejected: ReadonlySet<number>;
  readonly onToggleReject: (setId: string, index: number) => void;
}

export function ProposalPanel({
  set,
  version,
  onAdopt,
  onGoTo,
  rejected,
  onToggleReject,
}: Props) {
  const [views, setViews] = useState<readonly AiProposalView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  /** 提案の中身（まとまりは `id` で見分ける。描くたびに作り直されるまとまりで照らし直さない）。 */
  const proposals = useRef(set.proposals);
  proposals.current = set.proposals;
  // まとまりが変わったら、知らせを忘れる（却下はまとまりの id ごとに App が持つ）。
  useEffect(() => {
    void set.id;
    setMessage(null);
    // 照らし直しが届くまで、前のまとまりのカードで採用させない。
    setViews(null);
  }, [set.id]);
  // 今の本文と照らす（まとまりが変わったとき、本文が変わったとき）。
  useEffect(() => {
    void set.id;
    void version;
    let live = true;
    viewProposals(proposals.current)
      .then((v) => {
        if (!live) return;
        setViews(v);
        setError(null);
      })
      .catch((e: unknown) => {
        if (live) setError(String(e));
      });
    return () => {
      live = false;
    };
  }, [set.id, version]);

  if (error !== null) return <p className="notice notice-warn">{error}</p>;
  if (views === null)
    return <p className="context-faint">提案を本文と照らしています…</p>;
  if (views.length === 0)
    return <p className="context-empty">提案はありません。</p>;
  const ready = adoptableIndexes(views, rejected);
  const adopt = (indexes: readonly number[]) => {
    const chosen = indexes.flatMap((i) => {
      const v = views[i];
      return v === undefined ? [] : [v];
    });
    if (chosen.length === 0) return;
    setBusy(true);
    setMessage(null);
    onAdopt(set, chosen)
      .then((m) => setMessage(m))
      .catch((e: unknown) => setMessage(String(e)))
      .finally(() => setBusy(false));
  };
  return (
    <section className="ai-proposals" aria-label="提案">
      <div className="ai-proposals-head">
        <span className="context-faint">
          提案 {views.length}件 · 採用できる {ready.length}件
        </span>
        <button
          type="button"
          className="btn"
          disabled={busy || ready.length === 0}
          onClick={() => adopt(ready)}
        >
          <AiMark />
          採用できるものをすべて採用
        </button>
      </div>
      {message !== null && <p className="notice">{message}</p>}
      {views.map((v, i) => {
        const isRejected = rejected.has(i);
        return (
          <article
            // biome-ignore lint/suspicious/noArrayIndexKey: 同じ行IDの提案が並ぶことがあるので、並びの番号で見分ける
            key={i}
            className={`ai-proposal ${v.status.kind}${isRejected ? " rejected" : ""}`}
            aria-label={`^${v.proposal.line} の提案`}
          >
            <div className="ai-proposal-head">
              <span className="context-mono">^{v.proposal.line}</span>
              {v.scene !== null && (
                <span className="context-faint">{v.scene}</span>
              )}
              <span className="ai-proposal-status">
                {isRejected ? "却下しました" : proposalStatusText(v)}
              </span>
            </div>
            <p className="ai-proposal-line before">
              <span className="ai-proposal-label">今</span>
              {v.diff.map((d, n) =>
                d.kind === "insert" ? null : d.kind === "delete" ? (
                  // biome-ignore lint/suspicious/noArrayIndexKey: 差分の一続きは並びの順だけで見分ける
                  <del key={n}>{d.text}</del>
                ) : (
                  // biome-ignore lint/suspicious/noArrayIndexKey: 差分の一続きは並びの順だけで見分ける
                  <span key={n}>{d.text}</span>
                ),
              )}
            </p>
            <p className="ai-proposal-line after">
              <span className="ai-proposal-label">案</span>
              {v.diff.map((d, n) =>
                d.kind === "delete" ? null : d.kind === "insert" ? (
                  // biome-ignore lint/suspicious/noArrayIndexKey: 差分の一続きは並びの順だけで見分ける
                  <ins key={n}>{d.text}</ins>
                ) : (
                  // biome-ignore lint/suspicious/noArrayIndexKey: 差分の一続きは並びの順だけで見分ける
                  <span key={n}>{d.text}</span>
                ),
              )}
            </p>
            {v.proposal.reason !== null && (
              <p className="ai-proposal-reason">理由：{v.proposal.reason}</p>
            )}
            <div className="ai-finding-actions">
              <button
                type="button"
                className="btn btn-sm"
                disabled={busy || isRejected || v.status.kind !== "ready"}
                onClick={() => adopt([i])}
              >
                採用
              </button>
              <button
                type="button"
                className="btn btn-sm"
                disabled={busy || v.status.kind === "same"}
                onClick={() => onToggleReject(set.id, i)}
              >
                {isRejected ? "却下をやめる" : "却下"}
              </button>
              <button
                type="button"
                className="btn btn-sm"
                disabled={v.file === null}
                onClick={() => onGoTo(v)}
              >
                行へ移動
              </button>
            </div>
          </article>
        );
      })}
    </section>
  );
}
