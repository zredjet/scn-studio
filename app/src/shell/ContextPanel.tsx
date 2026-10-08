// 文脈パネル（docs/ui/mockups/02-editor.html の「シーン情報」）：今のシーンの登場キャラ（一人称と口調メモ）、
// 読み書きした変数、使った素材と、このファイルの問題。データはコアの assist::scene_info（T1-8、D047）。

import { useEffect, useRef } from "react";
import { CheckList } from "../check/CheckList.tsx";
import type { IrDiagnostic, SceneInfo } from "../core/types.ts";

interface Props {
  readonly scene: SceneInfo | null;
  /** 定義へ移る操作で選ばれたキャラクター（カードを見せて目立たせる）。 */
  readonly focusCharacter: { readonly id: string; readonly seq: number } | null;
  readonly diagnostics: readonly IrDiagnostic[];
  readonly files: readonly string[];
  readonly onSelectDiagnostic: (d: IrDiagnostic) => void;
  readonly onRegister: (d: IrDiagnostic, id: string) => Promise<string | null>;
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
  focusCharacter,
  diagnostics,
  files,
  onSelectDiagnostic,
  onRegister,
}: Props) {
  const cards = useRef(new Map<string, HTMLElement>());
  useEffect(() => {
    if (focusCharacter === null) return;
    cards.current
      .get(focusCharacter.id)
      ?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [focusCharacter]);

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
            <section
              key={c.id}
              ref={(el) => {
                if (el === null) cards.current.delete(c.id);
                else cards.current.set(c.id, el);
              }}
              className={`character-card${focusCharacter?.id === c.id ? " focused" : ""}`}
              aria-label={c.name}
            >
              <div className="character-head">
                <span className="character-initial" aria-hidden="true">
                  {[...c.name][0] ?? ""}
                </span>
                <div className="character-name">
                  <strong>{c.name}</strong>
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
        />
      </div>
    </aside>
  );
}
