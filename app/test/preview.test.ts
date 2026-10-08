// プレビューのランタイム（preview/runtime.ts、T1-11、D050）のテスト。サンプル「放課後の灯」の各エンディングまで、
// 選択肢を決めて通す（完了条件）。データはコアのテスト（crates/scn-core/tests/preview.rs）のスナップショットを
// 読む（コアのプレビューのデータと同じ形）。

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import type { Ir, IrInline, IrScene, Op, PreviewData } from "../src/core/ir.ts";
import {
  autoChoice,
  choose,
  initialVariables,
  jumpTo,
  lineIdsOf,
  next,
  type Program,
  program,
  type State,
  setVariable,
  skip,
  start,
  stopLocation,
} from "../src/preview/runtime.ts";

/** insta のスナップショット（`---` で囲んだ見出しの後ろが中身）。 */
function snapshot(name: string): string {
  const text = readFileSync(
    new URL(
      `../../crates/scn-core/tests/snapshots/${name}.snap`,
      import.meta.url,
    ),
    "utf8",
  );
  const body = text.split(/^---$/m).slice(2).join("---");
  return body.trim();
}

const DATA = JSON.parse(snapshot("preview__houkago")) as PreviewData;

function programOf(data: PreviewData): Program {
  return program(
    data.ir,
    data.variables.map((v) => ({
      key: v.key,
      type: v.type,
      values: v.values,
      initial: v.initial,
      scope: v.scope,
      range: v.range,
    })),
  );
}

const PROG = programOf(DATA);

const text = (inlines: readonly IrInline[]) =>
  inlines
    .map((i) =>
      i.kind === "text" || i.kind === "emphasis"
        ? i.text
        : i.kind === "ruby"
          ? i.base
          : "",
    )
    .join("");

/** 選択肢を表示文の一部で選びながら、止まるところ（エンディングなど）まで進める。通ったシーンも返す。 */
function play(
  prog: Program,
  picks: string[],
  options: { includeAiDrafts?: boolean; from?: State } = {},
): { state: State; scenes: string[] } {
  let s =
    options.from ??
    start(prog, { includeAiDrafts: options.includeAiDrafts ?? true });
  const scenes: string[] = [s.scene];
  for (let i = 0; i < 1000; i++) {
    if (scenes[scenes.length - 1] !== s.scene) scenes.push(s.scene);
    const stop = s.stop;
    if (stop.kind === "choice") {
      const want = picks.shift();
      assert.ok(want !== undefined, `${s.scene} の選択肢で、選ぶものがない`);
      const item = stop.items.find((it) => text(it.text).includes(want));
      assert.ok(
        item !== undefined,
        `${s.scene} に「${want}」が無い：${stop.items.map((it) => text(it.text)).join("／")}`,
      );
      s = choose(prog, s, item.index);
      continue;
    }
    if (
      stop.kind === "ending" ||
      stop.kind === "dead_end" ||
      stop.kind === "missing_scene" ||
      stop.kind === "no_caller"
    ) {
      break;
    }
    s = next(prog, s);
  }
  assert.deepEqual(picks, [], "選ばなかった選択肢が残っている");
  return { state: s, scenes };
}

test("サンプル：灯のエンディング（akari_end）まで、選択肢を決めて通せる", () => {
  const { state, scenes } = play(PROG, ["すぐに行く", "ごめん、遅くなって"]);
  assert.equal(state.stop.kind, "ending");
  assert.equal(
    state.stop.kind === "ending" ? state.stop.id : null,
    "akari_end",
  );
  assert.deepEqual(scenes, [
    "s010",
    "s011",
    "s012",
    "s013a",
    "s020",
    "s021",
    "s022",
    "s040",
  ]);
  assert.equal(state.vars.get("flag.勇気"), 1);
  assert.equal(state.vars.get("akari.好感度"), 3);
  assert.equal(state.vars.get("flag.ルート"), "灯");
  assert.deepEqual(state.warnings, []);
});

test("サンプル：紬のエンディング（tsumugi_end）まで、選択肢を決めて通せる", () => {
  const { state, scenes } = play(PROG, ["少し迷って", "ごめん"]);
  assert.equal(
    state.stop.kind === "ending" ? state.stop.id : null,
    "tsumugi_end",
  );
  assert.ok(scenes.includes("s013b"));
  assert.ok(scenes.includes("s030"));
  assert.equal(state.vars.get("tsumugi.好感度"), 1);
});

