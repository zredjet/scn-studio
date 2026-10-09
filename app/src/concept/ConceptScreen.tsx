// 企画の画面（T2-6、D057。docs/ui/mockups/01-ideas.html）：左にアイデアメモ（状態ごとの列、タグの絞り込み、
// メモの追加、直す、昇格）、中に企画書（フォームとソース。エンディングと本文の対応、主要キャラの欲求と葛藤）、
// 右にAIの問い返しと分類案（T3-10。`AiAside`）。
//
// 書き換えはすべてコアが行う（項目は直したキーの行だけ、本文はフロントマターの後ろだけ。D049）。書く時点の
// ファイルの中身を添え、外で同じところが変わっていれば、コアが書かずに知らせる。

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  type FindingRecord,
  mergedTags,
  type TaskRunner,
} from "../ai/model.ts";
import { BodyField } from "../bible/BibleScreen.tsx";
import {
  FieldInput,
  fieldClass,
  OriginBadge,
  type RefCandidate,
} from "../bible/FieldInput.tsx";
import {
  type FieldChange,
  isComposingKey,
  normalizeNewlines,
} from "../bible/fields.ts";
import { BODY_KEY, type FieldOrigin, fieldKey } from "../bible/origin.ts";
import { SourceView } from "../bible/SourceView.tsx";
import { useSourceGuard } from "../bible/useSourceGuard.ts";
import {
  createConcept,
  createIdea,
  promoteIdea,
  readBible,
  readConcept,
  readIdeas,
  setEntryBody,
  setEntryField,
  writeEntrySource,
} from "../core/api.ts";
import type {
  AssistData,
  BibleData,
  BibleWritten,
  ConceptData,
  EntryEdit,
  FieldDef,
  Idea,
  IdeaSuggestion,
  IrDiagnostic,
  JsonValue,
  Promotion,
  SceneSummary,
  SettingsWrittenExtra,
  Summary,
} from "../core/types.ts";
import { useScreenStatus } from "../shell/StatusBar.tsx";
import { conceptStatus } from "../shell/status.ts";
import { AiAside, type ConceptDraftAi } from "./AiAside.tsx";
import { IdeaCard } from "./IdeaCard.tsx";
import {
  allTags,
  FOLDED_STATUS,
  groupIdeas,
  STATUSES,
  statusCounts,
} from "./ideas.ts";

const CONCEPT_FILE = "concept.md";
const FORESHADOW_TYPE = "foreshadow";

/** 書いた後に App へ渡すもの。 */
interface Written extends SettingsWrittenExtra {
  readonly assist: AssistData;
  readonly summary: Summary;
  readonly checks: readonly IrDiagnostic[];
}

interface Props {
  readonly scenes: readonly SceneSummary[];
  /** 本文の章（スタブのシーンを足す先）。 */
  readonly chapters: readonly string[];
  /** 保存していない変更のある章（スタブのシーンを足さない）。 */
  readonly dirtyChapters: ReadonlySet<string>;
  /** 設定ファイルが外で変わった回数。 */
  readonly settingsVersion: number;
  /** アイデアメモが外で変わった回数。 */
  readonly ideasVersion: number;
  readonly onWritten: (result: Written) => void;
  /** 章にスタブのシーンを足した（エディタの中身を差し替える）。 */
  readonly onStoryWritten: (file: string, text: string) => void;
  readonly onOpenLocation: (file: string, offset: number) => void;
  /** 設定DBのエントリを開く。 */
  readonly onOpenEntry: (path: string) => void;
  /** 企画書のソースに保存していない変更があるか。 */
  readonly onUnsaved: (unsaved: boolean) => void;
  /** AIの問い返しと分類案（T3-10）。 */
  readonly ai: ConceptAi;
  /** AI が書いた項目（ファイルとキー → 出自。T4-3）。 */
  readonly aiFields: ReadonlyMap<string, FieldOrigin>;
}

