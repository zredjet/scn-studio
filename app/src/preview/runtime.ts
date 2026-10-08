// 簡易ADVプレビューのランタイム（T1-11、D050）：IRを解釈し、止まるところ（台詞・地の文、選択肢、
// エンディングなど）まで進める。React と DOM に依存しない（テストでサンプルを通して遊ぶため）。
//
// - 状態は書き換えずに新しく作る（「戻る」のために前の状態を残せる）。
// - 演出（背景、立ち絵、BGM、SE、トランジション、`@効果`、`@待機`）は、舞台の状態と出来事にして画面に渡す。
// - 意味は仕様書の10〜14節。仕様に無い実行時の扱い（行き止まり、無いシーン、呼び出し元の無い `@戻る`、
//   誤りのある条件、スタブ、AI下書きを含めないとき）は D050 で決めた。

import type { Ir, IrInline, IrScene, IrSpan, Op } from "../core/ir.ts";
import type { Route, RouteStep } from "../core/types.ts";
import { assign, condition, type Value, type Variables } from "./expr.ts";

/** 立ち絵の位置。 */
export type Position = "left" | "center" | "right";

/** 舞台の状態。 */
export interface Stage {
  readonly background: string | null;
  readonly sprites: Readonly<
    Partial<
      Record<
        Position,
        { readonly character: string; readonly expression: string }
      >
    >
  >;
  readonly bgm: string | null;
}

/** 画面に渡す出来事（音を鳴らす、トランジション）。止まるたびに新しくする。 */
export type StageEvent =
  | { readonly kind: "se"; readonly name: string }
  | {
      readonly kind: "transition";
      readonly target: "background" | Position;
      readonly name: string;
    }
  | {
      readonly kind: "fx";
      readonly name: string;
      readonly seconds: number | null;
    };

/** 選択肢の1つ。 */
export interface ChoiceItem {
  readonly index: number;
  readonly text: readonly IrInline[];
  /** 条件が偽か、条件を評価できなかった（選べない）。 */
  readonly enabled: boolean;
  readonly condText: string | null;
}

/** 止まったところ。 */
export type Stop =
  | {
      readonly kind: "line";
      readonly op: Extract<Op, { kind: "say" } | { kind: "narrate" }>;
    }
  | {
      readonly kind: "choice";
      readonly span: IrSpan;
      readonly lineId: string | null;
      readonly prompt: readonly IrInline[] | null;
      readonly items: readonly ChoiceItem[];
    }
  | {
      readonly kind: "wait";
      readonly span: IrSpan;
      readonly seconds: number;
      readonly label: string | null;
    }
  | {
      readonly kind: "ending";
      readonly span: IrSpan;
      readonly id: string | null;
    }
  /** スタブのシーンに入った（1回知らせてから、そのシーンの命令を続ける）。 */
  | { readonly kind: "stub"; readonly span: IrSpan }
  /** AI下書きのシーンの台詞と地の文を飛ばした（含めないとき。1回知らせる）。 */
  | { readonly kind: "ai_skipped"; readonly span: IrSpan }
  /** 出口の無いままシーンが終わった（W503）。 */
  | { readonly kind: "dead_end"; readonly span: IrSpan }
  /** 無いシーンへのジャンプ・呼び出し（E501）。 */
  | {
      readonly kind: "missing_scene";
      readonly span: IrSpan;
      readonly target: string;
    }
  /** 呼び出し元の無い `@戻る`。 */
  | { readonly kind: "no_caller"; readonly span: IrSpan };

/** バックログの1行。 */
export interface LogEntry {
  readonly scene: string;
  readonly span: IrSpan;
  readonly lineId: string | null;
  /** 話者のID（地の文と選んだ選択肢は null）。 */
  readonly speaker: string | null;
  readonly displayName: string | null;
  readonly style: string | null;
  readonly text: readonly IrInline[];
  /** 選んだ選択肢か。 */
  readonly chosen: boolean;
}

