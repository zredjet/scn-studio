// 検証の画面：プロジェクト全体の診断（読み込み・構文解析・意味解析と、検証の段階1）。
// 本文の診断を選ぶと、本文の画面のその行へ移る。

import type { IrDiagnostic } from "../core/types.ts";
import { groupDiagnostics } from "./group.ts";

interface Props {
  readonly diagnostics: readonly IrDiagnostic[];
  /** 本文のファイル（story/*.scn）。 */
  readonly files: readonly string[];
  readonly onSelect: (diagnostic: IrDiagnostic) => void;
  /** 見出しを出すか（文脈パネルでは出さない）。 */
  readonly heading?: string;
}

const LABEL = { error: "エラー", warning: "警告", info: "情報" } as const;

export function CheckList({ diagnostics, files, onSelect, heading }: Props) {
  const groups = groupDiagnostics(diagnostics, files);
  return (
    <section className="check-list" aria-label={heading ?? "問題"}>
      {heading !== undefined && <h2 className="panel-heading">{heading}</h2>}
      {groups.length === 0 && <p className="placeholder">問題はありません。</p>}
      {groups.map((group) => (
        <section key={group.file} className="check-file">
          <h3 className="file-name check-file-name">{group.file}</h3>
          <ul>
            {group.diagnostics.map((d) => (
              <li key={`${d.code}-${d.span[0]}-${d.span[1]}`}>
                <button
                  type="button"
                  className={`check-item ${d.severity}`}
                  disabled={!group.story}
                  title={group.story ? "この行へ移る" : undefined}
                  onClick={() => onSelect(d)}
                >
                  <span className="check-code">{d.code}</span>
                  <span className="check-severity">{LABEL[d.severity]}</span>
                  <span className="check-message">{d.message}</span>
                </button>
              </li>
            ))}
          </ul>
        </section>
      ))}
    </section>
  );
}
