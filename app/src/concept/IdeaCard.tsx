// アイデアメモのカード（企画の画面、T2-6、D057。docs/ui/mockups/01-ideas.html）。選んだカードは、状態・タグ・
// 本文・昇格先を直す欄と、昇格（設定、スタブシーン、伏線）の欄を開く。値は入力が決まったときに書く（変換中は
// 書かない。D049）。昇格はコアが行い（`promote_idea`）、メモには直したキーの行だけを書く。

import { type KeyboardEvent, useState } from "react";
import { BodyField } from "../bible/BibleScreen.tsx";
import { FieldInput } from "../bible/FieldInput.tsx";
import { type FieldChange, isComposingKey } from "../bible/fields.ts";
import type {
  EntryEdit,
  EntryType,
  FieldDef,
  Idea,
  Promotion,
} from "../core/types.ts";
import {
  defaultTitle,
  isValidId,
  STATUSES,
  suggestSceneId,
  targetLabel,
} from "./ideas.ts";

/** メモの項目（フォームの入力欄）。 */
const FIELDS: readonly FieldDef[] = [
  {
    key: "status",
    label: "状態",
    type: "select",
    kind: { kind: "select" },
    options: [...STATUSES],
  },
  {
    key: "tags",
    label: "タグ",
    type: "list<text>",
    kind: { kind: "list", item: { kind: "text" } },
    options: [],
  },
  {
    key: "promoted_to",
    label: "昇格先",
    type: "text",
    kind: { kind: "text" },
    options: [],
  },
];

const HINTS: Readonly<Record<string, string>> = {
  status: "未整理・採用・保留・没",
  tags: "Enter で足す",
  promoted_to: "scene/<シーンID> か <型ID>/<ID>。昇格で書きます",
};

/** 昇格の欄の種類。 */
type Mode = "entry" | "scene" | "foreshadow";

interface Props {
  readonly idea: Idea;
  readonly selected: boolean;
  readonly onSelect: () => void;
  /** 設定DBの型（昇格先の型を選ぶ）。伏線の型は除いてある。 */
  readonly types: readonly EntryType[];
  /** 章（スタブのシーンを足す先）と、保存していない変更のある章。 */
  readonly chapters: readonly string[];
  readonly dirtyChapters: ReadonlySet<string>;
  /** 今あるシーンID（シーンIDの案と、重なりの確かめ）。 */
  readonly sceneIds: readonly string[];
  readonly onCommitField: (
    key: string,
    change: FieldChange,
  ) => Promise<EntryEdit | null>;
  readonly onCommitBody: (
    body: string,
    seen: string,
  ) => Promise<EntryEdit | null>;
  /** 昇格する。できなければ誤りの文を返す（できれば null）。 */
  readonly onPromote: (promotion: Promotion) => Promise<string | null>;
  readonly onOpenTarget: () => void;
}

export function IdeaCard({
  idea,
  selected,
  onSelect,
  types,
  chapters,
  dirtyChapters,
  sceneIds,
  onCommitField,
  onCommitBody,
  onPromote,
  onOpenTarget,
}: Props) {
  const [mode, setMode] = useState<Mode | null>(null);
  const readable = idea.text !== null;
  const values: Record<string, string | readonly string[] | null> = {
    status: idea.status,
    tags: idea.tags,
    promoted_to: idea.promoted_to,
  };
  return (
    <article
      className={selected ? "card idea-card is-selected" : "card idea-card"}
      aria-label={idea.file}
    >
      <button
        type="button"
        className="idea-card-main"
        aria-expanded={selected}
        onClick={onSelect}
      >
        <p className="idea-text">
          {idea.body.trim() === "" ? "（本文なし）" : idea.body.trim()}
        </p>
        {idea.tags.length > 0 && (
          <span className="idea-tags">
            {idea.tags.map((tag) => (
              <span key={tag} className="tag">
                {tag}
              </span>
            ))}
          </span>
        )}
      </button>
      {idea.target !== null && (
        <button
          type="button"
          className={
            idea.target.kind === "missing"
              ? "idea-target missing"
              : "idea-target"
          }
          disabled={idea.target.kind === "missing"}
          onClick={onOpenTarget}
        >
          → {targetLabel(idea.target)}
        </button>
      )}
      {idea.problems.map((p) => (
        <span key={p} className="idea-problem">
          {p}
        </span>
      ))}
      {selected && readable && (
        <div className="idea-editor">
          <span className="idea-file">{idea.file}</span>
          {FIELDS.map((field) => (
            <FieldInput
              key={`${idea.file}:${field.key}`}
              field={field}
              value={values[field.key] ?? undefined}
              typeNames={new Map()}
              candidates={() => []}
              problems={[]}
              hint={HINTS[field.key]}
              fileText={idea.text ?? ""}
              onCommit={(change) => onCommitField(field.key, change)}
              onOpenRef={() => {}}
            />
          ))}
          <BodyField
            key={idea.file}
            id="idea-body"
            label="本文"
            rows={4}
            body={idea.body}
            fileText={idea.text ?? ""}
            onCommit={onCommitBody}
          />
          {idea.promoted_to === null && (
            <div className="idea-promote">
              <span className="idea-promote-label">昇格</span>
              {(
                [
                  ["entry", "設定"],
                  ["scene", "スタブシーン"],
                  ["foreshadow", "伏線"],
                ] as const
              ).map(([m, label]) => (
                <button
                  key={m}
                  type="button"
                  className="btn btn-sm"
                  aria-pressed={mode === m}
                  onClick={() => setMode(mode === m ? null : m)}
                >
                  {label}
                </button>
              ))}
            </div>
          )}
          {idea.promoted_to === null && mode !== null && (
            <PromoteForm
              key={mode}
              mode={mode}
              idea={idea}
              types={types}
              chapters={chapters}
              dirtyChapters={dirtyChapters}
              sceneIds={sceneIds}
              onPromote={async (promotion) => {
                const error = await onPromote(promotion);
                if (error === null) setMode(null);
                return error;
              }}
              onCancel={() => setMode(null)}
            />
          )}
        </div>
      )}
    </article>
  );
}

