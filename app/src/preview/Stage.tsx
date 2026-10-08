// プレビューの舞台（docs/ui/mockups/06-preview.html、T1-11、D050）：背景、立ち絵（左・中・右）、
// メッセージウィンドウ（名前欄、文字の出方）、選択肢、止まったときの知らせ。素材のファイルが無ければ
// 仮素材（名前入りの色付きの矩形と、人影）で見せる。トランジションは、名前によらずフェードだけを再現する
// （「即時」と None は切り替えるだけ）。

import {
  type ReactNode,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { IrInline, PreviewData } from "../core/ir.ts";
import { assetState, placeholderColors } from "./placeholder.ts";
import type { Position, Stage as StageState, Stop } from "./runtime.ts";
import type { Page, Piece } from "./text.ts";

/** 文字を出す速さ（1文字のミリ秒）。 */
const SPEED_MS = { slow: 70, normal: 35, fast: 12 } as const;
/** オートで、文字が出きってから次へ進むまでの時間。 */
const AUTO_DELAY_MS = 1400;

export type PlayMode = "normal" | "auto" | "skip";

/** 文字の出方の単位（1文字、ルビ1つ、待ち、効果音、速さ）。 */
type Unit =
  | { readonly kind: "char"; readonly text: string; readonly emphasis: boolean }
  | { readonly kind: "ruby"; readonly base: string; readonly reading: string }
  | { readonly kind: "newline" }
  | { readonly kind: "wait"; readonly seconds: number }
  | { readonly kind: "se"; readonly name: string }
  | { readonly kind: "speed"; readonly speed: keyof typeof SPEED_MS };

function unitsOf(page: Page): Unit[] {
  const units: Unit[] = [];
  for (const piece of page as readonly Piece[]) {
    if (piece.kind === "text") {
      for (const c of piece.text)
        units.push({ kind: "char", text: c, emphasis: piece.emphasis });
    } else {
      units.push(piece);
    }
  }
  return units;
}

function render(units: readonly Unit[], shown: number) {
  const out: ReactNode[] = [];
  let visibleCount = 0;
  units.forEach((u, i) => {
    if (u.kind === "char" || u.kind === "ruby") {
      if (visibleCount >= shown) return;
      visibleCount += 1;
    }
    const key = `u${i}`;
    if (u.kind === "char") {
      out.push(
        u.emphasis ? (
          <em key={key} className="preview-dot">
            {u.text}
          </em>
        ) : (
          <span key={key}>{u.text}</span>
        ),
      );
    } else if (u.kind === "ruby") {
      out.push(
        <ruby key={key}>
          {u.base}
          <rt>{u.reading}</rt>
        </ruby>,
      );
    } else if (u.kind === "newline" && visibleCount < shown) {
      out.push(<br key={key} />);
    }
  });
  return out;
}

/** 表示する文字の数（1文字、ルビ1つを1と数える）。 */
function visibleUnits(units: readonly Unit[]): number {
  return units.filter((u) => u.kind === "char" || u.kind === "ruby").length;
}

interface MessageProps {
  readonly pages: readonly Page[];
  /** 名前欄（無ければ出さない）。 */
  readonly name: string | null;
  readonly brackets: { readonly open: string; readonly close: string } | null;
  readonly thought: boolean;
  readonly mode: PlayMode;
  readonly fontSize: number;
  readonly lines: number;
  readonly onSe: (name: string) => void;
  /** 最後のページを読み終えて、次へ進む。 */
  readonly onDone: () => void;
  /** 次へ（ボタンと Enter）の合図。増えるたびに1回進める。 */
  readonly advanceSeq: number;
}

/** メッセージウィンドウ。文字を少しずつ出し、`{w}` で待ち、`{p}` でページを改める。 */
function MessageWindow({
  pages,
  name,
  brackets,
  thought,
  mode,
  fontSize,
  lines,
  onSe,
  onDone,
  advanceSeq,
}: MessageProps) {
  const [page, setPage] = useState(0);
  const [cursor, setCursor] = useState(0);
  const units = useMemo(() => unitsOf(pages[page] ?? []), [pages, page]);
  const total = visibleUnits(units);
  const shown = units
    .slice(0, cursor)
    .filter((u) => u.kind === "char" || u.kind === "ruby").length;
  const complete = cursor >= units.length;
  const speed = useRef<keyof typeof SPEED_MS>("normal");

  // ページを進める（全部出ていなければ、まず全部出す）。
  const advance = useRef(() => {});
  advance.current = () => {
    if (!complete) {
      setCursor(units.length);
      return;
    }
    if (page + 1 < pages.length) {
      setPage(page + 1);
      setCursor(0);
      return;
    }
    onDone();
  };

  // 文字を出す。
  useEffect(() => {
    if (complete) return;
    if (mode === "skip") {
      setCursor(units.length);
      return;
    }
    const unit = units[cursor];
    let delay: number = SPEED_MS[speed.current];
    if (unit?.kind === "wait") delay = Math.max(0, unit.seconds * 1000);
    else if (unit?.kind === "se") {
      onSe(unit.name);
      delay = 0;
    } else if (unit?.kind === "speed") {
      speed.current = unit.speed;
      delay = 0;
    } else if (unit?.kind === "newline") delay = 0;
    const timer = setTimeout(() => setCursor((c) => c + 1), delay);
    return () => clearTimeout(timer);
  }, [cursor, complete, mode, units, onSe]);

  // オートとスキップでは、出きったら進める。
  useEffect(() => {
    if (!complete || mode === "normal") return;
    const timer = setTimeout(
      () => advance.current(),
      mode === "skip" ? 40 : AUTO_DELAY_MS,
    );
    return () => clearTimeout(timer);
  }, [complete, mode]);

  // ボタンと Enter の「次へ」。
  const seen = useRef(advanceSeq);
  useEffect(() => {
    if (advanceSeq === seen.current) return;
    seen.current = advanceSeq;
    advance.current();
  }, [advanceSeq]);

  const last = page === pages.length - 1;
  return (
    <button
      type="button"
      className={`preview-window${thought ? " thought" : ""}`}
      style={{
        fontSize,
        minHeight: `calc(${lines} * ${fontSize}px * 1.8 + 42px)`,
      }}
      onClick={() => advance.current()}
      aria-label="メッセージウィンドウ（押すと次へ）"
    >
      {name !== null && <span className="preview-name">{name}</span>}
      <span className="preview-text">
        {brackets !== null && page === 0 && brackets.open}
        {render(units, shown)}
        {brackets !== null && last && complete && brackets.close}
      </span>
      {complete && (
        <span className="preview-next" aria-hidden="true">
          ▼
        </span>
      )}
      <span className="sr-only">{`${shown}/${total}`}</span>
    </button>
  );
}

interface Props {
  readonly data: PreviewData;
  readonly stage: StageState;
  readonly stop: Stop;
  /** 今の行のページ（台詞・地の文のとき）。 */
  readonly pages: readonly Page[] | null;
  readonly speakerName: string | null;
  readonly brackets: { readonly open: string; readonly close: string } | null;
  readonly thought: boolean;
  /** 止まりの通し番号（変わるたびにメッセージウィンドウを作り直す）。 */
  readonly stopSeq: number;
  readonly mode: PlayMode;
  readonly advanceSeq: number;
  /** 今鳴っている効果音の札（仮素材で無音のとき）。 */
  readonly seLabel: string | null;
  readonly fades: ReadonlySet<string>;
  readonly assetUrl: (path: string) => string;
  readonly onSe: (name: string) => void;
  readonly onDone: () => void;
  readonly onChoose: (index: number) => void;
  /** 知らせの板の文（エンディング、行き止まりなど）。 */
  readonly board: { readonly title: string; readonly body: string } | null;
  readonly inlineText: (inlines: readonly IrInline[]) => string;
}

const POSITIONS: readonly Position[] = ["left", "center", "right"];
const POSITION_LABEL = { left: "左", center: "中", right: "右" } as const;

export function PreviewStage({
  data,
  stage,
  stop,
  pages,
  speakerName,
  brackets,
  thought,
  stopSeq,
  mode,
  advanceSeq,
  seLabel,
  fades,
  assetUrl,
  onSe,
  onDone,
  onChoose,
  board,
  inlineText,
}: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(960);
  useLayoutEffect(() => {
    const el = ref.current;
    if (el === null) return;
    const observer = new ResizeObserver(() => setWidth(el.clientWidth));
    observer.observe(el);
    setWidth(el.clientWidth);
    return () => observer.disconnect();
  }, []);
  const { width: sw, height: sh } = data.screen;
  const win = data.message_window;
  // ウィンドウの内側の幅（舞台の94%から余白を除く）に、1行の字数と台詞の開き括弧が収まる文字の大きさ。
  const fontSize = Math.max(
    10,
    Math.floor((width * 0.94 - 64) / (win.chars_per_line + 1)),
  );

  const bg = stage.background;
  const bgState = bg === null ? null : assetState(data.assets.backgrounds, bg);
  const [c1, c2] = placeholderColors(bg ?? "");
  return (
    <div
      ref={ref}
      className="preview-stage"
      style={{ aspectRatio: `${sw} / ${sh}` }}
    >
      <div
        className={`preview-bg${fades.has("background") ? " fade" : ""}`}
        style={
          bgState?.kind === "file"
            ? { backgroundImage: `url("${assetUrl(bgState.path)}")` }
            : bg === null
              ? undefined
              : { background: `linear-gradient(180deg, ${c1}, ${c2})` }
        }
      />
      {bg !== null && bgState?.kind !== "file" && (
        <span className="preview-tag top-left">
          背景：{bg}
          {bgState?.kind === "unregistered"
            ? "（カタログに無い）"
            : "（仮素材）"}
        </span>
      )}
      {POSITIONS.map((p) => {
        const sprite = stage.sprites[p];
        if (sprite === undefined) return null;
        const state = assetState(
          data.assets.expressions[sprite.character],
          sprite.expression,
        );
        const name =
          data.characters.find((c) => c.id === sprite.character)?.name ??
          sprite.character;
        return (
          <div
            key={p}
            className={`preview-sprite ${p}${fades.has(p) ? " fade" : ""}`}
          >
            {state.kind === "file" ? (
              <img
                src={assetUrl(state.path)}
                alt={`${name} ${sprite.expression}`}
              />
            ) : (
              <div className="preview-silhouette" aria-hidden="true">
                <span className="head" />
                <span className="body" />
              </div>
            )}
            {state.kind !== "file" && (
              <span className="preview-tag sprite">
                立ち絵：{name} · {sprite.expression} · {POSITION_LABEL[p]}
              </span>
            )}
          </div>
        );
      })}
      {seLabel !== null && <span className="preview-tag se">{seLabel}</span>}
      {stage.bgm !== null && (
        <span className="preview-tag bgm">BGM {stage.bgm}</span>
      )}
      {stop.kind === "choice" && (
        <fieldset className="preview-choices" aria-label="選択肢">
          {stop.prompt !== null && (
            <p className="preview-prompt">{inlineText(stop.prompt)}</p>
          )}
          {stop.items.map((item) => (
            <button
              key={item.index}
              type="button"
              className="preview-choice"
              disabled={!item.enabled}
              onClick={() => onChoose(item.index)}
            >
              {inlineText(item.text)}
              {item.condText !== null && (
                <span className="preview-cond">
                  {item.enabled ? "" : "選べない："}
                  {item.condText}
                </span>
              )}
            </button>
          ))}
        </fieldset>
      )}
      {board !== null && (
        <div className="preview-board" role="status">
          <strong>{board.title}</strong>
          <span>{board.body}</span>
        </div>
      )}
      {pages !== null && (
        <MessageWindow
          key={stopSeq}
          pages={pages}
          name={speakerName}
          brackets={brackets}
          thought={thought}
          mode={mode}
          fontSize={fontSize}
          lines={win.lines}
          onSe={onSe}
          onDone={onDone}
          advanceSeq={advanceSeq}
        />
      )}
    </div>
  );
}
