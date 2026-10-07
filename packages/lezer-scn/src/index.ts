// .scn のハイライト用の言語サポート（CodeMirror 6）。
//
// Lezerは見た目のためだけに使う。台詞かどうか、参照が正しいかなどの意味の判断は、
// コア（scn-core）の解析結果に任せる（.claude/rules/frontend.md）。

import {
  HighlightStyle,
  LanguageSupport,
  LRLanguage,
  syntaxHighlighting,
} from "@codemirror/language";
import type { Extension } from "@codemirror/state";
import { tags as t } from "@lezer/highlight";
import { parser } from "./parser.ts";
import {
  createTokenizer,
  DEFAULT_BRACKETS,
  type DialogueBracket,
  scnTokens,
} from "./tokens.ts";

export { parser } from "./parser.ts";
export { DEFAULT_BRACKETS, type DialogueBracket } from "./tokens.ts";

/** 既定の括弧（「」だけ）の言語。 */
export const scnLanguage = LRLanguage.define({
  name: "scn",
  parser,
  languageData: { commentTokens: { line: "//" } },
});

export interface ScnOptions {
  /** 台詞に使える括弧（`project.toml` の `[dialogue.brackets]`）。省くと「」だけ。 */
  readonly brackets?: readonly DialogueBracket[];
}

/** 括弧を指定した構文解析器。 */
export function configureParser(brackets: readonly DialogueBracket[]) {
  if (brackets === DEFAULT_BRACKETS) return parser;
  return parser.configure({
    tokenizers: [{ from: scnTokens, to: createTokenizer(brackets) }],
  });
}

/** .scn の言語サポート。 */
export function scn(options: ScnOptions = {}): LanguageSupport {
  const brackets = options.brackets ?? DEFAULT_BRACKETS;
  if (brackets === DEFAULT_BRACKETS) return new LanguageSupport(scnLanguage);
  const language = LRLanguage.define({
    name: "scn",
    parser: configureParser(brackets),
    languageData: { commentTokens: { line: "//" } },
  });
  return new LanguageSupport(language);
}

/**
 * 色の対応。画面モック（docs/ui/mockups/02b-editor-source.html）に合わせ、
 * docs/ui/README.md のデザイントークン（CSS変数）を使う。引数、メタ情報の値、
 * 式（変数・値・演算子）、見出しのタイトルは、モックと同じく本文の色のままにする。
 * 緑青（--ai）はAIに関わるものだけに使う（docs/ui/README.md「色の意味」）。
 */
export const scnHighlightStyle = HighlightStyle.define([
  {
    tag: [
      t.processingInstruction,
      t.labelName,
      t.keyword,
      t.list,
      t.controlKeyword,
      t.controlOperator,
    ],
    color: "var(--accent)",
    fontWeight: "500",
  },
  {
    tag: [t.propertyName, t.annotation, t.macroName, t.special(t.string)],
    color: "var(--accent)",
  },
  { tag: [t.lineComment, t.docComment, t.meta], color: "var(--line-number)" },
  { tag: t.special(t.variableName), fontWeight: "700" },
  { tag: t.emphasis, textEmphasis: "filled sesame" },
  { tag: t.monospace, color: "var(--text-2)" },
  { tag: t.invalid, textDecoration: "underline wavy var(--warn)" },
]);

/** 色の対応をエディタに入れる拡張。 */
export const scnSyntaxHighlighting: Extension =
  syntaxHighlighting(scnHighlightStyle);
