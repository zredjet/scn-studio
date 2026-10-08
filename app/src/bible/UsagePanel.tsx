// 設定DBの画面の右の欄「逆引き」（docs/ui/mockups/04-bible.html、T1-10、D049）：数のカード、登場シーン
// （キャラクター以外は名前の出る行）、参照している先と参照されている元。データはコアの `bible::entry_usage`。
// 「AIに渡す内容」（T3-6）はコアの `ai::entry_context`。

import { useState } from "react";
import type { EntryRef, EntryUsage } from "../core/types.ts";
import { aiContextText } from "./aicontext.ts";
import { keyed } from "./fields.ts";

interface Props {
  readonly usage: EntryUsage | null;
  readonly typeNames: ReadonlyMap<string, string>;
  /** 本文のその行へ移る。 */
  readonly onOpenLocation: (file: string, offset: number) => void;
  /** 参照先（エントリかシーン）を開く。 */
  readonly onOpenRef: (ref: EntryRef) => void;
}

/** 最初に見せる登場シーンの数。 */
const FIRST_SCENES = 8;

function RefItem({
  item,
  note,
  onOpen,
}: {
  readonly item: EntryRef;
  readonly note: string;
  readonly onOpen: (ref: EntryRef) => void;
}) {
  return (
    <li>
      <button
        type="button"
        className="bible-ref-item"
        disabled={!item.exists}
        onClick={() => onOpen(item)}
      >
        <span>{item.name ?? item.id}</span>
        <span className="bible-ref-note">
          {item.exists ? note : `${note} · 無い`}
        </span>
      </button>
    </li>
  );
}

export function UsagePanel({
  usage,
  typeNames,
  onOpenLocation,
  onOpenRef,
}: Props) {
  const [all, setAll] = useState(false);
  if (usage === null) {
    return (
      <aside className="bible-usage" aria-label="逆引き">
        <h2 className="bible-usage-heading">逆引き</h2>
        <p className="context-empty">
          エントリを選ぶと、使われている場所が出ます。
        </p>
      </aside>
    );
  }
  const scenes = all ? usage.scenes : usage.scenes.slice(0, FIRST_SCENES);
  const ai = aiContextText(usage.ai);
  const typeName = (r: EntryRef) =>
    r.kind === "scene"
      ? "シーン"
      : (typeNames.get(r.type_id ?? "") ?? r.type_id ?? "");
  return (
    <aside className="bible-usage" aria-label="逆引き">
      <h2 className="bible-usage-heading">逆引き</h2>
      <div className="bible-usage-cards">
        <div className="bible-usage-card">
          <strong>{usage.scenes.length}</strong>
          <span>{usage.character ? "登場シーン" : "出るシーン"}</span>
        </div>
        <div className="bible-usage-card">
          <strong>{usage.lines}</strong>
          <span>{usage.character ? "台詞行" : "名前の出る行"}</span>
        </div>
        <div className="bible-usage-card">
          <strong className="mono">{usage.first ?? "—"}</strong>
          <span>初出</span>
        </div>
      </div>
      <h3 className="bible-usage-subheading">
        {usage.character ? "登場シーン" : "本文での出現（名前・呼び名）"}
      </h3>
      {usage.scenes.length === 0 ? (
        <p className="context-empty">まだ本文に出てきません。</p>
      ) : (
        <ul className="bible-usage-scenes">
          {scenes.map((s) => (
            <li key={s.scene}>
              <button
                type="button"
                className="bible-usage-scene"
                onClick={() => onOpenLocation(s.file, s.span[0])}
              >
                <span className="bible-usage-id">{s.scene}</span>
                <span className="bible-usage-title">{s.title}</span>
                <span className="bible-usage-count">
                  {s.count > 0 ? `${s.count}行` : ""}
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
      {usage.scenes.length > FIRST_SCENES && (
        <button
          type="button"
          className="bible-usage-more"
          onClick={() => setAll((a) => !a)}
        >
          {all ? "少なく表示" : `すべて表示（${usage.scenes.length}）`}
        </button>
      )}
      <h3 className="bible-usage-subheading">参照しているもの</h3>
      {usage.refs_out.length === 0 ? (
        <p className="context-empty">なし</p>
      ) : (
        <ul className="bible-refs">
          {keyed(usage.refs_out, (r) => `${r.field}-${r.kind}-${r.id}`).map(
            ([key, r]) => (
              <RefItem
                key={key}
                item={r}
                note={`${typeName(r)} · ${r.label}`}
                onOpen={onOpenRef}
              />
            ),
          )}
        </ul>
      )}
      <h3 className="bible-usage-subheading">参照されているもの</h3>
      {usage.refs_in.length === 0 ? (
        <p className="context-empty">なし</p>
      ) : (
        <ul className="bible-refs">
          {keyed(usage.refs_in, (r) => `${r.path}-${r.field}`).map(
            ([key, r]) => (
              <RefItem
                key={key}
                item={r}
                note={`${typeName(r)} · ${r.label}`}
                onOpen={onOpenRef}
              />
            ),
          )}
        </ul>
      )}
      <h3 className="bible-usage-subheading">AIに渡す内容</h3>
      {ai === null ? (
        <p className="context-empty">
          このエントリを文脈に入れるAIタスクはありません（ai/*.yaml の context
          に characters・terms を書くと入ります）。
        </p>
      ) : (
        <>
          <p className="bible-ai-lead">{ai.lead}</p>
          <ul className="bible-ai-tasks">
            {ai.tasks.map((t) => (
              <li key={t}>{t}</li>
            ))}
          </ul>
        </>
      )}
    </aside>
  );
}
