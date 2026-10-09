// 設定DBのフォームの入力欄（docs/ui/mockups/04-bible.html の「型から作るフォーム」、T1-10、D049）。
// 項目の型ごとに入力欄を選ぶ。値は、入力が決まったとき（フォーカスが外れた、Enter、選んだ、チップを足した・
// 消した）に保存する。変換中の Enter では決めない（.claude/rules/frontend.md）。

import { type ReactNode, useEffect, useRef, useState } from "react";
import type {
  EntryEdit,
  FieldDef,
  IrDiagnostic,
  JsonValue,
} from "../core/types.ts";
import {
  addItem,
  appended,
  type FieldChange,
  isComposingKey,
  keyed,
  kindLabel,
  listItems,
  parseInput,
  sameValue,
  scalarText,
  toggled,
  widgetOf,
  withoutItem,
} from "./fields.ts";
import { type FieldOrigin, originMark } from "./origin.ts";

/** 参照の候補（エントリかシーン）。 */
export interface RefCandidate {
  readonly id: string;
  readonly name: string;
}

interface Props {
  readonly field: FieldDef;
  readonly value: JsonValue | undefined;
  readonly typeNames: ReadonlyMap<string, string>;
  /** 参照の候補（型ID か `scene`）。 */
  readonly candidates: (target: string) => readonly RefCandidate[];
  /** この項目の診断（W805・W806）。 */
  readonly problems: readonly IrDiagnostic[];
  /** 添え書き（「台詞の話者判定に使われます」など）。 */
  readonly hint?: string;
  /** 消せない項目（選択肢の欄に「（未設定）」を出さない。変数の型）。 */
  readonly required?: boolean;
  /** エントリのファイルの今の中身（直し始めたときの中身を覚え、外の変更と照らすため）。 */
  readonly fileText: string;
  /** 直す。書いた結果（書かなければ null）を返す。 */
  readonly onCommit: (change: FieldChange) => Promise<EntryEdit | null>;
  /** 参照先を開く。 */
  readonly onOpenRef: (target: string, id: string) => void;
  /** この項目の出自（AI が書いた・人が直した。T4-3）。 */
  readonly origin?: FieldOrigin | null;
}

/** 項目の欄の class（AI が書いた項目には印。T4-3）。 */
export function fieldClass(origin: FieldOrigin | null | undefined): string {
  const mark = originMark(origin);
  return mark === null ? "bible-field" : `bible-field ${mark.className}`;
}

/** 項目の出自の札（T4-3）。 */
export function OriginBadge({
  origin,
}: {
  readonly origin: FieldOrigin | null | undefined;
}) {
  const mark = originMark(origin);
  if (mark === null) return null;
  return (
    <span className="field-origin-badge" title={mark.title}>
      {mark.text}
    </span>
  );
}

/** 値を決める（選ぶ、切り替える）。書く時点の中身と照らす。 */
function set(value: JsonValue): FieldChange {
  return { kind: "set", value, seen: null };
}

/** 書く時点の値から作る。 */
function update(
  make: (current: JsonValue | undefined) => JsonValue,
): FieldChange {
  return {
    kind: "update",
    update: (current) => ({ ok: true, value: make(current) }),
  };
}

/**
 * 1行と複数行の入力欄。下書きを持ち、決まったときに値にして渡す。打ち始めたときのファイルの中身（`seen`）を
 * 添えるので、打っている間に外で同じ項目が変わっていれば、コアが書かずに知らせる。打っていない間は、
 * 下書きと `seen` を今の値と中身に合わせる。
 */
