// .scn の外部の字句解析。行の種類を行頭で決め、文の中の記法、ディレクティブの引数、式を切り出す。
//
// 前の文字を読み返さず、今の位置から行の終わり（または必要な行まで）を先読みして決める。
// どの字句が来てよいかは、文法の状態（stack.canShift）で見分ける。
// 台詞の括弧の入れ子の深さだけは、ContextTracker（dialogueContext）で持つ。

import {
  ContextTracker,
  ExternalTokenizer,
  type InputStream,
  type Stack,
} from "@lezer/lr";
import * as T from "./parser.terms.ts";

/** 台詞に使える括弧（`project.toml` の `[dialogue.brackets]`）。 */
export interface DialogueBracket {
  readonly open: string;
  readonly close: string;
}

/** プロジェクトの外の既定（仕様書6節）。括弧は「」だけ。 */
export const DEFAULT_BRACKETS: readonly DialogueBracket[] = [
  { open: "「", close: "」" },
];

/** 台詞の中の文脈。台詞の外では null。 */
interface DialogueState {
  /** 開き括弧の文字コード。 */
  readonly open: number;
  /** 同じ種類の括弧の入れ子の深さ（開き括弧の直後は0）。 */
  readonly depth: number;
  /** 続きの行の先頭か（ContinueLine の直後）。 */
  readonly lineStart: boolean;
}

export const dialogueContext = new ContextTracker<DialogueState | null>({
  start: null,
  shift(context, term, _stack, input) {
    if (term === T.DialogueOpen) {
      return { open: input.next, depth: 0, lineStart: false };
    }
    if (context === null) return null;
    switch (term) {
      case T.NestedOpen:
        return {
          open: context.open,
          depth: context.depth + 1,
          lineStart: false,
        };
      case T.NestedClose:
        return {
          open: context.open,
          depth: Math.max(0, context.depth - 1),
          lineStart: false,
        };
      case T.ContinueLine:
        return { open: context.open, depth: context.depth, lineStart: true };
      case T.DialogueClose:
      case T.newline:
      case T.endOfFile:
        return null;
      default:
        return context.lineStart ? { ...context, lineStart: false } : context;
    }
  },
  hash: (context) =>
    context === null
      ? 0
      : context.open * 4096 +
        context.depth * 2 +
        (context.lineStart ? 1 : 0) +
        1,
  strict: true,
});

// ---- 文字の種類 ----

/** 行頭で読み飛ばし、行末で取り除く空白（半角空白とタブ）。 */
function isIndent(c: string | undefined): boolean {
  return c === " " || c === "\t";
}

/** 区切りとしての空白（半角空白、タブ、全角空白）。 */
function isSpace(c: string | undefined): boolean {
  return c === " " || c === "\t" || c === "\u3000";
}

/** 自動ルビの親文字になる文字（漢字と 々〆ヵヶ）。 */
function isKanji(code: number): boolean {
  return (
    (code >= 0x3400 && code <= 0x4dbf) ||
    (code >= 0x4e00 && code <= 0x9fff) ||
    (code >= 0xf900 && code <= 0xfaff) ||
    (code >= 0x20000 && code <= 0x3ffff) ||
    code === 0x3005 ||
    code === 0x3006 ||
    code === 0x30f5 ||
    code === 0x30f6
  );
}

const NAME_START = /[\p{L}_]/u;
const NAME_CONTINUE = /[\p{L}\p{N}_]/u;

// ---- 行の読み取り ----

interface LineText {
  /** 改行を除いた行の文字列（今の位置から）。 */
  readonly text: string;
  /** 改行の長さ（"\n" は1、"\r\n" は2、ファイルの終わりは0）。 */
  readonly newline: number;
}

/** `offset`（今の位置からの相対）から、行の終わりまでを読む。 */
function readLine(input: InputStream, offset: number): LineText {
  let text = "";
  let at = offset;
  for (;;) {
    const code = input.peek(at);
    if (code === -1) return { text, newline: 0 };
    if (code === 10) {
      if (text.endsWith("\r")) return { text: text.slice(0, -1), newline: 2 };
      return { text, newline: 1 };
    }
    text += String.fromCharCode(code);
    at += 1;
  }
}

