// 検証の画面の判断（docs/ui/decisions-v1.md の U19、見本 09-check.html。T4-19）：診断のコードごとの種類・短い名前・
// 説明・直し方の表、重さで絞る、種類ごと・ファイルごと・シーンごとにまとめる、行ごとの行き先。React と DOM に
// 依存させない（テストする。表が仕様書の診断の一覧を覆っていることも確かめる）。

import type { IrDiagnostic, SceneSummary, Severity } from "../core/types.ts";
import { diagnosticPlaces } from "./group.ts";

/** 問題の種類（一覧のまとまり）。並びは見本の順（構造・変数・表示・素材・行ID）から。 */
export const CATEGORIES = [
  "構造",
  "変数",
  "表示",
  "素材",
  "行ID",
  "台詞",
  "表記",
  "命令",
  "書式",
  "設定DB",
  "設定ファイル",
] as const;

export type Category = (typeof CATEGORIES)[number];

export interface CodeInfo {
  readonly category: Category;
  /** 短い名前（右の欄の見出し）。 */
  readonly title: string;
  /** 何が起きているか。 */
  readonly about: string;
  /** 直し方（順に試す）。 */
  readonly fixes: readonly string[];
}

/** コードごとの説明と直し方（仕様書 docs/spec/scn-format-v0.md の診断の一覧と同じコード）。 */
export const CODE_INFO: Readonly<Record<string, CodeInfo>> = {
  W101: {
    category: "書式",
    title: "シーンより前の本文",
    about:
      "最初のシーン見出しより前に本文があります。この行は読み飛ばされます。",
    fixes: ["シーン見出し（## 名前 #ID）の下へ移す", "要らなければ消す"],
  },
  E102: {
    category: "書式",
    title: "閉じていないメタ情報",
    about: "シーンのメタ情報の --- が閉じていません。",
    fixes: ["メタ情報の終わりに --- の行を足す"],
  },
  E103: {
    category: "書式",
    title: "閉じていないブロック",
    about: "@選択・@if・@raw のブロックが @end で閉じていません。",
    fixes: ["ブロックの終わりに @end を足す"],
  },
  E104: {
    category: "書式",
    title: "対応の無い @elif・@else・@end",
    about: "開いているブロックが無いのに @elif・@else・@end があります。",
    fixes: ["対応する @if・@選択 を足す", "余分な行を消す"],
  },
  E105: {
    category: "書式",
    title: "選択の外の選択肢",
    about: "選択ブロックの外に、選択肢の行（- で始まる行）があります。",
    fixes: ["@選択 の中へ移す", "地の文なら、- をやめる"],
  },
  E106: {
    category: "書式",
    title: "選択肢の無い選択",
    about: "@選択 に選択肢が1つもありません。",
    fixes: ["- 表示文 の行を足す", "要らなければ @選択 から @end までを消す"],
  },
  W107: {
    category: "書式",
    title: "IDの無いシーン",
    about:
      "シーン見出しにIDがありません。フローでは仮のIDになり、ジャンプ先にできません。",
    fixes: ["見出しの後ろに #s012 のようなIDを足す"],
  },
  E108: {
    category: "書式",
    title: "シーンIDの重複",
    about: "同じシーンIDが2つ以上あります。",
    fixes: ["どちらかのIDを変える（参照ごと変えるなら「名前の変更」（F2））"],
  },
  I109: {
    category: "書式",
    title: "知らないメタ情報のキー",
    about: "メタ情報に、決まっていないキーがあります。値はそのまま残ります。",
    fixes: [
      "綴りを確かめる（summary・status・route・tags）",
      "わざとなら、そのままでよい",
    ],
  },
  E110: {
    category: "書式",
    title: "status の値が不正",
    about: "status は draft・writing・done・ai-draft のどれかです。",
    fixes: ["決まった値のどれかにする"],
  },
  E111: {
    category: "書式",
    title: "IDの形が不正",
    about: "シーンIDやエンディングIDに使えない文字があります。",
    fixes: ["英数字と _ だけにする"],
  },
  W112: {
    category: "書式",
    title: "先頭以外の章見出し",
    about: "ファイルの先頭以外に # の行があります。地の文として扱います。",
    fixes: ["章見出しならファイルの先頭へ移す", "シーン見出しなら ## にする"],
  },
  W113: {
    category: "書式",
    title: "メタ情報のキーの重複",
    about: "メタ情報に同じキーが2つあります。後の値を使います。",
    fixes: ["どちらかの行を消す"],
  },
  E114: {
    category: "書式",
    title: "メタ情報の行の形",
    about: "メタ情報の行が「キー: 値」の形になっていません。",
    fixes: ["「キー: 値」の形に直す"],
  },
  E201: {
    category: "台詞",
    title: "閉じていない台詞",
    about: "台詞の閉じ括弧がありません。",
    fixes: ["台詞の終わりに閉じ括弧を足す"],
  },
  W202: {
    category: "台詞",
    title: "話者を解決できない",
    about:
      "台詞の形ですが、話者が設定DBのキャラクターに見つかりません。地の文として扱います。",
    fixes: [
      "設定DBにキャラクターを登録する（「登録」から）",
      "呼び名なら、キャラクターの呼び名に足す",
    ],
  },
  W203: {
    category: "表示",
    title: "ウィンドウからはみ出す",
    about: "台詞か地の文が、メッセージウィンドウの行数に収まりません。",
    fixes: [
      "文を分ける（行を分けるか、{p} で改ページする）",
      "言い回しを短くする",
    ],
  },
  I204: {
    category: "表記",
    title: "一人称の表記ゆれ",
    about: "台詞の一人称が、設定DBの一人称と読みが同じで書き方が違います。",
    fixes: ["設定DBの一人称の書き方にそろえる", "わざとなら、そのままでよい"],
  },
  I205: {
    category: "表記",
    title: "避ける書き方",
    about: "設定DBのエントリで避けると決めた書き方が、本文にあります。",
    fixes: ["エントリの名前の書き方にそろえる"],
  },
  I206: {
    category: "表記",
    title: "未登録の用語",
    about:
      "設定DBの名前・呼び名と、かなの書き方だけが違う書き方が本文にあります。",
    fixes: [
      "名前の書き方にそろえる",
      "別の書き方も使うなら、呼び名か避ける書き方に足す",
    ],
  },
  E301: {
    category: "行ID",
    title: "行IDの重複",
    about: "同じ行IDが2つ以上あります。ボイスや翻訳が取り違えられます。",
    fixes: ["写した行の行IDを消し、保存し直して新しい行IDを付ける"],
  },
  I302: {
    category: "行ID",
    title: "行IDが無い",
    about: "行IDを持てる行に、まだ行IDがありません。",
    fixes: ["保存すると付く（scn fmt でも付く）"],
  },
  W303: {
    category: "行ID",
    title: "行IDを持てない行",
    about: "行IDを持てない行（命令など）に行IDがあります。",
    fixes: ["その行の行IDを消す"],
  },
  E304: {
    category: "行ID",
    title: "行IDの形が不正",
    about:
      "行IDは ^ と小文字の英数字（0-9・a-z）4〜12字です。この候補は形に合わないので、行IDとして扱いません。",
    fixes: ["形の合わない ^… を消す（消してから保存すると、新しい行IDが付く）"],
  },
  W401: {
    category: "命令",
    title: "知らないディレクティブ",
    about: "決まっていない @ の命令があります。",
    fixes: ["綴りを確かめる（@背景・@立ち絵・@BGM など）"],
  },
  E402: {
    category: "命令",
    title: "引数の形が合わない",
    about: "ディレクティブの引数の数か、選択肢の形が合いません。",
    fixes: ["仕様の書き方（FORMAT.md）に合わせる"],
  },
  I403: {
    category: "素材",
    title: "カタログに無い素材",
    about: "素材カタログに無い意味名です。仮素材として扱います。",
    fixes: ["素材の画面の「未登録をカタログに足す」で足す", "綴りを確かめる"],
  },
  W404: {
    category: "素材",
    title: "立ち絵のキャラを解決できない",
    about: "@立ち絵・@退場 のキャラが、設定DBのキャラクターに見つかりません。",
    fixes: ["設定DBにキャラクターを登録する", "名前か呼び名に合わせる"],
  },
  W405: {
    category: "命令",
    title: "知らないインライン命令",
    about: "{…} のインライン命令が決まっていないか、値が合いません。",
    fixes: ["{w 0.5}・{p} などの決まった書き方にする"],
  },
  W406: {
    category: "命令",
    title: "全角の記号で始まる行",
    about:
      "行頭が全角の ＠・－＞・＃＃ などで、地の文になっています。誤入力のおそれがあります。",
    fixes: ["命令なら半角の @・->・## にする", "わざとなら、そのままでよい"],
  },
  E501: {
    category: "構造",
    title: "無いシーンへのジャンプ",
    about: "存在しないシーンへジャンプか呼び出しをしています。",
    fixes: [
      "行き先のシーンIDを確かめる",
      "行き先のシーンを足す（フローの「スタブを足す」）",
    ],
  },
  W502: {
    category: "構造",
    title: "実行されない行",
    about: "ジャンプ・@エンディング・@戻る の後に行があり、実行されません。",
    fixes: ["その行をジャンプの前へ移す", "要らなければ消す"],
  },
  W503: {
    category: "構造",
    title: "行き止まり",
    about: "シーンの終わりに出口の無い経路があります。",
    fixes: ["シーンの終わりに -> シーンID・@エンディング・@戻る を足す"],
  },
  W504: {
    category: "構造",
    title: "到達できないシーン",
    about: "開始シーンから、このシーンへ着く道がありません。",
    fixes: ["どこかのシーンからジャンプか選択肢でつなぐ", "要らなければ消す"],
  },
  W505: {
    category: "構造",
    title: "エンディングに着かないシーン",
    about: "このシーンから、どのエンディングにも着きません（ジャンプの循環）。",
    fixes: ["循環の途中から抜ける出口を足す"],
  },
  W506: {
    category: "構造",
    title: "選べない選択肢",
    about: "着いても条件が真にならず、決して選べない選択肢です（段階2）。",
    fixes: ["条件と、そこまでの変数の書き方を見直す"],
  },
  W507: {
    category: "構造",
    title: "真にならない条件",
    about: "評価されても決して真にならない @if・@elif の条件です（段階2）。",
    fixes: ["条件と、そこまでの変数の書き方を見直す"],
  },
  W508: {
    category: "構造",
    title: "選べる選択肢が無くなる",
    about:
      "条件のせいで、選べる選択肢が1つも無くなることがある選択です（段階2）。",
    fixes: ["条件の無い選択肢を1つ置く", "条件を見直す"],
  },
  W509: {
    category: "構造",
    title: "着かないシーン",
    about:
      "段階1では着けますが、条件と選択肢をどう選んでも着きません（段階2）。",
    fixes: ["このシーンへ向かう条件を見直す"],
  },
  E601: {
    category: "変数",
    title: "式の構文エラー",
    about: "条件か @設定 の式が読めません。",
    fixes: ["式の書き方（変数 演算子 値）に合わせる"],
  },
  W602: {
    category: "変数",
    title: "宣言されていない変数",
    about: "variables.yaml に宣言の無い変数を使っています。",
    fixes: ["変数の画面で宣言する", "綴りを確かめる"],
  },
  E603: {
    category: "変数",
    title: "型の不一致",
    about: "変数の型と、比べる値や入れる値の型が合いません。",
    fixes: ["値か宣言の型を合わせる"],
  },
  E604: {
    category: "変数",
    title: "整数でない += ・-=",
    about: "+=・-= を整数以外の変数に使っています。",
    fixes: ["整数の変数にする", "= で入れる"],
  },
  W605: {
    category: "変数",
    title: "書いて読まない変数",
    about:
      "立てた値がどの条件にも使われていません。伏線の回収漏れか、もう要らない変数です。",
    fixes: [
      "読む場所を足す（例：エピローグの @if で使う）",
      "要らないなら、宣言と書く行を消す",
    ],
  },
  W606: {
    category: "変数",
    title: "書かれない変数",
    about: "読まれるのに、一度も書かれない変数です。いつも初期値で比べます。",
    fixes: ["どこかで @設定 で書く", "初期値のままでよいなら、そのままでよい"],
  },
  W701: {
    category: "構造",
    title: "本文に無いエンディング",
    about: "企画書のエンディングが、本文の @エンディング にありません。",
    fixes: ["本文に @エンディング を書く", "企画書のエンディングを直す"],
  },
  W702: {
    category: "構造",
    title: "企画書に無いエンディング",
    about: "本文のエンディングが、企画書にありません。",
    fixes: ["企画書のエンディングに足す"],
  },
  W703: {
    category: "構造",
    title: "回収の無い伏線",
    about: "伏線に、回収するシーンがありません。",
    fixes: ["設定DBの伏線のエントリに回収シーンを書く"],
  },
  W704: {
    category: "構造",
    title: "仕込みを通らない回収",
    about:
      "仕込みのシーンを通らずに、回収のシーンへ着ける道があります（段階2）。",
    fixes: ["回収へ向かう道に条件を足す", "仕込みを通る道に寄せる"],
  },
  E801: {
    category: "設定ファイル",
    title: "読めないファイル",
    about:
      "ファイルかフォルダを読めません（構文の誤り、UTF-8でない、権限が無い）。無いものとして扱います。",
    fixes: ["書き方の誤りを直す", "UTF-8 で保存し直す"],
  },
  E802: {
    category: "設定ファイル",
    title: "項目の形が不正",
    about: "設定ファイルの項目の形が合いません。その項目を読み飛ばします。",
    fixes: ["作品フォルダの FORMAT.md（作品フォルダの規約）の書き方に合わせる"],
  },
  E803: {
    category: "設定ファイル",
    title: "対応していない版",
    about:
      "project.toml の format_version に対応していません。v0 として読みます。",
    fixes: ["format_version を 0 にする"],
  },
  W804: {
    category: "設定DB",
    title: "型定義の無いフォルダ",
    about: "bible/entries/ の下に、型定義の無いフォルダがあります。",
    fixes: ["設定DBの画面で型を作る", "フォルダの名前を型のIDに合わせる"],
  },
  W805: {
    category: "設定DB",
    title: "型に合わない値",
    about: "設定DBのエントリの項目の値が、型に合いません。",
    fixes: ["設定DBの画面のフォームで直す"],
  },
  W806: {
    category: "設定DB",
    title: "参照切れ",
    about: "設定DBの参照の参照先がありません。",
    fixes: ["参照先のエントリを作る", "参照を直す"],
  },
  W807: {
    category: "変数",
    title: "宣言の値が型に合わない",
    about: "変数の宣言の default・range・scope が型に合いません。",
    fixes: ["変数の画面の宣言のフォームで直す"],
  },
};

