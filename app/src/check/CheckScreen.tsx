// 検証の画面（docs/ui/decisions-v1.md の U19、見本 09-check.html。T4-19）：重さの札で絞り、種類ごと・ファイルごと・
// シーンごとにまとめた一覧。行ごとに行き先（今ある移り先）。段階1（保存のたび）と段階2（手で）を分けて示す。
// 右の欄に、選んだ問題の説明・該当の行・直し方（`fixes.ts` の表）。段階2の結果（カバレッジとエンディング）は一覧の下。

import { useMemo, useState } from "react";
import type {
  IrDiagnostic,
  Playthrough,
  SceneSummary,
  Severity,
} from "../core/types.ts";
import { Icon, type IconName } from "../shell/icons.tsx";
import {
  codeInfo,
  contextLines,
  destinationOf,
  filterBySeverity,
  GROUPING_LABEL,
  type Grouping,
  groupProblems,
  lineNumbers,
  sceneOf,
} from "./fixes.ts";
import { countBySeverity, unregisteredSpeaker } from "./group.ts";
import { PlaythroughResult } from "./PlaythroughPanel.tsx";

interface Props {
  readonly diagnostics: readonly IrDiagnostic[];
  /** 本文のファイル。 */
  readonly files: readonly string[];
  readonly scenes: readonly SceneSummary[];
  /** ファイルの今の中身（行の番号と該当の行）。 */
  readonly textOf: (file: string) => string | undefined;
  /** 行き先へ移る。 */
  readonly onOpen: (d: IrDiagnostic) => void;
  /** 未登録の話者を設定DBの画面で登録する。 */
  readonly onRegister: (name: string) => void;
  /** 右の欄を出す（問題を選んだとき。狭い窓や閉じた欄でも、説明と直し方が見えるように）。 */
  readonly onShowPanel: () => void;
  /** 段階2（自動巡回と表記の検査）。 */
  readonly stage2: {
    readonly report: Playthrough | null;
    readonly running: boolean;
    readonly error: string | null;
    /** 本文が変わったので、前の結果を捨てた。 */
    readonly discarded: boolean;
    /** 最後に調べた時刻（時:分）。 */
    readonly at: string | null;
    readonly onRun: () => void;
  };
}

const SEVERITIES: readonly {
  readonly severity: Severity;
  readonly label: string;
  readonly icon: IconName;
}[] = [
  { severity: "error", label: "エラー", icon: "error" },
  { severity: "warning", label: "警告", icon: "warning" },
  { severity: "info", label: "情報", icon: "info" },
];

const ICON: Readonly<Record<Severity, IconName>> = {
  error: "error",
  warning: "warning",
  info: "info",
};

/** 診断の見分け（一覧の選択とキー）。 */
function keyOf(d: IrDiagnostic): string {
  return `${d.code}\u0000${d.file}\u0000${d.span[0]}\u0000${d.span[1]}`;
}

