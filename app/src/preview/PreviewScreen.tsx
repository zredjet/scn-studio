// プレビューの画面（docs/ui/mockups/06-preview.html、T1-11、D050）：舞台、操作の行、情報の行、デバッグの欄。
// ランタイム（runtime.ts）で IR を解釈し、止まるたびに画面を描き直す。
//
// - 「戻る」のために、止まるたびの状態を残す。
// - 本文が変わってプレビューのデータを取り直したら、新しい本文の同じ行IDの行から続ける（T2-4）。行IDが無い
//   （消した、行IDの無い止まり）ときは、知らせて、このシーンの頭から続けるか最初からかを選ばせる。
// - 再生の頼み（エディタの「ここから再生」、デバッグの「最短経路で開始」）は、コアの最短経路の選択肢を選んで
//   ランタイムを速送りし、その行から始める（変数と舞台がそろう。D055）。
// - メインウィンドウの画面にも、別ウィンドウ（`PreviewWindow.tsx`）にも出す。縦書きは T2-5。

import {
  type ReactNode,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { routeAt, routeToScene } from "../core/api.ts";
import type { IrInline, IrSpan, PreviewData } from "../core/ir.ts";
import type { PreviewRequest, Route } from "../core/types.ts";
import { Icon } from "../shell/icons.tsx";
import { ScreenStatus } from "../shell/StatusBar.tsx";
import { previewStatus } from "../shell/status.ts";
import { type ChoicePolicy, DebugPanel } from "./DebugPanel.tsx";
import type { Value } from "./expr.ts";
import {
  autoChoice,
  canAdvance,
  choose,
  currentScene,
  initialVariables,
  jumpTo,
  lineIdsOf,
  next,
  type Program,
  program,
  replay,
  resumeAt,
  type State,
  setIncludeAiDrafts,
  setVariable,
  skip,
  start,
  stepsFromLog,
  stopLocation,
} from "./runtime.ts";
import { type PlayMode, PreviewStage } from "./Stage.tsx";
import {
  effectiveVolume,
  loadSoundPrefs,
  type SoundPrefs,
  saveSoundPrefs,
  volumePercent,
  withPercent,
} from "./sound.ts";
import { paginate, piecesOf } from "./text.ts";
import { VerticalView } from "./VerticalView.tsx";

/** プレビュー中の行（エディタに示す）。 */
export interface PreviewLine {
  readonly file: string;
  readonly lineId: string | null;
  readonly span: IrSpan;
}

interface Props {
  readonly data: PreviewData | null;
  readonly error: string | null;
  readonly assetUrl: (path: string) => string;
  /** 本文の画面へ移り、その行にカーソルを置く。 */
  readonly onShowInEditor: (line: PreviewLine) => void;
  /** 今の行（「エディタと同期」が入っていれば、エディタに印を付ける。null で消す）。 */
  readonly onLine: (line: PreviewLine | null) => void;
  /** エディタへ戻る（本文の画面）。別ウィンドウでは、ウィンドウを閉じる。 */
  readonly onClose: () => void;
  /** 閉じるボタンの名前。 */
  readonly closeLabel?: string;
  /** 再生の頼み（`seq` が変わったら従う）。 */
  readonly request?: PreviewRequest | null;
  /** 別ウィンドウで開く（メインウィンドウの画面だけ）。 */
  readonly onOpenWindow?: (() => void) | null;
}

/** 「戻る」で戻れる数。 */
const HISTORY_LIMIT = 200;
/** 効果音の札を出しておく時間。 */
const SE_LABEL_MS = 1500;
/** フェードの長さ（CSS と合わせる）。 */
const FADE_MS = 500;
/** 自動選択で選ぶまでの間。 */
const AUTO_CHOICE_MS = 400;

/** トランジションの名前のうち、切り替えるだけのもの（D050）。 */
const INSTANT = new Set(["", "即時", "None", "none"]);

function plain(inlines: readonly IrInline[]): string {
  return inlines
    .map((i) =>
      i.kind === "text" || i.kind === "emphasis"
        ? i.text
        : i.kind === "ruby"
          ? i.base
          : i.kind === "interpolation"
            ? `{=${i.expr_text}}`
            : "",
    )
    .join("");
}

function declared(data: PreviewData) {
  return data.variables.map((v) => ({
    key: v.key,
    type: v.type,
    values: v.values,
    initial: v.initial,
    scope: v.scope,
    range: v.range,
  }));
}

/** 音量とミュートを当てる。 */
function applySound(audio: HTMLAudioElement, prefs: SoundPrefs): void {
  audio.volume = effectiveVolume(prefs);
  audio.muted = prefs.muted;
}

/**
 * 効果音を1回鳴らす（ミュートなら鳴らさない）。鳴っている間は `playing` に入れておき、音量とミュートを変えたら
 * 当てる（長めの効果音も、ミュートで止める）。
 */
function playOnce(
  url: string,
  prefs: SoundPrefs,
  playing: Set<HTMLAudioElement>,
): void {
  if (prefs.muted) return;
  const audio = new Audio(url);
  applySound(audio, prefs);
  playing.add(audio);
  const done = () => playing.delete(audio);
  audio.addEventListener("ended", done);
  audio.addEventListener("error", done);
  void audio.play().catch(done);
}

export function PreviewScreen({
  data,
  error,
  assetUrl,
  onShowInEditor,
  onLine,
  onClose,
  closeLabel = "エディタへ戻る",
  request = null,
  onOpenWindow = null,
}: Props) {
  /** 実行しているデータ（本文が変わっても、選ぶまでは古いまま）。 */
  const [running, setRunning] = useState<{
    data: PreviewData;
    prog: Program;
  } | null>(null);
  const [history, setHistory] = useState<readonly State[]>([]);
  const [includeAi, setIncludeAi] = useState(true);
  const [sync, setSync] = useState(true);
  const [policy, setPolicy] = useState<ChoicePolicy>("stop");
  const [mode, setMode] = useState<PlayMode>("normal");
  const [stopSeq, setStopSeq] = useState(0);
  const [advanceSeq, setAdvanceSeq] = useState(0);
  const [seLabel, setSeLabel] = useState<string | null>(null);
  const [fades, setFades] = useState<ReadonlySet<string>>(new Set());
  /** 再生の頼みや、本文が変わって続けたことの知らせ。 */
  const [notice, setNotice] = useState<string | null>(null);
  /** 表示：ADV（舞台とメッセージウィンドウ）か、縦書き（通読。T2-5）。 */
  const [view, setView] = useState<"adv" | "vertical">("adv");
  /** 従った再生の頼み。 */
  const applied = useRef<number | null>(null);
  /** 再生の頼みの最短経路を待っている。 */
  const waiting = useRef(false);
  const bgm = useRef<HTMLAudioElement | null>(null);
  // 音量とミュート（BGM と効果音に共通。T3-1、H5）。鳴らしている BGM には、変えたときに当てる（鳴らし直さない）。
  const [sound, setSound] = useState<SoundPrefs>(loadSoundPrefs);
  const soundRef = useRef(sound);
  soundRef.current = sound;
  /** 鳴っている効果音。 */
  const effects = useRef(new Set<HTMLAudioElement>());
  /** 音量とミュートを変える（人が操作したときだけ、この端末に覚える）。 */
  const changeSound = useCallback((next: SoundPrefs) => {
    setSound(next);
    saveSoundPrefs(next);
  }, []);
  const logRef = useRef<HTMLDivElement>(null);
  const state = history[history.length - 1] ?? null;
  const prog = running?.prog ?? null;

  /** 新しい状態にする（「戻る」のために前の状態を残す）。 */
  const push = useCallback((next: State) => {
    setHistory((h) => [...h.slice(-(HISTORY_LIMIT - 1)), next]);
    setStopSeq((n) => n + 1);
  }, []);

  /** 最初から始める。永続の変数は今の値を残す。 */
  const restart = useCallback(
    (d: PreviewData, keep: ReadonlyMap<string, Value> | null) => {
      const p = program(d.ir, declared(d));
      setRunning({ data: d, prog: p });
      setHistory([]);
      setMode("normal");
      push(
        start(p, {
          includeAiDrafts: includeAi,
          vars: initialVariables(p, keep),
        }),
      );
    },
    [includeAi, push],
  );

  /** 最短経路で、ある行から始める。始められなければ知らせ、まだ動いていなければ最初から始める。 */
  const startRoute = useCallback(
    (d: PreviewData, route: Route | null, message: string | null) => {
      const fallback = (why: string) => {
        setNotice(why);
        if (running === null) restart(d, null);
      };
      if (route === null) {
        fallback(message ?? "この行からは始められません");
        return;
      }
      if (!route.reachable) {
        fallback(
          `${route.target.scene} の行には、開始シーンから着けません：${route.message ?? ""}`,
        );
        return;
      }
      const p = program(d.ir, declared(d));
      const keep = history[history.length - 1]?.vars ?? null;
      const result = replay(
        p,
        { includeAiDrafts: includeAi, vars: initialVariables(p, keep) },
        route.steps,
        route.target,
      );
      if (!result.ok) {
        fallback(result.error);
        return;
      }
      setRunning({ data: d, prog: p });
      setHistory([]);
      setMode("normal");
      push(result.state);
      setNotice(
        route.steps.length === 0
          ? `${route.target.scene} から再生しています`
          : `${route.target.scene} から再生しています（選択肢 ${route.steps.length} 個の最短経路で、変数と舞台をそろえました）`,
      );
    },
    [running, restart, history, includeAi, push],
  );

  // 初めてデータが届いたら始める（再生の頼みがあれば、それに従う）。
  useEffect(() => {
    if (data === null || running !== null) return;
    if (request !== null && applied.current !== request.seq) return;
    restart(data, null);
  }, [data, running, restart, request]);

  // 本文が変わったか：取り直したデータの中身を、動かしているデータと比べる（同じ中身なら知らせない）。
  const stale = useMemo(
    () =>
      data !== null &&
      running !== null &&
      data !== running.data &&
      JSON.stringify(data) !== JSON.stringify(running.data),
    [data, running],
  );

  // 本文が変わったら、新しい本文の同じ行IDの行から続ける（変数と舞台は今のまま）。
  useEffect(() => {
    // 再生の頼みを待っている間は続けない（頼みが、新しい本文で始め直す）。
    if (!stale || data === null || running === null || waiting.current) return;
    if (request !== null && applied.current !== request.seq) return;
    const current = history[history.length - 1];
    if (current === undefined) return;
    const lineId = stopLocation(current).lineId;
    if (lineId === null) return;
    const p = program(data.ir, declared(data));
    const resumed = resumeAt(
      p,
      { ...current, includeAiDrafts: includeAi },
      lineId,
    );
    if (resumed === null) return;
    setRunning({ data, prog: p });
    setHistory([]);
    push(resumed);
    setNotice("本文が変わったので、新しい本文の今の行から続けています");
  }, [stale, data, running, history, includeAi, push, request]);

  /** 永続の変数の今の値（最短経路を、プレビューと同じ初めの変数で探す）。 */
  const persistentNow = useCallback(
    (d: PreviewData) => {
      const vars = history[history.length - 1]?.vars;
      const out: Record<string, boolean | number | string> = {};
      if (vars === undefined) return out;
      for (const v of d.variables) {
        const value = vars.get(v.key);
        if (v.scope === "persistent" && value !== undefined) out[v.key] = value;
      }
      return out;
    },
    [history],
  );

  /** 最短経路を求めてから始める（ある行から、シーンの頭から）。 */
  const routeAndStart = useCallback(
    (d: PreviewData, target: PreviewRequest) => {
      if (target.kind === "start") return;
      const persistent = persistentNow(d);
      const asked =
        target.kind === "at"
          ? routeAt(target.file, target.offset, persistent)
          : routeToScene(target.scene, persistent);
      const missing =
        target.kind === "at"
          ? "この行からは始められません（シーンの外か、シーンの最後の命令より後ろです）"
          : `${target.scene} には命令がありません`;
      waiting.current = true;
      asked
        .then((route) => startRoute(d, route, route === null ? missing : null))
        .catch((e: unknown) => setNotice(String(e)))
        .finally(() => {
          waiting.current = false;
        });
    },
    [persistentNow, startRoute],
  );

  // 再生の頼みに従う（本文が変わったときの続きより後に書く。同じ描き直しで両方が動いたら、頼みが勝つ）。
  useEffect(() => {
    if (data === null || request === null || applied.current === request.seq) {
      return;
    }
    applied.current = request.seq;
    // 縦書きで読んでいても、再生の頼みは ADV で見せる。
    setView("adv");
    if (request.kind === "start") {
      setNotice(null);
      restart(data, history[history.length - 1]?.vars ?? null);
      return;
    }
    routeAndStart(data, request);
  }, [data, request, restart, routeAndStart, history]);

  // 縦書きビューの「プレビューで選んだ道」と、永続の変数の今の値。
  const current = history[history.length - 1] ?? null;
  const readingSteps = useMemo(
    () =>
      current === null || running === null
        ? []
        : stepsFromLog(running.prog, current.log),
    [current, running],
  );
  const persistent = useMemo(
    () => (data === null ? {} : persistentNow(data)),
    [data, persistentNow],
  );

  // 出来事（効果音、トランジション）。
  useEffect(() => {
    if (state === null || running === null) return;
    const fading = new Set<string>();
    for (const e of state.events) {
      if (e.kind === "se") {
        const sound = running.data.assets.se[e.name];
        if (sound?.file !== null && sound?.file !== undefined) {
          playOnce(assetUrl(sound.file), soundRef.current, effects.current);
        } else {
          setSeLabel(
            `SE ${e.name} · ${sound === undefined ? "カタログに無い" : "仮素材"}（無音）`,
          );
        }
      } else if (e.kind === "transition" && !INSTANT.has(e.name)) {
        fading.add(e.target);
      } else if (e.kind === "fx") {
        setSeLabel(`効果：${e.name}（再現しません）`);
      }
    }
    if (fading.size === 0) return;
    setFades(fading);
    const timer = setTimeout(() => setFades(new Set()), FADE_MS);
    return () => clearTimeout(timer);
  }, [state, running, assetUrl]);

  // 効果音の札を消す。
  useEffect(() => {
    if (seLabel === null) return;
    const timer = setTimeout(() => setSeLabel(null), SE_LABEL_MS);
    return () => clearTimeout(timer);
  }, [seLabel]);

  // BGM。ファイルがあれば鳴らし、無ければ無音。縦書きで読んでいる間は止める（縦書きの帯には音の操作を置かない。
  // 見本 06b。ADV に戻ると鳴らし直す。T4-19）。
  const bgmName = state?.stage.bgm ?? null;
  useEffect(() => {
    bgm.current?.pause();
    bgm.current = null;
    if (bgmName === null || running === null || view === "vertical") return;
    const sound = running.data.assets.bgm[bgmName];
    if (sound?.file === null || sound?.file === undefined) return;
    const audio = new Audio(assetUrl(sound.file));
    audio.loop = sound.loop;
    applySound(audio, soundRef.current);
    void audio.play().catch(() => {});
    bgm.current = audio;
    return () => audio.pause();
  }, [bgmName, running, assetUrl, view]);

  // 音量とミュートを変えたら、鳴らしている BGM と効果音に当てる（鳴らし直さない）。別ウィンドウのプレビューとは
  // 合わせない（ウィンドウどうしは Tauri のイベントでだけつなぐ。D055）。開いたときに、覚えた値を読む。
  useEffect(() => {
    if (bgm.current !== null) applySound(bgm.current, sound);
    for (const audio of effects.current) {
      if (sound.muted) {
        audio.pause();
        effects.current.delete(audio);
      } else {
        applySound(audio, sound);
      }
    }
  }, [sound]);

  // 閉じたら、鳴っている効果音を止める。
  useEffect(() => {
    const playing = effects.current;
    return () => {
      for (const audio of playing) audio.pause();
      playing.clear();
    };
  }, []);

  // エディタと同期：今の行を知らせる。
  const scene =
    state === null || prog === null ? undefined : currentScene(prog, state);
  const location = state === null ? null : stopLocation(state);
  const line: PreviewLine | null =
    scene === undefined || location === null
      ? null
      : { file: scene.file, lineId: location.lineId, span: location.span };
  const lineKey =
    line === null ? "" : `${line.file}:${line.span[0]}:${line.lineId}`;
  const lineRef = useRef(line);
  lineRef.current = line;
  useEffect(() => {
    void lineKey;
    onLine(sync ? lineRef.current : null);
  }, [lineKey, sync, onLine]);
  // 画面を離れても印は残す（本文へ戻ったときに、最後に見ていた行が分かるように）。

  /** 次へ。 */
  const advance = useCallback(() => {
    if (state === null || prog === null) return;
    if (state.stop.kind === "line") {
      setAdvanceSeq((n) => n + 1);
      return;
    }
    if (canAdvance(state.stop)) push(next(prog, state));
  }, [state, prog, push]);

  /** メッセージウィンドウが最後のページを読み終えた。 */
  const onDone = useCallback(() => {
    if (state !== null && prog !== null) push(next(prog, state));
  }, [state, prog, push]);

  // 待ち（`@待機`・`@効果`）は、秒数の後に進める。スキップなら、すぐ。
  useEffect(() => {
    if (state === null || prog === null) return;
    const stop = state.stop;
    if (stop.kind === "wait") {
      const timer = setTimeout(
        () => push(next(prog, state)),
        mode === "skip" ? 0 : stop.seconds * 1000,
      );
      return () => clearTimeout(timer);
    }
    if (
      (stop.kind === "stub" || stop.kind === "ai_skipped") &&
      mode !== "normal"
    ) {
      const timer = setTimeout(
        () => push(next(prog, state)),
        mode === "skip" ? 0 : 1500,
      );
      return () => clearTimeout(timer);
    }
    if (stop.kind === "choice" && policy !== "stop") {
      const index = autoChoice(stop, policy);
      if (index === null) return;
      const timer = setTimeout(
        () => push(choose(prog, state, index)),
        mode === "skip" ? 0 : AUTO_CHOICE_MS,
      );
      return () => clearTimeout(timer);
    }
    // スキップは、選択肢やエンディングで止める。
    if (mode === "skip" && !canAdvance(stop) && stop.kind !== "choice")
      setMode("normal");
    if (mode === "skip" && stop.kind === "choice" && policy === "stop")
      setMode("normal");
  }, [state, prog, mode, policy, push]);

  // Enter・Space で次へ（入力欄の中と、縦書きビューでは動かさない）。
  useEffect(() => {
    if (view !== "adv") return;
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (target?.closest("input, select, textarea, button") !== null) return;
      if (event.isComposing) return;
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        advance();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [advance, view]);

  const message = useMemo(() => {
    if (state === null || running === null || state.stop.kind !== "line") {
      return null;
    }
    const built = piecesOf(state.stop.op.text, state.vars);
    const split = paginate(built.pieces, running.data.message_window);
    return {
      pages: split.pages,
      warnings: split.overflowed
        ? [
            ...built.warnings,
            "この行はメッセージウィンドウからはみ出すので、ページを分けて見せています（W203）",
          ]
        : built.warnings,
    };
  }, [state, running]);

  /** 文中の効果音（`{se}`）。 */
  const playSe = useCallback(
    (name: string) => {
      const sound = running?.data.assets.se[name];
      if (sound?.file !== null && sound?.file !== undefined) {
        playOnce(assetUrl(sound.file), soundRef.current, effects.current);
      } else {
        setSeLabel(`SE ${name} · 仮素材（無音）`);
      }
    },
    [running, assetUrl],
  );

  if (data === null || running === null || state === null || prog === null) {
    return (
      <div className="preview-screen-body">
        <p className="placeholder preview-placeholder">
          {error ?? "プレビューを用意しています…"}
        </p>
      </div>
    );
  }

  const d = running.data;
  const stop = state.stop;
  const characterName = (id: string, displayName: string | null) =>
    displayName === null
      ? (d.characters.find((c) => c.id === id)?.name ?? id)
      : displayName === ""
        ? null
        : displayName;

  // 今の行のページ（止まりが変わったときだけ作る。描き直しのたびに作ると、文字の出方がやり直しになる）。
  const pages = message?.pages ?? null;
  let speakerName: string | null = null;
  let brackets: { open: string; close: string } | null = null;
  let thought = false;
  const warnings = [...state.warnings, ...(message?.warnings ?? [])];
  if (stop.kind === "line") {
    if (stop.op.kind === "say") {
      speakerName = characterName(stop.op.speaker, stop.op.display_name);
      const style = stop.op.style;
      const bracket = d.brackets.find((b) => b.style === style);
      brackets =
        bracket === undefined
          ? null
          : { open: bracket.open, close: bracket.close };
      thought = style === "thought";
    }
  }

  const board = (() => {
    const id = scene?.id ?? "";
    const sceneTitle = scene?.title ?? "";
    switch (stop.kind) {
      case "ending":
        return {
          title: `エンディング：${stop.id ?? "（IDなし）"}`,
          body: `${id} ${sceneTitle}。「最初から」で始め直せます`,
        };
      case "dead_end":
        return {
          title: "行き止まり",
          body: `${id} ${sceneTitle} の終わりに出口がありません（W503）`,
        };
      case "missing_scene":
        return {
          title: "無いシーン",
          body: `${stop.target} はありません（E501）`,
        };
      case "no_caller":
        return {
          title: "呼び出し元がありません",
          body: `${id} の @戻る は、@呼出 から来ていません`,
        };
      case "stub":
        return {
          title: `スタブ：${id} ${sceneTitle}`,
          body:
            scene?.meta.summary ??
            "本文の無いシーンです。次へで、このシーンの命令（ジャンプなど）を実行します",
        };
      case "ai_skipped":
        return {
          title: `AI下書き（省略）：${id} ${sceneTitle}`,
          body:
            scene?.meta.summary ??
            "AI下書きを含めないので、台詞と地の文を飛ばします",
        };
      default:
        return null;
    }
  })();

  const ids = scene === undefined ? [] : lineIdsOf(scene);
  const lineId = location?.lineId ?? null;
  const index = lineId === null ? -1 : ids.indexOf(lineId);
  const position = [
    scene?.id ?? "",
    lineId === null ? null : `^${lineId}`,
    index === -1 ? null : `${index + 1}/${ids.length}行`,
  ]
    .filter((x) => x !== null && x !== "")
    .join(" · ");

  /**
   * 作業領域の見出しの帯と知らせ（見本 06・06b。ADV と縦書きで、表示の切り替え・AI下書き・別ウィンドウ・閉じるを
   * 同じ場所に置き、表示ごとの操作は `controls` に入れる。T4-19）。
   */
  const head = (controls: ReactNode) => (
    <>
      <header className="work-head preview-head">
        <h1 className="h1">プレビュー</h1>
        <fieldset className="mode-switch" aria-label="表示">
          <button
            type="button"
            aria-pressed={view === "adv"}
            onClick={() => setView("adv")}
          >
            ADV
          </button>
          <button
            type="button"
            aria-pressed={view === "vertical"}
            onClick={() => {
              // 縦書きで読んでいる間に、ADV のオート・スキップで道が伸びないように止める。
              setMode("normal");
              setView("vertical");
            }}
          >
            縦書き
          </button>
        </fieldset>
        <label className="check">
          <input
            type="checkbox"
            checked={includeAi}
            onChange={(event) => {
              setIncludeAi(event.target.checked);
              push(setIncludeAiDrafts(state, event.target.checked));
            }}
          />
          AI下書きを含める
        </label>
        {controls}
        <span className="grow" />
        {view === "adv" && (
          <>
            <button
              type="button"
              className="btn"
              onClick={() => {
                setNotice(null);
                restart(data, state.vars);
              }}
            >
              最初から
            </button>
            <button
              type="button"
              className="btn btn-icon"
              aria-pressed={sound.muted}
              aria-label={sound.muted ? "音を出す" : "音を消す"}
              title="BGM と効果音を消す（全体。この端末に覚える）"
              onClick={() => changeSound({ ...sound, muted: !sound.muted })}
            >
              <Icon name={sound.muted ? "muted" : "volume"} size={16} />
            </button>
            <input
              className="preview-volume-range"
              type="range"
              aria-label="音量"
              title={`BGM と効果音の音量（全体。素材ごとの大きさは変えない）：${sound.muted ? "消している" : `${volumePercent(sound)}%`}`}
              min={0}
              max={100}
              step={5}
              value={volumePercent(sound)}
              onChange={(event) =>
                changeSound(withPercent(sound, Number(event.target.value)))
              }
            />
          </>
        )}
        {onOpenWindow !== null && (
          <button
            type="button"
            className="btn"
            title="プレビューを別ウィンドウに出して、本文と並べる"
            onClick={onOpenWindow}
          >
            別ウィンドウで開く
          </button>
        )}
        <button
          type="button"
          className="btn btn-icon"
          aria-label={closeLabel}
          title={closeLabel}
          onClick={onClose}
        >
          <Icon name="close" size={16} />
        </button>
      </header>
      {notice !== null && !stale && (
        <div className="notice preview-band" role="status">
          <span>{notice}</span>
          <button
            type="button"
            className="btn btn-sm"
            onClick={() => setNotice(null)}
          >
            閉じる
          </button>
        </div>
      )}
      {stale && (
        <div className="notice notice-warn preview-band" role="status">
          <span>
            本文が変わりました。プレビューは変わる前の本文のままです。
          </span>
          <button
            type="button"
            className="btn btn-sm"
            onClick={() => {
              const p = program(data.ir, declared(data));
              setRunning({ data, prog: p });
              push(
                jumpTo(
                  p,
                  { ...state, includeAiDrafts: includeAi },
                  state.scene,
                ),
              );
            }}
          >
            このシーンの頭から続ける
          </button>
          <button
            type="button"
            className="btn btn-sm"
            onClick={() => restart(data, state.vars)}
          >
            最初から
          </button>
        </div>
      )}
    </>
  );

  return (
    <div className="preview-screen-body">
      {view === "vertical" ? (
        <VerticalView
          data={data}
          steps={readingSteps}
          persistent={persistent}
          aiDrafts={includeAi}
          head={head}
          onShowInEditor={onShowInEditor}
        />
      ) : (
        <>
          <ScreenStatus
            screen="preview"
            items={previewStatus({
              position: position === "" ? null : position,
              screen: `${d.screen.width}×${d.screen.height} · ウィンドウ ${d.message_window.lines}行×${d.message_window.chars_per_line}字`,
            })}
          />
          <main className="preview-main" aria-label="ADV">
            {head(
              <label className="check">
                <input
                  type="checkbox"
                  checked={sync}
                  onChange={(event) => setSync(event.target.checked)}
                />
                エディタと同期
              </label>,
            )}
            <div className="preview-scroll">
              <div className="preview-stage-wrap">
                <PreviewStage
                  data={d}
                  stage={state.stage}
                  stop={stop}
                  pages={pages}
                  speakerName={speakerName}
                  brackets={brackets}
                  thought={thought}
                  stopSeq={stopSeq}
                  mode={mode}
                  advanceSeq={advanceSeq}
                  seLabel={seLabel}
                  fades={fades}
                  assetUrl={assetUrl}
                  onSe={playSe}
                  onDone={onDone}
                  onChoose={(i) => push(choose(prog, state, i))}
                  board={board}
                  inlineText={plain}
                />
              </div>
              <div className="preview-controls">
                <button
                  type="button"
                  className="btn"
                  disabled={history.length < 2}
                  onClick={() => {
                    setHistory((h) => h.slice(0, -1));
                    setStopSeq((n) => n + 1);
                    setMode("normal");
                  }}
                >
                  <Icon name="back" size={14} stroke={2} />
                  戻る
                </button>
                <button
                  type="button"
                  className="btn btn-primary"
                  disabled={!canAdvance(stop)}
                  onClick={advance}
                >
                  次へ
                  <Icon name="forward" size={14} stroke={2} />
                </button>
                <button
                  type="button"
                  className="btn"
                  aria-pressed={mode === "auto"}
                  onClick={() =>
                    setMode((m) => (m === "auto" ? "normal" : "auto"))
                  }
                >
                  オート
                </button>
                <button
                  type="button"
                  className="btn"
                  aria-pressed={mode === "skip"}
                  onClick={() => {
                    if (mode === "skip") {
                      setMode("normal");
                      return;
                    }
                    // 台詞と地の文を飛ばして、次の選択肢かエンディングまで進める。
                    push(skip(prog, state, null));
                    setMode("skip");
                  }}
                >
                  スキップ
                </button>
                <button
                  type="button"
                  className="btn"
                  onClick={() =>
                    logRef.current
                      ?.querySelector(".preview-log-section")
                      ?.scrollIntoView({ behavior: "smooth" })
                  }
                >
                  ログ
                </button>
                <span className="grow" />
                <span className="preview-position mono t-small t-2">
                  {position}
                </span>
              </div>
              <p className="preview-info t-small t-3">
                画面 {d.screen.width}×{d.screen.height} · ウィンドウ{" "}
                {d.message_window.lines}行×{d.message_window.chars_per_line}字 ·
                トランジション：フェードのみ再現 · 音：ファイルのある素材だけ
              </p>
            </div>
          </main>
          <div ref={logRef} className="preview-debug-wrap ctx-column">
            <DebugPanel
              data={d}
              scene={scene}
              position={position}
              vars={state.vars}
              log={state.log}
              warnings={warnings}
              policy={policy}
              speakerName={characterName}
              inlineText={plain}
              onPolicy={setPolicy}
              onVariable={(key, value) => push(setVariable(state, key, value))}
              onJump={(id) => {
                setMode("normal");
                push(jumpTo(prog, state, id));
              }}
              onRouteJump={(id) => {
                setMode("normal");
                routeAndStart(data, { seq: -1, kind: "scene", scene: id });
              }}
              onShowInEditor={() => {
                if (line !== null) onShowInEditor(line);
              }}
              onShowLog={(entry) => {
                const file = prog.scenes.get(entry.scene)?.file;
                if (file !== undefined) {
                  onShowInEditor({
                    file,
                    lineId: entry.lineId,
                    span: entry.span,
                  });
                }
              }}
            />
          </div>
        </>
      )}
    </div>
  );
}