/** コードの説明（表に無いコードは、種類「書式」と診断の文だけ）。 */
export function codeInfo(code: string): CodeInfo {
  return (
    CODE_INFO[code] ?? {
      category: "書式",
      title: code,
      about: "",
      fixes: [],
    }
  );
}

/** 重さで絞る（null はすべて）。 */
export function filterBySeverity(
  diagnostics: readonly IrDiagnostic[],
  severity: Severity | null,
): IrDiagnostic[] {
  return severity === null
    ? [...diagnostics]
    : diagnostics.filter((d) => d.severity === severity);
}

export type Grouping = "category" | "file" | "scene";

export const GROUPING_LABEL: Readonly<Record<Grouping, string>> = {
  category: "種類ごと",
  file: "ファイルごと",
  scene: "シーンごと",
};

/** 診断のあるシーン（同じ章で、診断より前で始まるいちばん後ろのシーン）。本文の外なら null。 */
export function sceneOf(
  scenes: readonly SceneSummary[],
  d: IrDiagnostic,
): SceneSummary | null {
  let found: SceneSummary | null = null;
  for (const s of scenes) {
    if (s.file !== d.file || s.span[0] > d.span[0]) continue;
    if (found === null || s.span[0] > found.span[0]) found = s;
  }
  return found;
}

