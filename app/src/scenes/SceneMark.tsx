// シーンの状態の印（形で分ける。docs/ui/decisions-v1.md の U11、T4-18）：本文の画面のシーン一覧とフローのノード。

import { MARK_LABEL, type SceneMark } from "./list.ts";

export function SceneMarkView({ mark }: { readonly mark: SceneMark }) {
  return (
    <span className={`st st-${mark}`} title={MARK_LABEL[mark]}>
      <i />
    </span>
  );
}
