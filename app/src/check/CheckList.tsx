// 検証の画面：プロジェクト全体の診断（読み込み・構文解析・意味解析と、検証の段階1）。
// 本文の診断を選ぶと、本文の画面のその行へ移る。未登録の話者（W202）は、その場で設定DBに登録できる
// （IDを入力してもらう。D047）。

import { useState } from "react";
import type { IrDiagnostic } from "../core/types.ts";
import { groupDiagnostics, unregisteredSpeaker } from "./group.ts";

interface Props {
  readonly diagnostics: readonly IrDiagnostic[];
  /** 本文のファイル（story/*.scn）。 */
  readonly files: readonly string[];
  readonly onSelect: (diagnostic: IrDiagnostic) => void;
  /** 見出しを出すか（文脈パネルでは出さない）。 */
  readonly heading?: string;
  /** 未登録の話者を登録する。エラーの文を返す（登録できれば null）。省くと「登録」を出さない。 */
  readonly onRegister?: (
    diagnostic: IrDiagnostic,
    id: string,
  ) => Promise<string | null>;
}

const LABEL = { error: "エラー", warning: "警告", info: "情報" } as const;

/** 未登録の話者を登録するための、IDの入力欄。 */
function RegisterForm({
  diagnostic,
  name,
  onRegister,
}: {
  readonly diagnostic: IrDiagnostic;
  readonly name: string;
  readonly onRegister: NonNullable<Props["onRegister"]>;
}) {
  const [open, setOpen] = useState(false);
  const [id, setId] = useState("");
  const [error, setError] = useState<string | null>(null);
  if (!open) {
    return (
      <button
        type="button"
        className="small-button check-register"
        onClick={() => setOpen(true)}
      >
        「{name}」を設定DBに登録
      </button>
    );
  }
  return (
    <form
      className="check-register-form"
      onSubmit={(event) => {
        event.preventDefault();
        void onRegister(diagnostic, id.trim()).then((message) => {
          setError(message);
          if (message === null) setOpen(false);
        });
      }}
    >
      <span>「{name}」のID</span>
      <label>
        （英数字と _）
        <input
          value={id}
          onChange={(event) => setId(event.target.value)}
          pattern="[A-Za-z0-9_]+"
          required
        />
      </label>
      <button type="submit" className="small-button">
        登録
      </button>
      <button
        type="button"
        className="small-button"
        onClick={() => setOpen(false)}
      >
        やめる
      </button>
      {error !== null && (
        <span className="check-register-error" role="alert">
          {error}
        </span>
      )}
    </form>
  );
}

export function CheckList({
  diagnostics,
  files,
  onSelect,
  heading,
  onRegister,
}: Props) {
  const groups = groupDiagnostics(diagnostics, files);
  return (
    <section className="check-list" aria-label={heading ?? "問題"}>
      {heading !== undefined && <h2 className="panel-heading">{heading}</h2>}
      {groups.length === 0 && <p className="placeholder">問題はありません。</p>}
      {groups.map((group) => (
        <section key={group.file} className="check-file">
          <h3 className="file-name check-file-name">{group.file}</h3>
          <ul>
            {group.diagnostics.map((d) => (
              <li key={`${d.code}-${d.span[0]}-${d.span[1]}`}>
                <button
                  type="button"
                  className={`check-item ${d.severity}`}
                  disabled={!group.story}
                  title={group.story ? "この行へ移る" : undefined}
                  onClick={() => onSelect(d)}
                >
                  <span className="check-code">{d.code}</span>
                  <span className="check-severity">{LABEL[d.severity]}</span>
                  <span className="check-message">{d.message}</span>
                </button>
                {group.story &&
                  onRegister !== undefined &&
                  unregisteredSpeaker(d) !== null && (
                    <RegisterForm
                      diagnostic={d}
                      name={unregisteredSpeaker(d) ?? ""}
                      onRegister={onRegister}
                    />
                  )}
              </li>
            ))}
          </ul>
        </section>
      ))}
    </section>
  );
}