/** 実行中のブロック（シーンの命令列、選択肢の中身、条件の枝）。 */
interface Frame {
  readonly ops: readonly Op[];
  readonly index: number;
  /** 中身を実行し終えたら飛ぶ先（選択肢の行の `-> ID`）。 */
  readonly after: string | null;
}

export interface State {
  readonly scene: string;
  readonly frames: readonly Frame[];
  /** `@呼出` で積んだ、呼び出し元のブロック。 */
  readonly calls: readonly {
    readonly scene: string;
    readonly frames: readonly Frame[];
  }[];
  readonly vars: Variables;
  readonly stage: Stage;
  readonly stop: Stop;
  readonly events: readonly StageEvent[];
  readonly log: readonly LogEntry[];
  /** 知らせ（誤りのある条件、書けなかった `@設定`、範囲の外の値など）。新しいものが後ろ。 */
  readonly warnings: readonly string[];
  /** AI下書きのシーンを含めるか。 */
  readonly includeAiDrafts: boolean;
  /** このシーンに入ってから、スタブ・AI下書きを知らせたか。 */
  readonly noticed: boolean;
}

/** 変数の宣言（初期値と範囲）。 */
export interface DeclaredVariable {
  readonly key: string;
  readonly type: string;
  readonly values: readonly string[];
  readonly initial: Value;
  readonly scope: string | null;
  readonly range: readonly [number, number] | null;
}

/** ランタイムが読むもの。 */
export interface Program {
  readonly scenes: ReadonlyMap<string, IrScene>;
  /** 開始のシーン（`start_scene`。無いか無いシーンを指せば最初のシーン）。 */
  readonly start: string | null;
  readonly variables: readonly DeclaredVariable[];
}

/** 何回まで命令を続けて実行するか（ジャンプの循環で止まらないとき）。 */
const MAX_STEPS = 100_000;

export function program(
  ir: Ir,
  variables: readonly DeclaredVariable[],
): Program {
  // シーンIDが重なっていれば（E108）、先に現れたほうへ飛ぶ（D036）。
  const scenes = new Map<string, IrScene>();
  for (const s of ir.scenes) if (!scenes.has(s.id)) scenes.set(s.id, s);
  const start =
    ir.start_scene !== null && scenes.has(ir.start_scene)
      ? ir.start_scene
      : (ir.scenes[0]?.id ?? null);
  return { scenes, start, variables };
}

/** 変数の初期値。`persistent` の変数は `keep` の値を残す（「最初から」でも周回の値は残す）。 */
export function initialVariables(
  prog: Program,
  keep: Variables | null = null,
): Map<string, Value> {
  const vars = new Map<string, Value>();
  for (const v of prog.variables) {
    const kept = keep?.get(v.key);
    vars.set(
      v.key,
      v.scope === "persistent" && kept !== undefined ? kept : v.initial,
    );
  }
  return vars;
}

const EMPTY_STAGE: Stage = { background: null, sprites: {}, bgm: null };

function sceneSpan(prog: Program, id: string): IrSpan {
  return prog.scenes.get(id)?.span ?? [0, 0];
}

/** 始める前の状態（シーンに入る前）。 */
function fresh(
  options: {
    readonly includeAiDrafts: boolean;
    readonly vars?: Variables;
  },
  prog: Program,
  first: string | null,
): State {
  return {
    scene: first ?? "",
    frames: [],
    calls: [],
    vars: options.vars ?? initialVariables(prog),
    stage: EMPTY_STAGE,
    stop: { kind: "dead_end", span: [0, 0] },
    events: [],
    log: [],
    warnings: [],
    includeAiDrafts: options.includeAiDrafts,
    noticed: false,
  };
}

