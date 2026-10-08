// 出力の画面（T1-12、D051）：台本（収録と校正の表）を CSV で書き出す。書き出す前に、中身を表で見られる。
// 書き出し先は作品フォルダの `export/script.csv`（CLI の `scn export script` と同じ）。
// Ren'Py への書き出し（T2-2、D053）は、出力プロファイル（export/renpy.yaml）に従って、書き出し先の
// game/scn/ を作り直す（CLI の `scn export renpy` と同じ）。

import { useEffect, useState } from "react";
import { keyed } from "../bible/fields.ts";
import type { RenpyResult } from "../core/api.ts";
import type { ScriptSheet, Summary } from "../core/types.ts";

interface Props {
  /** 本文が変わったら、台本を取り直す。 */
  readonly summary: Summary;
  /** 保存していないファイルの数（台本は、保存していない本文も含めて作る）。 */
  readonly unsaved: number;
  readonly load: () => Promise<ScriptSheet>;
  /** 書き出す。書いたファイルのパスを返す。 */
  readonly write: () => Promise<{
    readonly path: string;
    readonly rows: number;
  }>;
  /** Ren'Py へ書き出す。 */
  readonly writeRenpy: () => Promise<RenpyResult>;
}

const COLUMNS = [
  "行ID",
  "シーン",
  "キャラ",
  "台詞",
  "直前の地の文",
  "ボイスファイル名",
  "備考",
] as const;

/** Ren'Py への書き出し。書き出した後に、知らせとプロファイルの誤りを並べる。 */
function RenpyCard({
  unsaved,
  writeRenpy,
}: {
  readonly unsaved: number;
  readonly writeRenpy: () => Promise<RenpyResult>;
}) {
  const [result, setResult] = useState<RenpyResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  return (
    <section className="export-card" aria-label="Ren'Py">
      <div className="export-card-head">
        <div>
          <h2>Ren'Py</h2>
          <p className="export-note">
            出力プロファイル（export/renpy.yaml）に従って、Ren'Py
            のスクリプトを書き出します。書き出し先（既定は export/renpy）の
            game/scn/
            だけを毎回作り直し、ほかのファイルには触れません。書き出したものは手で直さず、本文を直して書き出し直してください。
          </p>
        </div>
        <button
          type="button"
          className="primary-small"
          disabled={busy}
          onClick={() => {
            setBusy(true);
            writeRenpy()
              .then((r) => {
                setResult(r);
                setError(null);
              })
              .catch((e: unknown) => setError(String(e)))
              .finally(() => setBusy(false));
          }}
        >
          書き出す
        </button>
      </div>
      {result !== null && (
        <p className="export-notice" role="status">
          {result.path} に書き出しました（{result.files}ファイル）
        </p>
      )}
      {error !== null && (
        <p className="export-error" role="alert">
          {error}
        </p>
      )}
      {unsaved > 0 && (
        <p className="export-warning">
          保存していないファイルが {unsaved}{" "}
          あります。保存していない本文も含めて書き出します。
        </p>
      )}
      {result !== null && result.problems.length > 0 && (
        <div className="export-warning">
          <p className="export-list-head">
            出力プロファイルの誤り（読み飛ばして書き出しました）
          </p>
          <ul className="export-list">
            {keyed(result.problems, (p) => p).map(([key, p]) => (
              <li key={key}>{p}</li>
            ))}
          </ul>
        </div>
      )}
      {result !== null && result.warnings.length > 0 && (
        <div className="export-info">
          <p className="export-list-head">知らせ</p>
          <ul className="export-list">
            {keyed(result.warnings, (w) => w).map(([key, w]) => (
              <li key={key}>{w}</li>
            ))}
          </ul>
        </div>
      )}
    </section>
  );
}

export function ExportScreen({
  summary,
  unsaved,
  load,
  write,
  writeRenpy,
}: Props) {
  const [sheet, setSheet] = useState<ScriptSheet | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void summary;
    let live = true;
    load()
      .then((s) => {
        if (live) {
          setSheet(s);
          setError(null);
        }
      })
      .catch((e: unknown) => {
        if (live) setError(String(e));
      });
    return () => {
      live = false;
    };
  }, [summary, load]);

  return (
    <div className="export-screen-body">
      <header className="export-head">
        <h1>出力</h1>
        <span className="flow-spacer" />
      </header>
      <RenpyCard unsaved={unsaved} writeRenpy={writeRenpy} />
      <section className="export-card" aria-label="台本">
        <div className="export-card-head">
          <div>
            <h2>台本（CSV）</h2>
            <p className="export-note">
              収録と校正の表。1行が1つの台詞で、ボイスファイル名は「キャラID_行ID」。UTF-8（BOM付き）と
              CRLF で書くので、Excel
              でそのまま開けます。書き出し先：export/script.csv
            </p>
          </div>
          <button
            type="button"
            className="primary-small"
            disabled={busy || sheet === null}
            onClick={() => {
              setBusy(true);
              write()
                .then((r) => {
                  setNotice(`${r.path} に書き出しました（${r.rows}行）`);
                  setError(null);
                })
                .catch((e: unknown) => setError(String(e)))
                .finally(() => setBusy(false));
            }}
          >
            書き出す
          </button>
        </div>
        {notice !== null && (
          <p className="export-notice" role="status">
            {notice}
          </p>
        )}
        {error !== null && (
          <p className="export-error" role="alert">
            {error}
          </p>
        )}
        {unsaved > 0 && (
          <p className="export-warning">
            保存していないファイルが {unsaved}{" "}
            あります。台本は、保存していない本文も含めて作ります（保存すると、行IDの無い行に行IDが付きます）。
          </p>
        )}
        {sheet !== null && sheet.missing_line_ids > 0 && (
          <p className="export-warning">
            行IDの無い台詞が {sheet.missing_line_ids}{" "}
            行あります。ボイスファイル名を付けられないので空にします（保存すると行IDが付きます）。
          </p>
        )}
        {sheet === null ? (
          <p className="placeholder">台本を作っています…</p>
        ) : sheet.rows.length === 0 ? (
          <p className="placeholder">台詞がありません。</p>
        ) : (
          <div className="bible-table-wrap export-table-wrap">
            <table className="bible-table export-table">
              <thead>
                <tr>
                  {COLUMNS.map((c) => (
                    <th key={c}>{c}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {keyed(
                  sheet.rows,
                  (r) => `${r.line_id}:${r.scene}:${r.text}`,
                ).map(([key, r]) => (
                  <tr key={key}>
                    <td className="mono">{r.line_id}</td>
                    <td>{r.scene}</td>
                    <td>{r.character}</td>
                    <td className="export-text">{r.text}</td>
                    <td className="export-text">{r.narration}</td>
                    <td className="mono">{r.voice}</td>
                    <td>{r.note}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </div>
  );
}