/** 行の残りの形。位置は、今の位置からの相対。 */
interface Layout {
  /** 本文の終わり（行IDと行末の空白を除く）。 */
  readonly contentEnd: number;
  /** 行IDの前の空白の始まり。行IDが無ければ -1。 */
  readonly idStart: number;
  /** 行IDの終わり。 */
  readonly idEnd: number;
}

/** 行末の行IDを切り出す（仕様書9節）。`rest` は今の位置から行の終わりまで。 */
function layout(rest: string): Layout {
  const trimmed = rest.replace(/[ \t]+$/, "");
  const match = / +\^[0-9A-Za-z]+$/.exec(trimmed);
  if (match === null) {
    return { contentEnd: trimmed.length, idStart: -1, idEnd: -1 };
  }
  const content = trimmed.slice(0, match.index).replace(/[ \t]+$/, "");
  return {
    contentEnd: content.length,
    idStart: match.index,
    idEnd: trimmed.length,
  };
}

function leadingRun(
  text: string,
  test: (c: string | undefined) => boolean,
  limit: number,
): number {
  let n = 0;
  while (n < limit && test(text[n])) n += 1;
  return n;
}

/** 行IDを切り出し、行頭の空白を除いた本文（仕様書4節の順）。行の種類の判定に使う。 */
function lineContent(text: string): string {
  const content = text.slice(0, layout(text).contentEnd);
  return content.slice(leadingRun(content, isIndent, content.length));
}

// ---- 文の中の記法（仕様書8節） ----

/** `|` の後ろから、`親文字《るび》` が読めれば、その長さ（`|` を含まない）。 */
function explicitRubyLength(text: string, from: number, end: number): number {
  const open = text.indexOf("《", from);
  if (open <= from || open >= end) return -1;
  const base = text.slice(from, open);
  if (/[|｜》]/.test(base)) return -1;
  const close = text.indexOf("》", open + 1);
  if (close < 0 || close >= end || close === open + 1) return -1;
  if (text.slice(open + 1, close).includes("《")) return -1;
  return close + 1 - from;
}

/** `《` から始まるルビの読み `《るび》` の長さ。読めなければ -1。 */
function readingLength(text: string, from: number, end: number): number {
  if (text[from] !== "《") return -1;
  const close = text.indexOf("》", from + 1);
  if (close < 0 || close >= end || close === from + 1) return -1;
  if (text.slice(from + 1, close).includes("《")) return -1;
  return close + 1 - from;
}

/** `from` から始まる漢字の並びの長さ（コード単位）。 */
function kanjiRunLength(text: string, from: number, end: number): number {
  let at = from;
  while (at < end) {
    const code = text.codePointAt(at);
    if (code === undefined || !isKanji(code)) break;
    at += code > 0xffff ? 2 : 1;
  }
  return at - from;
}

/** 自動ルビの親文字（直後に `《るび》` が続く漢字の並び）が `from` から始まれば、その長さ。 */
function autoRubyBaseLength(text: string, from: number, end: number): number {
  const run = kanjiRunLength(text, from, end);
  if (run === 0) return -1;
  return readingLength(text, from + run, end) > 0 ? run : -1;
}

type Token = readonly [term: number, length: number];

/** `at` から記法の字句が始まれば、それを返す。 */
function markupAt(
  text: string,
  at: number,
  end: number,
  prevIsKanji: boolean,
): Token | null {
  const c = text[at];
  const next = text[at + 1];
  if (c === "{") {
    if (next === "{") return [T.Escape, 2];
    const close = text.indexOf("}", at + 1);
    if (close < 0 || close >= end) return null;
    return [next === "=" ? T.Interpolation : T.InlineCommand, close + 1 - at];
  }
  if (c === "}" && next === "}") return [T.Escape, 2];
  if (c === "|" || c === "｜") {
    if (next === "《") return [T.Escape, 2];
    return explicitRubyLength(text, at + 1, end) > 0 ? [T.RubyMark, 1] : null;
  }
  if (c === "《" && next === "《") {
    const close = text.indexOf("》》", at + 2);
    if (close > at + 2 && close + 2 <= end)
      return [T.EmphasisDots, close + 2 - at];
    return null;
  }
  if (!prevIsKanji) {
    const base = autoRubyBaseLength(text, at, end);
    if (base > 0) return [T.RubyBase, base];
  }
  return null;
}

/**
 * 文の中の字句を1つ読む。`end` は本文の終わり。`stops` の文字（台詞の括弧）の手前でも止まる。
 */
