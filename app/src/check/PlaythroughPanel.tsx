// 検証の画面の自動巡回と表記の検査（検証の段階2。T3-2・T3-3、D063・D064）：ボタンで走らせ、カバレッジと
// エンディングごとの到達条件を出す。診断（W506〜W509、W704、I204〜I206）は、検証の一覧に足して出す（App）。本文が変わったら結果を捨てる（位置がずれる
// ため）。重いので、保存のたびには走らせない。

import { keyed } from "../bible/fields.ts";
import type { Playthrough } from "../core/types.ts";
import {
  exampleText,
  sortedEndings,
  summaryText,
  valuesText,
} from "./playthrough.ts";

interface Props {
  readonly report: Playthrough | null;
  readonly running: boolean;
  readonly error: string | null;
  /** 前の結果を、本文が変わったので捨てた。 */
  readonly discarded: boolean;
  readonly onRun: () => void;
}

export function PlaythroughPanel({
  report,
  running,
  error,
  discarded,
  onRun,
}: Props) {
  return (
    <section className="playthrough" aria-label="自動巡回と表記の検査">
      <div className="playthrough-head">
        <div>
          <h2 className="panel-heading">自動巡回と表記の検査（検証の段階2）</h2>
          <p className="playthrough-note">
            開始シーンから、選択肢の組み合わせを実際に実行して、選べない選択肢・真にならない条件・着かないシーン・仕込みを通らない伏線を調べます。あわせて、一人称と用語の表記ゆれを調べます。保存していない本文も含めます。
          </p>
        </div>
        <button
          type="button"
          className="primary-small"
          disabled={running}
          onClick={onRun}
        >
          {running
            ? "調べています…"
            : report === null
              ? "走らせる"
              : "もう一度"}
        </button>
      </div>
      {error !== null && (
        <p className="export-error" role="alert">
          {error}
        </p>
      )}
      {report === null && discarded && error === null && (
        <p className="playthrough-note">
          本文が変わったので、前の結果を消しました。もう一度走らせてください。
        </p>
      )}
      {report !== null && (
        <>
          <p className="playthrough-summary" role="status">
            {summaryText(report)}
          </p>
          {!report.complete && (
            <p className="export-warning">
              調べきれませんでした：{report.limit}
              （選べない選択肢・真にならない条件・着かないシーンは出していません）
            </p>
          )}
          {report.endings.length === 0 ? (
            <p className="playthrough-note">
              本文に @エンディング がありません。
            </p>
          ) : (
            <table className="bible-table playthrough-endings">
              <thead>
                <tr>
                  <th>エンディング</th>
                  <th>着くか</th>
                  <th>道の例（選択肢のいちばん少ない道）</th>
                  <th>着いたときの変数</th>
                </tr>
              </thead>
              <tbody>
                {keyed(sortedEndings(report), (e) => e.id).map(
                  ([key, ending]) => (
                    <tr key={key}>
                      <td>
                        <span className="mono">{ending.id}</span>
                        <span className="playthrough-scene">
                          {ending.scene}
                        </span>
                      </td>
                      <td>
                        {ending.reached ? (
                          <span className="playthrough-ok">着く</span>
                        ) : (
                          <span className="playthrough-ng">
                            {report.complete ? "着かない" : "見つかっていない"}
                          </span>
                        )}
                      </td>
                      <td>
                        {ending.reached ? exampleText(ending.example) : "—"}
                      </td>
                      <td className="playthrough-values">
                        {valuesText(ending).join("、")}
                      </td>
                    </tr>
                  ),
                )}
              </tbody>
            </table>
          )}
        </>
      )}
    </section>
  );
}
