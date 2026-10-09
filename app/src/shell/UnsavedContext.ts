// 保存していない変更を聞くダイアログを、画面（ソース表示を持つ設定DB・変数・企画）から開くための文脈（T4-20）。
// App がダイアログを出し、答えを返す。App の外（別ウィンドウなど）では OS の確認に戻る。

import { createContext, useContext } from "react";
import { confirm } from "../core/api.ts";
import type { UnsavedAsk, UnsavedChoice } from "./unsaved.ts";

export const AskUnsavedContext = createContext<
  (ask: UnsavedAsk) => Promise<UnsavedChoice>
>((ask) =>
  confirm(ask.message, ask.title).then((ok) => (ok ? "discard" : "cancel")),
);

export function useAskUnsaved(): (ask: UnsavedAsk) => Promise<UnsavedChoice> {
  return useContext(AskUnsavedContext);
}