/** 最初から始める（`scene` を渡すと、そのシーンから。変数は `vars` か初期値）。 */
export function start(
  prog: Program,
  options: {
    readonly includeAiDrafts: boolean;
    readonly scene?: string;
    readonly vars?: Variables;
  },
): State {
  const first = options.scene ?? prog.start;
  const base = fresh(options, prog, first);
  if (first === null) return base;
  return run(prog, enter(prog, base, first, [0, 0]));
}

/** 今のシーン。 */
export function currentScene(prog: Program, state: State): IrScene | undefined {
  return prog.scenes.get(state.scene);
}

/** シーンに入る（無いシーンなら止まる）。 */
function enter(
  prog: Program,
  state: State,
  target: string,
  span: IrSpan,
): State {
  const scene = prog.scenes.get(target);
  if (scene === undefined) {
    return {
      ...state,
      frames: [],
      stop: { kind: "missing_scene", span, target },
    };
  }
  return {
    ...state,
    scene: target,
    frames: [{ ops: scene.ops, index: 0, after: null }],
    noticed: false,
  };
}

/** 止まりから先へ進める（台詞・地の文、知らせ、待ち）。選択肢とエンディングでは進まない。 */
export function next(prog: Program, state: State): State {
  switch (state.stop.kind) {
    case "line":
    case "wait":
    case "stub":
    case "ai_skipped":
      return run(prog, { ...state, events: [] });
    default:
      return state;
  }
}

/** 選択肢を選ぶ。選べないものは選ばない。 */
export function choose(prog: Program, state: State, index: number): State {
  return chooseTo(prog, state, index, null).state;
}

/** 選択肢を選び、`until` の命令の手前か、止まるところまで実行する。 */
function chooseTo(
  prog: Program,
  state: State,
  index: number,
  until: RouteTarget | null,
): Ran {
  const stop = state.stop;
  if (stop.kind !== "choice") return { state, hit: false };
  const item = stop.items.find((i) => i.index === index);
  const choice = findChoice(state);
  const option = choice?.options[index];
  if (item === undefined || !item.enabled || option === undefined) {
    return { state, hit: false };
  }
  const entry: LogEntry = {
    scene: state.scene,
    span: option.span,
    lineId: option.line_id,
    speaker: null,
    displayName: null,
    style: null,
    text: option.text,
    chosen: true,
  };
  return runTo(
    prog,
    {
      ...state,
      events: [],
      log: [...state.log, entry],
      frames: [
        ...state.frames,
        { ops: option.body, index: 0, after: option.jump },
      ],
    },
    until,
  );
}

/** 今止まっている選択肢の命令（実行中のブロックの、1つ前の命令）。 */
function findChoice(state: State): Extract<Op, { kind: "choice" }> | null {
  const top = state.frames[state.frames.length - 1];
  const op = top?.ops[top.index - 1];
  return op?.kind === "choice" ? op : null;
}

/** 好きなシーンへ飛ぶ（デバッグの「この状態で開始」。呼び出しの積み重ねは空にする）。 */
export function jumpTo(prog: Program, state: State, scene: string): State {
  return run(
    prog,
    enter(
      prog,
      { ...state, calls: [], events: [], stage: state.stage },
      scene,
      sceneSpan(prog, scene),
    ),
  );
}

/** 変数の値を変える（デバッグの欄）。選択肢で止まっていれば、選べるかどうかを決め直す。 */
export function setVariable(state: State, key: string, value: Value): State {
  const vars = new Map(state.vars);
  vars.set(key, value);
  const next: State = { ...state, vars };
  const stop = state.stop;
  const choice = findChoice(state);
  if (stop.kind !== "choice" || choice === null) return next;
  const items = stop.items.map((item) => {
    const option = choice.options[item.index];
    if (option === undefined || option.cond_text === null) return item;
    return {
      ...item,
      enabled: condition(option.cond, option.cond_text, vars).value,
    };
  });
  return { ...next, stop: { ...stop, items } };
}