function inlineToken(
  text: string,
  end: number,
  stops: readonly string[],
): Token | null {
  if (end <= 0) return null;
  const markup = markupAt(text, 0, end, false);
  if (markup !== null) return markup;
  let at = 0;
  while (at < end) {
    const c = text[at];
    if (c !== undefined && stops.includes(c) && at > 0) break;
    if (at > 0) {
      const prev = text.codePointAt(at - 1);
      const prevIsKanji = prev !== undefined && isKanji(prev);
      if (markupAt(text, at, end, prevIsKanji) !== null) break;
    }
    const code = text.codePointAt(at);
    at += code !== undefined && code > 0xffff ? 2 : 1;
  }
  return [T.Text, Math.min(at, end)];
}

// ---- 式（仕様書14節） ----

function exprToken(text: string, end: number): Token | null {
  if (end <= 0) return null;
  const c = text[0];
  if (isSpace(c)) return [T.space, leadingRun(text, isSpace, end)];
  if (c === '"') {
    let at = 1;
    while (at < end) {
      if (text[at] === "\\" && text[at + 1] === '"') {
        at += 2;
        continue;
      }
      if (text[at] === '"') return [T.String, at + 1];
      at += 1;
    }
    return [T.String, end];
  }
  if (c !== undefined && c >= "0" && c <= "9") {
    return [
      T.Number,
      leadingRun(text, (d) => d !== undefined && d >= "0" && d <= "9", end),
    ];
  }
  const first = text.codePointAt(0);
  if (first !== undefined && NAME_START.test(String.fromCodePoint(first))) {
    let at = 0;
    let dots = 0;
    while (at < end) {
      const code = text.codePointAt(at);
      if (code === undefined) break;
      const ch = String.fromCodePoint(code);
      if (ch === "." && dots === 0 && at > 0) {
        dots += 1;
      } else if (!NAME_CONTINUE.test(ch)) {
        break;
      }
      at += ch.length;
    }
    const word = text.slice(0, at);
    if (word === "and" || word === "or" || word === "not")
      return [T.LogicOperator, at];
    if (word === "true" || word === "false") return [T.Boolean, at];
    return [T.VariableName, at];
  }
  const two = text.slice(0, 2);
  if (end >= 2) {
    if (two === "==" || two === "!=" || two === "<=" || two === ">=")
      return [T.CompareOperator, 2];
    if (two === "+=" || two === "-=") return [T.AssignOperator, 2];
  }
  if (c === "<" || c === ">") return [T.CompareOperator, 1];
  if (c === "=") return [T.AssignOperator, 1];
  if (c === "+" || c === "-" || c === "*" || c === "/")
    return [T.ArithmeticOperator, 1];
  if (c === "(" || c === ")") return [T.Paren, 1];
  return [T.InvalidChar, first !== undefined && first > 0xffff ? 2 : 1];
}

// ---- ディレクティブの引数（仕様書10節） ----

function argumentToken(text: string, end: number): Token | null {
  if (end <= 0) return null;
  if (isSpace(text[0])) return [T.space, leadingRun(text, isSpace, end)];
  if (text[0] === '"') {
    const close = text.indexOf('"', 1);
    return [T.Argument, close > 0 && close < end ? close + 1 : end];
  }
  return [
    T.Argument,
    leadingRun(text, (c) => c !== undefined && !isSpace(c), end),
  ];
}

// ---- 行の判定 ----

function directiveToken(name: string): number {
  switch (name) {
    case "選択":
    case "choice":
      return T.choiceKeyword;
    case "if":
    case "elif":
    case "設定":
    case "set":
      return T.exprKeyword;
    case "raw":
      return T.rawKeyword;
    default:
      return T.DirectiveName;
  }
}

/** 台詞候補の話者の長さ（仕様書6節「台詞の判定」の1）。当てはまらなければ -1。 */
function speakerLength(
  content: string,
  brackets: readonly DialogueBracket[],
): number {
  const isBracket = (c: string) =>
    c === "（" ||
    c === "）" ||
    brackets.some((b) => b.open === c || b.close === c);
  let count = 0;
  let at = 0;
  for (const ch of content) {
    if (isSpace(ch) || isBracket(ch)) break;
    count += 1;
    if (count > 20) return -1;
    at += ch.length;
  }
  if (count === 0 || at >= content.length) return -1;
  let open = at;
  if (content[open] === "（") {
    const close = content.indexOf("）", open + 1);
    if (close < 0) return -1;
    open = close + 1;
  }
  const opener = content[open];
  return brackets.some((b) => b.open === opener) ? at : -1;
}

