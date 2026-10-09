// 企画書と設定DBの AI の下書きの、値のある項目への案（T4-4、D080）：今の値と案を並べ、項目ごとに採用・却下する。
// 採用はコア（`adopt_field_proposal`）が、その項目の値を案で置き換えて書き、出自を ai と記録する。却下は画面の中だけ。

import { useState } from "react";
import type { FieldProposal, FieldsWritten } from "../core/types.ts";
import {
  CONCEPT_FIELD_LABELS,
  fieldLabel,
  fieldProposalKey,
  proposalFolded,
  proposalValueText,
} from "./fields.ts";

/** 案ごとの決め方（採用した・却下した）。 */
export type FieldDecision = "adopted" | "rejected";

interface Props {
  /** 下書きを書いた結果（案と、案を見せたときの中身）。 */
  readonly written: FieldsWritten;
  /** 実行の回（案を見分けるため。自律の段では回と段の名前）。 */
  readonly run: number | string;
  /** 採用・却下を出さないわけ（自律を AI のブランチで実行したとき。T4-6）。 */
  readonly readOnly?: string;
  readonly labels?: Readonly<Record<string, string>>;
  readonly decisions: ReadonlyMap<string, FieldDecision>;
  /** 採用する（誤りの文を返す）。 */
  readonly onAdopt: (proposal: FieldProposal) => Promise<string | null>;
  readonly onReject: (proposal: FieldProposal) => void;
}

export function FieldProposals({
  written,
  run,
  labels = CONCEPT_FIELD_LABELS,
  decisions,
  onAdopt,
  onReject,
  readOnly,
}: Props) {
  const [busy, setBusy] = useState<string | null>(null);
  const [problems, setProblems] = useState<ReadonlyMap<string, string>>(
    new Map(),
  );
  /** 決めた案のうち、開いて中身を見ているもの（T4-27）。 */
  const [opened, setOpened] = useState<ReadonlySet<string>>(new Set());
  const toggle = (key: string) =>
    setOpened((s) => {
      const next = new Set(s);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  if (written.proposals.length === 0 && written.notes.length === 0) return null;
  return (
    <section className="ai-field-proposals" aria-label="値のある項目への案">
      {written.notes.map((n) => (
        <p key={n} className="context-faint ai-prepared-note">
          {n}
        </p>
      ))}
      {written.proposals.map((p) => {
        const key = fieldProposalKey(run, p);
        const decision = decisions.get(key);
        const problem = problems.get(key);
        // 決めた案は1行に畳む（開けば中身を見られる。T4-27）。
        if (proposalFolded(decision, opened.has(key)))
          return (
            <article key={key} className="ai-field-proposal is-folded">
              <header>
                <span className="tag tag-ai">{fieldLabel(p.key, labels)}</span>
                <span className="context-faint">
                  {decision === "adopted" ? "採用しました" : "却下しました"}
                </span>
                <span className="context-faint ellipsis">{p.file}</span>
                <span className="grow" />
                <button
                  type="button"
                  className="btn btn-link btn-sm"
                  aria-expanded={false}
                  onClick={() => toggle(key)}
                >
                  中身を見る
                </button>
              </header>
            </article>
          );
        return (
          <article key={key} className="ai-field-proposal">
            <header>
              <span className="tag tag-ai">{fieldLabel(p.key, labels)}</span>
              <span className="context-faint">{p.file}</span>
              {decision !== undefined && (
                <>
                  <span className="grow" />
                  <button
                    type="button"
                    className="btn btn-link btn-sm"
                    aria-expanded={true}
                    onClick={() => toggle(key)}
                  >
                    畳む
                  </button>
                </>
              )}
            </header>
            <dl>
              <dt>今</dt>
              <dd className="ai-field-current">
                {proposalValueText(p.current)}
              </dd>
              <dt>案</dt>
              <dd className="ai-field-proposed">
                {proposalValueText(p.proposed)}
              </dd>
            </dl>
            {problem !== undefined && (
              <p className="notice notice-warn">{problem}</p>
            )}
            {readOnly !== undefined ? (
              <p className="context-faint">{readOnly}</p>
            ) : decision === undefined ? (
              <div className="ai-actions">
                <button
                  type="button"
                  className="btn btn-sm"
                  disabled={busy !== null}
                  onClick={() => {
                    setBusy(key);
                    onAdopt(p)
                      .then((message) =>
                        setProblems((m) => {
                          const next = new Map(m);
                          if (message === null) next.delete(key);
                          else next.set(key, message);
                          return next;
                        }),
                      )
                      .finally(() => setBusy(null));
                  }}
                >
                  採用
                </button>
                <button
                  type="button"
                  className="btn btn-sm"
                  disabled={busy !== null}
                  onClick={() => onReject(p)}
                >
                  却下
                </button>
              </div>
            ) : (
              <p className="context-faint">
                {decision === "adopted" ? "採用しました" : "却下しました"}
              </p>
            )}
          </article>
        );
      })}
    </section>
  );
}
