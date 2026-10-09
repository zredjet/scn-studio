// AI実行の画面（左端のバーの「AI」。docs/ui/mockups/07-ai.html、T3-7、D068）：段階ごとのタスクの一覧と実行の履歴、
// 選んだタスクの実行（送る中身の確認、取り消し、進み具合）と結果（指摘、シーンの要約、提案、生成の下書き）、実行の
// 設定。実行と指摘は App が持つ（画面を切り替えても続く）。自律は T3-9。プロットとフローへの支援（T3-10）：ルートの
// 骨格（対象のルートと書く章）、分岐案（提案ノード）、同期（要約のずれ）、メモの分類案。

import { useEffect, useRef, useState } from "react";
import type {
  AiBranches,
  AiConnectionChoice,
  AiPrepared,
  AiProposalFile,
  AiProposalView,
  AiTaskInfo,
  AutoResult,
  SceneSummary,
} from "../core/types.ts";
import { AutoPanel } from "./AutoPanel.tsx";
import { BranchProposals } from "./BranchProposals.tsx";
import {
  type AutoRun,
  blocker,
  connectionOptions,
  estimateText,
  type FindingRecord,
  fileProposalSet,
  groupTasks,
  type HistoryEntry,
  isSkeleton,
  LEVEL_LABEL,
  needsScene,
  type PlotAi,
  type ProposalSet,
  placeText,
  preparedFor,
  type RunResult,
  type RunState,
  routeNames,
  SEVERITY_LABEL,
  type SkeletonTarget,
  trimmedText,
} from "./model.ts";
import { ProposalPanel } from "./ProposalPanel.tsx";
import { SyncPanel } from "./SyncPanel.tsx";

/** 書く章の選択肢で、新しい章を作るもの。 */
const NEW_CHAPTER = "\u0000new";

/** 却下が無いとき。 */
const NO_REJECTED: ReadonlySet<number> = new Set();

interface Props {
  readonly tasks: readonly AiTaskInfo[] | null;
  readonly tasksError: string | null;
  /** タスクに選べる接続先（T4-1）。 */
  readonly connections: readonly AiConnectionChoice[];
  /** この端末で、タスクに使う接続先を選ぶ（null なら定義のまま。T4-1）。 */
  readonly onChooseConnection: (
    task: string,
    connection: string | null,
  ) => Promise<void>;
  readonly scenes: readonly SceneSummary[];
  /** エディタの今のシーン（対象のシーンの初めの値）。 */
  readonly currentScene: string | null;
  readonly running: RunState | null;
  /** タスクごとの最後の結果。 */
  readonly results: ReadonlyMap<string, RunResult>;
  readonly history: readonly HistoryEntry[];
  readonly findings: readonly FindingRecord[];
  readonly onPrepare: (
    task: string,
    scene: string | null,
    route?: string | null,
  ) => Promise<AiPrepared>;
  /**
   * 実行する。`digest` は確かめた送る中身の要約値（クラウドの接続先で要る）。`target` はルートの骨格を書く先
   * （T3-10）。
   */
  readonly onRun: (
    info: AiTaskInfo,
    scene: string | null,
    digest: string | null,
    target?: SkeletonTarget,
  ) => void;
  readonly onCancel: () => void;
  readonly onGoToFinding: (record: FindingRecord) => void;
  readonly onDismissFinding: (id: string) => void;
  readonly onOpenPreferences: () => void;
  /** 外部のAIエージェントが置いた提案（`ai/proposals/*.md`。T3-8）。 */
  readonly proposalFiles: readonly AiProposalFile[];
  /** 本文が変わった回数（提案を照らし直す）。 */
  readonly textVersion: number;
  /** 提案を採用する。結果の文を返す。 */
  readonly onAdopt: (
    set: ProposalSet,
    views: readonly AiProposalView[],
  ) => Promise<string>;
  readonly onGoToProposal: (view: AiProposalView) => void;
  /** 却下した提案（まとまりの id ごと）。 */
  readonly rejected: ReadonlyMap<string, ReadonlySet<number>>;
  readonly onToggleReject: (setId: string, index: number) => void;
  /** 自律（T3-9）：走っている回、最後の結果、誤り、実行と取り消し。 */
  readonly autoRun: AutoRun | null;
  readonly autoResult: AutoResult | null;
  readonly autoError: string | null;
  readonly onRunAuto: (
    info: AiTaskInfo,
    file: string,
    branch: boolean,
    confirmedCloud: readonly string[],
  ) => void;
  readonly onCancelAuto: () => void;
  /** AI のブランチ（Git）と、差分・取り込み・破棄。 */
  readonly branches: AiBranches | null;
  readonly onBranchDiff: (
    name: string,
  ) => Promise<{ base: string; stat: string; patch: string }>;
  readonly onBranchMerge: (name: string) => Promise<void>;
  /** 捨てる（人が確かめなければ false）。 */
  readonly onBranchDelete: (name: string) => Promise<boolean>;
  /** ほかの画面から開くタスク（`seq` が変わったら選ぶ。T3-10）。 */
  readonly request: { readonly task: string; readonly seq: number } | null;
  /** ルートの骨格を、LLM を呼ばずに書き直す（前の回の骨格。T3-10）。 */
  readonly onRewriteSkeleton: (
    info: AiTaskInfo,
    target: SkeletonTarget,
  ) => void;
  /** プロットとフローへの支援（T3-10）。 */
  readonly plot: PlotAi;
}