/** 台詞を打ち切る行か（仕様書6節「複数行にまたがる台詞」）。 */
function endsDialogue(line: string): boolean {
  const content = lineContent(line);
  if (content.startsWith("\\")) return false;
  return (
    content === "" ||
    content.startsWith("## ") ||
    content.startsWith("@") ||
    content === "-" ||
    content.startsWith("- ") ||
    content.startsWith("->")
  );
}

/** 改行のところで、閉じていない台詞が次の行へ続くか。途中のコメントの行は飛ばす。 */
function dialogueContinues(input: InputStream, newlineLength: number): boolean {
  let offset = newlineLength;
  for (;;) {
    if (input.peek(offset) === -1) return false;
    const line = readLine(input, offset);
    const trimmed = line.text.slice(
      leadingRun(line.text, isIndent, line.text.length),
    );
    if (!trimmed.startsWith("//")) return !endsDialogue(line.text);
    if (line.newline === 0) return false;
    offset += line.text.length + line.newline;
  }
}

/** メタ情報が閉じているか。今の行から、次のシーン見出しより前に `---` の行があるかを見る。 */
function metaClosed(input: InputStream): boolean {
  let offset = 0;
  for (;;) {
    const line = readLine(input, offset);
    const content = lineContent(line.text);
    if (content === "---") return true;
    if (content.startsWith("## ")) return false;
    if (line.newline === 0) return false;
    offset += line.text.length + line.newline;
  }
}

/** メタ情報の `キー: 値` の形なら、`キー:` の長さ。 */
function metaKeyLength(content: string): number {
  const match = /^[^\s:\u3000]+:(?= |$)/.exec(content);
  return match === null ? -1 : match[0].length;
}

// ---- 字句解析の本体 ----

