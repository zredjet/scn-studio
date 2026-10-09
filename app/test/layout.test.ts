// 骨格のテスト（T4-17、D086）：幅の決まりと右の欄の開け閉め（U06）、ステータスバーの中身（U05・U08）。

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  closeDrawer,
  isNarrow,
  NARROW_WIDTH,
  type PanelState,
  panelMode,
  parseClosedPanels,
  resized,
  togglePanel,
} from "../src/shell/layout.ts";
import {
  aiRunStatus,
  bibleStatus,
  checkStatus,
  conceptStatus,
  exportStatus,
  flowStatus,
  lineColumn,
  problemsTitle,
  saveLabel,
  saveState,
  saveTitle,
  textStatus,
  variablesStatus,
} from "../src/shell/status.ts";

const open: PanelState = { closed: new Set(), drawer: false };

test("狭い窓の境は 1280（それより狭いと狭い窓）", () => {
  assert.equal(NARROW_WIDTH, 1280);
  assert.equal(isNarrow(1279), true);
  assert.equal(isNarrow(1280), false);
  assert.equal(isNarrow(1440), false);
  assert.equal(isNarrow(1024), true);
});

test("広い窓：右の欄は並べて出し、開け閉めを画面ごとに覚える", () => {
  assert.equal(panelMode("text", false, open), "docked");
  const closedText = togglePanel(open, "text", false);
  assert.equal(panelMode("text", false, closedText), "hidden");
  // ほかの画面には効かない。
  assert.equal(panelMode("flow", false, closedText), "docked");
  assert.equal(
    panelMode("text", false, togglePanel(closedText, "text", false)),
    "docked",
  );
  // 右の欄の無い画面では、ボタンを押しても何も変えない。
  assert.equal(togglePanel(open, "check", false), open);
  assert.equal(panelMode("check", false, open), "hidden");
  assert.equal(panelMode("export", false, open), "hidden");
  assert.equal(panelMode("preferences", false, open), "hidden");
});

test("狭い窓：閉じて始め、開くと重ねる。重ねた欄は覚えない", () => {
  assert.equal(panelMode("text", true, open), "hidden");
  const drawer = togglePanel(open, "text", true);
  assert.equal(panelMode("text", true, drawer), "drawer");
  // 狭い窓の開け閉めは、広い窓で覚えた「閉じた画面」を変えない。
  assert.deepEqual([...drawer.closed], []);
  assert.equal(panelMode("text", true, closeDrawer(drawer)), "hidden");
  assert.equal(closeDrawer(open), open);
});

test("窓の幅が変わったとき：狭くなったら重ねた欄を閉じ、広げたら今の画面の右の欄を出す", () => {
  const drawer = togglePanel(open, "flow", true);
  // 広い → 広い、狭い → 狭いは変えない。
  assert.equal(resized(drawer, "flow", true, true), drawer);
  // 広い窓で閉じていた画面も、狭い窓から広げたら出す。ほかの画面の覚えは残す。
  const closed: PanelState = {
    closed: new Set(["flow", "bible"]),
    drawer: false,
  };
  const widened = resized(closed, "flow", true, false);
  assert.equal(panelMode("flow", false, widened), "docked");
  assert.equal(panelMode("bible", false, widened), "hidden");
  const narrowed = resized(drawer, "flow", false, true);
  assert.equal(narrowed.drawer, false);
});

test("覚えた「閉じている画面」を読む（右の欄のある画面だけ、壊れていれば無し）", () => {
  assert.deepEqual([...parseClosedPanels(null)], []);
  assert.deepEqual([...parseClosedPanels("not json")], []);
  assert.deepEqual([...parseClosedPanels('{"text":true}')], []);
  assert.deepEqual(
    [...parseClosedPanels('["text","check","nope","bible",3]')].sort(),
    ["bible", "text"],
  );
});

test("保存の様子：できなかった・保存中・保存していない・保存済みの順に強い（U08）", () => {
  assert.deepEqual(saveState(0, null, 0), { kind: "saved" });
  assert.deepEqual(saveState(0, null, 2), { kind: "unsaved", files: 2 });
  assert.deepEqual(saveState(1, null, 2), { kind: "saving" });
  assert.deepEqual(saveState(1, "書けません", 2), {
    kind: "failed",
    message: "書けません",
  });
  assert.deepEqual(
    [
      saveState(0, null, 0),
      saveState(1, null, 0),
      saveState(0, null, 1),
      saveState(0, "x", 0),
    ].map(saveLabel),
    ["保存済み", "保存中…", "保存していない変更", "保存できません"],
  );
  assert.match(saveTitle(saveState(0, null, 3)), /3/);
  assert.match(saveTitle(saveState(0, "書けません", 0)), /^書けません/);
  assert.equal(
    problemsTitle({ error: 0, warning: 5, info: 7 }),
    "エラー 0・警告 5・情報 7（押すと検証を開く）",
  );
});

