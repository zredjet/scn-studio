// テストの共通部品：行の種類の判定と、字句の一覧。

import type { Tree } from "@lezer/common";
import {
  configureParser,
  DEFAULT_BRACKETS,
  type DialogueBracket,
} from "../src/index.ts";

/** 行の種類として数える節。 */
const LINE_KINDS = new Set([
  "Comment",
  "ChapterHeading",
  "SceneHeading",
  "MetaDelimiter",
  "MetaEntry",
  "MetaBlank",
  "MetaInvalid",
  "Directive",
  "RawStart",
  "RawLine",
  "RawEnd",
  "ChoiceOption",
  "Jump",
  "Dialogue",
  "Narration",
]);

/** サンプル作品の括弧（samples/houkago/project.toml の [dialogue.brackets]）。 */
export const SAMPLE_BRACKETS: readonly DialogueBracket[] = [
  { open: "「", close: "」" },
  { open: "『", close: "』" },
];

export function parse(
  source: string,
  brackets: readonly DialogueBracket[] = DEFAULT_BRACKETS,
): Tree {
  return configureParser(brackets).parse(source);
}

/** 行の始まりの位置（0始まりの行番号ごと）。 */
function lineStarts(source: string): number[] {
  const starts = [0];
  for (let i = 0; i < source.length; i++) {
    if (source[i] === "\n") starts.push(i + 1);
  }
  return starts;
}

/**
 * 各行の種類。行頭の空白を除いた最初の文字の位置で、行の種類の節を探す。
 * 空白だけの行は、その行にかかる節（@raw の中の行、メタ情報の空行）があればそれ、無ければ Blank。
 */
export function lineKinds(source: string, tree: Tree): string[] {
  const kinds: string[] = [];
  const starts = lineStarts(source);
  for (const [index, start] of starts.entries()) {
    const end =
      index + 1 < starts.length
        ? (starts[index + 1] ?? source.length) - 1
        : source.length;
    if (index === starts.length - 1 && start === source.length) break;
    let pos = start;
    while (
      pos < end &&
      (source[pos] === " " || source[pos] === "\t" || source[pos] === "﻿")
    )
      pos++;
    const blank = pos >= end || (source[pos] === "\r" && pos + 1 >= end);
    let node: ReturnType<Tree["resolveInner"]> | null = tree.resolveInner(
      blank ? start : pos,
      1,
    );
    let kind = "";
    while (node !== null) {
      if (
        node.name === "LineId" &&
        (node.parent?.name === "Document" || node.parent?.name === "SceneMeta")
      ) {
        kind = "LineId";
        break;
      }
      if (LINE_KINDS.has(node.name)) {
        kind = node.name;
        break;
      }
      node = node.parent;
    }
    if (blank && kind !== "RawLine" && kind !== "MetaBlank") kind = "Blank";
    kinds.push(kind === "" ? "?" : kind);
  }
  return kinds;
}

/** 構文木にあるエラーの位置。 */
export function errors(tree: Tree): number[] {
  const found: number[] = [];
  tree.iterate({
    enter(node) {
      if (node.type.isError) found.push(node.from);
    },
  });
  return found;
}

/** 葉の字句を `名前:文字列` の形で並べる（区切りの空白は含まない）。 */
export function tokens(source: string, tree: Tree): string[] {
  const out: string[] = [];
  tree.iterate({
    enter(node) {
      if (node.node.firstChild === null && node.name !== "Document") {
        out.push(`${node.name}:${source.slice(node.from, node.to)}`);
      }
    },
  });
  return out;
}

/** 行の種類を、行番号と本文つきで書き出す（期待値のファイルの形）。 */
export function renderKinds(source: string, kinds: readonly string[]): string {
  const lines = source.split("\n");
  return `${kinds.map((kind, i) => `${String(i + 1).padStart(3)} ${kind.padEnd(13)} ${lines[i] ?? ""}`.trimEnd()).join("\n")}\n`;
}
