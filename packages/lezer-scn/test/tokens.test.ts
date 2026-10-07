// 字句の切り出し（行ID、文中の記法、ディレクティブの引数、式、選択肢、見出し）と、
// ハイライトの対応、括弧の設定、差分の再解析を確かめる。

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { TreeFragment } from "@lezer/common";
import { getStyleTags, highlightTree, tags as t } from "@lezer/highlight";
import {
  configureParser,
  parser,
  scn,
  scnHighlightStyle,
} from "../src/index.ts";
import {
  errors,
  lineKinds,
  parse,
  SAMPLE_BRACKETS,
  tokens,
} from "./support.ts";

/** 1行を解析し、その行の字句を返す。 */
function lex(line: string): string[] {
  const tree = parse(`${line}\n`, SAMPLE_BRACKETS);
  assert.deepEqual(errors(tree), [], line);
  return tokens(`${line}\n`, tree);
}

test("行ID：4文字と12文字、行末の空白、形の崩れた候補", () => {
  assert.deepEqual(lex("　本文。 ^abcd"), ["Text:　本文。", "LineId: ^abcd"]);
  assert.deepEqual(lex("　本文。  ^abcdef012345 \t"), [
    "Text:　本文。",
    "LineId:  ^abcdef012345",
  ]);
  assert.deepEqual(lex("　大文字。 ^K3F9"), [
    "Text:　大文字。",
    "LineId: ^K3F9",
  ]);
  assert.deepEqual(lex("　顔文字 ^_^"), ["Text:　顔文字 ^_^"]);
  assert.deepEqual(lex("　空白の無い^abcd"), ["Text:　空白の無い^abcd"]);
  assert.deepEqual(lex(" ^abcd"), ["LineId: ^abcd"]);
});

test("コメントの行からは行IDを切り出さない", () => {
  assert.deepEqual(lex("  // 灯「やあ」 ^k3f9"), [
    "Comment:// 灯「やあ」 ^k3f9",
  ]);
});

test("文中の記法：ルビ、傍点、インライン命令、変数の埋め込み、エスケープ", () => {
  assert.deepEqual(lex("　髪が|乱《みだ》れ、｜灯《あかり》と漢字《かんじ》"), [
    "Text:　髪が",
    "RubyMark:|",
    "RubyBase:乱",
    "RubyReading:《みだ》",
    "Text:れ、",
    "RubyMark:｜",
    "RubyBase:灯",
    "RubyReading:《あかり》",
    "Text:と",
    "RubyBase:漢字",
    "RubyReading:《かんじ》",
  ]);
  assert.deepEqual(
    lex("　《《強調》》と{w 0.5}{p}{se ドア}{speed 遅}{=灯.好感度}"),
    [
      "Text:　",
      "EmphasisDots:《《強調》》",
      "Text:と",
      "InlineCommand:{w 0.5}",
      "InlineCommand:{p}",
      "InlineCommand:{se ドア}",
      "InlineCommand:{speed 遅}",
      "Interpolation:{=灯.好感度}",
    ],
  );
  assert.deepEqual(lex("　{{と}}と|《と閉じない{"), [
    "Text:　",
    "Escape:{{",
    "Text:と",
    "Escape:}}",
    "Text:と",
    "Escape:|《",
    "Text:と閉じない{",
  ]);
  assert.deepEqual(lex("　ひらがな《るび》は文字のまま"), [
    "Text:　ひらがな《るび》は文字のまま",
  ]);
});

test("行頭のエスケープ", () => {
  assert.deepEqual(lex("\\@背景 は命令ではない"), [
    "Escape:\\",
    "Text:@背景 は命令ではない",
  ]);
  assert.deepEqual(lex("\\灯「台詞の形」"), [
    "Escape:\\",
    "Text:灯「台詞の形」",
  ]);
});

