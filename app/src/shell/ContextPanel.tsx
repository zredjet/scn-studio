// 文脈パネル（docs/ui/mockups/02-editor.html の「シーン情報」）：今のシーンの登場キャラ（一人称と口調メモ）、
// 読み書きした変数、使った素材と、このファイルの問題、今のシーンの AI の指摘（T3-7）。データはコアの
// assist::scene_info（T1-8、D047）。

import { type FindingRecord, SEVERITY_LABEL } from "../ai/model.ts";
import { CheckList } from "../check/CheckList.tsx";
import type { IrDiagnostic, SceneInfo, SceneSummary } from "../core/types.ts";

interface Props {
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
  // 立ち絵は、キャラクターごとに「灯/笑顔、灯/真顔」とまとめる。
  const assets: { kind: string; name: string; registered: boolean }[] = [];
  for (const a of scene?.assets ?? []) {
    const name = a.character === null ? a.name : `${a.character}/${a.name}`;
    assets.push({ kind: ASSET_KIND[a.kind], name, registered: a.registered });
  }

  return (
    <aside className="context-panel" aria-label="シーン情報">
      {scene !== null && (
        <>
          <h2 className="context-heading">登場キャラ</h2>
          {scene.characters.length === 0 && (
            <p className="context-empty">このシーンには出ていません。</p>
          )}
          {scene.characters.map((c) => (
            <section key={c.id} className="character-card" aria-label={c.name}>
              <div className="character-head">
                <span className="character-initial" aria-hidden="true">
                  {[...c.name][0] ?? ""}
                </span>
                <div className="character-name">
                  <button
                    type="button"
                    className="character-open"
                    title="設定DBで開く"
                    onClick={() => onOpenCharacter(c.id)}
                  >
                    {c.name}
                  </button>
                  <span>
                    {c.id} · このシーンで台詞 {c.lines}行
                  </span>
                </div>
              </div>
              {(c.first_person !== null || c.speech !== null) && (
                <dl className="character-fields">
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
            </section>
          ))}
          <h2 className="context-heading">変数</h2>
          {scene.variables.length === 0 ? (
            <p className="context-empty">読み書きしていません。</p>
          ) : (
            <ul className="context-rows">
              {scene.variables.map((v) => (
                <li key={v.key}>
                  <span className="context-mono">{v.key}</span>
                  <span className="context-faint">{v.type ?? "宣言なし"}</span>
                  <span className="context-right">
                    {usage(v.reads, v.writes)}
                  </span>
                </li>
              ))}
            </ul>
          )}
          <h2 className="context-heading">素材</h2>
          {assets.length === 0 ? (
            <p className="context-empty">使っていません。</p>
          ) : (
            <ul className="context-rows">
              {assets.map((a) => (
                <li key={`${a.kind}-${a.name}`}>
                  <span className="context-kind">{a.kind}</span>
                  <span>{a.name}</span>
                  {a.registered ? (
                    <span className="context-right">あり</span>
                  ) : (
                    <span className="context-right provisional">仮素材</span>
                  )}
                </li>
              ))}
            </ul>
          )}
        </>
      )}
      <div className="context-problems">
        <CheckList
          heading="このファイルの問題"
          diagnostics={diagnostics}
          files={files}
          onSelect={onSelectDiagnostic}
          onRegister={onRegister}
          textOf={textOf}
          scenes={scenes}
        />
      </div>
      {findings.length > 0 && (
        <section className="context-ai-findings" aria-label="AIの指摘">
          <h2 className="context-heading">AIの指摘</h2>
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
