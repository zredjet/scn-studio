// プレビューのデバッグの欄（docs/ui/mockups/06-preview.html、T1-11、D050）：現在位置、変数の今の値と編集、
// 任意のシーンへのジャンプ、選択肢の自動選択、バックログ、知らせ。開始シーンからの最短経路での変数の
// 復元はフェーズ2（「この状態で開始」は、今の値で始める）。

import { useState } from "react";
import { keyed } from "../bible/fields.ts";
import type { IrInline, IrScene, PreviewData } from "../core/ir.ts";
import type { Value } from "./expr.ts";
import type { LogEntry } from "./runtime.ts";

export type ChoicePolicy = "stop" | "first" | "random";

interface Props {
  readonly data: PreviewData;
  readonly scene: IrScene | undefined;
  /** 今の行（`3/9行`）。 */
  readonly position: string;
  readonly vars: ReadonlyMap<string, Value>;
  readonly log: readonly LogEntry[];
  readonly warnings: readonly string[];
  readonly policy: ChoicePolicy;
  readonly speakerName: (
    id: string,
    displayName: string | null,
  ) => string | null;
  readonly inlineText: (inlines: readonly IrInline[]) => string;
  readonly onPolicy: (policy: ChoicePolicy) => void;
  readonly onVariable: (key: string, value: Value) => void;
  readonly onJump: (scene: string) => void;
  readonly onShowInEditor: () => void;
  readonly onShowLog: (entry: LogEntry) => void;
}

const POLICIES: readonly { value: ChoicePolicy; label: string }[] = [
  { value: "stop", label: "毎回止まって選ぶ" },
  { value: "first", label: "常に1番目を選ぶ" },
  { value: "random", label: "ランダムに選ぶ" },
];

/** バックログに出す数。 */
const LOG_LIMIT = 30;

