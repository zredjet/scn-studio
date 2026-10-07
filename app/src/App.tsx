// アプリの骨格（docs/ui/README.md）：タイトルバー、左端のバー、画面（本文・フロー・検証）、ステータスバー。
// 作品フォルダを開く・作る・保存する。外部の変更（Claude Codeなどの書き換え）は、コアがファイルを監視して
// 取り込み、`workspace-changed` で知らせてくる（T1-6、D045）。
// 変換中は画面を変えない（D039）：解析の結果、外部の変更の知らせ、保存の結果で表示が変わるものは、
// 変換が終わってから反映する（whenIdle）。

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { CheckList } from "./check/CheckList.tsx";
import { countBySeverity } from "./check/group.ts";
import {
  addChapter,
  confirm,
  createProject,
  devReport,
  devSettings,
  guardClose,
  keepMine,
  onWorkspaceChanged,
  onWorkspaceError,
  openProject,
  parseBuffer,
  pickNewProjectFolder,
  pickProjectFolder,
  recentProjects,
  reloadFile,
  saveFile,
} from "./core/api.ts";
import { PositionMap } from "./core/position.ts";
import type {
  Bracket,
  IrDiagnostic,
  OpenedProject,
  RecentProject,
  SceneSummary,
  Summary,
  WorkspaceChanged,
} from "./core/types.ts";
import { ConflictBar } from "./editor/ConflictBar.tsx";
import {
  type Analysis,
  Editor,
  type Focus,
  type Replace,
} from "./editor/Editor.tsx";
import { LineBreakBar } from "./editor/LineBreakBar.tsx";
import { hasMixedLineBreaks, lineBreakOf } from "./editor/state.ts";
import { SceneGraph } from "./graph/SceneGraph.tsx";
import { SceneList } from "./scenes/SceneList.tsx";
import { LeftBar, type Screen } from "./shell/LeftBar.tsx";
import { StatusBar } from "./shell/StatusBar.tsx";
import { TitleBar } from "./shell/TitleBar.tsx";
import { Welcome } from "./shell/Welcome.tsx";

interface ProjectView {
  readonly root: string;
  readonly title: string;
  readonly brackets: readonly Bracket[];
  readonly summary: Summary;
  /** 検証の段階1の診断。 */
  readonly checks: readonly IrDiagnostic[];
  /** 本文のファイル（story/*.scn）。 */
  readonly files: readonly string[];
}

interface Conflict {
  readonly deleted: boolean;
}

function analysisFor(summary: Summary, file: string, text: string): Analysis {
  return {
    file,
    text,
    diagnostics: summary.diagnostics.filter((d) => d.file === file),
  };
}

function withFile(files: readonly string[], file: string): string[] {
  return files.includes(file) ? [...files] : [...files, file].sort();
}

function withoutFiles(
  set: ReadonlySet<string>,
  files: ReadonlySet<string>,
): ReadonlySet<string> {
  if (![...files].some((f) => set.has(f))) return set;
  return new Set([...set].filter((f) => !files.has(f)));
}

function withoutConflict(
  map: ReadonlyMap<string, Conflict>,
  file: string,
): ReadonlyMap<string, Conflict> {
  if (!map.has(file)) return map;
  const next = new Map(map);
  next.delete(file);
  return next;
}

