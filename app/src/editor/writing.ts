// 執筆表示（docs/ui/mockups/02-editor.html、D046）。記号を畳み、本文を読みやすく見せる。
//
// - 見た目だけを変え、本文の文字は変えない。行は1行ずつのまま、行ごとの飾りで見せる（行を消したり
//   まとめたりしない。どの行にもカーソルで入れる）。
// - ディレクティブはチップに、ルビは本文の上に描き、傍点・文中の命令・変数の埋め込みも畳む。
//   選択肢と `@if` のブロックは、行の背景と枠でカードのように見せる。
// - カーソルのある行は、記法をそのまま見せる。行IDは、どの行でも隠す（守り方は lineid.ts）。
//   ただし行IDに診断（E301・W303・E304）のある行は、気づけるように行IDを見せる（直すのはソース表示で。
//   執筆表示では行IDを守るので、書き換えられない。D047）。
// - 変換中は、飾りを作り直さず、前の飾りを変更に合わせてずらすだけにする（.claude/rules/frontend.md）。
//   変換が終わってから作り直す。
// - 飾りは Lezer の構文木から作る（見た目のため。意味の判断はコアに任せる）。見えている範囲だけを作る。

import { syntaxTree } from "@codemirror/language";
import { forEachDiagnostic, setDiagnosticsEffect } from "@codemirror/lint";
import {
  type ChangeDesc,
  type EditorState,
  type Extension,
  Facet,
  type Line,
  type Range,
  StateEffect,
} from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
  type ViewUpdate,
  WidgetType,
} from "@codemirror/view";
import type { SyntaxNode, Tree } from "@lezer/common";
import { prettyExpr } from "../core/expr.ts";
import { isComposing } from "./flags.ts";
import { lineIdGuard, lineIdOf } from "./lineid.ts";

/** シーンIDと題名（ジャンプの行に題名を添える）。 */
export const sceneTitles = Facet.define<
  ReadonlyMap<string, string>,
  ReadonlyMap<string, string>
>({
  combine: (values) => values[values.length - 1] ?? new Map(),
});

/** 変換が終わったので、飾りを作り直す。 */
export const refreshWriting = StateEffect.define<null>();

/** 文字の並び（チップや札の中身）。`className` があれば、その部分を包む。 */
interface Part {
  readonly text: string;
  readonly className?: string;
}

/** チップ、札、文中の命令など、文字を並べるだけの部品。 */
export class TagWidget extends WidgetType {
  readonly className: string;
  readonly parts: readonly Part[];
  /** シーンへ移る札なら、そのシーンID（Mod-クリックで移る。navigate.ts）。 */
  readonly sceneRef: string | null;

  constructor(
    className: string,
    parts: readonly Part[],
    sceneRef: string | null = null,
  ) {
    super();
    this.className = className;
    this.parts = parts;
    this.sceneRef = sceneRef;
  }

  eq(other: TagWidget): boolean {
    return (
      other.className === this.className &&
      other.sceneRef === this.sceneRef &&
      other.parts.length === this.parts.length &&
      other.parts.every(
        (p, i) =>
          p.text === this.parts[i]?.text &&
          p.className === this.parts[i]?.className,
      )
    );
  }

  toDOM(): HTMLElement {
    const span = document.createElement("span");
    span.className = this.className;
    if (this.sceneRef !== null) span.dataset.sceneRef = this.sceneRef;
    for (const part of this.parts) {
      if (part.className === undefined) {
        span.append(part.text);
      } else {
        const inner = document.createElement("span");
        inner.className = part.className;
        inner.textContent = part.text;
        span.append(inner);
      }
    }
    return span;
  }

  /** クリックはエディタに渡す（その行にカーソルが入り、記法が見える）。 */
  ignoreEvent(): boolean {
    return false;
  }
}

/** ルビ（親文字の上に読みを描く）。 */
export class RubyWidget extends WidgetType {
  readonly base: string;
  readonly reading: string;

  constructor(base: string, reading: string) {
    super();
    this.base = base;
    this.reading = reading;
  }