function tokenize(
  input: InputStream,
  stack: Stack,
  brackets: readonly DialogueBracket[],
): void {
  const accept = (token: Token | null) => {
    if (token !== null && token[1] > 0) input.acceptToken(token[0], token[1]);
  };
  if (input.next === -1) {
    if (stack.canShift(T.endOfFile)) input.acceptToken(T.endOfFile);
    return;
  }

  // @raw の中の行：解析せずに中身にする（仕様書15節）。
  if (stack.canShift(T.RawLine)) {
    const line = readLine(input, 0);
    if (line.text.replace(/^[ \t\u3000]+|[ \t\u3000]+$/g, "") === "@end") {
      accept([T.RawEnd, line.text.length + line.newline]);
      return;
    }
    if (!line.text.startsWith("## ")) {
      accept([T.RawLine, line.text.length + line.newline]);
      return;
    }
  }

  // メタ情報の行（仕様書5節）。閉じていないメタ情報は、`キー: 値` の形でない行で終わる。
  if (stack.canShift(T.MetaKey)) {
    const line = readLine(input, 0);
    const closed = metaClosed(input);
    const indent = leadingRun(line.text, isIndent, line.text.length);
    const whole = layout(line.text);
    if (whole.idStart >= 0 && whole.contentEnd === 0) {
      // 行IDだけの行は空行と同じ。閉じたメタ情報の中なら読み飛ばし（W303）、閉じていなければメタ情報を終える。
      if (closed) {
        accept([T.metaLineId, whole.idEnd]);
        return;
      }
    } else if (indent === line.text.length) {
      if (closed) {
        accept([T.MetaBlank, line.text.length + line.newline]);
        return;
      }
    } else if (indent > 0) {
      accept([T.space, indent]);
      return;
    } else if (line.text.startsWith("//")) {
      accept([T.metaComment, line.text.replace(/[ \t]+$/, "").length]);
      return;
    } else {
      const content = line.text.slice(0, layout(line.text).contentEnd);
      if (content === "---") {
        accept([T.MetaDelimiter, 3]);
        return;
      }
      const key = metaKeyLength(content);
      if (key > 0) {
        accept([T.MetaKey, key]);
        return;
      }
      if (closed) {
        accept([T.MetaInvalid, content.length]);
        return;
      }
    }
  }

  // 改行。閉じていない台詞なら、続きの行かどうかを決める。
  if (input.next === 10 || (input.next === 13 && input.peek(1) === 10)) {
    const length = input.next === 13 ? 2 : 1;
    if (stack.canShift(T.ContinueLine) && dialogueContinues(input, length)) {
      const next = readLine(input, length);
      accept([
        T.ContinueLine,
        length + leadingRun(next.text, isIndent, next.text.length),
      ]);
      return;
    }
    input.acceptToken(T.newline, length);
    return;
  }

  const line = readLine(input, 0);
  const rest = line.text;
  const shape = layout(rest);
  const context = stack.context as DialogueState | null;

  // 複数行の台詞の、続きの行の先頭。
  if (context?.lineStart) {
    if (rest.startsWith("//")) {
      accept([T.continueComment, rest.replace(/[ \t]+$/, "").length]);
      return;
    }
    if (rest.startsWith("\\")) {
      accept([T.Escape, 1]);
      return;
    }
  }

  // 行の始まり（仕様書4節）。
  if (stack.canShift(T.Comment)) {
    lineStart(input, stack, rest, shape, brackets, accept);
    return;
  }

  // ここからは行の途中。
  if (shape.idStart === 0 && stack.canShift(T.LineId)) {
    accept([T.LineId, shape.idEnd]);
    return;
  }
  if (
    rest.length > 0 &&
    leadingRun(rest, isIndent, rest.length) === rest.length
  ) {
    accept([T.space, rest.length]);
    return;
  }
  const end = shape.contentEnd;
  // 行IDの前の空白。本文の終わりから行IDまでは、区切りとして読み飛ばす。
  if (end === 0 && isIndent(rest[0])) {
    accept([
      T.space,
      leadingRun(
        rest,
        isIndent,
        shape.idStart > 0 ? shape.idStart : rest.length,
      ),
    ]);
    return;
  }

  if (stack.canShift(T.MetaValue)) {
    if (isIndent(rest[0])) accept([T.space, leadingRun(rest, isIndent, end)]);
    else accept([T.MetaValue, end]);
    return;
  }
  if (stack.canShift(T.ChapterTitle)) {
    if (isIndent(rest[0])) accept([T.space, leadingRun(rest, isIndent, end)]);
    else accept([T.ChapterTitle, end]);
    return;
  }
  if (stack.canShift(T.SceneId)) {
    headingToken(rest, end, accept);
    return;
  }
  if (stack.canShift(T.DialogueOpen)) {
    if (rest[0] === "（" && stack.canShift(T.DisplayName)) {
      const close = rest.indexOf("）");
      accept([T.DisplayName, close + 1]);
    } else {
      accept([T.DialogueOpen, 1]);
    }
    return;
  }
  if (stack.canShift(T.RubyReading)) {
    accept([T.RubyReading, Math.max(readingLength(rest, 0, end), 1)]);
    return;
  }
  if (stack.canShift(T.RubyBase) && !stack.canShift(T.Text)) {
    const open = rest.indexOf("《");
    accept([T.RubyBase, open > 0 ? open : 1]);
    return;
  }
  if (stack.canShift(T.IfKeyword)) {
    optionTextToken(rest, end, accept);
    return;
  }
  if (stack.canShift(T.optionArrow) && stack.canShift(T.VariableName)) {
    const arrow = rest.slice(0, end).lastIndexOf(" -> ");
    if (arrow === 0) accept([T.optionArrow, 3]);
    else accept(exprToken(rest, arrow > 0 ? arrow : end));
    return;
  }
  if (stack.canShift(T.SceneRef)) {
    if (isSpace(rest[0])) accept([T.space, leadingRun(rest, isSpace, end)]);
    else accept([T.SceneRef, end]);
    return;
  }
  if (stack.canShift(T.Argument)) {
    accept(argumentToken(rest, end));
    return;
  }
  if (stack.canShift(T.VariableName)) {
    accept(exprToken(rest, end));
    return;
  }
  if (
    context !== null &&
    (stack.canShift(T.DialogueClose) || stack.canShift(T.NestedOpen))
  ) {
    dialogueToken(rest, end, context, brackets, accept);
    return;
  }
  if (stack.canShift(T.Text)) {
    accept(inlineToken(rest, end, []));
    return;
  }
  if (isSpace(rest[0]))
    accept([T.space, leadingRun(rest, isSpace, rest.length)]);
}

type Accept = (token: Token | null) => void;

