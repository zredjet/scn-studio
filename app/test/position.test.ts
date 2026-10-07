// 位置の変換のテスト。全角文字（UTF-8で3バイト、UTF-16で1）と、サロゲートペア（4バイト、2）を含む。

import assert from "node:assert/strict";
import { test } from "node:test";
import { PositionMap } from "../src/core/position.ts";

/** 文字列の先頭から `prefix` の長さの位置が、両方の単位で一致することを確かめる。 */
function check(text: string, prefix: string) {
  assert.ok(text.startsWith(prefix));
  const map = new PositionMap(text);
  const bytes = Buffer.byteLength(prefix, "utf8");
  assert.equal(map.byteToUtf16(bytes), prefix.length, `byte→utf16：${prefix}`);
  assert.equal(map.utf16ToByte(prefix.length), bytes, `utf16→byte：${prefix}`);
}

test("ASCIIだけ", () => {
  check("abc def", "abc ");
  const map = new PositionMap("abc");
  assert.equal(map.byteLength, 3);
  assert.equal(map.utf16Length, 3);
});

test("全角文字（UTF-8で3バイト）", () => {
  const text = "灯「やっと来た。遅いよ」 ^k3f9";
  for (const prefix of [
    "",
    "灯",
    "灯「",
    "灯「やっと来た。遅いよ」",
    "灯「やっと来た。遅いよ」 ^",
  ]) {
    check(text, prefix);
  }
  const map = new PositionMap(text);
  assert.equal(map.byteLength, Buffer.byteLength(text, "utf8"));
  assert.equal(map.utf16Length, text.length);
});

test("サロゲートペア（絵文字や拡張漢字。UTF-8で4バイト、UTF-16で2）", () => {
  const text = "😀𠮷野家🎉 ^abcd";
  for (const prefix of ["😀", "😀𠮷", "😀𠮷野家", "😀𠮷野家🎉", text]) {
    check(text, prefix);
  }
});

test("全角空白、2バイトの文字、BOM", () => {
  const text = "\uFEFF## é #s1\n　本文";
  for (const prefix of [
    "\uFEFF",
    "\uFEFF## ",
    "\uFEFF## é",
    "\uFEFF## é #s1\n",
    "\uFEFF## é #s1\n　",
  ]) {
    check(text, prefix);
  }
});

test("CRLF の改行は、エディタでは1つの位置に数える", () => {
  const text = "a\r\nい\r\n";
  const map = new PositionMap(text);
  // バイト：a=0, \r=1, \n=2, い=3..5, \r=6, \n=7、全体8。エディタ：a=0, 改行=1, い=2, 改行=3、全体4。
  assert.equal(map.byteLength, 8);
  assert.equal(map.utf16Length, 4);
  assert.equal(map.byteToUtf16(1), 1);
  assert.equal(map.byteToUtf16(2), 1);
  assert.equal(map.byteToUtf16(3), 2);
  assert.equal(map.utf16ToByte(1), 1);
  assert.equal(map.utf16ToByte(2), 3);
  assert.equal(map.utf16ToByte(4), 8);
});

test("文字の途中を指すバイト位置は、その文字の先頭に丸める", () => {
  const map = new PositionMap("あい😀");
  assert.equal(map.byteToUtf16(1), 0);
  assert.equal(map.byteToUtf16(2), 0);
  assert.equal(map.byteToUtf16(3), 1);
  assert.equal(map.byteToUtf16(7), 2);
  assert.equal(map.byteToUtf16(8), 2);
  // サロゲートペアの間（UTF-16の位置3）は、絵文字の先頭のバイト位置にする。
  assert.equal(map.utf16ToByte(3), 6);
});

test("範囲の外は両端に切り詰める", () => {
  const map = new PositionMap("灯");
  assert.equal(map.byteToUtf16(-5), 0);
  assert.equal(map.byteToUtf16(100), 1);
  assert.equal(map.utf16ToByte(100), 3);
  const empty = new PositionMap("");
  assert.equal(empty.byteToUtf16(0), 0);
  assert.equal(empty.utf16ToByte(3), 0);
});

test("コアのスパン（サンプルの台詞の行ID）を、CodeMirrorの位置に変換できる", () => {
  const line = "灯「やっと来た。遅いよ」 ^k3f9";
  const start = Buffer.byteLength("灯「やっと来た。遅いよ」 ", "utf8");
  const end = Buffer.byteLength(line, "utf8");
  const map = new PositionMap(line);
  assert.equal(
    line.slice(map.byteToUtf16(start), map.byteToUtf16(end)),
    "^k3f9",
  );
});
