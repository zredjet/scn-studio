// ソース表示（`SourceView`）を持つ画面（設定DB、変数、企画）で、保存していない変更を捨ててよいかを聞いてから
// 移るためのフック（T4-12、U1）。聞くのは App の保存の確認のダイアログ（T4-20）。

import { useCallback, useRef } from "react";
import { useAskUnsaved } from "../shell/UnsavedContext.ts";
import { type DraftScreen, leaveSourceAsk } from "../shell/unsaved.ts";

/**
 * `onSourceUnsaved` を `SourceView` の `onUnsaved` に渡し、表示を切り替える前に `guard` を通す。
 * 保存していない変更が無ければそのまま移る。あれば `message` で聞き、「はい」なら変更を捨てて移る。
 * `onUnsaved` は App へ伝える（画面を移る前にも聞くため）。
 */
export function useSourceGuard(
  onUnsaved: (value: boolean) => void,
  message: string,
  screen: DraftScreen,
): {
  readonly onSourceUnsaved: (value: boolean) => void;
  readonly guard: (go: () => void) => void;
} {
  const unsaved = useRef(false);
  const ask = useAskUnsaved();
  const onSourceUnsaved = useCallback(
    (value: boolean) => {
      unsaved.current = value;
      onUnsaved(value);
    },
    [onUnsaved],
  );
  const guard = useCallback(
    (go: () => void) => {
      if (!unsaved.current) {
        go();
        return;
      }
      void ask(leaveSourceAsk(screen, message)).then((choice) => {
        if (choice !== "discard") return;
        unsaved.current = false;
        go();
      });
    },
    [ask, message, screen],
  );
  return { onSourceUnsaved, guard };
}
