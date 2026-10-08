// プレビューの画面（docs/ui/mockups/06-preview.html、T1-11、D050）：舞台、操作の行、情報の行、デバッグの欄。
// ランタイム（runtime.ts）で IR を解釈し、止まるたびに画面を描き直す。
//
// - 「戻る」のために、止まるたびの状態を残す。
// - 本文が変わってプレビューのデータを取り直したら、新しい本文の同じ行IDの行から続ける（T2-4）。行IDが無い
//   （消した、行IDの無い止まり）ときは、知らせて、このシーンの頭から続けるか最初からかを選ばせる。
// - 再生の頼み（エディタの「ここから再生」、デバッグの「最短経路で開始」）は、コアの最短経路の選択肢を選んで
//   ランタイムを速送りし、その行から始める（変数と舞台がそろう。D055）。
// - メインウィンドウの画面にも、別ウィンドウ（`PreviewWindow.tsx`）にも出す。縦書きは T2-5。

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { routeAt, routeToScene } from "../core/api.ts";
import type { IrInline, IrSpan, PreviewData } from "../core/ir.ts";
import type { PreviewRequest, Route } from "../core/types.ts";
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
  stopLocation,
} from "./runtime.ts";
import { type PlayMode, PreviewStage } from "./Stage.tsx";
import { paginate, piecesOf } from "./text.ts";

/** プレビュー中の行（エディタに示す）。 */
export interface PreviewLine {
  readonly file: string;
  readonly lineId: string | null;
  readonly span: IrSpan;
}

interface Props {
  readonly data: PreviewData | null;
  readonly error: string | null;
  readonly title: string;
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

export function PreviewScreen({
  data,
  error,
  title,
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
  /** 従った再生の頼み。 */
  const applied = useRef<number | null>(null);
  /** 再生の頼みの最短経路を待っている。 */
  const waiting = useRef(false);
  const bgm = useRef<HTMLAudioElement | null>(null);
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
    if (request.kind === "start") {
      setNotice(null);
      restart(data, history[history.length - 1]?.vars ?? null);
      return;
    }
    routeAndStart(data, request);
  }, [data, request, restart, routeAndStart, history]);

  // 出来事（効果音、トランジション）。
  useEffect(() => {
    if (state === null || running === null) return;
    const fading = new Set<string>();
    for (const e of state.events) {
      if (e.kind === "se") {
        const sound = running.data.assets.se[e.name];
        if (sound?.file !== null && sound?.file !== undefined) {
          void new Audio(assetUrl(sound.file)).play().catch(() => {});
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

  // BGM。ファイルがあれば鳴らし、無ければ無音。
  const bgmName = state?.stage.bgm ?? null;
  useEffect(() => {
    bgm.current?.pause();
    bgm.current = null;
    if (bgmName === null || running === null) return;
    const sound = running.data.assets.bgm[bgmName];
    if (sound?.file === null || sound?.file === undefined) return;
    const audio = new Audio(assetUrl(sound.file));
    audio.loop = sound.loop;
    void audio.play().catch(() => {});
    bgm.current = audio;
    return () => audio.pause();
  }, [bgmName, running, assetUrl]);

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

  // Enter・Space で次へ（入力欄の中では動かさない）。
  useEffect(() => {
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
  }, [advance]);

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
        void new Audio(assetUrl(sound.file)).play().catch(() => {});
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

  return (
    <div className="preview-screen-body">
      <div className="preview-main">
        <header className="preview-header">
          <strong>プレビュー</strong>
          <span className="preview-header-title">{title}</span>
          <span className="flow-spacer" />
          <label className="preview-check ai">
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
          <label className="preview-check sync">
            <input
              type="checkbox"
              checked={sync}
              onChange={(event) => setSync(event.target.checked)}
            />
            エディタと同期
          </label>
          <button
            type="button"
            className="preview-button"
            onClick={() => {
              setNotice(null);
              restart(data, state.vars);
            }}
          >
            最初から
          </button>
          {onOpenWindow !== null && (
            <button
              type="button"
              className="preview-button"
              title="プレビューを別ウィンドウに出して、本文と並べる"
              onClick={onOpenWindow}
            >
              別ウィンドウで開く
            </button>
          )}
          <button type="button" className="preview-button" onClick={onClose}>
            {closeLabel}
          </button>
        </header>
        {notice !== null && !stale && (
          <div className="preview-notice" role="status">
            <span>{notice}</span>
            <button
              type="button"
              className="preview-button"
              onClick={() => setNotice(null)}
            >
              閉じる
            </button>
          </div>
        )}
        {stale && (
          <div className="preview-stale" role="status">
            <span>
              本文が変わりました。プレビューは変わる前の本文のままです。
            </span>
            <button
              type="button"
              className="preview-button"
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
              className="preview-button"
              onClick={() => restart(data, state.vars)}
            >
              最初から
            </button>
          </div>
        )}
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
            className="preview-button"
            disabled={history.length < 2}
            onClick={() => {
              setHistory((h) => h.slice(0, -1));
              setStopSeq((n) => n + 1);
              setMode("normal");
            }}
          >
            戻る
          </button>
          <button
            type="button"
            className="preview-button primary"
            disabled={!canAdvance(stop)}
            onClick={advance}
          >
            次へ
          </button>
          <button
            type="button"
            className="preview-button"
            aria-pressed={mode === "auto"}
            onClick={() => setMode((m) => (m === "auto" ? "normal" : "auto"))}
          >
            オート
          </button>
          <button
            type="button"
            className="preview-button"
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
            className="preview-button"
            onClick={() =>
              logRef.current
                ?.querySelector(".preview-log-section")
                ?.scrollIntoView({ behavior: "smooth" })
            }
          >
            ログ
          </button>
          <span className="flow-spacer" />
          <span className="preview-position mono">{position}</span>
        </div>
        <p className="preview-info">
          画面 {d.screen.width}×{d.screen.height} · ウィンドウ{" "}
          {d.message_window.lines}行×{d.message_window.chars_per_line}字 ·
          トランジション：フェードのみ再現 · 音：ファイルのある素材だけ
        </p>
      </div>
      <div ref={logRef} className="preview-debug-wrap">
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
              onShowInEditor({ file, lineId: entry.lineId, span: entry.span });
            }
          }}
        />
      </div>
    </div>
  );
}
