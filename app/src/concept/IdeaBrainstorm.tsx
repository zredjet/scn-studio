// 企画の画面の「ネタ出し」（T4-29、D088）：方向性と数を書いて、アイデアの候補を出してもらい（タスク
// `idea_brainstorm`。出力 `idea_candidates`）、選んだものだけをアイデアメモ（未整理）として足す（本文の出自は ai。
// 書くのはコア `add_ai_ideas`）。足さなかった候補は残さない。前に作った作品にタスクの定義が無ければ、ひな形から
// 足せる。

import { type KeyboardEvent, useState } from "react";
import { ideaCount, ideaRequestArg, type TaskRunner } from "../ai/model.ts";
import { QuickRun } from "../ai/QuickRun.tsx";
import { isComposingKey } from "../bible/fields.ts";
import type { IdeaCandidate } from "../core/types.ts";

/** 候補を見分けるキー（回と順番）。 */
export function brainstormKey(run: number, index: number): string {
  return `${run}-${index}`;
}

/** ネタ出し（App が持つ）。 */
export interface IdeaBrainstormAi {
  /** タスクの定義があるか（無ければ、ひな形から足すボタンを出す）。 */
  readonly available: boolean;
  /** 頼み（`ideaRequestArg` の文字）で実行する。 */
  readonly runner: (request: string) => TaskRunner;
  /** 最後の回の結果（閉じた回は null）。 */
  readonly result:
    | {
        readonly run: number;
        readonly ok: true;
        readonly candidates: readonly IdeaCandidate[];
      }
    | { readonly run: number; readonly ok: false; readonly message: string }
    | null;
  /** メモにした候補（回と順番の `keyOf`。App が持つので、画面を作り直しても二重に足さない）。 */
  readonly added: ReadonlySet<string>;
  /** 選んだ候補（順番）をメモにする（足した数と、誤りの文を返す）。 */
  readonly onAdd: (
    run: number,
    indexes: readonly number[],
  ) => Promise<{ readonly count: number; readonly problem: string | null }>;
  readonly onDismiss: (run: number) => void;
  /** タスクの定義をひな形から足す（誤りの文を返す）。 */
  readonly onAddTask: () => Promise<string | null>;
  /** ほかのタスクを実行しているか。 */
  readonly otherRunning: boolean;
}

export function IdeaBrainstorm({ ai }: { readonly ai: IdeaBrainstormAi }) {
  const [direction, setDirection] = useState("");
  const [count, setCount] = useState("5");
  /** 選んだ候補（回と順番）。 */
  const [chosen, setChosen] = useState<ReadonlySet<string>>(new Set());
  const added = ai.added;
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const result = ai.result;
  const keyOf = brainstormKey;
  const toggle = (key: string) =>
    setChosen((s) => {
      const next = new Set(s);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  if (!ai.available)
    return (
      <section className="concept-ai-task" aria-label="ネタ出し">
        <h3 className="concept-ai-title">ネタ出し</h3>
        <p className="t-small t-2">
          この作品には、ネタ出しのタスクの定義（ai/idea_brainstorm.yaml）がありません。ひな形から足すと使えます
        </p>
        <div className="ai-actions">
          <button
            type="button"
            className="btn btn-sm"
            disabled={busy}
            onClick={() => {
              setBusy(true);
              ai.onAddTask()
                .then((p) => setMessage(p))
                .catch((e: unknown) => setMessage(String(e)))
                .finally(() => setBusy(false));
            }}
          >
            タスクの定義を足す
          </button>
        </div>
        {message !== null && <p className="notice notice-warn">{message}</p>}
      </section>
    );
  const picked =
    result?.ok === true
      ? result.candidates
          .map((_, i) => i)
          .filter((i) => {
            const key = keyOf(result.run, i);
            return chosen.has(key) && !added.has(key);
          })
      : [];
  const addPicked = () => {
    if (result?.ok !== true || picked.length === 0) return;
    setBusy(true);
    setMessage(null);
    ai.onAdd(result.run, picked)
      .then(({ count, problem }) =>
        setMessage(
          problem === null
            ? `${count}件をアイデアメモ（未整理）にしました`
            : count > 0
              ? `${count}件をメモにしました。${problem}`
              : problem,
        ),
      )
      .catch((e: unknown) => setMessage(String(e)))
      .finally(() => setBusy(false));
  };
  return (
    <section className="concept-ai-task" aria-label="ネタ出し">
      <h3 className="concept-ai-title">ネタ出し</h3>
      <p className="t-small t-2">
        方向性を伝えて、アイデアの候補を出してもらいます。選んだものだけをメモにします
      </p>
      <label className="idea-brainstorm-field">
        <span className="t-small t-2">方向性</span>
        <textarea
          className="textarea"
          rows={2}
          value={direction}
          placeholder="例：紬が主人公を試す展開"
          onChange={(e) => setDirection(e.target.value)}
        />
      </label>
      <label className="idea-brainstorm-count">
        <span className="t-small t-2">数</span>
        <input
          className="input mono"
          type="text"
          inputMode="numeric"
          value={count}
          onChange={(e) => setCount(e.target.value)}
          onBlur={() => setCount(String(ideaCount(count)))}
          onKeyDown={(event: KeyboardEvent<HTMLInputElement>) => {
            if (event.key === "Enter" && !isComposingKey(event))
              setCount(String(ideaCount(count)));
          }}
        />
        <span className="t-small t-3">（1〜10）</span>
      </label>
      <QuickRun
        // 方向性や数を変えたら、送る中身の確かめをやり直す（古い確かめで送らない）。
        key={ideaRequestArg(direction, ideaCount(count))}
        runner={ai.runner(ideaRequestArg(direction, ideaCount(count)))}
        scene={null}
        label="AIでネタ出し"
        disabledReason={ai.otherRunning ? "ほかのタスクを実行しています" : null}
      />
      {result !== null && (
        <div className="ai-result-head">
          <p className={result.ok ? "notice" : "notice notice-warn"}>
            {result.ok
              ? result.candidates.length === 0
                ? "候補を読めませんでした"
                : `候補 ${result.candidates.length}件。メモにするものを選びます`
              : result.message}
          </p>
          <button
            type="button"
            className="btn btn-sm"
            onClick={() => ai.onDismiss(result.run)}
          >
            結果を閉じる
          </button>
        </div>
      )}
      {result?.ok === true &&
        result.candidates.map((c, i) => {
          const key = keyOf(result.run, i);
          const done = added.has(key);
          return (
            <label
              key={key}
              className={
                done
                  ? "concept-ai-card idea-candidate is-done"
                  : "concept-ai-card idea-candidate"
              }
            >
              <input
                type="checkbox"
                checked={done || chosen.has(key)}
                disabled={done || busy}
                onChange={() => toggle(key)}
              />
              <span className="idea-candidate-text">
                {c.title !== "" && <strong>{c.title}</strong>}
                <span>{c.body}</span>
                {c.tags.length > 0 && (
                  <span className="idea-tags">
                    {c.tags.map((t) => (
                      <span key={t} className="tag">
                        {t}
                      </span>
                    ))}
                  </span>
                )}
                {done && <span className="t-small t-3">メモにしました</span>}
              </span>
            </label>
          );
        })}
      {result?.ok === true && result.candidates.length > 0 && (
        <div className="ai-actions">
          <button
            type="button"
            className="btn btn-sm btn-primary"
            disabled={busy || picked.length === 0}
            onClick={addPicked}
          >
            選んだものをメモにする（{picked.length}）
          </button>
        </div>
      )}
      {message !== null && <p className="context-faint">{message}</p>}
    </section>
  );
}