const RANK: Readonly<Record<Severity, number>> = {
  error: 0,
  warning: 1,
  info: 2,
};

/**
 * 一覧のまとまり。種類ごとは決まった順、ファイルごとは本文のファイルを `files` の順に先に、シーンごとは本文の順に
 * 並べ、シーンの外の診断はファイルの名前でまとめる。まとまりの中は重さ・ファイル・位置の順。同じ診断は1つにする。
 */
export function groupProblems(
  diagnostics: readonly IrDiagnostic[],
  grouping: Grouping,
  files: readonly string[],
  scenes: readonly SceneSummary[],
): { readonly label: string; readonly items: IrDiagnostic[] }[] {
  const seen = new Set<string>();
  const unique = diagnostics.filter((d) => {
    const key = `${d.code}\u0000${d.file}\u0000${d.span[0]}\u0000${d.span[1]}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
  const fileRank = (file: string) => {
    const i = files.indexOf(file);
    return i === -1 ? files.length : i;
  };
  const sorted = [...unique].sort(
    (a, b) =>
      RANK[a.severity] - RANK[b.severity] ||
      fileRank(a.file) - fileRank(b.file) ||
      (a.file < b.file ? -1 : a.file > b.file ? 1 : 0) ||
      a.span[0] - b.span[0],
  );
  const groups = new Map<string, IrDiagnostic[]>();
  const order: string[] = [];
  const keyOf = (d: IrDiagnostic): [string, number] => {
    if (grouping === "category") {
      const category = codeInfo(d.code).category;
      return [category, CATEGORIES.indexOf(category)];
    }
    if (grouping === "file") return [d.file, fileRank(d.file)];
    const s = sceneOf(scenes, d);
    if (s === null) return [d.file, scenes.length + fileRank(d.file)];
    return [`${s.id} ${s.title}`, scenes.indexOf(s)];
  };
  const rank = new Map<string, number>();
  for (const d of sorted) {
    const [label, r] = keyOf(d);
    if (!groups.has(label)) {
      groups.set(label, []);
      order.push(label);
      rank.set(label, r);
    }
    groups.get(label)?.push(d);
  }
  // 本文でないファイルは同じ順位なので、名前の順に並べる。
  return order
    .sort(
      (a, b) =>
        (rank.get(a) ?? 0) - (rank.get(b) ?? 0) || (a < b ? -1 : a > b ? 1 : 0),
    )
    .map((label) => ({ label, items: groups.get(label) ?? [] }));
}

/** 行ごとの行き先（今ある移り先だけ）。開けないファイルなら null。 */
export function destinationOf(
  d: IrDiagnostic,
  files: readonly string[],
): string | null {
  if (files.includes(d.file)) return "本文で開く";
  if (d.file.startsWith("bible/entries/")) return "設定で見る";
  if (d.file === "variables.yaml") return "変数で見る";
  if (d.file === "assets/catalog.yaml") return "素材で見る";
  if (d.file === "concept.md" || d.file.startsWith("ideas/"))
    return "企画を開く";
  return null;
}

/** 該当の行と前後1行（行の番号は1から）。中身が無ければ空。 */
export function contextLines(
  text: string | undefined,
  offset: number,
): { readonly line: number; readonly text: string; readonly at: boolean }[] {
  if (text === undefined) return [];
  const bytes = new TextEncoder().encode(text);
  const end = Math.min(offset, bytes.length);
  let line = 1;
  for (let i = 0; i < end; i += 1) if (bytes[i] === 0x0a) line += 1;
  const lines = text.split("\n");
  const from = Math.max(1, line - 1);
  const to = Math.min(lines.length, line + 1);
  const result = [];
  for (let n = from; n <= to; n += 1)
    result.push({
      line: n,
      text: (lines[n - 1] ?? "").replace(/\r$/, ""),
      at: n === line,
    });
  return result;
}

/** 診断の行の番号（1から。求め方は `group.ts` の `diagnosticPlaces` で、ファイルごとに1回）。中身の無いファイルは null。 */
export function lineNumbers(
  diagnostics: readonly IrDiagnostic[],
  textOf: (file: string) => string | undefined,
): Map<IrDiagnostic, number | null> {
  const byFile = new Map<string, IrDiagnostic[]>();
  for (const d of diagnostics) {
    const list = byFile.get(d.file);
    if (list === undefined) byFile.set(d.file, [d]);
    else list.push(d);
  }
  const result = new Map<IrDiagnostic, number | null>();
  for (const [file, list] of byFile) {
    const text = textOf(file);
    const places = text === undefined ? [] : diagnosticPlaces(list, text, []);
    list.forEach((d, i) => {
      result.set(d, places[i]?.line ?? null);
    });
  }
  return result;
}