export function AiScreen({
  tasks,
  tasksError,
  connections,
  onChooseConnection,
  scenes,
  currentScene,
  running,
  results,
  history,
  findings,
  onPrepare,
  onRun,
  onCancel,
  onGoToFinding,
  onDismissFinding,
  onOpenPreferences,
  proposalFiles,
  textVersion,
  onAdopt,
  onGoToProposal,
  rejected,
  onToggleReject,
  autoRun,
  autoResult,
  autoError,
  onRunAuto,
  onCancelAuto,
  branches,
  onBranchDiff,
  onBranchMerge,
  onBranchDelete,
  request,
  onRewriteSkeleton,
  plot,
}: Props) {
  const [selected, setSelected] = useState<string | null>(null);
  /** 開いている提案ファイル（選んでいればタスクの代わりに出す）。 */
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [scene, setScene] = useState<string | null>(currentScene);
  const [prepared, setPrepared] = useState<AiPrepared | null>(null);
  const [preparing, setPreparing] = useState(false);
  const [prepareError, setPrepareError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  /** タスクの接続先を書いている間（T4-1）と、書けなかったわけ。 */
  const [choosing, setChoosing] = useState(false);
  const [chooseError, setChooseError] = useState<string | null>(null);
  /** ルートの骨格（T3-10）：対象のルート、書く章（`NEW_CHAPTER` なら新しい章）、新しい章の名前。 */
  const [route, setRoute] = useState("");
  const [chapter, setChapter] = useState(plot.chapters[0] ?? "");
  const [newChapter, setNewChapter] = useState("");
  /** 今のタスクと対象（見る中身の応答が、頼んだ後に変わっていたら捨てる）。 */
  const current = useRef("");
  const info = tasks?.find((t) => t.task.id === selected) ?? tasks?.[0] ?? null;
  // ほかの画面から頼まれたタスクを選ぶ。
  useEffect(() => {
    if (request === null) return;
    setSelected(request.task);
    setSelectedFile(null);
  }, [request]);
  // タスクか対象（シーン、骨格のルート）を変えたら、見た中身は古い。
  useEffect(() => {
    void info?.task.id;
    void scene;
    void route;
    setPrepared(null);
    setPrepareError(null);
    setConfirming(false);
  }, [info?.task.id, scene, route]);

  if (tasks === null) {
    return (
      <div className="ai-screen-body">
        <p className="context-empty">{tasksError ?? "読み込んでいます…"}</p>
      </div>
    );
  }
  if (tasks.length === 0 || info === null) {
    return (
      <div className="ai-screen-body">
        <p className="context-empty">
          AIタスクの定義（ai/*.yaml）がありません。scn new
          で作った作品フォルダには、組み込みのタスクがあります。
        </p>
      </div>
    );
  }
  const task = info.task;
  const openFile =
    selectedFile === null
      ? null
      : (proposalFiles.find((f) => f.path === selectedFile) ?? null);
  const sceneNeeded = needsScene(task);
  const target = sceneNeeded ? scene : null;
  const skeleton = isSkeleton(task);
  const skeletonTarget: SkeletonTarget | undefined = skeleton
    ? {
        route: route.trim() === "" ? null : route.trim(),
        file: chapter === NEW_CHAPTER || chapter === "" ? null : chapter,
        newChapter: chapter === NEW_CHAPTER ? newChapter.trim() : null,
      }
    : undefined;
  const skeletonMissing =
    skeletonTarget !== undefined &&
    (skeletonTarget.route === null ||
      (skeletonTarget.file === null &&
        (skeletonTarget.newChapter ?? "") === ""));
  current.current = `${task.id}\n${target ?? ""}\n${skeletonTarget?.route ?? ""}`;
  const shown = preparedFor(prepared, task.id, target) ? prepared : null;
  const blocked = blocker(info);
  const isRunning = running !== null;
  const thisRunning = running?.task === task.id;
  const result = results.get(task.id);
  const cloud = info.connection?.cloud === true;
  const prepare = () => {
    const key = current.current;
    setPreparing(true);
    setPrepareError(null);
    onPrepare(task.id, target, skeletonTarget?.route ?? null)
      .then((p) => {
        if (current.current === key) setPrepared(p);
      })
      .catch((e: unknown) => {
        if (current.current === key) setPrepareError(String(e));
      })
      .finally(() => setPreparing(false));
  };
  const runNow = () => {
    setConfirming(false);
    // クラウドでは、見せた中身の要約値を添える（コアが組み立て直して、同じときだけ送る）。
    onRun(info, target, shown?.digest ?? null, skeletonTarget);
  };
  const resultFindings =
    result === undefined
      ? []
      : findings.filter((f) => f.run === result.run && f.task === task.id);

  return (
    <div className="ai-screen-body">
      <aside className="ai-task-list" aria-label="AIタスク">
        <h2 className="context-heading">AIタスク</h2>
        {groupTasks(tasks).map((group) => (
          <div key={group.level} className="ai-task-group">
            <span className="ai-task-level">{group.label}</span>
            {group.tasks.map((t) => (
              <button
                key={t.task.id}
                type="button"
                className="ai-task-item"
                aria-pressed={selectedFile === null && t.task.id === task.id}
                onClick={() => {
                  setSelected(t.task.id);
                  setSelectedFile(null);
                }}
              >
                <span className="ai-task-name">{t.task.name}</span>
                <span className="ai-task-place">{placeText(t)}</span>
              </button>
            ))}
          </div>
        ))}
        {proposalFiles.length > 0 && (
          <>
            <h2 className="context-heading">提案ファイル（ai/proposals）</h2>
            <div className="ai-task-group">
              {proposalFiles.map((f) => (
                <button
                  key={f.path}
                  type="button"
                  className="ai-task-item"
                  aria-pressed={selectedFile === f.path}
                  onClick={() => setSelectedFile(f.path)}
                >
                  <span className="ai-task-name">
                    {f.path.replace(/^ai\/proposals\//, "")}
                  </span>
                  <span className="ai-task-place">{f.proposals.length}件</span>
                </button>
              ))}
            </div>
          </>
        )}
        <h2 className="context-heading">実行履歴</h2>
        {history.length === 0 ? (
          <p className="context-empty">まだ実行していません。</p>
        ) : (
          <ul className="ai-history">
            {history.map((h) => (
              <li key={h.run}>
                <span>{h.taskName}</span>
                <span className="context-faint">
                  {h.at} · {h.status}
                </span>
              </li>
            ))}
          </ul>
        )}
      </aside>
      {openFile !== null ? (
        <main className="ai-run-main" aria-label="提案ファイル">
          <header className="ai-run-head">
            <span className="ai-badge">提案</span>
            <h1>{openFile.path}</h1>
            <span className="context-faint">
              {openFile.task ?? "タスクの記載なし"} ·{" "}
              {openFile.model ?? "モデルの記載なし"}
              {openFile.at !== null ? ` · ${openFile.at}` : ""}
            </span>
          </header>
          <p className="context-faint ai-scope-note">
            外部のAIエージェントが置いた、人の書いた行への変更案です。採用した行だけを置き換え（行IDは変えません）、出自を
            {fileProposalSet(openFile).task}{" "}
            として記録します。却下はこの画面の中だけで、ファイルは書き換えません。
          </p>
          {openFile.problems.map((p) => (
            <p key={p} className="export-warning">
              {p}
            </p>
          ))}
          <ProposalPanel
            set={fileProposalSet(openFile)}
            version={textVersion}
            onAdopt={onAdopt}
            onGoTo={onGoToProposal}
            rejected={rejected.get(fileProposalSet(openFile).id) ?? NO_REJECTED}
            onToggleReject={onToggleReject}
          />
        </main>
      ) : (
        <>
          <main className="ai-run-main" aria-label="実行">
            <header className="ai-run-head">
              <span className="ai-badge">{LEVEL_LABEL[task.level]}</span>
              <h1>{task.name}</h1>
              <span className="context-faint">
                {info.connection === null
                  ? "接続先なし"
                  : `${info.connection.name} · ${info.connection.model}（${cloud ? "クラウド" : "ローカル"}）`}
              </span>
              <label className="ai-connection-choice">
                <span>この端末の接続先</span>
                <select
                  value={info.override ?? ""}
                  disabled={
                    choosing ||
                    preparing ||
                    running !== null ||
                    autoRun !== null
                  }
                  onChange={(e) => {
                    const value = e.target.value;
                    setChoosing(true);
                    setChooseError(null);
                    // 送る中身の確かめは、接続先ごと（選び直したら見直す）。
                    setPrepared(null);
                    setConfirming(false);
                    onChooseConnection(task.id, value === "" ? null : value)
                      .catch((err: unknown) => setChooseError(String(err)))
                      .finally(() => setChoosing(false));
                  }}
                >
                  {connectionOptions(info, connections).map((o) => (
                    <option key={o.value} value={o.value}>
                      {o.label}
                    </option>
                  ))}
                </select>
              </label>
            </header>
            {chooseError !== null && (
              <p className="export-warning">{chooseError}</p>
            )}
            {blocked !== null && (
              <p className="export-warning">
                {blocked}
                {(info.connection_problem !== null ||
                  (cloud && info.connection?.allowed === false)) && (
                  <>
                    {" "}
                    <button
                      type="button"
                      className="small-button"
                      onClick={onOpenPreferences}
                    >
                      環境設定を開く
                    </button>
                  </>
                )}
              </p>
            )}
            {task.level === "auto" ? (
              <AutoPanel
                key={task.id}
                info={info}
                checks={tasks.filter((t) => task.checks.includes(t.task.id))}
                scenes={scenes}
                run={autoRun}
                result={autoResult}
                error={autoError}
                busy={isRunning || autoRun !== null || choosing}
                blocked={blocked}
                branches={branches}
                onRun={onRunAuto}
                onCancel={onCancelAuto}
                onDiff={onBranchDiff}
                onMerge={onBranchMerge}
                onDelete={onBranchDelete}
              />
            ) : (
              <>
                {sceneNeeded && (
                  <label className="ai-scene-select">
                    <span>対象のシーン</span>
                    <select
                      value={scene ?? ""}
                      onChange={(e) =>
                        setScene(e.target.value === "" ? null : e.target.value)
                      }
                    >
                      <option value="">選んでください</option>
                      {scenes.map((s) => (
                        <option key={s.id} value={s.id}>
                          {s.id} {s.title}
                        </option>
                      ))}
                    </select>
                  </label>
                )}
                {skeleton && (
                  <div className="ai-skeleton-target">
                    <label className="ai-scene-select">
                      <span>対象のルート</span>
                      <input
                        type="text"
                        value={route}
                        list="ai-route-names"
                        placeholder="例：紬"
                        onChange={(e) => setRoute(e.target.value)}
                      />
                      <datalist id="ai-route-names">
                        {routeNames(scenes).map((r) => (
                          <option key={r} value={r} />
                        ))}
                      </datalist>
                    </label>
                    <label className="ai-scene-select">
                      <span>書く章</span>
                      <select
                        value={chapter}
                        onChange={(e) => setChapter(e.target.value)}
                      >
                        {plot.chapters.map((c) => (
                          <option key={c} value={c}>
                            {c}
                          </option>
                        ))}
                        <option value={NEW_CHAPTER}>新しい章を作る…</option>
                      </select>
                    </label>
                    {chapter === NEW_CHAPTER && (
                      <label className="ai-scene-select">
                        <span>新しい章の名前</span>
                        <input
                          type="text"
                          value={newChapter}
                          placeholder="例：route_tsumugi.scn"
                          onChange={(e) => setNewChapter(e.target.value)}
                        />
                      </label>
                    )}
                    <p className="context-faint ai-scope-note">
                      足すのはスタブのシーン（見出し、summary・route、選択肢とジャンプ）だけで、今あるシーンは変えません。足したシーンは
                      status: ai-draft
                      になります。入口の辺は、フロー図で引きます。
                    </p>
                  </div>
                )}
                <div className="ai-run-actions">
                  {thisRunning ? (
                    <>
                      <span className="ai-test-state" role="status">
                        実行中 · 受け取った文字{" "}
                        {running.chars.toLocaleString("ja-JP")}
                      </span>
                      <button
                        type="button"
                        className="small-button"
                        onClick={onCancel}
                      >
                        取り消す
                      </button>
                    </>
                  ) : (
                    <button
                      type="button"
                      className="primary-small ai-run-button"
                      disabled={
                        blocked !== null ||
                        isRunning ||
                        confirming ||
                        (sceneNeeded && scene === null) ||
                        skeletonMissing ||
                        shown?.assembled.problem != null
                      }
                      onClick={() => {
                        if (cloud) {
                          setConfirming(true);
                          if (shown === null) prepare();
                        } else runNow();
                      }}
                    >
                      実行
                    </button>
                  )}
                  <button
                    type="button"
                    className="small-button"
                    disabled={preparing || (sceneNeeded && scene === null)}
                    onClick={prepare}
                  >
                    送る中身を見る
                  </button>
                  {isRunning && !thisRunning && (
                    <span className="context-faint">
                      ほかのタスクを実行しています
                    </span>
                  )}
                </div>
                {prepareError !== null && (
                  <p className="export-warning">{prepareError}</p>
                )}
                {shown !== null && (
                  <section className="ai-prepared" aria-label="送る中身">
                    <p className="export-info">
                      {estimateText(shown.assembled)}
                    </p>
                    {shown.assembled.problem !== null && (
                      <p className="export-warning">
                        {shown.assembled.problem}
                      </p>
                    )}
                    {trimmedText(shown.assembled).map((t) => (
                      <p key={t} className="context-faint ai-prepared-note">
                        {t}
                      </p>
                    ))}
                    {shown.assembled.notes.map((n) => (
                      <p key={n} className="context-faint ai-prepared-note">
                        {n}
                      </p>
                    ))}
                    {confirming && shown.assembled.problem === null && (
                      <div className="ai-confirm">
                        <p>
                          クラウド（{shown.connection.name} ·{" "}
                          {shown.connection.model}
                          ）へ次の中身を送ります。
                        </p>
                        <div className="ai-actions">
                          <button
                            type="button"
                            className="primary-small"
                            onClick={runNow}
                          >
                            送る
                          </button>
                          <button
                            type="button"
                            className="small-button"
                            onClick={() => setConfirming(false)}
                          >
                            やめる
                          </button>
                        </div>
                      </div>
                    )}
                    <details open={confirming}>
                      <summary>指示（system）と依頼（user）</summary>
                      <pre className="ai-preview">{shown.assembled.system}</pre>
                      <pre className="ai-preview">{shown.assembled.user}</pre>
                    </details>
                  </section>
                )}
                {confirming && shown === null && preparing && (
                  <p className="context-faint">送る中身を組み立てています…</p>
                )}
                {result !== undefined && !thisRunning && (
                  <section className="ai-result" aria-label="結果">
                    <h2 className="context-heading">
                      結果{result.scene !== null ? `（${result.scene}）` : ""}
                    </h2>
                    {result.kind === "failed" && (
                      <p className="export-warning">{result.message}</p>
                    )}
                    {result.kind === "cancelled" && (
                      <p className="context-faint">取り消しました。</p>
                    )}
                    {result.kind === "ok" && (
                      <>
                        <p className="context-faint">{result.tokens}</p>
                        {result.written !== null && (
                          <p
                            className={
                              result.wroteOk
                                ? "export-notice"
                                : "export-warning"
                            }
                          >
                            {result.written}
                          </p>
                        )}
                        {skeleton &&
                          !result.wroteOk &&
                          result.skeleton.length > 0 &&
                          skeletonTarget !== undefined && (
                            <div className="ai-actions">
                              <span className="context-faint">
                                骨格 {result.skeleton.length}シーン（
                                {result.skeleton.map((s) => s.id).join("、")}）
                              </span>
                              <button
                                type="button"
                                className="small-button"
                                disabled={skeletonMissing || isRunning}
                                onClick={() =>
                                  onRewriteSkeleton(info, skeletonTarget)
                                }
                              >
                                書く章を直して書き直す（LLM を呼ばずに）
                              </button>
                            </div>
                          )}
                        {task.output === "summary" &&
                          (result.summary === null ? (
                            <p className="export-warning">
                              要約を読めませんでした。
                            </p>
                          ) : (
                            <div className="export-info">
                              <p className="ai-summary">{result.summary}</p>
                              <p className="context-faint">
                                {result.summaryProblem === null
                                  ? "要約のキャッシュ（.cache/summaries.json）に入れました。要約を使うタスクの文脈に入ります。"
                                  : result.summaryProblem}
                              </p>
                            </div>
                          ))}
                        {task.output === "diff" && (
                          <ProposalPanel
                            set={{
                              id: `run-${result.run}`,
                              label: task.name,
                              task: task.id,
                              model: result.model,
                              proposals: result.proposals,
                            }}
                            version={textVersion}
                            onAdopt={onAdopt}
                            onGoTo={onGoToProposal}
                            rejected={
                              rejected.get(`run-${result.run}`) ?? NO_REJECTED
                            }
                            onToggleReject={onToggleReject}
                          />
                        )}
                        {task.output === "proposals" && (
                          <BranchProposals
                            result={result}
                            scenes={scenes}
                            hidden={plot.branchHidden}
                            onAdopt={(proposal, key, model) =>
                              plot.onAdoptBranch(proposal, key, model, task.id)
                            }
                            onReject={plot.onRejectBranch}
                          />
                        )}
                        {task.output === "ideas" && (
                          <div className="export-info">
                            <p>
                              分類案 {result.ideas.length}
                              件。企画の画面の「AIの問い返し」で、メモに当てます。
                            </p>
                            <ul className="ai-idea-list">
                              {result.ideas.map((s) => (
                                <li key={s.file}>
                                  <code>{s.file}</code>
                                  {s.status !== null && ` → ${s.status}`}
                                  {s.tags.length > 0 &&
                                    `（タグ：${s.tags.join("・")}）`}
                                  {s.reason !== null && `：${s.reason}`}
                                </li>
                              ))}
                            </ul>
                          </div>
                        )}
                        {task.output === "findings" &&
                          resultFindings.length === 0 && (
                            <p className="context-empty">
                              指摘はありません（閉じた指摘は出しません）。
                            </p>
                          )}
                        {resultFindings.map((r) => (
                          <div key={r.id} className="ai-finding-card">
                            <span>
                              <span
                                className={`ai-severity ${r.finding.severity}`}
                              >
                                {SEVERITY_LABEL[r.finding.severity]}
                              </span>
                              {r.finding.line !== null && (
                                <span className="context-mono">
                                  {" "}
                                  ^{r.finding.line}{" "}
                                </span>
                              )}
                              {r.finding.line === null &&
                                r.finding.scene !== null && (
                                  <span className="context-mono">
                                    {" "}
                                    {r.finding.scene}{" "}
                                  </span>
                                )}
                              {r.finding.message}
                              {r.finding.unresolved && (
                                <span className="context-faint">
                                  （本文に無い行ID・シーンです）
                                </span>
                              )}
                            </span>
                            <div className="ai-finding-actions">
                              <button
                                type="button"
                                className="small-button"
                                disabled={
                                  r.finding.file === null ||
                                  r.finding.span === null
                                }
                                onClick={() => onGoToFinding(r)}
                              >
                                行へ移動
                              </button>
                              <button
                                type="button"
                                className="small-button"
                                onClick={() => onDismissFinding(r.id)}
                              >
                                閉じる
                              </button>
                            </div>
                          </div>
                        ))}
                        {result.unread.length > 0 && (
                          <details>
                            <summary>
                              応答のうち読めなかった行（{result.unread.length}
                              行）
                            </summary>
                            <pre className="ai-preview">
                              {result.unread.join("\n")}
                            </pre>
                          </details>
                        )}
                      </>
                    )}
                  </section>
                )}
                {task.output === "summary" && (
                  <SyncPanel
                    key={task.id}
                    info={info}
                    chapters={plot.chapters}
                    drift={plot.drift}
                    pending={plot.driftPending}
                    driftError={plot.driftError}
                    hidden={plot.driftHidden}
                    running={plot.summarizeRun}
                    busy={isRunning || choosing}
                    result={plot.summarizeResult}
                    error={plot.summarizeError}
                    onSummarize={(file, confirmed) =>
                      plot.onSummarize(task.id, file, confirmed)
                    }
                    onCancel={plot.onCancelSummarize}
                    onAdopt={plot.onAdoptDrift}
                    onReject={plot.onRejectDrift}
                  />
                )}
              </>
            )}
          </main>
          <aside className="ai-run-settings" aria-label="実行の設定">
            <h2 className="context-heading">実行の設定</h2>
            <dl className="ai-fields">
              <dt>関わり方</dt>
              <dd>{LEVEL_LABEL[task.level]}</dd>
              <dt>モデル</dt>
              <dd>
                {info.connection === null
                  ? "なし"
                  : `${info.connection.name} · ${info.connection.model}`}
              </dd>
              <dt>文脈の長さ</dt>
              <dd>
                {info.connection === null
                  ? "—"
                  : `${info.connection.context_tokens.toLocaleString("ja-JP")} トークン`}
              </dd>
              {task.budget_tokens !== null && (
                <>
                  <dt>上限</dt>
                  <dd>{task.budget_tokens.toLocaleString("ja-JP")} トークン</dd>
                </>
              )}
              {task.max_iterations !== null && (
                <>
                  <dt>反復</dt>
                  <dd>最大 {task.max_iterations}回</dd>
                </>
              )}
              <dt>定義ファイル</dt>
              <dd>
                <code>{task.path}</code>
              </dd>
            </dl>
            {task.scope !== null && (
              <p className="ai-scope-note">
                書き込む範囲：スタブと空きのみ。人が書いた行は変更しません。変更が必要な場合は提案として出します。
              </p>
            )}
            <p className="context-faint ai-scope-note">
              文脈の項目：{task.context.map((c) => c.kind).join("、")}
            </p>
          </aside>
        </>
      )}
    </div>
  );
}