/** `{…}` の中身が、コアが読めるインライン命令か（コアの `inline::command`。読めなければ W405 で文字になる）。 */
function isInlineCommand(inner: string): boolean {
  const [name, ...args] = inner.split(/[ \t\u3000]/).filter((w) => w !== "");
  if (name === "p") return args.length === 0;
  const [arg] = args;
  if (args.length !== 1 || arg === undefined) return false;
  if (name === "w") return /^[0-9]+(\.[0-9]+)?$/.test(arg);
  if (name === "se") return true;
  if (name === "speed") return arg === "遅" || arg === "普通" || arg === "速";
  return false;
}

/**
 * インライン命令（`{w}`・`{p}`・`{se}`・`{speed}`）と空白だけか。閉じ括弧の後ろがこれだけなら、
 * 後ろに文は続かない（仕様書6節、D041）。
 */
function isCommandsOnly(text: string): boolean {
  let at = 0;
  while (at < text.length) {
    if (isSpace(text[at])) {
      at += 1;
      continue;
    }
    if (text[at] !== "{" || text[at + 1] === "{") return false;
    const close = text.indexOf("}", at + 1);
    if (close < 0 || !isInlineCommand(text.slice(at + 1, close))) return false;
    at = close + 1;
  }
  return true;
}

/**
 * 台詞候補の閉じ括弧の後ろに文が続くか（仕様書6節：続くなら、話者を解決できても地の文）。
 * 括弧の対応は生の文字で取り、閉じていなければ、コアと同じ規則で次の行以降を読む。
 */
function closesWithTrailingText(
  input: InputStream,
  content: string,
  speaker: number,
  brackets: readonly DialogueBracket[],
): boolean {
  let open = speaker;
  if (content[open] === "（") open = content.indexOf("）", open) + 1;
  const opener = content[open];
  const bracket = brackets.find((b) => b.open === opener);
  if (bracket === undefined) return false;
  let depth = 1;
  /** 本文の中で閉じ括弧を探す。見つかれば、その後ろに文があるか。 */
  const scan = (text: string, from: number): boolean | null => {
    for (let at = from; at < text.length; at++) {
      const c = text[at];
      if (c === bracket.close) {
        depth -= 1;
        if (depth === 0) return !isCommandsOnly(text.slice(at + 1));
      } else if (c === bracket.open) {
        depth += 1;
      }
    }
    return null;
  };
  const first = scan(content, open + 1);
  if (first !== null) return first;
  let offset = 0;
  for (;;) {
    const line = readLine(input, offset);
    if (line.newline === 0) return false;
    offset += line.text.length + line.newline;
    if (input.peek(offset) === -1) return false;
    const next = readLine(input, offset);
    const indent = leadingRun(next.text, isIndent, next.text.length);
    if (next.text.slice(indent).startsWith("//")) continue;
    if (endsDialogue(next.text)) return false;
    const body = lineContent(next.text);
    const found = scan(body.startsWith("\\") ? body.slice(1) : body, 0);
    if (found !== null) return found;
  }
}

/** 行の始まり。行頭の空白を読み飛ばしてから、行の種類を決める。 */
function lineStart(
  input: InputStream,
  stack: Stack,
  rest: string,
  shape: Layout,
  brackets: readonly DialogueBracket[],
  accept: Accept,
): void {
  const indent = leadingRun(
    rest,
    (c) => isIndent(c) || c === "\uFEFF",
    rest.length,
  );
  if (indent > 0) {
    // 行IDだけの行（` ^abcd`）は、空白ごと行IDにする。
    if (shape.idStart >= 0 && shape.contentEnd === 0)
      accept([T.LineId, shape.idEnd]);
    else accept([T.space, indent]);
    return;
  }
  if (rest.startsWith("//")) {
    accept([T.Comment, rest.replace(/[ \t]+$/, "").length]);
    return;
  }
  const content = rest.slice(0, shape.contentEnd);
  if (content === "" && shape.idStart >= 0) {
    accept([T.LineId, shape.idEnd]);
    return;
  }
  if (content.startsWith("\\")) {
    accept([T.Escape, 1]);
    return;
  }
  if (content.startsWith("# ")) {
    accept([T.ChapterMark, 1]);
    return;
  }
  if (content.startsWith("## ")) {
    accept([T.HeadingMark, 2]);
    return;
  }
  if (content === "---" && stack.canShift(T.MetaDelimiter)) {
    accept([T.MetaDelimiter, 3]);
    return;
  }
  if (content.startsWith("@")) {
    const nameEnd =
      1 +
      leadingRun(
        content.slice(1),
        (c) => c !== undefined && !isSpace(c),
        content.length - 1,
      );
    const term = directiveToken(content.slice(1, nameEnd));
    // @選択 の問いかけ文は文として読むので、前の区切りの空白を命令名に含める。
    const separator =
      term === T.choiceKeyword
        ? leadingRun(content.slice(nameEnd), isSpace, content.length - nameEnd)
        : 0;
    accept([term, nameEnd + separator]);
    return;
  }
  if (content === "-" || content.startsWith("- ")) {
    accept([T.OptionMark, 1]);
    return;
  }
  if (content.startsWith("->")) {
    accept([T.JumpArrow, 2]);
    return;
  }
  const speaker = speakerLength(content, brackets);
  if (
    speaker > 0 &&
    !closesWithTrailingText(input, content, speaker, brackets)
  ) {
    accept([T.Speaker, speaker]);
    return;
  }
  accept(inlineToken(rest, shape.contentEnd, []));
}

