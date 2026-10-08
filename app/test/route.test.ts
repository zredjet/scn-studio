// カーソル位置から再生（preview/runtime.ts の replay・resumeAt、T2-4、D055）のテスト。コアのテスト
// （crates/scn-core/tests/route.rs）が求めた、サンプルのすべての命令までの最短経路を、プレビューのランタイムで
// 速送りし、目的の命令の手前で止まり、コアと同じ変数になることを確かめる（完了条件）。

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import type { PreviewData } from "../src/core/ir.ts";
import type { RouteStep } from "../src/core/types.ts";
import {
  choose,
  next,
  type Program,
  program,
  type RouteTarget,
  replay,
  resumeAt,
  start,
  stepsFromLog,
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
  return text.split(/^---$/m).slice(2).join("---").trim();
}

interface Case {
  readonly kind: string;
  readonly route: {
    readonly target: RouteTarget & { readonly line_id: string | null };
    readonly reachable: boolean;
    readonly steps: readonly RouteStep[];
    readonly variables: Readonly<Record<string, boolean | number | string>>;
    readonly calls: readonly string[];
  };
}

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

for (const name of ["houkago", "amayadori"]) {
  test(`${name}：すべての命令へ、コアの最短経路で速送りすると、コアと同じ変数で手前に止まる`, () => {
    const prog = programOf(
      JSON.parse(snapshot(`preview__${name}`)) as PreviewData,
    );
    const cases = JSON.parse(snapshot(`route__${name}`)) as Case[];
    let reached = 0;
    for (const c of cases) {
      if (!c.route.reachable) continue;
      const where = `${c.route.target.scene} ${c.kind} ${c.route.target.span[0]}`;
      const result = replay(
        prog,
        { includeAiDrafts: true },
        c.route.steps,
        c.route.target,
      );
      assert.ok(result.ok, `${where}：${result.ok ? "" : result.error}`);
      assert.equal(result.at.scene, c.route.target.scene, where);
      assert.deepEqual(
        Object.fromEntries(result.at.vars),
        c.route.variables,
        where,
      );
      assert.deepEqual(
        result.at.calls.map((x) => x.scene),
        c.route.calls,
        where,
      );
      // AI下書きを含めなくても、同じ道で着く（飛ばすのは台詞と地の文だけ）。
      const skipped = replay(
        prog,
        { includeAiDrafts: false },
        c.route.steps,
        c.route.target,
      );
      assert.ok(skipped.ok, where);
      reached += 1;
    }
    assert.ok(reached > 20, `${name}：着ける命令が少なすぎる（${reached}）`);
  });
}

test("台詞から始めると、その台詞で止まる。道が合わなければ誤り", () => {
  const prog = programOf(
    JSON.parse(snapshot("preview__houkago")) as PreviewData,
  );
  const cases = JSON.parse(snapshot("route__houkago")) as Case[];
  const say = cases.find(
    (c) => c.kind === "say" && c.route.reachable && c.route.steps.length > 0,
  );
  assert.ok(say !== undefined);
  const result = replay(
    prog,
    { includeAiDrafts: true },
    say.route.steps,
    say.route.target,
  );
  assert.ok(result.ok);
  assert.equal(result.state.stop.kind, "line");
  assert.deepEqual(stopLocation(result.state).span, say.route.target.span);
  // 選択肢の道が無いと、選択肢で合わずに止まる。
  const wrong = replay(prog, { includeAiDrafts: true }, [], say.route.target);
  assert.equal(wrong.ok, false);
});

test("今の行IDから続ける：同じ行で止まり、変数と舞台は今のまま。無い行IDなら null", () => {
  const prog = programOf(
    JSON.parse(snapshot("preview__houkago")) as PreviewData,
  );
  let state = start(prog, { includeAiDrafts: true });
  state = next(prog, next(prog, state));
  const here = stopLocation(state);
  assert.ok(here.lineId !== null);
  const resumed = resumeAt(prog, state, here.lineId);
  assert.ok(resumed !== null);
  assert.deepEqual(stopLocation(resumed), here);
  assert.deepEqual(resumed.stage, state.stage);
  assert.deepEqual(resumed.vars, state.vars);
  // 今の行は実行し直すが、バックログに2つ並べない。
  assert.equal(resumed.log.length, state.log.length);
  assert.equal(resumeAt(prog, state, "zzzz"), null);
});

test("ADV で選んだ選択肢の順を、バックログから作る（縦書きビューの「プレビューで選んだ道」）", () => {
  const prog = programOf(
    JSON.parse(snapshot("preview__houkago")) as PreviewData,
  );
  let state = start(prog, { includeAiDrafts: true });
  const picks = [1, 0];
  for (let n = 0; n < 200 && picks.length > 0; n++) {
    if (state.stop.kind === "choice") {
      state = choose(prog, state, picks.shift() ?? 0);
    } else {
      state = next(prog, state);
    }
  }
  const steps = stepsFromLog(prog, state.log);
  assert.deepEqual(
    steps.map((s) => [s.scene, s.option]),
    [
      ["s011", 1],
      ["s012", 0],
    ],
  );
});