  eq(other: RubyWidget): boolean {
    return other.base === this.base && other.reading === this.reading;
  }

  toDOM(): HTMLElement {
    const ruby = document.createElement("ruby");
    ruby.className = "cm-scn-ruby";
    ruby.append(this.base);
    const rt = document.createElement("rt");
    rt.textContent = this.reading;
    ruby.append(rt);
    return ruby;
  }

  ignoreEvent(): boolean {
    return false;
  }
}

const hidden = Decoration.replace({});

function tag(className: string, ...parts: Part[]): Decoration {
  return Decoration.replace({ widget: new TagWidget(className, parts) });
}

/** メタ情報のキーと、状態の値の日本語（モックに合わせる。用語は docs/glossary.md）。 */
const META_LABELS: Readonly<Record<string, string>> = {
  summary: "要約",
  status: "状態",
  route: "ルート",
  tags: "タグ",
};
const STATUS_LABELS: Readonly<Record<string, string>> = {
  draft: "下書き",
  writing: "執筆中",
  done: "完成",
  "ai-draft": "AI下書き",
};

/** ブロック（選択肢、条件、生コード）。 */
interface Block {
  readonly kind: "choice" | "if" | "raw";
  options: number;
}

/** 行の頭にある、文書の直下の節（複数行にまたがる節なら、前の行から続いているもの）。 */
function topAt(tree: Tree, line: Line): SyntaxNode | null {
  const node = tree.topNode.childAfter(line.from);
  if (node === null || node.from > line.to) return null;
  return node;
}

/** 節の中で、この行で始まる子（`names` のどれか）。行末の改行まで含む子もある（`RawLine`、`RawEnd`）。 */
function childOnLine(
  node: SyntaxNode,
  line: Line,
  names: readonly string[],
): SyntaxNode | null {
  for (let c = node.firstChild; c !== null; c = c.nextSibling) {
    if (c.from >= line.from && c.from <= line.to && names.includes(c.name)) {
      return c;
    }
    if (c.from > line.to) break;
  }
  return null;
}

function directiveName(state: EditorState, node: SyntaxNode): string {
  const name = node.getChild("DirectiveName");
  return name === null ? "" : state.sliceDoc(name.from, name.to).trim();
}

/** 行の中身の終わり（行IDと行末の空白を除く）。 */
function contentEnd(state: EditorState, tree: Tree, line: Line): number {
  const id = lineIdOf(tree, line);
  const end = id?.from ?? line.to;
  const text = state.sliceDoc(line.from, end);
  return line.from + text.replace(/[ \t]+$/, "").length;
}

/** 行がどのシーンから始まるか（ブロックの入れ子を数えるため、シーンの見出しまで戻る）。 */
const SCAN_LIMIT = 5000;
function sceneStart(tree: Tree, state: EditorState, lineNo: number): number {
  let n = lineNo;
  for (let steps = 0; n > 1 && steps < SCAN_LIMIT; steps++) {
    const node = topAt(tree, state.doc.line(n));
    if (node?.name === "SceneHeading" && node.from >= state.doc.line(n).from) {
      return n;
    }
    n -= 1;
  }
  return n;
}

/** カーソル（選択）のある行。記法をそのまま見せる。 */
function rawLines(state: EditorState): Set<number> {
  const lines = new Set<number>();
  for (const r of state.selection.ranges) {
    const first = state.doc.lineAt(r.from).number;
    const last = state.doc.lineAt(r.to).number;
    for (let n = first; n <= last; n++) lines.add(n);
  }
  return lines;
}

interface Emit {
  readonly add: (from: number, to: number, deco: Decoration) => void;
}

