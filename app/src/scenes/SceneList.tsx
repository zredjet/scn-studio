// 本文の画面の一覧：章（ファイル）ごとのシーン。章を足せる。保存していない章には印を付ける。

import { useState } from "react";
import type { SceneSummary } from "../core/types.ts";

interface Props {
  /** 本文のファイル（story/*.scn）。シーンの無い章も出す。 */
  readonly files: readonly string[];
  readonly scenes: readonly SceneSummary[];
  readonly currentFile: string | null;
  /** 保存していない変更のあるファイル。 */
  readonly dirty: ReadonlySet<string>;
  readonly onSelectScene: (scene: SceneSummary) => void;
  readonly onSelectFile: (file: string) => void;
  /** 章を足す。失敗したら、理由を返す。 */
  readonly onAddChapter: (name: string) => Promise<string | null>;
}

export function SceneList({
  files,
  scenes,
  currentFile,
  dirty,
  onSelectScene,
  onSelectFile,
  onAddChapter,
}: Props) {
  const [adding, setAdding] = useState(false);
  const [name, setName] = useState("");
  const [problem, setProblem] = useState<string | null>(null);

  return (
    <nav className="scene-list" aria-label="シーン">
      <div className="panel-heading-row">
        <h2 className="panel-heading">ストーリー</h2>
        <button
          type="button"
          className="small-button"
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
          <button type="submit" className="small-button">
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
          <ul>
            {scenes.map((scene) =>
              scene.file === file ? (
                <li key={`${scene.id}-${scene.span[0]}`}>
                  <button
                    type="button"
                    className={scene.is_stub ? "scene stub" : "scene"}
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
