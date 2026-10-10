// 環境設定の頁（T4-25、N6。見本 11 の一覧のうち、今ある設定の頁）：このアプリの「文字と表示」「キーの割り当て」と、
// この作品（project.toml）の「作品の情報」「メッセージウィンドウ」「エディタ」。欄を離れたとき（変換中でない
// Enter、選んだとき）に保存する（Z1）。何を書くかは `project.ts`、書くのはコア。

import { type KeyboardEvent, type ReactNode, useEffect, useState } from "react";
import { isComposingKey } from "../bible/fields.ts";
import type {
  ProjectSettings,
  SceneSummary,
  SettingEdit,
} from "../core/types.ts";
import { IS_MAC, keyLabel } from "../shell/palette.ts";
import {
  DEFAULT_ZOOM,
  type ViewPrefs,
  ZOOM_CHOICES,
  zoomOf,
} from "../shell/viewPrefs.ts";
import { keyGroups } from "./keys.ts";
import {
  bracketAddPlan,
  bracketRemovePlan,
  bracketStylePlan,
  DEFAULT_SCREEN,
  DEFAULT_WINDOW,
  type EditPlan,
  editorPlan,
  screenPlan,
  startScenePlan,
  titlePlan,
  windowPlan,
  windowTogglePlan,
} from "./project.ts";

/** 表示の好み（この端末に覚える。App の `ViewPrefs`）。 */
export type ViewPrefsValue = ViewPrefs;

/** 欄の1行（見本 11 の pf-row：左にラベル、右に欄と説明）。 */
function Row({
  label,
  children,
}: {
  readonly label: string;
  readonly children: ReactNode;
}) {
  return (
    <div className="prefs-row">
      <span className="lbl prefs-row-label">{label}</span>
      <div className="prefs-row-body">{children}</div>
    </div>
  );
}

/** Enter（変換中でない）で決める。 */
function onEnter(commit: () => void) {
  return (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== "Enter" || isComposingKey(event)) return;
    event.preventDefault();
    commit();
  };
}

