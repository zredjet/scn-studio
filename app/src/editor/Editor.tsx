// 本文のエディタ（CodeMirror 6）。
//
// 日本語入力の約束（.claude/rules/frontend.md）：
// - 変換中は、解析の依頼をしない。入力が止まって200ミリ秒たち、変換中でなければ依頼する。
// - 変換中は、解析の結果（診断の下線）を反映しない。変換が終わってから反映する。
// - 変換中は、本文を置き換えない（保存で付いた行ID、外部の変更の読み込み直し）。変換が終わってから置き換える。
// - 変換中は、保存しない。保存のキーが押されていれば、変換が終わってから保存する。
// - 補完の窓、括弧の補い、正規化は、変換を確定した後にだけ動かす（complete.ts、typing.ts）。変換が始まったら
//   補完の窓を閉じる。basicSetup は使わず、拡張を1つずつ選ぶ。
// 状態の作り方（本文を黙って変えないこと）は state.ts にある。
//
// 保存は1つずつ行う。保存を待つ間の編集は消さず、保存で付いた行IDをその後の本文に入れる（replace.ts の
// savedChanges）。保存を待つ間にもう一度保存のキーが押されたら、終わってからもう一度保存する。
//
// 表示は、執筆表示とソース表示を切り替える（D046）。切り替えても本文は変わらない（飾りと行番号が変わるだけ）。
//
// 定義へ移る（F12、Mod-クリック。navigate.ts）、クイックオープン（Mod-P）、カーソルのあるシーンの知らせ
// （文脈パネルとステータスバー）も、ここからアプリへ知らせる（D047）。ここから再生（F5。T2-4）は、変換中は
// 知らせない。

import { closeCompletion } from "@codemirror/autocomplete";
import { setDiagnostics } from "@codemirror/lint";
import {
  ChangeSet,
  Compartment,
  type Extension,
  Prec,
  type Text,
} from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { scn } from "@scn-studio/lezer-scn";
import { useEffect, useRef } from "react";
import type {
  AssistData,
  Bracket,
  EditorSettings,
  IrDiagnostic,
} from "../core/types.ts";
import { type AssistEnv, assistEnv, completion } from "./complete.ts";
import { previewLine, setPreviewLine } from "./marker.ts";
import { type Definition, definitionAt } from "./navigate.ts";
import { aiLineMarks, aiLines } from "./provenance.ts";
import { replacement, savedChanges } from "./replace.ts";
import {
  createState,
  type Hooks,
  isComposing,
  lineBreakOf,
  matchLineBreaks,
  modeExtension,
  programmatic,
  toEditorDiagnostics,
  type ViewMode,
} from "./state.ts";
import { typewriterScroll } from "./typewriter.ts";
import { runAfterCommit, type TypingOptions, typingAssist } from "./typing.ts";
import { refreshWriting, sceneTitles } from "./writing.ts";

/** カーソルが止まってから、カーソルのある位置を知らせるまでの時間（ミリ秒）。 */
const CURSOR_DELAY_MS = 150;

/** 入力が止まってから解析を依頼するまでの時間（ミリ秒）。 */
export const ANALYZE_DELAY_MS = 200;

/** あるファイルの、ある本文に対する診断。 */
export interface Analysis {
  readonly file: string;
  /** 解析した本文。エディタの本文と違えば（古ければ）反映しない。 */
  readonly text: string;
  readonly diagnostics: readonly IrDiagnostic[];
}

export interface Focus {
  /** エディタの位置（position.ts の PositionMap で求める）。 */
  readonly offset: number;
  /** 同じ位置へもう一度移るときのための通し番号。 */
  readonly seq: number;
}

/** 本文の置き換え（外部の変更の読み込み直し）。 */
export interface Replace {
  readonly file: string;
  readonly text: string;
  /**
   * エディタの本文がこれのときだけ置き換える（知らせが届いてから置き換えるまでに編集していれば、置き換えずに
   * `onReplaceSkipped` を呼ぶ）。null ならいつも置き換える。
   */
  readonly expected: string | null;
  /** 同じ中身でもう一度置き換えるときのための通し番号。 */
  readonly seq: number;
}

