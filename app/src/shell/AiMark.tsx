// AI の操作の印（緑青の ✦。docs/ui/decisions-v1.md の U07、見本 00-components.html の形）。
// 副のボタンの中に置いて、AI に頼む操作だと分かるようにする。止まっているボタンでは灰色になる（components.css）。

export function AiMark({ size = 14 }: { readonly size?: number }) {
  return (
    <svg
      className="ai-mark"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M12 3l1.8 4.7 4.7 1.8-4.7 1.8L12 16l-1.8-4.7-4.7-1.8 4.7-1.8z" />
      <path d="M18.5 14.5l.8 2 2 .8-2 .8-.8 2-.8-2-2-.8 2-.8z" />
    </svg>
  );
}
