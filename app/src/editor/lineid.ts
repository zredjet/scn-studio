// 行IDを守る（D046、D047）。執筆表示では行IDを隠すので、人が気づかないうちに行IDがずれたり、
// 文の途中に残ったりしないようにする。ソース表示の扱いは下（行の分割と結合のときだけ）。
//
// - カーソルは行IDに入らない（行IDの手前で止まる。行IDの手前で → を押すと次の行の頭へ移る）。
// - 人の編集の後、編集にかかった行の行IDを、仕様書9節のきまりに合わせて置き直す：
//   - 行を分割したら、前半（元の行の中身の頭がある行）が元のIDを引き継ぐ。後半は行IDなし（保存のときに付く）。
//     行頭の字下げ（半角空白、タブ、段落頭の全角空白）の後ろで分割したときは、中身のある後半が引き継ぐ
//     （Home で字下げの後ろへ移ってから Enter を押すのは、上に行を足す操作なので）。
//   - 台詞の括弧の中での改行は分割ではなく、同じ台詞が複数行になるだけなので、行IDは閉じ括弧のある行
//     （台詞の最後の行。仕様書6節）に残す（D046）。
//   - 行を結合したら、先頭の行のIDを残す。ほかの行のIDは消す。
//   - 行の中身を書き換えても、IDは変えない。行IDだけを消す編集は、行IDを戻す。
//   - 行を消したとき、中身が空になったときは、IDも消す。
// - 行IDの手前で Delete を押すと、次の行と結合する（行IDを消さない）。
// 変換中の編集、取り消しとやり直し、プログラムによる置き換え（保存で付いた行ID、読み込み直し）には手を出さない。
//
// ソース表示（T1-8、D047）：行IDが見えるので、行の分割と結合（改行を足す・消す編集）のときだけ、
// 仕様書9節のきまりどおりに置き直す。行IDの文字を書き換える編集（重なった行IDを直すなど）は妨げず、
// カーソルも止めない。
//
// 執筆表示で写す（コピー）ときは、行IDを外す（貼り付けた行には、保存のときに新しい行IDが付く）。
// 切り取りは行の移動なので、行IDごと写す（設計の原則5。D047）。
//
// 行IDの位置は、Lezer の構文木の `LineId`（前の空白を含む）から取る。見た目のための文法だが、行IDの形は
// コアと同じ規則（仕様書9節）で切り出している。

import { syntaxTree } from "@codemirror/language";
import {
  type ChangeSet,
  EditorSelection,
  EditorState,
  type Extension,
  type Line,
  MapMode,
  Prec,
  type SelectionRange,
  type Text,
  type Transaction,
  type TransactionSpec,
} from "@codemirror/state";
import { type Command, EditorView, keymap } from "@codemirror/view";
import type { SyntaxNode, Tree } from "@lezer/common";
import { programmatic } from "./flags.ts";
import type { Replacement } from "./replace.ts";

/** 行末の行IDの候補（仕様書9節）。 */
const LINE_ID_AT_END = / \^[0-9A-Za-z]+[ \t]*$/;

/** 行IDの範囲（前の空白を含む）。 */
export interface IdRange {
  readonly from: number;
  readonly to: number;
}

/** その行の行ID（構文木の `LineId`）。無ければ null。 */
export function lineIdOf(tree: Tree, line: Line): IdRange | null {
  let found: IdRange | null = null;
  tree.iterate({
    from: line.from,
    to: line.to,
    enter: (node) => {
      if (found !== null) return false;
      if (
        node.name === "LineId" &&
        node.from >= line.from &&
        node.to <= line.to
      ) {
        found = { from: node.from, to: node.to };
        return false;
      }
      return undefined;
    },
  });
  return found;
}

interface OldId {
  /** 元の行の頭。 */
  readonly lineFrom: number;
  /**
   * 行IDを引き継ぐ行を決める位置：台詞の行なら閉じ括弧、ほかは中身の頭（行頭の半角空白・タブ・全角空白の
   * 後ろの、最初の文字）。この位置が変更の後にある行が、行IDを引き継ぐ。
   */
  readonly anchor: number;
  /** 元の行の終わり。 */
  readonly lineTo: number;
  readonly id: IdRange;
  readonly text: string;
}