/** 待っている保存。 */
interface Saving {
  readonly file: string;
  /** 保存を頼んだ本文。 */
  readonly base: string;
  /** 保存を頼んでからの編集。 */
  since: ChangeSet;
  /** 保存を待つ間に別のファイルへ切り替えたときの、そのファイルの最後の本文と改行。 */
  detached: { readonly doc: Text; readonly lineBreak: string } | null;
  /** 保存を待つ間に、本文が読み込み直された。付いた行IDは入れない。 */
  skip: boolean;
  /** 書いた本文。まだ届いていなければ undefined、保存できなければ null。 */
  result: string | null | undefined;
}

interface Props {
  readonly file: string;
  /** そのファイルを開いたときの本文（保存していない編集を含む）。 */
  readonly text: string;
  /** ディスクの中身。これと違えば、保存していない変更がある（改行の種類はエディタにそろえて比べる）。 */
  readonly savedText: string;
  readonly brackets: readonly Bracket[];
  /** 表示（執筆表示かソース表示か）。 */
  readonly viewMode: ViewMode;
  /** シーンIDと題名（執筆表示で、ジャンプに題名を添える。補完のシーンIDの候補）。 */
  readonly titles: ReadonlyMap<string, string>;
  /** 補完の材料（D047）。開いていなければ null。 */
  readonly assist: AssistData | null;
  /** 入力補助の設定（`project.toml` の `[editor]`）。 */
  readonly editorSettings: EditorSettings;
  readonly analysis: Analysis | null;
  readonly focus: Focus | null;
  readonly replace: Replace | null;
  /** 入力が止まり、変換中でないときに呼ぶ。本文を保存せずに解析を依頼する。 */
  readonly onIdle: (file: string, text: string) => void;
  /** 変換が始まった（true）、終わった（false）。 */
  readonly onComposingChange: (composing: boolean) => void;
  /** 保存していない変更があるか（人が編集したら、すぐに true を知らせる）。 */
  readonly onDirty: (file: string, dirty: boolean) => void;
  /** 保存する（変換中は呼ばない）。書いた本文を返す。保存できなければ null。 */
  readonly onSave: (file: string, text: string) => Promise<string | null>;
  /** 読み込み直しの知らせが届いてから置き換えるまでに編集していたので、置き換えなかった。 */
  readonly onReplaceSkipped: (file: string, text: string) => void;
  /** 表示の切り替えのキーが押された。 */
  readonly onToggleView: (file: string) => void;
  /** カーソルが止まった（エディタの位置）。変換中は知らせない。 */
  readonly onCursor: (file: string, offset: number) => void;
  /** 定義へ移る（F12、Mod-クリック）。 */
  readonly onDefinition: (definition: Definition) => void;
  /** クイックオープンのキー（Mod-P）が押された。 */
  readonly onQuickOpen: () => void;
  /** ここから再生のキー（F5）が押された（エディタの位置）。 */
  readonly onPlayHere?: (file: string, offset: number) => void;
  /** 名前の変更のキー（F2）が押された（エディタの位置と、今の本文。T2-8）。 */
  readonly onRename?: (file: string, offset: number, text: string) => void;
  /** タイプライタースクロール（カーソルの行を縦の真ん中に置く。T2-8）。 */
  readonly typewriter?: boolean;
  /** AIが書いた行（行IDから、今の出自 `ai`・`ai-edited`。T3-4）。印を付ける。 */
  readonly aiLines?: ReadonlyMap<string, string>;
  /** プレビュー中の行（「エディタと同期」。T1-11）。ほかのファイルなら印を付けない。 */
  readonly previewLine?: {
    readonly file: string;
    readonly offset: number;
  } | null;
}

/** AIが書いた行が無いとき（毎回新しい Map を作らないため）。 */
const NO_AI_LINES: ReadonlyMap<string, string> = new Map();

