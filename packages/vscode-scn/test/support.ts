// 文法のテストの共通部品：VS Code と同じ TextMate の処理系（vscode-textmate と Oniguruma）で、.scn の行を字句に
// 分け、行の種類（Lezer の文法の行の種類と同じ名前）を決める。

import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import oniguruma from "vscode-oniguruma";
import vsctm from "vscode-textmate";

const require = createRequire(import.meta.url);
const GRAMMAR = new URL("../syntaxes/scn.tmLanguage.json", import.meta.url);

/** 1つの字句：文字と、外から内への scope の並び。 */
export interface Token {
  readonly text: string;
  readonly scopes: readonly string[];
}

let grammarPromise: Promise<vsctm.IGrammar> | null = null;

async function grammar(): Promise<vsctm.IGrammar> {
  grammarPromise ??= (async () => {
    const wasm = readFileSync(
      require.resolve("vscode-oniguruma/release/onig.wasm"),
    );
    await oniguruma.loadWASM(
      wasm.buffer.slice(wasm.byteOffset, wasm.byteOffset + wasm.byteLength),
    );
    const registry = new vsctm.Registry({
      onigLib: Promise.resolve({
        createOnigScanner: (patterns: string[]) =>
          new oniguruma.OnigScanner(patterns),
        createOnigString: (s: string) => new oniguruma.OnigString(s),
      }),
      loadGrammar: async (scopeName: string) =>
        scopeName === "source.scn"
          ? vsctm.parseRawGrammar(
              readFileSync(GRAMMAR, "utf8"),
              GRAMMAR.pathname,
            )
          : null,
    });
    const loaded = await registry.loadGrammar("source.scn");
    if (loaded === null) throw new Error("文法を読めない");
    return loaded;
  })();
  return grammarPromise;
}

/** 本文を行ごとに字句に分ける（行をまたぐ状態も引き継ぐ）。 */
export async function tokenize(source: string): Promise<Token[][]> {
  const g = await grammar();
  let stack = vsctm.INITIAL;
  const out: Token[][] = [];
  for (const line of source.split("\n")) {
    const text = line.endsWith("\r") ? line.slice(0, -1) : line;
    const result = g.tokenizeLine(text, stack);
    stack = result.ruleStack;
    out.push(
      result.tokens.map((t, i) => ({
        text: text.slice(
          t.startIndex,
          result.tokens[i + 1]?.startIndex ?? text.length,
        ),
        scopes: t.scopes,
      })),
    );
  }
  return out;
}

/** scope から行の種類へ（内側の scope を先に見る）。 */
const KINDS: readonly [string, string][] = [
  ["comment.line.double-slash.scn", "Comment"],
  ["meta.raw.start.scn", "RawStart"],
  ["meta.raw.end.scn", "RawEnd"],
  ["meta.embedded.block.raw.scn", "RawLine"],
  ["markup.heading.chapter.scn", "ChapterHeading"],
  ["markup.heading.scene.scn", "SceneHeading"],
  ["punctuation.definition.meta.scn", "MetaDelimiter"],
  ["variable.other.meta-key.scn", "MetaEntry"],
  ["meta.directive.scn", "Directive"],
  ["meta.option.scn", "ChoiceOption"],
  ["meta.jump.scn", "Jump"],
  ["meta.dialogue.scn", "Dialogue"],
  ["meta.paragraph.narration.scn", "Narration"],
];

/** 行の種類：空白でない最初の字句の scope で決める（先頭の BOM は飛ばす）。 */
export function lineKind(tokens: readonly Token[]): string {
  const first = tokens.find((t) => t.text.replace(/^\uFEFF/, "").trim() !== "");
  if (first === undefined) return "Blank";
  for (const [scope, kind] of KINDS) {
    if (first.scopes.includes(scope)) return kind;
  }
  return "Unknown";
}

/** 本文のすべての行の種類。 */
export async function lineKinds(source: string): Promise<string[]> {
  const lines = await tokenize(source);
  return lines.map(lineKind);
}

/** その文字の字句の scope（最も内側）。 */
export function scopeOf(
  tokens: readonly Token[],
  text: string,
): string | undefined {
  return tokens.find((t) => t.text === text)?.scopes.at(-1);
}
