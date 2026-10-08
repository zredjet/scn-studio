// 設定DBの画面の「AIに渡す内容」（docs/ui/mockups/04-bible.html、T3-6、D067）の文。データはコアの
// `ai::entry_context`（逆引きと一緒に受け取る）。

import type { EntryAiContext } from "../core/types.ts";

export interface AiContextText {
  /** 全部を渡すときの部分と字数、どのシーンで渡るか。 */
  readonly lead: string;
  /** タスクごとの、渡す部分と字数。 */
  readonly tasks: readonly string[];
}

/** 「AIに渡す内容」の文。このエントリを文脈に入れるタスクが無ければ null。 */
export function aiContextText(ai: EntryAiContext | null): AiContextText | null {
  if (ai === null || ai.tasks.length === 0) return null;
  const chars = ai.parts.reduce((sum, p) => sum + p.chars, 0);
  const labels = ai.parts.map((p) => p.label).join("・");
  const where = ai.character ? "登場するシーン" : "名前が出るシーン";
  return {
    lead: `${labels}（約${chars}字）。${where}でAIタスクを実行すると、この内容が文脈に入ります（シーンを選ばないタスクでは、いつも入ります）。`,
    tasks: ai.tasks.map(
      (t) => `${t.name}：${t.labels.join("・")}（約${t.chars}字）`,
    ),
  };
}