export function Editor({
  file,
  text,
  savedText,
  brackets,
  viewMode,
  titles,
  assist,
  editorSettings,
  analysis,
  focus,
  replace,
  onIdle,
  onComposingChange,
  onDirty,
  onSave,
  onReplaceSkipped,
  onToggleView,
  onCursor,
  onDefinition,
  onQuickOpen,
  onPlayHere,
  onRename,
  typewriter = false,
  aiLines: aiLineMap = NO_AI_LINES,
  previewLine: preview = null,
}: Props) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const language = useRef(new Compartment());
  const mode = useRef(new Compartment());
  const titleSet = useRef(new Compartment());
  const assistSet = useRef(new Compartment());
  const typewriterSet = useRef(new Compartment());
  const aiLineSet = useRef(new Compartment());
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const cursorTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  /** 変換で入った範囲（変換中の編集に合わせてずらす。始まりは前へ、終わりは後ろへ広げる）。 */
  const composed = useRef<{ from: number; to: number } | null>(null);
  /** 変換が終わって、確定の後の入力補助を待っている範囲。 */
  const committed = useRef<{ from: number; to: number } | null>(null);
  /** 変換中に届いて、まだ反映していない診断。 */
  const deferred = useRef<Analysis | null>(null);
  /** 変換中に届いて、まだ当てていない置き換え。 */
  const deferredReplace = useRef<Replace | null>(null);
  /** エディタに今入っている本文のファイルと、最後に解析へ送った（または読み込んだ）本文。 */
  const loaded = useRef<{ file: string; sent: string } | null>(null);
  /** 保存していない変更があると知らせたか。 */
  const dirtyReported = useRef(false);
  const saving = useRef<Saving | null>(null);
  /** 保存を待つ間に、もう一度保存のキーが押された。 */
  const saveAgain = useRef(false);
  /** 変換中に保存のキーが押された。 */
  const saveAfterComposition = useRef(false);
  // 効果やコールバックの中では、作り直さずに最新の値を読む。
  const props = useRef({
    file,
    text,
    savedText,
    brackets,
    viewMode,
    titles,
    assist,
    editorSettings,
    onIdle,
    onComposingChange,
    onDirty,
    onSave,
    onReplaceSkipped,
    onToggleView,
    onCursor,
    onDefinition,
    onQuickOpen,
    onPlayHere,
    onRename,
    typewriter,
    aiLineMap,
  });
  props.current = {
    file,
    text,
    savedText,
    brackets,
    viewMode,
    titles,
    assist,
    editorSettings,
    onIdle,
    onComposingChange,
    onDirty,
    onSave,
    onReplaceSkipped,
    onToggleView,
    onCursor,
    onDefinition,
    onQuickOpen,
    onPlayHere,
    onRename,
    typewriter,
    aiLineMap,
  };

  /** 補完に要るもの。 */
  const assistEnvOf = (): AssistEnv => ({
    data: props.current.assist,
    scenes: [...props.current.titles].map(([id, title]) => ({ id, title })),
    brackets: props.current.brackets,
  });
  const typingOptions = (): TypingOptions => ({
    brackets: props.current.brackets,
    editor: props.current.editorSettings,
  });

  /** カーソルが止まったら、その位置を知らせる。変換中は知らせない。 */
  const scheduleCursor = () => {
    if (cursorTimer.current !== null) clearTimeout(cursorTimer.current);
    cursorTimer.current = setTimeout(() => {
      cursorTimer.current = null;
      const v = view.current;
      const current = loaded.current;
      if (v === null || current === null || isComposing(v)) return;
      props.current.onCursor(current.file, v.state.selection.main.head);
    }, CURSOR_DELAY_MS);
  };

  /** 定義へ移る。移る先が無ければ false。 */
  const goToDefinition = (v: EditorView, pos: number): boolean => {
    const found = definitionAt(v.state, pos);
    if (found === null) return false;
    props.current.onDefinition(found);
    return true;
  };

  /** エディタが足す拡張（入力補助、補完、定義へ移る操作、カーソルの知らせ）。 */
  const extras = (): Extension => [
    previewLine(),
    aiLineSet.current.of(aiLines.of(props.current.aiLineMap)),
    aiLineMarks(),
    typewriterSet.current.of(
      props.current.typewriter === true ? typewriterScroll() : [],
    ),
    assistSet.current.of(assistEnv.of(assistEnvOf())),
    completion(),
    typingAssist(typingOptions),
    Prec.high(
      keymap.of([
        {
          key: "F12",
          run: (v) => goToDefinition(v, v.state.selection.main.head),
        },
        {
          key: "Mod-p",
          preventDefault: true,
          run: () => {
            props.current.onQuickOpen();
            return true;
          },
        },
        {
          key: "F2",
          preventDefault: true,
          run: (v) => {
            const current = loaded.current;
            if (current === null || isComposing(v)) return true;
            // 本文は改行をそのまま（CRLF も）渡す（`doc.toString()` は `\n` でつなぐ）。
            props.current.onRename?.(
              current.file,
              v.state.selection.main.head,
              v.state.sliceDoc(),
            );
            return true;
          },
        },
        {
          key: "F5",
          preventDefault: true,
          run: (v) => {
            const current = loaded.current;
            if (current === null || isComposing(v)) return true;
            props.current.onPlayHere?.(
              current.file,
              v.state.selection.main.head,
            );
            return true;
          },
        },
      ]),
    ),
    EditorView.domEventHandlers({
      mousedown: (event, v) => {
        if (!(event.metaKey || event.ctrlKey)) return false;
        // 執筆表示のジャンプの札（シーンIDの印がある）。
        const target = event.target as HTMLElement | null;
        const ref =
          target?.closest<HTMLElement>("[data-scene-ref]")?.dataset.sceneRef;
        if (ref !== undefined) {
          props.current.onDefinition({ kind: "scene", id: ref });
          event.preventDefault();
          return true;
        }
        const pos = v.posAtCoords({ x: event.clientX, y: event.clientY });
        if (pos === null || !goToDefinition(v, pos)) return false;
        event.preventDefault();
        return true;
      },
    }),
    EditorView.updateListener.of((update) => {
      if (update.selectionSet || update.docChanged) scheduleCursor();
    }),
  ];

  /** 今の設定で、エディタの状態を作る。 */
  const makeState = (doc: string) =>
    createState(
      doc,
      props.current.brackets,
      language.current,
      stableHooks.current,
      {
        mode: props.current.viewMode,
        modeCompartment: mode.current,
        titles: props.current.titles,
        titlesCompartment: titleSet.current,
        extra: extras(),
      },
    );

  /** 保存していない変更があるかを、本文とディスクの中身を比べて知らせる。 */
  const reportDirty = () => {
    const v = view.current;
    const current = loaded.current;
    if (v === null || current === null) return;
    const doc = v.state.sliceDoc();
    const disk = props.current.savedText;
    const dirty =
      doc !== disk && doc !== matchLineBreaks(disk, v.state.lineBreak);
    dirtyReported.current = dirty;
    props.current.onDirty(current.file, dirty);
  };

  /** 診断を反映する。変換中なら、変換が終わるまで待つ。本文が変わっていれば、古いので捨てる。 */
  const apply = (result: Analysis) => {
    const v = view.current;
    if (v === null || result.file !== loaded.current?.file) return;
    if (isComposing(v)) {
      deferred.current = result;
      return;
    }
    deferred.current = null;
    // 改行の種類をそろえて読み込んだ本文でも、エディタの位置は同じ（改行はどれも1つに数える）。
    const doc = v.state.sliceDoc();
    if (
      doc !== result.text &&
      doc !== matchLineBreaks(result.text, v.state.lineBreak)
    ) {
      return;
    }
    v.dispatch(
      setDiagnostics(
        v.state,
        toEditorDiagnostics(result.text, result.diagnostics),
      ),
    );
  };

  /** 本文を置き換える（読み込み直し）。変換中なら、変換が終わるまで待つ。 */
  const applyReplace = (r: Replace) => {
    const v = view.current;
    const current = loaded.current;
    if (v === null || current === null || r.file !== current.file) return;
    if (isComposing(v)) {
      deferredReplace.current = r;
      return;
    }
    deferredReplace.current = null;
    const before = v.state.sliceDoc();
    const lineBreak = v.state.lineBreak;
    if (
      r.expected !== null &&
      before !== matchLineBreaks(r.expected, lineBreak)
    ) {
      props.current.onReplaceSkipped(current.file, before);
      return;
    }
    const s = saving.current;
    if (s !== null && s.file === current.file) s.skip = true;
    const incoming = /[\r\n]/.test(r.text) ? lineBreakOf(r.text) : lineBreak;
    if (incoming === lineBreak) {
      const change = replacement(before, matchLineBreaks(r.text, lineBreak));
      if (change !== null) {
        v.dispatch({ changes: change, annotations: programmatic.of(true) });
      }
    } else {
      // 改行の種類が変わった。エディタは1種類しか持てないので、状態を作り直す。
      v.setState(makeState(r.text));
    }
    current.sent = v.state.sliceDoc();
    reportDirty();
  };

  /** 今の本文を、解析へ送る。 */
  const send = () => {
    const v = view.current;
    const current = loaded.current;
    if (v === null || current === null) return;
    const doc = v.state.sliceDoc();
    current.sent = doc;
    props.current.onIdle(current.file, doc);
    reportDirty();
  };

  /** 入力が止まってから、解析を依頼する。変換中なら依頼せず、変換が終わるのを待つ。 */
  const schedule = () => {
    if (timer.current !== null) clearTimeout(timer.current);
    timer.current = setTimeout(() => {
      timer.current = null;
      const v = view.current;
      if (v === null || isComposing(v)) return;
      send();
    }, ANALYZE_DELAY_MS);
  };

  /** 保存を始める。変換中なら変換が終わってから、保存を待っているなら終わってから、もう一度呼ぶ。 */
  const startSave = () => {
    const v = view.current;
    const current = loaded.current;
    if (v === null || current === null) return;
    if (isComposing(v)) {
      saveAfterComposition.current = true;
      return;
    }
    if (saving.current !== null) {
      saveAgain.current = true;
      return;
    }
    const base = v.state.sliceDoc();
    const s: Saving = {
      file: current.file,
      base,
      since: ChangeSet.empty(v.state.doc.length),
      detached: null,
      skip: false,
      result: undefined,
    };
    saving.current = s;
    current.sent = base;
    void props.current.onSave(current.file, base).then((result) => {
      s.result = result;
      finishSave();
    });
  };

  /** 保存の結果を受け取り、付いた行IDを今の本文に入れる。変換中なら、変換が終わるまで待つ。 */
  const finishSave = () => {
    const s = saving.current;
    if (s === null || s.result === undefined) return;
    const v = view.current;
    if (s.detached === null && !s.skip && s.result !== null && v !== null) {
      if (isComposing(v)) return;
      const changes = savedChanges(s.base, s.result, s.since, v.state.doc);
      saving.current = null;
      if (changes.length > 0) {
        v.dispatch({ changes, annotations: programmatic.of(true) });
      }
      const current = loaded.current;
      if (current !== null && v.state.sliceDoc() === s.result) {
        current.sent = s.result;
      }
      reportDirty();
    } else {
      saving.current = null;
      if (s.detached !== null && !s.skip && s.result !== null) {
        // 保存を待つ間に別のファイルへ切り替えた。そのファイルの最後の本文に入れ、解析へ送る（コアの本文も直す）。
        const { doc, lineBreak } = s.detached;
        const changes = savedChanges(s.base, s.result, s.since, doc);
        const merged = ChangeSet.of(changes, doc.length)
          .apply(doc)
          .sliceString(0, undefined, lineBreak);
        props.current.onIdle(s.file, merged);
        props.current.onDirty(s.file, merged !== s.result);
      }
    }
    if (saveAgain.current) {
      saveAgain.current = false;
      startSave();
    }
  };

  const hooks = useRef<Hooks>({
    changed: () => {},
    save: () => {},
    compositionStarted: () => {},
    compositionEnded: () => {},
    toggleView: () => {},
  });
  hooks.current = {
    changed: (byUser, changes) => {
      for (const range of [composed, committed]) {
        const r = range.current;
        if (r !== null) {
          range.current = {
            from: changes.mapPos(r.from, -1),
            to: changes.mapPos(r.to, 1),
          };
        }
      }
      const s = saving.current;
      if (s !== null && s.detached === null && !s.skip) {
        s.since = s.since.compose(changes);
      }
      schedule();
      const current = loaded.current;
      if (byUser && current !== null && !dirtyReported.current) {
        dirtyReported.current = true;
        props.current.onDirty(current.file, true);
      }
    },
    save: () => startSave(),
    compositionStarted: () => {
      const v = view.current;
      if (v !== null) {
        // 変換中は補完の窓を出さない（.claude/rules/frontend.md）。
        closeCompletion(v);
        const from = v.state.selection.main.from;
        composed.current = { from, to: from };
      }
      props.current.onComposingChange(true);
    },
    compositionEnded: () => {
      props.current.onComposingChange(false);
      // 確定した範囲を、ここで取り出す（すぐ次の変換が始まっても混ざらないように）。
      committed.current = composed.current;
      composed.current = null;
      // 変換中に止めていた解析の依頼、診断の反映、本文の置き換え、保存を、変換が終わってから行う。
      schedule();
      setTimeout(() => {
        // 変換中に見送った執筆表示の飾りを作り直す。
        view.current?.dispatch({ effects: refreshWriting.of(null) });
        // 確定した文字の後の入力補助（括弧の補い、正規化）と、補完の窓。確定した文字だけを対象にし、
        // カーソルが確定した文字の後ろに無ければ（確定と同時に別の場所をクリックしたなど）動かさない。
        const v = view.current;
        const range = committed.current;
        committed.current = null;
        if (v !== null && range !== null && range.from < range.to) {
          const sel = v.state.selection.main;
          if (sel.empty && sel.head === range.to) {
            runAfterCommit(v, range.from, range.to, typingOptions());
          }
        }
        const pending = deferred.current;
        if (pending !== null) apply(pending);
        const waiting = deferredReplace.current;
        if (waiting !== null) applyReplace(waiting);
        finishSave();
        if (saveAfterComposition.current) {
          saveAfterComposition.current = false;
          startSave();
        }
      }, 0);
    },
    toggleView: () => {
      const current = loaded.current;
      if (current !== null) props.current.onToggleView(current.file);
    },
  };
  // 状態に渡すのは、いつも最新の hooks.current を呼ぶ関数。
  const stableHooks = useRef<Hooks>({
    changed: (byUser, changes) => hooks.current.changed(byUser, changes),
    save: () => hooks.current.save(),
    compositionStarted: () => hooks.current.compositionStarted(),
    compositionEnded: () => hooks.current.compositionEnded(),
    toggleView: () => hooks.current.toggleView(),
  });

  // エディタを作る。本文は下のファイルの効果で入れる。
  useEffect(() => {
    if (host.current === null) return;
    const v = new EditorView({ parent: host.current });
    view.current = v;
    return () => {
      if (timer.current !== null) clearTimeout(timer.current);
      if (cursorTimer.current !== null) clearTimeout(cursorTimer.current);
      v.destroy();
      view.current = null;
      loaded.current = null;
    };
  }, []);

  // ファイルを切り替えたら、本文を入れ替える。まだ送っていない編集があれば、先に送る。
  const makeStateRef = useRef(makeState);
  makeStateRef.current = makeState;
  const sendRef = useRef(send);
  sendRef.current = send;
  const reportRef = useRef(reportDirty);
  reportRef.current = reportDirty;
  useEffect(() => {
    const v = view.current;
    if (v === null || loaded.current?.file === file) return;
    if (timer.current !== null) clearTimeout(timer.current);
    timer.current = null;
    if (loaded.current !== null && v.state.sliceDoc() !== loaded.current.sent) {
      sendRef.current();
    }
    const s = saving.current;
    if (s !== null && s.detached === null && s.file === loaded.current?.file) {
      s.detached = { doc: v.state.doc, lineBreak: v.state.lineBreak };
    }
    deferred.current = null;
    deferredReplace.current = null;
    saveAfterComposition.current = false;
    v.setState(makeStateRef.current(props.current.text));
    loaded.current = { file, sent: v.state.sliceDoc() };
    // 保存を待つ間に、そのファイルへ戻ってきた。本文が離れたときのままなら、続けて編集を追う。
    if (s !== null && s.detached !== null && s.file === file) {
      if (v.state.doc.eq(s.detached.doc)) s.detached = null;
      else s.skip = true;
    }
    reportRef.current();
  }, [file]);

  // ディスクの中身が変わったら（保存、読み込み直し）、保存していない変更があるかを知らせ直す。
  useEffect(() => {
    if (savedText !== undefined) reportRef.current();
  }, [savedText]);

  // 表示を切り替える。本文は変わらない。
  useEffect(() => {
    view.current?.dispatch({
      effects: mode.current.reconfigure(modeExtension(viewMode)),
    });
  }, [viewMode]);

  // シーンの題名が変わったら、入れ替える。
  useEffect(() => {
    view.current?.dispatch({
      effects: titleSet.current.reconfigure(sceneTitles.of(titles)),
    });
  }, [titles]);

  // AIが書いた行の印を入れ替える。
  useEffect(() => {
    view.current?.dispatch({
      effects: aiLineSet.current.reconfigure(aiLines.of(aiLineMap)),
    });
  }, [aiLineMap]);

  // タイプライタースクロールの切り替え。
  useEffect(() => {
    view.current?.dispatch({
      effects: typewriterSet.current.reconfigure(
        typewriter ? typewriterScroll() : [],
      ),
    });
  }, [typewriter]);

  // 補完の材料が変わったら、入れ替える。
  const assistEnvRef = useRef(assistEnvOf);
  assistEnvRef.current = assistEnvOf;
  useEffect(() => {
    void assist;
    void titles;
    void brackets;
    view.current?.dispatch({
      effects: assistSet.current.reconfigure(
        assistEnv.of(assistEnvRef.current()),
      ),
    });
  }, [assist, titles, brackets]);

  // 括弧の設定が変わったら、言語を入れ替える。
  useEffect(() => {
    view.current?.dispatch({
      effects: language.current.reconfigure(scn({ brackets })),
    });
  }, [brackets]);

  const applyRef = useRef(apply);
  applyRef.current = apply;
  useEffect(() => {
    if (analysis !== null) applyRef.current(analysis);
  }, [analysis]);

  const replaceRef = useRef(applyReplace);
  replaceRef.current = applyReplace;
  useEffect(() => {
    if (replace !== null) replaceRef.current(replace);
  }, [replace]);

  useEffect(() => {
    const v = view.current;
    if (v === null || focus === null) return;
    const offset = Math.min(focus.offset, v.state.doc.length);
    v.dispatch({
      selection: { anchor: offset },
      effects: EditorView.scrollIntoView(offset, { y: "start", yMargin: 24 }),
    });
    v.focus();
  }, [focus]);

  // プレビュー中の行に印を付ける（カーソルもフォーカスも動かさない）。
  const previewAt = preview?.file === file ? preview.offset : null;
  useEffect(() => {
    // ファイルを替えると状態を作り直すので、印も付け直す。
    void file;
    const v = view.current;
    if (v === null) return;
    v.dispatch({
      effects:
        previewAt === null
          ? setPreviewLine.of(null)
          : [
              setPreviewLine.of(previewAt),
              EditorView.scrollIntoView(
                Math.min(previewAt, v.state.doc.length),
                {
                  y: "center",
                },
              ),
            ],
    });
  }, [previewAt, file]);

  return <div className="editor" ref={host} />;
}
