// フロー図からの編集の欄（T2-9、D060）：スタブのシーンを足す、引いた辺をジャンプか選択肢として書き足す。
// 書き足しはコアが行う（人が書いた行は変えない）。変換を確定する Enter では足さない。フォームの暗黙の送信に
// 頼らない。

import { type KeyboardEvent, useState } from "react";
import { isValidId, suggestSceneId } from "../concept/ideas.ts";

/** 前後の半角空白とタブを除く（全角空白は本文の一部なので残す）。 */
function trimHalf(text: string): string {
  return text.replace(/^[ \t]+|[ \t]+$/g, "");
}

/** 変換中でない Enter で `run` する。 */
function onEnter(run: () => void) {
  return (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing || event.keyCode === 229) return;
    if (event.key === "Enter") {
      event.preventDefault();
      run();
    }
  };
}

/** スタブを足す欄。 */
export function StubDialog({
  chapters,
  dirtyChapters,
  defaultChapter,
  sceneIds,
  onSubmit,
  onClose,
}: {
  readonly chapters: readonly string[];
  readonly dirtyChapters: ReadonlySet<string>;
  readonly defaultChapter: string | null;
  readonly sceneIds: readonly string[];
  /** 足す。できなければ誤りの文を返す（できれば null）。 */
  readonly onSubmit: (
    file: string,
    id: string,
    title: string,
    summary: string,
  ) => Promise<string | null>;
  readonly onClose: () => void;
}) {
  const clean = chapters.filter((c) => !dirtyChapters.has(c));
  const [chapter, setChapter] = useState(
    defaultChapter !== null && clean.includes(defaultChapter)
      ? defaultChapter
      : (clean[clean.length - 1] ?? ""),
  );
  const [id, setId] = useState(suggestSceneId(sceneIds));
  const [title, setTitle] = useState("");
  const [summary, setSummary] = useState("");
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const idProblem = !isValidId(id)
    ? "シーンIDは英数字と _ で書きます"
    : sceneIds.includes(id)
      ? `シーンID ${id} はもう使われています`
      : null;
  const ready =
    !busy && idProblem === null && trimHalf(title) !== "" && chapter !== "";
  const submit = () => {
    if (!ready) return;
    setBusy(true);
    void onSubmit(chapter, id, trimHalf(title), trimHalf(summary)).then((e) => {
      setBusy(false);
      if (e === null) onClose();
      else setProblem(e);
    });
  };
  return (
    <div className="quick-open-layer">
      <button
        type="button"
        className="quick-open-backdrop"
        aria-label="閉じる"
        tabIndex={-1}
        onClick={onClose}
      />
      <div className="rename-dialog" role="dialog" aria-label="スタブを足す">
        <h2>スタブのシーンを足す</h2>
        <p className="rename-current">
          章の末尾に、見出しと要約だけのシーンを書き足します。保存していない変更のある章には足せません。
        </p>
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
        <label className="idea-promote-field">
          シーンID
          <input
            value={id}
            spellCheck={false}
            onChange={(event) => setId(event.target.value)}
            onKeyDown={onEnter(submit)}
          />
        </label>
        <label className="idea-promote-field">
          題名
          <input
            value={title}
            // biome-ignore lint/a11y/noAutofocus: 開いてすぐ題名を打てるように
            autoFocus
            onChange={(event) => setTitle(event.target.value)}
            onKeyDown={onEnter(submit)}
          />
        </label>
        <label className="idea-promote-field">
          要約（省いてよい）
          <input
            value={summary}
            onChange={(event) => setSummary(event.target.value)}
            onKeyDown={onEnter(submit)}
          />
        </label>
        <p
          className={
            (problem ?? idProblem) ? "rename-problem" : "rename-preview"
          }
        >
          {problem ?? idProblem ?? "題名を打ってください。"}
        </p>
        <div className="rename-buttons">
          <button type="button" className="small-button" onClick={onClose}>
            やめる
          </button>
          <button
            type="button"
            className="primary-small"
            disabled={!ready}
            onClick={submit}
          >
            足す
          </button>
        </div>
      </div>
    </div>
  );
}

/** 引いた辺を書き足す欄：ジャンプか選択肢（表示文）。 */
export function EdgeDialog({
  from,
  to,
  onSubmit,
  onClose,
}: {
  readonly from: string;
  readonly to: string;
  /** 書き足す。できなければ誤りの文を返す（できれば null）。 */
  readonly onSubmit: (
    kind: "jump" | "choice",
    label: string,
  ) => Promise<string | null>;
  readonly onClose: () => void;
}) {
  const [kind, setKind] = useState<"jump" | "choice">("jump");
  const [label, setLabel] = useState("");
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const ready = !busy && (kind === "jump" || trimHalf(label) !== "");
  const submit = () => {
    if (!ready) return;
    setBusy(true);
    void onSubmit(kind, trimHalf(label)).then((e) => {
      setBusy(false);
      if (e === null) onClose();
      else setProblem(e);
    });
  };
  return (
    <div className="quick-open-layer">
      <button
        type="button"
        className="quick-open-backdrop"
        aria-label="閉じる"
        tabIndex={-1}
        onClick={onClose}
      />
      <div className="rename-dialog" role="dialog" aria-label="辺を足す">
        <h2>
          {from} → {to} を書き足す
        </h2>
        <fieldset className="flow-edge-kinds">
          <label>
            <input
              type="radio"
              name="edge-kind"
              checked={kind === "jump"}
              onChange={() => setKind("jump")}
            />
            ジャンプ（シーン {from} の終わりに <code>-&gt; {to}</code>）
          </label>
          <label>
            <input
              type="radio"
              name="edge-kind"
              checked={kind === "choice"}
              onChange={() => setKind("choice")}
            />
            選択肢（シーン {from} の最後の選択ブロックか、新しいブロックに）
          </label>
        </fieldset>
        {kind === "choice" && (
          <label className="idea-promote-field">
            選択肢の表示文
            <input
              value={label}
              // biome-ignore lint/a11y/noAutofocus: 選択肢を選んだらすぐ打てるように
              autoFocus
              onChange={(event) => setLabel(event.target.value)}
              onKeyDown={onEnter(submit)}
            />
          </label>
        )}
        {problem !== null && <p className="rename-problem">{problem}</p>}
        <div className="rename-buttons">
          <button type="button" className="small-button" onClick={onClose}>
            やめる
          </button>
          <button
            type="button"
            className="primary-small"
            disabled={!ready}
            onClick={submit}
          >
            書き足す
          </button>
        </div>
      </div>
    </div>
  );
}
