// 本文の画面の一覧（docs/ui/decisions-v1.md の U14、見本 02-editor.html。T4-18）：章（ファイル）ごとのシーン。
// 章は折りたためる（章の行に見出し・シーンの数、下にファイル名と字数）。シーンの行は「状態の印・名前・ルート名・ID」。
// カーソルのあるシーンの下に、アウトライン（演出・選択・分岐・出口）を行番号付きで出し、押すとその行へ移る。
// 章を足す・名前を変える・消す。保存していない章には印を付ける。

import { type RefObject, useRef, useState } from "react";
import type { ChapterTitle, SceneSummary } from "../core/types.ts";
import { Icon } from "../shell/icons.tsx";
import { fileChars, formatChars } from "../shell/scene.ts";
import { useOutsideClose, useOverlay } from "../shell/useOverlay.ts";
import {
  chapterName,
  filterScenes,
  MARK_LABEL,
  type OutlineRow,
  type SceneMark,
  sceneMark,
} from "./list.ts";
import { SceneMarkView } from "./SceneMark.tsx";

interface Props {
  /** 本文のファイル（`story/` の下の `*.scn`。下のフォルダも）。シーンの無い章も出す。 */
  readonly files: readonly string[];
  readonly scenes: readonly SceneSummary[];
  /** 章の見出し。 */
  readonly chapters: readonly ChapterTitle[];
  readonly currentFile: string | null;
  /** カーソルのあるシーン（一覧で示す。T4-13、U8）。 */
  readonly currentScene: string | null;
  /** カーソルのあるシーンのアウトライン（行番号付き）。 */
  readonly outline: readonly OutlineRow[];
  /** 保存していない変更のあるファイル。 */
  readonly dirty: ReadonlySet<string>;
  readonly onSelectScene: (scene: SceneSummary) => void;
  readonly onSelectFile: (file: string) => void;
  /** アウトラインの行へ移る（バイト位置）。 */
  readonly onGoTo: (file: string, offset: number) => void;
  /** 章を足す。失敗したら、理由を返す。 */
  readonly onAddChapter: (name: string) => Promise<string | null>;
  /** 章の名前を変える（T2-8）。失敗したら、理由を返す。 */
  readonly onRenameChapter: (
    file: string,
    name: string,
  ) => Promise<string | null>;
  /** 章を消す（確かめてから。T2-8）。 */
  readonly onDeleteChapter: (file: string) => void;
}

const MARKS: readonly SceneMark[] = ["writing", "draft", "done", "ai", "stub"];

