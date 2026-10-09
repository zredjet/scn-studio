// 検証の画面の自動巡回と表記の検査（検証の段階2。T3-2・T3-3、D063・D064）の結果：カバレッジと
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

/**
 * 段階2の結果（検証の画面の一覧の下。T4-19）：カバレッジの文、調べきれなかったときの知らせ、エンディングごとの
 * 到達条件の表。走らせるボタンと様子は、検証の画面の見出しの帯にある。
 */
export function PlaythroughResult({
  report,
}: {
  readonly report: Playthrough;
}) {
  return (
    <section className="playthrough" aria-label="段階2の結果">
      <h2 className="h2">段階2の結果：カバレッジとエンディング</h2>
      <p className="playthrough-summary" role="status">
        {summaryText(report)}
      </p>
      {!report.complete && (
        <p className="notice notice-warn">
          調べきれませんでした：{report.limit}
          （選べない選択肢・真にならない条件・着かないシーンは出していません）
        </p>
      )}
      {report.endings.length === 0 ? (
        <p className="empty">本文に @エンディング がありません。</p>
      ) : (
        <div className="table-wrap">
          <table className="table playthrough-endings">
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
                      <span className="playthrough-scene">{ending.scene}</span>
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
        </div>
      )}
    </section>
  );
}
