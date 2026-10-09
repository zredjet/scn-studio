// 文脈パネル（docs/ui/mockups/02-editor.html の「シーン情報」）：今のシーンの登場キャラ（一人称と口調メモ）、
// 読み書きした変数、使った素材と、このファイルの問題、今のシーンの AI の指摘（T3-7）。データはコアの
// assist::scene_info（T1-8、D047）。検索の欄で「すべての章」を探している間は、検索の結果（見本 02b。T4-22）。

import { type FindingRecord, SEVERITY_LABEL } from "../ai/model.ts";
import { CheckList } from "../check/CheckList.tsx";
import type { IrDiagnostic, SceneInfo, SceneSummary } from "../core/types.ts";
import {
  type FoundAll,
  type FoundMatch,
  sceneAt,
} from "../editor/projectSearch.ts";
import { Icon } from "./icons.tsx";

interface Props {
  /** すべての章の検索の結果（探していなければ null。T4-22）。 */
  readonly found: FoundAll | null;
  /** 検索の結果の一致へ移る。 */
  readonly onOpenFound: (match: FoundMatch) => void;
  /** 検索の結果を閉じる。 */
  readonly onCloseFound: () => void;
  readonly scene: SceneInfo | null;
  readonly diagnostics: readonly IrDiagnostic[];
  readonly files: readonly string[];
  /** ファイルの今の中身とシーン（問題に行の番号とシーンを添える。T4-13、U17）。 */
  readonly textOf: (file: string) => string | undefined;
  readonly scenes: readonly SceneSummary[];
  readonly onSelectDiagnostic: (d: IrDiagnostic) => void;
  /** 未登録の話者を設定DBの画面で登録する。 */
  readonly onRegister: (name: string) => void;
  /** キャラクターを設定DBの画面で開く。 */
  readonly onOpenCharacter: (id: string) => void;
  /** 今のシーンの AI の指摘（T3-7）。 */
  readonly findings: readonly FindingRecord[];
  /** 指摘の行へ移る。 */
  readonly onGoToFinding: (record: FindingRecord) => void;
  /** 指摘を閉じる（一覧と行の印から消す）。 */
  readonly onDismissFinding: (id: string) => void;
}

const ASSET_KIND = {
  background: "背景",
  expression: "立ち絵",
  bgm: "BGM",
  se: "SE",
} as const;

/** 変数の使い方（このシーンでの回数）。 */
function usage(reads: number, writes: number): string {
  const parts: string[] = [];
  if (writes > 0) parts.push(`書き ${writes}`);
  if (reads > 0) parts.push(`読み ${reads}`);
  return parts.join(" · ");
}

