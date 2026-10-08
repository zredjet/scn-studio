import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@xyflow/react/dist/style.css";
import "./styles/tokens.css";
import "./styles/app.css";
import "./styles/preview.css";
import { App } from "./App.tsx";
import { isPreviewWindow } from "./core/api.ts";
import { PreviewWindow } from "./preview/PreviewWindow.tsx";

const root = document.getElementById("root");
if (root !== null) {
  createRoot(root).render(
    <StrictMode>
      {/* プレビューの別ウィンドウ（T2-4）は、プレビューの画面だけを出す。 */}
      {isPreviewWindow() ? <PreviewWindow /> : <App />}
    </StrictMode>,
  );
}