export function App() {
  const [project, setProject] = useState<ProjectView | null>(null);
  const [screen, setScreen] = useState<Screen>("text");
  const [currentFile, setCurrentFile] = useState<string | null>(null);
  const [analysis, setAnalysis] = useState<Analysis | null>(null);
  const [focus, setFocus] = useState<Focus | null>(null);
  const [replace, setReplace] = useState<Replace | null>(null);
  const [dirty, setDirty] = useState<ReadonlySet<string>>(new Set());
  const [conflicts, setConflicts] = useState<ReadonlyMap<string, Conflict>>(
    new Map(),
  );
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [recent, setRecent] = useState<RecentProject[]>([]);
  /** ディスクの中身が変わった回数（エディタに渡す savedText を作り直すため）。 */
  const [savedVersion, setSavedVersion] = useState(0);
  /** プロジェクトを開いた回数。開くたびにエディタを作り直す。 */
  const [openSeq, setOpenSeq] = useState(0);

  /** 本文の今の中身（ファイルごと。保存していない編集を含む）。コアの状態と同じものを持つ。 */
  const buffers = useRef(new Map<string, string>());
  /** ディスクの中身（ファイルごと）。 */
  const saved = useRef(new Map<string, string>());
  /** 解析の依頼の通し番号。古い結果を捨てるのに使う。 */
  const request = useRef(0);
  /** 置き換えの通し番号。 */
  const replaceSeq = useRef(0);
  /** 変換中か。変換中に届いたもので表示が変わるものは、変換が終わってから反映する。 */
  const composing = useRef(false);
  /** 変換が終わってから行う、表示の更新。 */
  const afterComposition = useRef<(() => void)[]>([]);
  // 知らせの受け取りや、閉じる前の確認の中では、最新の値を読む。
  const latest = useRef({ project, currentFile, dirty, conflicts });
  latest.current = { project, currentFile, dirty, conflicts };

  const replaceText = useCallback(
    (file: string, text: string, expected: string | null) => {
      replaceSeq.current += 1;
      setReplace({ file, text, expected, seq: replaceSeq.current });
    },
    [],
  );

  /** 表示を変える。変換中なら、変換が終わるまで待つ（D039）。 */
  const whenIdle = useCallback((update: () => void) => {
    if (composing.current) afterComposition.current.push(update);
    else update();
  }, []);

  /** 解析の結果を画面に出す。変換中なら、変換が終わるまで待つ。 */
  const showSummary = useCallback(
    (summary: Summary, checks?: readonly IrDiagnostic[]) => {
      const file = latest.current.currentFile;
      const result =
        file === null
          ? null
          : analysisFor(summary, file, buffers.current.get(file) ?? "");
      whenIdle(() => {
        setProject((p) =>
          p === null ? null : { ...p, summary, checks: checks ?? p.checks },
        );
        setAnalysis(result);
      });
    },
    [whenIdle],
  );

  /** 外で消されたファイルを閉じる。 */
  const closeFile = useCallback((file: string) => {
    buffers.current.delete(file);
    saved.current.delete(file);
    const rest = (latest.current.project?.files ?? []).filter(
      (f) => f !== file,
    );
    setProject((p) =>
      p === null ? null : { ...p, files: p.files.filter((f) => f !== file) },
    );
    setDirty((set) => withoutFiles(set, new Set([file])));
    setConflicts((map) => withoutConflict(map, file));
    if (latest.current.currentFile === file) setCurrentFile(rest[0] ?? null);
  }, []);

  const load = useCallback((opened: OpenedProject) => {
    buffers.current = new Map(opened.files.map((f) => [f.path, f.text]));
    saved.current = new Map(opened.files.map((f) => [f.path, f.text]));
    const files = opened.files.map((f) => f.path);
    const first = files[0] ?? null;
    afterComposition.current = [];
    setOpenSeq((n) => n + 1);
    setFocus(null);
    setReplace(null);
    setDirty(new Set());
    setConflicts(new Map());
    setProject({
      root: opened.root,
      title: opened.title,
      brackets: opened.brackets,
      summary: opened.summary,
      checks: opened.checks,
      files,
    });
    setCurrentFile(first);
    setAnalysis(
      first === null
        ? null
        : analysisFor(opened.summary, first, buffers.current.get(first) ?? ""),
    );
    setScreen("text");
    setError(null);
    setNotice(`${opened.title} を開きました`);
    void recentProjects().then(setRecent);
  }, []);

  /** 保存していない変更があれば、捨ててよいかを聞く。 */
  const confirmDiscard = useCallback(async () => {
    if (latest.current.dirty.size === 0) return true;
    return confirm(
      "保存していない変更があります。保存せずに続けますか？",
      "保存していない変更",
    );
  }, []);

  const open = useCallback(
    async (path: string) => {
      if (!(await confirmDiscard())) return;
      try {
        load(await openProject(path));
      } catch (e) {
        setError(String(e));
      }
    },
    [confirmDiscard, load],
  );

  const openPicker = useCallback(async () => {
    const path = await pickProjectFolder();
    if (path !== null) await open(path);
  }, [open]);

  const create = useCallback(async () => {
    if (!(await confirmDiscard())) return;
    const path = await pickNewProjectFolder();
    if (path === null) return;
    try {
      load(await createProject(path));
    } catch (e) {
      setError(String(e));
    }
  }, [confirmDiscard, load]);

  // 起動時：最近開いたものを読み、開発用の作品フォルダがあれば開く（IMEテストと、受け渡しの時間を測るため）。
  useEffect(() => {
    void recentProjects().then(setRecent);
    devSettings()
      .then(async (dev) => {
        if (dev.project === null) return;
        const opened = await openProject(dev.project);
        load(opened);
        if (dev.measure_ipc) await measureIpc(opened);
      })
      .catch((e: unknown) => setError(`コアに接続できません：${String(e)}`));
  }, [load]);

  // 閉じる前に、保存していない変更があれば聞く。
  useEffect(() => {
    const unlisten = guardClose(confirmDiscard);
    return () => {
      void unlisten.then((f) => f());
    };
  }, [confirmDiscard]);

  // 外部の変更の知らせ。
  useEffect(() => {
    // 変換中は取り込まず（表示を変えず）、変換が終わってから取り込む。取り込むときの「まだ送っていない
    // 編集があるか」は、そのときのエディタで決める。
    const onChanged = (event: WorkspaceChanged) => {
      const { currentFile: current, dirty: dirtyNow } = latest.current;
      let files = latest.current.project?.files ?? [];
      const newConflicts = new Map(latest.current.conflicts);
      const closed = new Set<string>();
      const changed: string[] = [];
      const unreadable: string[] = [];
      for (const change of event.changes) {
        switch (change.kind) {
          case "reloaded":
          case "added": {
            const before = saved.current.get(change.file);
            files = withFile(files, change.file);
            saved.current.set(change.file, change.text);
            if (dirtyNow.has(change.file)) {
              // コアが読み込み直す前に、まだ送っていない編集があった。コアに自分の編集を戻し、選ばせる。
              newConflicts.set(change.file, { deleted: false });
              const mine = buffers.current.get(change.file);
              if (mine !== undefined)
                void parseBuffer(change.file, mine).catch(() => {});
            } else {
              buffers.current.set(change.file, change.text);
              if (change.file === current && before !== undefined)
                replaceText(change.file, change.text, before);
            }
            changed.push(change.file);
            break;
          }
          case "removed":
            if (dirtyNow.has(change.file)) {
              // コアが閉じる前に、まだ送っていない編集があった。閉じるか、残して保存で作り直すかを選ばせる。
              saved.current.delete(change.file);
              newConflicts.set(change.file, { deleted: true });
            } else {
              buffers.current.delete(change.file);
              saved.current.delete(change.file);
              files = files.filter((f) => f !== change.file);
              newConflicts.delete(change.file);
              closed.add(change.file);
            }
            changed.push(change.file);
            break;
          case "conflict":
            newConflicts.set(change.file, { deleted: change.deleted });
            changed.push(change.file);
            break;
          case "unreadable":
            unreadable.push(change.file);
            break;
          case "settings":
            changed.push("設定ファイル");
            break;
        }
      }
      setConflicts(newConflicts);
      if (closed.size > 0) setDirty((set) => withoutFiles(set, closed));
      if (current !== null && closed.has(current))
        setCurrentFile(files[0] ?? null);
      setSavedVersion((n) => n + 1);
      setProject((p) =>
        p === null
          ? null
          : {
              ...p,
              title: event.title,
              brackets: event.brackets,
              files,
            },
      );
      showSummary(event.summary, event.checks);
      if (changed.length > 0)
        setNotice(`外部の変更を取り込みました：${changed.join("、")}`);
      if (unreadable.length > 0)
        setError(
          `${unreadable.join("、")} をUTF-8で読めないので、外部の変更を取り込んでいません`,
        );
    };
    const unlistenChanged = onWorkspaceChanged((event) =>
      whenIdle(() => onChanged(event)),
    );
    const unlistenError = onWorkspaceError((message) =>
      whenIdle(() => setError(message)),
    );
    return () => {
      void unlistenChanged.then((f) => f());
      void unlistenError.then((f) => f());
    };
  }, [replaceText, showSummary, whenIdle]);

  const onIdle = useCallback(
    async (file: string, text: string) => {
      // 閉じたファイルの、まだ送っていなかった編集は捨てる。
      if (!buffers.current.has(file)) return;
      buffers.current.set(file, text);
      // 外で消されたのに残っているファイル（選ぶまで、コアは閉じている）は、選んだときに送る。
      if (latest.current.conflicts.get(file)?.deleted === true) return;
      const seq = ++request.current;
      try {
        const summary = await parseBuffer(file, text);
        if (seq !== request.current) return;
        showSummary(summary);
        whenIdle(() => setError(null));
      } catch (e) {
        whenIdle(() => setError(String(e)));
      }
    },
    [showSummary, whenIdle],
  );

  const onComposingChange = useCallback((value: boolean) => {
    composing.current = value;
    if (value) return;
    const waiting = afterComposition.current;
    afterComposition.current = [];
    for (const update of waiting) update();
  }, []);

  const onDirty = useCallback((file: string, value: boolean) => {
    if (!buffers.current.has(file)) return;
    setDirty((set) => {
      if (set.has(file) === value) return set;
      const next = new Set(set);
      if (value) next.add(file);
      else next.delete(file);
      return next;
    });
  }, []);

  /** 「自分の変更を残す」。ディスクの中身（外の中身）を覚え直す。 */
  const keep = useCallback(
    async (file: string, text: string) => {
      const kept = await keepMine(file, text);
      if (kept.disk === null) saved.current.delete(file);
      else saved.current.set(file, kept.disk);
      setConflicts((map) => withoutConflict(map, file));
      setSavedVersion((n) => n + 1);
      showSummary(kept.summary, kept.checks);
    },
    [showSummary],
  );

  /** 保存する（エディタが1つずつ呼ぶ）。書いた本文（付いた行IDを含む）を返す。 */
  const onSave = useCallback(
    async (file: string, text: string): Promise<string | null> => {
      buffers.current.set(file, text);
      try {
        // 外部の変更とぶつかっていたら、保存は「自分の変更を残す」ことにする。
        if (latest.current.conflicts.has(file)) await keep(file, text);
        const result = await saveFile(
          file,
          text,
          saved.current.get(file) ?? null,
        );
        if (result.kind === "conflict") {
          // まだ取り込んでいない外部の変更があったので、書かなかった。選ばせる。
          setConflicts((map) =>
            new Map(map).set(file, { deleted: result.deleted }),
          );
          whenIdle(() =>
            setNotice(
              `${file} は外部で${result.deleted ? "削除" : "変更"}されていたので、保存していません`,
            ),
          );
          return null;
        }
        saved.current.set(file, result.text);
        // 保存を待つ間に編集を送っていなければ、書いた本文が今の本文。送っていれば、エディタが行IDを入れて送り直す。
        if (buffers.current.get(file) === text)
          buffers.current.set(file, result.text);
        setSavedVersion((n) => n + 1);
        showSummary(result.summary, result.checks);
        whenIdle(() => {
          setNotice(
            result.assigned > 0
              ? `${file} を保存しました（行IDを ${result.assigned} 行に付けました）`
              : `${file} を保存しました`,
          );
          setError(null);
        });
        return result.text;
      } catch (e) {
        whenIdle(() => setError(String(e)));
        return null;
      }
    },
    [keep, showSummary, whenIdle],
  );

  /** 読み込み直しの知らせの後、置き換える前に編集していた。コアに自分の編集を戻し、選ばせる。 */
  const onReplaceSkipped = useCallback((file: string, text: string) => {
    buffers.current.set(file, text);
    void parseBuffer(file, text).catch(() => {});
    setConflicts((map) => new Map(map).set(file, { deleted: false }));
  }, []);

  const resolveConflict = useCallback(
    async (file: string, reload: boolean) => {
      try {
        if (reload) {
          const result = await reloadFile(file);
          if (result.text === null) {
            closeFile(file);
          } else {
            buffers.current.set(file, result.text);
            saved.current.set(file, result.text);
            if (latest.current.currentFile === file)
              replaceText(file, result.text, null);
            setConflicts((map) => withoutConflict(map, file));
          }
          setSavedVersion((n) => n + 1);
          showSummary(result.summary, result.checks);
        } else {
          await keep(file, buffers.current.get(file) ?? "");
        }
      } catch (e) {
        setError(String(e));
      }
    },
    [closeFile, keep, replaceText, showSummary],
  );

  const onAddChapter = useCallback(
    async (name: string): Promise<string | null> => {
      try {
        const result = await addChapter(name);
        buffers.current.set(result.file, "");
        saved.current.set(result.file, "");
        setProject((p) =>
          p === null ? null : { ...p, files: withFile(p.files, result.file) },
        );
        showSummary(result.summary, result.checks);
        setCurrentFile(result.file);
        return null;
      } catch (e) {
        return String(e);
      }
    },
    [showSummary],
  );

  /** 本文の画面で、あるファイルのある位置へ移る。 */
  const goTo = useCallback(
    (file: string, byteOffset: number | null) => {
      const text = buffers.current.get(file);
      if (text === undefined || project === null) return;
      setScreen("text");
      setCurrentFile(file);
      if (file !== currentFile)
        setAnalysis(analysisFor(project.summary, file, text));
      if (byteOffset !== null) {
        setFocus((f) => ({
          offset: new PositionMap(text).byteToUtf16(byteOffset),
          seq: (f?.seq ?? 0) + 1,
        }));
      }
    },
    [project, currentFile],
  );

  const selectScene = useCallback(
    (scene: SceneSummary) => goTo(scene.file, scene.span[0]),
    [goTo],
  );

  const allDiagnostics = useMemo(
    () =>
      project === null
        ? []
        : [...project.summary.diagnostics, ...project.checks],
    [project],
  );
  const counts = countBySeverity(allDiagnostics);
  const conflict =
    currentFile === null ? undefined : conflicts.get(currentFile);
  const savedText =
    currentFile === null ? "" : (saved.current.get(currentFile) ?? "");
  // savedVersion が変わるたびに、エディタへ渡すディスクの中身を読み直す。
  void savedVersion;
  const mixedLineBreak = useMemo(
    () => (hasMixedLineBreaks(savedText) ? lineBreakOf(savedText) : null),
    [savedText],
  );

  return (
    <div className="app">
      <TitleBar
        title={project?.title ?? null}
        recent={recent}
        onOpen={() => void openPicker()}
        onCreate={() => void create()}
        onOpenRecent={(path) => void open(path)}
      />
      {error !== null && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      <div className="body">
        {project !== null && (
          <LeftBar
            screen={screen}
            problems={counts.error + counts.warning}
            onSelect={setScreen}
          />
        )}
        <main className="workspace">
          {project === null ? (
            <Welcome
              recent={recent}
              onOpen={() => void openPicker()}
              onCreate={() => void create()}
              onOpenRecent={(path) => void open(path)}
            />
          ) : (
            <>
              <section
                className="screen text-screen"
                hidden={screen !== "text"}
                aria-label="本文"
              >
                <SceneList
                  files={project.files}
                  scenes={project.summary.scenes}
                  currentFile={currentFile}
                  dirty={dirty}
                  onSelectScene={selectScene}
                  onSelectFile={(file) => goTo(file, null)}
                  onAddChapter={onAddChapter}
                />
                <section className="editor-pane" aria-label="エディタ">
                  {currentFile !== null && conflict !== undefined && (
                    <ConflictBar
                      file={currentFile}
                      deleted={conflict.deleted}
                      onReload={() => void resolveConflict(currentFile, true)}
                      onKeep={() => void resolveConflict(currentFile, false)}
                    />
                  )}
                  {currentFile !== null && mixedLineBreak !== null && (
                    <LineBreakBar lineBreak={mixedLineBreak} />
                  )}
                  {currentFile !== null ? (
                    <Editor
                      key={openSeq}
                      file={currentFile}
                      text={buffers.current.get(currentFile) ?? ""}
                      savedText={savedText}
                      brackets={project.brackets}
                      analysis={analysis}
                      focus={focus}
                      replace={replace}
                      onIdle={onIdle}
                      onComposingChange={onComposingChange}
                      onDirty={onDirty}
                      onSave={onSave}
                      onReplaceSkipped={onReplaceSkipped}
                    />
                  ) : (
                    <p className="placeholder">
                      章がありません。左の「＋ 章」で足してください。
                    </p>
                  )}
                </section>
                <aside
                  className="context-panel"
                  aria-label="このファイルの問題"
                >
                  <CheckList
                    heading="このファイルの問題"
                    diagnostics={allDiagnostics.filter(
                      (d) => d.file === currentFile,
                    )}
                    files={project.files}
                    onSelect={(d) => goTo(d.file, d.span[0])}
                  />
                </aside>
              </section>
              {screen === "flow" && (
                <section className="screen flow-screen" aria-label="フロー">
                  <SceneGraph
                    ir={project.summary}
                    currentFile={currentFile}
                    onSelect={selectScene}
                  />
                </section>
              )}
              {screen === "check" && (
                <section className="screen check-screen" aria-label="検証">
                  <CheckList
                    heading="検証（プロジェクト全体）"
                    diagnostics={allDiagnostics}
                    files={project.files}
                    onSelect={(d) => goTo(d.file, d.span[0])}
                  />
                </section>
              )}
            </>
          )}
        </main>
      </div>
      <StatusBar
        file={currentFile}
        unsaved={dirty.size}
        counts={counts}
        notice={notice}
      />
    </div>
  );
}

/** 受け渡しの時間を測ったか。 */
let measured = false;

/**
 * 開発用：入力が止まったときと同じ受け渡し（parse_buffer）を何回か行い、かかった時間をターミナルに書く
 * （環境変数 `SCN_MEASURE_IPC` のとき。D045）。
 */
async function measureIpc(opened: OpenedProject) {
  // 開発時の React は効果を2回走らせるので、測るのは1回だけにする（同時に測ると、解析を待ち合って遅く出る）。
  if (measured) return;
  measured = true;
  const first = opened.files[0];
  if (first === undefined) return;
  const lines = opened.files.reduce((n, f) => n + f.text.split("\n").length, 0);
  const times: number[] = [];
  let size = 0;
  for (let i = 0; i < 10; i++) {
    const start = performance.now();
    const summary = await parseBuffer(first.path, first.text);
    times.push(performance.now() - start);
    if (i === 0) size = JSON.stringify(summary).length;
  }
  times.sort((a, b) => a - b);
  await devReport(
    `受け渡しの時間（parse_buffer、${opened.files.length}ファイル、${lines}行、表示に要る分のJSON 約${size}文字）：` +
      `中央値 ${times[5]?.toFixed(1)}ミリ秒、最小 ${times[0]?.toFixed(1)}、最大 ${times[9]?.toFixed(1)}`,
  );
}