/** 行IDを引き継ぐ行を決める位置（`OldId.anchor`）。 */
function anchorOf(
  state: EditorState,
  tree: Tree,
  line: Line,
  id: IdRange,
): number {
  for (
    let node: SyntaxNode | null = tree.resolveInner(id.from + 1, 1);
    node !== null;
    node = node.parent
  ) {
    if (node.name === "Dialogue") {
      const close = node.getChildren("DialogueClose").at(-1);
      // 閉じ括弧が同じ行にあるとき（1行の台詞、複数行の台詞の最後の行）だけ。
      if (
        close !== undefined &&
        close.from >= line.from &&
        close.from <= line.to
      ) {
        return close.from;
      }
      break;
    }
  }
  const text = state.doc.sliceString(line.from, id.from);
  const indent = /^[ \t\u3000]*/.exec(text)?.[0].length ?? 0;
  return indent < text.length ? line.from + indent : line.from;
}

/** 変更にかかった行（変更の前の文書）のうち、行IDのあるもの。 */
function touchedIds(state: EditorState, changes: ChangeSet): OldId[] {
  const tree = syntaxTree(state);
  const doc = state.doc;
  const seen = new Set<number>();
  const result: OldId[] = [];
  changes.iterChangedRanges((fromA, toA) => {
    let line = doc.lineAt(fromA);
    for (;;) {
      if (!seen.has(line.number)) {
        seen.add(line.number);
        const id = lineIdOf(tree, line);
        if (id !== null) {
          result.push({
            lineFrom: line.from,
            anchor: anchorOf(state, tree, line, id),
            lineTo: line.to,
            id,
            text: doc.sliceString(id.from, id.to),
          });
        }
      }
      if (line.to >= toA || line.number >= doc.lines) break;
      line = doc.line(line.number + 1);
    }
  });
  return result;
}

/** 行が改行ごと消されたか（中身だけを消して空の行が残ったときは、消されていない）。 */
function lineRemoved(changes: ChangeSet, old: OldId): boolean {
  let removed = false;
  changes.iterChanges((fromA, toA) => {
    if (
      fromA <= old.lineFrom &&
      toA >= old.lineTo &&
      (fromA < old.lineFrom || toA > old.lineTo)
    ) {
      removed = true;
    }
  });
  return removed;
}

/** 行IDの文字が、変更で書き換えられていないか（前後に足しただけなら、そのまま）。 */
function intact(changes: ChangeSet, id: IdRange): boolean {
  let ok = true;
  changes.iterChanges((fromA, toA) => {
    if (fromA < id.to && toA > id.from) ok = false;
    if (fromA === toA && fromA > id.from && fromA < id.to) ok = false;
  });
  return ok;
}

interface Placed {
  readonly old: OldId;
  /** 行IDを引き継ぐ行（行ごと消されたら null）。 */
  readonly target: number | null;
  /** 行IDの文字が今ある範囲（書き換えられていなければ）。 */
  readonly at: IdRange | null;
  /** 書き換えられた行IDの残り（消すもの）。 */
  readonly remnant: IdRange | null;
}

/**
 * 人の編集（`changes`。変更の前は `state`、後は `doc`）の後に、行IDを置き直す変更（`doc` の位置）。
 * 置き直すものが無ければ空。
 */
