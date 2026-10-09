// 名前の変更（F2。T2-8、D059）：シーンID、キャラクターの名前、変数、エントリのID。新しい名前を打つと、変わる
// ところの数を見積もって見せ、「変える」で作品の全体を置き換える。置き換えはコアが行う（行IDは変えない）。
// 変換を確定する Enter では変えない。

import { useEffect, useRef, useState } from "react";
import { renamePreview } from "../core/api.ts";
import type { RenamePreview, RenameSymbol } from "../core/types.ts";
import { useOverlay } from "./useOverlay.ts";

interface Props {
  readonly symbol: RenameSymbol;
  /** 変える。できなければ誤りの文を返す（できれば null）。 */
  readonly onApply: (next: string) => Promise<string | null>;
  readonly onClose: () => void;
}

/** 見積もりの見せ方。 */
function previewText(preview: RenamePreview): string {
  const parts = [
    `本文 ${preview.story_edits}か所（${preview.story.length}ファイル）`,
  ];
  if (preview.settings.length > 0)
    parts.push(`設定 ${preview.settings.length}ファイル`);
  if (preview.moves.length > 0)
    parts.push(`ファイル名の変更 ${preview.moves.length}`);
  return parts.join("・");
}

export function RenameDialog({ symbol, onApply, onClose }: Props) {
  const [next, setNext] = useState(symbol.current);
  const [preview, setPreview] = useState<RenamePreview | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const overlay = useOverlay<HTMLDivElement>(onClose);
  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, []);

  // 打つのが止まったら見積もる。
  useEffect(() => {
    if (next.trim() === "" || next.trim() === symbol.current) {
      setPreview(null);
      setProblem(null);
      return;
    }
    let live = true;
    const timer = setTimeout(() => {
      renamePreview(symbol.target, next)
        .then((p) => {
          if (!live) return;
          setPreview(p);
          setProblem(null);
        })
        .catch((e: unknown) => {
          if (!live) return;
          setPreview(null);
          setProblem(String(e));
        });
    }, 250);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [next, symbol]);

  const ready = !busy && preview !== null && problem === null;
  const apply = () => {
    if (!ready) return;
    setBusy(true);
    void onApply(next.trim()).then((error) => {
      setBusy(false);
      if (error === null) onClose();
      else setProblem(error);
    });
  };

  return (
    <div className="quick-open-layer">
      <button
        type="button"
        className="quick-open-backdrop"
        aria-label="閉じる"
        tabIndex={-1}
        onClick={onClose}
      />
      <div
        className="rename-dialog"
        role="dialog"
        aria-label="名前を変える"
        ref={overlay.ref}
        tabIndex={-1}
        onKeyDown={overlay.onKeyDown}
      >
        <h2>{symbol.label}を変える</h2>
        <p className="rename-current">
          今の名前：<code>{symbol.current}</code>
        </p>
        <input
          ref={input}
          value={next}
          aria-label="新しい名前"
          spellCheck={false}
          onChange={(event) => setNext(event.target.value)}
          onKeyDown={(event) => {
            // 変換を確定する Enter や Escape では動かない。
            if (event.nativeEvent.isComposing || event.keyCode === 229) return;
            if (event.key === "Enter") {
              event.preventDefault();
              apply();
            }
          }}
        />
        <p className={problem === null ? "rename-preview" : "rename-problem"}>
          {problem ??
            (preview === null
              ? "新しい名前を打ってください。"
              : `${previewText(preview)}を変えます。行IDは変えません。本文は保存していない変更ごと保存します。`)}
        </p>
        {preview !== null && preview.notes.length > 0 && (
          <ul className="rename-notes">
            {preview.notes.map((note) => (
              <li key={note}>{note}</li>
            ))}
          </ul>
        )}
        <div className="rename-buttons">
          <button type="button" className="btn btn-sm" onClick={onClose}>
            やめる
          </button>
          <button
            type="button"
            className="btn btn-primary"
            disabled={!ready}
            onClick={apply}
          >
            変える
          </button>
        </div>
      </div>
    </div>
  );
}