test("台詞：話者、表示名、括弧、入れ子、後ろの文", () => {
  assert.deepEqual(lex("灯（？？？）「誰？」 ^aaaa"), [
    "Speaker:灯",
    "DisplayName:（？？？）",
    "DialogueOpen:「",
    "Text:誰？",
    "DialogueClose:」",
    "LineId: ^aaaa",
  ]);
  assert.deepEqual(lex("灯「彼が「行こう」って」 ^aaaa"), [
    "Speaker:灯",
    "DialogueOpen:「",
    "Text:彼が",
    "NestedOpen:「",
    "Text:行こう",
    "NestedClose:」",
    "Text:って",
    "DialogueClose:」",
    "LineId: ^aaaa",
  ]);
  // 閉じ括弧の後ろに文が続けば、地の文（仕様書6節）。
  assert.deepEqual(lex("灯「行こう」と言った。"), [
    "Text:灯「行こう」と言った。",
  ]);
  // 後ろがインライン命令と空白だけなら、台詞のまま（D041）。
  assert.deepEqual(lex("灯「またね」 {p}　{w 0.5} ^aaaa"), [
    "Speaker:灯",
    "DialogueOpen:「",
    "Text:またね",
    "DialogueClose:」",
    "Text: ",
    "InlineCommand:{p}",
    "Text:　",
    "InlineCommand:{w 0.5}",
    "LineId: ^aaaa",
  ]);
  // 記法は、台詞の括弧の文字をまたがない。
  assert.deepEqual(lex("灯「見て|これ「x」《かの》」"), [
    "Speaker:灯",
    "DialogueOpen:「",
    "Text:見て|これ",
    "NestedOpen:「",
    "Text:x",
    "NestedClose:」",
    "Text:《かの》",
    "DialogueClose:」",
  ]);
  assert.deepEqual(lex("灯『心の声「引用」』"), [
    "Speaker:灯",
    "DialogueOpen:『",
    "Text:心の声「引用」",
    "DialogueClose:』",
  ]);
  // 空白で始まる行や、21文字以上の話者は台詞候補にならない。
  assert.deepEqual(lineKinds("　灯「やあ」\n", parse("　灯「やあ」\n")), [
    "Narration",
  ]);
  const long = `${"あ".repeat(21)}「やあ」`;
  assert.deepEqual(lineKinds(`${long}\n`, parse(`${long}\n`)), ["Narration"]);
});

test("ディレクティブ：命令名、引数、問いかけ文、式", () => {
  assert.deepEqual(lex('@bg "屋上 夕方"　fade'), [
    "DirectiveName:@bg",
    'Argument:"屋上 夕方"',
    "Argument:fade",
  ]);
  assert.deepEqual(lex("@選択 どうする？{w 0.5}早く ^aaaa"), [
    "DirectiveName:@選択 ",
    "Text:どうする？",
    "InlineCommand:{w 0.5}",
    "Text:早く",
    "LineId: ^aaaa",
  ]);
  assert.deepEqual(
    lex('@if not 勇気 >= 2 and 灯.好感度 < (3 + -1) or flag.ルート == "灯"'),
    [
      "DirectiveName:@if",
      "LogicOperator:not",
      "VariableName:勇気",
      "CompareOperator:>=",
      "Number:2",
      "LogicOperator:and",
      "VariableName:灯.好感度",
      "CompareOperator:<",
      "Paren:(",
      "Number:3",
      "ArithmeticOperator:+",
      "ArithmeticOperator:-",
      "Number:1",
      "Paren:)",
      "LogicOperator:or",
      "VariableName:flag.ルート",
      "CompareOperator:==",
      'String:"灯"',
    ],
  );
  assert.deepEqual(lex("@設定 灯.好感度 += 1"), [
    "DirectiveName:@設定",
    "VariableName:灯.好感度",
    "AssignOperator:+=",
    "Number:1",
  ]);
  assert.deepEqual(lex("@設定 flag.ルート = ”灯”"), [
    "DirectiveName:@設定",
    "VariableName:flag.ルート",
    "AssignOperator:=",
    "InvalidChar:”",
    "VariableName:灯",
    "InvalidChar:”",
  ]);
});