test("サンプル：bad_end は選択肢だけでは着けないので、デバッグのジャンプで s030 から通す", () => {
  // s030 へ行く道は s013b だけで、そこで必ず紬の好感度が1になる（選択肢だけでは bad_end に着かない）。
  const s = start(PROG, { includeAiDrafts: true });
  const { state } = play(PROG, [], { from: jumpTo(PROG, s, "s030") });
  assert.equal(state.stop.kind === "ending" ? state.stop.id : null, "bad_end");
  // 変数を変えてから飛んでも、その値で分かれる。
  const raised = setVariable(s, "tsumugi.好感度", 1);
  const tsumugi = play(PROG, [], { from: jumpTo(PROG, raised, "s030") });
  assert.equal(
    tsumugi.state.stop.kind === "ending" ? tsumugi.state.stop.id : null,
    "tsumugi_end",
  );
});

test("サンプル：bad_end へは、選択肢で s030（スタブ）まで進め、そこでデバッグの欄で変数を変えて通す", () => {
  // s030 へ行く道（s013b）で必ず紬の好感度が1になるので、選択肢だけでは bad_end に着かない（D050）。
  let s = start(PROG, { includeAiDrafts: true });
  for (
    let i = 0;
    i < 200 && !(s.scene === "s030" && s.stop.kind === "stub");
    i++
  ) {
    if (s.stop.kind === "choice") {
      const want = s.scene === "s011" ? "少し迷って" : "ごめん";
      const item = s.stop.items.find((it) => text(it.text).includes(want));
      s = choose(PROG, s, item?.index ?? 0);
    } else {
      s = next(PROG, s);
    }
  }
  assert.equal(s.stop.kind, "stub");
  assert.equal(s.vars.get("tsumugi.好感度"), 1);
  const { state } = play(PROG, [], {
    from: setVariable(s, "tsumugi.好感度", 0),
  });
  assert.equal(state.stop.kind === "ending" ? state.stop.id : null, "bad_end");
});

test("選択肢で止まっているときに変数を変えると、選べるかどうかを決め直す", () => {
  let s = start(PROG, { includeAiDrafts: true });
  while (s.scene !== "s012" || s.stop.kind !== "choice") {
    s = s.stop.kind === "choice" ? choose(PROG, s, 0) : next(PROG, s);
  }
  const before =
    s.stop.kind === "choice" ? s.stop.items.filter((i) => i.enabled).length : 0;
  const raised = setVariable(s, "flag.勇気", 2);
  const after =
    raised.stop.kind === "choice"
      ? raised.stop.items.filter((i) => i.enabled).length
      : 0;
  assert.equal(after, before + 1);
});

test("選択肢：条件が偽のものは選べず、選んでも進まない。自動選択は選べるものから選ぶ", () => {
  let s = start(PROG, { includeAiDrafts: true });
  while (s.stop.kind !== "choice") s = next(PROG, s);
  s = choose(PROG, s, 0);
  while (s.stop.kind !== "choice" && s.scene !== "s012") s = next(PROG, s);
  while (s.stop.kind !== "choice") s = next(PROG, s);
  const stop = s.stop;
  assert.equal(stop.kind, "choice");
  if (stop.kind !== "choice") return;
  const locked = stop.items.find((i) => !i.enabled);
  assert.ok(locked !== undefined, "勇気>=2 の選択肢は選べない");
  assert.equal(locked.condText, "勇気 >= 2");
  assert.equal(choose(PROG, s, locked.index), s);
  assert.notEqual(autoChoice(stop, "first"), locked.index);
  assert.notEqual(
    autoChoice(stop, "random", () => 0.99),
    locked.index,
  );
});

test("AI下書きを含めないと、AI下書きのシーンの台詞と地の文を飛ばし、1回知らせる", () => {
  let s = start(PROG, { includeAiDrafts: false });
  const kinds: string[] = [];
  for (let i = 0; i < 400 && s.stop.kind !== "ending"; i++) {
    if (s.stop.kind === "choice") {
      const want = s.scene === "s011" ? "少し迷って" : "ごめん";
      const item = s.stop.items.find((it) => text(it.text).includes(want));
      s = choose(PROG, s, item?.index ?? 0);
      continue;
    }
    kinds.push(`${s.scene}:${s.stop.kind}`);
    s = next(PROG, s);
  }
  assert.ok(kinds.includes("s013b:ai_skipped"));
  assert.ok(!kinds.includes("s013b:line"));
  // 飛ばしても、そのシーンの @設定 とジャンプは実行する。
  assert.equal(s.vars.get("tsumugi.好感度"), 1);
});