/** 宣言の型に合う値か（`=` で書く値。列挙は `values` のどれか）。宣言が無ければ何でもよい。 */
function fitsDeclared(
  decl: DeclaredVariable | undefined,
  value: Value,
): boolean {
  if (decl === undefined) return true;
  switch (decl.type) {
    case "int":
      return typeof value === "number";
    case "bool":
      return typeof value === "boolean";
    case "enum":
      return typeof value === "string" && decl.values.includes(value);
    default:
      return typeof value === "string";
  }
}

/** AI下書きを含めるかを変える。 */
export function setIncludeAiDrafts(state: State, include: boolean): State {
  return { ...state, includeAiDrafts: include };
}

function warn(state: State, message: string): State {
  return { ...state, warnings: [...state.warnings, message] };
}

function isAiDraft(prog: Program, state: State): boolean {
  return currentScene(prog, state)?.meta.status === "ai-draft";
}

/** 再生を始める命令（コアの `route::RouteTarget` のうち、ランタイムが使うところ）。 */
export type RouteTarget = Pick<Route["target"], "scene" | "span">;

/** 実行の結果。`hit` なら `until` の命令の手前で止めた（その命令はまだ実行していない）。 */
interface Ran {
  readonly state: State;
  readonly hit: boolean;
}

/** 止まるところまで命令を実行する。 */
function run(prog: Program, initial: State): State {
  return runTo(prog, initial, null).state;
}

/** 止まるところか、`until` の命令の手前まで実行する。 */
function runTo(prog: Program, initial: State, until: RouteTarget | null): Ran {
  let state = initial;
  const done = (s: State): Ran => ({ state: s, hit: false });
  for (let steps = 0; steps < MAX_STEPS; steps++) {
    if (state.stop.kind === "missing_scene" && state.frames.length === 0) {
      return done(state);
    }
    // 目的の命令の手前（シーンに入ったときの知らせより先に調べる。知らせは再生を始めてから出す）。
    if (until !== null && state.scene === until.scene) {
      const at = state.frames[state.frames.length - 1];
      const op = at?.ops[at.index];
      if (
        op !== undefined &&
        op.span[0] === until.span[0] &&
        op.span[1] === until.span[1]
      ) {
        return { state, hit: true };
      }
    }
    // シーンに入ったときの知らせ（スタブ、含めないAI下書き）。
    if (
      !state.noticed &&
      state.frames.length === 1 &&
      state.frames[0]?.index === 0
    ) {
      const scene = currentScene(prog, state);
      state = { ...state, noticed: true };
      if (scene?.is_stub === true) {
        return done({ ...state, stop: { kind: "stub", span: scene.span } });
      }
      if (
        scene !== undefined &&
        isAiDraft(prog, state) &&
        !state.includeAiDrafts
      ) {
        return done({
          ...state,
          stop: { kind: "ai_skipped", span: scene.span },
        });
      }
    }
    const top = state.frames[state.frames.length - 1];
    if (top === undefined) {
      return done({
        ...state,
        stop: { kind: "dead_end", span: sceneSpan(prog, state.scene) },
      });
    }
    const op = top.ops[top.index];
    if (op === undefined) {
      // ブロックの終わり。シーンの終わりなら行き止まり（呼ばれた先でも。13節）。
      const rest = state.frames.slice(0, -1);
      if (top.after !== null) {
        state = enter(
          prog,
          { ...state, frames: rest },
          top.after,
          sceneSpan(prog, state.scene),
        );
        continue;
      }
      if (rest.length === 0) {
        return done({
          ...state,
          frames: rest,
          stop: { kind: "dead_end", span: sceneSpan(prog, state.scene) },
        });
      }
      state = { ...state, frames: rest };
      continue;
    }
    const advanced: State = {
      ...state,
      frames: [...state.frames.slice(0, -1), { ...top, index: top.index + 1 }],
    };
    const result = exec(prog, advanced, op);
    if (result.stopped) return done(result.state);
    state = result.state;
  }
  return done(
    warn(
      {
        ...state,
        stop: { kind: "dead_end", span: sceneSpan(prog, state.scene) },
      },
      "命令を続けて実行しすぎました（ジャンプが循環しています）。止めます",
    ),
  );
}

