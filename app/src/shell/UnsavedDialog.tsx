// 保存していない変更の確認のダイアログ（見本 12-overlays.html の 3、T4-20）：保存しない（左に離す）・キャンセル・
// 保存する（主のボタン。Enter）。Esc はキャンセル。並べるものと保存できるかは `unsaved.ts`。

import { useEffect, useRef } from "react";
import { Dialog } from "./Overlay.tsx";
import type { UnsavedAsk, UnsavedChoice } from "./unsaved.ts";

export function UnsavedDialog({
  ask,
  saving,
  error,
  onChoose,
}: {
  readonly ask: UnsavedAsk;
  /** 「保存する」の途中。 */
  readonly saving: boolean;
  /** 保存できなかったわけ。 */
  readonly error: string | null;
  readonly onChoose: (choice: UnsavedChoice) => void;
}) {
  const main = useRef<HTMLButtonElement>(null);
  // Enter で主のボタン（保存する。出さないときはキャンセル）を押せるように、そこへフォーカスを入れる。
  useEffect(() => main.current?.focus(), []);
  return (
    <Dialog
      title={ask.title}
      closable={false}
      onClose={() => {
        if (!saving) onChoose("cancel");
      }}
      foot={
        <>
          <button
            type="button"
            className="btn btn-danger unsaved-discard"
            disabled={saving}
            onClick={() => onChoose("discard")}
          >
            保存しない
          </button>
          <button
            ref={ask.canSave ? undefined : main}
            type="button"
            className="btn"
            disabled={saving}
            onClick={() => onChoose("cancel")}
          >
            キャンセル
          </button>
          {ask.canSave && (
            <button
              ref={main}
              type="button"
              className="btn btn-primary"
              disabled={saving}
              onClick={() => onChoose("save")}
            >
              {saving ? "保存しています…" : "保存する"}
            </button>
          )}
        </>
      }
    >
      <p className="unsaved-message">{ask.message}</p>
      {(ask.files.length > 0 || ask.drafts.length > 0) && (
        <ul className="unsaved-files">
          {[...ask.files, ...ask.drafts].map((f) => (
            <li key={f}>
              <span className="save save-dirty" aria-hidden="true">
                <span className="dot" />
              </span>
              <span className={ask.files.includes(f) ? "mono" : undefined}>
                {f}
              </span>
            </li>
          ))}
        </ul>
      )}
      {ask.note !== null && <span className="hint">{ask.note}</span>}
      {error !== null && (
        <p className="notice notice-danger" role="alert">
          {error}
        </p>
      )}
    </Dialog>
  );
}
