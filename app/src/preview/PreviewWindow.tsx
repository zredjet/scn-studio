// プレビューの別ウィンドウ（T2-4、D055）：メインウィンドウと並べて使う。プレビューのデータは自分で取り、
// メインウィンドウとは Tauri のイベントでつなぐ。
//
// - メインウィンドウから：再生の頼み（`preview-request`。開いたときの頼みは `take_preview_request`）、
//   本文が変わった（`text-changed`。少し待ってからデータを取り直す）。
// - メインウィンドウへ：プレビュー中の行（`preview-line`。エディタの印）、その行をエディタで見せる
//   （`preview-show-in-editor`）。

import { useCallback, useEffect, useState } from "react";
import {
  assetUrl,
  closeThisWindow,
  onPreviewRequest,
  onTextChanged,
  previewData,
  sendPreviewLine,
  sendShowInEditor,
  takePreviewRequest,
} from "../core/api.ts";
import type { PreviewData } from "../core/ir.ts";
import type { PreviewRequest } from "../core/types.ts";
import { type PreviewLine, PreviewScreen } from "./PreviewScreen.tsx";

/** 本文が変わってから、データを取り直すまでの間。 */
const REFRESH_DELAY_MS = 300;

export function PreviewWindow() {
  const [data, setData] = useState<PreviewData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [request, setRequest] = useState<PreviewRequest | null>(null);
  const [version, setVersion] = useState(0);

  /** 頼みを受けたら、データを取り直してから一緒に渡す（本文を直した直後の頼みが、古いデータで動かないように）。 */
  const accept = useCallback((r: PreviewRequest) => {
    previewData()
      .then((d) => {
        setData(d);
        setError(null);
        setRequest(r);
      })
      .catch((e: unknown) => setError(String(e)));
  }, []);

  // 再生の頼み：開いたときの頼み（預けてある）と、開いた後の頼み（知らせ。預けたものも受け取って消す）。
  // 受け取ると消えるので、描き直しの片付けで捨てない（開発の StrictMode では、副作用が2回動く）。
  useEffect(() => {
    void takePreviewRequest().then((r) => {
      if (r !== null) accept(r);
    });
    const unlisten = onPreviewRequest((r) => {
      void takePreviewRequest();
      accept(r);
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, [accept]);

  // F5 でページを読み込み直さない（メインウィンドウでは「ここから再生」のキー）。
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "F5") event.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // 本文が変わったら、少し待ってからデータを取り直す。
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    const unlisten = onTextChanged(() => {
      if (timer !== null) clearTimeout(timer);
      timer = setTimeout(() => setVersion((n) => n + 1), REFRESH_DELAY_MS);
    });
    return () => {
      if (timer !== null) clearTimeout(timer);
      void unlisten.then((f) => f());
    };
  }, []);

  useEffect(() => {
    void version;
    let live = true;
    previewData()
      .then((d) => {
        if (live) {
          setData(d);
          setError(null);
        }
      })
      .catch((e: unknown) => {
        if (live) setError(String(e));
      });
    return () => {
      live = false;
    };
  }, [version]);

  const onLine = useCallback((line: PreviewLine | null) => {
    void sendPreviewLine(line);
  }, []);

  return (
    <div className="app preview-window-root">
      <section className="screen preview-screen" aria-label="プレビュー">
        <PreviewScreen
          data={data}
          error={error}
          title=""
          assetUrl={assetUrl}
          onShowInEditor={(line) => void sendShowInEditor(line)}
          onLine={onLine}
          onClose={() => void closeThisWindow()}
          closeLabel="閉じる"
          request={request}
        />
      </section>
    </div>
  );
}
