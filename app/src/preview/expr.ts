// プレビューの式の評価（T1-11、D050。仕様書14節の「評価」）。
//
// - 値は真偽・整数・文字列（列挙の値は文字列）。整数の割り算は0の向きへ切り捨て、0での割り算は誤り。
// - 書かれていない宣言の無い変数（W602）を読むと誤り。誤りのある条件は偽として扱い、知らせを出す
//   （止めない。壊れたまま書ける）。
// - 型が合わない演算（E603 の式など）も誤り。`==`・`!=` は型も比べる。

import type { IrExpr } from "../core/ir.ts";

export type Value = boolean | number | string;

/** 評価の結果。誤りなら、その文。 */
export type Evaluated =
  | { readonly ok: true; readonly value: Value }
  | { readonly ok: false; readonly error: string };

/** 変数の今の値（書かれていない宣言の無い変数は無い）。 */
export type Variables = ReadonlyMap<string, Value>;

function fail(error: string): Evaluated {
  return { ok: false, error };
}

function ok(value: Value): Evaluated {
  return { ok: true, value };
}

function int(v: Evaluated, op: string): number | string {
  if (!v.ok) return v.error;
  return typeof v.value === "number"
    ? v.value
    : `${op} は整数にだけ使えます（${show(v.value)}）`;
}

function bool(v: Evaluated, op: string): boolean | string {
  if (!v.ok) return v.error;
  return typeof v.value === "boolean"
    ? v.value
    : `${op} は真偽にだけ使えます（${show(v.value)}）`;
}

/** 値の見せ方（変数の埋め込み、デバッグの表示）。 */
export function show(value: Value): string {
  return typeof value === "string" ? value : String(value);
}

/** 式を評価する。 */
export function evaluate(expr: IrExpr, vars: Variables): Evaluated {
  switch (expr.kind) {
    case "int":
    case "string":
    case "bool":
      return ok(expr.value);
    case "var": {
      const value = vars.get(expr.name);
      return value === undefined
        ? fail(`変数 ${expr.name} はまだ書かれていません（宣言がありません）`)
        : ok(value);
    }
    case "unary": {
      const v = evaluate(expr.operand, vars);
      if (expr.op === "-") {
        const n = int(v, "-");
        return typeof n === "number" ? ok(n === 0 ? 0 : -n) : fail(n);
      }
      const b = bool(v, "not");
      return typeof b === "boolean" ? ok(!b) : fail(b);
    }
    case "binary":
      return binary(expr.op, expr.lhs, expr.rhs, vars);
  }
}

function binary(
  op: string,
  lhs: IrExpr,
  rhs: IrExpr,
  vars: Variables,
): Evaluated {
  if (op === "and" || op === "or") {
    // 左だけで決まれば右を評価しない。
    const l = bool(evaluate(lhs, vars), op);
    if (typeof l === "string") return fail(l);
    if (op === "and" && !l) return ok(false);
    if (op === "or" && l) return ok(true);
    const r = bool(evaluate(rhs, vars), op);
    return typeof r === "boolean" ? ok(r) : fail(r);
  }
  const lv = evaluate(lhs, vars);
  const rv = evaluate(rhs, vars);
  if (op === "==" || op === "!=") {
    if (!lv.ok) return lv;
    if (!rv.ok) return rv;
    const same = lv.value === rv.value;
    return ok(op === "==" ? same : !same);
  }
  const l = int(lv, op);
  if (typeof l === "string") return fail(l);
  const r = int(rv, op);
  if (typeof r === "string") return fail(r);
  switch (op) {
    case "+":
      return ok(l + r);
    case "-":
      return ok(l - r);
    case "*":
      return ok(l * r);
    case "/":
      if (r === 0) return fail("0 で割りました");
      return ok(Math.trunc(l / r) || 0);
    case "<":
      return ok(l < r);
    case "<=":
      return ok(l <= r);
    case ">":
      return ok(l > r);
    case ">=":
      return ok(l >= r);
    default:
      return fail(`知らない演算子です：${op}`);
  }
}

/** 条件を評価する。誤りなら偽とし、誤りの文を添える。式が崩れていれば（E601。`cond` が null）偽。 */
export function condition(
  expr: IrExpr | null,
  text: string | null,
  vars: Variables,
): { readonly value: boolean; readonly error: string | null } {
  if (expr === null) {
    return {
      value: false,
      error: `条件を読めません（${text ?? ""}）。偽として扱います`,
    };
  }
  const v = evaluate(expr, vars);
  if (!v.ok) return { value: false, error: `${v.error}。偽として扱います` };
  if (typeof v.value !== "boolean") {
    return {
      value: false,
      error: `条件が真偽ではありません（${show(v.value)}）。偽として扱います`,
    };
  }
  return { value: v.value, error: null };
}

/** `@設定` の新しい値（`=`・`+=`・`-=`）。誤りなら書かない。 */
export function assign(
  name: string,
  op: "=" | "+=" | "-=",
  expr: IrExpr,
  vars: Variables,
): Evaluated {
  const v = evaluate(expr, vars);
  if (!v.ok || op === "=") return v;
  const current = vars.get(name);
  if (current === undefined) {
    return fail(`変数 ${name} はまだ書かれていません（宣言がありません）`);
  }
  if (typeof current !== "number" || typeof v.value !== "number") {
    return fail(`${op} は整数にだけ使えます`);
  }
  return ok(op === "+=" ? current + v.value : current - v.value);
}
