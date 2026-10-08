// プレビューの式の評価と、メッセージの組み立て（preview/expr.ts・text.ts、T1-11、D050）のテスト。

import assert from "node:assert/strict";
import { test } from "node:test";
import type { IrExpr, IrInline } from "../src/core/ir.ts";
import { assign, condition, evaluate } from "../src/preview/expr.ts";
import { locateLine } from "../src/preview/locate.ts";
import { charWidth, paginate, piecesOf } from "../src/preview/text.ts";

const v = (name: string): IrExpr => ({ kind: "var", name });
const n = (value: number): IrExpr => ({ kind: "int", value });
const b = (op: string, lhs: IrExpr, rhs: IrExpr): IrExpr => ({
  kind: "binary",
  op,
  lhs,
  rhs,
});
const vars = new Map<string, number | boolean | string>([
  ["flag.勇気", 2],
  ["flag.ルート", "灯"],
  ["akari.転校済み", false],
]);

test("式：整数の演算、比較、真偽、列挙", () => {
  assert.deepEqual(evaluate(b(">=", v("flag.勇気"), n(2)), vars), {
    ok: true,
    value: true,
  });
  assert.deepEqual(evaluate(b("+", v("flag.勇気"), n(3)), vars), {
    ok: true,
    value: 5,
  });
  // 割り算は0の向きへ切り捨て。
  assert.deepEqual(evaluate(b("/", n(-7), n(2)), vars), {
    ok: true,
    value: -3,
  });
  assert.deepEqual(evaluate(b("/", n(7), n(2)), vars), { ok: true, value: 3 });
  assert.equal(evaluate(b("/", n(1), n(0)), vars).ok, false);
  assert.deepEqual(
    evaluate(b("==", v("flag.ルート"), { kind: "string", value: "灯" }), vars),
    { ok: true, value: true },
  );
  // 型の違う == は偽（型も比べる）。
  assert.deepEqual(
    evaluate(b("==", v("flag.勇気"), { kind: "string", value: "2" }), vars),
    { ok: true, value: false },
  );
  assert.deepEqual(
    evaluate({ kind: "unary", op: "not", operand: v("akari.転校済み") }, vars),
    { ok: true, value: true },
  );
  // and は左が偽なら右を評価しない（右の未定義の変数で誤りにならない）。
  assert.deepEqual(
    evaluate(b("and", v("akari.転校済み"), v("flag.無い")), vars),
    { ok: true, value: false },
  );
  assert.equal(evaluate(b("+", v("flag.ルート"), n(1)), vars).ok, false);
});

test("条件：誤りと崩れた式は偽として扱い、知らせる", () => {
  assert.deepEqual(
    condition(b(">=", v("flag.勇気"), n(1)), "勇気 >= 1", vars),
    {
      value: true,
      error: null,
    },
  );
  const undefinedVar = condition(v("flag.無い"), "無い", vars);
  assert.equal(undefinedVar.value, false);
  assert.match(undefinedVar.error ?? "", /まだ書かれていません/);
  const broken = condition(null, "勇気 >=", vars);
  assert.equal(broken.value, false);
  assert.match(broken.error ?? "", /読めません/);
  assert.equal(condition(n(1), "1", vars).value, false);
});

test("@設定：=、+=、-=。整数以外の += と、書かれていない変数への += は誤り", () => {
  assert.deepEqual(assign("flag.勇気", "+=", n(1), vars), {
    ok: true,
    value: 3,
  });
  assert.deepEqual(assign("flag.勇気", "-=", n(5), vars), {
    ok: true,
    value: -3,
  });
  assert.deepEqual(assign("flag.新しい", "=", n(1), vars), {
    ok: true,
    value: 1,
  });
  assert.equal(assign("flag.新しい", "+=", n(1), vars).ok, false);
  assert.equal(assign("flag.ルート", "+=", n(1), vars).ok, false);
});

test("文中の記法を部品にする：ルビ、傍点、待ち、埋め込み", () => {
  const inlines: IrInline[] = [
    { kind: "text", text: "風が" },
    { kind: "ruby", base: "乱", reading: "みだ" },
    { kind: "emphasis", text: "強い" },
    { kind: "wait", seconds: 0.5 },
    { kind: "interpolation", expr_text: "勇気", expr: v("flag.勇気") },
    { kind: "interpolation", expr_text: "無い", expr: v("flag.無い") },
  ];
  const { pieces, warnings } = piecesOf(inlines, vars);
  assert.deepEqual(pieces, [
    { kind: "text", text: "風が", emphasis: false },
    { kind: "ruby", base: "乱", reading: "みだ" },
    { kind: "text", text: "強い", emphasis: true },
    { kind: "wait", seconds: 0.5 },
    { kind: "text", text: "2", emphasis: false, uncounted: true },
    { kind: "text", text: "{=無い}", emphasis: false, uncounted: true },
  ]);
  assert.equal(warnings.length, 1);
});

