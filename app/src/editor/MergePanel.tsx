// マージの画面（T2-7、D058。見本 12-overlays.html の 4、T4-20）：保存していない変更があるのに外で変わったファイルを、
// 元・自分・外の3方向でマージする。重なった所ごとに、左（このアプリ）と右（外）を並べ、「このアプリ」「外」
// 「両方（外の行はコメントにして残す）」を選ぶ。選んだ側は藍の薄い面と言葉で示す（赤と緑は使わない）。重ならない
// 部分は、重なりの前後の数行だけを見せる。マージそのものと選んだ結果の組み立てはコアが行う。結果は保存していない
// 変更として開く（保存すると行IDを付けて書く）。

import { useState } from "react";
import type { MergeChoice, MergeView } from "../core/types.ts";
import { Icon } from "../shell/icons.tsx";
import { Dialog } from "../shell/Overlay.tsx";
import { contextOf, defaultChoices, linesOf, picked } from "./merge.ts";

/** ぶつかりの前後に見せる行の数。 */
const CONTEXT_LINES = 2;

const CHOICES: readonly { value: MergeChoice; label: string }[] = [
  { value: "mine", label: "このアプリ" },
  { value: "theirs", label: "外" },
  { value: "both", label: "両方を残す（外の行はコメントに）" },
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
  let conflictIndex = -1;
  return (
    <Dialog
      width={960}
      label={`${file} のマージ`}
      title={`${file.replace(/^story\//, "")} が外で変わりました`}
      // Esc は「あとで決める」と同じ（T4-14、U12）。
      onClose={onCancel}
      closable={false}
      head={
        view.conflicts > 0 ? (
          <span className="tag tag-warn">
            <Icon name="warning" size={11} stroke={2} />
            重なり {view.conflicts}か所
          </span>
        ) : (
          <span className="tag">重なりなし</span>
        )
      }
      foot={
        <>
          <button type="button" className="btn" onClick={onCancel}>
            あとで決める
          </button>
          <button
            type="button"
            className="btn btn-primary"
            disabled={busy}
            onClick={() => {
              setBusy(true);
              void onApply(choices).finally(() => setBusy(false));
            }}
          >
            選んだ内容で取り込む
          </button>
        </>
      }
    >
      <div className="merge-columns t-small t-2">
        <span>このアプリの変更</span>
        <span>外の変更</span>
      </div>
      {view.hunks.map((hunk, index) => {
        if (hunk.kind === "merged") {
          const before = index > 0;
          const after = index < view.hunks.length - 1;
          const context = contextOf(hunk.text, before, after, CONTEXT_LINES);
          return (
            // まとまった部分は並びが変わらないので、位置で見分けてよい。
            // biome-ignore lint/suspicious/noArrayIndexKey: マージの結果は並べ替えない
            <div key={index} className="merge-context">
              {context.head.length > 0 && (
                <pre className="merge-lines">{context.head.join("\n")}</pre>
              )}
              {context.omitted > 0 && (
                <span className="merge-omitted t-caption t-3">
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
        const choice = choices[at] ?? "both";
        return (
          <fieldset
            // biome-ignore lint/suspicious/noArrayIndexKey: マージの結果は並べ替えない
            key={index}
            className="merge-hunk"
            aria-label={`重なり ${at + 1}`}
          >
            <div className="merge-sides">
              <Lines
                text={hunk.mine}
                className={
                  picked(choice, "mine")
                    ? "merge-lines is-picked"
                    : "merge-lines"
                }
              />
              <Lines
                text={hunk.theirs}
                className={
                  picked(choice, "theirs")
                    ? "merge-lines is-picked"
                    : "merge-lines"
                }
              />
            </div>
            <div className="merge-bar">
              <span className="t-caption t-3">重なり {at + 1}</span>
              <span className="grow" />
              {CHOICES.map((c) => (
                <label key={c.value} className="check">
                  <input
                    type="radio"
                    name={`merge-${at}`}
                    value={c.value}
                    checked={choice === c.value}
                    onChange={() =>
                      setChoices((list) =>
                        list.map((x, i) => (i === at ? c.value : x)),
                      )
                    }
                  />
                  {c.label}
                </label>
              ))}
            </div>
          </fieldset>
        );
      })}
      <span className="hint">
        重ならない変更は、そのまま取り込みます。取り込んだ結果は保存していない変更として開き、保存すると書きます。行IDは変えません。@raw
        のブロックの中の重なりは、両方を残すと印のコメントが生コードになるので、どちらかを選んでください。
      </span>
    </Dialog>
  );
}
