// 設定ファイルをテキストで直す欄（設定DBのエントリのソース表示と、`variables.yaml`。T1-10、D049）。
// 保存は「保存」のボタンか Mod-S（変換中は保存しない）。書くときは、下書きの元になったディスクの中身を
// 添える。コアがそれと今の中身を照らし、外で変わっていれば書かずに知らせる（「読み込み直す」か
// 「上書きする」を選ぶ）。直している間に外で変わったと分かったときも、その場で同じ知らせを出す。

import { useEffect, useRef, useState } from "react";
import type { EntryEdit } from "../core/types.ts";
import { isComposingKey, normalizeNewlines } from "./fields.ts";

interface Props {
  /** 欄の名前（ファイルのパス）。 */
  readonly label: string;
  /** ディスクの中身。 */
  readonly text: string;
  /**
   * 書く。`seen` は下書きの元になったディスクの中身（コアが今の中身と照らす）。null なら、外で変わって
   * いても上書きする。
   */
  readonly onSave: (
    text: string,
    seen: string | null,
  ) => Promise<EntryEdit | null>;
  /** 保存していない変更があるか（閉じる前・切り替える前の確認に使う）。 */
  readonly onUnsaved: (unsaved: boolean) => void;
}

export function SourceView({ label, text, onSave, onUnsaved }: Props) {
  /** 下書きの元になったディスクの中身（改行もそのまま）。 */
  const [base, setBase] = useState(text);
  const [draft, setDraft] = useState(normalizeNewlines(text));
  /** 外の中身（直している間に外で変わった、または保存のときに衝突した）。 */
  const [conflict, setConflict] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const composing = useRef(false);
  /** 書いている途中か（自分の書いた中身が届いても、外の変更と取り違えない）。 */
  const writing = useRef(false);
  const unsaved = draft !== normalizeNewlines(base);
  const state = useRef({ base, unsaved });
  state.current = { base, unsaved };

  // ディスクの中身が変わったら（保存の後、外の変更）：直していなければ入れ替え、直していれば知らせる。
  useEffect(() => {
    const { base: prev, unsaved: editing } = state.current;
    if (text === prev || writing.current) return;
    if (editing) {
      setConflict(text);
      return;
    }
    setBase(text);
    setDraft(normalizeNewlines(text));
    setConflict(null);
  }, [text]);

  useEffect(() => {
    onUnsaved(unsaved);
  }, [unsaved, onUnsaved]);
  useEffect(() => () => onUnsaved(false), [onUnsaved]);

  const save = async (force: boolean) => {
    if (composing.current || writing.current) return;
    writing.current = true;
    setSaving(true);
    try {
      const edit = await onSave(draft, force ? null : base);
      if (edit === null) return;
      if (edit.kind === "saved") {
        setBase(edit.text);
        setDraft(normalizeNewlines(edit.text));
        setConflict(null);
      } else if (edit.kind === "conflict") {
        setConflict(edit.text);
      }
    } finally {
      writing.current = false;
      setSaving(false);
    }
  };

  return (
    <div className="bible-source">
      <div className="bible-source-bar">
        <span className="bible-source-label">{label}</span>
        {unsaved && <span className="unsaved">未保存</span>}
        <span className="flow-spacer" />
        <button
          type="button"
          className="small-button"
          disabled={!unsaved}
          onClick={() => {
            const next = conflict ?? base;
            setBase(next);
            setDraft(normalizeNewlines(next));
            setConflict(null);
          }}
        >
          元に戻す
        </button>
        <button
          type="button"
          className="primary-small"
          disabled={!unsaved || saving}
          onClick={() => void save(false)}
        >
          保存
        </button>
      </div>
      {conflict !== null && (
        <div className="conflict-bar" role="alert">
          <span>
            {label}{" "}
            は外で変わりました。読み込み直すと、ここでの変更は捨てます。上書きすると、外の変更を捨てます。
          </span>
          <button
            type="button"
            className="small-button"
            onClick={() => {
              setBase(conflict);
              setDraft(normalizeNewlines(conflict));
              setConflict(null);
            }}
          >
            読み込み直す
          </button>
          <button
            type="button"
            className="small-button"
            onClick={() => void save(true)}
          >
            上書きする
          </button>
        </div>
      )}
      <textarea
        className="bible-source-text"
        aria-label={`${label} のソース`}
        spellCheck={false}
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
        onCompositionStart={() => {
          composing.current = true;
        }}
        onCompositionEnd={() => {
          composing.current = false;
        }}
        onKeyDown={(event) => {
          if (
            (event.metaKey || event.ctrlKey) &&
            event.key.toLowerCase() === "s"
          ) {
            event.preventDefault();
            if (!isComposingKey(event)) void save(false);
          }
        }}
      />
    </div>
  );
}