/** 1つの命令を実行する。止まるなら `stopped`。 */
function exec(
  prog: Program,
  state: State,
  op: Op,
): { state: State; stopped: boolean } {
  const go = (s: State) => ({ state: s, stopped: false });
  const halt = (s: State) => ({ state: s, stopped: true });
  switch (op.kind) {
    case "say":
    case "narrate": {
      if (isAiDraft(prog, state) && !state.includeAiDrafts) return go(state);
      const entry: LogEntry = {
        scene: state.scene,
        span: op.span,
        lineId: op.line_id,
        speaker: op.kind === "say" ? op.speaker : null,
        displayName: op.kind === "say" ? op.display_name : null,
        style: op.kind === "say" ? op.style : null,
        text: op.text,
        chosen: false,
      };
      return halt({
        ...state,
        log: [...state.log, entry],
        stop: { kind: "line", op },
      });
    }
    case "break":
    case "raw":
    case "unknown_directive":
      return go(state);
    case "bg":
      return go({
        ...state,
        stage: { ...state.stage, background: op.name },
        events: [
          ...state.events,
          {
            kind: "transition",
            target: "background",
            name: op.transition ?? "",
          },
        ],
      });
    case "show": {
      const sprites = { ...state.stage.sprites };
      // 同じキャラはどこか1か所に出す。位置を省けば、出ていればその位置、出ていなければ中央。
      const was = (Object.keys(sprites) as Position[]).find(
        (p) => sprites[p]?.character === op.character,
      );
      const position: Position = op.position ?? was ?? "center";
      if (was !== undefined && was !== position) delete sprites[was];
      sprites[position] = {
        character: op.character,
        expression: op.expression,
      };
      return go({
        ...state,
        stage: { ...state.stage, sprites },
        events: [
          ...state.events,
          { kind: "transition", target: position, name: op.transition ?? "" },
        ],
      });
    }
    case "hide": {
      const sprites = { ...state.stage.sprites };
      const was = (Object.keys(sprites) as Position[]).find(
        (p) => sprites[p]?.character === op.character,
      );
      if (was === undefined) return go(state);
      delete sprites[was];
      return go({
        ...state,
        stage: { ...state.stage, sprites },
        events: [
          ...state.events,
          { kind: "transition", target: was, name: op.transition ?? "" },
        ],
      });
    }
    case "bgm":
      return go({ ...state, stage: { ...state.stage, bgm: op.name } });
    case "bgm_stop":
      return go({ ...state, stage: { ...state.stage, bgm: null } });
    case "se":
      return go({
        ...state,
        events: [...state.events, { kind: "se", name: op.name }],
      });
    case "fx": {
      const next = {
        ...state,
        events: [
          ...state.events,
          { kind: "fx" as const, name: op.name, seconds: op.seconds },
        ],
      };
      if (op.seconds === null || op.seconds <= 0) return go(next);
      return halt({
        ...next,
        stop: {
          kind: "wait",
          span: op.span,
          seconds: op.seconds,
          label: op.name,
        },
      });
    }
    case "wait":
      if (op.seconds <= 0) return go(state);
      return halt({
        ...state,
        stop: { kind: "wait", span: op.span, seconds: op.seconds, label: null },
      });
    case "set": {
      const v = assign(op.var, op.op, op.value, state.vars);
      if (!v.ok)
        return go(warn(state, `@設定 ${op.var} を書けません：${v.error}`));
      const decl = prog.variables.find((d) => d.key === op.var);
      if (!fitsDeclared(decl, v.value)) {
        return go(
          warn(
            state,
            `@設定 ${op.var} を書けません：宣言の型（${decl?.type ?? ""}）に合わない値です（${String(v.value)}）`,
          ),
        );
      }
      const vars = new Map(state.vars);
      vars.set(op.var, v.value);
      let next: State = { ...state, vars };
      if (
        decl?.range !== null &&
        decl?.range !== undefined &&
        typeof v.value === "number"
      ) {
        const [min, max] = decl.range;
        if (v.value < min || v.value > max) {
          next = warn(
            next,
            `${op.var} が範囲（${min}〜${max}）の外になりました：${v.value}`,
          );
        }
      }
      return go(next);
    }
    case "choice": {
      let next = state;
      const items: ChoiceItem[] = op.options.map((option, index) => {
        if (option.cond_text === null) {
          return { index, text: option.text, enabled: true, condText: null };
        }
        const c = condition(option.cond, option.cond_text, state.vars);
        if (c.error !== null) next = warn(next, c.error);
        return {
          index,
          text: option.text,
          enabled: c.value,
          condText: option.cond_text,
        };
      });
      return halt({
        ...next,
        stop: {
          kind: "choice",
          span: op.span,
          lineId: op.line_id,
          prompt: op.prompt,
          items,
        },
      });
    }
    case "if": {
      let next = state;
      for (const branch of op.branches) {
        const c = condition(branch.cond, branch.cond_text, next.vars);
        if (c.error !== null) next = warn(next, c.error);
        if (c.value) {
          return go({
            ...next,
            frames: [
              ...next.frames,
              { ops: branch.body, index: 0, after: null },
            ],
          });
        }
      }
      if (op.else !== null) {
        return go({
          ...next,
          frames: [...next.frames, { ops: op.else, index: 0, after: null }],
        });
      }
      return go(next);
    }
    case "jump": {
      const entered = enter(prog, state, op.target, op.span);
      return entered.frames.length === 0 ? halt(entered) : go(entered);
    }
    case "call": {
      const caller = { scene: state.scene, frames: state.frames };
      const entered = enter(
        prog,
        { ...state, calls: [...state.calls, caller] },
        op.target,
        op.span,
      );
      return entered.frames.length === 0 ? halt(entered) : go(entered);
    }
    case "return": {
      const caller = state.calls[state.calls.length - 1];
      if (caller === undefined)
        return halt({ ...state, stop: { kind: "no_caller", span: op.span } });
      return go({
        ...state,
        scene: caller.scene,
        frames: caller.frames,
        calls: state.calls.slice(0, -1),
        noticed: true,
      });
    }
    case "ending":
      return halt({
        ...state,
        stop: { kind: "ending", span: op.span, id: op.id },
      });
  }
}

