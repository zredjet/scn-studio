// 閉じていないブロックや括弧を含む入力でも、後続の行のハイライトが崩れないことを確かめる。

import assert from "node:assert/strict";
import { test } from "node:test";
import { errors, lineKinds, parse, SAMPLE_BRACKETS } from "./support.ts";

function kinds(source: string): string[] {
  const tree = parse(source, SAMPLE_BRACKETS);
  assert.deepEqual(errors(tree), [], `構文木にエラーがある：\n${source}`);
  return lineKinds(source, tree);
}

const cases: { name: string; source: string; expected: string[] }[] = [
  {
    name: "閉じていない @選択 の後のシーン",
    source:
      "## a #s1\n@選択 どうする ^aaaa\n- 行く ^aaab\n\n## b #s2\n　本文。 ^aaac\n",
    expected: [
      "SceneHeading",
      "Directive",
      "ChoiceOption",
      "Blank",
      "SceneHeading",
      "Narration",
    ],
  },
  {
    name: "閉じていない @if の後の行",
    source: "## a #s1\n@if 勇気 >= (\n灯「行こう」 ^aaaa\n-> s2\n",
    expected: ["SceneHeading", "Directive", "Dialogue", "Jump"],
  },
  {
    name: "@raw は字下げの無い ## の行で閉じる",
    source:
      "## a #s1\n@raw renpy\n  ## 字下げした行は中身\n  $ a = 1\n## b #s2\n灯「次のシーン」 ^aaaa\n",
    expected: [
      "SceneHeading",
      "RawStart",
      "RawLine",
      "RawLine",
      "SceneHeading",
      "Dialogue",
    ],
  },
  {
    name: "@raw の中の空行と、閉じていないままのファイルの終わり",
    source: "@raw renpy\n  $ a\n\n  @選択 これも中身",
    expected: ["RawStart", "RawLine", "RawLine", "RawLine"],
  },
  {
    name: "閉じていない台詞は空行で終わる",
    source: "灯「閉じ忘れ ^aaaa\n\n　次の段落。 ^aaab\n",
    expected: ["Dialogue", "Blank", "Narration"],
  },
  {
    name: "閉じていない台詞は、命令・見出し・選択肢・ジャンプの行で終わる",
    source:
      "灯「一\n@背景 屋上\n灯「二\n- 行く\n灯「三\n-> s1\n灯「四\n## b #s2\n",
    expected: [
      "Dialogue",
      "Directive",
      "Dialogue",
      "ChoiceOption",
      "Dialogue",
      "Jump",
      "Dialogue",
      "SceneHeading",
    ],
  },
  {
    name: "複数行の台詞の続き。途中のコメントは台詞に含めない",
    source:
      "灯「一行目\n// 途中のコメント\n  二行目\n\\@三行目」 ^aaaa\n　次の地の文。 ^aaab\n",
    expected: ["Dialogue", "Comment", "Dialogue", "Dialogue", "Narration"],
  },
  {
    name: "閉じていない台詞の後のコメントと見出し",
    source: "灯「閉じ忘れ\n// コメント\n## b #s2\n",
    expected: ["Dialogue", "Comment", "SceneHeading"],
  },
  {
    name: "入れ子の括弧が閉じていない台詞",
    source: "灯「あ「い」\n\n灯「次」 ^aaaa\n",
    expected: ["Dialogue", "Blank", "Dialogue"],
  },
  {
    name: "閉じていないメタ情報は、キー: 値 でない行で終わる",
    source:
      "## a #s1\n---\nsummary: 要約\n// コメントは続く\n灯「本文」 ^aaaa\n-> s2\n",
    expected: [
      "SceneHeading",
      "MetaDelimiter",
      "MetaEntry",
      "Comment",
      "Dialogue",
      "Jump",
    ],
  },
  {
    name: "閉じたメタ情報の中の空行と崩れた行",
    source:
      "## a #s1\n---\nsummary: 要約\n\nsummary 崩れ\n---\n　本文。 ^aaaa\n",
    expected: [
      "SceneHeading",
      "MetaDelimiter",
      "MetaEntry",
      "MetaBlank",
      "MetaInvalid",
      "MetaDelimiter",
      "Narration",
    ],
  },
  {
    name: "次のシーンの開きの --- を、閉じと取り違えない",
    source:
      "## a #s1\n---\nsummary: 閉じ忘れ\n\n## b #s2\n---\nsummary: b\n---\n",
    expected: [
      "SceneHeading",
      "MetaDelimiter",
      "MetaEntry",
      "Blank",
      "SceneHeading",
      "MetaDelimiter",
      "MetaEntry",
      "MetaDelimiter",
    ],
  },
  {
    name: "閉じていない文中の記法",
    source:
      "　閉じていない{や《や|や《《のある行。 ^aaaa\n灯「{w 長め}次の行」 ^aaab\n",
    expected: ["Narration", "Dialogue"],
  },
  {
    name: "シーンより前の行と、見出しの外の ---",
    source: "　シーンより前。\n---\n## a #s1\n\n---\n",
    expected: ["Narration", "Narration", "SceneHeading", "Blank", "Narration"],
  },
  {
    name: "CRLF の改行",
    source: "## a #s1\r\n灯「一行目\r\n二行目」 ^aaaa\r\n　次。\r\n",
    expected: ["SceneHeading", "Dialogue", "Dialogue", "Narration"],
  },
];