export function CheckScreen({
  diagnostics,
  files,
  scenes,
  textOf,
  onOpen,
  onRegister,
  onShowPanel,
  stage2,
}: Props) {
  const [severity, setSeverity] = useState<Severity | null>(null);
  const [grouping, setGrouping] = useState<Grouping>("category");
  const [selected, setSelected] = useState<string | null>(null);
  const counts = countBySeverity(diagnostics);
  const total = counts.error + counts.warning + counts.info;
  const groups = useMemo(
    () =>
      groupProblems(
        filterBySeverity(diagnostics, severity),
        grouping,
        files,
        scenes,
      ),
    [diagnostics, severity, grouping, files, scenes],
  );
  const lines = useMemo(
    () => lineNumbers(diagnostics, textOf),
    [diagnostics, textOf],
  );
  const current = diagnostics.find((d) => keyOf(d) === selected) ?? null;

  const place = (d: IrDiagnostic) => {
    const s = sceneOf(scenes, d);
    const line = lines.get(d) ?? null;
    const file = d.file.replace(/^story\//, "");
    return [s?.id, line === null ? file : `${file} ${line}行`]
      .filter((x) => x !== undefined)
      .join(" · ");
  };
  const stage2Text = stage2.running
    ? "調べています…"
    : stage2.report !== null
      ? `済み${stage2.at === null ? "" : `（${stage2.at} に調べた）`}`
      : stage2.discarded
        ? "本文が変わったので、前の結果を消しました"
        : "まだ";

  return (
    <div className="check-screen-body">
      <main className="check-main" aria-label="検証の一覧">
        <header className="work-head check-head">
          <h1 className="h1">検証</h1>
          <fieldset className="check-chips" aria-label="重さで絞る">
            <button
              type="button"
              className="chip"
              aria-pressed={severity === null}
              onClick={() => setSeverity(null)}
            >
              すべて {total}
            </button>
            {SEVERITIES.map((s) => (
              <button
                key={s.severity}
                type="button"
                className={`chip chip-${s.severity}`}
                aria-pressed={severity === s.severity}
                onClick={() =>
                  setSeverity(severity === s.severity ? null : s.severity)
                }
              >
                <Icon name={s.icon} size={13} stroke={2} />
                {s.label} {counts[s.severity]}
              </button>
            ))}
          </fieldset>
          <select
            className="select check-grouping"
            aria-label="まとめ方"
            value={grouping}
            onChange={(event) => setGrouping(event.target.value as Grouping)}
          >
            {(Object.keys(GROUPING_LABEL) as Grouping[]).map((g) => (
              <option key={g} value={g}>
                {GROUPING_LABEL[g]}
              </option>
            ))}
          </select>
          <span className="grow" />
          <button
            type="button"
            className="btn"
            disabled={stage2.running}
            title="段階2（自動巡回と表記の検査）を走らせる。保存していない本文も含める"
            onClick={stage2.onRun}
          >
            {stage2.running
              ? "調べています…"
              : stage2.report === null
                ? "段階2を調べる"
                : "段階2を調べ直す"}
          </button>
        </header>
        <div className="check-stages">
          <span>
            <Icon name="saved" size={13} stroke={2} />
            段階1 構文・参照・構造（保存のたびに）
          </span>
          <span>
            {stage2.report !== null ? (
              <Icon name="saved" size={13} stroke={2} />
            ) : (
              <span className="check-stage-dot" aria-hidden="true" />
            )}
            段階2 到達・条件・表記（手で）：{stage2Text}
          </span>
        </div>
        {stage2.error !== null && (
          <p className="notice notice-danger check-band" role="alert">
            {stage2.error}
          </p>
        )}
        <div className="check-scroll">
          {total === 0 ? (
            <p className="empty">問題はありません。</p>
          ) : groups.length === 0 ? (
            <p className="empty">この重さの問題はありません。</p>
          ) : (
            <div className="table-wrap">
              <table className="table check-table">
                <thead>
                  <tr>
                    <th>コード</th>
                    <th>内容</th>
                    <th>場所</th>
                    <th aria-label="行き先" />
                  </tr>
                </thead>
                <tbody>
                  {groups.map((g) => (
                    <GroupRows
                      key={g.label}
                      label={g.label}
                      items={g.items}
                      selected={selected}
                      files={files}
                      place={place}
                      onSelect={(d) => {
                        setSelected(keyOf(d));
                        onShowPanel();
                      }}
                      onOpen={onOpen}
                      onRegister={onRegister}
                    />
                  ))}
                </tbody>
              </table>
            </div>
          )}
          {stage2.report !== null && (
            <PlaythroughResult report={stage2.report} />
          )}
        </div>
      </main>
      <aside className="check-side ctx-column" aria-label="選んだ問題">
        {current === null ? (
          <>
            <div className="panel-head">
              <h2 className="h-label">選んだ問題</h2>
            </div>
            <p className="empty check-side-empty">
              問題を選ぶと、説明・該当の行・直し方が出ます。
            </p>
          </>
        ) : (
          <Detail
            d={current}
            files={files}
            place={place(current)}
            text={textOf(current.file)}
            onOpen={onOpen}
          />
        )}
      </aside>
    </div>
  );
}

function GroupRows({
  label,
  items,
  selected,
  files,
  place,
  onSelect,
  onOpen,
  onRegister,
}: {
  readonly label: string;
  readonly items: readonly IrDiagnostic[];
  readonly selected: string | null;
  readonly files: readonly string[];
  readonly place: (d: IrDiagnostic) => string;
  readonly onSelect: (d: IrDiagnostic) => void;
  readonly onOpen: (d: IrDiagnostic) => void;
  readonly onRegister: (name: string) => void;
}) {
  return (
    <>
      <tr className="grp">
        <td colSpan={4}>
          {label} <span className="check-count">{items.length}</span>
        </td>
      </tr>
      {items.map((d) => {
        const destination = destinationOf(d, files);
        const speaker = unregisteredSpeaker(d);
        const isSelected = keyOf(d) === selected;
        return (
          <tr key={keyOf(d)} className={isSelected ? "is-selected" : undefined}>
            <td className={`check-code ${d.severity}`}>
              <button
                type="button"
                className="check-select"
                aria-pressed={isSelected}
                title="説明と直し方を右の欄に出す"
                onClick={() => onSelect(d)}
              >
                <Icon name={ICON[d.severity]} size={13} stroke={2} />
                <span className="mono">{d.code}</span>
              </button>
            </td>
            <td>
              <button
                type="button"
                className="check-row-message"
                onClick={() => onSelect(d)}
              >
                {d.message}
              </button>
              {speaker !== null && (
                <button
                  type="button"
                  className="btn btn-link btn-sm"
                  onClick={() => onRegister(speaker)}
                >
                  {speaker} を登録
                </button>
              )}
            </td>
            <td className="check-row-place">{place(d)}</td>
            <td className="check-action">
              {destination !== null && (
                <button
                  type="button"
                  className="btn btn-sm"
                  onClick={() => onOpen(d)}
                >
                  {destination}
                </button>
              )}
            </td>
          </tr>
        );
      })}
    </>
  );
}

function Detail({
  d,
  files,
  place,
  text,
  onOpen,
}: {
  readonly d: IrDiagnostic;
  readonly files: readonly string[];
  readonly place: string;
  readonly text: string | undefined;
  readonly onOpen: (d: IrDiagnostic) => void;
}) {
  const info = codeInfo(d.code);
  const destination = destinationOf(d, files);
  const around = contextLines(text, d.span[0]);
  return (
    <>
      <div className="panel-head">
        <h2 className="h2">{info.title}</h2>
        <span className="mono t-caption t-3">{d.code}</span>
      </div>
      <section className="section" aria-label="説明">
        {info.about !== "" && (
          <p className="t-small check-about">{info.about}</p>
        )}
        <p className="t-small t-2 check-message-full">{d.message}</p>
        {around.length > 0 && (
          <pre className="check-snippet">
            {around.map((l) => (
              <span key={l.line} className={l.at ? "at" : undefined}>
                <span className="no">{l.line}</span>
                {l.text}
                {"\n"}
              </span>
            ))}
          </pre>
        )}
        <span className="t-caption t-3">{place}</span>
        {destination !== null && (
          <button
            type="button"
            className="btn btn-sm check-open"
            onClick={() => onOpen(d)}
          >
            {destination}
          </button>
        )}
      </section>
      {info.fixes.length > 0 && (
        <section className="section" aria-label="直し方">
          <h3 className="h-label">直し方</h3>
          <ol className="check-fixes">
            {info.fixes.map((f) => (
              <li key={f}>{f}</li>
            ))}
          </ol>
        </section>
      )}
    </>
  );
}
