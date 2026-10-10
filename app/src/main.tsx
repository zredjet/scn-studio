import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@xyflow/react/dist/style.css";
import "./styles/tokens.css";
import "./styles/components.css";
import "./styles/app.css";
import "./styles/preview.css";
import { App } from "./App.tsx";
import { isPreviewWindow, setUiZoom } from "./core/api.ts";
import { PreviewWindow } from "./preview/PreviewWindow.tsx";
import { loadViewPrefs } from "./shell/viewPrefs.ts";

// 画面の倍率（T4-30）は、描き始める前に覚えた値に合わせるよう頼む（終わるのは待たない。プレビューの別ウィンドウも
// 開いたときに読む）。
void setUiZoom(loadViewPrefs().zoom).catch(() => {
  // 合わせられなくても、100% で使える（メインウィンドウは App が誤りを出す）。
});

const root = document.getElementById("root");
if (root !== null) {
  createRoot(root).render(
    <StrictMode>
      {/* プレビューの別ウィンドウ（T2-4）は、プレビューの画面だけを出す。 */}
      {isPreviewWindow() ? <PreviewWindow /> : <App />}
    </StrictMode>,
  );
}
