// フローの画面の文脈パネル（docs/ui/mockups/03-flow.html の「選択中のシーン」）：選んだシーンのIDと
// ファイル、題名、要約、ルート・状態・字数、出口と入口、読み書きする変数、AIの提案（分岐案。T3-10）。
// まとまりを選んだときは、中のシーンの一覧。

import { BranchProposals } from "../ai/BranchProposals.tsx";
import { QuickRun } from "../ai/QuickRun.tsx";
import { prettyExpr } from "../core/expr.ts";
import type {
  GraphEdge,
  GraphGroup,
  IrDiagnostic,
  SceneGraphData,
  SceneSummary,
} from "../core/types.ts";
import { Icon } from "../shell/icons.tsx";
import {
  edgeLabel,
  entriesOf,
  type LegendCounts,
  NO_ROUTE,
  routeCounts,
  sceneOfDiagnostic,
  structureProblems,
} from "./model.ts";
import type { FlowAi } from "./SceneGraph.tsx";

interface Props {
  readonly graph: SceneGraphData;
  readonly scenes: readonly SceneSummary[];
  /** 選んだノード（シーンID、章のファイル、ルートの名前）。 */
  readonly selected: string | null;
  readonly onOpen: (scene: SceneSummary) => void;
  readonly ai: FlowAi;
  /** 何も選んでいないときの「フローの概要」（T4-18、U15）。 */
  readonly overview: {
    readonly checks: readonly IrDiagnostic[];
    readonly counts: LegendCounts;
    /** エンディングの数を数えたか（シーンの粒度の構造グラフからだけ数える）。 */
    readonly endingsKnown: boolean;
    readonly start: string | null;
    /** ノードを選ぶ（シーンID）。 */
    readonly onSelect: (id: string) => void;
    readonly onOpenConcept: () => void;
  };
}

/** 辺の見分け（一覧のキー）。 */
function edgeKey(e: GraphEdge): string {
  return [e.from, e.to, e.kind, e.via, e.label ?? "", e.cond_text ?? ""].join(
    "|",
  );
}

/**
 * 一覧の項目に、重ならないキーを付ける。同じ辺（選択肢2つが同じ行き先へ合流する）、同じエンディング、
 * 重なったシーンID（E108）が並ぶことがあるので、同じキーには何番目かを足す。
 */
function keyed<T>(
  items: readonly T[],
  keyOf: (item: T) => string,
): [string, T][] {
  const counts = new Map<string, number>();
  return items.map((item) => {
    const key = keyOf(item);
    const n = counts.get(key) ?? 0;
    counts.set(key, n + 1);
    return [n === 0 ? key : `${key}#${n}`, item];
  });
}

const STATUS: Readonly<Record<string, string>> = {
  draft: "下書き",
  writing: "執筆中",
  done: "完成",
  "ai-draft": "AI下書き",
};

function Target({
  id,
  scenes,
}: {
  readonly id: string;
  readonly scenes: readonly SceneSummary[];
}) {
  const scene = scenes.find((s) => s.id === id);
  return (
    <span>
      → {id} {scene?.title ?? "（無いシーン）"}
      {scene?.is_stub === true && <span className="tag tag-stub">スタブ</span>}
      {scene?.meta.status === "ai-draft" && (
        <span className="tag tag-ai">AI下書き</span>
      )}
    </span>
  );
}

function ExitItem({
  edge,
  scenes,
}: {
  readonly edge: GraphEdge;
  readonly scenes: readonly SceneSummary[];
}) {
  const label = edgeLabel(edge);
  return (
    <li className="flow-exit">
      <span className="flow-exit-cond">{label ?? "無条件"}</span>
      <Target id={edge.to} scenes={scenes} />
    </li>
  );
}

