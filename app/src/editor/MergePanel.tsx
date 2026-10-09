// マージの画面（T2-7、D058）：保存していない変更があるのに外で変わったファイルを、元・自分・外の3方向で
// マージする。ぶつかりごとに「自分」「外」「両方（外の行はコメントにして残す）」を選ぶ。まとまった部分は、
// ぶつかりの前後の数行だけを見せる。マージそのものと選んだ結果の組み立てはコアが行う。結果は保存していない
// 変更として開く（保存すると行IDを付けて書く）。

import { useState } from "react";
import type { MergeChoice, MergeView } from "../core/types.ts";
import { useOverlay } from "../shell/useOverlay.ts";
import { contextOf, defaultChoices, linesOf } from "./merge.ts";

/** ぶつかりの前後に見せる行の数。 */
const CONTEXT_LINES = 2;

const CHOICES: readonly { value: MergeChoice; label: string }[] = [
  { value: "mine", label: "自分の変更" },
  { value: "theirs", label: "外の変更" },
  { value: "both", label: "両方（外の行はコメントにして残す）" },
];

interface Props {
  readonly file: string;
  readonly view: MergeView;
  readonly onApply: (choices: readonly MergeChoice[]) => Promise<void>;
  readonly onCancel: () => void;
}

function Lines({
  text,
  className,
}: {
  readonly text: string;
  readonly className: string;
}) {
  const lines = linesOf(text);
  return (
    <pre className={className}>
      {lines.length === 0 ? (
        <span className="merge-empty">（行なし）</span>
      ) : (
        lines.join("\n")
      )}
    </pre>
  );
}

export function MergePanel({ file, view, onApply, onCancel }: Props) {
  const [choices, setChoices] = useState<MergeChoice[]>(() =>
    defaultChoices(view),
  );
  const [busy, setBusy] = useState(false);
  // Esc は「やめる」と同じ（T4-14、U12）。開いたら枠にフォーカスを入れる（Esc が届くように）。
  const overlay = useOverlay<HTMLElement>(onCancel);
  let conflictIndex = -1;
  return (
    <div className="merge-overlay">
      <section
        ref={overlay.ref}
        className="merge-panel"
        aria-label={`${file} のマージ`}
        tabIndex={-1}
        onKeyDown={overlay.onKeyDown}
      >
        <header className="merge-head">
          <div>
            <h2>マージ：{file}</h2>
            <p className="merge-note">
              {view.conflicts === 0
                ? "ぶつかりはありません。自分の変更と外の変更を両方取り込みます。"
                : `ぶつかり ${view.conflicts} か所。それぞれ、どちらを取るかを選んでください。`}
              マージした結果は、保存していない変更として開きます（保存すると書きます）。行IDは変えません。
              @raw
              のブロックの中のぶつかりは、両方を選ぶと印のコメントが生コードになるので、自分か外を選んでください。
            </p>
          </div>
        </header>
        <div className="merge-body">
          {view.hunks.map((hunk, index) => {
            if (hunk.kind === "merged") {
              const before = index > 0;
              const after = index < view.hunks.length - 1;
              const context = contextOf(
                hunk.text,
                before,
                after,
                CONTEXT_LINES,
              );
              return (
                // まとまった部分は並びが変わらないので、位置で見分けてよい。
                // biome-ignore lint/suspicious/noArrayIndexKey: マージの結果は並べ替えない
                <div key={index} className="merge-context">
                  {context.head.length > 0 && (
                    <pre className="merge-lines">{context.head.join("\n")}</pre>
                  )}
                  {context.omitted > 0 && (
                    <span className="merge-omitted">
                      … {context.omitted}行（変わらないか、片方だけの変更）…
                    </span>
                  )}
                  {context.tail.length > 0 && (
                    <pre className="merge-lines">{context.tail.join("\n")}</pre>
                  )}
                </div>
              );
            }
            conflictIndex += 1;
            const at = conflictIndex;
            return (
              <fieldset
                // biome-ignore lint/suspicious/noArrayIndexKey: マージの結果は並べ替えない
                key={index}
                className="merge-conflict"
                aria-label={`ぶつかり ${at + 1}`}
              >
                <legend>ぶつかり {at + 1}</legend>
                <div className="merge-sides">
                  <div>
                    <span className="merge-side-label">自分の変更</span>
                    <Lines text={hunk.mine} className="merge-lines mine" />
                  </div>
                  <div>
                    <span className="merge-side-label">外の変更</span>
                    <Lines text={hunk.theirs} className="merge-lines theirs" />
                  </div>
                </div>
                <div className="merge-choices">
                  {CHOICES.map((choice) => (
                    <label key={choice.value}>
                      <input
                        type="radio"
                        name={`merge-${at}`}
                        value={choice.value}
                        checked={choices[at] === choice.value}
                        onChange={() =>
                          setChoices((list) =>
                            list.map((c, i) => (i === at ? choice.value : c)),
                          )
                        }
                      />
                      {choice.label}
                    </label>
                  ))}
                </div>
              </fieldset>
            );
          })}
        </div>
        <footer className="merge-foot">
          <button type="button" className="small-button" onClick={onCancel}>
            やめる
          </button>
          <button
            type="button"
            className="primary-small"
            disabled={busy}
            onClick={() => {
              setBusy(true);
              void onApply(choices).finally(() => setBusy(false));
            }}
          >
            マージする
          </button>
        </footer>
      </section>
    </div>
  );
}
