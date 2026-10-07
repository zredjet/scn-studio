// 左の一覧：ファイルごとのシーン（IRから）。

import type { Ir } from "../core/types.ts";

interface Props {
  readonly ir: Ir;
  readonly currentFile: string | null;
  readonly onSelect: (sceneIndex: number) => void;
}

export function SceneList({ ir, currentFile, onSelect }: Props) {
  const files: string[] = [];
  for (const scene of ir.scenes) {
    if (!files.includes(scene.file)) files.push(scene.file);
  }
  return (
    <nav className="scene-list" aria-label="シーン">
      <h2 className="panel-heading">ストーリー</h2>
      {files.map((file) => (
        <section
          key={file}
          className={file === currentFile ? "file current" : "file"}
        >
          <h3 className="file-name">{file.replace(/^story\//, "")}</h3>
          <ul>
            {ir.scenes.map((scene, index) =>
              scene.file === file ? (
                <li key={`${scene.id}-${scene.span[0]}`}>
                  <button
                    type="button"
                    className={scene.is_stub ? "scene stub" : "scene"}
                    onClick={() => onSelect(index)}
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
