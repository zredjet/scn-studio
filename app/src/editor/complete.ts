// 補完（T1-8、D047）。@codemirror/autocomplete の窓を使い、候補はここで作る。
//
// - 打つたびに自動では出さない（`activateOnTyping: false`）。変換を確定した後と、変換を使わずに打った後に、
//   typing.ts の afterCommit が窓を開く。変換中は開かず、変換が始まったら閉じる（.claude/rules/frontend.md）。
// - 候補の材料は、コアの completion_data（キャラクター、素材の意味名、変数、命令）と、シーンの一覧。
// - 文脈は、カーソルより前の行の文字で決める（見た目のための判断。意味の判断はコアに任せる）。
//   - 行頭のキャラ名 → `名前「」` に展開し、カーソルを括弧の中へ
//   - `@`（全角の `＠` も）→ 命令。命令の後ろ → 引数（背景名、キャラ、差分、位置、曲名、効果音、シーンID、変数）
//   - `->` → シーンID。無いIDなら、今の章の終わりにスタブのシーンを作る候補も出す
//   - シーン見出しの ` #` の後ろ → 空いているシーンID
//   - 文中の `{se ` → 効果音

import {
  autocompletion,
  type Completion,
  type CompletionContext,
  type CompletionResult,
} from "@codemirror/autocomplete";
import { type EditorState, type Extension, Facet } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import type { AssistData, Bracket } from "../core/types.ts";

/** 補完に要るもの。エディタが入れ物（Compartment）で入れ替える。 */
export interface AssistEnv {
  readonly data: AssistData | null;
  /** シーンIDと題名（全体）。 */
  readonly scenes: readonly { readonly id: string; readonly title: string }[];
  readonly brackets: readonly Bracket[];
}

export const assistEnv = Facet.define<AssistEnv, AssistEnv>({
  combine: (values) =>
    values[values.length - 1] ?? { data: null, scenes: [], brackets: [] },
});

/** シーンIDの形（仕様書5節）。 */
const SCENE_ID = /^[A-Za-z0-9_]+$/;

/** 補完の窓に出す1つ。`apply` が関数なら、選んだときの書き換えを自分で行う。 */
type Option = Completion;

function option(
  label: string,
  detail?: string,
  apply?: Completion["apply"],
): Option {
  return {
    label,
    ...(detail === undefined ? {} : { detail }),
    ...(apply === undefined ? {} : { apply }),
  };
}

/** 前方一致で絞る（大文字と小文字は区別しない）。 */
function matches(candidate: string, typed: string): boolean {
  return candidate.toLowerCase().startsWith(typed.toLowerCase());
}

/** 台詞の括弧の最初の組（`project.toml` の `[dialogue.brackets]`。無ければ「」）。 */
function firstBracket(brackets: readonly Bracket[]): Bracket {
  return brackets[0] ?? { open: "「", close: "」" };
}

/** キャラ名を `名前「」` にするときの、入れる文字列と、カーソルの位置（入れた文字列の頭から）。 */
export function speakerInsertion(
  name: string,
  brackets: readonly Bracket[],
): { insert: string; cursor: number } {
  const b = firstBracket(brackets);
  return {
    insert: `${name}${b.open}${b.close}`,
    cursor: name.length + b.open.length,
  };
}

/**
 * 空いているシーンIDの提案。`base`（直前の見出しのIDなど）の頭の文字と桁数をまねて、使われていない
 * 次の番号を返す。`base` が無いか番号で終わらなければ、全体で最も大きい `s` と数字のIDの次（無ければ s001）。
 */
export function suggestSceneIds(
  used: readonly string[],
  base: string | null,
): string[] {
  const taken = new Set(used);
  const pattern = /^([A-Za-z_]*?)(\d+)$/;
  const next = (prefix: string, from: number, width: number): string => {
    let n = from;
    for (;;) {
      const id = `${prefix}${String(n).padStart(width, "0")}`;
      if (!taken.has(id)) return id;
      n += 1;
    }
  };
  const result: string[] = [];
  const m = base === null ? null : pattern.exec(base);
  if (m !== null) {
    const prefix = m[1] ?? "";
    const digits = m[2] ?? "0";
    result.push(next(prefix, Number(digits) + 1, digits.length));
  }
  const numbered = used
    .map((id) => /^s(\d+)$/.exec(id))
    .filter((x): x is RegExpExecArray => x !== null);
  const max = numbered.reduce((a, x) => Math.max(a, Number(x[1])), 0);
  const width = numbered.reduce((a, x) => Math.max(a, (x[1] ?? "").length), 3);
  const fallback = next("s", max + 1, width);
  if (!result.includes(fallback)) result.push(fallback);
  return result;
}

