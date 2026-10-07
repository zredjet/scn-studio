// 執筆表示（writing.ts）と表示の切り替えのテスト。CodeMirrorの状態を使う（画面は出さない）。

import assert from "node:assert/strict";
import { test } from "node:test";
import { ensureSyntaxTree } from "@codemirror/language";
import { Compartment, EditorSelection, EditorState } from "@codemirror/state";
import type { DecorationSet } from "@codemirror/view";
import { scn } from "@scn-studio/lezer-scn";
import {
  createState,
  editorThemeSpec,
  modeExtension,
} from "../src/editor/state.ts";
import {
  nextDecorations,
  RubyWidget,
  sceneTitles,
  TagWidget,
  writingDecorations,
} from "../src/editor/writing.ts";

const BRACKETS = [{ open: "「", close: "」" }];
const HOOKS = {
  changed: () => {},
  save: () => {},
  compositionStarted: () => {},
  compositionEnded: () => {},
  toggleView: () => {},
};

const DOC = [
  "## 屋上 #s012",
  "---",
  "status: writing",
  "---",
  "@背景 屋上/夕方 フェード ^d001",
  "灯「｜湊《みなと》、{w 0.5}来て」 ^a001",
  "@選択 どうする ^a002",
  "- 行く if 勇気 >= 2 -> s014 ^a003",
  "  @設定 勇気 += 1",
  "@end",
  "",
].join("\n");

function state(doc: string, cursor: number): EditorState {
  const s = EditorState.create({
    doc,
    selection: EditorSelection.cursor(cursor),
    extensions: [scn(), sceneTitles.of(new Map([["s014", "沈黙のあと"]]))],
  });
  ensureSyntaxTree(s, s.doc.length, 5000);
  return s;
}

interface Seen {
  readonly line: number;
  readonly from: number;
  readonly to: number;
  /** 行の飾りの class、部品の class（"ruby" はルビ）、隠すだけなら "hidden"、印なら class。 */
  readonly kind: string;
  readonly text: string;
}

function seen(s: EditorState, set: DecorationSet): Seen[] {
  const out: Seen[] = [];
  set.between(0, s.doc.length, (from, to, deco) => {
    const spec = deco.spec as {
      widget?: unknown;
      class?: string;
    };
    const line = s.doc.lineAt(from).number;
    if (spec.widget instanceof TagWidget) {
      out.push({
        line,
        from,
        to,
        kind: spec.widget.className,
        text: spec.widget.parts.map((p) => p.text).join(""),
      });
    } else if (spec.widget instanceof RubyWidget) {
      out.push({
        line,
        from,
        to,
        kind: "ruby",
        text: `${spec.widget.base}/${spec.widget.reading}`,
      });
    } else if (from === to) {
      out.push({ line, from, to, kind: `line ${spec.class ?? ""}`, text: "" });
    } else {
      out.push({
        line,
        from,
        to,
        kind: spec.class ?? "hidden",
        text: s.sliceDoc(from, to),
      });
    }
  });
  return out;
}

function onLine(list: readonly Seen[], line: number): Seen[] {
  return list.filter((d) => d.line === line);
}

test("執筆表示とソース表示を切り替えても、本文は変わらない", () => {
  const mode = new Compartment();
  const titles = new Compartment();
  for (const doc of [DOC, DOC.replaceAll("\n", "\r\n")]) {
    const writing = createState(doc, BRACKETS, new Compartment(), HOOKS, {
      mode: "writing",
      modeCompartment: mode,
      titles: new Map(),
      titlesCompartment: titles,
    });
    assert.equal(writing.sliceDoc(), doc);
    const source = writing.update({
      effects: mode.reconfigure(modeExtension("source")),
    }).state;
    assert.equal(source.sliceDoc(), doc);
    const back = source.update({
      effects: mode.reconfigure(modeExtension("writing")),
    }).state;
    assert.equal(back.sliceDoc(), doc);
    // 飾りを作っても、本文は変わらない。
    writingDecorations(back, [{ from: 0, to: back.doc.length }]);
    assert.equal(back.sliceDoc(), doc);
  }
});