export function lineIdFixes(
  state: EditorState,
  changes: ChangeSet,
  doc: Text,
  restore = true,
): Replacement[] {
  // `restore` が false（ソース表示）なら、書き換えられた行IDは置き直しの対象にしない。
  const olds = touchedIds(state, changes).filter(
    (old) => restore || intact(changes, old.id),
  );
  if (olds.length === 0) return [];
  const placed: Placed[] = olds.map((old) => {
    // 引き継ぐ位置が消されたら（中身を消したなど）、行の頭で決める。
    const head = lineRemoved(changes, old)
      ? null
      : (changes.mapPos(old.anchor, 1, MapMode.TrackDel) ??
        changes.mapPos(old.lineFrom, 1, MapMode.TrackDel));
    const whole = intact(changes, old.id);
    const from = changes.mapPos(old.id.from, 1);
    const to = changes.mapPos(old.id.to, -1);
    return {
      old,
      target: head === null ? null : doc.lineAt(head).number,
      at: whole ? { from, to } : null,
      remnant: !whole && from < to ? { from, to } : null,
    };
  });
  // 今の行ごとに、引き継ぐ行ID（引き継ぐ位置がこの行にあるもののうち、いちばん前の行のもの）と、
  // この行にある行IDの文字を集める。
  const lines = new Map<number, { heir: Placed | null; present: IdRange[] }>();
  const entry = (n: number) => {
    let e = lines.get(n);
    if (e === undefined) {
      e = { heir: null, present: [] };
      lines.set(n, e);
    }
    return e;
  };
  for (const p of placed) {
    if (p.target !== null) {
      const e = entry(p.target);
      if (e.heir === null || p.old.lineFrom < e.heir.old.lineFrom) e.heir = p;
    }
    if (p.at !== null) entry(doc.lineAt(p.at.from).number).present.push(p.at);
  }
  const result: Replacement[] = [];
  const remove = (r: IdRange) => result.push({ ...r, insert: "" });
  for (const p of placed) {
    if (p.remnant !== null) remove(p.remnant);
  }
  for (const [n, { heir, present }] of lines) {
    // 引き継ぐ行IDの無い行（行ごと消された行の残りなど）は、そのままにする。
    if (heir === null) continue;
    const line = doc.line(n);
    const at = heir.at;
    // 行IDの文字を除いた中身が空（半角空白とタブだけ）なら、この行の行IDを消す（行を消したとみなす）。
    // 全角空白は本文として扱う。
    const content = [...present]
      .sort((a, b) => b.from - a.from)
      .reduce(
        (text, r) =>
          text.slice(0, r.from - line.from) + text.slice(r.to - line.from),
        line.text,
      );
    if (/^[ \t]*$/.test(content)) {
      for (const r of present) remove(r);
      continue;
    }
    // 置き直しの対象でない行IDが、もう行末にあれば何もしない（行IDを2つ並べない）。
    if (LINE_ID_AT_END.test(content)) continue;
    const inPlace =
      at !== null &&
      doc.lineAt(at.from).number === n &&
      /^[ \t]*$/.test(doc.sliceString(at.to, line.to));
    for (const r of present) {
      if (!(inPlace && at !== null && r.from === at.from)) remove(r);
    }
    if (at !== null && !inPlace) remove(at);
    if (!inPlace) {
      result.push({ from: line.to, to: line.to, insert: heir.old.text });
    }
  }
  return dedupe(result);
}