for (const { name, source, expected } of cases) {
  test(name, () => {
    assert.deepEqual(kinds(source), expected);
  });
}

// ---- でたらめな入力 ----

/** 再現できる乱数（mulberry32）。 */
function random(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let x = state;
    x = Math.imul(x ^ (x >>> 15), x | 1);
    x ^= x + Math.imul(x ^ (x >>> 7), x | 61);
    return ((x ^ (x >>> 14)) >>> 0) / 4294967296;
  };
}

const FRAGMENTS = [
  "",
  "# 第1章",
  "## 見出し #s1",
  "## 見出し",
  "---",
  "summary: 要約",
  "summary 崩れ",
  "// コメント",
  "@背景 屋上/夕方 フェード",
  '@bg "屋上',
  "@設定 灯.好感度 += (1",
  "@選択 どうする{w 0.5}",
  "@if 勇気 >= 2 and not 迷い",
  "@else",
  "@end",
  "@raw renpy",
  "- 行く if 勇気 >= 2 -> s2",
  "- -> s2",
  "-",
  "-> s2",
  "灯「やあ」",
  "灯「開いたまま",
  "灯（？？？）「誰？",
  "灯『心の声",
  "閉じる」",
  "「「「",
  "彼は「行こう」と言った。",
  "　地の文|乱《みだ》れ《《傍点》》{p}{se ドア}{=x + 1}{{}}",
  "漢字《かんじ》《",
  "|《",
  "{w",
  "\\@背景",
  "^abcd",
  " ^abcd",
  "　",
  "\t",
];

test("でたらめな行の組み合わせでも、エラーにならず、空行の後の見出しはシーン見出しになる", () => {
  const next = random(20261007);
  const pick = <T>(items: readonly T[]): T =>
    items[Math.floor(next() * items.length)] as T;
  for (let round = 0; round < 2000; round++) {
    const lines: string[] = [];
    const count = Math.floor(next() * 30);
    for (let i = 0; i < count; i++) {
      const indent = pick(["", "", "  ", "\t", "　"]);
      const id = pick(["", "", " ^k3f9", " ^K3F9", " ^_^"]);
      lines.push(`${indent}${pick(FRAGMENTS)}${id}`);
    }
    // 途中で切れた入力も混ぜる。
    let source = lines.join(pick(["\n", "\r\n"]));
    source = source.slice(0, Math.floor(next() * (source.length + 1)));
    const tree = parse(
      `${source}\n\n## 回復 #s9\n灯「回復した」 ^zzzz\n`,
      SAMPLE_BRACKETS,
    );
    const all = lineKinds(
      `${source}\n\n## 回復 #s9\n灯「回復した」 ^zzzz\n`,
      tree,
    );
    assert.deepEqual(
      all.slice(-2),
      ["SceneHeading", "Dialogue"],
      `回復できない入力：\n${source}`,
    );
    assert.ok(!all.includes("?"), `種類の分からない行がある：\n${source}`);
    assert.deepEqual(errors(tree), [], `構文木にエラーがある：\n${source}`);
  }
});

