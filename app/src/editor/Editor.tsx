// 本文のエディタ（CodeMirror 6）。
//
// 日本語入力の約束（.claude/rules/frontend.md）：
// - 変換中は、解析の依頼をしない。入力が止まって200ミリ秒たち、変換中でなければ依頼する。
// - 変換中は、解析の結果（診断の下線）を反映しない。変換が終わってから反映する。
// - 補完と括弧の自動補完は入れない（basicSetup を使わず、拡張を1つずつ選ぶ）。
// 状態の作り方（本文を黙って変えないこと）は state.ts にある。

import { setDiagnostics } from "@codemirror/lint";
import { Compartment } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { scn } from "@scn-studio/lezer-scn";
import { useEffect, useRef } from "react";
import type { Bracket, IrDiagnostic } from "../core/types.ts";
import {
  createState,
  type Hooks,
  isComposing,
  toEditorDiagnostics,
} from "./state.ts";

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

interface Props {
  readonly file: string;
  /** そのファイルを開いたときの本文。 */
  readonly text: string;
  readonly brackets: readonly Bracket[];
  readonly analysis: Analysis | null;
  readonly focus: Focus | null;
  /** 入力が止まり、変換中でないときに呼ぶ。本文を保存せずに解析を依頼する。 */
  readonly onIdle: (file: string, text: string) => void;
  /** 変換が始まった（true）、終わった（false）。 */
  readonly onComposingChange: (composing: boolean) => void;
}

export function Editor({
  file,
  text,
  brackets,
  analysis,
  focus,
  onIdle,
  onComposingChange,
}: Props) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const language = useRef(new Compartment());
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  /** 変換中に届いて、まだ反映していない診断。 */
  const deferred = useRef<Analysis | null>(null);
  /** エディタに今入っている本文のファイルと、最後に解析へ送った（または読み込んだ）本文。 */
  const loaded = useRef<{ file: string; sent: string } | null>(null);
  // 効果やコールバックの中では、作り直さずに最新の値を読む。
  const props = useRef({ file, text, brackets, onIdle, onComposingChange });
  props.current = { file, text, brackets, onIdle, onComposingChange };

  /** 診断を反映する。変換中なら、変換が終わるまで待つ。本文が変わっていれば、古いので捨てる。 */
  const apply = (result: Analysis) => {
    const v = view.current;
    if (v === null || result.file !== loaded.current?.file) return;
    if (isComposing(v)) {
      deferred.current = result;
      return;
    }
    deferred.current = null;
    if (v.state.sliceDoc() !== result.text) return;
    v.dispatch(
      setDiagnostics(
        v.state,
        toEditorDiagnostics(result.text, result.diagnostics),
      ),
    );
  };

  /** 今の本文を、解析へ送る。 */
  const send = () => {
    const v = view.current;
    const current = loaded.current;
    if (v === null || current === null) return;
    const doc = v.state.sliceDoc();
    current.sent = doc;
    props.current.onIdle(current.file, doc);
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

  const hooks = useRef<Hooks>({
    changed: () => {},
    compositionStarted: () => {},
    compositionEnded: () => {},
  });
  hooks.current = {
    changed: schedule,
    compositionStarted: () => props.current.onComposingChange(true),
    compositionEnded: () => {
      props.current.onComposingChange(false);
      // 変換中に止めていた解析の依頼と、診断の反映を、変換が終わってから行う。
      schedule();
      const pending = deferred.current;
      if (pending !== null) setTimeout(() => apply(pending), 0);
    },
  };
  // 状態に渡すのは、いつも最新の hooks.current を呼ぶ関数。
  const stableHooks = useRef<Hooks>({
    changed: () => hooks.current.changed(),
    compositionStarted: () => hooks.current.compositionStarted(),
    compositionEnded: () => hooks.current.compositionEnded(),
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
  const sendRef = useRef(send);
  sendRef.current = send;
  useEffect(() => {
    const v = view.current;
    if (v === null || loaded.current?.file === file) return;
    if (timer.current !== null) clearTimeout(timer.current);
    timer.current = null;
    if (loaded.current !== null && v.state.sliceDoc() !== loaded.current.sent) {
      sendRef.current();
    }
    deferred.current = null;
    loaded.current = { file, sent: props.current.text };
    v.setState(
      createState(
        props.current.text,
        props.current.brackets,
        language.current,
        stableHooks.current,
      ),
    );
  }, [file]);

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
