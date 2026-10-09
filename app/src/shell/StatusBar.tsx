// ステータスバー（docs/ui/decisions-v1.md の U05・U08、T4-17）：左は画面ごとの情報（各画面が `useScreenStatus` で
// 出す。本文は App が出す）、真ん中は知らせ、右は全画面で共通の保存の様子・問題の数（押すと検証）・形式。
// 本文の行と列は、カーソルが動くたびに App を描き直さないよう、小さなストア（`cursorStore`）から描く。

import {
  createContext,
  type ReactNode,
  useContext,
  useEffect,
  useSyncExternalStore,
} from "react";
import type { Severity } from "../core/types.ts";
import { Icon } from "./icons.tsx";
import type { Screen } from "./LeftBar.tsx";
import {
  lineColumn,
  problemsTitle,
  type SaveState,
  type StatusItems,
  saveLabel,
  saveTitle,
} from "./status.ts";

/** 画面がステータスバーの左に出すものを App へ渡す。 */
export const StatusContext = createContext<
  (screen: Screen, items: StatusItems) => void
>(() => {});

/** 画面の情報をステータスバーの左に出す（中身が変わったときだけ渡す）。 */
export function useScreenStatus(screen: Screen, items: StatusItems): void {
  const publish = useContext(StatusContext);
  const key = JSON.stringify(items);
  useEffect(() => {
    publish(screen, JSON.parse(key) as StatusItems);
  }, [publish, screen, key]);
}

/** `useScreenStatus` を、早く返す画面の描画の途中から使うための部品（何も描かない）。 */
export function ScreenStatus({
  screen,
  items,
}: {
  readonly screen: Screen;
  readonly items: StatusItems;
}) {
  useScreenStatus(screen, items);
  return null;
}

type Cursor = { readonly file: string; readonly offset: number } | null;
let cursor: Cursor = null;
const listeners = new Set<() => void>();

/** 本文のカーソル（行と列を出すため）。 */
export const cursorStore = {
  set(next: Cursor): void {
    cursor = next;
    for (const l of listeners) l();
  },
  subscribe(listener: () => void): () => void {
    listeners.add(listener);
    return () => listeners.delete(listener);
  },
  get(): Cursor {
    return cursor;
  },
};

/** 本文の行と列。 */
export function CursorPosition({
  file,
  textOf,
}: {
  readonly file: string;
  readonly textOf: (file: string) => string | undefined;
}) {
  const at = useSyncExternalStore(cursorStore.subscribe, cursorStore.get);
  const text = at === null || at.file !== file ? undefined : textOf(file);
  if (at === null || text === undefined) return null;
  const { line, column } = lineColumn(text, at.offset);
  return (
    <span>
      {line}行 {column}列
    </span>
  );
}

interface Props {
  readonly items: StatusItems;
  /** 左に差し込むもの（本文の行と列）と、その位置（既定は2つ目の後）。 */
  readonly extra?: ReactNode;
  readonly extraAt?: number;
  readonly notice: string | null;
  /** 保存の様子（作品を開いていなければ null）。 */
  readonly save: SaveState | null;
  readonly onSaveProblem: (message: string) => void;
  readonly counts: Record<Severity, number>;
  readonly onProblems: (() => void) | null;
}

export function StatusBar({
  items,
  extra,
  extraAt = 2,
  notice,
  save,
  onSaveProblem,
  counts,
  onProblems,
}: Props) {
  const left: ReactNode[] = items.map((item, i) => (
    <span
      // 中身は画面ごとに決まった並びなので、順番で見分ける。
      // biome-ignore lint/suspicious/noArrayIndexKey: 並びは画面ごとに決まっている
      key={i}
      className={[
        item.strong === true ? "sb-mode" : "",
        item.hideNarrow === true ? "hide-narrow" : "",
      ]
        .filter((c) => c !== "")
        .join(" ")}
      title={item.title}
    >
      {item.text}
    </span>
  ));
  if (extra !== undefined)
    left.splice(Math.min(extraAt, left.length), 0, extra);
  return (
    <footer className="sb">
      <div className="sb-l">{left}</div>
      <span className="sb-notice" role="status">
        {notice ?? ""}
      </span>
      <div className="sb-r">
        {save !== null && <SaveView state={save} onProblem={onSaveProblem} />}
        {onProblems !== null && (
          <button
            type="button"
            className="sb-btn"
            title={problemsTitle(counts)}
            aria-label={problemsTitle(counts)}
            onClick={onProblems}
          >
            <Icon name="error" size={13} stroke={2} className="ic-error" />
            {counts.error}
            <Icon name="warning" size={13} stroke={2} className="ic-warn" />
            {counts.warning}
            <Icon name="info" size={13} stroke={2} className="ic-info" />
            {counts.info}
          </button>
        )}
        <span className="sb-btn hide-narrow">形式 v0</span>
      </div>
    </footer>
  );
}

function SaveView({
  state,
  onProblem,
}: {
  readonly state: SaveState;
  readonly onProblem: (message: string) => void;
}) {
  const label = saveLabel(state);
  const title = saveTitle(state);
  if (state.kind === "failed")
    return (
      <button
        type="button"
        className="sb-btn save save-error"
        title={title}
        onClick={() => onProblem(state.message)}
      >
        <Icon name="error" size={13} stroke={2} />
        {label}
      </button>
    );
  return (
    <span
      className={
        state.kind === "unsaved" ? "sb-btn save save-dirty" : "sb-btn save"
      }
      title={title}
    >
      {state.kind === "unsaved" ? (
        <span className="dot" aria-hidden="true" />
      ) : (
        <Icon
          name={state.kind === "saving" ? "saving" : "saved"}
          size={13}
          stroke={2}
        />
      )}
      {label}
    </span>
  );
}