test("ステータスバーの左：本文（表示モード・シーンと字数・章の字数・道）", () => {
  const items = textStatus({
    mode: "writing",
    scene: { id: "s012", title: "放課後の屋上", chars: "161字" },
    file: "story/chapter01.scn",
    fileChars: "451字",
    route: "共通のみ 249字・約1分",
  });
  assert.deepEqual(
    items.map((i) => [i.text, i.strong === true, i.hideNarrow === true]),
    [
      ["執筆表示", true, false],
      ["s012 放課後の屋上 · 161字", false, false],
      ["story/chapter01.scn · 451字", false, true],
      ["共通のみ 249字・約1分", false, true],
    ],
  );
  const empty = textStatus({
    mode: null,
    scene: null,
    file: null,
    fileChars: null,
    route: null,
  });
  assert.deepEqual(
    empty.map((i) => i.text),
    ["本文", "ファイル未選択"],
  );
  assert.equal(
    textStatus({
      ...{
        mode: "source",
        scene: null,
        file: null,
        fileChars: null,
        route: null,
      },
    })[0]?.text,
    "ソース表示",
  );
});

test("行と列は1から数え、列は文字で数える（サロゲートペアを1字に）", () => {
  const text = "一行目\n😀あい\n";
  assert.deepEqual(lineColumn(text, 0), { line: 1, column: 1 });
  assert.deepEqual(lineColumn(text, 4), { line: 2, column: 1 });
  // 😀 は UTF-16 で2つ。その後ろは3列目ではなく2列目。
  assert.deepEqual(lineColumn(text, 6), { line: 2, column: 2 });
  assert.deepEqual(lineColumn(text, 999), { line: 3, column: 1 });
});

test("ステータスバーの左：画面ごとの中身（U05）", () => {
  const texts = (items: readonly { text: string }[]) =>
    items.map((i) => i.text);
  assert.deepEqual(
    texts(
      conceptStatus({
        ideas: [
          { status: "採用" },
          { status: "未整理" },
          { status: "採用" },
          { status: "保留" },
          { status: "未整理" },
        ],
        order: ["未整理", "採用", "保留", "没"],
        endings: 4,
        missingEndings: 1,
      }),
    ),
    [
      "企画",
      "メモ 5（未整理 2・採用 2・保留 1）",
      "エンディング 4（本文に無い 1）",
    ],
  );
  assert.deepEqual(
    texts(
      conceptStatus({
        ideas: null,
        order: [],
        endings: null,
        missingEndings: 0,
      }),
    ),
    ["企画"],
  );
  assert.deepEqual(
    texts(
      flowStatus({
        level: "scene",
        scenes: 13,
        stubs: 5,
        aiDrafts: 2,
        selected: null,
      }),
    ),
    [
      "フロー",
      "シーン表示",
      "13シーン（スタブ 5・AI下書き 2）",
      "選んでいない",
    ],
  );
  assert.deepEqual(
    texts(
      bibleStatus({
        type: { name: "キャラクター", entries: 4 },
        types: 3,
        entries: 6,
        broken: 0,
      }),
    ),
    ["設定", "キャラクター 4", "3型・6エントリ", "参照切れ 0"],
  );
  assert.deepEqual(
    texts(variablesStatus({ count: 6, writtenNotRead: 1, unused: 1 })),
    ["変数", "6", "書いて読まない 1", "使っていない 1"],
  );
  assert.deepEqual(
    texts(
      checkStatus({
        stage2: "done",
        counts: { error: 0, warning: 5, info: 7 },
      }),
    ),
    ["検証", "段階2 済み", "12件（警告 5・情報 7）"],
  );
  assert.deepEqual(
    texts(
      checkStatus({
        stage2: "none",
        counts: { error: 0, warning: 0, info: 0 },
      }),
    ),
    ["検証", "段階2 まだ", "0件"],
  );
  assert.deepEqual(
    texts(aiRunStatus({ running: 1, pending: 0, history: 14 })),
    ["AI実行", "実行中 1", "取り込み待ち 0", "履歴 14"],
  );
  assert.deepEqual(texts(exportStatus(null)), [
    "出力",
    "まだ書き出していません",
  ]);
  assert.deepEqual(texts(exportStatus("21:02 · 収録の台本")), [
    "出力",
    "最後の書き出し 21:02 · 収録の台本",
  ]);
});
