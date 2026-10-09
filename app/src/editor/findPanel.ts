// 本文の検索と置換の欄（見本 02b-editor-source.html の findbar。T4-20、U9）：CodeMirror の検索の欄を自前にする。
// 探す文字（件数「2 / 4」）・前へ・次へ・正規表現・大文字と小文字・語として・すべて選ぶ・閉じる、置き換える文字・
// 置換・すべて置換、
// 名前の変更（F2）への案内。置き換えは `@codemirror/search` のコマンドで行い、行IDにかかる変更は `lineid.ts` の
// `replaceFilter` が除く。変換中の Enter・Esc では動かない（.claude/rules/frontend.md）。作品全体と「本文だけ」は
// 作らない（N1）。

import {
  closeSearchPanel,
  findNext,
  findPrevious,
  getSearchQuery,
  replaceAll,
  replaceNext,
  SearchQuery,
  selectMatches,
  setSearchQuery,
} from "@codemirror/search";
import type { EditorState } from "@codemirror/state";
import {
  type EditorView,
  type Panel,
  runScopeHandlers,
  type ViewUpdate,
} from "@codemirror/view";
import { IS_MAC } from "../shell/palette.ts";

/** 数える一致の上限（それより多ければ「1000+」）。 */
export const COUNT_LIMIT = 1000;

/** 一致の数と、選んでいる一致の番号（1から。選んでいなければ 0）。 */
export interface MatchCount {
  readonly total: number;
  readonly current: number;
  /** 上限で数えるのをやめた。 */
  readonly capped: boolean;
}

/** 一致を数える（問い合わせの `test`（執筆表示で行IDを除く）も当てる）。 */
export function countMatches(
  state: EditorState,
  query: SearchQuery,
  limit = COUNT_LIMIT,
): MatchCount {
  if (!query.valid || query.search === "")
    return { total: 0, current: 0, capped: false };
  const sel = state.selection.main;
  let total = 0;
  let current = 0;
  const cursor = query.getCursor(state);
  for (let next = cursor.next(); !next.done; next = cursor.next()) {
    const { from, to } = next.value;
    if (
      query.test !== undefined &&
      !query.test(state.sliceDoc(from, to), state, from, to)
    )
      continue;
    total += 1;
    if (from === sel.from && to === sel.to) current = total;
    if (total >= limit) return { total, current, capped: true };
  }
  return { total, current, capped: false };
}

/** 件数の文（`2 / 4`、選んでいなければ `4件`、無ければ `0件`）。 */
export function countText(count: MatchCount): string {
  const total = count.capped ? `${count.total}+` : String(count.total);
  return count.current > 0 ? `${count.current} / ${total}` : `${total}件`;
}

function isComposing(event: KeyboardEvent): boolean {
  return event.isComposing || event.keyCode === 229;
}

const SVG = "http://www.w3.org/2000/svg";

function icon(paths: readonly string[], size = 15): SVGSVGElement {
  const svg = document.createElementNS(SVG, "svg");
  svg.setAttribute("width", String(size));
  svg.setAttribute("height", String(size));
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "2");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  svg.setAttribute("aria-hidden", "true");
  for (const d of paths) {
    const path = document.createElementNS(SVG, "path");
    path.setAttribute("d", d);
    svg.append(path);
  }
  return svg;
}

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Readonly<Record<string, string>>,
  ...children: (Node | string)[]
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, v);
  node.append(...children);
  return node;
}

