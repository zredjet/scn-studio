// 検証の画面：プロジェクト全体の診断（段階0と段階1）を、ファイルごとにまとめる。

import type { IrDiagnostic, SceneSummary, Severity } from "../core/types.ts";

export interface FileDiagnostics {
  readonly file: string;
  /** 本文のファイル（`story/` の下の `*.scn`）か。本文なら、選ぶとエディタのその行へ移れる。 */
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

/**
 * W202 のうち、話者が設定DBに無いもの（2人以上に当てはまるものは除く。登録しても解決しないため）の
 * 話者名。当てはまらなければ null。コアの診断の文で見分ける（文がそろっていることは、コアのテスト
 * `crates/scn-core/tests/assist.rs` の `unregistered_speaker_message` で確かめる）。
 */
export function unregisteredSpeaker(d: IrDiagnostic): string | null {
  if (d.code !== "W202") return null;
  return (
    /^話者 (.+) を設定DBのキャラクターに解決できません/.exec(d.message)?.[1] ??
    null
  );
}

/** 診断の位置：行の番号（1から）と、本文ならシーンのID（T4-13、U17）。 */
export interface DiagnosticPlace {
  readonly line: number;
  readonly scene: string | null;
}

/**
 * 1つのファイルの診断の位置を、そのファイルの中身から求める（`diagnostics` と同じ順）。改行（`\n`）は UTF-8 で
 * 1バイトなので、診断のバイト位置までの改行を数える。シーンは、そのファイルで診断の位置より前（同じ位置も）で
 * 始まる、いちばん後ろのシーン。
 */
export function diagnosticPlaces(
  diagnostics: readonly IrDiagnostic[],
  text: string,
  scenes: readonly SceneSummary[],
): DiagnosticPlace[] {
  const bytes = new TextEncoder().encode(text);
  const breaks: number[] = [];
  for (let i = 0; i < bytes.length; i++) if (bytes[i] === 0x0a) breaks.push(i);
  const lineOf = (byte: number): number => {
    // byte より前の改行の数（二分探索）。
    let lo = 0;
    let hi = breaks.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if ((breaks[mid] ?? 0) < byte) lo = mid + 1;
      else hi = mid;
    }
    return lo + 1;
  };
  return diagnostics.map((d) => {
    let scene: { id: string; start: number } | null = null;
    for (const s of scenes) {
      if (s.file !== d.file) continue;
      if (
        s.span[0] <= d.span[0] &&
        (scene === null || s.span[0] >= scene.start)
      )
        scene = { id: s.id, start: s.span[0] };
    }
    return { line: lineOf(d.span[0]), scene: scene?.id ?? null };
  });
}