/** 同じ範囲を2回消さない。 */
function dedupe(changes: Replacement[]): Replacement[] {
  const seen = new Set<string>();
  return changes
    .filter((c) => {
      const key = `${c.from}:${c.to}:${c.insert}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    })
    .sort((a, b) => a.from - b.from || a.to - b.to);
}

/** 行IDを置き直さない変更（変換中の入力、取り消し、プログラムによる置き換え）。 */
function untouchable(tr: Transaction): boolean {
  return (
    tr.annotation(programmatic) === true ||
    tr.isUserEvent("input.type.compose") ||
    tr.isUserEvent("undo") ||
    tr.isUserEvent("redo")
  );
}

/**
 * カーソルを行IDに入れない。行IDの中か後ろへ動いたら、行IDの手前に戻す。ただし行IDの手前から
 * → で1文字進んだときは、次の行の頭へ移す（行IDを飛び越す）。
 */
export function clampSelection(
  state: EditorState,
  before: EditorSelection,
  after: EditorSelection,
  pointer: boolean,
): EditorSelection | null {
  const tree = syntaxTree(state);
  const doc = state.doc;
  let changed = false;
  const clamp = (pos: number, old: number | null): number => {
    const line = doc.lineAt(pos);
    const id = lineIdOf(tree, line);
    if (id === null || pos <= id.from || pos > id.to) return pos;
    changed = true;
    if (
      !pointer &&
      old === id.from &&
      pos === id.from + 1 &&
      line.number < doc.lines
    ) {
      return line.to + 1;
    }
    return id.from;
  };
  const ranges = after.ranges.map((range: SelectionRange, i: number) => {
    const old = before.ranges[i] ?? before.main;
    const head = clamp(range.head, old.head);
    const anchor = range.empty ? head : clamp(range.anchor, null);
    return EditorSelection.range(anchor, head);
  });
  return changed ? EditorSelection.create(ranges, after.mainIndex) : null;
}

/** 改行を足すか消す編集か（行の分割と結合）。 */
export function structural(tr: Transaction): boolean {
  let found = false;
  const doc = tr.startState.doc;
  tr.changes.iterChanges((fromA, toA, _fromB, _toB, inserted) => {
    if (
      inserted.lines > 1 ||
      doc.lineAt(fromA).number !== doc.lineAt(toA).number
    ) {
      found = true;
    }
  });
  return found;
}

/** 行IDを守る変更の絞り込み（執筆表示）。 */
const guard = EditorState.transactionFilter.of(
  (tr): Transaction | readonly TransactionSpec[] => {
    if (untouchable(tr)) return tr;
    if (tr.docChanged) {
      const fixes = lineIdFixes(tr.startState, tr.changes, tr.newDoc);
      if (fixes.length === 0) return tr;
      return [tr, { changes: fixes, sequential: true }];
    }
    if (tr.selection === undefined) return tr;
    const selection = clampSelection(
      tr.startState,
      tr.startState.selection,
      tr.newSelection,
      tr.isUserEvent("select.pointer"),
    );
    if (selection === null) return tr;
    return [tr, { selection, sequential: true }];
  },
);

/** 行IDの手前で Delete を押したら、次の行と結合する（行IDは消さない）。 */
export const deleteForwardAtLineId: Command = ({ state, dispatch }) => {
  const range = state.selection.main;
  if (state.selection.ranges.length !== 1 || !range.empty) return false;
  const line = state.doc.lineAt(range.head);
  const id = lineIdOf(syntaxTree(state), line);
  if (id === null || range.head !== id.from) return false;
  if (line.number >= state.doc.lines) return true;
  dispatch(
    state.update({
      changes: { from: line.to, to: line.to + 1 },
      userEvent: "delete.forward",
      scrollIntoView: true,
    }),
  );
  return true;
};

/** ソース表示：行の分割と結合のときだけ、行IDを置き直す（書き換えられた行IDは戻さない）。 */
const sourceGuard = EditorState.transactionFilter.of(
  (tr): Transaction | readonly TransactionSpec[] => {
    if (untouchable(tr) || !tr.docChanged || !structural(tr)) return tr;
    const fixes = lineIdFixes(tr.startState, tr.changes, tr.newDoc, false);
    if (fixes.length === 0) return tr;
    return [tr, { changes: fixes, sequential: true }];
  },
);

/**
 * 写す文字（執筆表示のコピー）：選んだ範囲から行IDを外す。範囲が空なら、カーソルのある行を写す
 * （CodeMirror と同じ）。テストから直接使う。
 */
export function copyWithoutLineIds(state: EditorState): string {
  const tree = syntaxTree(state);
  const doc = state.doc;
  const strip = (from: number, to: number): string => {
    let text = "";
    let at = from;
    for (let n = doc.lineAt(from).number; n <= doc.lineAt(to).number; n++) {
      const line = doc.line(n);
      const id = lineIdOf(tree, line);
      if (id !== null && id.from < to && id.to > from) {
        text += doc.sliceString(at, Math.max(at, id.from));
        at = Math.min(to, id.to);
      }
    }
    return text + doc.sliceString(at, to);
  };
  const ranges = state.selection.ranges;
  if (ranges.every((r) => r.empty)) {
    const lines = [...new Set(ranges.map((r) => doc.lineAt(r.head).number))];
    return lines
      .map((n) => {
        const line = doc.line(n);
        return strip(line.from, line.to) + state.lineBreak;
      })
      .join("");
  }
  return ranges
    .filter((r) => !r.empty)
    .map((r) => strip(r.from, r.to).split("\n").join(state.lineBreak))
    .join(state.lineBreak);
}

/** 執筆表示のコピー：行IDを外して写す。切り取りは CodeMirror に任せる（行IDごと写す）。 */
const copyHandler = EditorView.domEventHandlers({
  copy(event, view) {
    const data = event.clipboardData;
    if (data === null) return false;
    data.clearData();
    data.setData("text/plain", copyWithoutLineIds(view.state));
    event.preventDefault();
    return true;
  },
});

/** 行IDを守る拡張。執筆表示（`writing`）とソース表示（`source`）で扱いが違う。 */
export function lineIdGuard(mode: "writing" | "source" = "writing"): Extension {
  if (mode === "source") return sourceGuard;
  return [
    guard,
    copyHandler,
    Prec.high(keymap.of([{ key: "Delete", run: deleteForwardAtLineId }])),
  ];
}
