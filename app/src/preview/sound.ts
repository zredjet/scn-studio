// プレビューの音の大きさ（T3-1、H5、R192）：BGM と効果音に共通の音量（0〜1）とミュート。素材ごとの大きさは変えない
// （最終的な音量はゲームの側で決める）。この端末の localStorage に、人が操作したときだけ覚える（読めなくても鳴らせる）。
// 別ウィンドウのプレビューとは合わせず、開いたときに覚えた値を読む（ウィンドウどうしは Tauri のイベントでだけつなぐ。
// D055）。

export interface SoundPrefs {
  /** 音量（0〜1）。 */
  readonly volume: number;
  readonly muted: boolean;
}

export const DEFAULT_SOUND: SoundPrefs = { volume: 1, muted: false };

export const SOUND_PREFS_KEY = "scn-studio.preview-sound";

function clamp01(n: number): number {
  return Number.isFinite(n)
    ? Math.min(1, Math.max(0, n))
    : DEFAULT_SOUND.volume;
}

/** 覚えた値を読む（無い、壊れているときは既定）。 */
export function parseSoundPrefs(raw: string | null): SoundPrefs {
  if (raw === null) return DEFAULT_SOUND;
  try {
    const v = JSON.parse(raw) as Partial<Record<keyof SoundPrefs, unknown>>;
    return {
      volume:
        typeof v.volume === "number" ? clamp01(v.volume) : DEFAULT_SOUND.volume,
      muted: v.muted === true,
    };
  } catch {
    return DEFAULT_SOUND;
  }
}

export function loadSoundPrefs(): SoundPrefs {
  try {
    return parseSoundPrefs(localStorage.getItem(SOUND_PREFS_KEY));
  } catch {
    return DEFAULT_SOUND;
  }
}

export function saveSoundPrefs(prefs: SoundPrefs): void {
  try {
    localStorage.setItem(SOUND_PREFS_KEY, JSON.stringify(prefs));
  } catch {
    // 覚えられなくても、今のプレビューには使える。
  }
}

/** 鳴らす大きさ（ミュートなら0）。 */
export function effectiveVolume(prefs: SoundPrefs): number {
  return prefs.muted ? 0 : clamp01(prefs.volume);
}

/** 音量のつまみの値（0〜100）。 */
export function volumePercent(prefs: SoundPrefs): number {
  return Math.round(clamp01(prefs.volume) * 100);
}

/** つまみの値（0〜100）から。音量を上げたら、ミュートを解く。 */
export function withPercent(prefs: SoundPrefs, percent: number): SoundPrefs {
  const volume = clamp01(percent / 100);
  return { volume, muted: prefs.muted && volume <= prefs.volume };
}
