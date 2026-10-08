// 設定DBの画面（docs/ui/mockups/04-bible.html、T1-10、D049）：型の一覧、エントリの一覧と検索、
// 型から作るフォームとソース表示、逆引き。
//
// - フォームで直した値は、コアがエントリのファイルの、そのキーの値だけを差し替えて書く（ほかの行と本文は
//   変えない）。差し替えられない書き方なら、書かずにソース表示で直すよう知らせる。
// - 書くのは入力が決まったとき（項目ごとの自動保存）。書き込みは1つずつ順に行い、それぞれ、その時点で
//   画面が知っているファイルの中身を添える（外で変わっていれば、コアが重ねるか、書かずに知らせる）。
// - エントリを新しく作る（「＋」）。未登録の話者の登録（T1-8）もここへ渡される。
// - 型定義の編集、差分の追加、AIに渡す内容は作らない（フェーズ2・3）。

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  confirm,
  createEntry,
  entryUsage,
  readBible,
  setEntryBody,
  setEntryField,
  writeEntrySource,
} from "../core/api.ts";
import type {
  AssistData,
  BibleData,
  BibleWritten,
  EntryEdit,
  EntryFile,
  EntryRef,
  EntryUsage,
  IrDiagnostic,
  SceneSummary,
  Summary,
} from "../core/types.ts";
import { FieldInput, type RefCandidate } from "./FieldInput.tsx";
import {
  diagnosticsByKey,
  entryName,
  type FieldChange,
  fileLabel,
  formFields,
  initialOf,
  kindLabel,
  matchesQuery,
  normalizeNewlines,
  typeOfPath,
} from "./fields.ts";
import { SourceView } from "./SourceView.tsx";
import { UsagePanel } from "./UsagePanel.tsx";

/** ほかの画面からの頼み（エントリを開く、キャラクターを開く、新しいエントリを作る）。 */
export type BibleRequest =
  | { readonly kind: "entry"; readonly path: string; readonly seq: number }
  | { readonly kind: "character"; readonly id: string; readonly seq: number }
  | {
      readonly kind: "new";
      readonly typeId: string | null;
      readonly name: string;
      readonly seq: number;
    };

interface Props {
  readonly scenes: readonly SceneSummary[];
  /** すべての診断（段階0と段階1）。 */
  readonly diagnostics: readonly IrDiagnostic[];
  /** 設定ファイルが外で変わった回数（読み直す）。 */
  readonly settingsVersion: number;
  readonly request: BibleRequest | null;
  /** 書いた後：補完の材料、表示に要る分、検証の段階1の診断を画面に渡す。 */
  readonly onWritten: (result: {
    readonly assist: AssistData;
    readonly summary: Summary;
    readonly checks: readonly IrDiagnostic[];
  }) => void;
  /** 本文のその位置へ移る。 */
  readonly onOpenLocation: (file: string, offset: number) => void;
  /** 保存していないソース表示の変更があるか（閉じる前の確認）。 */
  readonly onUnsaved: (unsaved: boolean) => void;
  /** 頼みを処理した（App が頼みを消す。画面を開き直したときに繰り返さない）。 */
  readonly onRequestHandled: () => void;
}

type Status =
  | { readonly kind: "idle" }
  | { readonly kind: "saving" }
  | { readonly kind: "saved" }
  | { readonly kind: "error"; readonly message: string };

/** 添え書き（モックの「台詞の話者判定に使われます」など）。キャラクターの型のときだけ。 */
const CHARACTER_HINTS: Readonly<Record<string, string>> = {
  name: "台詞の話者判定に使われます",
  aliases: "台詞の話者判定に使われます",
};

