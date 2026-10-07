// 本文のエディタ（CodeMirror 6）。
//
// 日本語入力の約束（.claude/rules/frontend.md）：
// - 変換中は、解析の依頼をしない。入力が止まって200ミリ秒たち、変換中でなければ依頼する。
// - 変換中は、解析の結果（診断の下線）を反映しない。変換が終わってから反映する。
// - 変換中は、本文を置き換えない（保存で付いた行ID、外部の変更の読み込み直し）。変換が終わってから置き換える。
// - 変換中は、保存しない。保存のキーが押されていれば、変換が終わってから保存する。
// - 補完と括弧の自動補完は入れない（basicSetup を使わず、拡張を1つずつ選ぶ）。
// 状態の作り方（本文を黙って変えないこと）は state.ts にある。
//
// 保存は1つずつ行う。保存を待つ間の編集は消さず、保存で付いた行IDをその後の本文に入れる（replace.ts の
// savedChanges）。保存を待つ間にもう一度保存のキーが押されたら、終わってからもう一度保存する。
//
// 表示は、執筆表示とソース表示を切り替える（D046）。切り替えても本文は変わらない（飾りと行番号が変わるだけ）。

import { setDiagnostics } from "@codemirror/lint";
import { ChangeSet, Compartment, type Text } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { scn } from "@scn-studio/lezer-scn";
import { useEffect, useRef } from "react";
import type { Bracket, IrDiagnostic } from "../core/types.ts";
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
import { refreshWriting, sceneTitles } from "./writing.ts";

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
  /** シーンIDと題名（執筆表示で、ジャンプに題名を添える）。 */
  readonly titles: ReadonlyMap<string, string>;
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
}

export function Editor({
  file,
  text,
  savedText,
  brackets,
  viewMode,
  titles,
  analysis,
  focus,
  replace,
  onIdle,
  onComposingChange,
  onDirty,
  onSave,
  onReplaceSkipped,
  onToggleView,
}: Props) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const language = useRef(new Compartment());
  const mode = useRef(new Compartment());
  const titleSet = useRef(new Compartment());
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
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
    onIdle,
    onComposingChange,
    onDirty,
    onSave,
    onReplaceSkipped,
    onToggleView,
  });
  props.current = {
    file,
    text,
    savedText,
    brackets,
    viewMode,
    titles,
    onIdle,
    onComposingChange,
    onDirty,
    onSave,
    onReplaceSkipped,
    onToggleView,
  };

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
    compositionStarted: () => props.current.onComposingChange(true),
    compositionEnded: () => {
      props.current.onComposingChange(false);
      // 変換中に止めていた解析の依頼、診断の反映、本文の置き換え、保存を、変換が終わってから行う。
      schedule();
      setTimeout(() => {
        // 変換中に見送った執筆表示の飾りを作り直す。
        view.current?.dispatch({ effects: refreshWriting.of(null) });
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

  return <div className="editor" ref={host} />;
}