export function SceneList({
  files,
  scenes,
  chapters,
  currentFile,
  currentScene,
  outline,
  dirty,
  onSelectScene,
  onSelectFile,
  onGoTo,
  onAddChapter,
  onRenameChapter,
  onDeleteChapter,
}: Props) {
  const [adding, setAdding] = useState(false);
  const [name, setName] = useState("");
  const [problem, setProblem] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  /** たたんでいる章。 */
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  /** 操作のメニューを開いている章。 */
  const [menu, setMenu] = useState<string | null>(null);
  /** 名前を変えている章と、新しい名前。 */
  const [renaming, setRenaming] = useState<{
    readonly file: string;
    readonly name: string;
    readonly problem: string | null;
  } | null>(null);
  const commitRename = () => {
    if (renaming === null) return;
    const { file, name: next } = renaming;
    void onRenameChapter(file, next).then((error) => {
      if (error === null) setRenaming(null);
      else setRenaming((r) => (r === null ? r : { ...r, problem: error }));
    });
  };
  const shown = filterScenes(scenes, query);
  const filtering = query.trim() !== "";
  const toggle = (file: string) =>
    setCollapsed((set) => {
      const next = new Set(set);
      if (next.has(file)) next.delete(file);
      else next.add(file);
      return next;
    });

  return (
    <nav className="scene-list list-column" aria-label="シーン">
      <div className="panel-head">
        <h2 className="h-label">ストーリー</h2>
        <button
          type="button"
          className="btn btn-icon"
          aria-label="章を足す"
          aria-expanded={adding}
          title="章を足す"
          onClick={() => {
            setAdding(!adding);
            setProblem(null);
          }}
        >
          ＋
        </button>
      </div>
      {adding && (
        <form
          className="add-chapter"
          onSubmit={(event) => {
            event.preventDefault();
            void onAddChapter(name).then((error) => {
              setProblem(error);
              if (error === null) {
                setAdding(false);
                setName("");
              }
            });
          }}
        >
          <input
            className="input"
            aria-label="章のファイル名"
            placeholder="chapter04"
            value={name}
            onChange={(event) => setName(event.target.value)}
          />
          <button type="submit" className="btn btn-sm">
            足す
          </button>
          {problem !== null && <p className="form-problem">{problem}</p>}
        </form>
      )}
      <label className="search scene-search">
        <Icon name="search" size={14} />
        <input
          type="search"
          aria-label="シーンを絞り込む"
          placeholder="シーンを絞り込む"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
      </label>
      <div className="scene-chapters">
        {files.map((file) => {
          const inFile = shown.filter((s) => s.file === file);
          if (filtering && inFile.length === 0) return null;
          const open = filtering || !collapsed.has(file);
          const all = scenes.filter((s) => s.file === file).length;
          return (
            <section
              key={file}
              className={file === currentFile ? "file current" : "file"}
            >
              <div className="chapter-row">
                <button
                  type="button"
                  className="chapter-toggle"
                  aria-expanded={open}
                  aria-label={open ? "章をたたむ" : "章を開く"}
                  onClick={() => toggle(file)}
                >
                  <span className={open ? "chev open" : "chev"}>
                    <Icon name="chevron" size={12} stroke={2.2} />
                  </span>
                </button>
                <button
                  type="button"
                  className="file-button chapter-name"
                  title={file}
                  onClick={() => onSelectFile(file)}
                >
                  {chapterName(file, chapters)}
                </button>
                <span className="chapter-count">{all}</span>
                <ChapterMenu
                  open={menu === file}
                  onToggle={() => setMenu((m) => (m === file ? null : file))}
                  onClose={() => setMenu(null)}
                  onRename={() =>
                    setRenaming({
                      file,
                      // 名前を変えるのは同じフォルダの中だけなので、ファイル名だけを出す。
                      name: file.replace(/^.*\//, "").replace(/\.scn$/, ""),
                      problem: null,
                    })
                  }
                  onDelete={() => onDeleteChapter(file)}
                />
              </div>
              <div className="chapter-sub">
                {file.replace(/^.*\//, "")} ·{" "}
                {formatChars(fileChars(scenes, file))}
                {dirty.has(file) && (
                  <span
                    className="dirty-mark"
                    title="保存していない変更があります"
                  >
                    ●
                  </span>
                )}
              </div>
              {renaming?.file === file && (
                <div className="add-chapter">
                  <input
                    className="input"
                    aria-label="章の新しいファイル名"
                    value={renaming.name}
                    // biome-ignore lint/a11y/noAutofocus: 名前を変えるを押した直後に打てるように
                    autoFocus
                    onChange={(event) =>
                      setRenaming({
                        file,
                        name: event.target.value,
                        problem: null,
                      })
                    }
                    onKeyDown={(event) => {
                      // 変換を確定する Enter では変えない。
                      if (
                        event.nativeEvent.isComposing ||
                        event.keyCode === 229
                      )
                        return;
                      if (event.key === "Enter") {
                        event.preventDefault();
                        commitRename();
                      } else if (event.key === "Escape") setRenaming(null);
                    }}
                  />
                  <button
                    type="button"
                    className="btn btn-sm"
                    onClick={commitRename}
                  >
                    変える
                  </button>
                  <button
                    type="button"
                    className="btn btn-sm"
                    onClick={() => setRenaming(null)}
                  >
                    やめる
                  </button>
                  {renaming.problem !== null && (
                    <p className="form-problem">{renaming.problem}</p>
                  )}
                </div>
              )}
              {open && (
                <ul>
                  {inFile.map((scene) => {
                    const current =
                      file === currentFile && scene.id === currentScene;
                    const mark = sceneMark(scene);
                    return (
                      <li key={`${scene.id}-${scene.span[0]}`}>
                        <button
                          type="button"
                          className={[
                            "scene",
                            mark === "stub" ? "stub" : "",
                            current ? "current" : "",
                          ]
                            .filter((c) => c !== "")
                            .join(" ")}
                          aria-current={current ? "location" : undefined}
                          onClick={() => onSelectScene(scene)}
                        >
                          <SceneMarkView mark={mark} />
                          <span className="scene-title">{scene.title}</span>
                          {scene.meta.route !== null && (
                            <span className="scene-route hide-narrow">
                              {scene.meta.route}
                            </span>
                          )}
                          <span className="scene-id">{scene.id}</span>
                        </button>
                        {current && outline.length > 0 && (
                          <ul className="scene-outline">
                            {outline.map((row) => (
                              <li key={`${row.kind}-${row.start}`}>
                                <button
                                  type="button"
                                  className={`outline-row ${row.kind}`}
                                  title={`${row.line}行へ`}
                                  onClick={() => onGoTo(file, row.start)}
                                >
                                  <span className="outline-kind">
                                    {row.label}
                                  </span>
                                  <span className="outline-text">
                                    {row.text}
                                  </span>
                                  <span className="outline-line">
                                    {row.line}
                                  </span>
                                </button>
                              </li>
                            ))}
                          </ul>
                        )}
                      </li>
                    );
                  })}
                </ul>
              )}
            </section>
          );
        })}
        {filtering && shown.length === 0 && (
          <p className="empty scene-empty">合うシーンがありません。</p>
        )}
      </div>
      <div className="scene-legend">
        {MARKS.map((m) => (
          <span key={m} className="legend-mark">
            <SceneMarkView mark={m} />
            {MARK_LABEL[m]}
          </span>
        ))}
      </div>
    </nav>
  );
}

function ChapterMenu({
  open,
  onToggle,
  onClose,
  onRename,
  onDelete,
}: {
  readonly open: boolean;
  readonly onToggle: () => void;
  readonly onClose: () => void;
  readonly onRename: () => void;
  readonly onDelete: () => void;
}) {
  const button = useRef<HTMLButtonElement>(null);
  return (
    <span className="chapter-menu-anchor">
      <button
        ref={button}
        type="button"
        className="btn btn-icon chapter-more"
        aria-label="章の操作"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={onToggle}
      >
        …
      </button>
      {open && (
        <ChapterMenuPopover
          anchor={button}
          onClose={onClose}
          onRename={() => {
            onClose();
            onRename();
          }}
          onDelete={() => {
            onClose();
            onDelete();
          }}
        />
      )}
    </span>
  );
}

function ChapterMenuPopover({
  anchor,
  onClose,
  onRename,
  onDelete,
}: {
  readonly anchor: RefObject<HTMLButtonElement | null>;
  readonly onClose: () => void;
  readonly onRename: () => void;
  readonly onDelete: () => void;
}) {
  const { ref, onKeyDown } = useOverlay<HTMLDivElement>(onClose);
  useOutsideClose(ref, anchor, onClose);
  return (
    <div
      ref={ref}
      className="popover chapter-menu"
      role="menu"
      aria-label="章の操作"
      tabIndex={-1}
      onKeyDown={onKeyDown}
    >
      <div className="menu">
        <button
          type="button"
          role="menuitem"
          className="menu-item"
          onClick={onRename}
        >
          ファイルの名前を変える
        </button>
        <button
          type="button"
          role="menuitem"
          className="menu-item menu-danger"
          onClick={onDelete}
        >
          章を消す…
        </button>
      </div>
    </div>
  );
}