/** 本文の中の記法（ルビ、傍点、文中の命令、変数の埋め込み、エスケープ、話者名）を畳む。 */
function inlineDecorations(
  state: EditorState,
  node: SyntaxNode,
  line: Line,
  out: Emit,
): void {
  const cursor = node.cursor();
  if (!cursor.firstChild()) return;
  do {
    if (cursor.from < line.from || cursor.to > line.to) continue;
    const text = state.sliceDoc(cursor.from, cursor.to);
    switch (cursor.name) {
      case "Ruby": {
        const base = cursor.node.getChild("RubyBase");
        const reading = cursor.node.getChild("RubyReading");
        if (base === null || reading === null) break;
        const r = state.sliceDoc(reading.from, reading.to);
        out.add(
          cursor.from,
          cursor.to,
          Decoration.replace({
            widget: new RubyWidget(
              state.sliceDoc(base.from, base.to),
              r.replace(/^《/, "").replace(/》$/, ""),
            ),
          }),
        );
        break;
      }
      case "EmphasisDots":
        out.add(
          cursor.from,
          cursor.to,
          tag("cm-scn-emphasis", {
            text: text.replace(/^《《/, "").replace(/》》$/, ""),
          }),
        );
        break;
      case "InlineCommand":
        out.add(
          cursor.from,
          cursor.to,
          tag("cm-scn-inline", {
            text: text.replace(/^\{/, "").replace(/\}$/, ""),
          }),
        );
        break;
      case "Interpolation":
        out.add(
          cursor.from,
          cursor.to,
          tag("cm-scn-inline cm-scn-interpolation", {
            text: text.replace(/^\{=/, "").replace(/\}$/, ""),
          }),
        );
        break;
      case "Escape":
        // 先頭の1文字（`\`、`{{` `}}` の片方、`｜《` の `｜`）だけを隠す。残りは文字として見せる。
        out.add(cursor.from, cursor.from + 1, hidden);
        break;
      case "Speaker":
        out.add(
          cursor.from,
          cursor.to,
          Decoration.mark({ class: "cm-scn-speaker" }),
        );
        break;
    }
  } while (cursor.nextSibling());
}

/** ディレクティブのチップ（`@` を外した名前と、` · ` でつないだ引数）。 */
function directiveChip(
  state: EditorState,
  node: SyntaxNode,
  name: string,
  end: number,
): Decoration {
  const nameNode = node.getChild("DirectiveName");
  const args = node.getChildren("Argument");
  const rest =
    args.length > 0
      ? args.map((a) => state.sliceDoc(a.from, a.to)).join(" · ")
      : state.sliceDoc(nameNode?.to ?? end, end).trim();
  return tag(
    "cm-scn-chip",
    { text: name.replace(/^@/, ""), className: "cm-scn-chip-label" },
    { text: rest },
  );
}

/** ジャンプの札（`→ s014 題名`）。 */
function jumpTag(state: EditorState, ref: string): Decoration {
  const title = state.facet(sceneTitles).get(ref);
  const parts: Part[] = [{ text: `→ ${ref}` }];
  if (title !== undefined) {
    parts.push({ text: ` ${title}`, className: "cm-scn-jump-title" });
  }
  return Decoration.replace({
    widget: new TagWidget("cm-scn-jump-tag", parts, ref === "" ? null : ref),
  });
}

/**
 * 執筆表示の飾り（`ranges` の行だけ）。カーソルのある行は記法をそのまま見せ、行IDだけを隠す。
 * テストから直接使う。
 */
export function writingDecorations(
  state: EditorState,
  ranges: readonly { readonly from: number; readonly to: number }[],
): DecorationSet {
  const tree = syntaxTree(state);
  const doc = state.doc;
  const raw = rawLines(state);
  const decorations: Range<Decoration>[] = [];
  const done = new Set<number>();
  // 行IDの診断（エディタの診断の文は「コード 文」の形。state.ts の toEditorDiagnostics）。
  const idDiagnostics: { from: number; to: number }[] = [];
  forEachDiagnostic(state, (d, from, to) => {
    if (/^(E301|W303|E304) /.test(d.message)) idDiagnostics.push({ from, to });
  });
  for (const range of ranges) {
    const first = doc.lineAt(range.from).number;
    const last = doc.lineAt(range.to).number;
    const stack: Block[] = [];
    for (let n = sceneStart(tree, state, first); n <= last; n++) {
      const line = doc.line(n);
      const emit = n >= first && !done.has(n);
      if (emit) done.add(n);
      const out: Emit = {
        add: (from, to, deco) => {
          decorations.push(deco.range(from, to));
        },
      };
      decorateLine(
        state,
        tree,
        line,
        raw.has(n),
        stack,
        emit ? out : null,
        idDiagnostics,
      );
    }
  }
  return Decoration.set(decorations, true);
}