export function FlowPanel({
  graph,
  scenes,
  selected,
  onOpen,
  ai,
  overview,
}: Props) {
  if (selected === null) {
    return <FlowOverview scenes={scenes} overview={overview} />;
  }
  if (graph.level !== "scene") {
    const group: GraphGroup | undefined = graph.groups.find(
      (g) => (g.id ?? "") === selected,
    );
    if (group === undefined) return null;
    return (
      <aside className="flow-panel ctx-column" aria-label="選択中のまとまり">
        <div className="flow-panel-head">
          <span className="flow-panel-id">
            {graph.level === "chapter" ? "章" : "ルート"} ·{" "}
            {group.scenes.length}シーン
          </span>
          <h2>{group.id ?? NO_ROUTE}</h2>
        </div>
        <h3 className="context-heading">シーン</h3>
        <ul className="flow-scene-list">
          {keyed(group.scenes, (id) => id).map(([key, id]) => {
            const scene = scenes.find((s) => s.id === id);
            return (
              <li key={key}>
                <button
                  type="button"
                  className="flow-scene-item"
                  disabled={scene === undefined}
                  onClick={() => scene !== undefined && onOpen(scene)}
                >
                  <span className="flow-scene-id">{id}</span>
                  {scene?.title ?? ""}
                </button>
              </li>
            );
          })}
        </ul>
        <h3 className="context-heading">外への出口 {group.exits.length}</h3>
        <ul className="flow-exits">
          {keyed(group.exits, edgeKey).map(([key, e]) => (
            <ExitItem key={key} edge={e} scenes={scenes} />
          ))}
        </ul>
      </aside>
    );
  }
  const group = graph.groups.find((g) => g.id === selected);
  const scene = scenes.find((s) => s.id === selected);
  if (group === undefined || scene === undefined) return null;
  const entries = entriesOf(graph, selected);
  return (
    <aside className="flow-panel ctx-column" aria-label="選択中のシーン">
      <div className="flow-panel-head">
        <span className="flow-panel-id">
          {scene.id} · {scene.file.replace(/^story\//, "")}
        </span>
        <h2>{scene.title}</h2>
        <div>
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => onOpen(scene)}
          >
            本文を開く
          </button>
        </div>
      </div>
      <section className="flow-section">
        <h3 className="context-heading">要約</h3>
        <p className="flow-summary">{scene.meta.summary ?? "（要約なし）"}</p>
        <dl className="flow-meta">
          <dt>ルート</dt>
          <dd>{scene.meta.route ?? NO_ROUTE}</dd>
          <dt>状態</dt>
          <dd>
            {scene.is_stub
              ? "スタブ"
              : scene.meta.status === null
                ? "—"
                : (STATUS[scene.meta.status] ?? scene.meta.status)}
          </dd>
          <dt>分量</dt>
          <dd>{scene.chars.toLocaleString("en-US")}字</dd>
        </dl>
      </section>
      <h3 className="context-heading">出口 {group.exits.length}</h3>
      <ul className="flow-exits">
        {keyed(group.exits, edgeKey).map(([key, e]) => (
          <ExitItem key={key} edge={e} scenes={scenes} />
        ))}
        {keyed(group.endings, (id) => `ending-${id}`).map(([key, id]) => (
          <li key={key} className="flow-exit">
            <span className="flow-exit-cond">エンディング</span>
            <span>{id}</span>
          </li>
        ))}
      </ul>
      <h3 className="context-heading">入口 {entries.length}</h3>
      <ul className="flow-entries">
        {keyed(entries, edgeKey).map(([key, e]) => {
          const from = scenes.find((s) => s.id === e.from);
          const label = edgeLabel(e);
          return (
            <li key={key}>
              {e.from} {from?.title ?? ""} から
              {label === null ? "無条件" : `（${label}）`}
            </li>
          );
        })}
      </ul>
      <h3 className="context-heading">変数</h3>
      <dl className="flow-vars">
        <dt>書き</dt>
        <dd>{group.writes.length === 0 ? "—" : group.writes.join("、")}</dd>
        <dt>読み</dt>
        <dd>
          {group.reads.length === 0
            ? "—"
            : group.reads.map(prettyExpr).join("、")}
        </dd>
      </dl>
      <section className="flow-section flow-ai" aria-label="AIの提案">
        <h3 className="context-heading">AIの提案</h3>
        {ai.runner.info !== null && (
          <span className="context-faint">
            {ai.runner.info.task.name}
            {ai.runner.info.connection !== null &&
              ` · ${ai.runner.info.connection.name}（${ai.runner.info.connection.cloud ? "クラウド" : "ローカル"}）`}
          </span>
        )}
        <QuickRun
          runner={ai.runner}
          scene={scene.id}
          label={
            ai.result?.kind === "ok" && ai.result.scene === scene.id
              ? "別の案を出す"
              : "分岐案を出す"
          }
        />
        {ai.result?.kind === "failed" && ai.result.scene === scene.id && (
          <p className="notice notice-warn">{ai.result.message}</p>
        )}
        {ai.result?.scene === scene.id && (
          <BranchProposals
            result={ai.result}
            scenes={scenes}
            hidden={ai.hidden}
            onAdopt={ai.onAdopt}
            onReject={ai.onReject}
          />
        )}
      </section>
    </aside>
  );
}

/** 何も選んでいないときの右の欄：フローの概要（数・ルート・構造の問題・スタブ・操作。U15）。 */
function FlowOverview({
  scenes,
  overview,
}: {
  readonly scenes: readonly SceneSummary[];
  readonly overview: Props["overview"];
}) {
  const { counts, endingsKnown, checks, start, onSelect, onOpenConcept } =
    overview;
  const startScene = scenes.find((s) => s.id === start) ?? null;
  const stubs = scenes.filter((s) => s.is_stub);
  const problems = structureProblems(checks);
  return (
    <aside className="flow-panel ctx-column" aria-label="フローの概要">
      <div className="panel-head">
        <h2 className="h-label">フローの概要</h2>
      </div>
      <section className="section">
        <p className="t-small t-2 flow-guide">
          シーンを選ぶと、ここに入口・出口・変数が出ます。ダブルクリックで本文を開きます。
        </p>
      </section>
      <section className="section" aria-label="数">
        <h3 className="h-label">数</h3>
        <dl className="kv">
          <dt>シーン</dt>
          <dd>{scenes.length}</dd>
          <dt>スタブ</dt>
          <dd>{counts.stubs}</dd>
          <dt>AI下書き</dt>
          <dd>{counts.aiDrafts}</dd>
          {endingsKnown && (
            <>
              <dt>エンディング</dt>
              <dd>{counts.endings}</dd>
            </>
          )}
          <dt>開始</dt>
          <dd>
            {startScene === null ? (
              "—"
            ) : (
              <button
                type="button"
                className="btn-link-inline"
                onClick={() => onSelect(startScene.id)}
              >
                {startScene.id} {startScene.title}
              </button>
            )}
          </dd>
        </dl>
      </section>
      <section className="section" aria-label="ルート">
        <h3 className="h-label">ルート</h3>
        <ul className="flow-route-counts">
          {routeCounts(scenes).map((r) => (
            <li key={r.route}>
              <span>{r.route}</span>
              <span className="t-caption t-3">{r.count}シーン</span>
            </li>
          ))}
        </ul>
      </section>
      <section className="section" aria-label="構造の問題">
        <h3 className="h-label">構造の問題</h3>
        {problems.length === 0 ? (
          <p className="empty">ありません。</p>
        ) : (
          problems.map((d) => {
            const at = sceneOfDiagnostic(scenes, d);
            const concept = d.file === "concept.md";
            return (
              <div
                key={`${d.code}-${d.file}-${d.span[0]}`}
                className="card flow-problem-card"
              >
                <div className="flow-problem-row">
                  <Icon
                    name="warning"
                    size={14}
                    stroke={2}
                    className="ic-warn"
                  />
                  <span className="flow-problem-text">{d.message}</span>
                  {concept ? (
                    <button
                      type="button"
                      className="btn btn-sm"
                      onClick={onOpenConcept}
                    >
                      企画
                    </button>
                  ) : (
                    at !== null && (
                      <button
                        type="button"
                        className="btn btn-sm"
                        onClick={() => onSelect(at.id)}
                      >
                        選ぶ
                      </button>
                    )
                  )}
                </div>
                <span className="mono t-caption t-3">{d.code}</span>
              </div>
            );
          })
        )}
      </section>
      <section className="section" aria-label="スタブ">
        <h3 className="h-label">スタブ（本文なし）</h3>
        {stubs.length === 0 ? (
          <p className="empty">ありません。</p>
        ) : (
          <div className="flow-stub-chips">
            {stubs.map((s) => (
              <button
                key={`${s.id}-${s.span[0]}`}
                type="button"
                className="chip flow-stub-chip"
                title={`${s.title}を選ぶ`}
                onClick={() => onSelect(s.id)}
              >
                {s.id}
              </button>
            ))}
          </div>
        )}
      </section>
      <section className="section" aria-label="操作">
        <h3 className="h-label">操作</h3>
        <dl className="kv flow-ops">
          <dt>ダブルクリック</dt>
          <dd>本文を開く</dd>
          <dt>ドラッグ</dt>
          <dd>並べ替え（layout.json に保存）</dd>
          <dt>右の点から引く</dt>
          <dd>辺を足す</dd>
        </dl>
      </section>
    </aside>
  );
}