test("執筆表示：行IDを隠し、ディレクティブをチップに、ルビを本文の上に描く", () => {
  const s = state(DOC, DOC.length);
  const list = seen(s, writingDecorations(s, [{ from: 0, to: s.doc.length }]));
  // 見出し：`## ` を隠し、シーンIDは札。
  assert.deepEqual(
    onLine(list, 1).map((d) => [d.kind, d.text]),
    [
      ["line cm-scn-scene", ""],
      ["hidden", "## "],
      ["cm-scn-scene-id", "s012"],
    ],
  );
  // メタ情報：`---` を隠し、値は日本語の札。
  assert.deepEqual(
    onLine(list, 2).map((d) => d.kind),
    ["line cm-scn-meta-delim", "hidden"],
  );
  assert.deepEqual(
    onLine(list, 3).map((d) => [d.kind, d.text]),
    [
      ["line cm-scn-meta", ""],
      ["cm-scn-meta-pill", "状態執筆中"],
    ],
  );
  // ディレクティブ：`@` を外した名前と引数のチップ。行IDは隠す。
  assert.deepEqual(
    onLine(list, 5).map((d) => [d.kind, d.text]),
    [
      ["line cm-scn-directive", ""],
      ["cm-scn-chip", "背景屋上/夕方 · フェード"],
      ["hidden", " ^d001"],
    ],
  );
  // 台詞：話者名、ルビ、文中の命令。
  assert.deepEqual(
    onLine(list, 6).map((d) => [d.kind, d.text]),
    [
      ["line cm-scn-body", ""],
      ["cm-scn-speaker", "灯"],
      ["ruby", "湊/みなと"],
      ["cm-scn-inline", "w 0.5"],
      ["hidden", " ^a001"],
    ],
  );
  // 選択肢：カードの頭、番号、条件、ジャンプ（題名を添える）。
  assert.deepEqual(
    onLine(list, 7).map((d) => [d.kind, d.text]),
    [
      [
        "line cm-scn-head cm-scn-head-choice cm-scn-in-choice cm-scn-start-choice",
        "",
      ],
      ["cm-scn-badge cm-scn-badge-choice", "選択"],
      ["hidden", " ^a002"],
    ],
  );
  assert.deepEqual(
    onLine(list, 8).map((d) => [d.kind, d.text]),
    [
      ["line cm-scn-option cm-scn-in-choice", ""],
      ["cm-scn-option-no", "1"],
      ["cm-scn-cond", "条件 勇気 ≥ 2"],
      ["cm-scn-jump-tag", "→ s014 沈黙のあと"],
      ["hidden", " ^a003"],
    ],
  );
  // ブロックの中の字下げ（半角空白）は隠し、カードの余白で見せる。
  assert.deepEqual(
    onLine(list, 9).map((d) => [d.kind, d.text]),
    [
      ["line cm-scn-directive cm-scn-in-choice", ""],
      ["hidden", "  "],
      ["cm-scn-chip", "設定勇気 += 1"],
    ],
  );
  assert.deepEqual(
    onLine(list, 10).map((d) => [d.kind, d.text]),
    [
      ["line cm-scn-end cm-scn-in-choice cm-scn-end-choice", ""],
      ["hidden", "@end"],
    ],
  );
});

test("ブロックが閉じたら、後ろの行はカードに入れない（@raw の @end は改行まで節に含まれる）", () => {
  const doc = [
    "## a #s1",
    "@raw renpy",
    '  $ renpy.music.set_volume(0.6, channel="music")',
    "@end",
    "灯「やあ」 ^a001",
    "",
  ].join("\n");
  const s = state(doc, doc.length);
  const list = seen(s, writingDecorations(s, [{ from: 0, to: s.doc.length }]));
  assert.deepEqual(
    onLine(list, 4).map((d) => [d.kind, d.text]),
    [
      ["line cm-scn-end cm-scn-in-raw cm-scn-end-raw", ""],
      ["hidden", "@end"],
    ],
  );
  assert.equal(onLine(list, 5)[0]?.kind, "line cm-scn-body");
});

