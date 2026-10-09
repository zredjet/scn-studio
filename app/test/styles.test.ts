// 見た目の直書きのテスト（T4-16、D086）。CSS と CodeMirror のテーマが、色・文字の大きさ・太さ・角丸・影を
// デザイントークン（app/src/styles/tokens.css。値の正は docs/ui/mockups/scn-ui.css）から取っていることを確かめる。

import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { test } from "node:test";
import { scnHighlightStyle } from "@scn-studio/lezer-scn";
import { editorThemeSpec } from "../src/editor/state.ts";
import { writingThemeSpec } from "../src/editor/writing.ts";

/** トークンを定義する tokens.css を除いた、画面の CSS。 */
const CSS_FILES = ["app.css", "components.css", "preview.css"];

type Decl = {
  readonly where: string;
  readonly prop: string;
  readonly value: string;
};

/** CSS の宣言を、規則の選択子と一緒に取り出す（入れ子の @media の中も）。 */
function cssDecls(file: string): Decl[] {
  const text = readFileSync(
    new URL(`../src/styles/${file}`, import.meta.url),
    "utf8",
  ).replace(/\/\*[\s\S]*?\*\//g, "");
  const decls: Decl[] = [];
  for (const [, selector = "", body = ""] of text.matchAll(
    /([^{}]+)\{([^{}]*)\}/g,
  )) {
    const where = `${file} ${selector.trim().split(/\s+/).join(" ")}`;
    for (const part of body.split(";")) {
      const i = part.indexOf(":");
      if (i < 0) continue;
      decls.push({
        where,
        prop: part.slice(0, i).trim(),
        value: part.slice(i + 1).trim(),
      });
    }
  }
  return decls;
}

/** CodeMirror のテーマ（`EditorView.theme` に渡すもの）の宣言を取り出す。名前は CSS の書き方にする。 */
function themeDecls(
  name: string,
  spec: Record<string, Record<string, string>>,
): Decl[] {
  return Object.entries(spec).flatMap(([selector, style]) =>
    Object.entries(style).map(([prop, value]) => ({
      where: `${name} ${selector}`,
      prop: prop.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`),
      value: String(value),
    })),
  );
}

const COLOR_LITERAL =
  /#[0-9a-f]{3,8}\b|\b(?:rgba?|hsla?|hwb|lab|lch|oklch|oklab)\(/i;
/** 色の名前（色を取るプロパティの値だけで探す。`transparent` と `currentColor` は可）。 */
const COLOR_NAME =
  /\b(?:white|black|red|green|blue|gr[ae]y|silver|yellow|orange|purple|pink|navy|teal|maroon|olive|lime|aqua|fuchsia)\b/i;
const COLOR_PROP =
  /color|background|border|outline|fill|stroke|shadow|decoration/;
/** 白の文字（塗った札やボタンの上の文字）だけは直書きを認める（見本 v1 も同じ）。 */
const WHITE_TEXT = /^#fff(?:fff)?$/i;

/** トークンを通らない値を探す。決めた例外は通す。 */
function problems(decls: readonly Decl[]): string[] {
  const found: string[] = [];
  for (const { where, prop, value } of decls) {
    if (prop.startsWith("--")) continue;
    const bad = (why: string) =>
      found.push(`${where} { ${prop}: ${value} } — ${why}`);
    if (
      (COLOR_LITERAL.test(value) ||
        (COLOR_PROP.test(prop) && COLOR_NAME.test(value))) &&
      !(prop === "color" && WHITE_TEXT.test(value))
    ) {
      bad("色はトークンで");
    }
    if (
      prop === "font-size" &&
      !/^var\(--fs-[a-z0-9]+\)$|^[\d.]+em$|^inherit$/.test(value)
    ) {
      bad("文字の大きさは --fs-* で（相対の em は可）");
    }
    if (
      prop === "font" &&
      /\d+(?:\.\d+)?px\s*(?:\/|\s)/.test(value.replace(/\/\s*\d+px/, ""))
    ) {
      bad("文字の大きさは --fs-* で");
    }
    if (
      (prop === "font-weight" && !/^(?:400|600|inherit|normal)$/.test(value)) ||
      (prop === "font" && /^(?!400\s|600\s)\d{3}\s/.test(value))
    ) {
      bad("太さは 400 と 600 だけ");
    }
    if (
      prop.match(/^border(?:-[a-z]+)*-radius$/) &&
      !value
        .split(/\s+/)
        .every((v) => /^var\(--r(?:-[a-z]+)?\)$|^0$|^999px$|^[\d.]+%$/.test(v))
    ) {
      bad("角丸は --r-sm / --r / --r-lg（丸は 50% か 999px）");
    }
    if (
      (prop === "box-shadow" && !/^none$|^inset\s/.test(value)) ||
      (prop === "text-shadow" && value !== "none") ||
      (prop === "filter" && /drop-shadow/.test(value))
    ) {
      bad("影は使わない（左の線の inset だけ可）");
    }
  }
  return found;
}

test("画面の CSS は、色・文字の大きさ・太さ・角丸・影をトークンから取る", () => {
  const decls = CSS_FILES.flatMap(cssDecls);
  // 読み取りが壊れて何も拾わなくなったら気づけるように、数を確かめる。
  assert.ok(decls.filter((d) => d.prop === "font-size").length > 200);
  assert.deepEqual(problems(decls), []);
});

test("CodeMirror のテーマと構文の色は、トークンから取る", () => {
  const highlight = scnHighlightStyle.specs.map((spec) =>
    Object.fromEntries(Object.entries(spec).filter(([k]) => k !== "tag")),
  );
  const decls = [
    ...themeDecls("editorThemeSpec", editorThemeSpec),
    ...themeDecls("writingThemeSpec", writingThemeSpec),
    ...themeDecls(
      "scnHighlightStyle",
      Object.fromEntries(
        highlight.map((style, i) => [
          String(i),
          style as Record<string, string>,
        ]),
      ),
    ),
  ];
  assert.deepEqual(problems(decls), []);
});

test("使っているトークンは、すべて tokens.css に定義がある", () => {
  const tokens = readFileSync(
    new URL("../src/styles/tokens.css", import.meta.url),
    "utf8",
  );
  const defined = new Set(
    [...tokens.matchAll(/(--[a-z0-9-]+)\s*:/g)].map((m) => m[1]),
  );
  // 画面が要素ごとに入れる変数（縦書きの字数と行数、入れ子の深さ）は除く。
  const local = new Set(["--v-chars", "--v-lines", "--scn-depth"]);
  const srcDir = new URL("../src/", import.meta.url);
  const scripts = readdirSync(srcDir, { recursive: true })
    .map(String)
    .filter((f) => /\.tsx?$/.test(f))
    .map((f) => readFileSync(new URL(f, srcDir), "utf8"));
  const sources = [
    ...CSS_FILES.map((f) =>
      readFileSync(new URL(`../src/styles/${f}`, import.meta.url), "utf8"),
    ),
    // 構文の色（packages/lezer-scn）。`tag` は JSON にできないので除く。
    JSON.stringify(
      scnHighlightStyle.specs.map((spec) =>
        Object.fromEntries(Object.entries(spec).filter(([k]) => k !== "tag")),
      ),
    ),
    // 画面のコード（テーマの指定と、実行時に読む `tokenColor("--…")` など）。
    ...scripts,
  ];
  const missing = new Set<string>();
  for (const src of sources) {
    for (const [, name = ""] of src.matchAll(
      /(?:var\(|["'`])(--[a-z][a-z0-9-]*)/g,
    )) {
      if (!defined.has(name) && !local.has(name)) missing.add(name);
    }
  }
  assert.deepEqual([...missing], []);
});