export function ContextPanel({
  found,
  onOpenFound,
  onCloseFound,
  scene,
  diagnostics,
  files,
  textOf,
  scenes,
  onSelectDiagnostic,
  onRegister,
  onOpenCharacter,
  findings,
  onGoToFinding,
  onDismissFinding,
}: Props) {
  // 素材は種類ごとに1行にまとめる（立ち絵は「灯/笑顔・灯/真顔」）。カタログに無いものは仮素材。
  const assets = new Map<string, { name: string; registered: boolean }[]>();
  for (const a of scene?.assets ?? []) {
    const name = a.character === null ? a.name : `${a.character}/${a.name}`;
    const kind = ASSET_KIND[a.kind];
    assets.set(kind, [
      ...(assets.get(kind) ?? []),
      { name, registered: a.registered },
    ]);
  }

  if (found !== null) {
    return (
      <FoundList
        found={found}
        scenes={scenes}
        onOpen={onOpenFound}
        onClose={onCloseFound}
      />
    );
  }

  return (
    <aside className="context-panel ctx-column" aria-label="このシーン">
      <div className="panel-head">
        <h2 className="h-label">
          {scene === null ? "このファイル" : "このシーン"}
        </h2>
      </div>
      {scene !== null && (
        <>
          <section className="section" aria-label="登場キャラ">
            <h3 className="h-label">登場キャラ</h3>
            {scene.characters.length === 0 && (
              <p className="empty">このシーンには出ていません。</p>
            )}
            {scene.characters.map((c) => (
              <div key={c.id} className="card character-card">
                <div className="character-head">
                  <span className="character-initial" aria-hidden="true">
                    {[...c.name][0] ?? ""}
                  </span>
                  <div className="character-name">
                    <strong>{c.name}</strong>
                    <span className="t-caption t-3">
                      {c.id} · このシーンの台詞 {c.lines}行
                    </span>
                  </div>
                  <button
                    type="button"
                    className="btn btn-link btn-sm"
                    title="設定DBで開く"
                    onClick={() => onOpenCharacter(c.id)}
                  >
                    設定
                  </button>
                </div>
                {(c.first_person !== null || c.speech !== null) && (
                  <dl className="kv">
                    {c.first_person !== null && (
                      <>
                        <dt>一人称</dt>
                        <dd>{c.first_person}</dd>
                      </>
                    )}
                    {c.speech !== null && (
                      <>
                        <dt>口調</dt>
                        <dd>{c.speech}</dd>
                      </>
                    )}
                  </dl>
                )}
              </div>
            ))}
          </section>
          <section className="section" aria-label="変数">
            <h3 className="h-label">変数</h3>
            {scene.variables.length === 0 ? (
              <p className="empty">読み書きしていません。</p>
            ) : (
              <ul className="context-rows">
                {scene.variables.map((v) => (
                  <li key={v.key}>
                    <span className="mono context-var">{v.key}</span>
                    {v.type === null && (
                      <span className="tag tag-warn">宣言なし</span>
                    )}
                    <span className="context-right">
                      {usage(v.reads, v.writes)}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </section>
          <section className="section" aria-label="素材">
            <h3 className="h-label">素材</h3>
            {assets.size === 0 ? (
              <p className="empty">使っていません。</p>
            ) : (
              <dl className="kv">
                {[...assets].map(([kind, list]) => (
                  <div key={kind} className="kv-row">
                    <dt>{kind}</dt>
                    <dd>
                      {list.map((a, i) => (
                        <span key={a.name}>
                          {i > 0 && "・"}
                          {a.name}
                          {!a.registered && (
                            <span className="tag tag-ph context-ph">
                              仮素材
                            </span>
                          )}
                        </span>
                      ))}
                    </dd>
                  </div>
                ))}
              </dl>
            )}
          </section>
        </>
      )}
      <div className="section context-problems">
        <CheckList
          heading={scene === null ? "このファイルの問題" : "このシーンの問題"}
          diagnostics={diagnostics}
          files={files}
          onSelect={onSelectDiagnostic}
          onRegister={onRegister}
          textOf={textOf}
          scenes={scenes}
        />
      </div>
      {findings.length > 0 && (
        <section className="section context-ai-findings" aria-label="AIの指摘">
          <h3 className="h-label">AIの指摘</h3>
          {findings.map((r) => (
            <div key={r.id} className="ai-finding-card">
              <span className="ai-finding-source">
                {r.taskName} · {r.source}
              </span>
              <span>
                <span className={`ai-severity ${r.finding.severity}`}>
                  {SEVERITY_LABEL[r.finding.severity]}
                </span>
                {r.finding.line !== null && (
                  <span className="context-mono"> ^{r.finding.line} </span>
                )}
                {r.finding.message}
                {r.finding.unresolved && (
                  <span className="context-faint">（行が見つかりません）</span>
                )}
              </span>
              <div className="ai-finding-actions">
                <button
                  type="button"
                  className="btn btn-sm"
                  disabled={r.finding.file === null || r.finding.span === null}
                  onClick={() => onGoToFinding(r)}
                >
                  行へ移動
                </button>
                <button
                  type="button"
                  className="btn btn-sm"
                  onClick={() => onDismissFinding(r.id)}
                >
                  閉じる
                </button>
              </div>
            </div>
          ))}
        </section>
      )}
    </aside>
  );
}

/** すべての章の検索の結果（章ごとに、行・シーン・前後の文字）。 */
function FoundList({
  found,
  scenes,
  onOpen,
  onClose,
}: {
  readonly found: FoundAll;
  readonly scenes: readonly SceneSummary[];
  readonly onOpen: (match: FoundMatch) => void;
  readonly onClose: () => void;
}) {
  return (
    <aside className="context-panel ctx-column" aria-label="検索の結果">
      <div className="panel-head">
        <h2 className="h-label">検索の結果</h2>
        <span className="t-caption t-3">
          {found.capped ? `${found.total}+` : found.total}件 ·{" "}
          {found.files.length}章
        </span>
        <button
          type="button"
          className="btn btn-icon"
          aria-label="検索の結果を閉じる"
          title="検索の結果を閉じる"
          onClick={onClose}
        >
          <Icon name="close" size={14} />
        </button>
      </div>
      {found.invalid && (
        <p className="empty found-note">正規表現が正しくありません。</p>
      )}
      {!found.invalid && found.total === 0 && (
        <p className="empty found-note">見つかりません。</p>
      )}
      {found.files.map((f) => (
        <section key={f.file} className="found-file" aria-label={f.file}>
          <h3 className="menu-label">
            {f.file.replace(/^story\//, "")}
            <span className="found-count">{f.matches.length}</span>
          </h3>
          {f.matches.map((m) => (
            <button
              key={`${m.start}`}
              type="button"
              className="lrow found-row"
              onClick={() => onOpen(m)}
            >
              <span className="found-place">
                <span className="id">{m.line}行</span>
                <span className="id">{sceneAt(scenes, m)?.id ?? ""}</span>
              </span>
              <span className="found-text t-small">
                {m.before}
                <mark>{m.match}</mark>
                {m.after}
              </span>
            </button>
          ))}
        </section>
      ))}
      {found.capped && (
        <p className="empty found-note">
          多いので、ここまでで探すのをやめました。言葉を足して絞ってください。
        </p>
      )}
    </aside>
  );
}
