// 変数の画面（T1-10、D049。モックが無いので、04-bible.html の「紐づく変数」の表の見た目に合わせた）：
// `variables.yaml` の宣言（型、既定値、範囲、スコープ、説明）と、宣言されずに使われた変数、読み書きの位置。
// 宣言は、選んだ変数のフォーム（T2-10、D061。直した項目の行だけを書く）か、`variables.yaml` のテキストで直す。
// 変数を足す欄と、宣言の無い変数を宣言するボタンもある。

import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { FieldInput } from "../bible/FieldInput.tsx";
import { type FieldChange, isComposingKey } from "../bible/fields.ts";
import { SourceView } from "../bible/SourceView.tsx";
import { useSourceGuard } from "../bible/useSourceGuard.ts";
import {
  addVariable,
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
import { Icon } from "../shell/icons.tsx";
import { useScreenStatus } from "../shell/StatusBar.tsx";
import { variablesStatus } from "../shell/status.ts";
import {
  commitRange,
  declFields,
  declValue,
  type RangeDraft,
  rangeShown,
} from "./form.ts";
import {
  characterOf,
  groupRows,
  locationLines,
  namespaceCounts,
  namespaceOf,
  PROBLEM_LABEL,
  problemsOf,
  rangeText,
  type VariableProblem,
} from "./rows.ts";

const PROBLEMS: readonly VariableProblem[] = [
  "written-not-read",
  "unused",
  "undeclared",
];

/** 保存のしかた（スコープ）の見せ方。 */
function scopeText(scope: string | null): string {
  if (scope === "save") return "セーブごと";
  if (scope === "persistent") return "周回で残る";
  return scope ?? "—";
}

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
  /** 本文の今の中身（書く場所・読む場所の行を出す。T4-18）。 */
  readonly textOf: (file: string) => string | undefined;
  /** 紐づくキャラクターを設定DBで開く。 */
  readonly onOpenCharacter: (id: string) => void;
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
  textOf,
  onOpenCharacter,
}: Props) {
  const [data, setData] = useState<VariablesData | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [view, setView] = useState<"table" | "source">("table");
  const [query, setQuery] = useState("");
  /** 一覧の絞り込み（名前空間か問題。T4-18）。 */
  const [namespace, setNamespace] = useState<string | null>(null);
  const [problem, setProblem] = useState<VariableProblem | null>(null);
  const [error, setError] = useState<string | null>(null);
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
  /** ソース表示に保存していない変更があれば、一覧に切り替える前に聞く。 */
  const { onSourceUnsaved, guard } = useSourceGuard(
    onUnsaved,
    `${FILE} に保存していない変更があります。捨てて移りますか？`,
    "variables",
  );

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

  const groups = useMemo(
    () => groupRows(data?.rows ?? [], { namespace, problem, query }),
    [data, namespace, problem, query],
  );
  const namespaces = useMemo(
    () => namespaceCounts(data?.rows ?? [], characters),
    [data, characters],
  );
  const row = data?.rows.find((r) => r.key === selected) ?? null;
  const rowWho = row === null ? null : characterOf(row.key, characters);
  const allRows = data?.rows ?? [];
  useScreenStatus(
    "variables",
    variablesStatus({
      count: allRows.length,
      writtenNotRead: allRows.filter(
        (r) => r.writes.length > 0 && r.reads.length === 0,
      ).length,
      unused: allRows.filter(
        (r) => r.writes.length === 0 && r.reads.length === 0,
      ).length,
    }),
  );
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
      <nav className="variables-list list-column" aria-label="名前空間と問題">
        <div className="panel-head">
          <h2 className="h-label">名前空間</h2>
        </div>
        <div className="variables-lrows">
          <button
            type="button"
            className={
              namespace === null && problem === null
                ? "lrow is-selected"
                : "lrow"
            }
            onClick={() => {
              setNamespace(null);
              setProblem(null);
            }}
          >
            <span className="name">すべて</span>
            <span className="meta">{allRows.length}</span>
          </button>
          {namespaces.map((n) => (
            <button
              key={n.namespace}
              type="button"
              className={
                namespace === n.namespace ? "lrow is-selected" : "lrow"
              }
              onClick={() => {
                setNamespace(n.namespace);
                setProblem(null);
              }}
            >
              <span className="name">
                <span className="mono">{n.namespace}</span>
                {n.who !== null && (
                  <span className="variables-who">{n.who}</span>
                )}
              </span>
              <span className="meta">{n.count}</span>
            </button>
          ))}
        </div>
        <div className="panel-head variables-problems-head">
          <h2 className="h-label">問題で絞る</h2>
        </div>
        <div className="variables-lrows">
          {PROBLEMS.map((p) => {
            const count = allRows.filter((r) =>
              problemsOf(r).includes(p),
            ).length;
            return (
              <button
                key={p}
                type="button"
                className={problem === p ? "lrow is-selected" : "lrow"}
                onClick={() => {
                  setProblem(problem === p ? null : p);
                  setNamespace(null);
                }}
              >
                {count > 0 ? (
                  <Icon
                    name="warning"
                    size={13}
                    stroke={2}
                    className="ic-warn"
                  />
                ) : (
                  <span className="variables-noicon" />
                )}
                <span className={count > 0 ? "name" : "name t-3"}>
                  {PROBLEM_LABEL[p]}
                </span>
                <span className="meta">{count}</span>
              </button>
            );
          })}
        </div>
        <p className="variables-foot">
          宣言は {FILE}。キャラに紐づく変数は、キャラのIDを名前空間にする。
        </p>
      </nav>
      <main className="variables-main" aria-label="変数の一覧">
        <header className="work-head variables-head">
          <h1 className="h1">変数</h1>
          {view === "table" && (
            <label className="search variables-search">
              <Icon name="search" size={14} />
              <input
                type="search"
                aria-label="名前・説明で絞り込む"
                placeholder="名前・説明で絞り込む"
                value={query}
                onChange={(event) => setQuery(event.target.value)}
              />
            </label>
          )}
          <span className="grow" />
          {error !== null && (
            <span className="bible-status error" role="alert">
              {error}
            </span>
          )}
          <fieldset className="mode-switch" aria-label="表示モード">
            <button
              type="button"
              aria-pressed={view === "table"}
              onClick={() => guard(() => setView("table"))}
            >
              表
            </button>
            <button
              type="button"
              aria-pressed={view === "source"}
              title={`${FILE} を直す`}
              onClick={() => setView("source")}
            >
              ソース
            </button>
          </fieldset>
          {view === "table" && (
            <button
              type="button"
              className="btn"
              aria-expanded={adding !== null}
              onClick={() =>
                setAdding(
                  adding === null ? { key: "", type: "int", values: "" } : null,
                )
              }
            >
              ＋ 変数を足す
            </button>
          )}
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
              className="btn btn-primary"
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
            {data.rows.length === 0 ? (
              <p className="placeholder">
                変数がありません。「ソース」で {FILE} に宣言を書けます。
              </p>
            ) : groups.length === 0 ? (
              <p className="placeholder">合う変数がありません。</p>
            ) : (
              <div className="table-wrap">
                <table className="table variables-table">
                  <thead>
                    <tr>
                      <th>名前</th>
                      <th>型</th>
                      <th>初期値</th>
                      <th>範囲・値</th>
                      <th>保存</th>
                      <th className="num">書き</th>
                      <th className="num">読み</th>
                      <th>問題</th>
                    </tr>
                  </thead>
                  <tbody>
                    {groups.map((g) => {
                      const who = characterOf(`${g.namespace}.`, characters);
                      return (
                        <Fragment key={g.namespace}>
                          <tr className="grp">
                            <td colSpan={8}>
                              <span className="mono">{g.namespace}</span>
                              {who !== null && (
                                <span className="variables-grp-note">
                                  {who}（設定DBのキャラクター）
                                </span>
                              )}
                            </td>
                          </tr>
                          {g.rows.map((r) => {
                            const problems = problemOf(r.span);
                            return (
                              <tr
                                key={r.key}
                                className={
                                  r.key === selected ? "is-selected" : undefined
                                }
                              >
                                <td>
                                  <button
                                    type="button"
                                    className="variables-name mono"
                                    onClick={() => setSelected(r.key)}
                                  >
                                    {r.key}
                                  </button>
                                  {r.desc !== null && r.desc !== "" && (
                                    <div className="variables-desc">
                                      {r.desc}
                                    </div>
                                  )}
                                </td>
                                <td className="mono">{r.type ?? "—"}</td>
                                <td className="mono">{r.default ?? "—"}</td>
                                <td>{rangeText(r)}</td>
                                <td>{scopeText(r.scope)}</td>
                                <td className="num">{r.writes.length}</td>
                                <td className="num">{r.reads.length}</td>
                                <td>
                                  {!r.declared && (
                                    <span className="tag tag-warn">
                                      宣言なし
                                    </span>
                                  )}
                                  {problems.map((d) => (
                                    <span
                                      key={`${d.code}-${d.span[0]}`}
                                      className="tag tag-warn"
                                      title={d.message}
                                    >
                                      {d.code}
                                    </span>
                                  ))}
                                </td>
                              </tr>
                            );
                          })}
                        </Fragment>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            )}
            <p className="variables-note-foot">
              「書き」と「読み」は本文で使っている場所の数。変数を選ぶと右の欄に場所を並べる。名前を変えるときは、宣言と参照をまとめて変える「名前の変更」（F2）を使う。
            </p>
          </div>
        )}
      </main>
      <aside className="variables-side ctx-column" aria-label="選んだ変数">
        {row === null ? (
          <>
            <div className="panel-head">
              <h2 className="h-label">選んだ変数</h2>
            </div>
            <p className="empty variables-side-empty">
              変数を選ぶと、宣言のフォームと、書く場所・読む場所が出ます。
            </p>
          </>
        ) : (
          <>
            <div className="panel-head">
              <h2 className="h2 mono">{row.key}</h2>
            </div>
            <section className="section" aria-label="宣言">
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
                      className="btn btn-sm"
                      onClick={() => declare(row.key, t, [])}
                    >
                      {t} として宣言する
                    </button>
                  ))}
                </div>
              )}
            </section>
            {rowWho !== null && (
              <section className="section" aria-label="紐づくエントリ">
                <h3 className="h-label">紐づくエントリ</h3>
                <button
                  type="button"
                  className="lrow variables-entry"
                  title="設定DBで開く"
                  onClick={() => onOpenCharacter(namespaceOf(row.key))}
                >
                  <span className="character-initial small" aria-hidden="true">
                    {[...rowWho][0] ?? ""}
                  </span>
                  <span className="name">{rowWho}</span>
                  <span className="id">{namespaceOf(row.key)}</span>
                </button>
              </section>
            )}
            {(
              [
                ["書く場所", row.writes],
                ["読む場所", row.reads],
              ] as const
            ).map(([label, list]) => (
              <section key={label} className="section" aria-label={label}>
                <h3 className="h-label">
                  {label} <span className="t-3">{list.length}</span>
                </h3>
                {list.length === 0 ? (
                  <p className="empty">ありません。</p>
                ) : (
                  <ul className="variables-places">
                    {locationLines(list, textOf).map((l) => (
                      <li key={`${l.file}-${l.offset}`}>
                        <button
                          type="button"
                          className="variables-place"
                          title={titles.get(l.scene) ?? l.scene}
                          onClick={() => onOpenLocation(l.file, l.offset)}
                        >
                          <span className="id">{l.scene}</span>
                          <span className="mono variables-place-text">
                            {l.text}
                          </span>
                          {l.line !== null && (
                            <span className="meta">{l.line}行</span>
                          )}
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </section>
            ))}
          </>
        )}
      </aside>
    </div>
  );
}
