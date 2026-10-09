// 作品全体の検索のテスト（T4-22、N1）：すべての章の一致、位置（UTF-8 のバイト）と行、「本文だけ」の絞り込み、
// 正規表現・大文字と小文字・語として、上限、前後の文字（行IDは見せない）。

import assert from "node:assert/strict";
import { test } from "node:test";
import type { SceneSummary, SearchRanges } from "../src/core/types.ts";
import {
  type FindSpec,
  findAll,
  findRegExp,
  insideRanges,
  sceneAt,
} from "../src/editor/projectSearch.ts";

const spec = (search: string, more: Partial<FindSpec> = {}): FindSpec => ({
  search,
  regexp: false,
  caseSensitive: false,
  wholeWord: false,
  bodyOnly: false,
  ...more,
});

const a = "## 屋上 #s1\n灯「好感度が上がる」 ^k3f9\n@設定 灯.好感度 += 1\n";
const b = "## 雨 #s2\r\n　😀好感度。 ^k3fa\r\n";
const files = [
  { file: "story/a.scn", text: a },
  { file: "story/b.scn", text: b },
];
const bytes = (text: string, upTo: number) =>
  new TextEncoder().encode(text.slice(0, upTo)).length;

test("すべての章の一致を、章の順に、バイトの位置と行で", () => {
  const found = findAll(files, spec("好感度"), new Map(), false);
  assert.equal(found.total, 3);
  assert.deepEqual(
    found.files.map((f) => [f.file, f.matches.map((m) => m.line)]),
    [
      ["story/a.scn", [2, 3]],
      ["story/b.scn", [2]],
    ],
  );
  const first = found.files[0]?.matches[0];
  assert.equal(first?.start, bytes(a, a.indexOf("好感度")));
  assert.equal(first?.end, (first?.start ?? 0) + 9);
  // 絵文字（サロゲートペア）と \r\n の後でも、バイトの位置が合う。
  const emoji = found.files[1]?.matches[0];
  assert.equal(emoji?.start, bytes(b, b.indexOf("好感度")));
  // 前後の文字。行IDは見せない。
  assert.equal(first?.before, "灯「");
  assert.equal(first?.after, "が上がる」");
  assert.equal(emoji?.after, "。");
});

test("「本文だけ」：本文の範囲に入る一致だけ", () => {
  const start = bytes(a, a.indexOf("好感度"));
  const ranges = new Map<string, SearchRanges>([
    ["story/a.scn", { text: [[start, start + 15]], line_ids: [] }],
  ]);
  const found = findAll(
    files,
    spec("好感度", { bodyOnly: true }),
    ranges,
    false,
  );
  // 演出の行（@設定）と、範囲の無い章（b）は除く。
  assert.equal(found.total, 1);
  assert.equal(found.files[0]?.matches[0]?.line, 2);
  assert.equal(insideRanges([[10, 20]], 10, 20), true);
  assert.equal(insideRanges([[10, 20]], 9, 12), false);
  assert.equal(
    insideRanges(
      [
        [0, 5],
        [10, 20],
      ],
      12,
      15,
    ),
    true,
  );
});

test("正規表現・大文字と小文字・語として・上限", () => {
  const text = [{ file: "story/c.scn", text: "Akari akari AKARIS\n" }];
  assert.equal(findAll(text, spec("akari"), new Map(), false).total, 3);
  assert.equal(
    findAll(text, spec("akari", { caseSensitive: true }), new Map(), false)
      .total,
    1,
  );
  assert.equal(
    findAll(text, spec("akari", { wholeWord: true }), new Map(), false).total,
    2,
  );
  assert.equal(
    findAll(text, spec("a\\w+i\\b", { regexp: true }), new Map(), false).total,
    2,
  );
  // 文字どおりに探すときは、記号を正規表現として読まない。
  assert.equal(findAll(text, spec("a.ari"), new Map(), false).total, 0);
  // 正しくない式。
  assert.equal(findRegExp(spec("(", { regexp: true })), null);
  assert.equal(
    findAll(text, spec("(", { regexp: true }), new Map(), false).invalid,
    true,
  );
  const capped = findAll(text, spec("a"), new Map(), false, 2);
  assert.deepEqual([capped.total, capped.capped], [2, true]);
});

test("執筆表示では、隠れた行IDにかかる一致を除く（範囲はコアから）", () => {
  const text = "灯「k3f9 を探す」 ^k3f9\n";
  const id = bytes(text, text.indexOf("^"));
  const ranges = new Map<string, SearchRanges>([
    ["story/a.scn", { text: [], line_ids: [[id, id + 5]] }],
  ]);
  const one = [{ file: "story/a.scn", text }];
  assert.equal(findAll(one, spec("k3f9"), ranges, false).total, 2);
  assert.equal(findAll(one, spec("k3f9"), ranges, true).total, 1);
});

test("CodeMirror にそろえる：文字どおりの \\n と、1行ずつ当てる正規表現", () => {
  const one = [{ file: "story/a.scn", text: "あい\nうえ\n" }];
  // 文字どおりに探す \n は改行として読む。
  assert.equal(findAll(one, spec("い\\nう"), new Map(), false).total, 1);
  // 改行を含まない式は、行をまたいで当たらない（[^x] は CodeMirror も行をまたぐ）。
  assert.equal(
    findAll(one, spec("い.う", { regexp: true }), new Map(), false).total,
    0,
  );
  assert.equal(
    findAll(one, spec("い\\sう", { regexp: true }), new Map(), false).total,
    1,
  );
});

test("一致のシーン：同じ章で、一致より前に見出しのある最後のシーン", () => {
  const scene = (id: string, file: string, at: number): SceneSummary => ({
    id,
    title: id,
    file,
    span: [at, at + 4],
    meta: { summary: null, status: null, route: null, tags: [], extra: {} },
    is_stub: false,
    chars: 0,
  });
  const scenes = [
    scene("s1", "story/a.scn", 10),
    scene("s2", "story/a.scn", 50),
    scene("t1", "story/b.scn", 0),
  ];
  assert.equal(sceneAt(scenes, { file: "story/a.scn", start: 30 })?.id, "s1");
  // 見出しの位置ちょうどは、そのシーン。
  assert.equal(sceneAt(scenes, { file: "story/a.scn", start: 50 })?.id, "s2");
  // 最初の見出しより前（章の見出しなど）は無し。別の章のシーンは拾わない。
  assert.equal(sceneAt(scenes, { file: "story/a.scn", start: 3 }), null);
  assert.equal(sceneAt(scenes, { file: "story/c.scn", start: 30 }), null);
});
