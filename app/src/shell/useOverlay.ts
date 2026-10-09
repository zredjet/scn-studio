// 重ね表示（パレット、シーンを開く、名前の変更、フローのダイアログ、マージ）の共通の扱い（T4-14、U12）。

import {
  type KeyboardEvent,
  type RefObject,
  useEffect,
  useRef,
  useState,
} from "react";

/**
 * 重ね表示の外枠に `ref`・`tabIndex={-1}`・`onKeyDown` を付ける。
 * - 中のどこにフォーカスがあっても Esc で閉じる（変換中の Esc では閉じない）。
 * - 開いたときに中のどこにもフォーカスが無ければ、外枠に入れる（Esc が届くように）。
 * - 閉じたら、開く前にフォーカスのあった場所へ戻す（閉じた後にほかの場所へフォーカスが移っていれば戻さない）。
 *   開く前の場所は最初の描画で覚える（中の欄の autoFocus や、入力欄にフォーカスを入れる effect より前）。
 */
export function useOverlay<T extends HTMLElement>(
  onClose: () => void,
): {
  readonly ref: RefObject<T | null>;
  readonly onKeyDown: (event: KeyboardEvent) => void;
} {
  const ref = useRef<T>(null);
  const [before] = useState(() =>
    typeof document === "undefined" ? null : document.activeElement,
  );
  useEffect(() => {
    const root = ref.current;
    if (root !== null && !root.contains(document.activeElement)) root.focus();
    return () => {
      const now = document.activeElement;
      if (
        before instanceof HTMLElement &&
        before.isConnected &&
        (now === null || now === document.body)
      ) {
        before.focus();
      }
    };
  }, [before]);
  return {
    ref,
    onKeyDown: (event) => {
      if (event.key !== "Escape") return;
      if (event.nativeEvent.isComposing || event.keyCode === 229) return;
      event.preventDefault();
      event.stopPropagation();
      onClose();
    },
  };
}

/**
 * ポップオーバーとメニュー（作品メニュー、AI の様子、再生の始め方。T4-17）を、外を押したら閉じる。開いたボタン
 * （`anchor`）を押したときは閉じない（ボタンが自分で開け閉めする）。
 */
export function useOutsideClose(
  ref: RefObject<HTMLElement | null>,
  anchor: RefObject<HTMLElement | null>,
  onClose: () => void,
): void {
  const close = useRef(onClose);
  close.current = onClose;
  useEffect(() => {
    const onDown = (event: MouseEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (ref.current?.contains(target) || anchor.current?.contains(target))
        return;
      close.current();
    };
    // フローの図などが押した知らせを止めるので、捕捉の段階で受ける。
    document.addEventListener("mousedown", onDown, true);
    return () => document.removeEventListener("mousedown", onDown, true);
  }, [ref, anchor]);
}
