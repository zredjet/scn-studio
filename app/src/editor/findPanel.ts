// 本文の検索と置換の欄（見本 02b-editor-source.html の findbar。T4-20、U9）：CodeMirror の検索の欄を自前にする。
// 探す文字（件数「2 / 4」）・前へ・次へ・正規表現・大文字と小文字・語として・すべて選ぶ・本文だけ・探す範囲・閉じる、
// 置き換える文字・置換・すべて置換、名前の変更（F2）への案内。置き換えは `@codemirror/search` のコマンドで行い、
// 行IDにかかる変更は `lineid.ts` の `replaceFilter` が除く。変換中の Enter・Esc では動かない
// （.claude/rules/frontend.md）。
// 探す範囲（T4-22、N1）：このファイル・このシーン（問い合わせの `test` で今のシーンの範囲に絞る）・すべての章（この章は
// 同じに光らせ、すべての章の一致は App が右の欄に並べる。`findContext` の `onFindAll`）。「本文だけ」は、コアの本文の
// 範囲（台詞と地の文）に入る一致だけ。範囲は App が `findContext` で渡す（エディタの位置）。

import {
  closeSearchPanel,
  findNext,
  findPrevious,
  getSearchQuery,
  openSearchPanel,
  replaceAll,
  replaceNext,
  SearchQuery,
  searchPanelOpen,
  selectMatches,
  setSearchQuery,
} from "@codemirror/search";
import {
  type EditorState,
  Facet,
  StateEffect,
  StateField,
} from "@codemirror/state";
import {
  EditorView,
  type Panel,
  runScopeHandlers,
  type ViewUpdate,
} from "@codemirror/view";
import { IS_MAC } from "../shell/palette.ts";
import { type FindSpec, insideRanges } from "./projectSearch.ts";

/** 探す範囲。 */
export type FindScope = "file" | "scene" | "chapters";

/** 検索の欄が App へ知らせること（T4-22）。 */
export interface FindContext {
  /** すべての章を探す（null で探すのをやめる）。 */
  readonly onFindAll: (spec: FindSpec | null) => void;
  /** 本文の範囲が要る（「本文だけ」か「すべての章」を選んだ）。 */
  readonly onNeedRanges: () => void;
  /** 検索の欄を閉じた（章の切り替えで開き直すときは知らせない）。 */
  readonly onClosed: () => void;
}

const NO_CONTEXT: FindContext = {
  onFindAll: () => {},
  onNeedRanges: () => {},
  onClosed: () => {},
};

export const findContext = Facet.define<FindContext, FindContext>({
  combine: (values) => values[values.length - 1] ?? NO_CONTEXT,
});

type Ranges = readonly (readonly [number, number])[];

/** この章の本文の範囲と今のシーンの範囲（エディタの位置）。まだ無ければ null。 */
export interface FindRanges {
  readonly body: Ranges | null;
  readonly scene: readonly [number, number] | null;
}

/** 範囲を入れる（入れたものだけ入れ替える）。 */
export const setFindRanges = StateEffect.define<Partial<FindRanges>>();

/**
 * 範囲は、コアが作ったときの本文で求めて入れ、その後の編集ではずらして使う（新しい範囲が届くまで、ずれたままに
 * しない）。範囲の端に打った文字は範囲に入れる。
 */
export const findRanges = StateField.define<FindRanges>({
  create: () => ({ body: null, scene: null }),
  update(value, tr) {
    let next = value;
    if (tr.docChanged) {
      const map = ([from, to]: readonly [number, number]) =>
        [tr.changes.mapPos(from, -1), tr.changes.mapPos(to, 1)] as const;
      next = {
        body: next.body === null ? null : next.body.map(map),
        scene: next.scene === null ? null : map(next.scene),
      };
    }
    for (const e of tr.effects) {
      if (e.is(setFindRanges)) next = { ...next, ...e.value };
    }
    return next;
  },
});

/** 検索の欄を閉じたことを App へ知らせる（章の切り替えの `setState` では呼ばれない）。 */
export const findPanelWatch = EditorView.updateListener.of((update) => {
  if (searchPanelOpen(update.startState) && !searchPanelOpen(update.state))
    update.state.facet(findContext).onClosed();
});

