// AI の操作の印（緑青の ✦。docs/ui/decisions-v1.md の U07、見本 00-components.html の形）。
// 副のボタンの中に置いて、AI に頼む操作だと分かるようにする。止まっているボタンでは灰色になる（components.css）。
// 接続先のせいで使えないときは、鍵の印にする（U03。わけはボタンの説明に書く）。

import { Icon } from "./icons.tsx";

export function AiMark({
  size = 14,
  locked = false,
}: {
  readonly size?: number;
  readonly locked?: boolean;
}) {
  return locked ? (
    <Icon name="lock" size={size - 2} stroke={2} className="ai-lock" />
  ) : (
    <Icon name="ai" size={size} className="ai-mark" />
  );
}
