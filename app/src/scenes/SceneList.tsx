// 本文の画面の一覧：章（ファイル）ごとのシーン。章を足せる。保存していない章には印を付ける。

import { useState } from "react";
import type { SceneSummary } from "../core/types.ts";

interface Props {
  /** 本文のファイル（`story/` の下の `*.scn`。下のフォルダも）。シーンの無い章も出す。 */
  readonly files: readonly string[];
  readonly scenes: readonly SceneSummary[];
  readonly currentFile: string | null;
  /** カーソルのあるシーン（一覧で示す。T4-13、U8）。 */
  readonly currentScene: string | null;
  /** 保存していない変更のあるファイル。 */
  readonly dirty: ReadonlySet<string>;
  readonly onSelectScene: (scene: SceneSummary) => void;
  readonly onSelectFile: (file: string) => void;
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

export function SceneList({
  files,
  scenes,
  currentFile,
  currentScene,
  dirty,
  onSelectScene,
  onSelectFile,
  onAddChapter,
  onRenameChapter,
  onDeleteChapter,
}: Props) {
  const [adding, setAdding] = useState(false);
  const [name, setName] = useState("");
  const [problem, setProblem] = useState<string | null>(null);
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

  return (
    <nav className="scene-list" aria-label="シーン">
      <div className="panel-heading-row">
        <h2 className="panel-heading">ストーリー</h2>
        <button
          type="button"
          className="btn btn-sm"
          onClick={() => {
            setAdding(!adding);
            setProblem(null);
          }}
        >
          ＋ 章
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
      {files.map((file) => (
        <section
          key={file}
          className={file === currentFile ? "file current" : "file"}
        >
          <h3 className="file-name">
            <button
              type="button"
              className="file-button"
              onClick={() => onSelectFile(file)}
            >
              {file.replace(/^story\//, "")}
              {dirty.has(file) && (
                <span
                  className="dirty-mark"
                  title="保存していない変更があります"
                >
                  ●
                </span>
              )}
            </button>
          </h3>
          {file === currentFile && renaming?.file !== file && (
            <div className="file-actions">
              <button
                type="button"
                className="btn btn-link"
                onClick={() =>
                  setRenaming({
                    file,
                    // 名前を変えるのは同じフォルダの中だけなので、ファイル名だけを出す。
                    name: file.replace(/^.*\//, "").replace(/\.scn$/, ""),
                    problem: null,
                  })
                }
              >
                名前を変える
              </button>
              <button
                type="button"
                className="btn btn-link btn-danger"
                onClick={() => onDeleteChapter(file)}
              >
                削除
              </button>
            </div>
          )}
          {renaming?.file === file && (
            <div className="add-chapter">
              <input
                aria-label="章の新しいファイル名"
                value={renaming.name}
                // biome-ignore lint/a11y/noAutofocus: 名前を変えるを押した直後に打てるように
                autoFocus
                onChange={(event) =>
                  setRenaming({ file, name: event.target.value, problem: null })
                }
                onKeyDown={(event) => {
                  // 変換を確定する Enter では変えない。
                  if (event.nativeEvent.isComposing || event.keyCode === 229)
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
          <ul>
            {scenes.map((scene) =>
              scene.file === file ? (
                <li key={`${scene.id}-${scene.span[0]}`}>
                  <button
                    type="button"
                    className={[
                      "scene",
                      scene.is_stub ? "stub" : "",
                      file === currentFile && scene.id === currentScene
                        ? "current"
                        : "",
                    ]
                      .filter((c) => c !== "")
                      .join(" ")}
                    aria-current={
                      file === currentFile && scene.id === currentScene
                        ? "location"
                        : undefined
                    }
                    onClick={() => onSelectScene(scene)}
                  >
                    <span className="scene-title">{scene.title}</span>
                    <span className="scene-id">#{scene.id}</span>
                  </button>
                </li>
              ) : null,
            )}
          </ul>
        </section>
      ))}
    </nav>
  );
}