function VariableInput({
  name,
  type,
  values,
  value,
  onChange,
}: {
  readonly name: string;
  readonly type: string;
  readonly values: readonly string[];
  readonly value: Value | undefined;
  readonly onChange: (value: Value) => void;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  if (type === "bool") {
    return (
      <input
        type="checkbox"
        aria-label={name}
        checked={value === true}
        onChange={(event) => onChange(event.target.checked)}
      />
    );
  }
  if (type === "enum") {
    return (
      <select
        aria-label={name}
        value={typeof value === "string" ? value : ""}
        onChange={(event) => onChange(event.target.value)}
      >
        {values.map((v) => (
          <option key={v} value={v}>
            {v}
          </option>
        ))}
      </select>
    );
  }
  const shown = draft ?? (value === undefined ? "" : String(value));
  const commit = () => {
    if (draft === null) return;
    if (type === "int") {
      const n = Number(draft.trim());
      if (draft.trim() !== "" && Number.isInteger(n)) onChange(n);
    } else {
      onChange(draft);
    }
    setDraft(null);
  };
  return (
    <input
      aria-label={name}
      className={type === "int" ? "preview-var-int" : "preview-var-text"}
      inputMode={type === "int" ? "numeric" : undefined}
      value={shown}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={commit}
      onKeyDown={(event) => {
        if (event.key === "Enter" && !event.nativeEvent.isComposing) commit();
      }}
    />
  );
}

export function DebugPanel({
  data,
  scene,
  position,
  vars,
  log,
  warnings,
  policy,
  speakerName,
  inlineText,
  onPolicy,
  onVariable,
  onJump,
  onShowInEditor,
  onShowLog,
}: Props) {
  const [jump, setJump] = useState("");
  const declared = new Set(data.variables.map((v) => v.key));
  // 宣言の無い変数で、書かれたもの（W602）も出す。
  const undeclared = [...vars.keys()].filter((k) => !declared.has(k));
  const ordered = [
    ...data.variables.filter((v) => v.scope !== "persistent"),
    ...data.variables.filter((v) => v.scope === "persistent"),
  ];
  return (
    <aside className="preview-debug" aria-label="デバッグ">
      <section>
        <h2 className="preview-debug-heading">現在位置</h2>
        <p className="preview-debug-where">
          <span className="mono">{scene?.id ?? "—"}</span> {scene?.title ?? ""}
        </p>
        <p className="preview-debug-sub mono">{position}</p>
        <button type="button" className="small-button" onClick={onShowInEditor}>
          エディタで表示
        </button>
      </section>
      <section>
        <h2 className="preview-debug-heading">変数</h2>
        <ul className="preview-vars">
          {ordered.map((v, i) => (
            <li
              key={v.key}
              className={
                v.scope === "persistent" &&
                ordered[i - 1]?.scope !== "persistent"
                  ? "persistent first"
                  : v.scope === "persistent"
                    ? "persistent"
                    : undefined
              }
            >
              <span className="mono preview-var-name">{v.key}</span>
              {v.scope === "persistent" && (
                <span className="preview-badge">永続</span>
              )}
              <VariableInput
                name={v.key}
                type={v.type}
                values={v.values}
                value={vars.get(v.key)}
                onChange={(value) => onVariable(v.key, value)}
              />
            </li>
          ))}
          {undeclared.map((key) => (
            <li key={key}>
              <span className="mono preview-var-name">{key}</span>
              <span className="preview-badge warn">宣言なし</span>
              <VariableInput
                name={key}
                type={
                  typeof vars.get(key) === "number"
                    ? "int"
                    : typeof vars.get(key) === "boolean"
                      ? "bool"
                      : "string"
                }
                values={[]}
                value={vars.get(key)}
                onChange={(value) => onVariable(key, value)}
              />
            </li>
          ))}
        </ul>
      </section>
      <section>
        <h2 className="preview-debug-heading">ジャンプ</h2>
        <div className="preview-jump">
          <select
            aria-label="ジャンプ先のシーン"
            value={jump}
            onChange={(event) => setJump(event.target.value)}
          >
            <option value="">シーンを選ぶ…</option>
            {data.ir.scenes.map((s) => (
              <option key={s.id} value={s.id}>
                {s.id} {s.title}
              </option>
            ))}
          </select>
          <button
            type="button"
            className="small-button"
            disabled={jump === ""}
            onClick={() => onJump(jump)}
          >
            この状態で開始
          </button>
        </div>
        <p className="preview-note">変数は、上の今の値のままで始めます。</p>
      </section>
      <section>
        <h2 className="preview-debug-heading">選択肢</h2>
        <div
          className="preview-policy"
          role="radiogroup"
          aria-label="選択肢の自動選択"
        >
          {POLICIES.map((p) => (
            <label key={p.value}>
              <input
                type="radio"
                name="preview-policy"
                checked={policy === p.value}
                onChange={() => onPolicy(p.value)}
              />
              {p.label}
            </label>
          ))}
        </div>
      </section>
      {warnings.length > 0 && (
        <section>
          <h2 className="preview-debug-heading">知らせ</h2>
          <ul className="preview-warnings">
            {keyed(warnings.slice(-8), (w) => w).map(([key, w]) => (
              <li key={key}>{w}</li>
            ))}
          </ul>
        </section>
      )}
      <section className="preview-log-section">
        <h2 className="preview-debug-heading">バックログ</h2>
        <ol className="preview-log">
          {keyed(
            log.slice(-LOG_LIMIT),
            (e) => `${e.scene}:${e.span[0]}:${e.chosen}`,
          ).map(([key, entry, i], _, shown) => {
            const name =
              entry.speaker === null
                ? null
                : speakerName(entry.speaker, entry.displayName);
            return (
              <li
                key={key}
                className={i === shown.length - 1 ? "current" : undefined}
              >
                <button type="button" onClick={() => onShowLog(entry)}>
                  {entry.chosen && (
                    <span className="preview-log-chosen">選択：</span>
                  )}
                  {name !== null && (
                    <span className="preview-log-name">{name}</span>
                  )}
                  {inlineText(entry.text)}
                </button>
              </li>
            );
          })}
        </ol>
      </section>
    </aside>
  );
}
