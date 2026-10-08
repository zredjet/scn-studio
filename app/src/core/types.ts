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
  /** 表示する文字の数（D047）。 */
  readonly chars: number;
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

/** 構造グラフの粒度（D048）。 */
export type GraphLevel = "scene" | "chapter" | "route";

/** 構造グラフの辺（コアの `graph::GraphEdge`）。 */
export interface GraphEdge {
  readonly from: string;
  readonly to: string;
  readonly kind: "jump" | "call";
  readonly via: "direct" | "choice" | "if" | "else";
  readonly label: string | null;
  readonly cond_text: string | null;
  /** 行き先のシーンがあるか（無ければ E501）。 */
  readonly to_exists: boolean;
  /** 行き先のまとまり（ルートの無いシーンや、無いシーンなら null）。 */
  readonly to_group: string | null;
}

/** 構造グラフのまとまり（シーン、章、ルート。コアの `graph::Group`）。 */
export interface GraphGroup {
  /** シーンID、章のファイル、ルートの名前。ルートの無いシーンのまとまりは null。 */
  readonly id: string | null;
  readonly title: string | null;
  readonly file: string | null;
  readonly route: string | null;
  readonly status: string | null;
  readonly is_stub: boolean | null;
  readonly scenes: readonly string[];
  readonly exits: readonly GraphEdge[];
  readonly endings: readonly string[];
  readonly reads: readonly string[];
  readonly writes: readonly string[];
}

/** 構造グラフ（コアの `graph::Graph`）。 */
export interface SceneGraphData {
  readonly level: GraphLevel;
  readonly groups: readonly GraphGroup[];
}

export interface LayoutPoint {
  readonly x: number;
  readonly y: number;
}

/** フロー図の配置（`layout.json`。D048）。 */
export type Layout = Readonly<
  Record<GraphLevel, Readonly<Record<string, LayoutPoint>>>
>;

