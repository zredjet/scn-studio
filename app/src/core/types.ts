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

/** 命令。フェーズ0の画面では種類と位置しか使わないので、細部は仕様書17節を見る。 */
export interface IrOp {
  readonly kind: string;
  readonly span: ByteSpan;
  readonly [field: string]: unknown;
}

export interface IrScene {
  readonly id: string;
  readonly title: string;
  readonly file: string;
  readonly span: ByteSpan;
  readonly meta: IrMeta;
  readonly is_stub: boolean;
  readonly ops: readonly IrOp[];
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

export interface Ir {
  readonly ir_version: number;
  readonly start_scene: string | null;
  readonly scenes: readonly IrScene[];
  readonly edges: readonly IrEdge[];
  readonly diagnostics: readonly IrDiagnostic[];
}

/** 台詞の括弧（project.toml の [dialogue.brackets]）。 */
export interface Bracket {
  readonly open: string;
  readonly close: string;
}

/** `open_project` の結果。 */
export interface OpenedProject {
  readonly root: string;
  readonly files: readonly { readonly path: string; readonly text: string }[];
  readonly brackets: readonly Bracket[];
  readonly ir: Ir;
}
