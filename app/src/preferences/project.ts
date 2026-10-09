// 環境設定の「この作品」の頁（T4-25、N6）：欄の値から `project.toml` の直し方を決める。書くのはコア
// （`set_project_settings`。コメントと並びを残し、読み直して確かめる）。React と DOM に依存させない（テストする）。

import type { ProjectSettings, SettingEdit } from "../core/types.ts";
import { parseContextTokens } from "./connection.ts";

/** `[screen]` が無いときのプレビューの画面の大きさ（仕様書の既定）。 */
export const DEFAULT_SCREEN = { width: 1280, height: 720 } as const;
/** はみ出しを調べ始めるときに書くメッセージウィンドウの大きさ（プレビューの既定と同じ）。 */
export const DEFAULT_WINDOW = { lines: 3, chars_per_line: 24 } as const;

/** 正の整数の欄（空は null。全角の数字と桁区切りも読む）。 */
export const parsePositive = parseContextTokens;

/** 書くか：変わっていない・正しくない（わけ）・書く（直し方）。 */
export type EditPlan =
  | { readonly kind: "none" }
  | { readonly kind: "invalid"; readonly message: string }
  | { readonly kind: "save"; readonly edits: readonly SettingEdit[] };

const NONE: EditPlan = { kind: "none" };

function save(...edits: SettingEdit[]): EditPlan {
  return { kind: "save", edits };
}

function invalid(message: string): EditPlan {
  return { kind: "invalid", message };
}

/** 作品名（空なら消す。アプリはフォルダ名で見せる）。 */
export function titlePlan(s: ProjectSettings, text: string): EditPlan {
  const value = text.trim();
  if (value === (s.title ?? "")) return NONE;
  return save({ path: ["title"], value: value === "" ? null : value });
}

/** 開始シーン（空なら消す。最初のシーンから始める）。 */
export function startScenePlan(s: ProjectSettings, id: string): EditPlan {
  if (id === (s.start_scene ?? "")) return NONE;
  return save({ path: ["start_scene"], value: id === "" ? null : id });
}

/** 画面の大きさ（両方を空にすると表ごと消して既定の大きさ。片方だけは書かない）。 */
export function screenPlan(
  s: ProjectSettings,
  width: string,
  height: string,
): EditPlan {
  const w = parsePositive(width);
  const h = parsePositive(height);
  if (w === "invalid" || h === "invalid")
    return invalid("画面の幅と高さは正の整数で書きます");
  if (w === null && h === null)
    return s.screen === null ? NONE : save({ path: ["screen"], value: null });
  if (w === null || h === null)
    return invalid(
      "画面の幅と高さは両方を書きます（両方を空にすると既定の 1280×720）",
    );
  if (s.screen !== null && s.screen.width === w && s.screen.height === h)
    return NONE;
  return save(
    { path: ["screen", "width"], value: w },
    { path: ["screen", "height"], value: h },
  );
}

/** はみ出しを調べるか（調べるなら既定の大きさで書き、やめるなら表ごと消す）。 */
export function windowTogglePlan(s: ProjectSettings, on: boolean): EditPlan {
  if (on === (s.message_window !== null)) return NONE;
  return on
    ? save(
        { path: ["message_window", "lines"], value: DEFAULT_WINDOW.lines },
        {
          path: ["message_window", "chars_per_line"],
          value: DEFAULT_WINDOW.chars_per_line,
        },
      )
    : save({ path: ["message_window"], value: null });
}

/** メッセージウィンドウの行数と1行の字数（両方とも正の整数）。 */
export function windowPlan(
  s: ProjectSettings,
  lines: string,
  chars: string,
): EditPlan {
  const l = parsePositive(lines);
  const c = parsePositive(chars);
  if (l === null || c === null || l === "invalid" || c === "invalid")
    return invalid("行数と1行の字数は、正の整数で書きます");
  const now = s.message_window;
  if (now !== null && now.lines === l && now.chars_per_line === c) return NONE;
  return save(
    { path: ["message_window", "lines"], value: l },
    { path: ["message_window", "chars_per_line"], value: c },
  );
}

/** エディタの入力補助（`[editor]` の2つ）。 */
export function editorPlan(
  s: ProjectSettings,
  key: "normalize_ellipsis" | "indent_narration",
  on: boolean,
): EditPlan {
  if (s.editor[key] === on) return NONE;
  return save({ path: ["editor", key], value: on });
}

/** 括弧の組の表示スタイル。 */
export function bracketStylePlan(
  s: ProjectSettings,
  pair: string,
  style: string,
): EditPlan {
  const value = style.trim();
  if (value === "")
    return invalid(
      "表示スタイルは英字の名前で書きます（normal・thought など）",
    );
  if (s.brackets.find((b) => b.pair === pair)?.style === value) return NONE;
  return save({ path: ["dialogue", "brackets", pair], value });
}

/** 括弧の組を足す（開き括弧と閉じ括弧の2文字）。 */
export function bracketAddPlan(
  s: ProjectSettings,
  pair: string,
  style: string,
): EditPlan {
  const written = pair.trim();
  if (Array.from(written).length !== 2)
    return invalid("括弧は、開き括弧と閉じ括弧の2文字で書きます（例：『』）");
  if (s.brackets.some((b) => b.pair === written))
    return invalid(`${written} はもうあります`);
  return bracketStylePlan(s, written, style);
}

/** 括弧の組を消す（1組は残す）。 */
export function bracketRemovePlan(s: ProjectSettings, pair: string): EditPlan {
  if (!s.brackets.some((b) => b.pair === pair)) return NONE;
  if (s.brackets.length <= 1) return invalid("台詞の括弧は、1組は残します");
  return save({ path: ["dialogue", "brackets", pair], value: null });
}