test("選択肢の行：表示文、条件、ジャンプ", () => {
  assert.deepEqual(lex("- 黙ってうなずく if 勇気 >= 2 -> s014 ^aaaa"), [
    "OptionMark:-",
    "Text:黙ってうなずく",
    "IfKeyword: if",
    "VariableName:勇気",
    "CompareOperator:>=",
    "Number:2",
    "JumpArrow: ->",
    "SceneRef:s014",
    "LineId: ^aaaa",
  ]);
  assert.deepEqual(lex("- -> s014"), [
    "OptionMark:-",
    "JumpArrow: ->",
    "SceneRef:s014",
  ]);
  assert.deepEqual(lex("- What if I said no"), [
    "OptionMark:-",
    "Text:What",
    "IfKeyword: if",
    "VariableName:I",
    "VariableName:said",
    "VariableName:no",
  ]);
  assert.deepEqual(lex("-"), ["OptionMark:-"]);
});

test("見出しとジャンプ", () => {
  assert.deepEqual(lex("# 第1章 放課後"), [
    "ChapterMark:#",
    "ChapterTitle:第1章 放課後",
  ]);
  assert.deepEqual(lex("## 放課後の屋上 #s012"), [
    "HeadingMark:##",
    "SceneTitle:放課後の屋上",
    "SceneId: #s012",
  ]);
  assert.deepEqual(lex("## #s1"), ["HeadingMark:##", "SceneId: #s1"]);
  assert.deepEqual(lex("## IDの無い見出し"), [
    "HeadingMark:##",
    "SceneTitle:IDの無い見出し",
  ]);
  assert.deepEqual(lex("->s014"), ["JumpArrow:->", "SceneRef:s014"]);
});

test("括弧の設定：既定は「」だけで、『』は地の文", () => {
  const source = "灯『心の声』\n";
  assert.deepEqual(lineKinds(source, parser.parse(source)), ["Narration"]);
  assert.deepEqual(lineKinds(source, parse(source, SAMPLE_BRACKETS)), [
    "Dialogue",
  ]);
  const support = scn({ brackets: SAMPLE_BRACKETS });
  assert.equal(support.language.name, "scn");
});

test("ハイライトのタグ", () => {
  const source = "@背景 屋上 ^aaaa\n";
  const tree = parse(source);
  const tagsAt = (name: string) => {
    let found: readonly unknown[] | null = null;
    tree.iterate({
      enter(node) {
        if (node.name === name) found = getStyleTags(node)?.tags ?? null;
      },
    });
    return found;
  };
  assert.deepEqual(tagsAt("DirectiveName"), [t.keyword]);
  assert.deepEqual(tagsAt("LineId"), [t.docComment]);
});

test("差分の再解析が、全体の解析と同じ結果になる", () => {
  const sample = readFileSync(
    new URL("../../../samples/houkago/story/chapter01.scn", import.meta.url),
    "utf8",
  );
  const p = configureParser(SAMPLE_BRACKETS);
  const edits: [from: number, to: number, insert: string][] = [
    [
      sample.indexOf("灯「やっと来た"),
      sample.indexOf("灯「やっと来た") + 1,
      "店員",
    ],
    [sample.indexOf("遅いよ」"), sample.indexOf("遅いよ」") + 4, "遅いよ"],
    [
      sample.indexOf("summary: 灯が転校"),
      sample.indexOf("summary: 灯が転校"),
      "## 割り込み #x1\n",
    ],
    [sample.indexOf("@raw renpy"), sample.indexOf("@raw renpy") + 4, "@end"],
    [
      sample.indexOf("@end\n\n灯「やっと"),
      sample.indexOf("@end\n\n灯「やっと") + 4,
      "",
    ],
    [sample.indexOf("-> s014"), sample.indexOf("-> s014"), "灯「あ「い"],
    [sample.indexOf(" ^k3fa"), sample.indexOf(" ^k3fa") + 1, ""],
  ];
  for (const [from, to, insert] of edits) {
    assert.ok(from >= 0);
    const before = p.parse(sample);
    const after = sample.slice(0, from) + insert + sample.slice(to);
    const fragments = TreeFragment.applyChanges(TreeFragment.addTree(before), [
      { fromA: from, toA: to, fromB: from, toB: from + insert.length },
    ]);
    const incremental = p.parse(after, fragments);
    const fresh = p.parse(after);
    assert.deepEqual(
      tokens(after, incremental),
      tokens(after, fresh),
      `編集 ${from}〜${to} → ${insert}`,
    );
  }
});