test("ページに分ける：{p} と、ウィンドウからはみ出すとき（全角1、半角0.5）", () => {
  assert.equal(charWidth("あ"), 1);
  assert.equal(charWidth("a"), 0.5);
  assert.equal(charWidth("ｱ"), 0.5);
  const { pieces } = piecesOf(
    [
      { kind: "text", text: "一ページ目" },
      { kind: "page_break" },
      { kind: "text", text: "二ページ目" },
    ],
    vars,
  );
  const split = paginate(pieces, { lines: 3, chars_per_line: 24 });
  assert.equal(split.pages.length, 2);
  assert.equal(split.overflowed, false);
  // 2行×4字に、全角10字は3行になるので2ページ。半角8字は1行に収まる。
  const long = paginate(
    [{ kind: "text", text: "あいうえおかきくけこ", emphasis: false }],
    {
      lines: 2,
      chars_per_line: 4,
    },
  );
  assert.equal(long.pages.length, 2);
  assert.ok(long.overflowed);
  const textOf = (
    page: readonly { kind: string; text?: string }[] | undefined,
  ) => (page ?? []).map((p) => p.text ?? "").join("");
  assert.equal(textOf(long.pages[0]), "あいうえおかきく");
  assert.equal(textOf(long.pages[1]), "けこ");
  const half = paginate([{ kind: "text", text: "abcdefgh", emphasis: false }], {
    lines: 1,
    chars_per_line: 4,
  });
  assert.equal(half.pages.length, 1);
  // 複数行の台詞の改行で行を改める。
  const lines = paginate(
    [
      { kind: "text", text: "一", emphasis: false },
      { kind: "newline" },
      { kind: "text", text: "二", emphasis: false },
      { kind: "newline" },
      { kind: "text", text: "三", emphasis: false },
    ],
    { lines: 2, chars_per_line: 10 },
  );
  assert.equal(lines.pages.length, 2);
});

test("ページに分ける：各ページの最初と最後の空の行は数えず、描かない（W203 と同じ）", () => {
  const nl = { kind: "newline" } as const;
  const t = (text: string) =>
    ({ kind: "text", text, emphasis: false }) as const;
  const window = { lines: 3, chars_per_line: 24 };
  // 灯「\nあ\nい\nう\n」：括弧だけの行を数えないので、3行に収まる。
  const a = paginate([nl, t("あ"), nl, t("い"), nl, t("う"), nl], window);
  assert.equal(a.overflowed, false);
  assert.equal(a.pages.length, 1);
  assert.deepEqual(a.pages[0], [t("あ"), nl, t("い"), nl, t("う")]);
  // 灯「え\nお\nか\n」
  const b = paginate([t("え"), nl, t("お"), nl, t("か"), nl], window);
  assert.equal(b.overflowed, false);
  // {p} の前後の改行も数えない。待ちは隣の行へ移して残す。
  const c = paginate(
    [
      t("一"),
      nl,
      { kind: "wait", seconds: -1 },
      nl,
      { kind: "wait", seconds: 1 },
      t("二"),
    ],
    { lines: 1, chars_per_line: 24 },
  );
  assert.equal(c.overflowed, false);
  assert.equal(c.pages.length, 2);
  assert.deepEqual(c.pages[1], [{ kind: "wait", seconds: 1 }, t("二")]);
});

test("プレビュー中の行を本文の中で探す：行IDで、無ければバイト位置で", () => {
  const text =
    "## 屋上 #s012\n灯「やあ」 ^k3f9\r\n😀地の文 ^k3fa\n行IDの無い行\n";
  // エディタの位置は、`\r\n` を1つと数える（core/position.ts）。
  assert.equal(locateLine(text, "k3f9", 0), text.indexOf("灯"));
  assert.equal(locateLine(text, "k3fa", 0), text.indexOf("😀") - 1);
  // 行IDが見つからなければ、バイト位置（絵文字は4バイト、UTF-16で2）。
  const byte = new TextEncoder().encode(
    text.slice(0, text.indexOf("行ID")),
  ).length;
  assert.equal(locateLine(text, "zzzz", byte), text.indexOf("行ID") - 1);
  assert.equal(locateLine(text, null, byte), text.indexOf("行ID") - 1);
  assert.equal(locateLine(text, null, 10_000), null);
  // 行の途中の ^ は行IDではない。
  assert.equal(locateLine("a ^k3f9 b\n", "k3f9", 0), 0);
});

test("ページに分ける：W203（check::max_page_lines）と同じ数え方", () => {
  const nl = { kind: "newline" } as const;
  const t = (text: string) =>
    ({ kind: "text", text, emphasis: false }) as const;
  // 灯「あ\nい\n{se 足音}\n」（2行×2字）：最初と最後の空の行を1つずつ除くので、効果音だけの行は数え、3行ではみ出す。
  const se = paginate(
    [t("あ"), nl, t("い"), nl, { kind: "se", name: "足音" }, nl],
    { lines: 2, chars_per_line: 2 },
  );
  assert.equal(se.overflowed, true);
  // 半角と全角が混ざる行は、行の幅の合計で数える（aああaあ は 2行×2字 に収まる）。
  const mixed = paginate([t("aああaあ")], { lines: 2, chars_per_line: 2 });
  assert.equal(mixed.overflowed, false);
  assert.equal(mixed.pages.length, 1);
  // 変数の埋め込みは数えない。
  const value = paginate(
    [{ kind: "text", text: "とても長い値", emphasis: false, uncounted: true }],
    { lines: 1, chars_per_line: 2 },
  );
  assert.equal(value.overflowed, false);
});