/** 企画の画面の AI（T3-10）：問い返しと分類案のタスク、その結果。 */
export interface ConceptAi {
  readonly questions: TaskRunner;
  readonly sort: TaskRunner;
  readonly findings: readonly FindingRecord[];
  readonly onDismissFinding: (id: string) => void;
  readonly suggestions: readonly {
    readonly key: string;
    readonly suggestion: IdeaSuggestion;
  }[];
  readonly onHideSuggestion: (key: string) => void;
  /** AI実行の画面で、ルートの骨格のタスクを開く（無ければ null）。 */
  readonly onSkeleton: (() => void) | null;
  /** 企画書の下書き（T4-4）。 */
  readonly draft: ConceptDraftAi;
}

/** 企画書のフォームの項目。 */
const TEXT_FIELDS: readonly FieldDef[] = [
  {
    key: "logline",
    label: "ログライン",
    type: "longtext",
    kind: { kind: "longtext" },
    options: [],
  },
  {
    key: "theme",
    label: "テーマ",
    type: "text",
    kind: { kind: "text" },
    options: [],
  },
  {
    key: "tone",
    label: "トーン",
    type: "text",
    kind: { kind: "text" },
    options: [],
  },
];

const VOLUME_FIELDS: readonly (FieldDef & { readonly sub: string })[] = [
  {
    key: "volume_chars",
    sub: "chars",
    label: "想定文字数",
    type: "number",
    kind: { kind: "number" },
    options: [],
  },
  {
    key: "volume_routes",
    sub: "routes",
    label: "ルート数",
    type: "number",
    kind: { kind: "number" },
    options: [],
  },
];

const HINTS: Readonly<Record<string, string>> = {
  logline: "作品を一文で",
  theme: "",
  tone: "",
  volume_chars: "字（目安）",
  volume_routes: "目安",
};

/** `volume` の中の1つの値を差し替えた値（null なら除き、空になれば null）。 */
function withVolume(
  current: JsonValue | undefined,
  sub: string,
  value: JsonValue,
): JsonValue {
  const base =
    current !== null &&
    typeof current === "object" &&
    !Array.isArray(current) &&
    current !== undefined
      ? { ...(current as { readonly [key: string]: JsonValue }) }
      : {};
  if (value === null) delete base[sub];
  else base[sub] = value;
  return Object.keys(base).length === 0 ? null : base;
}

/** 書いている状態。 */
type Status =
  | { readonly kind: "idle" }
  | { readonly kind: "saving" }
  | { readonly kind: "saved" }
  | { readonly kind: "error"; readonly message: string };