/** シーンの行ID（台詞・地の文・選択肢。本文の順）。プレビューの「3/9行」に使う。 */
export function lineIdsOf(scene: IrScene): string[] {
  const ids: string[] = [];
  const walk = (ops: readonly Op[]) => {
    for (const op of ops) {
      if (
        (op.kind === "say" || op.kind === "narrate" || op.kind === "choice") &&
        op.line_id !== null
      ) {
        ids.push(op.line_id);
      }
      if (op.kind === "choice") {
        for (const o of op.options) {
          if (o.line_id !== null) ids.push(o.line_id);
          walk(o.body);
        }
      }
      if (op.kind === "if") {
        for (const b of op.branches) walk(b.body);
        if (op.else !== null) walk(op.else);
      }
    }
  };
  walk(scene.ops);
  return ids;
}

/** 今止まっている行の位置（行IDと span）。 */
export function stopLocation(state: State): {
  lineId: string | null;
  span: IrSpan;
} {
  const stop = state.stop;
  if (stop.kind === "line")
    return { lineId: stop.op.line_id, span: stop.op.span };
  if (stop.kind === "choice") return { lineId: stop.lineId, span: stop.span };
  return { lineId: null, span: stop.span };
}

/** 自動選択：選べるもののうち、1番目か、乱数で選ぶ。選べるものが無ければ null。 */
export function autoChoice(
  stop: Stop,
  policy: "first" | "random",
  random: () => number = Math.random,
): number | null {
  if (stop.kind !== "choice") return null;
  const enabled = stop.items.filter((i) => i.enabled);
  if (enabled.length === 0) return null;
  if (policy === "first") return enabled[0]?.index ?? null;
  return (
    enabled[Math.min(enabled.length - 1, Math.floor(random() * enabled.length))]
      ?.index ?? null
  );
}