/** `search({ createPanel })` に渡す、検索と置換の欄。 */
export function createFindPanel(view: EditorView): Panel {
  const query = getSearchQuery(view.state);
  const field = el("input", {
    type: "search",
    "aria-label": "探す文字",
    "main-field": "true",
    spellcheck: "false",
  });
  field.value = query.search;
  const counter = el("span", { class: "t-caption t-2 nowrap findbar-count" });
  const replaceField = el("input", {
    class: "input is-sm findbar-field",
    "aria-label": "置き換える文字",
    placeholder: "置き換える文字",
    spellcheck: "false",
  });
  replaceField.value = query.replace;
  const regexp = el(
    "button",
    {
      type: "button",
      class: "chip mono",
      title: "正規表現",
      "aria-pressed": String(query.regexp),
    },
    ".*",
  );
  const caseSensitive = el(
    "button",
    {
      type: "button",
      class: "chip",
      title: "大文字と小文字を区別",
      "aria-pressed": String(query.caseSensitive),
    },
    "Aa",
  );
  const wholeWord = el(
    "button",
    {
      type: "button",
      class: "chip",
      title: "語として探す",
      "aria-pressed": String(query.wholeWord),
    },
    "語",
  );
  const button = (label: string, title: string, child: Node | string) =>
    el(
      "button",
      { type: "button", class: "btn btn-icon", "aria-label": label, title },
      child,
    );
  const prev = button(
    `前へ（${IS_MAC ? "⇧Enter" : "Shift+Enter"}）`,
    `前へ（${IS_MAC ? "⇧Enter" : "Shift+Enter"}）`,
    icon(["M15 6l-6 6 6 6"]),
  );
  const next = button("次へ（Enter）", "次へ（Enter）", icon(["M9 6l6 6-6 6"]));
  const close = button(
    "検索を閉じる（Esc）",
    "検索を閉じる（Esc）",
    icon(["M6 6l12 12M18 6L6 18"]),
  );
  const selectAll = el(
    "button",
    { type: "button", class: "btn btn-sm", title: "一致をすべて選ぶ" },
    "すべて選ぶ",
  );
  const replaceOne = el(
    "button",
    { type: "button", class: "btn btn-sm" },
    "置換",
  );
  const replaceEvery = el(
    "button",
    { type: "button", class: "btn btn-sm" },
    "すべて置換",
  );
  const kbd = el("kbd", {}, "F2");
  const hint = el(
    "span",
    { class: "t-small t-3 findbar-hint" },
    "変数・キャラ・シーンIDは ",
    kbd,
    " の「名前の変更」で、参照ごと変える。行IDは置換の対象にしない。",
  );
  const searchBox = el(
    "label",
    { class: "search is-sm findbar-field" },
    icon(["M20 20l-4.5-4.5"], 15),
    field,
    counter,
  );
  // 虫めがねの丸。
  const circle = document.createElementNS(SVG, "circle");
  circle.setAttribute("cx", "11");
  circle.setAttribute("cy", "11");
  circle.setAttribute("r", "6");
  searchBox.firstChild?.appendChild(circle);
  const dom = el(
    "div",
    { class: "findbar", role: "search", "aria-label": "検索と置換" },
    el(
      "div",
      { class: "findbar-row" },
      searchBox,
      prev,
      next,
      el("span", { class: "findbar-sep", "aria-hidden": "true" }),
      regexp,
      caseSensitive,
      wholeWord,
      selectAll,
      el("span", { class: "grow" }),
      close,
    ),
    el(
      "div",
      { class: "findbar-row" },
      replaceField,
      replaceOne,
      replaceEvery,
      hint,
    ),
  );

  const commit = () => {
    const now = getSearchQuery(view.state);
    const q = new SearchQuery({
      search: field.value,
      replace: replaceField.value,
      regexp: regexp.getAttribute("aria-pressed") === "true",
      caseSensitive: caseSensitive.getAttribute("aria-pressed") === "true",
      wholeWord: wholeWord.getAttribute("aria-pressed") === "true",
      literal: now.literal,
    });
    if (!q.eq(now)) view.dispatch({ effects: setSearchQuery.of(q) });
  };
  const toggle = (b: HTMLButtonElement) => () => {
    b.setAttribute(
      "aria-pressed",
      String(b.getAttribute("aria-pressed") !== "true"),
    );
    commit();
  };
  const showCount = (state: EditorState) => {
    const q = getSearchQuery(state);
    counter.textContent =
      q.search === ""
        ? ""
        : q.valid
          ? countText(countMatches(state, q))
          : "正しくない式";
  };

  field.addEventListener("input", commit);
  replaceField.addEventListener("input", commit);
  regexp.addEventListener("click", toggle(regexp));
  caseSensitive.addEventListener("click", toggle(caseSensitive));
  wholeWord.addEventListener("click", toggle(wholeWord));
  selectAll.addEventListener("click", () => selectMatches(view));
  prev.addEventListener("click", () => findPrevious(view));
  next.addEventListener("click", () => findNext(view));
  close.addEventListener("click", () => closeSearchPanel(view));
  replaceOne.addEventListener("click", () => replaceNext(view));
  replaceEvery.addEventListener("click", () => replaceAll(view));
  dom.addEventListener("keydown", (event) => {
    // 変換を確定する Enter や、変換をやめる Esc では動かない。
    if (isComposing(event)) return;
    // 検索のキー（Esc で閉じる、Mod-G など）。
    if (runScopeHandlers(view, event, "search-panel")) {
      event.preventDefault();
      return;
    }
    if (event.key !== "Enter") return;
    if (event.target === field) {
      event.preventDefault();
      if (event.shiftKey) findPrevious(view);
      else findNext(view);
    } else if (event.target === replaceField) {
      event.preventDefault();
      replaceNext(view);
    }
  });
  showCount(view.state);

  return {
    dom,
    top: true,
    // 開いたら探す文字の欄へ（初めて開いたときは `openSearchPanel` が入れないため）。
    mount() {
      field.focus();
      field.select();
    },
    update(update: ViewUpdate) {
      const before = getSearchQuery(update.startState);
      const after = getSearchQuery(update.state);
      if (!after.eq(before)) {
        // ほかから問い合わせが変わった（選んだ文字で開いたときなど）。
        if (field.value !== after.search) field.value = after.search;
        if (replaceField.value !== after.replace)
          replaceField.value = after.replace;
        regexp.setAttribute("aria-pressed", String(after.regexp));
        caseSensitive.setAttribute("aria-pressed", String(after.caseSensitive));
        wholeWord.setAttribute("aria-pressed", String(after.wholeWord));
      }
      if (update.docChanged || update.selectionSet || !after.eq(before))
        showCount(update.state);
    },
  };
}
