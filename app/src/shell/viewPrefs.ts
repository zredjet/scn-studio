// 表示の好み（集中モード、タイプライタースクロール、ゴシック体。T2-8。画面の倍率。T4-30）。この端末の WebView の
// localStorage に覚える（読めなくても表示は動く）。メインウィンドウとプレビューの別ウィンドウは、開いたときに読む。

export interface ViewPrefs {
  readonly focus: boolean;
  readonly typewriter: boolean;
  readonly gothic: boolean;
  /** 画面の倍率（百分率。`ZOOM_CHOICES` のどれか）。 */
  readonly zoom: number;
}

/** 選べる画面の倍率（百分率）。 */
export const ZOOM_CHOICES: readonly number[] = [70, 80, 90, 100, 110, 120, 130];

export const DEFAULT_ZOOM = 100;

const VIEW_PREFS_KEY = "scn-studio.view";

/** 覚えた倍率を読む（数でなければ既定。範囲の外は端に、間の値はいちばん近い選択肢に）。 */
export function zoomOf(raw: unknown): number {
  if (typeof raw !== "number" || !Number.isFinite(raw)) return DEFAULT_ZOOM;
  let best = DEFAULT_ZOOM;
  for (const z of ZOOM_CHOICES) {
    if (Math.abs(z - raw) < Math.abs(best - raw)) best = z;
  }
  return best;
}

/** 覚えた文字（JSON）を読む。壊れていれば既定。 */
export function parseViewPrefs(raw: string | null): ViewPrefs {
  const none = {
    focus: false,
    typewriter: false,
    gothic: false,
    zoom: DEFAULT_ZOOM,
  };
  if (raw === null) return none;
  try {
    const v: unknown = JSON.parse(raw);
    if (typeof v !== "object" || v === null) return none;
    const p = v as Partial<Record<keyof ViewPrefs, unknown>>;
    return {
      focus: p.focus === true,
      typewriter: p.typewriter === true,
      gothic: p.gothic === true,
      zoom: zoomOf(p.zoom),
    };
  } catch {
    return none;
  }
}

export function loadViewPrefs(): ViewPrefs {
  try {
    return parseViewPrefs(localStorage.getItem(VIEW_PREFS_KEY));
  } catch {
    return parseViewPrefs(null);
  }
}

export function saveViewPrefs(prefs: ViewPrefs): void {
  try {
    localStorage.setItem(VIEW_PREFS_KEY, JSON.stringify(prefs));
  } catch {
    // 覚えられなくても、今の表示には使える。
  }
}