/** 止まりが、続けて進められるもの（台詞・地の文、待ち、知らせ）か。 */
export function canAdvance(stop: Stop): boolean {
  return (
    stop.kind === "line" ||
    stop.kind === "wait" ||
    stop.kind === "stub" ||
    stop.kind === "ai_skipped"
  );
}

/**
 * スキップ：次の選択肢か、エンディング・行き止まりなどまで進める。`policy` があれば選択肢も自動で選んで
 * 進め（選べるものが無ければ止まる）、`limit` 回選んだら止まる。
 */
export function skip(
  prog: Program,
  state: State,
  policy: "first" | "random" | null = null,
  limit = 1000,
  random: () => number = Math.random,
): State {
  let s = state;
  let chosen = 0;
  for (let i = 0; i < MAX_STEPS; i++) {
    if (canAdvance(s.stop)) {
      s = next(prog, s);
      continue;
    }
    if (s.stop.kind === "choice" && policy !== null && chosen < limit) {
      const index = autoChoice(s.stop, policy, random);
      if (index === null) return s;
      s = choose(prog, s, index);
      chosen += 1;
      continue;
    }
    return s;
  }
  return s;
}

/** 道の再生の結果。 */
export type Replayed =
  | {
      readonly ok: true;
      /** 目的の命令から、最初の止まりまで進めた状態（ここから再生する）。 */
      readonly state: State;
      /** 目的の命令の手前の状態（変数はコアの `route` と同じになる）。 */
      readonly at: State;
    }
  | { readonly ok: false; readonly error: string };

/**
 * カーソル位置から再生（T2-4、D055）：開始シーンから始め、道（コアの `route` の最短経路）の選択肢を順に選んで
 * 速送りし、目的の命令の手前で止めてから、最初の止まりまで進める。台詞・地の文・待ち・知らせは飛ばし、
 * バックログには残す。道が合わなければ（本文が変わった、評価が違う）誤り。
 */
export function replay(
  prog: Program,
  options: { readonly includeAiDrafts: boolean; readonly vars?: Variables },
  steps: readonly RouteStep[],
  target: RouteTarget,
): Replayed {
  const first = prog.start;
  if (first === null) return { ok: false, error: "シーンがありません" };
  let ran = runTo(
    prog,
    enter(prog, fresh(options, prog, first), first, [0, 0]),
    target,
  );
  let next = 0;
  for (let n = 0; n < MAX_STEPS; n++) {
    if (ran.hit) {
      return {
        ok: true,
        at: ran.state,
        state: run(prog, { ...ran.state, events: [] }),
      };
    }
    const state = ran.state;
    const stop = state.stop;
    switch (stop.kind) {
      case "line":
      case "wait":
      case "stub":
      case "ai_skipped":
        ran = runTo(prog, { ...state, events: [] }, target);
        break;
      case "choice": {
        const step = steps[next];
        const item = stop.items.find((i) => i.index === step?.option);
        if (
          step === undefined ||
          step.scene !== state.scene ||
          step.choice[0] !== stop.span[0] ||
          item === undefined ||
          !item.enabled
        ) {
          return {
            ok: false,
            error: `道が本文と合いません（${state.scene} の選択肢）。本文が変わったかもしれません`,
          };
        }
        next += 1;
        ran = chooseTo(prog, state, step.option, target);
        break;
      }
      default:
        return {
          ok: false,
          error: `目的の行に着く前に止まりました（${state.scene}）`,
        };
    }
  }
  return { ok: false, error: "命令を続けて実行しすぎました" };
}