/** 探す範囲と「本文だけ」（エディタごとに覚える。章を切り替えて欄を開き直しても残す）。 */
interface FindOptions {
  readonly scope: FindScope;
  readonly bodyOnly: boolean;
}

const optionsOf = new WeakMap<EditorView, FindOptions>();

/** 章を切り替えて開き直す欄（すべての章を探し直さない。× で閉じた結果を戻さないため）。 */
const reopening = new WeakSet<EditorView>();

/** 章を切り替えたときに、同じ問い合わせで欄を開き直す（すべての章の結果は App が持ったまま）。 */
export function reopenFindPanel(view: EditorView): void {
  reopening.add(view);
  try {
    openSearchPanel(view);
  } finally {
    reopening.delete(view);
  }
}

type SearchTest = NonNullable<SearchQuery["test"]>;

function inScene(state: EditorState, from: number, to: number): boolean {
  const r = state.field(findRanges, false)?.scene ?? null;
  return r !== null && from >= r[0] && to <= r[1];
}

function inBody(state: EditorState, from: number, to: number): boolean {
  const ranges = state.field(findRanges, false)?.body ?? null;
  return ranges !== null && insideRanges(ranges, from, to);
}

// 範囲の絞り方ごとに同じ関数を使う（問い合わせが同じなら `SearchQuery.eq` で同じと見るため）。範囲は呼ばれたときの
// 状態から読む。
const SCENE_TEST: SearchTest = (_m, state, from, to) =>
  inScene(state, from, to);
const BODY_TEST: SearchTest = (_m, state, from, to) => inBody(state, from, to);
const SCENE_BODY_TEST: SearchTest = (_m, state, from, to) =>
  inScene(state, from, to) && inBody(state, from, to);