/** 新しいスタブのシーン（今の章の終わりに足す。D047）。`endsWithNewline` は文書が改行で終わるか。 */
export function stubText(
  id: string,
  endsWithNewline: boolean,
  lineBreak: string,
): string {
  const lines = ["", `## （未定） #${id}`, "---", "status: draft", "---", ""];
  return (endsWithNewline ? "" : lineBreak) + lines.join(lineBreak);
}

/** 変数の候補の書き方：`flag.` は省き、キャラクターの名前空間は名前で書く（本文での書き方）。 */
function variableLabel(key: string, data: AssistData): string {
  if (key.startsWith("flag.")) return key.slice("flag.".length);
  const dot = key.indexOf(".");
  if (dot < 0) return key;
  const ns = key.slice(0, dot);
  const character = data.characters.find((c) => c.id === ns);
  return character === undefined ? key : `${character.name}${key.slice(dot)}`;
}

/** キャラクターを、ID・名前・呼び名のどれかで引く（話者の解決と同じ順。仕様書6節）。 */
function findCharacter(data: AssistData, text: string) {
  return (
    data.characters.find((c) => c.id === text) ??
    data.characters.find((c) => c.name === text) ??
    data.characters.find((c) => c.aliases.includes(text))
  );
}

/** 命令を、日本語名か英語名で引く。 */
function findDirective(data: AssistData, name: string) {
  return data.directives.find((d) => d.ja === name || d.en === name);
}

/**
 * カーソルより前の行の文字（`before`）と後ろ（`after`）から、補完の候補を作る。`lineFrom` は行の頭、
 * `pos` はカーソル。候補が無ければ null。テストから直接使う。
 */