test("エスケープは先頭の1文字だけを隠す（`{{` は `{`、`｜《` は `《` と見せる）", () => {
  const doc = "地の文{{笑}}と｜《と\n\\@で始まる地の文\n\n";
  const s = state(doc, doc.length);
  const list = seen(s, writingDecorations(s, [{ from: 0, to: s.doc.length }]));
  const hiddenOn = (line: number) =>
    onLine(list, line)
      .filter((d) => d.kind === "hidden")
      .map((d) => d.text);
  assert.deepEqual(hiddenOn(1), ["{", "}", "｜"]);
  // 行頭の `\` も隠す。
  assert.deepEqual(hiddenOn(2), ["\\"]);
});

test("カーソルのある行は記法をそのまま見せる。行IDだけは隠す", () => {
  const line6 = DOC.indexOf("灯「");
  const s = state(DOC, line6 + 1);
  const list = seen(s, writingDecorations(s, [{ from: 0, to: s.doc.length }]));
  assert.deepEqual(
    onLine(list, 6).map((d) => [d.kind, d.text]),
    [
      ["line cm-scn-body", ""],
      ["hidden", " ^a001"],
    ],
  );
  // ほかの行は畳んだまま。
  assert.ok(onLine(list, 5).some((d) => d.kind === "cm-scn-chip"));
});

test("変換中は、飾りを作り直さない（変換中の行と、その後の行の畳み方が変わらない）", () => {
  // 1行目の「灯」の後ろで変換し、「「やあ」を入れる。台詞が閉じないので、作り直すと2行目は
  // 1行目の台詞の続きになり、話者名の飾りが消える。変換中は、前の飾りをずらすだけにする。
  const doc = "灯\n紬「うん」 ^b001\n";
  const s = state(doc, 1);
  const prev = writingDecorations(s, [{ from: 0, to: s.doc.length }]);
  const tr = s.update({
    changes: { from: 1, insert: "「やあ" },
    selection: { anchor: 4 },
    userEvent: "input.type.compose",
  });
  ensureSyntaxTree(tr.state, tr.state.doc.length, 5000);
  const full = [{ from: 0, to: tr.state.doc.length }];
  const rebuilt = seen(tr.state, writingDecorations(tr.state, full));
  assert.ok(
    !onLine(rebuilt, 2).some((d) => d.kind === "cm-scn-speaker"),
    "作り直すと、2行目の話者名の飾りは消える（このテストの前提）",
  );
  const during = nextDecorations(prev, tr.changes, true, true, () =>
    writingDecorations(tr.state, full),
  );
  const kept = seen(tr.state, during);
  // 変換中：前の飾りを、変更に合わせてずらしただけ。
  assert.deepEqual(kept, seen(tr.state, prev.map(tr.changes)));
  assert.ok(onLine(kept, 2).some((d) => d.kind === "cm-scn-speaker"));
  assert.deepEqual(
    onLine(kept, 1).map((d) => d.kind),
    onLine(seen(s, prev), 1).map((d) => d.kind),
  );
  // 変換が終わったら作り直す。
  const after = nextDecorations(during, tr.changes, false, true, () =>
    writingDecorations(tr.state, full),
  );
  assert.deepEqual(seen(tr.state, after), rebuilt);
});

test("フォーカス中の選択の色は、CodeMirror の基本テーマより詳細度が高い", () => {
  const key = Object.keys(editorThemeSpec).find((k) =>
    k.includes(".cm-selectionBackground"),
  );
  assert.ok(key?.includes("&.cm-editor.cm-focused > .cm-scroller"));
});
