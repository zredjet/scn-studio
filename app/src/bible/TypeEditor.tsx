// 型定義の編集（T2-10、D061）：設定DBの型（`bible/types/<型>.yaml`）の項目を、名前・型・選択肢を直す、
// 並べ替える、消す、足す。書くのはコア（直した項目の行だけを差し替え、読み直して確かめる）。項目は key で
// 指し、画面に出している型定義を読んだときの中身を添える（外で変わっていれば、コアが書かずに知らせる）。
// 1行の `- { … }` で書かれていない型定義は、書かずにソースで直すよう知らせる。
// 項目の key は変えない（エントリが使っているため）。

import { useEffect, useRef, useState } from "react";
import { confirm, editType, typeText } from "../core/api.ts";
import type {
  BibleWritten,
  EntryEdit,
  EntryType,
  FieldDef,
  TypeEdit,
} from "../core/types.ts";
import { isComposingKey } from "./fields.ts";
import { needsOptions, parseOptions, typeChoices } from "./typeform.ts";

interface Props {
  readonly type: EntryType;
  readonly types: readonly EntryType[];
  /** 書いた結果を画面に入れる（設定DBの画面の `applied`）。 */
  readonly onWritten: (result: BibleWritten) => EntryEdit;
  readonly onClose: () => void;
}

interface Draft {
  readonly label: string;
  readonly type: string;
  readonly options: string;
}

function draftOf(field: FieldDef): Draft {
  return {
    label: field.label,
    type: field.type,
    options: field.options.join("、"),
  };
}

