// 書き出しのプラグインと PDF の組版（T3-14、D075）の画面の判断：作品フォルダのコマンドを実行する前の確かめ方と、
// 結果の見せ方。

import type { PluginItem, PluginRun } from "../core/types.ts";

/** 作品フォルダのコマンドを実行する前に見せる文。 */
export function approvalMessage(profile: string, commandLine: string): string {
  return [
    `作品フォルダの ${profile} に書かれたコマンドを実行します。`,
    "",
    commandLine,
    "",
    "コマンドは、この端末であなたの権限で動き、ファイルを読み書きできます。中身を知っている作品フォルダでだけ実行してください。",
    "この端末では、次からは確かめません（コマンドが変われば、また確かめます）。",
  ].join("\n");
}

/** 確かめ直す回数の上限（見せている間に、何度もコマンドが変わるとき）。 */
const MAX_ASKS = 3;

/**
 * 実行する。確かめが要る（`needs_approval`）なら、人にコマンドを見せて聞き、「はい」なら見せたコマンドの1行を
 * 添えて呼び直す（コアは、今のコマンドが見せたものと同じときだけ実行する。見せている間に変わっていれば、
 * また `needs_approval` が返るので、新しいコマンドを見せて聞き直す）。断られたら null。
 */
export async function runApproved(
  run: (approved: string | null) => Promise<PluginRun>,
  ask: (message: string, title: string) => Promise<boolean>,
): Promise<PluginRun | null> {
  let result = await run(null);
  for (let i = 0; i < MAX_ASKS; i += 1) {
    const shown = result.needs_approval;
    if (typeof shown !== "string") return result;
    const ok = await ask(
      approvalMessage(result.profile ?? "", shown),
      "作品フォルダのコマンドを実行",
    );
    if (!ok) return null;
    result = await run(shown);
  }
  return typeof result.needs_approval === "string" ? null : result;
}

/** 画面に見せるコマンドの出力の後ろの部分（コアは後ろの 64KB を返す。長ければ前を切る）。 */
export function outputTail(text: string, limit = 4000): string {
  return text.length <= limit ? text : `…${text.slice(text.length - limit)}`;
}

/** プラグインの種類の名前。 */
export function pluginKindLabel(kind: PluginItem["kind"]): string {
  switch (kind) {
    case "template":
      return "テンプレート";
    case "command":
      return "外部コマンド";
    default:
      return "読めません";
  }
}

/** 書き出せたときの知らせ（失敗や、まだ何もしていなければ null）。 */
export function pluginRunText(run: PluginRun): string | null {
  if (typeof run.error === "string" || typeof run.path !== "string")
    return null;
  return typeof run.pdf === "string"
    ? `${run.path} に書き出し、${run.pdf} を組みました`
    : `${run.path} に書き出しました`;
}
