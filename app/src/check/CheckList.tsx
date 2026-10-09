// 検証の画面：プロジェクト全体の診断（読み込み・構文解析・意味解析と、検証の段階1）。
// 本文の診断を選ぶと、本文の画面のその行へ移る（設定DBのエントリと変数の診断は、その画面を開く）。
// 未登録の話者（W202）は、設定DBの画面の「作る」の欄に名前を入れて開く（D047、D049）。

import type { IrDiagnostic, SceneSummary } from "../core/types.ts";
import {
  diagnosticPlaces,
  groupDiagnostics,
  unregisteredSpeaker,
} from "./group.ts";

interface Props {
  readonly diagnostics: readonly IrDiagnostic[];
  /** 本文のファイル（`story/` の下の `*.scn`）。 */
  readonly files: readonly string[];
  readonly onSelect: (diagnostic: IrDiagnostic) => void;
  /** 見出しを出すか（文脈パネルでは出さない）。 */
  readonly heading?: string;
  /** 未登録の話者を設定DBの画面で登録する（話者名を渡す）。省くと「登録」を出さない。 */
  readonly onRegister?: (name: string) => void;
  /** 本文のファイルのほかに、選んで開けるファイル（設定DBのエントリ、変数）。 */
  readonly canOpen?: (file: string) => boolean;
  /** ファイルの今の中身（行の番号を出すため。無ければ出さない。T4-13、U17）。 */
  readonly textOf?: (file: string) => string | undefined;
  /** シーン（本文の診断にシーンのIDを添えるため）。 */
  readonly scenes?: readonly SceneSummary[];
}

const LABEL = { error: "エラー", warning: "警告", info: "情報" } as const;

/** 未登録の話者を、設定DBの画面で登録する（名前を入れた「作る」の欄を開く。D047、D049）。 */
function RegisterButton({
  name,
  onRegister,
}: {
  readonly name: string;
  readonly onRegister: NonNullable<Props["onRegister"]>;
}) {
  return (
    <button
      type="button"
      className="small-button check-register"
      onClick={() => onRegister(name)}
    >
      「{name}」を設定DBに登録…
    </button>
  );
}

export function CheckList({
  diagnostics,
  files,
  onSelect,
  heading,
  onRegister,
  canOpen,
  textOf,
  scenes,
}: Props) {
  const groups = groupDiagnostics(diagnostics, files);
  return (
    <section className="check-list" aria-label={heading ?? "問題"}>
      {heading !== undefined && <h2 className="panel-heading">{heading}</h2>}
      {groups.length === 0 && <p className="placeholder">問題はありません。</p>}
      {groups.map((group) => {
        const text = textOf?.(group.file);
        const places =
          text === undefined
            ? null
            : diagnosticPlaces(group.diagnostics, text, scenes ?? []);
        return (
          <section key={group.file} className="check-file">
            <h3 className="file-name check-file-name">{group.file}</h3>
            <ul>
              {group.diagnostics.map((d, i) => {
                const place = places?.[i];
                return (
                  <li key={`${d.code}-${d.span[0]}-${d.span[1]}`}>
                    <button
                      type="button"
                      className={`check-item ${d.severity}`}
                      disabled={!group.story && canOpen?.(group.file) !== true}
                      title={
                        group.story
                          ? "この行へ移る"
                          : canOpen?.(group.file) === true
                            ? "開く"
                            : undefined
                      }
                      onClick={() => onSelect(d)}
                    >
                      <span className="check-code">{d.code}</span>
                      <span className="check-severity">
                        {LABEL[d.severity]}
                      </span>
                      <span className="check-text">
                        <span className="check-message">{d.message}</span>
                        {place !== undefined && (
                          <span className="check-place">
                            {place.line}行
                            {place.scene !== null && ` · ${place.scene}`}
                          </span>
                        )}
                      </span>
                    </button>
                    {group.story &&
                      onRegister !== undefined &&
                      unregisteredSpeaker(d) !== null && (
                        <RegisterButton
                          name={unregisteredSpeaker(d) ?? ""}
                          onRegister={onRegister}
                        />
                      )}
                  </li>
                );
              })}
            </ul>
          </section>
        );
      })}
    </section>
  );
}
