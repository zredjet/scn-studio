// 縦書きビュー（T2-5、D056。docs/ui/mockups/06b-preview-vertical.html）：道（共通、ルート、エンディング、
// プレビューで選んだ道）を選び、コアの通読（`route::read`）を一本道として縦書きで読む。確認用と位置づける
// （WebView の組版は OS で違う。厳密な組版は PDF・EPUB の書き出しで行う）。組み方は `vertical.ts`。

import { type CSSProperties, useEffect, useMemo, useState } from "react";
import {
  type PersistentValues,
  readingChoices,
  readRoute,
} from "../core/api.ts";
import type { PreviewData } from "../core/ir.ts";
import type {
  ReadingChoice,
  ReadingResult,
  ReadingTarget,
  RouteStep,
} from "../core/types.ts";
import type { PreviewLine } from "./PreviewScreen.tsx";
import {
  clampSize,
  manuscriptSheets,
  type PageLine,
  pageOfParagraph,
  paginate,
  paragraphsOf,
  type Token,
} from "./vertical.ts";

interface Props {
  /** プレビューのデータ（括弧。変わったら読み直す）。 */
  readonly data: PreviewData;
  /** ADV で選んだ選択肢の順（「プレビューで選んだ道」）。 */
  readonly steps: readonly RouteStep[];
  readonly persistent: PersistentValues;
  readonly onShowInEditor: (line: PreviewLine) => void;
}

/** 道の選択肢の見分け（select の value）。 */
function keyOf(target: ReadingTarget): string {
  switch (target.kind) {
    case "common":
      return "common";
    case "route":
      return `route:${target.name}`;
    case "ending":
      return `ending:${target.id}`;
    case "steps":
      return "steps";
  }
}

function TokenView({ token }: { readonly token: Token }) {
  switch (token.kind) {
    case "text":
      return <>{token.text}</>;
    case "ruby":
      return (
        <ruby>
          {token.base}
          <rt>{token.reading}</rt>
        </ruby>
      );
    case "dots":
      return <span className="v-dots">{token.text}</span>;
    case "tcy":
      return <span className="v-tcy">{token.text}</span>;
  }
}

function LineView({ line }: { readonly line: PageLine }) {
  return (
    <div className={`v-line ${line.kind}`}>
      {line.tokens.map((token, i) => (
        // 1行のまとまりは並びが変わらないので、位置で見分けてよい。
        // biome-ignore lint/suspicious/noArrayIndexKey: 行の中のまとまりは並べ替えない
        <TokenView key={i} token={token} />
      ))}
    </div>
  );
}

const STOP_TEXT: Readonly<Record<string, string>> = {
  ending: "",
  dead_end: "（ここで行き止まり）",
  route_changed: "（ここで共通のルートが終わります）",
  limit: "（長すぎるので、ここで止めました）",
};

