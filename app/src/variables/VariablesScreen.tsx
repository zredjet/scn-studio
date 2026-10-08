// 変数の画面（T1-10、D049。モックが無いので、04-bible.html の「紐づく変数」の表の見た目に合わせた）：
// `variables.yaml` の宣言（型、既定値、範囲、スコープ、説明）と、宣言されずに使われた変数、読み書きの位置。
// 宣言の編集は `variables.yaml` をテキストで直す（フォームでの編集はフェーズ2）。

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { SourceView } from "../bible/SourceView.tsx";
import { confirm, readVariables, writeVariables } from "../core/api.ts";
import type {
  AssistData,
  EntryEdit,
  IrDiagnostic,
  SceneSummary,
  Summary,
  VariablesData,
} from "../core/types.ts";
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
      <aside className="bible-usage" aria-label="読み書きしている場所">
        <h2 className="bible-usage-heading">読み書きしている場所</h2>
        {row === null ? (
          <p className="context-empty">
            変数を選ぶと、読み書きしているシーンが出ます。
          </p>
        ) : (
          <>
            <p className="variables-selected mono">{row.key}</p>
            {row.desc !== null && (
              <p className="variables-selected-desc">{row.desc}</p>
            )}
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