function TextBox({
  field,
  value,
  multiline,
  fileText,
  onCommit,
}: {
  readonly field: FieldDef;
  readonly value: JsonValue | undefined;
  readonly multiline: boolean;
  readonly fileText: string;
  readonly onCommit: (change: FieldChange) => Promise<EntryEdit | null>;
}) {
  const text = scalarText(value) ?? "";
  const [draft, setDraft] = useState(text);
  const [error, setError] = useState<string | null>(null);
  /** 人が打ってから、まだ渡していないか。 */
  const touched = useRef(false);
  /** 打った回数（渡した後に打ち直したかを見分ける）。 */
  const typed = useRef(0);
  /** 下書きの元になったファイルの中身。 */
  const seen = useRef(fileText);
  /** 今の値（書いた結果が届いたときに、下書きを戻すため）。 */
  const current = useRef(text);
  current.current = text;
  // 打っていなければ、下書きと元の中身を今のものにする（保存の後、外の変更、衝突の後）。
  useEffect(() => {
    if (touched.current) return;
    seen.current = fileText;
    setDraft(text);
  }, [text, fileText]);
  const change = (next: string) => {
    touched.current = true;
    typed.current += 1;
    setDraft(next);
  };
  const commit = () => {
    if (!touched.current) return;
    const parsed = parseInput(field.kind, draft);
    if (!parsed.ok) {
      setError(parsed.error);
      return;
    }
    setError(null);
    touched.current = false;
    if (sameValue(parsed.value, value)) {
      // 書くことが無い（「１７」と 17 のように、書き方だけ違う）。今の値の書き方に戻す。
      setDraft(text);
      return;
    }
    const stamp = typed.current;
    void onCommit({
      kind: "set",
      value: parsed.value,
      seen: seen.current,
    }).then((edit) => {
      if (edit === null || edit.kind === "created") return;
      if (edit.kind === "saved") {
        // 書いた中身を、次の元にする（書いた後に打ち直していても）。
        seen.current = edit.text;
        return;
      }
      // 書いた後に打ち直していれば、その下書きを残す。
      if (typed.current !== stamp) return;
      if (edit.kind === "unsupported") {
        // ソース表示へ写せるよう、下書きを残す（後の読み直しで消さない。Escape で戻す）。
        touched.current = true;
        return;
      }
      // 衝突：今の中身を出す（知らせのとおり）。もう一度決めれば、今の中身に対して書く。
      seen.current = edit.text;
      setDraft(current.current);
    });
  };
  const common = {
    id: `field-${field.key}`,
    value: draft,
    placeholder: "[未定]",
    "aria-invalid": error !== null,
    onBlur: commit,
  };
  return (
    <>
      {multiline ? (
        <textarea
          {...common}
          rows={3}
          onChange={(event) => change(event.target.value)}
          onKeyDown={(event) => {
            if (
              event.key === "Enter" &&
              (event.metaKey || event.ctrlKey) &&
              !isComposingKey(event)
            ) {
              event.preventDefault();
              commit();
            }
          }}
        />
      ) : (
        <input
          {...common}
          inputMode={field.kind.kind === "number" ? "decimal" : undefined}
          onChange={(event) => change(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !isComposingKey(event)) {
              event.preventDefault();
              commit();
            }
            // 変換を取り消す Escape では、下書きを捨てない。
            if (event.key === "Escape" && !isComposingKey(event)) {
              touched.current = false;
              seen.current = fileText;
              setDraft(text);
              setError(null);
            }
          }}
        />
      )}
      {error !== null && (
        <span className="bible-field-error" role="alert">
          {error}
        </span>
      )}
    </>
  );
}

/** リスト（チップ）。末尾の入力欄で足し、×で消す。 */
function Chips({
  field,
  value,
  onCommit,
}: {
  readonly field: FieldDef;
  readonly value: JsonValue | undefined;
  readonly onCommit: (change: FieldChange) => Promise<EntryEdit | null>;
}) {
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const items = listItems(value);
  const add = () => {
    if (draft.trim() === "") return;
    // 型に合うかは今の値で先に確かめ、書く時点の値に足す。
    const parsed = addItem(field.kind, value, draft);
    if (!parsed.ok) {
      setError(parsed.error);
      return;
    }
    setError(null);
    const text = draft;
    setDraft("");
    onCommit({
      kind: "update",
      update: (current) => addItem(field.kind, current, text),
    });
  };
  return (
    <>
      <div className="bible-chips">
        {keyed(items, (item) => item).map(([key, item]) => (
          <span key={key} className="bible-chip">
            {item}
            <button
              type="button"
              aria-label={`${item}を削除`}
              onClick={() =>
                onCommit(update((current) => withoutItem(current, item)))
              }
            >
              ×
            </button>
          </span>
        ))}
        <input
          id={`field-${field.key}`}
          aria-label={`${field.label}を追加`}
          placeholder="追加…"
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onBlur={add}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !isComposingKey(event)) {
              event.preventDefault();
              add();
            }
          }}
        />
      </div>
      {error !== null && (
        <span className="bible-field-error" role="alert">
          {error}
        </span>
      )}
    </>
  );
}

/** 参照を選ぶ（候補に無い今の値も出す）。 */
function RefSelect({
  field,
  target,
  value,
  candidates,
  onCommit,
  onOpenRef,
}: {
  readonly field: FieldDef;
  readonly target: string;
  readonly value: string;
  readonly candidates: readonly RefCandidate[];
  readonly onCommit: (change: FieldChange) => Promise<EntryEdit | null>;
  readonly onOpenRef: (target: string, id: string) => void;
}) {
  const known = candidates.some((c) => c.id === value);
  return (
    <div className="bible-ref">
      <select
        id={`field-${field.key}`}
        value={value}
        onChange={(event) =>
          onCommit(set(event.target.value === "" ? null : event.target.value))
        }
      >
        <option value="">（未設定）</option>
        {!known && value !== "" && (
          <option value={value}>{value}（無い）</option>
        )}
        {candidates.map((c) => (
          <option key={c.id} value={c.id}>
            {c.name === c.id ? c.id : `${c.name}（${c.id}）`}
          </option>
        ))}
      </select>
      <button
        type="button"
        className="small-button"
        disabled={!known}
        onClick={() => onOpenRef(target, value)}
      >
        開く
      </button>
    </div>
  );
}

