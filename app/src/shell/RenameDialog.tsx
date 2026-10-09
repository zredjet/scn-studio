// 名前の変更（F2。T2-8、D059。見本 12-overlays.html の 3、T4-20）：シーンID、キャラクターの名前、変数、エントリのID。
// 新しい名前を打つと、変わる場所（章ごとの行・設定・ファイル名。コアの `rename_preview`）をダイアログの中で見せ、
// 「Nか所を変える」で作品の全体を置き換える。置き換えはコアが行う（行IDは変えない）。変換を確定する Enter では
// 変えない。

import { useEffect, useRef, useState } from "react";
import { renamePreview } from "../core/api.ts";
import type { RenamePreview, RenameSymbol } from "../core/types.ts";
import { Dialog } from "./Overlay.tsx";
import { renameCount, renamePlaces } from "./writes.ts";

interface Props {
  readonly symbol: RenameSymbol;
  /** 変える。できなければ誤りの文を返す（できれば null）。 */
  readonly onApply: (next: string) => Promise<string | null>;
  readonly onClose: () => void;
}

export function RenameDialog({ symbol, onApply, onClose }: Props) {
  const [next, setNext] = useState(symbol.current);
  const [preview, setPreview] = useState<RenamePreview | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const input = useRef<HTMLInputElement>(null);
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
  const count = preview === null ? 0 : renameCount(preview);
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
    <Dialog
      title="名前の変更"
      onClose={onClose}
      foot={
        <>
          <button type="button" className="btn" onClick={onClose}>
            キャンセル
          </button>
          <button
            type="button"
            className="btn btn-primary"
            disabled={!ready}
            onClick={apply}
          >
            {preview === null ? "変える" : `${count}か所を変える`}
          </button>
        </>
      }
    >
      <div className="field">
        <span className="lbl">今の名前（{symbol.label}）</span>
        <input
          className="input is-readonly mono"
          value={symbol.current}
          readOnly
          aria-label="今の名前"
        />
      </div>
      <div className="field">
        <span className="lbl">新しい名前</span>
        <input
          ref={input}
          className="input mono"
          value={next}
          aria-label="新しい名前"
          aria-invalid={problem !== null}
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
        {problem !== null ? (
          <span className="hint write-problem" role="alert">
            {problem}
          </span>
        ) : preview === null ? (
          <span className="hint">新しい名前を打ってください。</span>
        ) : (
          preview.notes.map((note) => (
            <span key={note} className="hint">
              {note}
            </span>
          ))
        )}
      </div>
      {preview !== null && (
        <div className="write-places">
          <span className="lbl">変わる場所　{count}</span>
          <div className="table-wrap">
            <table className="table">
              <tbody>
                {renamePlaces(preview).map((row) => (
                  <tr key={`${row.file}\u0000${row.what}`}>
                    <td className="mono t-small">{row.file}</td>
                    <td className="t-small t-2">{row.what}</td>
                    <td className="num">{row.count}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <span className="hint">
            行IDは変えません。本文は保存していない変更ごと保存します。
          </span>
        </div>
      )}
    </Dialog>
  );
}