/** 探す範囲と「本文だけ」の `test`（この章だけ。すべての章は App が探す）。 */
export function findTest(options: FindOptions): SearchTest | undefined {
  if (options.scope === "scene")
    return options.bodyOnly ? SCENE_BODY_TEST : SCENE_TEST;
  return options.bodyOnly ? BODY_TEST : undefined;
}

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
  let options: FindOptions = optionsOf.get(view) ?? {
    scope: "file",
    bodyOnly: false,
  };
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
  const bodyOnly = el(
    "button",
    {
      type: "button",
      class: "chip",
      title: "台詞と地の文だけを探す（演出・ID・コメントを除く）",
      "aria-pressed": String(options.bodyOnly),
    },
    "本文だけ",
  );
  const scope = el(
    "select",
    { class: "select is-sm findbar-scope", "aria-label": "探す範囲" },
    el("option", { value: "file" }, "このファイル"),
    el("option", { value: "chapters" }, "すべての章"),
    el("option", { value: "scene" }, "このシーン"),
  );
  scope.value = options.scope;
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
      bodyOnly,
      scope,
      el("span", { class: "grow" }),
      close,
    ),
    el(
      "div",
      { class: "findbar-row" },
      replaceField,
      replaceOne,
      replaceEvery,
      selectAll,
      hint,
    ),
  );

  const pressed = (b: HTMLButtonElement) =>
    b.getAttribute("aria-pressed") === "true";
  /** すべての章を探す頼み（すべての章でなければ、やめる）。 */
  const findAll = () => {
    const context = view.state.facet(findContext);
    context.onFindAll(
      options.scope === "chapters" && field.value !== ""
        ? {
            search: field.value,
            regexp: pressed(regexp),
            caseSensitive: pressed(caseSensitive),
            wholeWord: pressed(wholeWord),
            bodyOnly: options.bodyOnly,
          }
        : null,
    );
  };
  /** 自分で問い合わせを入れている間（ほかから入った問い合わせと見分ける）。 */
  let dispatching = false;
  /** 探す文字の欄で変換している間（すべての章は、変換が終わってから探す）。 */
  let composing = false;
  /** 問い合わせを入れる。`force` なら同じ問い合わせでも入れ直す（光らせ直す）。 */
  const apply = (force: boolean) => {
    const now = getSearchQuery(view.state);
    const q = new SearchQuery({
      search: field.value,
      replace: replaceField.value,
      regexp: pressed(regexp),
      caseSensitive: pressed(caseSensitive),
      wholeWord: pressed(wholeWord),
      literal: now.literal,
      test: findTest(options),
    });
    if (!force && q.eq(now)) return;
    dispatching = true;
    try {
      view.dispatch({ effects: setSearchQuery.of(q) });
    } finally {
      dispatching = false;
    }
  };
  /** 欄を直した：問い合わせを入れ、すべての章も探し直す。 */
  const commit = (force = false) => {
    apply(force);
    if (!composing) findAll();
  };
  /** 範囲が変わった：光らせ直すだけ（すべての章は App が範囲の読み直しで探し直す。閉じた結果も戻さない）。 */
  const refresh = () => apply(true);
  const needRanges = () => {
    if (options.bodyOnly || options.scope === "chapters")
      view.state.facet(findContext).onNeedRanges();
  };
  const setOptions = (next: FindOptions) => {
    options = next;
    optionsOf.set(view, next);
    needRanges();
    commit(true);
  };
  const toggle = (b: HTMLButtonElement) => () => {
    b.setAttribute("aria-pressed", String(!pressed(b)));
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

  field.addEventListener("input", () => commit());
  field.addEventListener("compositionstart", () => {
    composing = true;
  });
  field.addEventListener("compositionend", () => {
    composing = false;
    commit();
  });
  replaceField.addEventListener("input", () => commit());
  bodyOnly.addEventListener("click", () => {
    bodyOnly.setAttribute("aria-pressed", String(!options.bodyOnly));
    setOptions({ ...options, bodyOnly: !options.bodyOnly });
  });
  scope.addEventListener("change", () => {
    const value = scope.value;
    setOptions({
      ...options,
      scope: value === "chapters" || value === "scene" ? value : "file",
    });
  });
  regexp.addEventListener("click", toggle(regexp));
  caseSensitive.addEventListener("click", toggle(caseSensitive));
  wholeWord.addEventListener("click", toggle(wholeWord));
  selectAll.addEventListener("click", () => selectMatches(view));
  prev.addEventListener("click", () => findPrevious(view));
  next.addEventListener("click", () => findNext(view));
  // 閉じたことは `findPanelWatch` が App へ知らせる（すべての章の結果も消す。Esc がエディタにあっても同じ）。
  const closePanel = () => {
    closeSearchPanel(view);
  };
  close.addEventListener("click", closePanel);
  replaceOne.addEventListener("click", () => replaceNext(view));
  replaceEvery.addEventListener("click", () => replaceAll(view));
  dom.addEventListener("keydown", (event) => {
    // 変換を確定する Enter や、変換をやめる Esc では動かない。
    if (isComposing(event)) return;
    if (event.key === "Escape") {
      event.preventDefault();
      closePanel();
      return;
    }
    // 検索のキー（Mod-G など）。
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
  needRanges();
  // 章を切り替えて開き直した欄は、覚えた範囲で光らせ直すだけ（すべての章は頼み直さない）。新しく開いた欄は、
  // すべての章を覚えていれば探し直す（CodeMirror は作ったときの更新で `update` を呼ばないため）。
  if (
    !reopening.has(view) &&
    options.scope === "chapters" &&
    query.search !== ""
  )
    queueMicrotask(() => commit(true));
  else if (findTest(options) !== undefined || query.test !== undefined)
    queueMicrotask(refresh);

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
      if (!after.eq(before) && !dispatching) {
        // ほかから問い合わせが入った（欄を開いたまま、選んだ文字で ⌘F など）。欄をそろえ、範囲の `test` を付け直し、
        // すべての章も探し直す（更新の中では入れられないので、後で）。
        if (field.value !== after.search) field.value = after.search;
        if (replaceField.value !== after.replace)
          replaceField.value = after.replace;
        regexp.setAttribute("aria-pressed", String(after.regexp));
        caseSensitive.setAttribute("aria-pressed", String(after.caseSensitive));
        wholeWord.setAttribute("aria-pressed", String(after.wholeWord));
        queueMicrotask(() => commit(true));
      }
      // 本文の範囲や今のシーンを入れ直したら、光らせ直す。
      const rangesChanged = update.transactions.some((tr) =>
        tr.effects.some((e) => e.is(setFindRanges)),
      );
      if (rangesChanged && findTest(options) !== undefined)
        queueMicrotask(refresh);
      if (
        update.docChanged ||
        update.selectionSet ||
        rangesChanged ||
        !after.eq(before)
      )
        showCount(update.state);
    },
  };
}
