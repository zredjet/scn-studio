// コマンドパレット（Mod-Shift-P。T2-8、D059）の判断：コマンドの絞り込み。React と DOM に依存させない（テストする）。

/** パレットのコマンド。 */
export interface Command {
  readonly id: string;
  /** 見せる名前。 */
  readonly label: string;
  /** 絞り込みに使う、ほかの言い方（英語の名前など）。 */
  readonly keywords?: string;
  /** キー（見せるだけ）。 */
  readonly keys?: string;
  readonly run: () => void;
}

/** 絞り込み：名前かほかの言い方に、空白で区切った語がすべて含まれる（大文字と小文字は区別しない）。 */
export function filterCommands(
  commands: readonly Command[],
  query: string,
): Command[] {
  const words = query
    .toLowerCase()
    .split(/[\s　]+/)
    .filter((w) => w !== "");
  return commands.filter((c) => {
    const hay = `${c.label} ${c.keywords ?? ""}`.toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}
