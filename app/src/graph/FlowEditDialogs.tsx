// フロー図からの編集の欄（T2-9、D060）：スタブのシーンを足す、引いた辺をジャンプか選択肢として書き足す。
// 書き足しはコアが行う（人が書いた行は変えない）。書き込む行は、足す前にコア（`stub_preview`・
// `flow_edge_preview`。足すときと同じ中身）に聞いてダイアログの中で見せる（見本 12-overlays.html の 3、T4-20）。
// 変換を確定する Enter では足さない。フォームの暗黙の送信に頼らない。

import { type KeyboardEvent, useEffect, useState } from "react";
import { isValidId, suggestSceneId } from "../concept/ideas.ts";
import { flowEdgePreview, stubPreview } from "../core/api.ts";
import type { WritePreview } from "../core/types.ts";
import { Icon } from "../shell/icons.tsx";
import { Dialog } from "../shell/Overlay.tsx";
import { writtenAt } from "../shell/writes.ts";

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

/** 書き込む行をコアに聞く（打つのが止まってから。`key` が null なら聞かない）。 */
function useWritePreview(
  key: string | null,
  ask: () => Promise<WritePreview>,
): { readonly preview: WritePreview | null; readonly problem: string | null } {
  const [state, setState] = useState<{
    readonly key: string | null;
    readonly preview: WritePreview | null;
    readonly problem: string | null;
  }>({ key: null, preview: null, problem: null });
  // biome-ignore lint/correctness/useExhaustiveDependencies: 聞く中身は `key` で決まる
  useEffect(() => {
    if (key === null) return;
    let live = true;
    const timer = setTimeout(() => {
      ask()
        .then((preview) => {
          if (live) setState({ key, preview, problem: null });
        })
        .catch((e: unknown) => {
          if (live) setState({ key, preview: null, problem: String(e) });
        });
    }, 200);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [key]);
  // 打ち直した後は、前の見せ方を出さない。
  return state.key === key
    ? { preview: state.preview, problem: state.problem }
    : { preview: null, problem: null };
}

/** 書き込む行の欄。 */
function WriteLines({
  preview,
  problem,
  waiting,
}: {
  readonly preview: WritePreview | null;
  readonly problem: string | null;
  /** まだ欄が埋まっていないときの文。 */
  readonly waiting: string;
}) {
  return (
    <div className="write-places">
      <span className="lbl">
        書き込む行
        {preview !== null && <span className="hint">{writtenAt(preview)}</span>}
      </span>
      {preview !== null ? (
        <pre className="code-box">{preview.lines.join("\n")}</pre>
      ) : problem !== null ? (
        <span className="hint write-problem" role="alert">
          {problem}
        </span>
      ) : (
        <span className="hint">{waiting}</span>
      )}
    </div>
  );
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
  /** 足すときに断られたわけ（そのときの欄の中身。打ち直したら出さない）。 */
  const [failed, setFailed] = useState<{
    readonly key: string;
    readonly message: string;
  } | null>(null);
  const [busy, setBusy] = useState(false);
  const idProblem = !isValidId(id)
    ? "シーンIDは英数字と _ で書きます"
    : sceneIds.includes(id)
      ? `シーンID ${id} はもう使われています`
      : null;
  const filled = idProblem === null && trimHalf(title) !== "" && chapter !== "";
  const key = filled
    ? JSON.stringify([chapter, id, trimHalf(title), trimHalf(summary)])
    : null;
  const written = useWritePreview(key, () =>
    stubPreview(
      chapter,
      id,
      trimHalf(title),
      trimHalf(summary) === "" ? null : trimHalf(summary),
    ),
  );
  // 書き込む行を見せてから足す（見本 12 の 3）。
  const ready = !busy && key !== null && written.preview !== null;
  const submit = () => {
    if (!ready) return;
    setBusy(true);
    void onSubmit(chapter, id, trimHalf(title), trimHalf(summary)).then((e) => {
      setBusy(false);
      if (e === null) onClose();
      else setFailed({ key, message: e });
    });
  };
  return (
    <Dialog
      title="スタブを足す"
      onClose={onClose}
      foot={
        <>
          <button type="button" className="btn" onClick={onClose}>
            キャンセル
          </button>
          <button
            type="button"
            className="btn btn-primary"
            disabled={!ready}
            onClick={submit}
          >
            足す
          </button>
        </>
      }
    >
      <div className="grid-2">
        <label className="field span-2">
          <span className="lbl">名前</span>
          <input
            className="input"
            value={title}
            autoFocus
            onChange={(event) => setTitle(event.target.value)}
            onKeyDown={onEnter(submit)}
          />
        </label>
        <label className="field">
          <span className="lbl">シーンID</span>
          <input
            className="input mono"
            value={id}
            spellCheck={false}
            aria-invalid={idProblem !== null}
            onChange={(event) => setId(event.target.value)}
            onKeyDown={onEnter(submit)}
          />
        </label>
        <label className="field">
          <span className="lbl">章</span>
          <select
            className="select"
            value={chapter}
            onChange={(event) => setChapter(event.target.value)}
          >
            {chapters.map((c) => (
              <option key={c} value={c} disabled={dirtyChapters.has(c)}>
                {c.replace(/^story\//, "")}
                {dirtyChapters.has(c) ? "（保存していない変更あり）" : ""}
              </option>
            ))}
          </select>
        </label>
        <label className="field span-2">
          <span className="lbl">
            あらすじ<span className="hint">省いてよい（summary に書く）</span>
          </span>
          <input
            className="input"
            value={summary}
            onChange={(event) => setSummary(event.target.value)}
            onKeyDown={onEnter(submit)}
          />
        </label>
      </div>
      {idProblem !== null && (
        <span className="hint write-problem" role="alert">
          {idProblem}
        </span>
      )}
      <WriteLines
        preview={written.preview}
        problem={written.problem}
        waiting={
          filled
            ? "書き込む行を求めています…"
            : "名前を打つと、章の末尾に書き込む行が出ます。保存していない変更のある章には足せません。"
        }
      />
      {failed !== null && failed.key === key && (
        <p className="notice notice-danger" role="alert">
          {failed.message}
        </p>
      )}
    </Dialog>
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
  /** 足すときに断られたわけ（そのときの欄の中身。打ち直したら出さない）。 */
  const [failed, setFailed] = useState<{
    readonly key: string;
    readonly message: string;
  } | null>(null);
  const [busy, setBusy] = useState(false);
  const filled = kind === "jump" || trimHalf(label) !== "";
  const key = filled ? JSON.stringify([kind, trimHalf(label)]) : null;
  const written = useWritePreview(key, () =>
    flowEdgePreview(
      kind === "jump"
        ? { kind, from, to }
        : { kind, from, to, label: trimHalf(label) },
    ),
  );
  // 書き込む行を見せてから足す（見本 12 の 3）。
  const ready = !busy && key !== null && written.preview !== null;
  const submit = () => {
    if (!ready) return;
    setBusy(true);
    void onSubmit(kind, trimHalf(label)).then((e) => {
      setBusy(false);
      if (e === null) onClose();
      else setFailed({ key, message: e });
    });
  };
  return (
    <Dialog
      title="辺を足す"
      onClose={onClose}
      foot={
        <>
          <button type="button" className="btn" onClick={onClose}>
            キャンセル
          </button>
          <button
            type="button"
            className="btn btn-primary"
            disabled={!ready}
            onClick={submit}
          >
            つなぐ
          </button>
        </>
      }
    >
      <div className="edge-ends">
        <label className="field">
          <span className="lbl">元</span>
          <input className="input is-readonly mono" value={from} readOnly />
        </label>
        <Icon name="forward" size={16} stroke={2} />
        <label className="field">
          <span className="lbl">先</span>
          <input className="input is-readonly mono" value={to} readOnly />
        </label>
      </div>
      <div className="field">
        <span className="lbl">つなぎ方</span>
        <fieldset className="mode-switch" aria-label="つなぎ方">
          <button
            type="button"
            aria-pressed={kind === "jump"}
            onClick={() => setKind("jump")}
          >
            ジャンプ
          </button>
          <button
            type="button"
            aria-pressed={kind === "choice"}
            onClick={() => setKind("choice")}
          >
            選択肢
          </button>
        </fieldset>
        <span className="hint">
          {kind === "jump"
            ? `シーン ${from} の終わりに -> ${to} を足します`
            : `シーン ${from} の最後の選択ブロックか、新しいブロックに足します`}
        </span>
      </div>
      {kind === "choice" && (
        <label className="field">
          <span className="lbl">選択肢の表示文</span>
          <input
            className="input"
            value={label}
            autoFocus
            onChange={(event) => setLabel(event.target.value)}
            onKeyDown={onEnter(submit)}
          />
        </label>
      )}
      <WriteLines
        preview={written.preview}
        problem={written.problem}
        waiting={
          filled
            ? "書き込む行を求めています…"
            : "選択肢の表示文を打つと、書き込む行が出ます。"
        }
      />
      {failed !== null && failed.key === key && (
        <p className="notice notice-danger" role="alert">
          {failed.message}
        </p>
      )}
    </Dialog>
  );
}
