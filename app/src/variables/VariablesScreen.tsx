// 変数の画面（T1-10、D049。モックが無いので、04-bible.html の「紐づく変数」の表の見た目に合わせた）：
// `variables.yaml` の宣言（型、既定値、範囲、スコープ、説明）と、宣言されずに使われた変数、読み書きの位置。
// 宣言は、選んだ変数のフォーム（T2-10、D061。直した項目の行だけを書く）か、`variables.yaml` のテキストで直す。
// 変数を足す欄と、宣言の無い変数を宣言するボタンもある。

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { FieldInput } from "../bible/FieldInput.tsx";
import { type FieldChange, isComposingKey } from "../bible/fields.ts";
import { SourceView } from "../bible/SourceView.tsx";
import {
  addVariable,
  confirm,
  readVariables,
  setVariableField,
  writeVariables,
} from "../core/api.ts";
import type {
  AssistData,
  EntryEdit,
  IrDiagnostic,
  JsonValue,
  SceneSummary,
  Summary,
  VariablesData,
  VariablesWritten,
} from "../core/types.ts";
import {
  commitRange,
  declFields,
  declValue,
  type RangeDraft,
  rangeShown,
} from "./form.ts";
import {
  characterOf,
  locationsByScene,
  matchesVariable,
  typeText,
  usageText,
} from "./rows.ts";

interface Props {
  readonly scenes: readonly SceneSummary[];
  readonly diagnostics: readonly IrDiagnostic[];
  readonly characters: readonly {
    readonly id: string;
    readonly name: string;
  }[];
  /** 設定ファイルが外で変わった回数（読み直す）。 */
  readonly settingsVersion: number;
  readonly onWritten: (result: {
    readonly assist: AssistData;
    readonly summary: Summary;
    readonly checks: readonly IrDiagnostic[];
  }) => void;
  readonly onOpenLocation: (file: string, offset: number) => void;
  readonly onUnsaved: (unsaved: boolean) => void;
}

const FILE = "variables.yaml";