/** 行IDの命令（台詞・地の文・`@選択`。選択肢の行なら、その `@選択`）までの、実行中のブロックの積み重ね。 */
function framesTo(
  ops: readonly Op[],
  after: string | null,
  lineId: string,
): Frame[] | null {
  for (let index = 0; index < ops.length; index++) {
    const op = ops[index];
    if (op === undefined) continue;
    const here: Frame = { ops, index, after };
    if (
      (op.kind === "say" || op.kind === "narrate" || op.kind === "choice") &&
      op.line_id === lineId
    ) {
      return [here];
    }
    const inside: Frame = { ops, index: index + 1, after };
    if (op.kind === "choice") {
      for (const option of op.options) {
        if (option.line_id === lineId) return [here];
        const inner = framesTo(option.body, option.jump, lineId);
        if (inner !== null) return [inside, ...inner];
      }
    }
    if (op.kind === "if") {
      for (const branch of op.branches) {
        const inner = framesTo(branch.body, null, lineId);
        if (inner !== null) return [inside, ...inner];
      }
      if (op.else !== null) {
        const inner = framesTo(op.else, null, lineId);
        if (inner !== null) return [inside, ...inner];
      }
    }
  }
  return null;
}

/**
 * 本文が変わったとき、新しい本文の同じ行IDの行から続ける（T2-4）。変数・舞台・バックログは今のまま。
 * `@呼出` の積み重ねは、古い本文を指すので捨てる（呼び出し元へは戻らない）。行IDが無ければ null。
 */
export function resumeAt(
  prog: Program,
  state: State,
  lineId: string,
): State | null {
  for (const scene of prog.scenes.values()) {
    const frames = framesTo(scene.ops, null, lineId);
    if (frames === null) continue;
    // 今の行は実行し直すので、バックログの最後のその行は消す（2つ並ばないように）。
    const last = state.log[state.log.length - 1];
    const log =
      last !== undefined && !last.chosen && last.lineId === lineId
        ? state.log.slice(0, -1)
        : state.log;
    const resumed = run(prog, {
      ...state,
      scene: scene.id,
      frames,
      calls: [],
      events: [],
      log,
      noticed: true,
    });
    return state.calls.length > 0
      ? warn(
          resumed,
          "本文が変わったので、@呼出 の呼び出し元へは戻りません（デバッグの欄から始め直せます）",
        )
      : resumed;
  }
  return null;
}

/**
 * バックログの選んだ選択肢から、道（選択肢の順）を作る（縦書きビューの「プレビューで選んだ道」。T2-5）。
 * 選択肢の行の位置で、その `@選択` と何番目かを探す。
 */
export function stepsFromLog(
  prog: Program,
  log: readonly LogEntry[],
): RouteStep[] {
  const find = (
    ops: readonly Op[],
    span: IrSpan,
  ): { choice: IrSpan; option: number } | null => {
    for (const op of ops) {
      if (op.kind === "choice") {
        const index = op.options.findIndex(
          (o) => o.span[0] === span[0] && o.span[1] === span[1],
        );
        if (index !== -1) return { choice: op.span, option: index };
        for (const o of op.options) {
          const inner = find(o.body, span);
          if (inner !== null) return inner;
        }
      }
      if (op.kind === "if") {
        for (const b of op.branches) {
          const inner = find(b.body, span);
          if (inner !== null) return inner;
        }
        if (op.else !== null) {
          const inner = find(op.else, span);
          if (inner !== null) return inner;
        }
      }
    }
    return null;
  };
  const steps: RouteStep[] = [];
  for (const entry of log) {
    if (!entry.chosen) continue;
    const scene = prog.scenes.get(entry.scene);
    const found = scene === undefined ? null : find(scene.ops, entry.span);
    if (found !== null) steps.push({ scene: entry.scene, ...found });
  }
  return steps;
}