/**
 * 1行の飾り。ブロックの入れ子（`stack`）を、この行の分だけ進める。`out` が null なら、入れ子を数えるだけ
 * （見えている範囲より前の行）。
 */
function decorateLine(
  state: EditorState,
  tree: Tree,
  line: Line,
  raw: boolean,
  stack: Block[],
  emitter: Emit | null,
  idDiagnostics: readonly { readonly from: number; readonly to: number }[],
): void {
  // 飾りは行の中に収める（改行をまたぐ置き換えは、ビュープラグインからは作れない）。
  const out: Emit = {
    add: (from, to, deco) => {
      if (emitter === null || from > line.to) return;
      emitter.add(from, Math.min(to, line.to), deco);
    },
  };
  const classes: string[] = [];
  const node = topAt(tree, line);
  const continued = node !== null && node.from < line.from;
  const end = contentEnd(state, tree, line);
  const inner = stack[stack.length - 1];
  const depth = stack.length;
  let lineDepth = depth;
  /** この行の後でブロックを開く・閉じる。 */
  let open: Block | null = null;
  let close = false;
  const rest: (() => void)[] = [];

  const kind = node?.name ?? "";
  if (kind === "SceneHeading" && !continued) stack.length = 0;
  switch (kind) {
    case "ChapterHeading": {
      classes.push("cm-scn-chapter");
      const mark = node?.getChild("ChapterMark");
      const title = node?.getChild("ChapterTitle");
      if (mark && title)
        rest.push(() => out.add(mark.from, title.from, hidden));
      break;
    }
    case "SceneHeading": {
      classes.push("cm-scn-scene");
      lineDepth = 0;
      const mark = node?.getChild("HeadingMark");
      const title = node?.getChild("SceneTitle");
      const id = node?.getChild("SceneId");
      const next = title ?? id;
      if (mark) rest.push(() => out.add(mark.from, next?.from ?? end, hidden));
      if (id) {
        const text = state.sliceDoc(id.from, id.to).replace(/^\s*#/, "");
        rest.push(() =>
          out.add(id.from, id.to, tag("cm-scn-scene-id", { text })),
        );
      }
      break;
    }
    case "SceneMeta": {
      lineDepth = 0;
      const entry =
        node && childOnLine(node, line, ["MetaEntry", "MetaDelimiter"]);
      if (entry?.name === "MetaDelimiter") {
        classes.push("cm-scn-meta-delim");
        rest.push(() => out.add(entry.from, entry.to, hidden));
      } else if (entry?.name === "MetaEntry") {
        classes.push("cm-scn-meta");
        const key = entry.getChild("MetaKey");
        const value = entry.getChild("MetaValue");
        const k = key ? state.sliceDoc(key.from, key.to).replace(/:$/, "") : "";
        let v = value ? state.sliceDoc(value.from, value.to) : "";
        if (k === "status") v = STATUS_LABELS[v] ?? v;
        if (k === "tags") v = v.replace(/^\[/, "").replace(/\]$/, "");
        const to = value?.to ?? key?.to ?? end;
        rest.push(() =>
          out.add(
            entry.from,
            to,
            tag(
              "cm-scn-meta-pill",
              { text: META_LABELS[k] ?? k, className: "cm-scn-meta-label" },
              { text: v },
            ),
          ),
        );
      } else {
        classes.push("cm-scn-meta");
      }
      break;
    }
    case "Comment":
      classes.push("cm-scn-comment");
      break;
    case "Directive": {
      const name = node ? directiveName(state, node) : "";
      const nameNode = node?.getChild("DirectiveName");
      switch (name) {
        case "@選択":
        case "@choice":
          classes.push("cm-scn-head", "cm-scn-head-choice");
          open = { kind: "choice", options: 0 };
          if (nameNode) {
            rest.push(() =>
              out.add(
                nameNode.from,
                nameNode.to,
                tag("cm-scn-badge cm-scn-badge-choice", { text: "選択" }),
              ),
            );
          }
          if (node) rest.push(() => inlineDecorations(state, node, line, out));
          break;
        case "@if":
        case "@elif":
        case "@else": {
          if (name === "@if") open = { kind: "if", options: 0 };
          else lineDepth = Math.max(0, depth - 1);
          classes.push(
            "cm-scn-head",
            name === "@if" ? "cm-scn-head-if" : "cm-scn-head-else",
          );
          const expr = state.sliceDoc(nameNode?.to ?? end, end);
          const badge =
            name === "@if" ? "分岐" : name === "@elif" ? "または" : "それ以外";
          const from = nameNode?.from ?? line.from;
          rest.push(() =>
            out.add(
              from,
              end,
              tag(
                "cm-scn-branch",
                {
                  text: badge,
                  className:
                    name === "@else"
                      ? "cm-scn-badge cm-scn-badge-else"
                      : "cm-scn-badge cm-scn-badge-if",
                },
                ...(expr.trim() === ""
                  ? []
                  : [{ text: prettyExpr(expr), className: "cm-scn-expr" }]),
              ),
            ),
          );
          break;
        }
        case "@end":
          classes.push("cm-scn-end");
          close = true;
          lineDepth = Math.max(0, depth - 1);
          if (nameNode) rest.push(() => out.add(nameNode.from, end, hidden));
          break;
        default:
          classes.push("cm-scn-directive");
          if (nameNode && node) {
            rest.push(() =>
              out.add(
                nameNode.from,
                end,
                directiveChip(state, node, name, end),
              ),
            );
          }
      }
      break;
    }
    case "RawBlock": {
      const start = node && childOnLine(node, line, ["RawStart"]);
      const rawEnd = node && childOnLine(node, line, ["RawEnd"]);
      if (start) {
        classes.push("cm-scn-head", "cm-scn-head-raw");
        open = { kind: "raw", options: 0 };
        const nameNode = start.getChild("DirectiveName");
        if (nameNode) {
          rest.push(() =>
            out.add(
              nameNode.from,
              end,
              directiveChip(state, start, "@raw", end),
            ),
          );
        }
      } else if (rawEnd) {
        classes.push("cm-scn-end");
        close = true;
        lineDepth = Math.max(0, depth - 1);
        rest.push(() => out.add(rawEnd.from, end, hidden));
      } else {
        classes.push("cm-scn-raw");
      }
      break;
    }
    case "ChoiceOption": {
      classes.push("cm-scn-option");
      if (inner?.kind === "choice") inner.options += 1;
      const number = inner?.kind === "choice" ? inner.options : 0;
      const optionNode = node;
      if (optionNode === null) break;
      const mark = optionNode.getChild("OptionMark");
      if (mark && number > 0) {
        rest.push(() =>
          out.add(
            mark.from,
            mark.to,
            tag("cm-scn-option-no", { text: String(number) }),
          ),
        );
      } else if (mark) {
        rest.push(() => out.add(mark.from, mark.to, hidden));
      }
      const ifKeyword = optionNode.getChild("IfKeyword");
      const arrow = optionNode.getChild("JumpArrow");
      const ref = optionNode.getChild("SceneRef");
      if (ifKeyword) {
        const condEnd = arrow?.from ?? end;
        const expr = state.sliceDoc(ifKeyword.to, condEnd);
        rest.push(() =>
          out.add(
            ifKeyword.from,
            condEnd,
            tag(
              "cm-scn-cond",
              { text: "条件", className: "cm-scn-cond-label" },
              { text: ` ${prettyExpr(expr)}` },
            ),
          ),
        );
      }
      if (arrow) {
        const refText = ref ? state.sliceDoc(ref.from, ref.to) : "";
        rest.push(() => out.add(arrow.from, end, jumpTag(state, refText)));
      }
      rest.push(() => inlineDecorations(state, optionNode, line, out));
      break;
    }
    case "Jump": {
      classes.push("cm-scn-jump");
      const ref = node?.getChild("SceneRef");
      const refText = ref ? state.sliceDoc(ref.from, ref.to) : "";
      if (node)
        rest.push(() => out.add(node.from, end, jumpTag(state, refText)));
      break;
    }
    case "Dialogue":
    case "Narration":
      classes.push("cm-scn-body");
      if (node) rest.push(() => inlineDecorations(state, node, line, out));
      break;
    default:
      break;
  }

  const block = stack[stack.length - 1];
  if (block !== undefined) classes.push(`cm-scn-in-${block.kind}`);
  else if (open !== null) classes.push(`cm-scn-in-${open.kind}`);
  if (close && block !== undefined) classes.push(`cm-scn-end-${block.kind}`);
  if (open !== null) classes.push(`cm-scn-start-${open.kind}`);
  if (emitter !== null) {
    out.add(
      line.from,
      line.from,
      Decoration.line(
        lineDepth > 0
          ? {
              class: classes.join(" "),
              attributes: { style: `--scn-depth: ${lineDepth}` },
            }
          : { class: classes.join(" ") },
      ),
    );
    // 行IDは、カーソルのある行でも隠す。行IDに診断があれば、見せる。
    const id = lineIdOf(tree, line);
    if (id !== null) {
      const flagged = idDiagnostics.some(
        (d) => d.from < id.to && d.to > id.from,
      );
      out.add(
        id.from,
        id.to,
        flagged ? Decoration.mark({ class: "cm-scn-line-id" }) : hidden,
      );
    }
  }

  if (!raw && emitter !== null) {
    // 行頭の半角空白とタブ（整形用）は見せない。字下げはカードの余白で見せる。
    const indent = /^[ \t]+/.exec(line.text)?.[0].length ?? 0;
    if (indent > 0 && !continued)
      out.add(line.from, line.from + indent, hidden);
    for (const f of rest) f();
  }

  if (open !== null) stack.push(open);
  if (close) stack.pop();
}

/**
 * 次の飾り。変換中は作り直さず、前の飾りを変更に合わせてずらすだけにする（変換中の行の畳み方や
 * 飾りを変えない）。テストから直接使う。
 */
export function nextDecorations(
  prev: DecorationSet,
  changes: ChangeDesc,
  composing: boolean,
  rebuild: boolean,
  build: () => DecorationSet,
): DecorationSet {
  if (composing) return prev.map(changes);
  return rebuild ? build() : prev;
}

const writingPlugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    /** 変換中に作り直しを見送った。 */
    stale = false;

    constructor(view: EditorView) {
      this.decorations = writingDecorations(view.state, view.visibleRanges);
    }

    update(update: ViewUpdate) {
      const composing = isComposing(update.view);
      const rebuild =
        this.stale ||
        update.docChanged ||
        update.selectionSet ||
        update.viewportChanged ||
        syntaxTree(update.startState) !== syntaxTree(update.state) ||
        update.startState.facet(sceneTitles) !==
          update.state.facet(sceneTitles) ||
        update.transactions.some((tr) =>
          tr.effects.some(
            (e) => e.is(refreshWriting) || e.is(setDiagnosticsEffect),
          ),
        );
      this.stale = composing && rebuild;
      this.decorations = nextDecorations(
        this.decorations,
        update.changes,
        composing,
        rebuild,
        () => writingDecorations(update.state, update.view.visibleRanges),
      );
    }
  },
  { decorations: (plugin) => plugin.decorations },
);