const bracketCases: { name: string; source: string; expected: string[] }[] = [
  {
    name: "はぐれた | や記法が閉じ括弧をまたいでも、次の行は崩れない",
    source:
      "灯「見て|これ」と言った。彼女《かのじょ》は\n　次の地の文。\n灯「次の台詞」\n",
    expected: ["Narration", "Narration", "Dialogue"],
  },
  {
    name: "台詞の中の { が閉じ括弧をまたいでも、次の行は崩れない",
    source: "灯「えっと{」と言いかけて}やめた\n灯「次の台詞」\n",
    expected: ["Narration", "Dialogue"],
  },
  {
    name: "台詞の中のルビが入れ子の括弧をまたがない",
    source: "灯「見て|これ「x」《かの》」\n　次の地の文。\n",
    expected: ["Dialogue", "Narration"],
  },
  {
    name: "メタ情報の中の行IDだけの行",
    source: "## a #s1\n---\nsummary: 要約\n ^abcd\n---\n　本文。 ^aaaa\n",
    expected: [
      "SceneHeading",
      "MetaDelimiter",
      "MetaEntry",
      "LineId",
      "MetaDelimiter",
      "Narration",
    ],
  },
  {
    name: "閉じていないメタ情報は、行IDだけの行で終わる",
    source: "## a #s1\n---\nsummary: 要約\n ^abcd\n　本文。 ^aaaa\n",
    expected: [
      "SceneHeading",
      "MetaDelimiter",
      "MetaEntry",
      "LineId",
      "Narration",
    ],
  },
];

for (const { name, source, expected } of bracketCases) {
  test(name, () => {
    assert.deepEqual(kinds(source), expected);
  });
}

const trailingCases: { name: string; source: string; expected: string[] }[] = [
  {
    name: "閉じ括弧の後に文が続く行は、話者を解決できてもできなくても地の文に見せる",
    source:
      "彼は「行こう」と言った。 ^aaaa\n灯「行こう」と言った。\n灯「行こう」　 ^aaab\n灯「あ「い」う」と\n",
    expected: ["Narration", "Narration", "Dialogue", "Narration"],
  },
  {
    name: "複数行の台詞の、閉じ括弧の後に文が続くか",
    source:
      "彼は「一行目\n// コメント\n二行目」と言った。\n灯「一行目\n二行目」 ^aaaa\n",
    expected: ["Narration", "Comment", "Narration", "Dialogue", "Dialogue"],
  },
  {
    name: "閉じないまま打ち切られる台詞は、台詞に見せる",
    source: "灯「一行目\n\n　地の文」と言った。\n",
    expected: ["Dialogue", "Blank", "Narration"],
  },
  {
    name: "閉じ括弧の後ろがインライン命令と空白だけなら、台詞に見せる（D041）",
    source:
      "灯「またね」{p} ^aaaa\n灯「……」 {w 0.5}　{se ドア}{speed 遅}\n灯「一行目\n二行目」{p}\n",
    expected: ["Dialogue", "Dialogue", "Dialogue", "Dialogue"],
  },
  {
    name: "閉じ括弧の後ろの記法が文を作るか、命令として読めなければ、地の文に見せる（D041）",
    source:
      "灯「行こう」{=flag.名前}\n灯「行こう」｜乱《みだ》\n灯「行こう」《《強く》》\n灯「またね」{shake}\n灯「またね」{w 長め}\n灯「またね」{{p}}\n灯「またね」{p\n",
    expected: [
      "Narration",
      "Narration",
      "Narration",
      "Narration",
      "Narration",
      "Narration",
      "Narration",
    ],
  },
];

for (const { name, source, expected } of trailingCases) {
  test(name, () => {
    assert.deepEqual(kinds(source), expected);
  });
}