export function BibleScreen({
  scenes,
  diagnostics,
  settingsVersion,
  request,
  onWritten,
  onOpenLocation,
  onUnsaved,
  onRequestHandled,
}: Props) {
  const [data, setData] = useState<BibleData | null>(null);
  const [typeId, setTypeId] = useState<string | null>(null);
  const [path, setPath] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [view, setView] = useState<"form" | "source">("form");
  const [status, setStatus] = useState<Status>({ kind: "idle" });
  const [usage, setUsage] = useState<EntryUsage | null>(null);
  const [creating, setCreating] = useState<{
    id: string;
    name: string;
    error: string | null;
  } | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** 書き込みを1つずつ順に行う。 */
  const queue = useRef<Promise<void>>(Promise.resolve());
  /** 最新の設定DBの中身（書いた結果は、描き直しを待たずにここへ入れる。続けて書くときに使う）。 */
  const latest = useRef(data);
  latest.current = data;
  /** ソース表示に保存していない変更があるか（切り替える前に聞く）。 */
  const unsaved = useRef(false);
  const onSourceUnsaved = useCallback(
    (value: boolean) => {
      unsaved.current = value;
      onUnsaved(value);
    },
    [onUnsaved],
  );
  /** ソース表示の保存していない変更を捨ててよいかを聞いてから、移る。 */
  const guard = useCallback((go: () => void) => {
    if (!unsaved.current) {
      go();
      return;
    }
    void confirm(
      "ソース表示に保存していない変更があります。捨てて移りますか？",
      "保存していない変更",
    ).then((ok) => {
      if (!ok) return;
      unsaved.current = false;
      go();
    });
  }, []);

  // 読む（開いたとき、設定が外で変わったとき）。
  useEffect(() => {
    void settingsVersion;
    readBible()
      .then((d) => {
        setData(d);
        setError(null);
      })
      .catch((e: unknown) => setError(String(e)));
  }, [settingsVersion]);

  // 型の一覧：型の定義と、型の定義の無いフォルダ（W804）。キャラクターの型は、定義もフォルダも無くても
  // 出す（未登録の話者を登録できるように）。
  const types = useMemo(() => {
    if (data === null) return [];
    const list = data.types.map((t) => ({
      id: t.id,
      name: t.name,
      defined: true,
    }));
    for (const id of [
      ...data.files.map((f) => typeOfPath(f.path)),
      data.character_type,
    ]) {
      if (!list.some((t) => t.id === id)) {
        list.push({ id, name: id, defined: false });
      }
    }
    return list;
  }, [data]);
  const typeNames = useMemo(
    () => new Map(types.map((t) => [t.id, t.name])),
    [types],
  );

  // 選んでいる型が無ければ、キャラクターの型（無ければ最初の型）を選ぶ。
  const currentType =
    types.find((t) => t.id === typeId) ??
    types.find((t) => t.id === data?.character_type) ??
    types[0] ??
    null;
  const files = useMemo(
    () =>
      (data?.files ?? []).filter(
        (f) => currentType !== null && typeOfPath(f.path) === currentType.id,
      ),
    [data, currentType],
  );
  const shown = files.filter((f) => matchesQuery(f, query));
  const file =
    data?.files.find(
      (f) => f.path === path && typeOfPath(f.path) === currentType?.id,
    ) ??
    files[0] ??
    null;
  const entry = file?.entry ?? null;
  const entryType = data?.types.find((t) => t.id === entry?.type_id);

  // ほかの画面からの頼み（設定DBを読み終えてから、1回だけ）。
  const handled = useRef<number | null>(null);
  useEffect(() => {
    if (request === null || data === null || handled.current === request.seq)
      return;
    handled.current = request.seq;
    onRequestHandled();
    if (request.kind === "entry") {
      setTypeId(typeOfPath(request.path));
      setPath(request.path);
      setQuery("");
    } else if (request.kind === "character") {
      const found = data.files.find(
        (f) =>
          f.entry !== null &&
          f.entry.type_id === data.character_type &&
          f.entry.id === request.id,
      );
      setTypeId(data.character_type);
      if (found !== undefined) setPath(found.path);
      setQuery("");
    } else {
      setTypeId(request.typeId ?? data.character_type);
      setCreating({ id: "", name: request.name, error: null });
    }
  }, [request, data, onRequestHandled]);

  // 逆引きを取り直す（エントリを選んだとき、設定や本文が変わったとき）。
  const entryPath = entry?.path ?? null;
  useEffect(() => {
    void scenes;
    void data;
    if (entryPath === null) {
      setUsage(null);
      return;
    }
    let live = true;
    entryUsage(entryPath)
      .then((u) => {
        if (live) setUsage(u);
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [entryPath, data, scenes]);

  /** 書いた結果を画面に入れる。 */
  const applied = useCallback(
    (result: BibleWritten): EntryEdit => {
      latest.current = result.bible;
      setData(result.bible);
      onWritten(result);
      const edit = result.edit;
      if (edit.kind === "conflict") {
        setStatus({
          kind: "error",
          message:
            "外で同じところが変わっていたので、書いていません。今の中身を読み込みました。もう一度直してください",
        });
      } else if (edit.kind === "unsupported") {
        setStatus({ kind: "error", message: edit.message });
      } else {
        setStatus({ kind: "saved" });
      }
      return edit;
    },
    [onWritten],
  );

  /** 書き込みを順に行う。`write` は書く時点で呼ぶ（null を返せば書かない）。 */
  const enqueue = useCallback(
    (write: () => Promise<BibleWritten> | null) =>
      new Promise<EntryEdit | null>((resolve) => {
        queue.current = queue.current.then(async () => {
          try {
            const pending = write();
            if (pending === null) {
              resolve(null);
              return;
            }
            setStatus({ kind: "saving" });
            resolve(applied(await pending));
          } catch (e) {
            setStatus({ kind: "error", message: String(e) });
            resolve(null);
          }
        });
      }),
    [applied],
  );

  /** 書く時点のファイル（最新の中身）。読めなければ知らせて null。 */
  const latestFile = (target: string): EntryFile | null => {
    const found = latest.current?.files.find((f) => f.path === target);
    if (found === undefined || found.text === null) {
      setStatus({ kind: "error", message: "ファイルを読めません" });
      return null;
    }
    return found;
  };

  const commitField = (
    key: string,
    change: FieldChange,
  ): Promise<EntryEdit | null> => {
    if (entry === null) return Promise.resolve(null);
    const target = entry.path;
    return enqueue(() => {
      const current = latestFile(target);
      if (current === null || current.text === null) return null;
      if (change.kind === "set") {
        return setEntryField(
          target,
          key,
          change.value,
          change.seen ?? current.text,
        );
      }
      const next = change.update(current.entry?.fields[key]);
      if (!next.ok) {
        setStatus({ kind: "error", message: next.error });
        return null;
      }
      return setEntryField(target, key, next.value, current.text);
    });
  };

  const commitBody = (
    body: string,
    seen: string,
  ): Promise<EntryEdit | null> => {
    if (entry === null) return Promise.resolve(null);
    if (normalizeNewlines(body) === normalizeNewlines(entry.body)) {
      return Promise.resolve(null);
    }
    const target = entry.path;
    return enqueue(() => setEntryBody(target, body, seen));
  };

  const candidates = useCallback(
    (target: string): RefCandidate[] => {
      if (target === "scene") {
        return scenes.map((s) => ({ id: s.id, name: `${s.id} ${s.title}` }));
      }
      return (data?.files ?? []).flatMap((f) =>
        f.entry !== null && f.entry.type_id === target
          ? [{ id: f.entry.id, name: entryName(f.entry) }]
          : [],
      );
    },
    [data, scenes],
  );

  /** エントリを開く（ソース表示の保存していない変更があれば聞く）。 */
  const openPath = (target: string) =>
    guard(() => {
      setTypeId(typeOfPath(target));
      setPath(target);
      setQuery("");
    });

  const openRef = (target: string, id: string) => {
    if (target === "scene") {
      // 本文の画面へ移る前の確認は、App が行う（ここで聞くと2回になる）。
      const s = scenes.find((x) => x.id === id);
      if (s !== undefined) onOpenLocation(s.file, s.span[0]);
      return;
    }
    const found = data?.files.find(
      (f) =>
        f.entry !== null && f.entry.type_id === target && f.entry.id === id,
    );
    if (found !== undefined) openPath(found.path);
  };

  const openUsageRef = (r: EntryRef) => {
    if (r.kind === "scene") openRef("scene", r.id);
    else if (r.path !== null) openPath(r.path);
  };

  const create = async () => {
    if (creating === null || currentType === null) return;
    try {
      const result = await createEntry(
        currentType.id,
        creating.id.trim(),
        creating.name.trim(),
      );
      applied(result);
      if (result.edit.kind === "created") setPath(result.edit.path);
      setCreating(null);
      setQuery("");
      setView("form");
    } catch (e) {
      setCreating((c) => (c === null ? null : { ...c, error: String(e) }));
    }
  };

  const fileDiagnostics = diagnostics.filter((d) => d.file === file?.path);
  const byKey = diagnosticsByKey(entry, fileDiagnostics);
  const brokenRefs = diagnostics.filter((d) => d.code === "W806").length;
  const entryCount = data?.files.filter((f) => f.entry !== null).length ?? 0;

  if (data === null) {
    return (
      <div className="bible-screen-body">
        <p className="placeholder">{error ?? "設定DBを読んでいます…"}</p>
      </div>
    );
  }

  const isCharacter = entry !== null && entry.type_id === data.character_type;
  const form = entry === null ? null : formFields(entryType, entry);
  const definedType = data.types.find((t) => t.id === currentType?.id);

  return (
    <div className="bible-screen-body">
      <aside className="bible-types" aria-label="設定の型">
        <div className="bible-types-head">
          <h2 className="bible-small-heading">設定の型</h2>
        </div>
        <div className="bible-type-list">
          {types.map((t) => (
            <button
              key={t.id}
              type="button"
              className="bible-type-item"
              aria-pressed={t.id === currentType?.id}
              onClick={() =>
                guard(() => {
                  setTypeId(t.id);
                  setPath(null);
                  setQuery("");
                  setCreating(null);
                })
              }
            >
              <span>{t.defined ? t.name : `${t.id}（型の定義なし）`}</span>
              <span className="bible-count">
                {data.files.filter((f) => typeOfPath(f.path) === t.id).length}
              </span>
            </button>
          ))}
        </div>
        {definedType !== undefined && (
          <section className="bible-type-fields" aria-label="型の項目">
            <h2 className="bible-small-heading">型の項目</h2>
            <span className="bible-type-file">
              bible/types/{definedType.id}.yaml
              {definedType.builtin !== null ? " · 組み込み" : ""}
            </span>
            <dl className="bible-type-grid">
              {definedType.fields.map((f) => (
                <div key={f.key} className="bible-type-row">
                  <dt>{f.label}</dt>
                  <dd>{kindLabel(f.kind, typeNames)}</dd>
                </div>
              ))}
            </dl>
          </section>
        )}
        <p className="bible-types-foot">
          {data.types.length}型 · {entryCount}エントリ · 参照切れ {brokenRefs}
        </p>
      </aside>

      <aside className="bible-entries" aria-label="エントリ">
        <div className="bible-search-row">
          <label className="bible-search">
            <span aria-hidden="true">⌕</span>
            <input
              type="search"
              placeholder="名前・呼び名で検索"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
            />
          </label>
          <button
            type="button"
            className="bible-add"
            title="エントリを作る"
            aria-label="エントリを作る"
            disabled={
              definedType === undefined &&
              currentType?.id !== data.character_type
            }
            onClick={() =>
              guard(() => {
                setView("form");
                setCreating({ id: "", name: "", error: null });
              })
            }
          >
            ＋
          </button>
        </div>
        {creating !== null && (
          <form
            className="bible-create"
            onSubmit={(event) => {
              event.preventDefault();
              guard(() => void create());
            }}
          >
            <span className="bible-create-title">
              {currentType?.name ?? ""}を作る
            </span>
            <label>
              名前
              <input
                value={creating.name}
                onChange={(event) =>
                  setCreating({ ...creating, name: event.target.value })
                }
                required
              />
            </label>
            <label>
              ID（英数字と _）
              <input
                // biome-ignore lint/a11y/noAutofocus: 作るときはIDから入れる
                autoFocus
                value={creating.id}
                pattern="[A-Za-z0-9_]+"
                onChange={(event) =>
                  setCreating({ ...creating, id: event.target.value })
                }
                required
              />
            </label>
            <div className="bible-create-buttons">
              <button type="submit" className="primary-small">
                作る
              </button>
              <button
                type="button"
                className="small-button"
                onClick={() => setCreating(null)}
              >
                やめる
              </button>
            </div>
            {creating.error !== null && (
              <span className="bible-field-error" role="alert">
                {creating.error}
              </span>
            )}
          </form>
        )}
        <ul className="bible-entry-list">
          {shown.map((f) => {
            const label = fileLabel(f);
            return (
              <li key={f.path}>
                <button
                  type="button"
                  className="bible-entry-item"
                  aria-pressed={f.path === file?.path}
                  onClick={() => {
                    if (f.path !== file?.path) guard(() => setPath(f.path));
                  }}
                >
                  <span className="bible-initial">{initialOf(label)}</span>
                  <span className="bible-entry-text">
                    <span className="bible-entry-name">{label}</span>
                    <span className="bible-entry-sub">
                      {f.entry === null ? "読めないエントリ" : f.entry.id}
                    </span>
                  </span>
                </button>
              </li>
            );
          })}
          {shown.length === 0 && (
            <li className="context-empty">
              {files.length === 0
                ? "エントリがありません。「＋」で作れます。"
                : "見つかりません。"}
            </li>
          )}
        </ul>
      </aside>

      <main className="bible-main" aria-label="エントリの中身">
        {file === null ? (
          <p className="placeholder">エントリを選んでください。</p>
        ) : (
          <>
            <header className="bible-head">
              <span className="bible-initial large">
                {initialOf(fileLabel(file))}
              </span>
              <div className="bible-head-text">
                <h1>{fileLabel(file)}</h1>
                <span className="bible-path">{file.path}</span>
              </div>
              <span className="flow-spacer" />
              <span className={`bible-status ${status.kind}`} role="status">
                {status.kind === "saving"
                  ? "保存中…"
                  : status.kind === "saved"
                    ? "保存済み"
                    : status.kind === "error"
                      ? status.message
                      : ""}
              </span>
              <fieldset className="mode-switch" aria-label="表示モード">
                <button
                  type="button"
                  aria-pressed={view === "form" && entry !== null}
                  disabled={entry === null}
                  onClick={() => guard(() => setView("form"))}
                >
                  フォーム
                </button>
                <button
                  type="button"
                  aria-pressed={view === "source" || entry === null}
                  onClick={() => setView("source")}
                >
                  ソース
                </button>
              </fieldset>
            </header>
            {(byKey.get("") ?? []).length > 0 && (
              <ul className="bible-file-problems">
                {(byKey.get("") ?? []).map((d) => (
                  <li key={`${d.code}-${d.span[0]}`} className={d.severity}>
                    {d.code} {d.message}
                  </li>
                ))}
              </ul>
            )}
            {file.text === null ? (
              <p className="placeholder">
                このファイルはUTF-8で読めないので、ここでは直せません。
              </p>
            ) : view === "source" || entry === null || form === null ? (
              <SourceView
                key={file.path}
                label={file.path}
                text={file.text}
                onUnsaved={onSourceUnsaved}
                onSave={(text, seen) =>
                  enqueue(() => writeEntrySource(file.path, text, seen))
                }
              />
            ) : (
              <div className="bible-form-scroll">
                <form
                  className="bible-form"
                  onSubmit={(event) => event.preventDefault()}
                >
                  {form.fields.map((field) => (
                    <FieldInput
                      key={`${entry.path}:${field.key}`}
                      field={field}
                      value={entry.fields[field.key]}
                      typeNames={typeNames}
                      candidates={candidates}
                      problems={byKey.get(field.key) ?? []}
                      hint={
                        isCharacter ? CHARACTER_HINTS[field.key] : undefined
                      }
                      fileText={file.text ?? ""}
                      onCommit={(value) => commitField(field.key, value)}
                      onOpenRef={openRef}
                    />
                  ))}
                  {form.extra.length > 0 && (
                    <div className="bible-field">
                      <span className="bible-label">
                        型に無い項目
                        <span className="bible-label-note">
                          読むだけ（ソース表示で直す）
                        </span>
                      </span>
                      <dl className="bible-extra">
                        {form.extra.map((key) => (
                          <div key={key}>
                            <dt>{key}</dt>
                            <dd>{JSON.stringify(entry.fields[key])}</dd>
                          </div>
                        ))}
                      </dl>
                    </div>
                  )}
                  {isCharacter && (
                    <div className="bible-field">
                      <span className="bible-label">
                        立ち絵差分
                        <span className="bible-label-note">
                          素材カタログから（組み込み）
                        </span>
                      </span>
                      {(data.expressions[entry.id] ?? []).length === 0 ? (
                        <span className="context-empty">
                          素材カタログに差分がありません
                        </span>
                      ) : (
                        <div className="bible-chips readonly">
                          {(data.expressions[entry.id] ?? []).map((e) => (
                            <span key={e} className="bible-chip">
                              {e}
                            </span>
                          ))}
                        </div>
                      )}
                    </div>
                  )}
                  {isCharacter && (
                    <div className="bible-field">
                      <span className="bible-label">
                        紐づく変数
                        <span className="bible-label-note">
                          キャラクターIDを名前空間にした変数（組み込み）
                        </span>
                      </span>
                      {(usage?.variables ?? []).length === 0 ? (
                        <span className="context-empty">ありません</span>
                      ) : (
                        <div className="bible-table-wrap">
                          <table className="bible-table">
                            <thead>
                              <tr>
                                <th>名前</th>
                                <th>型</th>
                                <th>初期値</th>
                                <th>スコープ</th>
                                <th>使用箇所</th>
                              </tr>
                            </thead>
                            <tbody>
                              {(usage?.variables ?? []).map((v) => (
                                <tr key={v.key}>
                                  <td className="mono">
                                    {entryName(entry)}
                                    {v.key.slice(entry.id.length)}
                                  </td>
                                  <td>
                                    {v.type ?? "宣言なし"}
                                    {v.range === null
                                      ? ""
                                      : ` ${v.range[0]}〜${v.range[1]}`}
                                  </td>
                                  <td>{v.default ?? "—"}</td>
                                  <td>{v.scope ?? "—"}</td>
                                  <td>
                                    書き {v.writes.length} · 読み{" "}
                                    {v.reads.length}
                                  </td>
                                </tr>
                              ))}
                            </tbody>
                          </table>
                        </div>
                      )}
                    </div>
                  )}
                  <BodyField
                    key={`${entry.path}:body`}
                    body={entry.body}
                    fileText={file.text ?? ""}
                    onCommit={commitBody}
                  />
                </form>
              </div>
            )}
          </>
        )}
      </main>

      <UsagePanel
        usage={entry === null ? null : usage}
        typeNames={typeNames}
        onOpenLocation={onOpenLocation}
        onOpenRef={openUsageRef}
      />
    </div>
  );
}

/**
 * 本文（Markdown）。フォーカスが外れたときに、変わっていれば書く。打ち始めたときのファイルの中身を添える
 * （打っている間に外で本文が変わっていれば、コアが書かずに知らせる）。打っていない間は、下書きと元の中身を
 * 今のものに合わせる。
 */
function BodyField({
  body,
  fileText,
  onCommit,
}: {
  readonly body: string;
  readonly fileText: string;
  readonly onCommit: (body: string, seen: string) => Promise<EntryEdit | null>;
}) {
  const [draft, setDraft] = useState(normalizeNewlines(body));
  const touched = useRef(false);
  const typed = useRef(0);
  const seen = useRef(fileText);
  const current = useRef(body);
  current.current = body;
  useEffect(() => {
    if (touched.current) return;
    seen.current = fileText;
    setDraft(normalizeNewlines(body));
  }, [body, fileText]);
  const commit = () => {
    if (!touched.current) return;
    touched.current = false;
    const stamp = typed.current;
    void onCommit(draft, seen.current).then((edit) => {
      if (edit === null || edit.kind === "created") return;
      if (edit.kind === "saved") {
        seen.current = edit.text;
        return;
      }
      if (typed.current !== stamp) return;
      if (edit.kind === "unsupported") {
        touched.current = true;
        return;
      }
      // 衝突：今の本文を出す。
      seen.current = edit.text;
      setDraft(normalizeNewlines(current.current));
    });
  };
  return (
    <div className="bible-field">
      <label className="bible-label" htmlFor="field-body">
        本文（Markdown）
        <span className="bible-label-note">フロントマターの後ろ</span>
      </label>
      <textarea
        id="field-body"
        className="bible-body"
        rows={5}
        value={draft}
        onChange={(event) => {
          touched.current = true;
          typed.current += 1;
          setDraft(event.target.value);
        }}
        onBlur={commit}
      />
    </div>
  );
}
