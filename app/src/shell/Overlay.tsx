// 重ね表示の部品（docs/ui/mockups/12-overlays.html、U19。T4-20）。
// - ダイアログ（`Dialog`）：後ろを暗くし、真ん中に出す。Esc で閉じる（外を押しても閉じない。打ちかけを失わないため）。
// - 探す窓（`Finder`）：⌘K・⌘P。後ろを暗くし、窓の上から 72px に出す。Esc と外を押すと閉じる。
// - ポップオーバー（暗くしない、外を押すと閉じる）はタイトルバーのメニューと AI の様子（`TitleBar.tsx` の `tb-pop`）。
// 幅は 320・480・600・960 の4つ。Esc とフォーカスの戻し方は `useOverlay`（T4-14、U12）。

import type { ReactNode } from "react";
import { Icon } from "./icons.tsx";
import { useOverlay } from "./useOverlay.ts";

export type OverlayWidth = 320 | 480 | 600 | 960;

export function Dialog({
  label,
  title,
  width = 480,
  onClose,
  head,
  closable = true,
  foot,
  children,
}: {
  /** 読み上げの名前（無ければ題）。 */
  readonly label?: string;
  readonly title: ReactNode;
  readonly width?: OverlayWidth;
  readonly onClose: () => void;
  /** 題の右に並べるもの（札など）。 */
  readonly head?: ReactNode;
  /** 右上の閉じるボタン（答えを選ばせるダイアログでは出さない）。 */
  readonly closable?: boolean;
  /** 下の帯のボタン（主のボタンは右に1つ。消す・保存しないは左に離す）。 */
  readonly foot?: ReactNode;
  readonly children: ReactNode;
}) {
  const overlay = useOverlay<HTMLDivElement>(onClose);
  return (
    <div className="overlay-layer">
      <div className="overlay-scrim" aria-hidden="true" />
      <div
        className={`dialog overlay-box ov-${width}`}
        role="dialog"
        aria-modal="true"
        aria-label={label ?? (typeof title === "string" ? title : undefined)}
        ref={overlay.ref}
        tabIndex={-1}
        onKeyDown={overlay.onKeyDown}
      >
        <div className="dialog-head">
          <h2 className="h2">{title}</h2>
          {head}
          {closable && (
            <button
              type="button"
              className="btn btn-icon"
              aria-label="閉じる"
              title="閉じる（Esc）"
              onClick={onClose}
            >
              <Icon name="close" size={16} />
            </button>
          )}
        </div>
        <div className="dialog-body">{children}</div>
        {foot !== undefined && <div className="dialog-foot">{foot}</div>}
      </div>
    </div>
  );
}

export function Finder({
  label,
  onClose,
  children,
}: {
  readonly label: string;
  readonly onClose: () => void;
  readonly children: ReactNode;
}) {
  const overlay = useOverlay<HTMLDivElement>(onClose);
  return (
    <div className="overlay-layer is-top">
      <button
        type="button"
        className="overlay-scrim"
        aria-label="閉じる"
        tabIndex={-1}
        onClick={onClose}
      />
      <div
        className="popover overlay-box ov-600 finder"
        role="dialog"
        aria-modal="true"
        aria-label={label}
        ref={overlay.ref}
        tabIndex={-1}
        onKeyDown={overlay.onKeyDown}
      >
        {children}
      </div>
    </div>
  );
}