test("でたらめな編集でも、差分の再解析が全体の解析と同じ結果になる", () => {
  let state = 12345;
  const next = () => {
    state = (Math.imul(state, 1103515245) + 12345) >>> 0;
    return state / 4294967296;
  };
  const snippets = [
    "「",
    "」",
    "『",
    "\n",
    "\n\n",
    "@raw x\n",
    "@end\n",
    "---\n",
    "## h #s9\n",
    "//",
    " ^abcd",
    "- ",
    "->",
    "\\",
    "《",
    "》",
    "|",
    "{",
    "}",
    "灯",
    "（",
    "）",
    ":",
    " if ",
    " -> ",
    "　",
  ];
  const p = configureParser(SAMPLE_BRACKETS);
  let doc = readFileSync(
    new URL("../../../samples/houkago/story/chapter01.scn", import.meta.url),
    "utf8",
  );
  let tree = p.parse(doc);
  for (let round = 0; round < 300; round++) {
    const from = Math.floor(next() * (doc.length + 1));
    const to = Math.min(doc.length, from + Math.floor(next() * 4));
    const insert =
      next() < 0.3
        ? ""
        : (snippets[Math.floor(next() * snippets.length)] ?? "");
    const after = doc.slice(0, from) + insert + doc.slice(to);
    const fragments = TreeFragment.applyChanges(TreeFragment.addTree(tree), [
      { fromA: from, toA: to, fromB: from, toB: from + insert.length },
    ]);
    tree = p.parse(after, fragments);
    const fresh = p.parse(after);
    assert.deepEqual(
      tokens(after, tree),
      tokens(after, fresh),
      `編集 ${round}：${from}〜${to} → ${JSON.stringify(insert)}`,
    );
    doc = after;
  }
});

test("色の対応が画面モックに合う（引数・値・式は本文の色、命令名・行ID・コメントは色付き）", () => {
  const source = [
    "## 放課後の屋上 #s012",
    "---",
    "summary: 灯が転校を打ち明けかける",
    "---",
    "@背景 屋上/夕方 フェード",
    "- 黙ってうなずく if 勇気 >= 2 ^k3fe",
    '@設定 flag.ルート = "灯"',
    "// ここで転校の話を出すか要検討",
    "",
  ].join("\n");
  const tree = parse(source);
  const styled = new Map<string, string>();
  highlightTree(tree, scnHighlightStyle, (from, to, classes) => {
    styled.set(source.slice(from, to), classes);
  });
  const has = (text: string) => styled.has(text);
  for (const text of [
    "##",
    " #s012",
    "summary:",
    "@背景",
    "-",
    " if",
    " ^k3fe",
    "@設定",
    "// ここで転校の話を出すか要検討",
    "---",
  ]) {
    assert.ok(has(text), `色が付いていない：${text}`);
  }
  for (const text of [
    "放課後の屋上",
    "灯が転校を打ち明けかける",
    "屋上/夕方",
    "フェード",
    "黙ってうなずく",
    "勇気",
    ">=",
    "2",
    "flag.ルート",
    "=",
    '"灯"',
  ]) {
    assert.ok(!has(text), `本文の色のはずが色付き：${text}`);
  }
});
