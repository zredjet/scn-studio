// 骨格の幅の決まり（docs/ui/decisions-v1.md の U01・U06、T4-17）：狭い窓の境と、右の欄を出すかどうか。
// 右の欄の開け閉めは画面ごとに覚える（D086）。狭い窓（1280 より狭い）では閉じて始め、開くと作業領域の上に重ねる。
// 重ねた欄は覚えず、Esc・外を押す・もう一度ボタン・画面の移動で閉じる。

import type { Screen } from "./LeftBar.tsx";

/** これより狭い窓では、右の欄を閉じて始め、一覧を 220 にする（U06）。 */
export const NARROW_WIDTH = 1280;

export function isNarrow(width: number): boolean {
  return width < NARROW_WIDTH;
}

/** 右の欄のある画面（環境設定は右の欄を持たない。U19）。 */
export const CONTEXT_SCREENS: ReadonlySet<Screen> = new Set<Screen>([
  "check",
  "export",
  "text",
  "flow",
  "bible",
  "assets",
  "variables",
  "concept",
  "ai",
  "preview",
]);

export interface PanelState {
  /** 広い窓で、右の欄を閉じている画面（覚える）。 */
  readonly closed: ReadonlySet<Screen>;
  /** 狭い窓で、右の欄を重ねて開いているか（覚えない）。 */
  readonly drawer: boolean;
}

/** 右の欄の出し方：並べる・重ねる・出さない。 */
export type PanelMode = "docked" | "drawer" | "hidden";

export function panelMode(
  screen: Screen,
  narrow: boolean,
  state: PanelState,
): PanelMode {
  if (!CONTEXT_SCREENS.has(screen)) return "hidden";
  if (narrow) return state.drawer ? "drawer" : "hidden";
  return state.closed.has(screen) ? "hidden" : "docked";
}

/** 右の欄の開け閉め（タイトルバーのボタンと ⌥⌘B）。右の欄の無い画面では何もしない。 */
export function togglePanel(
  state: PanelState,
  screen: Screen,
  narrow: boolean,
): PanelState {
  if (!CONTEXT_SCREENS.has(screen)) return state;
  if (narrow) return { ...state, drawer: !state.drawer };
  const closed = new Set(state.closed);
  if (closed.has(screen)) closed.delete(screen);
  else closed.add(screen);
  return { ...state, closed };
}

/** 右の欄を出す（出力の画面でカードを選んだとき。設定と書き出すボタンが右の欄にあるため。T4-19）。 */
export function showPanel(
  state: PanelState,
  screen: Screen,
  narrow: boolean,
): PanelState {
  if (!CONTEXT_SCREENS.has(screen)) return state;
  if (narrow) return state.drawer ? state : { ...state, drawer: true };
  if (!state.closed.has(screen)) return state;
  const closed = new Set(state.closed);
  closed.delete(screen);
  return { ...state, closed };
}

/** 重ねた欄を閉じる（Esc・外を押す・画面の移動）。 */
export function closeDrawer(state: PanelState): PanelState {
  return state.drawer ? { ...state, drawer: false } : state;
}

/**
 * 窓の幅が変わった。狭くなったら重ねた欄は閉じて始め、1280 以上に広げたら今の画面の右の欄を必ず出す
 * （decisions-v1.md の「決めた」の2）。
 */
export function resized(
  state: PanelState,
  screen: Screen,
  wasNarrow: boolean,
  narrow: boolean,
): PanelState {
  if (wasNarrow === narrow) return state;
  if (narrow) return closeDrawer(state);
  if (!state.closed.has(screen)) return state;
  const closed = new Set(state.closed);
  closed.delete(screen);
  return { closed, drawer: false };
}

const PANELS_KEY = "scn-studio.panels";

/** 覚えた「閉じている画面」を読む（読めなければ、どれも開いている）。 */
export function parseClosedPanels(raw: string | null): Set<Screen> {
  if (raw === null) return new Set();
  try {
    const value: unknown = JSON.parse(raw);
    if (!Array.isArray(value)) return new Set();
    return new Set(
      value.filter(
        (s): s is Screen =>
          typeof s === "string" && CONTEXT_SCREENS.has(s as Screen),
      ),
    );
  } catch {
    return new Set();
  }
}

export function loadClosedPanels(): Set<Screen> {
  try {
    return parseClosedPanels(localStorage.getItem(PANELS_KEY));
  } catch {
    return new Set();
  }
}

export function saveClosedPanels(closed: ReadonlySet<Screen>): void {
  try {
    localStorage.setItem(PANELS_KEY, JSON.stringify([...closed].sort()));
  } catch {
    // 覚えられなくても、今の開け閉めは効く。
  }
}
