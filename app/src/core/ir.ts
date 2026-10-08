// IRの型（コアの `ir::Ir`。仕様書17節、D036）。プレビュー（T1-11）が受け取る。入力のたびの解析では
// 命令列を渡さない（D042）ので、プレビューの画面を開いているときだけ取る。

import type { IrMeta } from "./types.ts";

/** バイト位置の範囲（そのシーンのファイルの先頭から）。 */
export type IrSpan = readonly [number, number];

export type IrExpr =
  | { readonly kind: "int"; readonly value: number }
  | { readonly kind: "string"; readonly value: string }
  | { readonly kind: "bool"; readonly value: boolean }
  | { readonly kind: "var"; readonly name: string }
  | {
      readonly kind: "unary";
      readonly op: "-" | "not";
      readonly operand: IrExpr;
    }
  | {
      readonly kind: "binary";
      readonly op: string;
      readonly lhs: IrExpr;
      readonly rhs: IrExpr;
    };

export type IrInline =
  | { readonly kind: "text"; readonly text: string }
  | { readonly kind: "ruby"; readonly base: string; readonly reading: string }
  | { readonly kind: "emphasis"; readonly text: string }
  | { readonly kind: "wait"; readonly seconds: number }
  | { readonly kind: "page_break" }
  | { readonly kind: "se"; readonly name: string }
  | { readonly kind: "speed"; readonly speed: "slow" | "normal" | "fast" }
  | {
      readonly kind: "interpolation";
      readonly expr_text: string;
      readonly expr: IrExpr | null;
    }
  | { readonly kind: "line_break" };

export interface IrOption {
  readonly span: IrSpan;
  readonly line_id: string | null;
  readonly text: readonly IrInline[];
  readonly cond_text: string | null;
  readonly cond: IrExpr | null;
  readonly jump: string | null;
  readonly body: readonly Op[];
}

export interface IrBranch {
  readonly span: IrSpan;
  readonly cond_text: string;
  readonly cond: IrExpr | null;
  readonly body: readonly Op[];
}

export type Op =
  | {
      readonly kind: "say";
      readonly span: IrSpan;
      readonly line_id: string | null;
      readonly speaker: string;
      readonly display_name: string | null;
      readonly style: string;
      readonly text: readonly IrInline[];
    }
  | {
      readonly kind: "narrate";
      readonly span: IrSpan;
      readonly line_id: string | null;
      readonly text: readonly IrInline[];
    }
  | { readonly kind: "break"; readonly span: IrSpan }
  | {
      readonly kind: "bg";
      readonly span: IrSpan;
      readonly name: string;
      readonly transition: string | null;
    }
  | {
      readonly kind: "show";
      readonly span: IrSpan;
      readonly character: string;
      readonly expression: string;
      readonly position: "left" | "center" | "right" | null;
      readonly transition: string | null;
    }
  | {
      readonly kind: "hide";
      readonly span: IrSpan;
      readonly character: string;
      readonly transition: string | null;
    }
  | { readonly kind: "bgm"; readonly span: IrSpan; readonly name: string }
  | { readonly kind: "bgm_stop"; readonly span: IrSpan }
  | { readonly kind: "se"; readonly span: IrSpan; readonly name: string }
  | {
      readonly kind: "fx";
      readonly span: IrSpan;
      readonly name: string;
      readonly seconds: number | null;
    }
  | { readonly kind: "wait"; readonly span: IrSpan; readonly seconds: number }
  | {
      readonly kind: "set";
      readonly span: IrSpan;
      readonly var: string;
      readonly op: "=" | "+=" | "-=";
      readonly value: IrExpr;
    }
  | {
      readonly kind: "choice";
      readonly span: IrSpan;
      readonly line_id: string | null;
      readonly prompt: readonly IrInline[] | null;
      readonly options: readonly IrOption[];
    }
  | {
      readonly kind: "if";
      readonly span: IrSpan;
      readonly branches: readonly IrBranch[];
      readonly else: readonly Op[] | null;
    }
  | { readonly kind: "jump"; readonly span: IrSpan; readonly target: string }
  | { readonly kind: "call"; readonly span: IrSpan; readonly target: string }
  | { readonly kind: "return"; readonly span: IrSpan }
  | {
      readonly kind: "ending";
      readonly span: IrSpan;
      readonly id: string | null;
    }
  | {
      readonly kind: "raw";
      readonly span: IrSpan;
      readonly engine: string;
      readonly lines: readonly string[];
    }
  | {
      readonly kind: "unknown_directive";
      readonly span: IrSpan;
      readonly name: string;
      readonly args: readonly string[];
    };

export interface IrScene {
  readonly id: string;
  readonly title: string;
  readonly file: string;
  readonly span: IrSpan;
  readonly meta: IrMeta;
  readonly is_stub: boolean;
  readonly ops: readonly Op[];
}

export interface Ir {
  readonly ir_version: number;
  readonly start_scene: string | null;
  readonly scenes: readonly IrScene[];
}

/** プレビューのデータ（コアの `preview::PreviewData`。T1-11、D050）。 */
export interface PreviewData {
  readonly ir: Ir;
  readonly screen: { readonly width: number; readonly height: number };
  readonly message_window: {
    readonly lines: number;
    readonly chars_per_line: number;
  };
  readonly brackets: readonly {
    readonly open: string;
    readonly close: string;
    readonly style: string;
  }[];
  readonly characters: readonly {
    readonly id: string;
    readonly name: string;
  }[];
  readonly variables: readonly {
    readonly key: string;
    readonly type: string;
    readonly values: readonly string[];
    readonly initial: boolean | number | string;
    readonly bad_default: boolean;
    readonly scope: string | null;
    readonly range: readonly [number, number] | null;
  }[];
  readonly assets: {
    /** 意味名ごとのファイル（絶対パス）。登録されているがファイルが無ければ null。 */
    readonly backgrounds: Readonly<Record<string, string | null>>;
    readonly expressions: Readonly<
      Record<string, Readonly<Record<string, string | null>>>
    >;
    readonly bgm: Readonly<
      Record<string, { readonly file: string | null; readonly loop: boolean }>
    >;
    readonly se: Readonly<
      Record<string, { readonly file: string | null; readonly loop: boolean }>
    >;
  };
}
