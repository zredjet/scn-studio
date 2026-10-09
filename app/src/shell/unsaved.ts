// 保存していない変更の確認（見本 12-overlays.html の 3、T4-20）：OS の確認をやめ、3択のダイアログ（保存しない・
// キャンセル・保存する）にする。何を並べ、保存できるかの判断。React と DOM に依存させない（テストする）。

/** 選んだ答え。 */
export type UnsavedChoice = "save" | "discard" | "cancel";

/** ソース表示の書きかけを持てる画面（App の `settingsDrafts` のキー）。 */
export type DraftScreen = "bible" | "variables" | "concept";

export const DRAFT_LABEL: Readonly<Record<DraftScreen, string>> = {
  bible: "設定DBのソース表示",
  variables: "変数のソース表示",
  concept: "企画のソース表示",
};

/** 聞く中身。 */
export interface UnsavedAsk {
  readonly title: string;
  readonly message: string;
  /** 保存していない本文のファイル（`story/` から）。 */
  readonly files: readonly string[];
  /** ソース表示の書きかけ（画面の名前）。 */
  readonly drafts: readonly string[];
  /** 「保存する」を出すか（本文だけのとき。ソース表示の書きかけは、その画面で保存する）。 */
  readonly canSave: boolean;
  /** 「保存する」を出さないわけ（出すなら null）。 */
  readonly note: string | null;
}

/**
 * 作品を閉じる・切り替える・アプリを終えるとき。本文はここから保存できる。ソース表示の書きかけはその画面の
 * 中にしか無いので、あれば「保存する」を出さない（キャンセルして、その画面で保存する）。
 */
export function leaveProjectAsk(
  dirty: Iterable<string>,
  drafts: Iterable<DraftScreen>,
): UnsavedAsk {
  const files = [...dirty].sort();
  const draftLabels = [...drafts].sort().map((d) => DRAFT_LABEL[d]);
  const canSave = files.length > 0 && draftLabels.length === 0;
  return {
    title: "変更を保存しますか",
    message: "閉じる前に、保存していない変更があります。",
    files,
    drafts: draftLabels,
    canSave,
    note:
      draftLabels.length === 0
        ? null
        : "ソース表示の書きかけは、ここからは保存できません。キャンセルして、その画面で保存してください。",
  };
}

/** ソース表示の書きかけを捨てて移るか（画面を移る、表示を切り替える）。保存はその画面で行う。 */
export function leaveSourceAsk(
  screen: DraftScreen,
  message: string,
): UnsavedAsk {
  return {
    title: "保存していない変更",
    message,
    files: [],
    drafts: [DRAFT_LABEL[screen]],
    canSave: false,
    note: null,
  };
}