export function ConceptScreen({
  scenes,
  chapters,
  dirtyChapters,
  settingsVersion,
  ideasVersion,
  onWritten,
  onStoryWritten,
  onOpenLocation,
  onOpenEntry,
  onUnsaved,
  ai,
  aiFields,
}: Props) {
  const [ideas, setIdeas] = useState<readonly Idea[] | null>(null);
  const [concept, setConcept] = useState<ConceptData | null>(null);
  const [bible, setBible] = useState<BibleData | null>(null);
  const [tag, setTag] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [showFolded, setShowFolded] = useState(false);
  const [view, setView] = useState<"form" | "source">("form");
  /** 企画書のソースに保存していない変更があれば、フォームへ切り替える前に聞く（T4-12、U1）。 */
  const { onSourceUnsaved, guard } = useSourceGuard(
    onUnsaved,
    `${CONCEPT_FILE} に保存していない変更があります。捨てて移りますか？`,
    "concept",
  );
  const [status, setStatus] = useState<Status>({ kind: "idle" });
  const [draft, setDraft] = useState("");
  /** 書く時点の最新の中身（メモと企画書）。 */
  const latest = useRef<{
    ideas: readonly Idea[] | null;
    concept: ConceptData | null;
  }>({ ideas: null, concept: null });
  latest.current = { ideas, concept };
  const queue = useRef<Promise<void>>(Promise.resolve());
  /** 読み直しの通し番号と、最後に画面に入れた読み直しの番号。 */
  const readSeq = useRef(0);
  const appliedSeq = useRef(0);

  /**
   * メモ・企画書・設定DBを読み直す。すでに入れた読み直しより前に頼んだものの結果は捨てる（書く前の古い中身で
   * 戻さないため）。書いた後に頼んだ読み直しは、どれが先に終わっても入る。
   */
  const reload = useCallback(async () => {
    readSeq.current += 1;
    const seq = readSeq.current;
    const [i, c, b] = await Promise.all([
      readIdeas(),
      readConcept(),
      readBible(),
    ]);
    if (seq < appliedSeq.current) return;
    appliedSeq.current = seq;
    latest.current = { ideas: i, concept: c };
    setIdeas(i);
    setConcept(c);
    setBible(b);
  }, []);

  // 開いたとき、設定・メモ・本文（シーンの一覧）が変わったときに読み直す。
  useEffect(() => {
    void settingsVersion;
    void ideasVersion;
    void scenes;
    reload().catch((e: unknown) =>
      setStatus({ kind: "error", message: String(e) }),
    );
  }, [settingsVersion, ideasVersion, scenes, reload]);

  /** 書き込みを順に行う。`write` は書く時点で呼ぶ（null なら書かない）。 */
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
            const result = await pending;
            onWritten(result);
            await reload();
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
            resolve(edit);
          } catch (e) {
            setStatus({ kind: "error", message: String(e) });
            resolve(null);
          }
        });
      }),
    [onWritten, reload],
  );

  /** 文書（メモか企画書）の1つの項目を直す。`read` は書く時点の中身と値を返す。 */
  const commitField = (
    path: string,
    key: string,
    change: FieldChange,
    read: () => {
      readonly text: string | null;
      readonly value: JsonValue | undefined;
    } | null,
  ): Promise<EntryEdit | null> =>
    enqueue(() => {
      const now = read();
      if (now === null || now.text === null) return null;
      if (change.kind === "set") {
        return setEntryField(path, key, change.value, change.seen ?? now.text);
      }
      const next = change.update(now.value);
      if (!next.ok) {
        setStatus({ kind: "error", message: next.error });
        return null;
      }
      return setEntryField(path, key, next.value, now.text);
    });

  const ideaNow = (file: string) =>
    latest.current.ideas?.find((i) => i.file === file) ?? null;

  const promote = useCallback(
    (file: string, promotion: Promotion) =>
      new Promise<string | null>((resolve) => {
        queue.current = queue.current.then(async () => {
          const idea = latest.current.ideas?.find((i) => i.file === file);
          if (idea === undefined || idea.text === null) {
            resolve("メモを読めません");
            return;
          }
          try {
            setStatus({ kind: "saving" });
            const result = await promoteIdea(file, idea.text, promotion);
            const edit = result.edit;
            // 本文を差し替えてから、表示に要る分を渡す（外部の変更の取り込みと同じ順）。
            if (
              (edit.kind === "saved" || edit.kind === "memo_unwritten") &&
              edit.scene !== null
            ) {
              onStoryWritten(edit.scene.file, edit.scene.text);
            }
            onWritten(result);
            await reload();
            if (edit.kind === "memo_unwritten") {
              setStatus({
                kind: "error",
                message: `昇格先（${edit.target}）は作りましたが、メモに書けませんでした：${edit.message}`,
              });
              resolve(null);
            } else if (edit.kind === "conflict") {
              setStatus({ kind: "idle" });
              resolve(
                "メモが外で変わっていたので、昇格していません。今の中身を読み込みました",
              );
            } else if (edit.kind === "unsupported") {
              setStatus({ kind: "idle" });
              resolve(edit.message);
            } else {
              setStatus({ kind: "saved" });
              resolve(null);
            }
          } catch (e) {
            setStatus({ kind: "idle" });
            resolve(String(e));
          }
        });
      }),
    [onWritten, onStoryWritten, reload],
  );

  const addIdea = () => {
    const body = draft;
    if (body.trim() === "") return;
    queue.current = queue.current.then(async () => {
      try {
        setStatus({ kind: "saving" });
        const created = await createIdea(body, tag === null ? [] : [tag]);
        setDraft("");
        await reload();
        setSelected(created.file);
        setStatus({ kind: "saved" });
      } catch (e) {
        setStatus({ kind: "error", message: String(e) });
      }
    });
  };

  /** 問い返しの質問を、アイデアメモにする（タグ「問い返し」）。 */
  const makeMemo = (text: string) =>
    new Promise<string | null>((resolve) => {
      queue.current = queue.current.then(async () => {
        try {
          setStatus({ kind: "saving" });
          await createIdea(text, ["問い返し"]);
          await reload();
          setStatus({ kind: "saved" });
          resolve(null);
        } catch (e) {
          setStatus({ kind: "error", message: String(e) });
          resolve(String(e));
        }
      });
    });

  /** 分類案を当てる：メモの状態を書き、タグを足す（直したキーの行だけ。コアが書く）。 */
  const applySuggestion = async (s: IdeaSuggestion): Promise<string | null> => {
    const status = s.status;
    if (status !== null) {
      const edit = await enqueue(() => {
        const idea = ideaNow(s.file);
        return idea === null || idea.text === null
          ? null
          : setEntryField(s.file, "status", status, idea.text);
      });
      if (edit?.kind !== "saved") return "メモの状態を書けませんでした";
    }
    if (s.tags.length > 0) {
      const edit = await enqueue(() => {
        const idea = ideaNow(s.file);
        return idea === null || idea.text === null
          ? null
          : setEntryField(
              s.file,
              "tags",
              mergedTags(idea.tags, s.tags),
              idea.text,
            );
      });
      if (edit?.kind !== "saved") return "メモのタグを書けませんでした";
    }
    return null;
  };

  const openTarget = (idea: Idea) => {
    const target = idea.target;
    if (target === null) return;
    if (target.kind === "scene") onOpenLocation(target.file, target.span[0]);
    else if (target.kind === "entry") onOpenEntry(target.path);
  };

  const groups = useMemo(() => groupIdeas(ideas ?? [], tag), [ideas, tag]);
  const tags = useMemo(() => allTags(ideas ?? []), [ideas]);
  const counts = useMemo(() => statusCounts(ideas ?? []), [ideas]);
  const sceneIds = useMemo(() => scenes.map((s) => s.id), [scenes]);
  const types = useMemo(
    () => (bible?.types ?? []).filter((t) => t.id !== FORESHADOW_TYPE),
    [bible],
  );
  const characterType = bible?.character_type ?? "character";
  const candidates = useCallback(
    (target: string): RefCandidate[] =>
      (bible?.files ?? []).flatMap((f) =>
        f.entry !== null && f.entry.type_id === target
          ? [
              {
                id: f.entry.id,
                name:
                  typeof f.entry.fields.name === "string"
                    ? f.entry.fields.name
                    : f.entry.id,
              },
            ]
          : [],
      ),
    [bible],
  );
  const typeNames = useMemo(
    () => new Map((bible?.types ?? []).map((t) => [t.id, t.name] as const)),
    [bible],
  );
  const missingEndings =
    concept?.endings.filter((e) => e.scenes.length === 0).length ?? 0;
  useScreenStatus(
    "concept",
    conceptStatus({
      ideas,
      order: STATUSES,
      endings: concept?.endings.length ?? null,
      missingEndings,
    }),
  );

  const conceptNow = () => {
    const c = latest.current.concept;
    return c === null ? null : c;
  };

  return (
    <div className="concept-body">
      <main className="ideas-main" aria-label="アイデアメモ">
        <header className="work-head">
          <h1 className="h1">アイデアメモ</h1>
          <span className="t-caption t-3 mono ideas-count">
            ideas/ · {ideas?.length ?? 0}枚（
            {STATUSES.map((s) => `${s} ${counts[s] ?? 0}`).join(" · ")}）
          </span>
        </header>
        <fieldset className="ideas-filter" aria-label="タグで絞り込む">
          <button
            type="button"
            className="chip"
            aria-pressed={tag === null}
            onClick={() => setTag(null)}
          >
            すべて
          </button>
          {tags.map((t) => (
            <button
              key={t}
              type="button"
              className="chip"
              aria-pressed={tag === t}
              onClick={() => setTag(tag === t ? null : t)}
            >
              {t}
            </button>
          ))}
        </fieldset>
        <div className="ideas-add">
          <label htmlFor="new-idea" className="sr-only">
            メモを追加
          </label>
          <input
            id="new-idea"
            className="input"
            value={draft}
            placeholder={
              tag === null
                ? "思いついたことを書いてEnter（未整理に入ります）"
                : `思いついたことを書いてEnter（未整理に、タグ「${tag}」で入ります）`
            }
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key !== "Enter" || isComposingKey(event)) return;
              event.preventDefault();
              addIdea();
            }}
          />
          <button
            type="button"
            className="btn btn-primary"
            disabled={draft.trim() === ""}
            onClick={addIdea}
          >
            追加
          </button>
        </div>
        {status.kind === "error" && (
          <p className="ideas-error" role="alert">
            {status.message}
          </p>
        )}
        <div className="ideas-scroll">
          {ideas === null ? (
            <p className="placeholder">読み込んでいます…</p>
          ) : (
            <div className="ideas-columns">
              {groups.map((group) => {
                const folded = group.status === FOLDED_STATUS && !showFolded;
                return (
                  <section
                    key={group.status}
                    className={
                      group.status === FOLDED_STATUS
                        ? "ideas-column folded"
                        : "ideas-column"
                    }
                    aria-label={group.status}
                  >
                    <h2>
                      {group.status} {group.ideas.length}
                    </h2>
                    {folded ? (
                      group.ideas.length > 0 && (
                        <button
                          type="button"
                          className="ideas-show-folded"
                          onClick={() => setShowFolded(true)}
                        >
                          {group.status} {group.ideas.length}件を表示
                        </button>
                      )
                    ) : (
                      <>
                        {group.ideas.map((idea) => (
                          <IdeaCard
                            key={idea.file}
                            idea={idea}
                            selected={selected === idea.file}
                            onSelect={() =>
                              setSelected(
                                selected === idea.file ? null : idea.file,
                              )
                            }
                            types={types}
                            chapters={chapters}
                            dirtyChapters={dirtyChapters}
                            sceneIds={sceneIds}
                            onCommitField={(key, change) =>
                              commitField(idea.file, key, change, () => {
                                const now = ideaNow(idea.file);
                                if (now === null) return null;
                                const values: Record<string, JsonValue> = {
                                  status: now.status,
                                  tags: now.tags,
                                  promoted_to: now.promoted_to,
                                };
                                return { text: now.text, value: values[key] };
                              })
                            }
                            onCommitBody={(body, seen) => {
                              const now = ideaNow(idea.file);
                              if (
                                now === null ||
                                normalizeNewlines(body) ===
                                  normalizeNewlines(now.body)
                              )
                                return Promise.resolve(null);
                              return enqueue(() =>
                                setEntryBody(idea.file, body, seen),
                              );
                            }}
                            onPromote={(promotion) =>
                              promote(idea.file, promotion)
                            }
                            onOpenTarget={() => openTarget(idea)}
                          />
                        ))}
                        {group.status === FOLDED_STATUS &&
                          group.ideas.length > 0 && (
                            <button
                              type="button"
                              className="ideas-show-folded"
                              onClick={() => setShowFolded(false)}
                            >
                              畳む
                            </button>
                          )}
                      </>
                    )}
                  </section>
                );
              })}
            </div>
          )}
        </div>
      </main>
      <section className="concept-panel" aria-label="企画書">
        <header className="work-head concept-head">
          <h2 className="h1">企画書</h2>
          <span className="t-caption t-3 mono ideas-count">
            {CONCEPT_FILE}
            {concept?.endings_listed &&
              ` · エンディング ${concept.endings.length}（本文に無いもの ${missingEndings}）`}
          </span>
          <span className="grow" />
          {concept?.text !== null && concept !== null && (
            <fieldset className="mode-switch" aria-label="表示モード">
              <button
                type="button"
                aria-pressed={view === "form"}
                onClick={() => guard(() => setView("form"))}
              >
                フォーム
              </button>
              <button
                type="button"
                aria-pressed={view === "source"}
                onClick={() => setView("source")}
              >
                ソース
              </button>
            </fieldset>
          )}
        </header>
        <div className="concept-scroll">
          {concept === null ? (
            <p className="placeholder">読み込んでいます…</p>
          ) : concept.text === null ? (
            <div className="concept-missing">
              <p>
                企画書（concept.md）がありません。作ると、ログライン・テーマ・エンディングなどを書く欄ができます。
              </p>
              <button
                type="button"
                className="btn btn-primary"
                onClick={() => void enqueue(() => createConcept())}
              >
                企画書を作る
              </button>
            </div>
          ) : view === "source" ? (
            <SourceView
              label={CONCEPT_FILE}
              text={concept.text}
              onUnsaved={onSourceUnsaved}
              onSave={(text, seen) =>
                enqueue(() => writeEntrySource(CONCEPT_FILE, text, seen))
              }
            />
          ) : (
            <form
              className="concept-form"
              onSubmit={(event) => event.preventDefault()}
            >
              {concept.problems.map((p) => (
                <p key={p} className="idea-problem">
                  {p}
                </p>
              ))}
              {TEXT_FIELDS.map((field) => (
                <FieldInput
                  key={field.key}
                  field={field}
                  value={concept.fields[field.key]}
                  typeNames={typeNames}
                  candidates={candidates}
                  problems={[]}
                  hint={HINTS[field.key]}
                  fileText={concept.text ?? ""}
                  onCommit={(change) =>
                    commitField(CONCEPT_FILE, field.key, change, () => {
                      const now = conceptNow();
                      return now === null
                        ? null
                        : { text: now.text, value: now.fields[field.key] };
                    })
                  }
                  onOpenRef={() => {}}
                  origin={aiFields.get(fieldKey(CONCEPT_FILE, field.key))}
                />
              ))}
              <div className="concept-grid">
                {VOLUME_FIELDS.map((field) => {
                  const volume = concept.fields.volume;
                  const value =
                    volume !== null &&
                    typeof volume === "object" &&
                    !Array.isArray(volume)
                      ? (volume as { readonly [key: string]: JsonValue })[
                          field.sub
                        ]
                      : undefined;
                  return (
                    <FieldInput
                      key={field.key}
                      field={field}
                      value={value}
                      typeNames={typeNames}
                      candidates={candidates}
                      problems={[]}
                      hint={HINTS[field.key]}
                      fileText={concept.text ?? ""}
                      onCommit={(change) =>
                        commitField(
                          CONCEPT_FILE,
                          "volume",
                          {
                            kind: "update",
                            update: (current) => {
                              if (change.kind === "set") {
                                return {
                                  ok: true,
                                  value: withVolume(
                                    current,
                                    field.sub,
                                    change.value,
                                  ),
                                };
                              }
                              const sub =
                                current !== null &&
                                typeof current === "object" &&
                                !Array.isArray(current)
                                  ? (
                                      current as {
                                        readonly [key: string]: JsonValue;
                                      }
                                    )[field.sub]
                                  : undefined;
                              const next = change.update(sub);
                              return next.ok
                                ? {
                                    ok: true,
                                    value: withVolume(
                                      current,
                                      field.sub,
                                      next.value,
                                    ),
                                  }
                                : next;
                            },
                          },
                          () => {
                            const now = conceptNow();
                            return now === null
                              ? null
                              : { text: now.text, value: now.fields.volume };
                          },
                        )
                      }
                      onOpenRef={() => {}}
                      origin={aiFields.get(fieldKey(CONCEPT_FILE, "volume"))}
                    />
                  );
                })}
              </div>
              <FieldInput
                field={{
                  key: "main_characters",
                  label: "主要キャラ",
                  type: `list<ref<${characterType}>>`,
                  kind: {
                    kind: "list",
                    item: { kind: "ref", target: characterType },
                  },
                  options: [],
                }}
                value={concept.fields.main_characters}
                typeNames={typeNames}
                candidates={candidates}
                problems={[]}
                hint="設定DBのキャラクター（欲求と葛藤は設定DBに書く）"
                fileText={concept.text ?? ""}
                onCommit={(change) =>
                  commitField(CONCEPT_FILE, "main_characters", change, () => {
                    const now = conceptNow();
                    return now === null
                      ? null
                      : { text: now.text, value: now.fields.main_characters };
                  })
                }
                onOpenRef={(_, id) => {
                  const path = concept.main_characters.find(
                    (c) => c.id === id,
                  )?.path;
                  if (path !== null && path !== undefined) onOpenEntry(path);
                }}
                origin={aiFields.get(fieldKey(CONCEPT_FILE, "main_characters"))}
              />
              {concept.main_characters.length > 0 && (
                <div className="concept-characters">
                  {concept.main_characters.map((c) => (
                    <button
                      key={c.id}
                      type="button"
                      className="concept-character"
                      disabled={c.path === null}
                      onClick={() => {
                        if (c.path !== null) onOpenEntry(c.path);
                      }}
                    >
                      <span className="concept-character-name">
                        {c.name ?? `${c.id}（設定DBに無い）`}
                      </span>
                      {c.traits.map((t) => (
                        <span key={t.label}>
                          <span className="concept-trait-label">{t.label}</span>
                          {t.value === "" ? "（未記入）" : t.value}
                        </span>
                      ))}
                    </button>
                  ))}
                </div>
              )}
              <div
                className={fieldClass(
                  aiFields.get(fieldKey(CONCEPT_FILE, "endings")),
                )}
              >
                <span className="bible-label">
                  エンディング
                  <span className="bible-label-note">
                    本文の @エンディング と照合（直すときはソースで）
                  </span>
                  <OriginBadge
                    origin={aiFields.get(fieldKey(CONCEPT_FILE, "endings"))}
                  />
                </span>
                {!concept.endings_listed ? (
                  <p className="concept-note">
                    企画書に endings
                    が書かれていないので、本文と照らしていません。
                  </p>
                ) : (
                  <div className="bible-table-wrap">
                    <table className="bible-table concept-endings">
                      <thead>
                        <tr>
                          <th scope="col">名前</th>
                          <th scope="col">到達イメージ</th>
                          <th scope="col">本文</th>
                        </tr>
                      </thead>
                      <tbody>
                        {concept.endings.map((e) => (
                          <tr key={e.id}>
                            <td className="concept-ending-name">
                              {e.name === "" ? e.id : e.name}
                              <span className="concept-ending-id">{e.id}</span>
                            </td>
                            <td className="concept-intent">{e.intent}</td>
                            <td>
                              {e.scenes.length === 0 ? (
                                <span className="concept-missing-scene">
                                  未作成
                                </span>
                              ) : (
                                e.scenes.map((s) => (
                                  <button
                                    key={s.id}
                                    type="button"
                                    className="concept-scene"
                                    title={s.title}
                                    onClick={() =>
                                      onOpenLocation(s.file, s.span[0])
                                    }
                                  >
                                    {s.id}
                                  </button>
                                ))
                              )}
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                )}
                {concept.endings_listed && concept.unplanned.length > 0 && (
                  <p className="concept-note">
                    企画書に無いエンディング（本文だけにある）：
                    {concept.unplanned.map((u) => (
                      <span key={u.id} className="concept-unplanned">
                        {u.id}
                        {u.scenes.map((s) => (
                          <button
                            key={s.id}
                            type="button"
                            className="concept-scene"
                            onClick={() => onOpenLocation(s.file, s.span[0])}
                          >
                            {s.id}
                          </button>
                        ))}
                      </span>
                    ))}
                  </p>
                )}
              </div>
              <BodyField
                key={CONCEPT_FILE}
                id="concept-body"
                label="本文（やりたいこと・やらないこと・参考にしたい感触）"
                rows={10}
                origin={aiFields.get(fieldKey(CONCEPT_FILE, BODY_KEY))}
                body={concept.body}
                fileText={concept.text ?? ""}
                onCommit={(body, seen) => {
                  const now = conceptNow();
                  if (
                    now === null ||
                    normalizeNewlines(body) === normalizeNewlines(now.body)
                  )
                    return Promise.resolve(null);
                  return enqueue(() => setEntryBody(CONCEPT_FILE, body, seen));
                }}
              />
            </form>
          )}
          <p className="concept-status" aria-live="polite">
            {status.kind === "saving"
              ? "書いています…"
              : status.kind === "saved"
                ? "書きました"
                : ""}
          </p>
        </div>
      </section>
      <AiAside
        questions={ai.questions}
        sort={ai.sort}
        findings={ai.findings}
        onDismissFinding={ai.onDismissFinding}
        suggestions={ai.suggestions}
        onHideSuggestion={ai.onHideSuggestion}
        ideas={ideas ?? []}
        onMakeMemo={makeMemo}
        onApply={applySuggestion}
        onSkeleton={ai.onSkeleton}
        draft={ai.draft}
      />
    </div>
  );
}
