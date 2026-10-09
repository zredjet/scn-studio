// 企画書と設定DBの AI の下書き（T4-4・T4-5、D080）の見せ方：項目の名前、案の値の文、書いた結果の文。
// 書くのはコア（`write_concept_draft`・`adopt_field_proposal`）で、画面は案を並べて採用・却下させるだけ。

import type {
  EntriesWritten,
  FieldProposal,
  FieldsWritten,
  JsonValue,
} from "../core/types.ts";

/** 本文の項目（コアの `provenance::BODY_KEY`）。 */
const BODY_KEY = "#body";

/** 企画書の項目の名前。 */
export const CONCEPT_FIELD_LABELS: Readonly<Record<string, string>> = {
  logline: "ログライン",
  theme: "テーマ",
  tone: "トーン",
  volume: "分量",
  main_characters: "主要キャラ",
  endings: "エンディング",
  [BODY_KEY]: "本文",
};

/** 項目の名前（知らない項目はキーのまま）。 */
export function fieldLabel(
  key: string,
  labels: Readonly<Record<string, string>> = CONCEPT_FIELD_LABELS,
): string {
  return (
    labels[key] ?? (key === BODY_KEY ? "本文" : key === "name" ? "名前" : key)
  );
}

/** 案や今の値を、読める1つの文にする。 */
export function proposalValueText(value: JsonValue): string {
  if (value === null) return "（空）";
  if (typeof value === "string") return value.trim() === "" ? "（空）" : value;
  if (typeof value === "number" || typeof value === "boolean")
    return String(value);
  if (Array.isArray(value)) {
    if (value.length === 0) return "（空）";
    return value
      .map((item) => {
        if (item !== null && typeof item === "object" && !Array.isArray(item)) {
          const o = item as { readonly [key: string]: JsonValue };
          const id = typeof o.id === "string" ? o.id : "";
          const rest = [o.name, o.intent]
            .filter((v): v is string => typeof v === "string" && v !== "")
            .join("：");
          return rest === "" ? id : `${id}（${rest}）`;
        }
        return proposalValueText(item);
      })
      .join("、");
  }
  const o = value as { readonly [key: string]: JsonValue };
  if ("chars" in o || "routes" in o) {
    const parts: string[] = [];
    if (typeof o.chars === "number")
      parts.push(`字数 ${o.chars.toLocaleString("ja-JP")}`);
    if (typeof o.routes === "number") parts.push(`ルート ${o.routes}`);
    if (parts.length > 0) return parts.join("・");
  }
  return JSON.stringify(value);
}

/** 書いた結果の文（何を書いたか、案の数、書けなかったわけ）。 */
export function fieldsWrittenText(
  written: FieldsWritten,
  labels: Readonly<Record<string, string>> = CONCEPT_FIELD_LABELS,
): string {
  const parts: string[] = [];
  parts.push(
    written.written.length > 0
      ? `${written.file} の空いている項目に書きました：${written.written
          .map((k) => fieldLabel(k, labels))
          .join("、")}`
      : written.notes.length > 0
        ? `${written.file} に書いた項目はありません（読まなかった・書けなかったわけは下に）`
        : `${written.file} に書く項目はありませんでした`,
  );
  if (written.proposals.length > 0)
    parts.push(
      `値のある項目への案 ${written.proposals.length} 件（採用を選べます）`,
    );
  if (written.provenance_problem !== null)
    parts.push(`出自を記録できませんでした：${written.provenance_problem}`);
  return parts.join("。");
}

/** 設定の下書きを書いた結果の文（T4-5）：作ったエントリ、空欄に書いたエントリ、案の数、書かなかったものの数。 */
export function entriesWrittenText(written: EntriesWritten): string {
  const parts: string[] = [];
  if (written.created.length > 0)
    parts.push(
      `設定DBにエントリを作りました（status: ai-draft）：${written.created
        .map(entryName)
        .join("、")}`,
    );
  const filled = written.files.filter(
    (f) => !written.created.includes(f.file) && f.written.length > 0,
  );
  if (filled.length > 0)
    parts.push(
      `空いている項目に書きました：${filled
        .map(
          (f) =>
            `${entryName(f.file)}（${f.written.map((k) => fieldLabel(k, {})).join("、")}）`,
        )
        .join("、")}`,
    );
  const proposals = written.files.reduce((n, f) => n + f.proposals.length, 0);
  if (proposals > 0)
    parts.push(`値のある項目への案 ${proposals} 件（採用を選べます）`);
  if (written.notes.length > 0)
    parts.push(`書かなかったもの ${written.notes.length} 件（わけは下に）`);
  if (written.provenance_problem !== null)
    parts.push(`出自を記録できませんでした：${written.provenance_problem}`);
  return parts.length === 0 ? "書くものはありませんでした" : parts.join("。");
}

/** エントリのパスから、型とIDの見せ方（`character/sora`）。 */
export function entryName(path: string): string {
  return path.replace(/^bible\/entries\//, "").replace(/\.md$/, "");
}

/** 案を画面の中で見分けるキー（採用・却下を覚える）。 */
export function fieldProposalKey(run: number, p: FieldProposal): string {
  return `${run}\u0000${p.file}\u0000${p.key}`;
}