export function FieldInput({
  field,
  value,
  typeNames,
  candidates,
  problems,
  hint,
  required = false,
  fileText,
  onCommit,
  onOpenRef,
  origin = null,
}: Props) {
  const widget = widgetOf(field.kind, value);
  let control: ReactNode;
  switch (widget) {
    case "text":
    case "asset":
    case "number":
    case "date":
      control = (
        <TextBox
          field={field}
          value={value}
          multiline={false}
          fileText={fileText}
          onCommit={onCommit}
        />
      );
      break;
    case "longtext":
      control = (
        <TextBox
          field={field}
          value={value}
          multiline
          fileText={fileText}
          onCommit={onCommit}
        />
      );
      break;
    case "bool":
      control = (
        <label className="bible-check">
          <input
            id={`field-${field.key}`}
            type="checkbox"
            checked={value === true}
            onChange={(event) => onCommit(set(event.target.checked))}
          />
          {value === true ? "はい" : "いいえ"}
        </label>
      );
      break;
    case "select": {
      const current = scalarText(value) ?? "";
      control = (
        <select
          id={`field-${field.key}`}
          value={current}
          onChange={(event) =>
            onCommit(set(event.target.value === "" ? null : event.target.value))
          }
        >
          {(!required || current === "") && (
            <option value="">（未設定）</option>
          )}
          {current !== "" && !field.options.includes(current) && (
            <option value={current}>{current}（選択肢に無い）</option>
          )}
          {field.options.map((o) => (
            <option key={o} value={o}>
              {o}
            </option>
          ))}
        </select>
      );
      break;
    }
    case "multiselect": {
      const chosen = listItems(value);
      control = (
        <div className="bible-multi" id={`field-${field.key}`}>
          {field.options.map((o) => (
            <label key={o} className="bible-check">
              <input
                type="checkbox"
                checked={chosen.includes(o)}
                onChange={(event) => {
                  const on = event.target.checked;
                  onCommit(update((current) => toggled(current, o, on)));
                }}
              />
              {o}
            </label>
          ))}
        </div>
      );
      break;
    }
    case "ref": {
      const target = field.kind.kind === "ref" ? field.kind.target : "";
      control = (
        <RefSelect
          field={field}
          target={target}
          value={scalarText(value) ?? ""}
          candidates={candidates(target)}
          onCommit={onCommit}
          onOpenRef={onOpenRef}
        />
      );
      break;
    }
    case "list":
      control = <Chips field={field} value={value} onCommit={onCommit} />;
      break;
    case "list-ref": {
      const target =
        field.kind.kind === "list" && field.kind.item.kind === "ref"
          ? field.kind.item.target
          : "";
      const list = candidates(target);
      const items = listItems(value);
      control = (
        <div className="bible-chips">
          {keyed(items, (id) => id).map(([key, id]) => {
            const found = list.find((c) => c.id === id);
            return (
              <span
                key={key}
                className={
                  found === undefined ? "bible-chip missing" : "bible-chip"
                }
              >
                <button
                  type="button"
                  className="bible-chip-link"
                  disabled={found === undefined}
                  onClick={() => onOpenRef(target, id)}
                >
                  {found === undefined ? `${id}（無い）` : found.name}
                </button>
                <button
                  type="button"
                  aria-label={`${found?.name ?? id}を削除`}
                  onClick={() =>
                    onCommit(update((current) => withoutItem(current, id)))
                  }
                >
                  ×
                </button>
              </span>
            );
          })}
          <select
            id={`field-${field.key}`}
            aria-label={`${field.label}を追加`}
            value=""
            onChange={(event) => {
              const id = event.target.value;
              if (id !== "")
                onCommit(update((current) => appended(current, id)));
            }}
          >
            <option value="">追加…</option>
            {list
              .filter((c) => !items.includes(c.id))
              .map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name === c.id ? c.id : `${c.name}（${c.id}）`}
                </option>
              ))}
          </select>
        </div>
      );
      break;
    }
    case "readonly":
      control = (
        <div className="bible-readonly">
          <code>{JSON.stringify(value)}</code>
          <span>型に合わない形なので、ソース表示で直してください</span>
        </div>
      );
      break;
  }
  return (
    <div className={fieldClass(origin)}>
      <label className="bible-label" htmlFor={`field-${field.key}`}>
        {field.label}
        <span className="bible-label-note">
          {hint ?? kindLabel(field.kind, typeNames)}
        </span>
        <OriginBadge origin={origin} />
      </label>
      {control}
      {problems.map((d) => (
        <span
          key={`${d.code}-${d.span[0]}`}
          className={`bible-field-problem ${d.severity}`}
        >
          {d.code} {d.message}
        </span>
      ))}
    </div>
  );
}