test("例外を見落とさない（直書きの色・px の文字・影は見つける）", () => {
  const decls: Decl[] = [
    { where: "x", prop: "background", value: "#f7f8fa" },
    { where: "x", prop: "font-size", value: "12.5px" },
    { where: "x", prop: "box-shadow", value: "0 12px 32px var(--scrim)" },
    { where: "x", prop: "font-weight", value: "700" },
    { where: "x", prop: "border-radius", value: "12px" },
    { where: "x", prop: "font", value: "600 14px/20px var(--font-ui)" },
    { where: "x", prop: "font", value: "700 var(--fs-base)/1 var(--font-ui)" },
    { where: "x", prop: "text-shadow", value: "0 1px 2px var(--scrim)" },
    {
      where: "x",
      prop: "filter",
      value: "drop-shadow(0 1px 2px var(--scrim))",
    },
    { where: "x", prop: "border-color", value: "red" },
  ];
  assert.equal(problems(decls).length, decls.length);
  const allowed: Decl[] = [
    { where: "x", prop: "color", value: "#ffffff" },
    { where: "x", prop: "box-shadow", value: "inset 3px 0 0 var(--ai-line)" },
    { where: "x", prop: "border-radius", value: "40% 40% 0 0" },
    {
      where: "x",
      prop: "font",
      value: "600 var(--fs-display)/30px var(--font-text)",
    },
    { where: "x", prop: "font-size", value: "0.85em" },
  ];
  assert.deepEqual(problems(allowed), []);
});
