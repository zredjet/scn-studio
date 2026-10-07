// 検証の画面：プロジェクト全体の診断（段階0と段階1）を、ファイルごとにまとめる。

import type { IrDiagnostic, Severity } from "../core/types.ts";

export interface FileDiagnostics {
  readonly file: string;
  /** 本文のファイル（story/*.scn）か。本文なら、選ぶとエディタのその行へ移れる。 */
  readonly story: boolean;
  /** 位置の順。 */
  readonly diagnostics: readonly IrDiagnostic[];
}

const RANK: Record<Severity, number> = { error: 0, warning: 1, info: 2 };

/**
 * 診断をファイルごとにまとめる。本文のファイルを `files` の順に先に並べ、続けてほかのファイル（設定ファイル、
 * 企画書など）をパスの順に並べる。同じ診断（コード・ファイル・位置が同じ）は1つにする。
 */
export function groupDiagnostics(
  diagnostics: readonly IrDiagnostic[],
  files: readonly string[],
): FileDiagnostics[] {
  const byFile = new Map<string, IrDiagnostic[]>();
  const seen = new Set<string>();
  for (const d of diagnostics) {
    const key = `${d.code}\u0000${d.file}\u0000${d.span[0]}\u0000${d.span[1]}`;
    if (seen.has(key)) continue;
    seen.add(key);
    const list = byFile.get(d.file) ?? [];
    list.push(d);
    byFile.set(d.file, list);
  }
  const order = (file: string) => {
    const index = files.indexOf(file);
    return index === -1 ? files.length : index;
  };
  return [...byFile.keys()]
    .sort((a, b) => order(a) - order(b) || (a < b ? -1 : a > b ? 1 : 0))
    .map((file) => ({
      file,
      story: files.includes(file),
      diagnostics: (byFile.get(file) ?? []).sort(
        (a, b) => a.span[0] - b.span[0] || RANK[a.severity] - RANK[b.severity],
      ),
    }));
}

/** 重大度ごとの件数。 */
export function countBySeverity(
  diagnostics: readonly IrDiagnostic[],
): Record<Severity, number> {
  const counts: Record<Severity, number> = { error: 0, warning: 0, info: 0 };
  for (const d of diagnostics) counts[d.severity] += 1;
  return counts;
}