export function VariablesScreen({
  scenes,
  diagnostics,
  characters,
  settingsVersion,
  onWritten,
  onOpenLocation,
  onUnsaved,
}: Props) {
  const [data, setData] = useState<VariablesData | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [view, setView] = useState<"table" | "source">("table");
  const [query, setQuery] = useState("");
  const [error, setError] = useState<string | null>(null);
  /** ソース表示に保存していない変更があるか（一覧に切り替える前に聞く）。 */
  const unsaved = useRef(false);
  /** 書く時点の中身（フォームは、書く順に1つずつ書く）。 */
  const latest = useRef<VariablesData | null>(null);
  latest.current = data;
  const queue = useRef<Promise<void>>(Promise.resolve());
  /** 範囲の入れかけ（片方だけ入れた値。もう片方が決まったら `[最小, 最大]` で書く）。欄にも出す。 */
  const [rangeDraft, setRangeDraft] = useState<RangeDraft | null>(null);
  const rangeDraftRef = useRef(rangeDraft);
  rangeDraftRef.current = rangeDraft;
  /** 値の無い変数を列挙（enum）にしようとしている（値を入れてから、値・型の順に書く）。 */
  const [enumPending, setEnumPending] = useState<string | null>(null);
  const enumPendingRef = useRef(enumPending);
  enumPendingRef.current = enumPending;
  /** 変数を足す欄。 */
  const [adding, setAdding] = useState<{
    readonly key: string;
    readonly type: string;
    readonly values: string;
  } | null>(null);
  const onSourceUnsaved = useCallback(
    (value: boolean) => {
      unsaved.current = value;
      onUnsaved(value);
    },
    [onUnsaved],
  );
  const guard = (go: () => void) => {
    if (!unsaved.current) {
      go();
      return;
    }
    void confirm(
      "variables.yaml に保存していない変更があります。捨てて移りますか？",
      "保存していない変更",
    ).then((ok) => {
      if (!ok) return;
      unsaved.current = false;
      go();
    });
  };

  // 読む（開いたとき、設定が外で変わったとき、本文が変わったとき）。
  useEffect(() => {
    void settingsVersion;
    void scenes;
    readVariables()
      .then((d) => {
        setData(d);
        setError(null);
      })
      .catch((e: unknown) => setError(String(e)));
  }, [settingsVersion, scenes]);

  const rows = useMemo(
    () => (data?.rows ?? []).filter((r) => matchesVariable(r, query)),
    [data, query],
  );
  const row = data?.rows.find((r) => r.key === selected) ?? null;
  /** フォームに出す行（列挙にするのを待っている間は、型を enum にして値の欄を出す）。 */
  const shownRow =
    row !== null && enumPending === row.key ? { ...row, type: "enum" } : row;
  // 変数を選び直したとき、宣言の範囲が変わったとき（書いた、外で変わった）は、範囲の入れかけを捨てる。
  const writtenRange = JSON.stringify(row?.range ?? null);
  useEffect(() => {
    void selected;
    void writtenRange;
    rangeDraftRef.current = null;
    setRangeDraft(null);
  }, [selected, writtenRange]);
  const titles = useMemo(
    () => new Map(scenes.map((s) => [s.id, s.title])),
    [scenes],
  );
  const fileProblems = diagnostics.filter((d) => d.file === FILE);
  const problemOf = (span: readonly [number, number] | null) =>
    span === null
      ? []
      : fileProblems.filter(
          (d) => d.span[0] === span[0] && d.span[1] === span[1],
        );

  /** 書いた結果を画面に入れる。 */
  const applied = (result: VariablesWritten): EntryEdit => {
    latest.current = result.variables;
    setData(result.variables);
    onWritten(result);
    const edit = result.edit;
    if (edit.kind === "conflict")
      setError(
        "外で同じところが変わっていたので、書いていません。今の中身を読み込みました。もう一度直してください",
      );
    else if (edit.kind === "unsupported") setError(edit.message);
    else setError(null);
    return edit;
  };

  /** フォームの1つの項目を書く（書く順に1つずつ。範囲は両方そろってから）。 */
  const commitDecl = (
    key: string,
    fieldKey: string,
    change: FieldChange,
  ): Promise<EntryEdit | null> =>
    new Promise((resolve) => {
      queue.current = queue.current.then(async () => {
        const now = latest.current;
        const row = now?.rows.find((r) => r.key === key);
        if (now === null || row === undefined) {
          resolve(null);
          return;
        }
        let value: JsonValue;
        if (change.kind === "set") value = change.value;
        else {
          const next = change.update(declValue(row, fieldKey));
          if (!next.ok) {
            setError(next.error);
            resolve(null);
            return;
          }
          value = next.value;
        }
        let field = fieldKey;
        if (fieldKey === "range_min" || fieldKey === "range_max") {
          const next = commitRange(row, rangeDraftRef.current, fieldKey, value);
          rangeDraftRef.current = next.draft;
          setRangeDraft(next.draft);
          if (next.write === undefined) {
            resolve(null);
            return;
          }
          field = "range";
          value = next.write;
        }
        // 値の無い変数を列挙にするときは、先に値を入れてもらう（値の無い列挙は読み飛ばされるため）。
        if (field === "type") {
          if (
            value === "enum" &&
            row.type !== "enum" &&
            row.values.length === 0
          ) {
            setEnumPending(key);
            setError(null);
            resolve(null);
            return;
          }
          setEnumPending(null);
        }
        // 空にした任意の項目は消す。
        if (value === "" && ["default", "scope", "desc"].includes(field))
          value = null;
        try {
          const seen =
            change.kind === "set" && change.seen !== null
              ? change.seen
              : (now.text ?? "");
          const edit = applied(await setVariableField(key, field, value, seen));
          // 列挙にするのを待っていた変数は、値を書いた後に型を書く。
          if (
            field === "values" &&
            enumPendingRef.current === key &&
            edit.kind === "saved" &&
            Array.isArray(value) &&
            value.length > 0
          ) {
            setEnumPending(null);
            resolve(
              applied(await setVariableField(key, "type", "enum", edit.text)),
            );
            return;
          }
          resolve(edit);
        } catch (e) {
          setError(String(e));
          resolve(null);
        }
      });
    });

  /** 変数を宣言する（足す欄、宣言の無い変数の「宣言する」）。 */
  const declare = (key: string, type: string, values: readonly string[]) => {
    queue.current = queue.current.then(async () => {
      try {
        applied(await addVariable(key, type, values));
        setAdding(null);
        setSelected(key.includes(".") ? key : `flag.${key}`);
      } catch (e) {
        setError(String(e));
      }
    });
  };

  /** 書く。`seen` は下書きの元になった中身（無いファイルは空の文字列）。null なら上書きする。 */
  const save = async (
    text: string,
    seen: string | null,
  ): Promise<EntryEdit | null> => {
    try {
      const result = await writeVariables(text, seen);
      setData(result.variables);
      onWritten(result);
      setError(null);
      return result.edit;
    } catch (e) {
      setError(String(e));
      return null;
    }
  };

  return (
    <div className="variables-screen-body">
      <main className="variables-main" aria-label="変数の一覧">
        <header className="variables-head">
          <h1>変数</h1>
          <span className="bible-path">{FILE}</span>
          <span className="flow-spacer" />
          {error !== null && (
            <span className="bible-status error" role="alert">
              {error}
            </span>
          )}
          {view === "table" && (
            <button
              type="button"
              className="small-button"
              onClick={() =>
                setAdding(
                  adding === null ? { key: "", type: "int", values: "" } : null,
                )
              }
            >
              ＋ 変数
            </button>
          )}
          <fieldset className="mode-switch" aria-label="表示モード">
            <button
              type="button"
              aria-pressed={view === "table"}
              onClick={() => guard(() => setView("table"))}
            >
              一覧
            </button>
            <button
              type="button"
              aria-pressed={view === "source"}
              onClick={() => setView("source")}
            >
              {FILE} を直す
            </button>
          </fieldset>
        </header>
        {adding !== null && view === "table" && (
          <div className="variables-add">
            <input
              aria-label="変数の名前"
              placeholder="名前空間.名前（例 akari.信頼）"
              value={adding.key}
              onChange={(event) =>
                setAdding({ ...adding, key: event.target.value })
              }
              onKeyDown={(event) => {
                if (isComposingKey(event) || event.key !== "Enter") return;
                event.preventDefault();
                declare(
                  adding.key,
                  adding.type,
                  adding.values
                    .split(/[、,]/)
                    .map((v) => v.trim())
                    .filter((v) => v !== ""),
                );
              }}
            />
            <select
              aria-label="型"
              value={adding.type}
              onChange={(event) =>
                setAdding({ ...adding, type: event.target.value })
              }
            >
              {["bool", "int", "string", "enum"].map((t) => (
                <option key={t} value={t}>
                  {t}
                </option>
              ))}
            </select>
            {adding.type === "enum" && (
              <input
                aria-label="値"
                placeholder="値（、で区切る）"
                value={adding.values}
                onChange={(event) =>
                  setAdding({ ...adding, values: event.target.value })
                }
              />
            )}
            <button
              type="button"
              className="primary-small"
              disabled={adding.key.trim() === ""}
              onClick={() =>
                declare(
                  adding.key,
                  adding.type,
                  adding.values
                    .split(/[、,]/)
                    .map((v) => v.trim())
                    .filter((v) => v !== ""),
                )
              }
            >
              宣言する
            </button>
          </div>
        )}
        {fileProblems.length > 0 && (
          <ul className="bible-file-problems">
            {fileProblems.map((d) => (
              <li key={`${d.code}-${d.span[0]}`} className={d.severity}>
                {d.code} {d.message}
              </li>
            ))}
          </ul>
        )}
        {data === null ? (
          <p className="placeholder">変数を読んでいます…</p>
        ) : view === "source" ? (
          <SourceView
            label={FILE}
            text={data.text ?? ""}
            onSave={save}
            onUnsaved={onSourceUnsaved}
          />
        ) : (
          <div className="variables-table-scroll">
            <label className="bible-search variables-search">
              <span aria-hidden="true">⌕</span>
              <input
                type="search"
                placeholder="名前・説明で絞り込む"
                value={query}
                onChange={(event) => setQuery(event.target.value)}
              />
            </label>
            {data.rows.length === 0 ? (
              <p className="placeholder">
                変数がありません。「{FILE} を直す」で宣言を書けます。
              </p>
            ) : (
              <div className="bible-table-wrap">
                <table className="bible-table variables-table">
                  <thead>
                    <tr>
                      <th>名前</th>
                      <th>型</th>
                      <th>初期値</th>
                      <th>スコープ</th>
                      <th>説明</th>
                      <th>使用箇所</th>
                    </tr>
                  </thead>
                  <tbody>
                    {rows.map((r) => {
                      const who = characterOf(r.key, characters);
                      const problems = problemOf(r.span);
                      return (
                        <tr
                          key={r.key}
                          className={
                            r.key === selected ? "selected" : undefined
                          }
                        >
                          <td className="mono">
                            <button
                              type="button"
                              className="variables-name"
                              onClick={() => setSelected(r.key)}
                            >
                              {r.key}
                            </button>
                            {who !== null && (
                              <span className="variables-who">{who}</span>
                            )}
                            {!r.declared && (
                              <span className="flow-badge stub">宣言なし</span>
                            )}
                            {problems.map((d) => (
                              <span
                                key={`${d.code}-${d.span[0]}`}
                                className="flow-badge warn"
                                title={d.message}
                              >
                                {d.code}
                              </span>
                            ))}
                          </td>
                          <td>{typeText(r)}</td>
                          <td>{r.default ?? "—"}</td>
                          <td>{r.scope ?? "—"}</td>
                          <td className="variables-desc">{r.desc ?? ""}</td>
                          <td>{usageText(r)}</td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            )}
          </div>
        )}
      </main>
      <aside className="bible-usage" aria-label="選んだ変数">
        {row === null ? (
          <>
            <h2 className="bible-usage-heading">選んだ変数</h2>
            <p className="context-empty">
              変数を選ぶと、宣言のフォームと、読み書きしているシーンが出ます。
            </p>
          </>
        ) : (
          <>
            <h2 className="bible-usage-heading">宣言</h2>
            <p className="variables-selected mono">{row.key}</p>
            {row.declared ? (
              <form
                className="variables-form"
                aria-label="宣言"
                onSubmit={(event) => event.preventDefault()}
              >
                {enumPending === row.key && (
                  <p className="variables-note">
                    値を入れると、値と型（enum）の順に書きます。
                  </p>
                )}
                {rangeDraft !== null && rangeDraft.key === row.key && (
                  <p className="variables-note">
                    範囲は、最小と最大の両方がそろうと書きます（両方を空にすると消します）。
                  </p>
                )}
                {declFields(shownRow ?? row).map((field) => (
                  <FieldInput
                    key={`${row.key}:${field.key}`}
                    field={field}
                    value={
                      field.key === "range_min" || field.key === "range_max"
                        ? rangeShown(row, rangeDraft, field.key)
                        : declValue(shownRow ?? row, field.key)
                    }
                    required={field.key === "type"}
                    typeNames={new Map()}
                    candidates={() => []}
                    problems={[]}
                    hint=""
                    fileText={data?.text ?? ""}
                    onCommit={(change) =>
                      commitDecl(row.key, field.key, change)
                    }
                    onOpenRef={() => {}}
                  />
                ))}
              </form>
            ) : (
              <div className="variables-undeclared">
                <p className="context-empty">
                  この変数は宣言されていません（W602）。
                </p>
                {(["int", "bool", "string"] as const).map((t) => (
                  <button
                    key={t}
                    type="button"
                    className="small-button"
                    onClick={() => declare(row.key, t, [])}
                  >
                    {t} として宣言する
                  </button>
                ))}
              </div>
            )}
            <h2 className="bible-usage-heading">読み書きしている場所</h2>
            {locationsByScene(row).length === 0 ? (
              <p className="context-empty">本文で使われていません。</p>
            ) : (
              <ul className="bible-usage-scenes">
                {locationsByScene(row).map((s) => (
                  <li key={s.scene}>
                    <button
                      type="button"
                      className="bible-usage-scene"
                      onClick={() => onOpenLocation(s.file, s.offset)}
                    >
                      <span className="bible-usage-id">{s.scene}</span>
                      <span className="bible-usage-title">
                        {titles.get(s.scene) ?? ""}
                      </span>
                      <span className="bible-usage-count">
                        {[
                          s.writes > 0 ? `書き ${s.writes}` : "",
                          s.reads > 0 ? `読み ${s.reads}` : "",
                        ]
                          .filter((x) => x !== "")
                          .join(" · ")}
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </>
        )}
      </aside>
    </div>
  );
}