export function VerticalView({
  data,
  steps,
  persistent,
  onShowInEditor,
}: Props) {
  const [choices, setChoices] = useState<readonly ReadingChoice[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [result, setResult] = useState<ReadingResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notes, setNotes] = useState(true);
  /** AI下書き（status: ai-draft）のシーンを含めるか（T3-9）。 */
  const [aiDrafts, setAiDrafts] = useState(true);
  // 入力欄の値はそのまま持ち、使うときに範囲に収める（打っている途中で値を書き換えない）。
  const [perLineText, setPerLineText] = useState("40");
  const [perPageText, setPerPageText] = useState("16");
  const perLine = clampSize(perLineText, 10, 60, 40);
  const perPage = clampSize(perPageText, 4, 40, 16);
  const [page, setPage] = useState(0);

  // 道の候補（本文が変わったら取り直す）。
  useEffect(() => {
    void data;
    let live = true;
    readingChoices()
      .then((c) => {
        if (live) setChoices(c);
      })
      .catch((e: unknown) => {
        if (live) setError(String(e));
      });
    return () => {
      live = false;
    };
  }, [data]);

  const options = useMemo(() => {
    const all: ReadingChoice[] = [...choices];
    if (steps.length > 0) {
      all.push({
        label: `プレビューで選んだ道（選択肢 ${steps.length} 個）`,
        target: { kind: "steps", steps },
      });
    }
    return all;
  }, [choices, steps]);
  const current =
    options.find((o) => keyOf(o.target) === selected) ?? options[0] ?? null;
  const persistentKey = JSON.stringify(persistent);
  // 描き直しのたびに道のオブジェクトが作り直されても、同じ道なら読み直さない。
  const targetKey = current === null ? null : JSON.stringify(current.target);

  // 通読する（道か本文が変わったら）。
  useEffect(() => {
    void data;
    if (targetKey === null) return;
    let live = true;
    readRoute(
      JSON.parse(targetKey) as ReadingTarget,
      JSON.parse(persistentKey) as PersistentValues,
      aiDrafts,
    )
      .then((r) => {
        if (!live) return;
        setResult(r);
        setError(null);
      })
      .catch((e: unknown) => {
        if (live) setError(String(e));
      });
    return () => {
      live = false;
    };
  }, [targetKey, data, persistentKey, aiDrafts]);

  const paragraphs = useMemo(
    () =>
      result === null
        ? []
        : paragraphsOf(result.reading.items, data.brackets, notes),
    [result, data.brackets, notes],
  );
  const pages = useMemo(
    () => paginate(paragraphs, perLine, perPage),
    [paragraphs, perLine, perPage],
  );
  const sheets = useMemo(() => manuscriptSheets(paragraphs), [paragraphs]);
  const shown = Math.min(page, pages.length - 1);
  const lines = pages[shown] ?? [];
  // シーンの見出しと、その段落の番号（同じシーンが2度出ても見分ける）。
  const scenes = paragraphs.flatMap((p, index) =>
    p.kind === "heading" ? [{ p, index }] : [],
  );
  const first = lines.find((l) => paragraphs[l.paragraph]?.file !== null);
  const firstParagraph =
    first === undefined ? undefined : paragraphs[first.paragraph];
  const stop = result === null ? "" : (STOP_TEXT[result.reading.stop] ?? "");

  return (
    <div className="vertical-body">
      <div className="vertical-main">
        <div className="vertical-toolbar">
          <label className="vertical-field">
            ルート
            <select
              value={current === null ? "" : keyOf(current.target)}
              onChange={(event) => {
                setSelected(event.target.value);
                setPage(0);
              }}
            >
              {options.map((o) => (
                <option key={keyOf(o.target)} value={keyOf(o.target)}>
                  {o.label}
                </option>
              ))}
            </select>
          </label>
          <label className="vertical-field">
            <input
              type="checkbox"
              checked={notes}
              onChange={(event) => setNotes(event.target.checked)}
            />
            選んだ選択肢を注記
          </label>
          <label className="vertical-field">
            <input
              type="checkbox"
              checked={aiDrafts}
              onChange={(event) => setAiDrafts(event.target.checked)}
            />
            AI下書きを含める
          </label>
          <label className="vertical-field">
            <input
              type="number"
              min={10}
              max={60}
              value={perLineText}
              aria-label="1行の字数"
              onChange={(event) => setPerLineText(event.target.value)}
            />
            字×
            <input
              type="number"
              min={4}
              max={40}
              value={perPageText}
              aria-label="1ページの行数"
              onChange={(event) => setPerPageText(event.target.value)}
            />
            行
          </label>
          <span className="flow-spacer" />
          <span className="vertical-count">
            {shown + 1} / {pages.length}ページ · 原稿用紙換算 {sheets}枚
          </span>
        </div>
        {error !== null && (
          <p className="vertical-error" role="alert">
            {error}
          </p>
        )}
        {result !== null && result.reading.stop === "unreachable" ? (
          <p className="placeholder">
            {result.label} には着けません：{result.reading.message ?? ""}
          </p>
        ) : (
          <div className="vertical-stage">
            <button
              type="button"
              className="vertical-turn"
              aria-label="次のページ"
              disabled={shown >= pages.length - 1}
              onClick={() => setPage(shown + 1)}
            >
              ‹
            </button>
            <div
              className="vertical-page"
              style={
                { "--v-chars": perLine, "--v-lines": perPage } as CSSProperties
              }
            >
              {lines.map((line, i) => (
                // biome-ignore lint/suspicious/noArrayIndexKey: ページの行は並べ替えない
                <LineView key={i} line={line} />
              ))}
              {shown === pages.length - 1 && stop !== "" && (
                <div className="v-line note">{stop}</div>
              )}
            </div>
            <button
              type="button"
              className="vertical-turn"
              aria-label="前のページ"
              disabled={shown <= 0}
              onClick={() => setPage(shown - 1)}
            >
              ›
            </button>
          </div>
        )}
        <p className="vertical-info">
          {perLine}字×{perPage}行 · 話者名は小説の形では出しません ·
          ルビ・傍点・縦中横に対応 · 確認用（厳密な組版は PDF・EPUB
          の書き出しで行う）
        </p>
      </div>
      <aside className="vertical-side" aria-label="シーン">
        <h2 className="preview-debug-heading">シーン</h2>
        <ul className="vertical-scenes">
          {scenes.map(({ p, index }) => (
            <li key={index}>
              <button
                type="button"
                className="small-button"
                onClick={() => {
                  const at = pageOfParagraph(pages, index);
                  if (at !== null) setPage(at);
                }}
              >
                {p.scene}{" "}
                {p.tokens
                  .map((t) => (t.kind === "ruby" ? t.base : t.text))
                  .join("")}
              </button>
            </li>
          ))}
        </ul>
        <button
          type="button"
          className="small-button"
          disabled={
            firstParagraph === undefined || firstParagraph.span === null
          }
          onClick={() => {
            if (
              firstParagraph === undefined ||
              firstParagraph.file === null ||
              firstParagraph.span === null
            ) {
              return;
            }
            onShowInEditor({
              file: firstParagraph.file,
              lineId: firstParagraph.lineId,
              span: firstParagraph.span,
            });
          }}
        >
          このページをエディタで表示
        </button>
      </aside>
    </div>
  );
}