export function TypeEditor({ type, types, onWritten, onClose }: Props) {
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [adding, setAdding] = useState<{
    readonly key: string;
    readonly label: string;
    readonly type: string;
    readonly options: string;
  }>({ key: "", label: "", type: "text", options: "" });
  /** 画面に出している型定義のファイルの中身（設定DBを読み直すたびに取り直す）。 */
  const seen = useRef<string | null>(null);
  useEffect(() => {
    let live = true;
    // 取り直している間は前の中身のまま（その間に書いても、外の変更とのぶつかりを確かめられる）。
    typeText(type.id)
      .then((text) => {
        if (live) seen.current = text;
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [type]);

  const write = async (edit: TypeEdit): Promise<boolean> => {
    setBusy(true);
    try {
      const base = seen.current ?? (await typeText(type.id));
      const result = onWritten(await editType(type.id, edit, base));
      if (result.kind === "saved") {
        seen.current = result.text;
        setMessage(null);
        return true;
      }
      if (result.kind === "unsupported") setMessage(result.message);
      else if (result.kind === "conflict") {
        seen.current = result.text;
        setMessage(
          "外で型定義が変わっていたので、書いていません。今の中身を確かめて、もう一度直してください",
        );
      }
      return false;
    } catch (e) {
      setMessage(String(e));
      return false;
    } finally {
      setBusy(false);
    }
  };

  const add = async () => {
    const ok = await write({
      kind: "add",
      key: adding.key.trim(),
      label: adding.label.trim(),
      type: adding.type,
      options: parseOptions(adding.options),
    });
    if (ok) setAdding({ key: "", label: "", type: "text", options: "" });
  };

  return (
    <section className="type-editor" aria-label="型定義の編集">
      <header className="type-editor-head">
        <h1>{type.name} の項目</h1>
        <span className="bible-path">bible/types/{type.id}.yaml</span>
        <span className="flow-spacer" />
        <button type="button" className="small-button" onClick={onClose}>
          閉じる
        </button>
      </header>
      <p className="type-editor-note">
        名前・型・選択肢を直すと、その項目の行だけを書き換えます。項目を消しても、エントリに書いた値は消しません。
        id と name は、どの型にもある決まりの項目です。
      </p>
      {message !== null && (
        <p className="bible-field-error" role="alert">
          {message}
        </p>
      )}
      <div className="type-editor-rows">
        {type.fields.map((field, i) => (
          <TypeFieldRow
            key={`${field.key}:${field.label}:${field.type}:${field.options.join("、")}`}
            field={field}
            types={types}
            busy={busy}
            first={i === 0}
            last={i === type.fields.length - 1}
            onSet={(d) =>
              write({
                kind: "set",
                key: field.key,
                label: d.label.trim(),
                type: d.type,
                options: parseOptions(d.options),
              })
            }
            onMove={(delta) =>
              void write({ kind: "move", key: field.key, delta })
            }
            onRemove={() =>
              void confirm(
                `項目「${field.label}」（${field.key}）を型から消しますか？エントリに書いた値は消しません。`,
                "項目を消す",
              ).then((ok) => {
                if (ok) void write({ kind: "remove", key: field.key });
              })
            }
          />
        ))}
        {type.fields.length === 0 && (
          <p className="context-empty">項目がありません。下の欄で足せます。</p>
        )}
      </div>
      <form
        className="type-editor-add"
        aria-label="項目を足す"
        onSubmit={(event) => event.preventDefault()}
      >
        <input
          aria-label="キー"
          className="mono"
          placeholder="キー（英数字と _）"
          value={adding.key}
          onChange={(event) =>
            setAdding({ ...adding, key: event.target.value })
          }
        />
        <input
          aria-label="項目の名前"
          placeholder="項目の名前"
          value={adding.label}
          onChange={(event) =>
            setAdding({ ...adding, label: event.target.value })
          }
        />
        <select
          aria-label="型"
          value={adding.type}
          onChange={(event) =>
            setAdding({ ...adding, type: event.target.value })
          }
        >
          {typeChoices(types).map((t) => (
            <option key={t} value={t}>
              {t}
            </option>
          ))}
        </select>
        {needsOptions(adding.type) && (
          <input
            aria-label="選択肢"
            placeholder="選択肢（、で区切る）"
            value={adding.options}
            onChange={(event) =>
              setAdding({ ...adding, options: event.target.value })
            }
          />
        )}
        <button
          type="button"
          className="primary-small"
          disabled={
            busy || adding.key.trim() === "" || adding.label.trim() === ""
          }
          onClick={() => void add()}
        >
          足す
        </button>
      </form>
    </section>
  );
}

function TypeFieldRow({
  field,
  types,
  busy,
  first,
  last,
  onSet,
  onMove,
  onRemove,
}: {
  readonly field: FieldDef;
  readonly types: readonly EntryType[];
  readonly busy: boolean;
  readonly first: boolean;
  readonly last: boolean;
  readonly onSet: (draft: Draft) => Promise<boolean>;
  readonly onMove: (delta: number) => void;
  readonly onRemove: () => void;
}) {
  const [draft, setDraft] = useState<Draft>(() => draftOf(field));
  /** 最後に書いた中身（Enter の後にフォーカスが外れても、同じものを2回書かない）。 */
  const sent = useRef<string | null>(null);
  const signature = (d: Draft) =>
    JSON.stringify([d.label.trim(), d.type, parseOptions(d.options)]);
  const changed = signature(draft) !== signature(draftOf(field));
  const commit = (next: Draft) => {
    if (busy || sent.current === signature(next)) return;
    // 選択肢の要る型へ変えたときは、選択肢が入るまで書かない。
    if (needsOptions(next.type) && parseOptions(next.options).length === 0)
      return;
    if (next.label.trim() === "") return;
    sent.current = signature(next);
    void onSet(next).then((ok) => {
      if (!ok) {
        sent.current = null;
        setDraft(draftOf(field));
      }
    });
  };
  return (
    <div className="type-editor-row">
      <span className="type-editor-key mono">{field.key}</span>
      <input
        aria-label={`${field.key} の名前`}
        value={draft.label}
        onChange={(event) => setDraft({ ...draft, label: event.target.value })}
        onBlur={() => {
          if (changed) commit(draft);
        }}
        onKeyDown={(event) => {
          if (isComposingKey(event) || event.key !== "Enter") return;
          event.preventDefault();
          if (changed) commit(draft);
        }}
      />
      <select
        aria-label={`${field.key} の型`}
        value={draft.type}
        disabled={busy}
        onChange={(event) => {
          const type = event.target.value;
          // 選択肢の要らない型に変えたら、選択肢は書かない。
          const next = {
            ...draft,
            type,
            options: needsOptions(type) ? draft.options : "",
          };
          setDraft(next);
          commit(next);
        }}
      >
        {typeChoices(types, field.type).map((t) => (
          <option key={t} value={t}>
            {t}
          </option>
        ))}
      </select>
      {(needsOptions(draft.type) || draft.options !== "") && (
        <input
          aria-label={`${field.key} の選択肢`}
          placeholder="選択肢（、で区切る）"
          value={draft.options}
          onChange={(event) =>
            setDraft({ ...draft, options: event.target.value })
          }
          onBlur={() => {
            if (changed) commit(draft);
          }}
          onKeyDown={(event) => {
            if (isComposingKey(event) || event.key !== "Enter") return;
            event.preventDefault();
            if (changed) commit(draft);
          }}
        />
      )}
      <span className="type-editor-buttons">
        <button
          type="button"
          className="small-button"
          aria-label={`${field.label} を前へ`}
          disabled={busy || first}
          onClick={() => onMove(-1)}
        >
          ↑
        </button>
        <button
          type="button"
          className="small-button"
          aria-label={`${field.label} を後ろへ`}
          disabled={busy || last}
          onClick={() => onMove(1)}
        >
          ↓
        </button>
        <button
          type="button"
          className="small-button"
          disabled={busy}
          onClick={onRemove}
        >
          消す
        </button>
      </span>
    </div>
  );
}
