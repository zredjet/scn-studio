// 入力補助（complete.ts、typing.ts、navigate.ts、T1-8、D047）のテスト。どの補助にも、変換中は動かないことの
// テストを付ける（計画書 T1-8 の完了条件）。

import assert from "node:assert/strict";
import { test } from "node:test";
import { completionStatus } from "@codemirror/autocomplete";
import { ensureSyntaxTree } from "@codemirror/language";
import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { scn } from "@scn-studio/lezer-scn";
import { unregisteredSpeaker } from "../src/check/group.ts";
import type { AssistData } from "../src/core/types.ts";
import {
  type AssistEnv,
  assistEnv,
  completeAt,
  completion,
  speakerInsertion,
  stubText,
  suggestSceneIds,
} from "../src/editor/complete.ts";
import { lineIdGuard } from "../src/editor/lineid.ts";
import { definitionAt } from "../src/editor/navigate.ts";
import {
  afterCommit,
  closingFor,
  deleteBracketPair,
  enterWithIndent,
  narrationIndent,
  normalizeInserted,
  type TypingOptions,
  typingAssist,
} from "../src/editor/typing.ts";

const BRACKETS = [
  { open: "「", close: "」" },
  { open: "『", close: "』" },
];

const DATA: AssistData = {
  characters: [
    { id: "akari", name: "灯", aliases: ["あかり", "灯ちゃん"] },
    { id: "minato", name: "湊", aliases: [] },
  ],
  backgrounds: ["屋上/夕方", "教室/昼"],
  expressions: { akari: ["笑顔", "真顔"] },
  bgm: ["切ない", "日常"],
  se: ["風", "チャイム"],
  variables: [
    { key: "akari.好感度", type: "int", values: [] },
    { key: "flag.勇気", type: "int", values: [] },
  ],
  directives: [
    { ja: "背景", en: "bg", args: "背景名 [トランジション]" },
    { ja: "立ち絵", en: "show", args: "キャラ 差分 [位置] [トランジション]" },
    { ja: "BGM", en: "bgm", args: "曲名、または 停止" },
    { ja: "設定", en: "set", args: "変数 演算子 式" },
    { ja: "呼出", en: "call", args: "シーンID" },
    { ja: "if", en: "if", args: "式（@end で閉じる）" },
    { ja: "end", en: "end", args: "" },
  ],
  positions: ["左", "中", "右"],
  bgm_stop: "停止",
};

const ENV: AssistEnv = {
  data: DATA,
  scenes: [
    { id: "s010", title: "登校" },
    { id: "s011", title: "昼休みの教室" },
    { id: "s013a", title: "打ち明ける" },
    { id: "_anon_chapter01_1", title: "IDの無い場面" },
  ],
  brackets: BRACKETS,
};

/** カーソルの前（行の頭から）だけの行で、補完の候補の名前を返す。 */
function labels(before: string, after = ""): string[] | null {
  const result = completeAt(ENV, 0, before, after, before.length);
  return result === null ? null : result.options.map((o) => o.label);
}

const OPTIONS: TypingOptions = {
  brackets: BRACKETS,
  editor: { normalize_ellipsis: true, indent_narration: true },
};

function state(doc: string, cursor = doc.length): EditorState {
  const s = EditorState.create({
    doc,
    selection: EditorSelection.cursor(cursor),
    extensions: [scn({ brackets: BRACKETS })],
  });
  ensureSyntaxTree(s, s.doc.length, 5000);
  return s;
}

/** 命令を試すための、エディタの代わり（変換中かどうかを選べる）。 */
function fakeView(s: EditorState, composing: boolean) {
  let result = s;
  const view = {
    state: s,
    composing,
    compositionStarted: composing,
    dispatch: (spec: Parameters<EditorState["update"]>[0]) => {
      result = s.update(spec).state;
    },
  };
  return { view: view as unknown as EditorView, after: () => result };
}