test("スタブのシーンは1回知らせてから、そのシーンの命令（ジャンプ）を続ける", () => {
  let s = jumpTo(PROG, start(PROG, { includeAiDrafts: true }), "s022");
  assert.equal(s.stop.kind, "stub");
  s = next(PROG, s);
  assert.equal(s.scene, "s040");
  // 行き止まりのスタブ（s014）。
  const dead = next(PROG, jumpTo(PROG, s, "s014"));
  assert.equal(dead.stop.kind, "dead_end");
});

test("スキップ：次の選択肢まで進め、自動選択ならエンディングまで", () => {
  const s = start(PROG, { includeAiDrafts: true });
  assert.equal(skip(PROG, s).stop.kind, "choice");
  const end = skip(PROG, s, "first");
  assert.equal(end.stop.kind, "ending");
});

test("今の行の位置と、シーンの行ID（3/9行）", () => {
  let s = jumpTo(PROG, start(PROG, { includeAiDrafts: true }), "s012");
  const scene = PROG.scenes.get("s012") as IrScene;
  const ids = lineIdsOf(scene);
  assert.ok(ids.length > 3);
  assert.equal(ids[0], "k3f9");
  assert.deepEqual(stopLocation(s).lineId, "k3f9");
  s = next(PROG, next(PROG, s));
  assert.equal(stopLocation(s).lineId, "k3fb");
  assert.equal(ids.indexOf("k3fb"), 2);
  // バックログに入っている。
  assert.deepEqual(
    s.log.slice(-3).map((e) => e.lineId),
    ["k3f9", "k3fa", "k3fb"],
  );
});

test("舞台：背景、立ち絵の位置（省けば今の位置か中央）、BGM、効果音の出来事", () => {
  const s = jumpTo(PROG, start(PROG, { includeAiDrafts: true }), "s012");
  assert.equal(s.stage.background, "屋上/夕方");
  assert.equal(s.stage.bgm, "切ない");
  assert.equal(s.stage.sprites.left?.character, "akari");
  assert.ok(s.events.some((e) => e.kind === "se" && e.name === "風"));
  assert.ok(
    s.events.some((e) => e.kind === "transition" && e.target === "background"),
  );
});

/** 小さなIR（呼び出しと戻り、無いシーン、呼び出し元の無い @戻る、待ち、誤りのある条件）。 */
function tiny(scenes: { id: string; ops: Op[] }[]): Program {
  const ir: Ir = {
    ir_version: 1,
    start_scene: null,
    scenes: scenes.map((s) => ({
      id: s.id,
      title: s.id,
      file: "story/a.scn",
      span: [0, 0],
      meta: { summary: null, status: null, route: null, tags: [], extra: {} },
      is_stub: false,
      ops: s.ops,
    })),
  };
  return program(ir, [
    {
      key: "flag.n",
      type: "int",
      values: [],
      initial: 0,
      scope: "save",
      range: [0, 1],
    },
    {
      key: "周回.p",
      type: "bool",
      values: [],
      initial: false,
      scope: "persistent",
      range: null,
    },
  ]);
}
const SP = [0, 0] as const;
const say = (t: string): Op => ({
  kind: "narrate",
  span: SP,
  line_id: null,
  text: [{ kind: "text", text: t }],
});

test("@呼出 と @戻る、無いシーン、呼び出し元の無い @戻る、待ち、範囲の外の値", () => {
  const prog = tiny([
    {
      id: "a",
      ops: [
        { kind: "call", span: SP, target: "b" },
        say("戻った"),
        { kind: "wait", span: SP, seconds: 1 },
        {
          kind: "set",
          span: SP,
          var: "flag.n",
          op: "+=",
          value: { kind: "int", value: 5 },
        },
        { kind: "jump", span: SP, target: "zz" },
      ],
    },
    { id: "b", ops: [say("呼ばれた"), { kind: "return", span: SP }] },
    { id: "c", ops: [{ kind: "return", span: SP }] },
  ]);
  let s = start(prog, { includeAiDrafts: true });
  assert.equal(s.scene, "b");
  s = next(prog, s);
  assert.equal(s.scene, "a");
  assert.equal(s.stop.kind, "line");
  s = next(prog, s);
  assert.equal(s.stop.kind, "wait");
  s = next(prog, s);
  assert.equal(s.stop.kind, "missing_scene");
  assert.equal(s.vars.get("flag.n"), 5);
  assert.match(s.warnings.join("\n"), /範囲（0〜1）の外/);
  const orphan = jumpTo(prog, s, "c");
  assert.equal(orphan.stop.kind, "no_caller");
});

