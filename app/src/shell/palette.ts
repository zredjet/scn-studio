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

/** この端末が macOS か（キーの書き方を ⌘ にする）。 */
export const IS_MAC =
  typeof navigator !== "undefined" && /Mac/.test(navigator.userAgent);

/**
 * キーの書き方を OS に合わせる（T4-14、U11）。CodeMirror の書き方（`Mod-Shift-P`）を、macOS では `⌘⇧P`、ほかでは
 * `Ctrl+Shift+P` にする。`F5` などの修飾の無いキーはそのまま。
 */
export function keyLabel(keys: string, mac: boolean): string {
  const parts = keys.split("-");
  const key = parts.pop() ?? "";
  const name = key.length === 1 ? key.toUpperCase() : key;
  const mods = parts.map((m) => {
    if (m === "Mod") return mac ? "⌘" : "Ctrl";
    if (m === "Shift") return mac ? "⇧" : "Shift";
    if (m === "Alt") return mac ? "⌥" : "Alt";
    if (m === "Ctrl") return mac ? "⌃" : "Ctrl";
    return m;
  });
  return mac ? `${mods.join("")}${name}` : [...mods, name].join("+");
}

/** 画面を切り替えるキー（`Mod-` に続けて押す。左端のバーの順に 1〜9、環境設定は `,`。T4-14、U10）。 */
export const SCREEN_KEYS = [
  ["1", "concept"],
  ["2", "text"],
  ["3", "flow"],
  ["4", "bible"],
  ["5", "assets"],
  ["6", "variables"],
  ["7", "check"],
  ["8", "ai"],
  ["9", "export"],
  [",", "preferences"],
] as const;

/** 画面の名前（`SCREEN_KEYS` の2つ目）。 */
export type ScreenKeyTarget = (typeof SCREEN_KEYS)[number][1];

/** `Mod-` と一緒に押したキーで移る画面。割り当てが無ければ null。 */
export function screenForKey(key: string): ScreenKeyTarget | null {
  return SCREEN_KEYS.find(([k]) => k === key)?.[1] ?? null;
}

/** 画面に移るキー（`Mod-1` など）。割り当てが無ければ null。 */
export function keyForScreen(screen: string): string | null {
  const found = SCREEN_KEYS.find(([, s]) => s === screen);
  return found === undefined ? null : `Mod-${found[0]}`;
}