// ---- 補完 ----

test("補完：行頭のキャラ名（名前、ID、呼び名で引く）", () => {
  assert.deepEqual(labels("灯"), ["灯"]);
  assert.deepEqual(labels("あか"), ["灯"]);
  assert.deepEqual(labels("mi"), ["湊"]);
  // 後ろに文字があれば出さない（行の途中）。キャラ名でない語では出さない。
  assert.equal(labels("灯", "は"), null);
  assert.equal(labels("坂道"), null);
  // 選ぶと `名前「」` にし、カーソルを括弧の中へ置く。
  assert.deepEqual(speakerInsertion("灯", BRACKETS), {
    insert: "灯「」",
    cursor: 2,
  });
});

test("補完：@ と全角の ＠ で命令、命令の後ろで引数", () => {
  assert.deepEqual(labels("@背"), ["@背景"]);
  assert.deepEqual(labels("＠b"), ["@背景", "@BGM"]);
  assert.deepEqual(labels("@背景 屋"), ["屋上/夕方"]);
  assert.deepEqual(labels("@立ち絵 "), ["灯", "湊"]);
  assert.deepEqual(labels("@立ち絵 灯 "), ["笑顔", "真顔"]);
  assert.deepEqual(labels("@立ち絵 あかり 笑顔 "), ["左", "中", "右"]);
  assert.deepEqual(labels("@BGM "), ["切ない", "日常", "停止"]);
  assert.deepEqual(labels("@設定 "), ["灯.好感度", "勇気"]);
  assert.deepEqual(labels("@if 灯.好感度 >= 3 && 勇"), ["勇気"]);
  assert.deepEqual(labels("@呼出 s01"), ["s010", "s011", "s013a"]);
  // 選んだ命令は、全角の ＠ でも `@` にする。
  const result = completeAt(ENV, 0, "＠背", "", 2);
  assert.equal(result?.from, 0);
  assert.equal(result?.options[0]?.apply, "@背景 ");
});

test("補完：-> のシーンID。無いIDならスタブを作る候補", () => {
  assert.deepEqual(labels("-> s01"), ["s010", "s011", "s013a"]);
  // IDの無い見出しの仮のIDは、飛び先に出さない（言語サーバーの補完と同じ）。
  assert.ok(!(labels("-> _anon") ?? []).includes("_anon_chapter01_1"));
  assert.deepEqual(labels("@呼出 _"), null);
  assert.deepEqual(labels("- 行く -> s013"), ["s013a"]);
  const missing = completeAt(ENV, 0, "-> s099", "", 7);
  assert.deepEqual(
    missing?.options.map((o) => [o.label, o.detail]),
    [["s099", "新しいスタブを章の終わりに作る"]],
  );
  assert.equal(
    stubText("s099", true, "\n"),
    "\n## （未定） #s099\n---\nstatus: draft\n---\n",
  );
  assert.equal(
    stubText("s099", false, "\r\n"),
    "\r\n\r\n## （未定） #s099\r\n---\r\nstatus: draft\r\n---\r\n",
  );
});

test("補完：シーン見出しの ` #` の後ろで、空いているシーンID", () => {
  assert.deepEqual(suggestSceneIds(["s010", "s011", "s013a"], "s011"), [
    "s012",
  ]);
  assert.deepEqual(suggestSceneIds(["s010", "s011", "s012"], null), ["s013"]);
  assert.deepEqual(suggestSceneIds([], null), ["s001"]);
  assert.deepEqual(labels("## 新しいシーン #"), ["s012"]);
  assert.deepEqual(labels("## 新しいシーン #s01"), ["s012"]);
});

test("補完：文中の {se の後ろで効果音", () => {
  assert.deepEqual(labels("　ドアが{se チ"), ["チャイム"]);
});

test("補完の窓は、変換中は開かない（確定した後に開く）", () => {
  const s = state("灯");
  assert.equal(afterCommit(s, 0, 1, OPTIONS, true), null);
  assert.equal(afterCommit(s, 0, 1, OPTIONS, false)?.complete, true);
});

