// コアから受け取る型（IRと診断）。形は仕様書17節（docs/spec/scn-format-v0.md）。
// フロントでは、コアの型をここ1か所で定義する（.claude/rules/frontend.md）。

/** UTF-8のバイト位置の範囲 `[開始, 終了]`。CodeMirrorの位置に使う前に position.ts で変換する。 */
export type ByteSpan = readonly [start: number, end: number];

export type Severity = "error" | "warning" | "info";

export interface IrDiagnostic {
  readonly code: string;
  readonly severity: Severity;
  readonly file: string;
  readonly span: ByteSpan;
  readonly message: string;
}

export interface IrMeta {
  readonly summary: string | null;
  readonly status: string | null;
  readonly route: string | null;
  readonly tags: readonly string[];
  readonly extra: Readonly<Record<string, string>>;
}

export interface IrInline {
  readonly kind: string;
  readonly text?: string;
}

/** シーンの一覧の1つ（IRのシーンから命令列を除いたもの。D042）。 */
export interface SceneSummary {
  readonly id: string;
  readonly title: string;
  readonly file: string;
  readonly span: ByteSpan;
  readonly meta: IrMeta;
  readonly is_stub: boolean;
}

export interface IrEdge {
  readonly from: string;
  readonly to: string;
  readonly kind: "jump" | "call";
  readonly via: "direct" | "choice" | "if" | "else";
  readonly label: string | null;
  readonly cond_text: string | null;
  readonly span: ByteSpan;
}

/** 表示に要る分（診断、シーンの一覧、辺）。入力が止まるたびに受け取る（D042）。 */
export interface Summary {
  readonly start_scene: string | null;
  readonly scenes: readonly SceneSummary[];
  readonly edges: readonly IrEdge[];
  /** 読み込み・構文解析・意味解析の診断（段階0）。 */
  readonly diagnostics: readonly IrDiagnostic[];
}

/** 台詞の括弧（project.toml の [dialogue.brackets]）。 */
export interface Bracket {
  readonly open: string;
  readonly close: string;
}

export interface SourceText {
  readonly path: string;
  readonly text: string;
}

/** `open_project`・`create_project` の結果。 */
export interface OpenedProject {
  readonly root: string;
  readonly title: string;
  readonly files: readonly SourceText[];
  readonly brackets: readonly Bracket[];
  readonly summary: Summary;
  /** 検証の段階1の診断（D043）。 */
  readonly checks: readonly IrDiagnostic[];
}

/**
 * `save_file` の結果。`text` は書いた本文（保存のときに付けた行IDを含む。D045）。
 * まだ取り込んでいない外部の変更があれば、書かずに `conflict`（`deleted` なら外で消された）。
 */
export type SavedFile =
  | {
      readonly kind: "saved";
      readonly text: string;
      readonly assigned: number;
      readonly summary: Summary;
      readonly checks: readonly IrDiagnostic[];
    }
  | { readonly kind: "conflict"; readonly deleted: boolean };

/** `reload_file` の結果。外で消されていれば `text` は null。 */
export interface ReloadedFile {
  readonly text: string | null;
  readonly summary: Summary;
  readonly checks: readonly IrDiagnostic[];
}

/** `keep_mine` の結果。`disk` はディスクの中身（外の中身。消されていれば null）。 */
export interface KeptFile {
  readonly disk: string | null;
  readonly summary: Summary;
  readonly checks: readonly IrDiagnostic[];
}

/** `add_chapter` の結果。 */
export interface AddedChapter {
  readonly file: string;
  readonly summary: Summary;
  readonly checks: readonly IrDiagnostic[];
}

/** 外部の変更を取り込んだ結果（コアの `workspace::Change`）。 */
export type Change =
  | { readonly kind: "reloaded"; readonly file: string; readonly text: string }
  | { readonly kind: "added"; readonly file: string; readonly text: string }
  | { readonly kind: "removed"; readonly file: string }
  | {
      readonly kind: "conflict";
      readonly file: string;
      readonly deleted: boolean;
    }
  | { readonly kind: "unreadable"; readonly file: string }
  | { readonly kind: "settings" };

/** `workspace-changed` の知らせ。 */
export interface WorkspaceChanged {
  readonly changes: readonly Change[];
  readonly brackets: readonly Bracket[];
  readonly title: string;
  readonly summary: Summary;
  readonly checks: readonly IrDiagnostic[];
}

export interface RecentProject {
  readonly path: string;
  readonly title: string;
}

/** 開発用の設定（リリースビルドでは project は null、measure_ipc は false）。 */
export interface DevSettings {
  readonly project: string | null;
  readonly measure_ipc: boolean;
}