/** 1つの文字の欄（保存した値が変わったら、欄も合わせる）。 */
function TextField({
  value,
  label,
  placeholder,
  width,
  numeric,
  onCommit,
}: {
  readonly value: string;
  readonly label: string;
  readonly placeholder?: string;
  readonly width: number;
  readonly numeric?: boolean;
  readonly onCommit: (text: string) => void;
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  return (
    <input
      className={numeric ? "input mono" : "input"}
      style={{ width }}
      type="text"
      inputMode={numeric ? "numeric" : undefined}
      aria-label={label}
      value={draft}
      placeholder={placeholder}
      spellCheck={false}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={() => onCommit(draft)}
      onKeyDown={onEnter(() => onCommit(draft))}
    />
  );
}

// ---- このアプリ ----

/** 文字と表示（この端末に覚える）。 */
export function DisplayPage({
  view,
  onView,
}: {
  readonly view: ViewPrefsValue;
  readonly onView: (next: Partial<ViewPrefsValue>) => void;
}) {
  return (
    <div className="prefs-form">
      <p className="hint">この端末に覚えます（作品ごとではありません）。</p>
      <Row label="画面の倍率">
        <select
          className="select"
          style={{ width: 200 }}
          aria-label="画面の倍率"
          value={view.zoom}
          onChange={(e) => onView({ zoom: zoomOf(Number(e.target.value)) })}
        >
          {ZOOM_CHOICES.map((z) => (
            <option key={z} value={z}>
              {z}%{z === DEFAULT_ZOOM ? "（既定）" : ""}
            </option>
          ))}
        </select>
        <span className="hint">
          文字も欄もまとめて拡大・縮小します。プレビューの別ウィンドウは開き直すと合います
        </span>
      </Row>
      <Row label="本文の書体">
        <select
          className="select"
          style={{ width: 200 }}
          aria-label="本文の書体"
          value={view.gothic ? "gothic" : "mincho"}
          onChange={(e) => onView({ gothic: e.target.value === "gothic" })}
        >
          <option value="mincho">明朝</option>
          <option value="gothic">ゴシック</option>
        </select>
        <span className="hint">OS に入っている書体を使います</span>
      </Row>
      <Row label="集中モード">
        <label className="check">
          <input
            type="checkbox"
            checked={view.focus}
            onChange={(e) => onView({ focus: e.target.checked })}
          />
          本文の画面で、左端のバーと横の欄を隠す（
          {keyLabel("Mod-Shift-F", IS_MAC)}）
        </label>
      </Row>
      <Row label="タイプライタースクロール">
        <label className="check">
          <input
            type="checkbox"
            checked={view.typewriter}
            onChange={(e) => onView({ typewriter: e.target.checked })}
          />
          書いている行を、いつも画面の同じ高さに置く
        </label>
      </Row>
    </div>
  );
}

/** キーの割り当て（見るだけ）。 */
export function KeysPage() {
  return (
    <div className="prefs-form">
      <p className="hint">今は変えられません。見るだけです。</p>
      {keyGroups().map((group) => (
        <section
          key={group.label}
          className="prefs-keys"
          aria-label={group.label}
        >
          <h2 className="h-label">{group.label}</h2>
          <table className="table prefs-keys-table">
            <tbody>
              {group.rows.map((row) => (
                <tr key={`${row.keys}-${row.label}`}>
                  <td className="mono prefs-key">
                    {keyLabel(row.keys, IS_MAC)}
                  </td>
                  <td>{row.label}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      ))}
    </div>
  );
}

// ---- この作品（project.toml） ----

export type ProjectPageKind = "project" | "window" | "editor";

/** この作品の頁。`onEdit` は書いて、書けなかったわけ（書けたら null）を返す。 */
export function ProjectPage({
  kind,
  settings,
  error,
  scenes,
  onEdit,
}: {
  readonly kind: ProjectPageKind;
  readonly settings: ProjectSettings | null;
  readonly error: string | null;
  readonly scenes: readonly SceneSummary[];
  readonly onEdit: (edits: readonly SettingEdit[]) => Promise<string | null>;
}) {
  const [message, setMessage] = useState<string | null>(null);
  if (settings === null)
    return (
      <div className="prefs-form">
        {error !== null ? (
          <p className="notice notice-warn">{error}</p>
        ) : (
          <p className="hint">project.toml を読んでいます</p>
        )}
      </div>
    );
  const run = (plan: EditPlan) => {
    if (plan.kind === "none") return;
    if (plan.kind === "invalid") {
      setMessage(plan.message);
      return;
    }
    onEdit(plan.edits)
      .then((problem) => setMessage(problem ?? "project.toml に保存しました"))
      .catch((e: unknown) => setMessage(String(e)));
  };
  return (
    <div className="prefs-form">
      <p className="hint">
        作品フォルダの project.toml
        に書きます（チームで共有します）。欄を離れると保存します。コメントと並びは残します（消した項目の直上のコメントは一緒に消えます）。
      </p>
      {settings.problems.length > 0 && (
        <div className="notice notice-warn">
          {settings.problems.map((p) => (
            <p key={p} className="prefs-problem">
              {p}
            </p>
          ))}
        </div>
      )}
      {kind === "project" && (
        <ProjectInfo settings={settings} scenes={scenes} run={run} />
      )}
      {kind === "window" && <WindowSize settings={settings} run={run} />}
      {kind === "editor" && <EditorAssist settings={settings} run={run} />}
      {message !== null && (
        <p className="prefs-message" role="status">
          {message}
        </p>
      )}
    </div>
  );
}

function ProjectInfo({
  settings,
  scenes,
  run,
}: {
  readonly settings: ProjectSettings;
  readonly scenes: readonly SceneSummary[];
  readonly run: (plan: EditPlan) => void;
}) {
  const start = settings.start_scene ?? "";
  const known = start === "" || scenes.some((s) => s.id === start);
  const [pair, setPair] = useState("");
  const [style, setStyle] = useState("");
  const add = () => {
    const plan = bracketAddPlan(settings, pair, style);
    run(plan);
    if (plan.kind === "save") {
      setPair("");
      setStyle("");
    }
  };
  return (
    <>
      <Row label="作品名">
        <TextField
          label="作品名"
          value={settings.title ?? ""}
          placeholder="（フォルダ名で見せます）"
          width={320}
          onCommit={(text) => run(titlePlan(settings, text))}
        />
      </Row>
      <Row label="開始シーン">
        <select
          className="select"
          style={{ width: 320 }}
          aria-label="開始シーン"
          value={start}
          onChange={(e) => run(startScenePlan(settings, e.target.value))}
        >
          <option value="">（最初のシーン）</option>
          {!known && <option value={start}>{start}（無いシーン）</option>}
          {scenes.map((s) => (
            <option key={s.id} value={s.id}>
              {s.id} {s.title}
            </option>
          ))}
        </select>
        <span className="hint">
          プレビューと自動巡回を始めるシーン。書かなければ最初のシーン
        </span>
      </Row>
      <Row label="台詞の括弧">
        <div className="prefs-brackets">
          {settings.brackets.map((b) => (
            <div key={b.pair} className="prefs-bracket">
              <span className="prefs-bracket-pair">{b.pair}</span>
              <TextField
                label={`${b.pair} の表示スタイル`}
                value={b.style}
                width={160}
                onCommit={(text) =>
                  run(bracketStylePlan(settings, b.pair, text))
                }
              />
              <button
                type="button"
                className="btn btn-sm"
                disabled={settings.brackets.length <= 1}
                onClick={() => run(bracketRemovePlan(settings, b.pair))}
              >
                消す
              </button>
            </div>
          ))}
          <div className="prefs-bracket">
            <input
              className="input prefs-bracket-new"
              type="text"
              aria-label="足す括弧（2文字）"
              placeholder="『』"
              value={pair}
              spellCheck={false}
              onChange={(e) => setPair(e.target.value)}
              onKeyDown={onEnter(add)}
            />
            <input
              className="input"
              style={{ width: 160 }}
              type="text"
              aria-label="足す括弧の表示スタイル"
              placeholder="thought"
              value={style}
              spellCheck={false}
              onChange={(e) => setStyle(e.target.value)}
              onKeyDown={onEnter(add)}
            />
            <button type="button" className="btn btn-sm" onClick={add}>
              足す
            </button>
          </div>
          <span className="hint">
            開き括弧と閉じ括弧の2文字と、表示スタイル（英字。normal・thought
            など）。変えると、本文の読み方が変わります
          </span>
        </div>
      </Row>
    </>
  );
}

function WindowSize({
  settings,
  run,
}: {
  readonly settings: ProjectSettings;
  readonly run: (plan: EditPlan) => void;
}) {
  const screenOf = (s: ProjectSettings) => ({
    width: s.screen === null ? "" : String(s.screen.width),
    height: s.screen === null ? "" : String(s.screen.height),
  });
  const windowOf = (s: ProjectSettings) => ({
    lines: String(s.message_window?.lines ?? DEFAULT_WINDOW.lines),
    chars: String(
      s.message_window?.chars_per_line ?? DEFAULT_WINDOW.chars_per_line,
    ),
  });
  const [screen, setScreen] = useState(screenOf(settings));
  const [win, setWin] = useState(windowOf(settings));
  const screenKey = JSON.stringify(settings.screen);
  const windowKey = JSON.stringify(settings.message_window);
  // 保存した値（や外で直した値）が変わったら、欄も合わせる。
  // biome-ignore lint/correctness/useExhaustiveDependencies: 値の変わり目だけで合わせる
  useEffect(() => setScreen(screenOf(settings)), [screenKey]);
  // biome-ignore lint/correctness/useExhaustiveDependencies: 値の変わり目だけで合わせる
  useEffect(() => setWin(windowOf(settings)), [windowKey]);
  const checking = settings.message_window !== null;
  const commitScreen = (next = screen) =>
    run(screenPlan(settings, next.width, next.height));
  const commitWindow = (next = win) =>
    run(windowPlan(settings, next.lines, next.chars));
  const sizeInput = (
    label: string,
    value: string,
    onChange: (text: string) => void,
    commit: () => void,
    placeholder?: string,
    disabled?: boolean,
  ) => (
    <input
      className="input mono"
      style={{ width: 96 }}
      type="text"
      inputMode="numeric"
      aria-label={label}
      value={value}
      placeholder={placeholder}
      disabled={disabled}
      spellCheck={false}
      onChange={(e) => onChange(e.target.value)}
      onBlur={commit}
      onKeyDown={onEnter(commit)}
    />
  );
  return (
    <>
      <Row label="画面の大きさ">
        <div className="prefs-size">
          {sizeInput(
            "画面の幅",
            screen.width,
            (width) => setScreen((s) => ({ ...s, width })),
            () => commitScreen(),
            String(DEFAULT_SCREEN.width),
          )}
          <span>×</span>
          {sizeInput(
            "画面の高さ",
            screen.height,
            (height) => setScreen((s) => ({ ...s, height })),
            () => commitScreen(),
            String(DEFAULT_SCREEN.height),
          )}
          <span className="t-small t-2">ピクセル</span>
        </div>
        <span className="hint">
          プレビューの画面。両方を空にすると既定の 1280×720
        </span>
      </Row>
      <Row label="メッセージウィンドウ">
        <label className="check">
          <input
            type="checkbox"
            checked={checking}
            onChange={(e) => run(windowTogglePlan(settings, e.target.checked))}
          />
          はみ出し（W203）を調べる
        </label>
        <div className="prefs-size">
          {sizeInput(
            "行数",
            win.lines,
            (lines) => setWin((w) => ({ ...w, lines })),
            () => commitWindow(),
            undefined,
            !checking,
          )}
          <span className="t-small t-2">行 ×</span>
          {sizeInput(
            "1行の字数",
            win.chars,
            (chars) => setWin((w) => ({ ...w, chars })),
            () => commitWindow(),
            undefined,
            !checking,
          )}
          <span className="t-small t-2">字（全角で数える）</span>
        </div>
        <span className="hint">
          プレビューのメッセージウィンドウにも使います（調べないときは
          3行×24字で見せます）
        </span>
      </Row>
    </>
  );
}

function EditorAssist({
  settings,
  run,
}: {
  readonly settings: ProjectSettings;
  readonly run: (plan: EditPlan) => void;
}) {
  return (
    <>
      <Row label="三点リーダとダッシュ">
        <label className="check">
          <input
            type="checkbox"
            checked={settings.editor.normalize_ellipsis}
            onChange={(e) =>
              run(editorPlan(settings, "normalize_ellipsis", e.target.checked))
            }
          />
          「...」を「……」に、「--」を「――」にする
        </label>
      </Row>
      <Row label="段落頭の字下げ">
        <label className="check">
          <input
            type="checkbox"
            checked={settings.editor.indent_narration}
            onChange={(e) =>
              run(editorPlan(settings, "indent_narration", e.target.checked))
            }
          />
          地の文の段落頭に全角空白を入れる
        </label>
      </Row>
    </>
  );
}
