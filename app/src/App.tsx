// フェーズ0の最小の画面：左にシーンの一覧、中央に本文のエディタ、右にシーンのグラフ。
// 見た目の作り込み、保存、ファイル監視はしない（計画書 T0-7「やらないこと」）。

import { useCallback, useEffect, useRef, useState } from "react";
import { devProjectPath, openProject, parseBuffer } from "./core/api.ts";
import { PositionMap } from "./core/position.ts";
import type { Bracket, Ir } from "./core/types.ts";
import { type Analysis, Editor, type Focus } from "./editor/Editor.tsx";
import { SceneGraph } from "./graph/SceneGraph.tsx";
import { SceneList } from "./scenes/SceneList.tsx";

interface ProjectView {
  readonly root: string;
  readonly brackets: readonly Bracket[];
  readonly ir: Ir;
}

function analysisFor(ir: Ir, file: string, text: string): Analysis {
  return {
    file,
    text,
    diagnostics: ir.diagnostics.filter((d) => d.file === file),
  };
}

export function App() {
  const [project, setProject] = useState<ProjectView | null>(null);
  const [currentFile, setCurrentFile] = useState<string | null>(null);
  const [analysis, setAnalysis] = useState<Analysis | null>(null);
  const [focus, setFocus] = useState<Focus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pathInput, setPathInput] = useState("");
  /** 保存していない本文（ファイルごと）。コアの状態と同じものを持つ。 */
  const buffers = useRef(new Map<string, string>());
  /** 解析の依頼の通し番号。古い結果を捨てるのに使う。 */
  const request = useRef(0);
  /** プロジェクトを開いた回数。開くたびにエディタを作り直す。 */
  const [openSeq, setOpenSeq] = useState(0);
  /** 変換中か。変換中に届いた解析の結果は、変換が終わってから画面に反映する。 */
  const composing = useRef(false);
  const pending = useRef<{ ir: Ir; analysis: Analysis } | null>(null);

  const showResult = useCallback((ir: Ir, result: Analysis) => {
    setProject((p) => (p === null ? null : { ...p, ir }));
    setAnalysis(result);
  }, []);

  const open = useCallback(async (path: string) => {
    try {
      const opened = await openProject(path);
      buffers.current = new Map(opened.files.map((f) => [f.path, f.text]));
      const first = opened.files[0]?.path ?? null;
      pending.current = null;
      setOpenSeq((n) => n + 1);
      setFocus(null);
      setProject({
        root: opened.root,
        brackets: opened.brackets,
        ir: opened.ir,
      });
      setCurrentFile(first);
      setAnalysis(
        first === null
          ? null
          : analysisFor(opened.ir, first, buffers.current.get(first) ?? ""),
      );
      setPathInput(opened.root);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  // 開発用：起動時にサンプル作品を開く（IMEテストのため）。
  useEffect(() => {
    devProjectPath()
      .then((path) => {
        if (path !== null) return open(path);
      })
      .catch((e: unknown) => setError(`コアに接続できません：${String(e)}`));
  }, [open]);

  const onIdle = useCallback(
    async (file: string, text: string) => {
      buffers.current.set(file, text);
      const seq = ++request.current;
      try {
        const ir = await parseBuffer(file, text);
        if (seq !== request.current) return;
        const result = analysisFor(ir, file, text);
        // 変換中は、グラフ・一覧・診断の表示を変えない。
        if (composing.current) pending.current = { ir, analysis: result };
        else showResult(ir, result);
        setError(null);
      } catch (e) {
        setError(String(e));
      }
    },
    [showResult],
  );

  const onComposingChange = useCallback(
    (value: boolean) => {
      composing.current = value;
      const waiting = pending.current;
      if (!value && waiting !== null) {
        pending.current = null;
        showResult(waiting.ir, waiting.analysis);
      }
    },
    [showResult],
  );

  const selectScene = useCallback(
    (index: number) => {
      const scene = project?.ir.scenes[index];
      if (scene === undefined) return;
      const text = buffers.current.get(scene.file) ?? "";
      setCurrentFile(scene.file);
      if (scene.file !== currentFile && project !== null) {
        setAnalysis(analysisFor(project.ir, scene.file, text));
      }
      setFocus((f) => ({
        offset: new PositionMap(text).byteToUtf16(scene.span[0]),
        seq: (f?.seq ?? 0) + 1,
      }));
    },
    [project, currentFile],
  );

  const counts = { error: 0, warning: 0, info: 0 };
  for (const d of project?.ir.diagnostics ?? []) counts[d.severity] += 1;

  return (
    <div className="app">
      <header className="titlebar">
        <span className="app-name">scn-studio（仮称）</span>
        <form
          className="open-form"
          onSubmit={(event) => {
            event.preventDefault();
            void open(pathInput);
          }}
        >
          <input
            aria-label="プロジェクトのフォルダ"
            placeholder="プロジェクトのフォルダのパス"
            value={pathInput}
            onChange={(event) => setPathInput(event.target.value)}
          />
          <button type="submit">開く</button>
        </form>
      </header>
      {error !== null && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      <main className="workspace">
        {project !== null && (
          <SceneList
            ir={project.ir}
            currentFile={currentFile}
            onSelect={selectScene}
          />
        )}
        <section className="editor-pane" aria-label="本文">
          {project !== null && currentFile !== null ? (
            <Editor
              key={openSeq}
              file={currentFile}
              text={buffers.current.get(currentFile) ?? ""}
              brackets={project.brackets}
              analysis={analysis}
              focus={focus}
              onIdle={onIdle}
              onComposingChange={onComposingChange}
            />
          ) : (
            <p className="placeholder">プロジェクトを開いてください。</p>
          )}
        </section>
        {project !== null && (
          <SceneGraph
            ir={project.ir}
            currentFile={currentFile}
            onSelect={selectScene}
          />
        )}
      </main>
      <footer className="statusbar">
        <span>{currentFile ?? "ファイル未選択"}</span>
        <span>
          エラー {counts.error}・警告 {counts.warning}・情報 {counts.info}
        </span>
        <span>形式 v0</span>
      </footer>
    </div>
  );
}