/**
 * 執筆表示の見た目（docs/ui/mockups/02-editor.html）。テストで中身を確かめるので、書き出す。
 * 行の種類ごとの指定は `&.cm-writing` を付けて詳細度をそろえ、後に書いたものが勝つようにする。
 * 行には margin を付けない（CodeMirror が行の高さを測るときにずれるため）。余白は padding で付ける。
 * 行の背景は、選択の色（行の後ろに描かれる）が透けるよう少し薄める。
 */
export const writingThemeSpec = {
  "&.cm-writing": { backgroundColor: "var(--paper)" },
  "&.cm-writing .cm-scroller": {
    fontFamily: "var(--font-ui)",
    fontSize: "13px",
    lineHeight: "1.6",
  },
  "&.cm-writing .cm-content": {
    maxWidth: "780px",
    margin: "0 auto",
    padding: "28px 40px 56px",
  },
  "&.cm-writing .cm-line": { padding: "3px 14px" },
  // 行の種類
  "&.cm-writing .cm-scn-body": {
    fontFamily: "var(--font-body)",
    fontSize: "17px",
    lineHeight: "1.95",
  },
  "&.cm-writing .cm-scn-chapter": {
    paddingTop: "12px",
    fontFamily: "var(--font-body)",
    fontSize: "15px",
    color: "var(--text-muted)",
  },
  "&.cm-writing .cm-scn-scene": {
    paddingTop: "18px",
    paddingBottom: "6px",
    fontFamily: "var(--font-body)",
    fontSize: "26px",
    fontWeight: "600",
    lineHeight: "1.5",
  },
  "&.cm-writing .cm-scn-meta": { fontSize: "12px" },
  "&.cm-writing .cm-scn-meta-delim": {
    fontSize: "10px",
    lineHeight: "1",
    color: "var(--line-number)",
  },
  "&.cm-writing .cm-scn-directive, &.cm-writing .cm-scn-head-raw": {
    paddingTop: "4px",
    paddingBottom: "4px",
  },
  "&.cm-writing .cm-scn-comment": {
    backgroundColor: "color-mix(in srgb, #f6f7f9 80%, transparent)",
    fontSize: "13px",
    color: "var(--text-faint)",
  },
  "&.cm-writing .cm-scn-head-choice, &.cm-writing .cm-scn-option": {
    fontFamily: "var(--font-body)",
    fontSize: "16px",
    lineHeight: "1.9",
  },
  "&.cm-writing .cm-scn-raw": {
    fontFamily: "var(--font-mono)",
    fontSize: "12.5px",
  },
  "&.cm-writing .cm-scn-end": { fontSize: "8px", lineHeight: "1" },
  // 選択肢と条件のカード（行の背景と枠）
  "&.cm-writing .cm-scn-in-choice": {
    paddingLeft: "calc(14px + var(--scn-depth, 0) * 18px)",
    borderLeft: "1px solid var(--accent-border)",
    borderRight: "1px solid var(--accent-border)",
    backgroundColor:
      "color-mix(in srgb, var(--accent-tint-2) 80%, transparent)",
  },
  "&.cm-writing .cm-scn-in-if, &.cm-writing .cm-scn-in-raw": {
    paddingLeft: "calc(14px + var(--scn-depth, 0) * 18px)",
    borderLeft: "1px solid var(--border)",
    borderRight: "1px solid var(--border)",
    backgroundColor: "color-mix(in srgb, var(--panel-bg-2) 80%, transparent)",
  },
  "&.cm-writing .cm-scn-start-choice": {
    paddingTop: "8px",
    borderTop: "1px solid var(--accent-border)",
    borderTopLeftRadius: "var(--radius-card)",
    borderTopRightRadius: "var(--radius-card)",
  },
  "&.cm-writing .cm-scn-start-if, &.cm-writing .cm-scn-start-raw": {
    paddingTop: "8px",
    borderTop: "1px solid var(--border)",
    borderTopLeftRadius: "var(--radius-card)",
    borderTopRightRadius: "var(--radius-card)",
  },
  "&.cm-writing .cm-scn-end-choice": {
    borderBottom: "1px solid var(--accent-border)",
    borderBottomLeftRadius: "var(--radius-card)",
    borderBottomRightRadius: "var(--radius-card)",
  },
  "&.cm-writing .cm-scn-end-if, &.cm-writing .cm-scn-end-raw": {
    borderBottom: "1px solid var(--border)",
    borderBottomLeftRadius: "var(--radius-card)",
    borderBottomRightRadius: "var(--radius-card)",
  },
  // 行の中の部品
  ".cm-scn-speaker": {
    fontFamily: "var(--font-ui)",
    fontSize: "15px",
    fontWeight: "700",
    color: "var(--accent)",
  },
  ".cm-scn-ruby rt": { fontSize: "10px" },
  ".cm-scn-line-id": {
    fontFamily: "var(--font-mono)",
    fontSize: "12px",
    color: "var(--line-number)",
  },
  ".cm-scn-emphasis": { textEmphasis: "filled sesame" },
  ".cm-scn-inline": {
    margin: "0 2px",
    padding: "0 4px",
    borderRadius: "4px",
    backgroundColor: "var(--accent-tint-2)",
    fontFamily: "var(--font-mono)",
    fontSize: "12px",
    color: "var(--accent)",
  },
  ".cm-scn-scene-id": {
    marginLeft: "12px",
    padding: "1px 8px",
    borderRadius: "4px",
    backgroundColor: "#f1f3f6",
    fontFamily: "var(--font-mono)",
    fontSize: "12px",
    fontWeight: "400",
    color: "var(--text-muted)",
    verticalAlign: "middle",
  },
  ".cm-scn-meta-pill": {
    padding: "2px 10px",
    borderRadius: "999px",
    backgroundColor: "#f1f3f6",
    color: "var(--text-2)",
  },
  ".cm-scn-meta-label": { marginRight: "6px", color: "var(--text-faint)" },
  ".cm-scn-chip": {
    display: "inline-flex",
    alignItems: "center",
    gap: "6px",
    padding: "2px 10px",
    border: "1px solid #dce0e6",
    borderRadius: "6px",
    backgroundColor: "#f1f3f6",
    fontSize: "12.5px",
    lineHeight: "1.5",
    color: "#2e3540",
  },
  ".cm-scn-chip-label": { color: "var(--text-faint)" },
  ".cm-scn-jump-tag": {
    fontFamily: "var(--font-ui)",
    fontSize: "13px",
    fontWeight: "500",
    color: "var(--accent-strong)",
  },
  ".cm-scn-jump-title": { fontWeight: "400" },
  ".cm-scn-badge": {
    marginRight: "10px",
    padding: "1px 8px",
    borderRadius: "4px",
    fontFamily: "var(--font-ui)",
    fontSize: "11.5px",
    fontWeight: "700",
    verticalAlign: "middle",
  },
  ".cm-scn-badge-choice": {
    backgroundColor: "var(--accent)",
    color: "#ffffff",
  },
  ".cm-scn-badge-if": {
    backgroundColor: "var(--text-muted)",
    color: "#ffffff",
  },
  ".cm-scn-badge-else": { backgroundColor: "#e6e9ee", color: "var(--text-2)" },
  ".cm-scn-expr": { fontFamily: "var(--font-mono)", fontSize: "13px" },
  ".cm-scn-option-no": {
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    width: "20px",
    height: "20px",
    marginRight: "8px",
    borderRadius: "50%",
    backgroundColor: "var(--accent-tint)",
    fontFamily: "var(--font-ui)",
    fontSize: "11.5px",
    fontWeight: "700",
    color: "var(--accent-strong)",
    verticalAlign: "middle",
  },
  ".cm-scn-cond": {
    marginLeft: "8px",
    padding: "1px 8px",
    border: "1px solid var(--accent-border)",
    borderRadius: "4px",
    backgroundColor: "#ffffff",
    fontFamily: "var(--font-mono)",
    fontSize: "12px",
    color: "var(--accent-strong)",
    verticalAlign: "middle",
  },
  ".cm-scn-cond-label": { fontFamily: "var(--font-ui)" },
};

/** 執筆表示の拡張（飾り、見た目、行IDを守る扱い）。 */
export function writingView(): Extension {
  return [
    writingPlugin,
    lineIdGuard("writing"),
    EditorView.theme(writingThemeSpec),
    EditorView.editorAttributes.of({ class: "cm-writing" }),
  ];
}