/** `read_layout` の結果。`problem` は読み飛ばしたもの。 */
export interface LoadedLayout {
  readonly layout: Layout;
  readonly problem: string | null;
  /**
   * `invalid`：壊れている（ノードを動かしても上書きしない。自動整列のときだけ書き直す）。
   * `unreadable`：読めない（書かない）。
   */
  readonly state: "ok" | "invalid" | "unreadable";
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
  readonly editor: EditorSettings;
  readonly assist: AssistData;
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
  | { readonly kind: "layout" }
  | { readonly kind: "settings" }
  /** アイデアメモ（`ideas/*.md`）が外で変わった（T2-6）。 */
  | { readonly kind: "ideas" };

/** `workspace-changed` の知らせ。 */
export interface WorkspaceChanged {
  readonly changes: readonly Change[];
  readonly brackets: readonly Bracket[];
  readonly title: string;
  readonly editor: EditorSettings;
  readonly assist: AssistData;
  readonly summary: Summary;
  readonly checks: readonly IrDiagnostic[];
}

/** 補完の材料（コアの `assist::completion_data`。D047）。 */
export interface AssistData {
  readonly characters: readonly {
    readonly id: string;
    readonly name: string;
    readonly aliases: readonly string[];
  }[];
  readonly backgrounds: readonly string[];
  /** キャラクターIDごとの立ち絵の差分。 */
  readonly expressions: Readonly<Record<string, readonly string[]>>;
  readonly bgm: readonly string[];
  readonly se: readonly string[];
  readonly variables: readonly {
    readonly key: string;
    readonly type: string;
    readonly values: readonly string[];
  }[];
  readonly directives: readonly {
    readonly ja: string;
    readonly en: string;
    readonly args: string;
  }[];
  readonly positions: readonly string[];
  readonly bgm_stop: string;
}

/** エディタの入力補助の設定（`project.toml` の `[editor]`。D047）。 */
export interface EditorSettings {
  readonly normalize_ellipsis: boolean;
  readonly indent_narration: boolean;
}

/** 今のシーンの情報（文脈パネル。コアの `assist::SceneInfo`）。 */
export interface SceneInfo {
  readonly scene: string;
  readonly characters: readonly {
    readonly id: string;
    readonly name: string;
    readonly first_person: string | null;
    readonly speech: string | null;
    readonly lines: number;
  }[];
  readonly variables: readonly {
    readonly key: string;
    readonly type: string | null;
    readonly reads: number;
    readonly writes: number;
  }[];
  readonly assets: readonly {
    readonly kind: "background" | "expression" | "bgm" | "se";
    readonly character: string | null;
    readonly name: string;
    readonly registered: boolean;
  }[];
}

// ---- 設定DBと変数（T1-10、D049） ----

/** JSONの値（設定DBのフロントマターの値）。 */
export type JsonValue =
  | null
  | boolean
  | number
  | string
  | readonly JsonValue[]
  | { readonly [key: string]: JsonValue };

/** 項目の型（コアの `bible::FieldType`）。 */
export type FieldKind =
  | {
      readonly kind:
        | "text"
        | "longtext"
        | "number"
        | "bool"
        | "date"
        | "select"
        | "multiselect"
        | "asset";
    }
  | { readonly kind: "ref"; readonly target: string }
  | { readonly kind: "list"; readonly item: FieldKind };

/** 型の項目の定義。 */
export interface FieldDef {
  readonly key: string;
  readonly label: string;
  /** 書かれたとおりの型。 */
  readonly type: string;
  readonly kind: FieldKind;
  readonly options: readonly string[];
}

/** 設定DBの型（`bible/types/<型>.yaml`）。 */
export interface EntryType {
  readonly id: string;
  readonly name: string;
  readonly builtin: string | null;
  readonly fields: readonly FieldDef[];
}

/** 設定DBのエントリ（読めたもの）。 */
export interface Entry {
  readonly type_id: string;
  readonly id: string;
  readonly path: string;
  readonly fields: Readonly<Record<string, JsonValue>>;
  /** フロントマターのキーの位置（診断の位置と同じ）。 */
  readonly key_spans: Readonly<Record<string, readonly [number, number]>>;
  readonly body: string;
}

/** エントリのファイル（読めなかったものを含む）。 */
export interface EntryFile {
  readonly path: string;
  /** ファイルの中身。UTF-8で読めなければ null。 */
  readonly text: string | null;
  /** 読めたエントリ。読めなければ（E801・E802）null。 */
  readonly entry: Entry | null;
}

/** 設定DBの中身（`read_bible`）。 */
export interface BibleData {
  readonly character_type: string;
  readonly types: readonly EntryType[];
  readonly files: readonly EntryFile[];
  /** キャラクターIDごとの立ち絵の差分（素材カタログ）。 */
  readonly expressions: Readonly<Record<string, readonly string[]>>;
  /** キャラクターIDごとの、ファイルの無い差分（仮素材。T2-3）。 */
  readonly placeholders: Readonly<Record<string, readonly string[]>>;
}

/** 設定DBと変数を書いた結果（コアの `workspace::EntryEdit` と、エントリを作ったとき）。 */
export type EntryEdit =
  | { readonly kind: "saved"; readonly text: string }
  | { readonly kind: "conflict"; readonly text: string }
  | { readonly kind: "unsupported"; readonly message: string }
  | { readonly kind: "created"; readonly path: string };

/** 設定DBを書いた後に受け取るもの。 */
export interface BibleWritten {
  readonly edit: EntryEdit;
  readonly bible: BibleData;
  readonly assist: AssistData;
  readonly summary: Summary;
  readonly checks: readonly IrDiagnostic[];
}

/** 本文の中の位置（コアの `index::Location`）。 */
export interface Location {
  readonly scene: string;
  readonly file: string;
  readonly span: readonly [number, number];
}

/** 変数の一覧の1行（コアの `bible::VariableRow`）。 */
export interface VariableRow {
  readonly key: string;
  readonly declared: boolean;
  readonly type: string | null;
  readonly values: readonly string[];
  readonly default: string | null;
  readonly range: readonly [number, number] | null;
  readonly scope: string | null;
  readonly desc: string | null;
  /** `variables.yaml` の中の宣言の位置。 */
  readonly span: readonly [number, number] | null;
  readonly reads: readonly Location[];
  readonly writes: readonly Location[];
}

/** 項目の参照（コアの `bible::EntryRef`）。 */
export interface EntryRef {
  readonly field: string;
  readonly label: string;
  readonly kind: "entry" | "scene";
  readonly type_id: string | null;
  readonly id: string;
  readonly name: string | null;
  readonly path: string | null;
  readonly exists: boolean;
}

/** エントリの逆引き（コアの `bible::EntryUsage`）。 */
export interface EntryUsage {
  readonly character: boolean;
  readonly scenes: readonly {
    readonly scene: string;
    readonly title: string;
    readonly file: string;
    readonly span: readonly [number, number];
    readonly count: number;
  }[];
  readonly lines: number;
  readonly first: string | null;
  readonly refs_out: readonly EntryRef[];
  readonly refs_in: readonly EntryRef[];
  readonly variables: readonly VariableRow[];
}

/** 変数の画面の中身（`read_variables`）。 */
export interface VariablesData {
  /** `variables.yaml` の中身（無ければ null）。 */
  readonly text: string | null;
  readonly rows: readonly VariableRow[];
}

/** `variables.yaml` を書いた後に受け取るもの。 */
export interface VariablesWritten {
  readonly edit: EntryEdit;
  readonly variables: VariablesData;
  readonly assist: AssistData;
  readonly summary: Summary;
  readonly checks: readonly IrDiagnostic[];
}

/** エディタの表示：執筆表示（記号を畳む）か、ソース表示（生のテキスト）か（D046）。 */
export type ViewMode = "writing" | "source";

export interface RecentProject {
  readonly path: string;
  readonly title: string;
}

/** 開発用の設定（リリースビルドでは project は null、measure_ipc は false）。 */
export interface DevSettings {
  readonly project: string | null;
  readonly measure_ipc: boolean;
}

// ---- 台本（T1-12、D051） ----

/** 数（全体のうち、着いた・選べた数。コアの `playthrough::Coverage`）。 */
export interface Coverage {
  readonly reached: number;
  readonly total: number;
}

/** エンディングの到達条件（コアの `playthrough::EndingReach`）。 */
export interface EndingReach {
  readonly id: string;
  readonly scene: string;
  readonly reached: boolean;
  readonly states: number;
  /** 選択肢のいちばん少ない道で選ぶ選択肢の表示文。 */
  readonly example: readonly string[];
  /** 着いたときの、流れに関わる変数の値（多ければ先頭だけ）。 */
  readonly values: Readonly<
    Record<string, readonly (boolean | number | string)[]>
  >;
  /** 値が多くて、`values` に書ききれなかった変数。 */
  readonly more: readonly string[];
}

/** 自動巡回の結果（検証の段階2。コアの `playthrough::Playthrough`、T3-2）。 */
export interface Playthrough {
  readonly complete: boolean;
  readonly limit: string | null;
  readonly states: number;
  readonly starts: number;
  readonly scenes: Coverage;
  readonly options: Coverage;
  readonly endings: readonly EndingReach[];
  readonly diagnostics: readonly IrDiagnostic[];
}

/** 台本の1行（コアの `script::ScriptRow`）。 */
export interface ScriptRow {
  readonly line_id: string;
  readonly scene: string;
  readonly character: string;
  readonly text: string;
  readonly narration: string;
  readonly voice: string;
  readonly note: string;
}

/** 台本（コアの `script::ScriptSheet`）。 */
export interface ScriptSheet {
  readonly rows: readonly ScriptRow[];
  /** 行IDの無い台詞の数。 */
  readonly missing_line_ids: number;
  /** 収録の基準と比べた数（基準が無ければ null。T2-10）。 */
  readonly retakes: {
    readonly changed: number;
    readonly added: number;
    readonly removed: number;
    /** 基準にあって、今の台本に無い行ID。 */
    readonly removed_ids: readonly string[];
  } | null;
  /** 収録の基準を読めなかったわけ。 */
  readonly baseline_problem: string | null;
}

/** 台本の形式（コアの `script::ScriptFormat`）。 */
export type ScriptFormat = "csv" | "xlsx";

/** 素材の種類（コアの索引と同じ）。 */
export type AssetKind = "background" | "expression" | "bgm" | "se";

/** 素材の状態（コアの `assets::AssetState`）。 */
export type AssetState = "file" | "placeholder" | "unregistered";

/** 素材の一覧の1行（コアの `assets::AssetRow`）。 */
export interface AssetRow {
  readonly kind: AssetKind;
  /** 立ち絵のキャラクターID（解決できなければ書かれた名前）。 */
  readonly character: string | null;
  readonly character_name: string | null;
  readonly name: string;
  readonly state: AssetState;
  /** カタログに書かれたファイル（`assets/` から）。 */
  readonly file: string | null;
  /** 実在するファイルの絶対パス。 */
  readonly path: string | null;
  readonly uses: readonly Location[];
  /** 使用シーン（初めて使われた順）。最初が初出。 */
  readonly scenes: readonly string[];
  /** カタログに足せるか。 */
  readonly addable: boolean;
}

export interface AssetCharacter {
  readonly id: string;
  readonly name: string;
  readonly registered: boolean;
}

/** 素材の一覧（コアの `assets::AssetList`）。 */
export interface AssetList {
  readonly rows: readonly AssetRow[];
  readonly characters: readonly AssetCharacter[];
}

/** 素材を指すもの（コアの `workspace::AssetKey`）。 */
export interface AssetKey {
  readonly kind: AssetKind;
  readonly character: string | null;
  readonly name: string;
}

/** カタログを書いた結果（コアの `workspace::CatalogEdit`）。 */
export type CatalogEdit =
  | { readonly kind: "saved"; readonly text: string }
  | { readonly kind: "unchanged" }
  | { readonly kind: "unsupported"; readonly message: string };

/** カタログを書いた後に受け取るもの。 */
export interface CatalogWritten {
  readonly edit: CatalogEdit;
  readonly assets: AssetList;
  readonly assist: AssistData;
  readonly summary: Summary;
  readonly checks: readonly IrDiagnostic[];
}

/** 最短経路で選ぶ選択肢（コアの `route::RouteStep`）。 */
export interface RouteStep {
  readonly scene: string;
  readonly choice: readonly [number, number];
  readonly option: number;
}

/** 最短経路（コアの `route::Route`。T2-4、D055）。 */
export interface Route {
  readonly target: {
    readonly scene: string;
    readonly file: string;
    readonly span: readonly [number, number];
    readonly line_id: string | null;
  };
  readonly reachable: boolean;
  readonly steps: readonly RouteStep[];
  readonly variables: Readonly<Record<string, boolean | number | string>>;
  readonly calls: readonly string[];
  readonly message: string | null;
}

/** プレビューへの再生の頼み（メインウィンドウの画面にも、別ウィンドウにも渡す）。`seq` で見分ける。 */
export type PreviewRequest =
  /** 最初から再生する。 */
  | { readonly seq: number; readonly kind: "start" }
  /** ファイルのある位置（バイト）から、最短経路で再生する。 */
  | {
      readonly seq: number;
      readonly kind: "at";
      readonly file: string;
      readonly offset: number;
    }
  /** シーンの頭から、最短経路で再生する。 */
  | { readonly seq: number; readonly kind: "scene"; readonly scene: string };

/** 通読の文中の記法（IR と同じ形）。 */
type ReadingInline = import("./ir.ts").IrInline;

/** 通読する道（コアの `route::ReadingTarget`。T2-5、D056）。 */
export type ReadingTarget =
  | { readonly kind: "common" }
  | { readonly kind: "route"; readonly name: string }
  | { readonly kind: "ending"; readonly id: string }
  | { readonly kind: "steps"; readonly steps: readonly RouteStep[] };

/** 通読の道の候補（コアの `route::ReadingChoice`）。 */
export interface ReadingChoice {
  readonly label: string;
  readonly target: ReadingTarget;
}

/** 通読の本文の並び（コアの `route::ReadingItem`）。 */
export type ReadingItem =
  | {
      readonly kind: "scene";
      readonly id: string;
      readonly title: string;
      readonly route: string | null;
      readonly file: string;
      readonly span: readonly [number, number];
    }
  | {
      readonly kind: "say";
      readonly file: string;
      readonly span: readonly [number, number];
      readonly line_id: string | null;
      readonly speaker: string;
      readonly style: string;
      readonly text: readonly ReadingInline[];
    }
  | {
      readonly kind: "narrate";
      readonly file: string;
      readonly span: readonly [number, number];
      readonly line_id: string | null;
      readonly text: readonly ReadingInline[];
    }
  | { readonly kind: "break" }
  | {
      readonly kind: "choice";
      readonly file: string;
      readonly span: readonly [number, number];
      readonly prompt: readonly ReadingInline[] | null;
      readonly options: readonly {
        readonly text: readonly ReadingInline[];
        readonly enabled: boolean;
      }[];
      readonly chosen: number;
    }
  | { readonly kind: "ending"; readonly id: string | null };

/** 通読の結果（コアの `route::Reading` と、道の名前）。 */
export interface ReadingResult {
  readonly label: string;
  readonly reading: {
    readonly target: ReadingTarget;
    readonly items: readonly ReadingItem[];
    /** `ending`・`dead_end`・`route_changed`・`limit`・`unreachable`。 */
    readonly stop: string;
    readonly message: string | null;
  };
}

/** アイデアメモの昇格先（コアの `ideas::IdeaTarget`）。 */
export type IdeaTarget =
  | {
      readonly kind: "scene";
      readonly id: string;
      readonly title: string;
      readonly file: string;
      readonly span: readonly [number, number];
    }
  | {
      readonly kind: "entry";
      readonly type_id: string;
      readonly type_name: string;
      readonly id: string;
      readonly name: string;
      readonly path: string;
    }
  | { readonly kind: "missing"; readonly value: string };

/** アイデアメモ1枚（コアの `ideas::Idea`、T2-6）。 */
export interface Idea {
  readonly file: string;
  /** ファイルの中身（UTF-8で読めなければ null）。 */
  readonly text: string | null;
  /** `未整理`・`採用`・`保留`・`没`。 */
  readonly status: string;
  readonly tags: readonly string[];
  readonly promoted_to: string | null;
  readonly target: IdeaTarget | null;
  readonly body: string;
  readonly problems: readonly string[];
}

/** 昇格のしかた（コアの `ideas::Promotion`）。 */
export type Promotion =
  | {
      readonly kind: "entry";
      readonly type_id: string;
      readonly id: string;
      readonly name: string;
    }
  | {
      readonly kind: "scene";
      readonly file: string;
      readonly id: string;
      readonly title: string;
    };

/** 昇格の結果（コアの `workspace::Promoted`）。 */
export type Promoted =
  | {
      readonly kind: "saved";
      readonly text: string;
      readonly target: string;
      readonly scene: { readonly file: string; readonly text: string } | null;
    }
  | {
      /** 昇格先は作ったが、メモを書けなかった。 */
      readonly kind: "memo_unwritten";
      readonly target: string;
      readonly scene: { readonly file: string; readonly text: string } | null;
      readonly message: string;
    }
  | { readonly kind: "conflict"; readonly text: string }
  | { readonly kind: "unsupported"; readonly message: string };

/** 昇格した後に受け取るもの（設定DBを書いた後と同じ形）。 */
export interface PromotedWritten extends Omit<BibleWritten, "edit"> {
  readonly edit: Promoted;
}

/** 本文のシーン（企画書のエンディングの場所）。 */
export interface ConceptScene {
  readonly id: string;
  readonly title: string;
  readonly file: string;
  readonly span: readonly [number, number];
}

/** 企画書の画面のデータ（コアの `concept::ConceptData`、T2-6）。 */
export interface ConceptData {
  /** `concept.md` の中身（無ければ null）。 */
  readonly text: string | null;
  readonly problems: readonly string[];
  /** フロントマターの値（フォームの入力欄の値）。 */
  readonly fields: Readonly<Record<string, JsonValue>>;
  readonly logline: string;
  readonly theme: string;
  readonly tone: string;
  readonly volume_chars: number | null;
  readonly volume_routes: number | null;
  readonly main_characters: readonly {
    readonly id: string;
    readonly name: string | null;
    readonly path: string | null;
    readonly traits: readonly {
      readonly label: string;
      readonly value: string;
    }[];
  }[];
  readonly endings: readonly {
    readonly id: string;
    readonly name: string;
    readonly intent: string;
    readonly scenes: readonly ConceptScene[];
  }[];
  readonly endings_listed: boolean;
  readonly unplanned: readonly {
    readonly id: string;
    readonly scenes: readonly ConceptScene[];
  }[];
  readonly body: string;
}

/** マージの結果の一続き（コアの `merge::Hunk`、T2-7）。 */
export type MergeHunk =
  | { readonly kind: "merged"; readonly text: string }
  | {
      readonly kind: "conflict";
      readonly base: string;
      readonly mine: string;
      readonly theirs: string;
    };

/** 外部の変更とのマージの結果（コアの `merge::Merge`）。 */
export interface MergeView {
  readonly hunks: readonly MergeHunk[];
  readonly conflicts: number;
  /** 入力（元・自分・外）の見分け。当てるときに返す。 */
  readonly token: string;
}

/** ぶつかりの選び方（コアの `merge::Choice`）。 */
export type MergeChoice = "mine" | "theirs" | "both";

/** マージを当てた結果。 */
export interface MergeApplied {
  readonly text: string;
  readonly disk: string | null;
  readonly summary: Summary;
  readonly checks: readonly IrDiagnostic[];
}

/** 名前を変えるもの（コアの `rename::RenameTarget`、T2-8）。 */
export type RenameTarget =
  | { readonly kind: "scene"; readonly id: string }
  | { readonly kind: "character_name"; readonly id: string }
  | { readonly kind: "variable"; readonly key: string }
  | {
      readonly kind: "entry_id";
      readonly type_id: string;
      readonly id: string;
    };

/** カーソルのところの、名前を変えられるもの（コアの `rename::Symbol`）。 */
export interface RenameSymbol {
  readonly target: RenameTarget;
  readonly current: string;
  readonly label: string;
}

/** 名前の変更の見積もり。 */
export interface RenamePreview {
  readonly story_edits: number;
  readonly story: readonly string[];
  readonly settings: readonly string[];
  readonly moves: readonly { readonly from: string; readonly to: string }[];
  /** 知らせ（変えないもの、変わってしまうもの）。 */
  readonly notes: readonly string[];
}

/** 名前を変えた後に受け取るもの（設定DBを書いた後と同じ形）。 */
export interface RenameWritten extends Omit<BibleWritten, "edit"> {
  readonly edit: {
    readonly story: readonly { readonly file: string; readonly text: string }[];
    readonly settings: readonly string[];
    readonly story_edits: number;
    readonly notes: readonly string[];
    /** 途中で書けなかったとき、その理由（それまでに書いたものは story と settings にある）。 */
    readonly problem: string | null;
  };
}

/** 道の文字数と、読み終えるまでの目安（コアの `route::RouteStat`）。 */
export interface RouteStat {
  readonly label: string;
  readonly route: string | null;
  readonly chars: number;
  readonly minutes: number;
  readonly stop: string;
}

/** フロー図で引いた辺（コアの `flowedit::FlowEdge`、T2-9）。 */
export type FlowEdge =
  | { readonly kind: "jump"; readonly from: string; readonly to: string }
  | {
      readonly kind: "choice";
      readonly from: string;
      readonly to: string;
      readonly label: string;
    };

/** 本文に書き足した結果（章と、表示に要る分）。 */
export interface StoryWrittenResult {
  readonly file: string;
  readonly text: string;
  readonly summary: Summary;
  readonly checks: readonly IrDiagnostic[];
}

/** 型定義の編集（コアの `typedef::TypeEdit`、T2-10）。 */
export type TypeEdit =
  | {
      readonly kind: "add";
      readonly key: string;
      readonly label: string;
      readonly type: string;
      readonly options: readonly string[];
    }
  | {
      readonly kind: "set";
      readonly key: string;
      readonly label: string;
      readonly type: string;
      readonly options: readonly string[];
    }
  | { readonly kind: "move"; readonly key: string; readonly delta: number }
  | { readonly kind: "remove"; readonly key: string };
