// 一度開いた画面を、ほかの画面へ移っても隠して残す（T4-13、U5）。

import { memo, type ReactNode } from "react";

/**
 * `active` が false の間は描き直さない。隠している画面は、選んでいるもの・絞り込み・スクロールをそのまま持ち、
 * 本文の解析のたびに描き直さない（見せたときに、新しい中身で描き直す）。隠すのは外の `hidden` で行う。
 */
export const KeepAlive = memo(
  function KeepAlive({
    children,
  }: {
    readonly active: boolean;
    readonly children: ReactNode;
  }) {
    return children;
  },
  (_previous, next) => !next.active,
);