// ---- 括弧の補い ----

test("括弧の補い：確定した文字に閉じていない開き括弧があれば、閉じ括弧を足す", () => {
  assert.equal(closingFor("「", "", BRACKETS), "」");
  assert.equal(closingFor("「やあ", "", BRACKETS), "」");
  assert.equal(closingFor("『「", "", BRACKETS), "」』");
  assert.equal(closingFor("「やあ」", "", BRACKETS), "");
  // 後ろにもう閉じ括弧があれば足さない。
  assert.equal(closingFor("「", "」", BRACKETS), "");
  const s = state("灯「");
  const result = afterCommit(s, 1, 2, OPTIONS, false);
  const after = s.update(result?.spec ?? {}).state;
  assert.equal(after.sliceDoc(), "灯「」");
  assert.equal(after.selection.main.head, 2);
});

test("括弧の補い：閉じ括弧を打つと、補った閉じ括弧に重ねる", () => {
  // 「やあ」」の、カーソルの前の 」 が今打ったもの。
  const s = state("灯「やあ」」", 5);
  const result = afterCommit(s, 4, 5, OPTIONS, false);
  const after = s.update(result?.spec ?? {}).state;
  assert.equal(after.sliceDoc(), "灯「やあ」");
  assert.equal(after.selection.main.head, 5);
});

test("括弧の補い：変換中は何もしない", () => {
  assert.equal(afterCommit(state("灯「"), 1, 2, OPTIONS, true), null);
});

test("括弧の補い：空の括弧の間の Backspace は両方を消す。変換中は何もしない", () => {
  const s = state("灯「」", 2);
  const run = deleteBracketPair(() => OPTIONS);
  const done = fakeView(s, false);
  assert.ok(run(done.view));
  assert.equal(done.after().sliceDoc(), "灯");
  const composing = fakeView(s, true);
  assert.ok(!run(composing.view));
  assert.equal(composing.after().sliceDoc(), "灯「」");
});

// ---- 正規化 ----

test("正規化：... ・・・ 単独の … を ……、-- －－ を ―― にする", () => {
  assert.equal(normalizeInserted("待って...", ""), "待って……");
  assert.equal(normalizeInserted("・・・", ""), "……");
  assert.equal(normalizeInserted("…", ""), "……");
  assert.equal(normalizeInserted("……", ""), "……");
  assert.equal(normalizeInserted("それは--", ""), "それは――");
  assert.equal(normalizeInserted("－－", ""), "――");
  // 直前が … なら、続けて打っているので単独とみなさない。
  assert.equal(normalizeInserted("…", "…"), "…");
});

test("正規化：1文字ずつ打って、直前の文字と合わせて ... や -- になったら置き換える", () => {
  const cases: [string, string][] = [
    ["坂道の...", "坂道の……"],
    ["それは--", "それは――"],
    ["・・・", "……"],
  ];
  for (const [doc, expected] of cases) {
    const s = state(doc);
    const result = afterCommit(s, doc.length - 1, doc.length, OPTIONS, false);
    const after = s.update(result?.spec ?? {}).state;
    assert.equal(after.sliceDoc(), expected);
    assert.equal(after.selection.main.head, expected.length);
  }
  // 1つ目と2つ目の . では、まだ置き換えない。
  const two = state("坂道の..");
  assert.equal(afterCommit(two, 4, 5, OPTIONS, false)?.spec, null);
});

test("正規化：設定で切ってあれば何もしない。変換中は何もしない", () => {
  const s = state("待って...");
  const off = {
    ...OPTIONS,
    editor: { ...OPTIONS.editor, normalize_ellipsis: false },
  };
  assert.equal(afterCommit(s, 0, s.doc.length, off, false)?.spec, null);
  assert.equal(afterCommit(s, 0, s.doc.length, OPTIONS, true), null);
  const on = afterCommit(s, 0, s.doc.length, OPTIONS, false);
  assert.equal(s.update(on?.spec ?? {}).state.sliceDoc(), "待って……");
});

