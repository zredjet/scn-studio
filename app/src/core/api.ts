// コアの呼び出し（Tauriの invoke）。コンポーネントから直接 invoke を呼ばない（.claude/rules/frontend.md）。

import { invoke } from "@tauri-apps/api/core";
import type { Ir, OpenedProject } from "./types.ts";

/** プロジェクトを開き、本文とIRを受け取る。 */
export function openProject(path: string): Promise<OpenedProject> {
  return invoke<OpenedProject>("open_project", { path });
}

/** 保存していない編集中の本文で、プロジェクト全体を解析し直す。 */
export function parseBuffer(file: string, text: string): Promise<Ir> {
  return invoke<Ir>("parse_buffer", { file, text });
}

/** 開発用：起動時に開くサンプル作品のパス。リリースビルドでは null。 */
export function devProjectPath(): Promise<string | null> {
  return invoke<string | null>("dev_project_path");
}