test("宣言の型に合わない値の @設定 は書かずに知らせる", () => {
  const prog = tiny([
    {
      id: "a",
      ops: [
        {
          kind: "set",
          span: SP,
          var: "flag.n",
          op: "=",
          value: { kind: "string", value: "x" },
        },
        { kind: "ending", span: SP, id: "end" },
      ],
    },
  ]);
  const s = start(prog, { includeAiDrafts: true });
  assert.equal(s.vars.get("flag.n"), 0);
  assert.match(s.warnings.join("\n"), /宣言の型（int）に合わない/);
});

test("誤りのある条件は偽として扱い、知らせる。「最初から」でも永続の変数は残す", () => {
  const prog = tiny([
    {
      id: "a",
      ops: [
        {
          kind: "if",
          span: SP,
          branches: [
            { span: SP, cond_text: "x", cond: null, body: [say("真")] },
          ],
          else: [say("偽")],
        },
        { kind: "ending", span: SP, id: "end" },
      ],
    },
  ]);
  const s = start(prog, { includeAiDrafts: true });
  assert.equal(s.stop.kind === "line" ? text(s.stop.op.text) : "", "偽");
  assert.match(s.warnings[0] ?? "", /条件を読めません/);
  const kept = initialVariables(
    prog,
    new Map<string, boolean | number | string>([
      ["周回.p", true],
      ["flag.n", 1],
    ]),
  );
  assert.equal(kept.get("周回.p"), true);
  assert.equal(kept.get("flag.n"), 0);
});

test("プレビューのデータ：画面とウィンドウの大きさ、括弧と表示スタイル、素材（サンプルはすべて仮素材）", () => {
  assert.deepEqual(DATA.screen, { width: 1280, height: 720 });
  assert.deepEqual(DATA.message_window, { lines: 3, chars_per_line: 24 });
  assert.deepEqual(
    DATA.brackets.map((b) => `${b.open}${b.close}${b.style}`),
    ["「」normal", "『』thought"],
  );
  assert.equal(DATA.assets.backgrounds["屋上/夕方"], null);
  const sound = (table: typeof DATA.assets.bgm, name: string) => table[name];
  assert.equal(sound(DATA.assets.bgm, "日常")?.loop, true);
  assert.equal(sound(DATA.assets.se, "風")?.loop, false);
  assert.equal(
    DATA.variables.find((v) => v.key === "flag.ルート")?.initial,
    "共通",
  );
});

/** 選択肢のすべての組み合わせをたどって、着けるエンディングを集める（選べない選択肢は選ばない）。 */
function endingsByChoices(prog: Program): Set<string> {
  const found = new Set<string>();
  const walk = (from: State, depth: number) => {
    let s = from;
    for (let i = 0; i < 1000 && depth < 50; i++) {
      const stop = s.stop;
      if (stop.kind === "choice") {
        for (const item of stop.items.filter((it) => it.enabled)) {
          walk(choose(prog, s, item.index), depth + 1);
        }
        return;
      }
      if (stop.kind === "ending") {
        found.add(stop.id ?? "");
        return;
      }
      if (
        stop.kind === "dead_end" ||
        stop.kind === "missing_scene" ||
        stop.kind === "no_caller"
      ) {
        return;
      }
      s = next(prog, s);
    }
  };
  walk(start(prog, { includeAiDrafts: true }), 0);
  return found;
}

test("ゲートの短編（samples/amayadori）：選択肢だけで、すべてのエンディングに着ける", () => {
  const data = JSON.parse(snapshot("preview__amayadori")) as PreviewData;
  const prog = programOf(data);
  assert.deepEqual([...endingsByChoices(prog)].sort(), [
    "bus_end",
    "rain_end",
    "stay_end",
  ]);
  // 誤りの知らせが出ないこと（akari_end の道筋と同じく、通して遊べる）。
  const bus = play(prog, ["話しかける", "傘を差し出す"]);
  assert.equal(
    bus.state.stop.kind === "ending" ? bus.state.stop.id : null,
    "bus_end",
  );
  assert.deepEqual(bus.state.warnings, []);
  assert.ok(bus.scenes.includes("s90"), "@呼出 で構内放送を通る");
});

test("サンプル（samples/houkago）：選択肢だけで着けるエンディングは akari_end と tsumugi_end", () => {
  assert.deepEqual([...endingsByChoices(PROG)].sort(), [
    "akari_end",
    "tsumugi_end",
  ]);
});