// ---- 段落頭の字下げ ----

test("字下げ：Enter で地の文の行を離れるとき、段落頭に全角空白を足す", () => {
  assert.deepEqual(narrationIndent(state("坂道の途中で"), 3), {
    from: 0,
    insert: "　",
  });
  // 台詞、もう全角空白がある行、`\` で始まる行、空の行には足さない。
  assert.equal(narrationIndent(state("灯「やあ」"), 2), null);
  assert.equal(narrationIndent(state("　坂道"), 2), null);
  assert.equal(narrationIndent(state("\\@地の文"), 2), null);
  assert.equal(narrationIndent(state(""), 0), null);
  const s = state("坂道の途中で");
  const enter = enterWithIndent(() => OPTIONS);
  const done = fakeView(s, false);
  assert.ok(enter(done.view));
  assert.equal(done.after().sliceDoc(), "　坂道の途中で\n");
  assert.equal(done.after().selection.main.head, done.after().doc.length);
});

test("字下げ：設定で切ってあれば何もしない。変換中は何もしない", () => {
  const s = state("坂道の途中で");
  const off = {
    ...OPTIONS,
    editor: { ...OPTIONS.editor, indent_narration: false },
  };
  assert.ok(!enterWithIndent(() => off)(fakeView(s, false).view));
  const composing = fakeView(s, true);
  assert.ok(!enterWithIndent(() => OPTIONS)(composing.view));
  assert.equal(composing.after().sliceDoc(), "坂道の途中で");
});

// ---- 定義へ移る ----

test("定義へ移る：-> のシーンID、選択肢の ->、@呼出 の引数、話者名", () => {
  const doc =
    "灯「やあ」\n-> s011\n@選択 q\n- 行く -> s013a\n@end\n@呼出 s010\n";
  const s = state(doc);
  assert.deepEqual(definitionAt(s, doc.indexOf("s011") + 1), {
    kind: "scene",
    id: "s011",
  });
  assert.deepEqual(definitionAt(s, doc.indexOf("s013a") + 2), {
    kind: "scene",
    id: "s013a",
  });
  assert.deepEqual(definitionAt(s, doc.indexOf("s010") + 1), {
    kind: "scene",
    id: "s010",
  });
  assert.deepEqual(definitionAt(s, 0), { kind: "character", name: "灯" });
  assert.equal(definitionAt(s, doc.indexOf("やあ") + 1), null);
});

// ---- レビューで直した点（T1-8） ----

test("正規化は本文だけ：メタ情報の --- 、@raw の中身、コメント、ディレクティブでは動かさない", () => {
  const cases: [string, number][] = [
    ["## a #s1\n---", "## a #s1\n---".length],
    ["## a #s1\n--", "## a #s1\n--".length],
    ["@raw lua\n-- x", "@raw lua\n--".length],
    ["// 待って...", "// 待って...".length],
    ["@背景 屋上...", "@背景 屋上...".length],
  ];
  for (const [doc, at] of cases) {
    const s = state(doc, at);
    const result = afterCommit(s, at - 1, at, OPTIONS, false);
    assert.equal(result?.spec, null, doc);
  }
  // 地の文、台詞、選択肢の表示文、@選択 の問いかけ文では動く。
  for (const doc of [
    "待って...",
    "灯「待って...",
    "- 待って...",
    "@選択 待って...",
  ]) {
    const s = state(doc);
    const result = afterCommit(s, doc.length - 1, doc.length, OPTIONS, false);
    assert.ok(result?.spec !== null, doc);
  }
});