export function completeAt(
  env: AssistEnv,
  lineFrom: number,
  before: string,
  after: string,
  pos: number,
): CompletionResult | null {
  const data = env.data;
  const restEmpty = after.replace(/ +\^[0-9A-Za-z]+[ \t]*$/, "").trim() === "";
  const result = (from: number, options: Option[]): CompletionResult | null =>
    options.length === 0 ? null : { from, options, filter: false };

  // シーン見出しの ` #` の後ろ：空いているシーンID。
  const heading = /^[ \t]*## .* #([A-Za-z0-9_]*)$/.exec(before);
  if (heading !== null) {
    if (!restEmpty) return null;
    const typed = heading[1] ?? "";
    const used = env.scenes.map((s) => s.id);
    const base = typed === "" ? null : typed;
    const ids = suggestSceneIds(used, base).filter((id) => matches(id, typed));
    return result(
      pos - typed.length,
      ids.map((id) => option(id, "空いているシーンID")),
    );
  }

  // `->` の後ろ：シーンID。
  const jump = /(?:^[ \t]*|[ \t])->[ \t]*([A-Za-z0-9_]*)$/.exec(before);
  if (jump !== null) {
    const typed = jump[1] ?? "";
    const options = env.scenes
      .filter((s) => matches(s.id, typed))
      .map((s) => option(s.id, s.title));
    // 打ったIDで始まるシーンが1つも無いときだけ、スタブを作る候補を出す（途中まで打った段階では出さない）。
    if (typed !== "" && SCENE_ID.test(typed) && options.length === 0) {
      options.push(
        option(
          typed,
          "新しいスタブを章の終わりに作る",
          (view, _c, from, to) => {
            const state = view.state;
            const end = state.doc.length;
            const last = end === 0 ? "" : state.sliceDoc(end - 1);
            view.dispatch({
              changes: [
                { from, to, insert: typed },
                {
                  from: end,
                  insert: stubText(
                    typed,
                    end === 0 || last === "\n" || last === "\r",
                    state.lineBreak,
                  ),
                },
              ],
              userEvent: "input.complete",
            });
          },
        ),
      );
    }
    return result(pos - typed.length, options);
  }

  if (data === null) return null;

  // 文中の `{se ` の後ろ：効果音。
  const se = /\{se[ \t]+([^}\s]*)$/.exec(before);
  if (se !== null) {
    const typed = se[1] ?? "";
    return result(
      pos - typed.length,
      data.se.filter((n) => matches(n, typed)).map((n) => option(n, "効果音")),
    );
  }

  // `@` の後ろ：命令の名前。全角の `＠` も受け、選んだら `@` にする。
  const name = /^([ \t]*)([@＠])([^\s@＠]*)$/.exec(before);
  if (name !== null) {
    const typed = name[3] ?? "";
    const from = lineFrom + (name[1] ?? "").length;
    const options = data.directives
      .filter((d) => matches(d.ja, typed) || matches(d.en, typed))
      .map((d) =>
        option(
          `@${d.ja}`,
          d.ja === d.en ? d.args : `${d.en}　${d.args}`,
          `@${d.ja}${d.args === "" ? "" : " "}`,
        ),
      );
    return result(from, options);
  }

  // 命令の引数。
  const directive = /^[ \t]*[@＠](\S+)[ 　\t]+(.*)$/.exec(before);
  if (directive !== null) {
    const info = findDirective(data, directive[1] ?? "");
    if (info === undefined) return null;
    const args = (directive[2] ?? "").split(/[ 　\t]+/);
    const typed = args.pop() ?? "";
    const from = pos - typed.length;
    const pick = (names: readonly string[], detail: string) =>
      result(
        from,
        names.filter((n) => matches(n, typed)).map((n) => option(n, detail)),
      );
    const characters = data.characters.map((c) => c.name);
    const variables = () =>
      data.variables.map((v) => variableLabel(v.key, data));
    switch (info.en) {
      case "bg":
        return args.length === 0 ? pick(data.backgrounds, "背景") : null;
      case "show":
        if (args.length === 0) return pick(characters, "キャラ");
        if (args.length === 1) {
          const character = findCharacter(data, args[0] ?? "");
          return character === undefined
            ? null
            : pick(
                data.expressions[character.id] ?? [],
                `${character.name}の差分`,
              );
        }
        return args.length === 2 ? pick(data.positions, "位置") : null;
      case "hide":
        return args.length === 0 ? pick(characters, "キャラ") : null;
      case "bgm":
        return args.length === 0
          ? pick([...data.bgm, data.bgm_stop], "曲名")
          : null;
      case "se":
        return args.length === 0 ? pick(data.se, "効果音") : null;
      case "call":
        return args.length === 0
          ? pick(
              env.scenes.map((s) => s.id),
              "シーンID",
            )
          : null;
      case "set":
        return args.length === 0 ? pick(variables(), "変数") : null;
      case "if":
      case "elif": {
        // 式の中の、今打っている名前。
        const word =
          /([^\s!=<>()&|+\-*/]*)$/.exec(directive[2] ?? "")?.[1] ?? "";
        return result(
          pos - word.length,
          variables()
            .filter((n) => matches(n, word))
            .map((n) => option(n, "変数")),
        );
      }
      default:
        return null;
    }
  }

  // 行頭のキャラ名：`名前「」` に展開する。行の後ろに文字が無いときだけ。
  const speaker = /^([ \t]*)([^\s@＠#/{}\\\-「」『』（）()〈〉<>]+)$/.exec(
    before,
  );
  if (speaker !== null && restEmpty) {
    const typed = speaker[2] ?? "";
    const from = lineFrom + (speaker[1] ?? "").length;
    // 1文字の前方一致では出さない（短い地の文を確定した直後の Enter が、候補に取られないように）。
    // 名前・呼び名・IDとちょうど同じなら、1文字でも出す。
    const hits = (candidate: string) =>
      candidate === typed || (typed.length >= 2 && matches(candidate, typed));
    const options = data.characters
      .map((c) => {
        const alias = c.aliases.find(hits);
        const hit = hits(c.name) || hits(c.id) || alias !== undefined;
        return hit ? { c, alias } : null;
      })
      .filter((x) => x !== null)
      .map(({ c, alias }) =>
        option(
          c.name,
          alias === undefined ? c.id : `${alias}（${c.id}）`,
          (view: EditorView, _c: Completion, f: number, t: number) => {
            const { insert, cursor } = speakerInsertion(c.name, env.brackets);
            view.dispatch({
              changes: { from: f, to: t, insert },
              selection: { anchor: f + cursor },
              userEvent: "input.complete",
            });
          },
        ),
      );
    return result(from, options);
  }
  return null;
}

/** 補完の候補を作る（CodeMirror の補完の源）。 */
export function scnCompletions(
  context: CompletionContext,
): CompletionResult | null {
  const state: EditorState = context.state;
  const env = state.facet(assistEnv);
  const line = state.doc.lineAt(context.pos);
  const before = line.text.slice(0, context.pos - line.from);
  const after = line.text.slice(context.pos - line.from);
  return completeAt(env, line.from, before, after, context.pos);
}

/** 補完の拡張。打つたびには出さない（typing.ts の afterCommit が開く）。 */
export function completion(): Extension {
  return autocompletion({
    override: [scnCompletions],
    activateOnTyping: false,
    // 命令を選んだら、続けて引数の候補を出す。
    activateOnCompletion: (c) => c.label.startsWith("@"),
    icons: false,
    closeOnBlur: true,
  });
}