/** シーン見出しの `##` の後ろ。最後の ` #` より前がタイトル、` #` からがシーンID（仕様書5節）。 */
function headingToken(rest: string, end: number, accept: Accept): void {
  const hash = rest.slice(0, end).lastIndexOf(" #");
  if (hash === 0) {
    accept([T.SceneId, end]);
    return;
  }
  const indent = leadingRun(rest, isIndent, hash > 0 ? hash : end);
  if (indent > 0) {
    accept([T.space, indent]);
    return;
  }
  const titleEnd = rest
    .slice(0, hash > 0 ? hash : end)
    .replace(/[ \t]+$/, "").length;
  if (titleEnd > 0) {
    accept([T.SceneTitle, titleEnd]);
    return;
  }
  accept([T.space, leadingRun(rest, isIndent, hash > 0 ? hash : end)]);
}

/** 選択肢の行の表示文。右から最後の ` -> ` と、その前の最後の ` if ` を切り出す（仕様書11節）。 */
function optionTextToken(rest: string, end: number, accept: Accept): void {
  const arrow = rest.slice(0, end).lastIndexOf(" -> ");
  const condition = rest.slice(0, arrow >= 0 ? arrow : end).lastIndexOf(" if ");
  if (condition === 0) {
    accept([T.IfKeyword, 3]);
    return;
  }
  if (arrow === 0) {
    accept([T.optionArrow, 3]);
    return;
  }
  const textEnd = condition > 0 ? condition : arrow > 0 ? arrow : end;
  if (isIndent(rest[0])) {
    accept([T.space, leadingRun(rest, isIndent, textEnd)]);
    return;
  }
  accept(inlineToken(rest, textEnd, []));
}

/** 台詞の括弧の中。同じ種類の括弧は入れ子として数える（仕様書6節）。 */
function dialogueToken(
  rest: string,
  end: number,
  context: DialogueState,
  brackets: readonly DialogueBracket[],
  accept: Accept,
): void {
  const open = String.fromCharCode(context.open);
  const close = brackets.find((b) => b.open === open)?.close ?? "";
  const c = rest[0];
  if (end > 0 && c === open) {
    accept([T.NestedOpen, 1]);
    return;
  }
  if (end > 0 && c === close) {
    accept([context.depth > 0 ? T.NestedClose : T.DialogueClose, 1]);
    return;
  }
  // 括弧の対応は、記法より先に生の文字で取る（コアと同じ）。記法の字句が括弧をまたがないよう、
  // 次の括弧の文字の手前で切る。
  let limit = end;
  for (const bracket of [open, close]) {
    const at = rest.indexOf(bracket, 1);
    if (at > 0 && at < limit) limit = at;
  }
  accept(inlineToken(rest, limit, []));
}

/** 台詞の括弧を指定して、字句解析を作る。 */
export function createTokenizer(
  brackets: readonly DialogueBracket[],
): ExternalTokenizer {
  return new ExternalTokenizer(
    (input, stack) => tokenize(input, stack, brackets),
    {
      contextual: true,
    },
  );
}

/** 既定の括弧（「」だけ）の字句解析。文法に組み込む。 */
export const scnTokens = createTokenizer(DEFAULT_BRACKETS);