test("括弧の補い：閉じ括弧ごと確定しても、閉じ括弧は2つにならない", () => {
  // 「灯「」」の括弧の間で「やあ」」を1回で確定した。
  const doc = "灯「やあ」」";
  const s = state(doc, 5);
  const result = afterCommit(s, 2, 5, OPTIONS, false);
  const after = s.update(result?.spec ?? {}).state;
  assert.equal(after.sliceDoc(), "灯「やあ」");
  assert.equal(after.selection.main.head, 5);
});

test("括弧の補い：文の途中（カーソルの後ろに文字がある）では補わない", () => {
  const s = state("「やっと来た", 1);
  assert.equal(afterCommit(s, 0, 1, OPTIONS, false)?.spec, null);
  // 後ろが行IDだけなら補う。
  const id = state("灯「 ^abcd", 2);
  const result = afterCommit(id, 1, 2, OPTIONS, false);
  assert.equal(id.update(result?.spec ?? {}).state.sliceDoc(), "灯「」 ^abcd");
});

test("字下げ：行の頭（中身より前）の Enter では足さない。行IDを守る扱いと一緒に動く", () => {
  assert.equal(narrationIndent(state("坂道の途中で ^abcdef"), 0), null);
  const doc = "坂道の途中で ^abcdef\n";
  const s = EditorState.create({
    doc,
    selection: EditorSelection.cursor("坂道の途中で".length),
    extensions: [scn({ brackets: BRACKETS }), lineIdGuard("writing")],
  });
  ensureSyntaxTree(s, s.doc.length, 5000);
  const done = fakeView(s, false);
  assert.ok(enterWithIndent(() => OPTIONS)(done.view));
  assert.equal(done.after().sliceDoc(), "　坂道の途中で ^abcdef\n\n");
});

test("補完の窓は、変換中の入力でも、ふつうの入力でも、自動では開かない（確定の後に開く）", () => {
  const s = EditorState.create({
    doc: "",
    extensions: [completion(), assistEnv.of(ENV)],
  });
  const composed = s.update({
    changes: { from: 0, insert: "灯" },
    userEvent: "input.type.compose",
  }).state;
  assert.equal(completionStatus(composed), null);
  const typed = s.update({
    changes: { from: 0, insert: "灯" },
    userEvent: "input.type",
  }).state;
  assert.equal(completionStatus(typed), null);
});

test("変換を使わずに打った文字の入力補助は、変換中は CodeMirror に任せる（何もしない）", () => {
  const s = EditorState.create({
    doc: "灯",
    extensions: [typingAssist(() => OPTIONS)],
  });
  const handlers = s.facet(EditorView.inputHandler);
  assert.ok(handlers.length > 0);
  const composing = fakeView(s, true);
  let inserted = false;
  for (const handler of handlers) {
    const handled = handler(composing.view, 1, 1, "「", () => {
      inserted = true;
      return s.update({ changes: { from: 1, insert: "「" } });
    });
    assert.equal(handled, false);
  }
  assert.equal(inserted, false);
  assert.equal(composing.after().sliceDoc(), "灯");
});

test("補完：1文字の前方一致ではキャラ名の候補を出さない（ちょうど同じなら出す）", () => {
  assert.equal(labels("あ"), null);
  assert.deepEqual(labels("灯"), ["灯"]);
  assert.deepEqual(labels("あか"), ["灯"]);
});

test("未登録の話者の登録は、設定DBに無い話者だけ（2人以上に当てはまるものは除く）", () => {
  const base = {
    code: "W202",
    severity: "warning" as const,
    file: "story/a.scn",
    span: [0, 3] as [number, number],
  };
  assert.equal(
    unregisteredSpeaker({
      ...base,
      message:
        "話者 店員 を設定DBのキャラクターに解決できません。地の文として扱います",
    }),
    "店員",
  );
  assert.equal(
    unregisteredSpeaker({
      ...base,
      message:
        "話者 先生 は2人以上のキャラクター（a、b）に当てはまります。地の文として扱います",
    }),
    null,
  );
});