/** 昇格の欄：設定（型・ID・名前）、スタブシーン（章・シーンID・題名）、伏線（ID・名前）。 */
function PromoteForm({
  mode,
  idea,
  types,
  chapters,
  dirtyChapters,
  sceneIds,
  onPromote,
  onCancel,
}: {
  readonly mode: Mode;
  readonly idea: Idea;
  readonly types: readonly EntryType[];
  readonly chapters: readonly string[];
  readonly dirtyChapters: ReadonlySet<string>;
  readonly sceneIds: readonly string[];
  readonly onPromote: (promotion: Promotion) => Promise<string | null>;
  readonly onCancel: () => void;
}) {
  const [typeId, setTypeId] = useState(types[0]?.id ?? "");
  const [id, setId] = useState(
    mode === "scene" ? suggestSceneId(sceneIds) : "",
  );
  const [name, setName] = useState(
    mode === "scene" ? defaultTitle(idea.body) : "",
  );
  const clean = chapters.filter((c) => !dirtyChapters.has(c));
  const [chapter, setChapter] = useState(clean[clean.length - 1] ?? "");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const idProblem =
    id === ""
      ? null
      : !isValidId(id)
        ? "IDは英数字と _ で書きます"
        : mode === "scene" && sceneIds.includes(id)
          ? `シーンID ${id} はもう使われています`
          : null;
  const ready =
    !busy &&
    isValidId(id) &&
    idProblem === null &&
    name.trim() !== "" &&
    (mode !== "entry" || typeId !== "") &&
    (mode !== "scene" || (chapter !== "" && !dirtyChapters.has(chapter)));

  const submit = () => {
    if (!ready) return;
    const promotion: Promotion =
      mode === "scene"
        ? { kind: "scene", file: chapter, id, title: name.trim() }
        : {
            kind: "entry",
            type_id: mode === "foreshadow" ? "foreshadow" : typeId,
            id,
            name: name.trim(),
          };
    setBusy(true);
    void onPromote(promotion).then((e) => {
      setBusy(false);
      setError(e);
    });
  };
  const onKey = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== "Enter" || isComposingKey(event)) return;
    event.preventDefault();
    submit();
  };

  return (
    <form
      className="idea-promote-form"
      aria-label="昇格"
      // 暗黙の送信（入力欄の Enter）では昇格しない。変換を確定する Enter と取り違えないように、昇格は
      // ボタンと、変換中でない Enter（onKey）だけで行う。
      onSubmit={(event) => event.preventDefault()}
    >
      <p className="idea-promote-note">
        {mode === "scene"
          ? "章の末尾に、見出しと要約（メモの最初の行）だけのスタブのシーンを足します。"
          : mode === "foreshadow"
            ? "設定DBに伏線のエントリを作ります（本文はメモの本文）。仕込みと回収は設定DBで書きます。"
            : "設定DBにエントリを作ります（本文はメモの本文）。"}
        メモには状態「採用」と昇格先を書きます。
      </p>
      {mode === "entry" && (
        <label className="idea-promote-field">
          型
          <select
            value={typeId}
            onChange={(event) => setTypeId(event.target.value)}
          >
            {types.map((t) => (
              <option key={t.id} value={t.id}>
                {t.name}
              </option>
            ))}
          </select>
        </label>
      )}
      {mode === "scene" && (
        <label className="idea-promote-field">
          章
          <select
            value={chapter}
            onChange={(event) => setChapter(event.target.value)}
          >
            {chapters.map((c) => (
              <option key={c} value={c} disabled={dirtyChapters.has(c)}>
                {c}
                {dirtyChapters.has(c) ? "（保存していない変更あり）" : ""}
              </option>
            ))}
          </select>
        </label>
      )}
      <label className="idea-promote-field">
        {mode === "scene" ? "シーンID" : "ID"}
        <input
          value={id}
          spellCheck={false}
          onChange={(event) => setId(event.target.value)}
          onKeyDown={onKey}
        />
      </label>
      <label className="idea-promote-field">
        {mode === "scene" ? "題名" : "名前"}
        <input
          value={name}
          onChange={(event) => setName(event.target.value)}
          onKeyDown={onKey}
        />
      </label>
      {idProblem !== null && <span className="idea-problem">{idProblem}</span>}
      {error !== null && (
        <span className="idea-problem" role="alert">
          {error}
        </span>
      )}
      <div className="idea-promote-buttons">
        <button type="button" className="btn btn-sm" onClick={onCancel}>
          やめる
        </button>
        <button
          type="button"
          className="btn btn-primary"
          disabled={!ready}
          onClick={submit}
        >
          昇格する
        </button>
      </div>
    </form>
  );
}
