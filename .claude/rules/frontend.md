---
paths:
  - "app/src/**/*"
  - "app/index.html"
  - "app/package.json"
  - "packages/**/*"
---

# フロントエンドの約束

## 基本

- TypeScriptは `strict`。`any` を使わない。コアから受け取るIRや診断の型は、1か所（`app/src/core/types.ts` など）で定義する。
- Reactは関数コンポーネントとフックで書く。状態はまずReact標準の仕組みで持ち、状態管理ライブラリを足すときは人に確認する。
- コアの呼び出し（Tauriの `invoke`）は1つのモジュール（`app/src/core/` など）にまとめ、コンポーネントから直接呼ばない。
- UIコンポーネント集は使わない。見た目は `docs/ui/README.md` のデザイントークンをCSS変数にして使う。

## エディタ（CodeMirror 6）

- 日本語入力の変換中は、補完、自動整形、括弧の自動補完、ライブ解析の結果反映を行わない。判定には `view.composing` と compositionstart／compositionend を使う。
- 変換中は、本文を置き換えない（保存で付いた行ID、外部の変更の読み込み直し）。保存もしない。変換が終わってから行う。本文の置き換えは、全体を入れ替えず、違う範囲だけを置き換える（カーソルを動かさないため。`app/src/editor/replace.ts`）。ただし、読み込み直しで改行の種類（`\r\n` と `\n`）が変わるときだけは、エディタが改行を1種類しか持てないので、状態を作り直す（D045）。
- 外部の変更とのマージ（T2-7、D058）は、コア（`merge_view`・`apply_merge`）が行い、画面（`editor/MergePanel.tsx`）はぶつかりを並べて選ばせるだけ。開く前に、まだ送っていない編集をコアへ送る。当てるときは、見せたマージの `token` を返す（コアが入力と照らす）。当てた結果は、開いたときの自分の本文と照らして置き換える（後で打った編集があれば置き換えずに、もう一度選ばせる）。
- マージの元は、知らせの途中の競合と、置き換えを見送ったときだけ App が覚えて渡す（`mergeBases`。置き換えを頼むときに、見送ったときの元を `replaceText` の4つ目の引数で渡す）。保存、読み込み直す、自分の変更を残す、マージで忘れる。
- 執筆表示（`app/src/editor/writing.ts`）は見た目だけを変え、本文の文字は変えない。飾りは Lezer の構文木から、見えている範囲だけを作る。改行をまたぐ置き換えは作らない（ビュープラグインからは作れない）。変換中は飾りを作り直さず、前の飾りを変更に合わせてずらすだけにし、変換が終わってから作り直す。
- AI が書いた行の印（`editor/provenance.ts`。T3-4）は、行末の行IDで出自を引いて、行に線を付けるだけ（本文は変えない）。変換中は作り直さず、前の印をずらすだけ。出自はアプリが作品を開いたとき・保存と読み込み直しのたび・`ai/provenance.jsonl` が外で変わったとき（変更の知らせ `provenance`）に取り直す。`ai` は実線、`ai-edited` は点線（色だけで分けない）。
- 執筆表示では行IDを隠し、守る（`app/src/editor/lineid.ts`）。カーソルを行IDに入れない。人の編集の後、分割と結合で仕様書9節のきまりどおりに行IDを置き直す。変換中の入力、取り消し、プログラムによる置き換えには手を出さない（D046）。
- 入力補助（補完の窓、括弧の補い、正規化、段落頭の字下げ。`complete.ts`・`typing.ts`）は、変換を確定した後か、変換を使わずに打った後（と Enter）にだけ動かす。補完は自動で出さず（`activateOnTyping: false`）、確定の後に開き、変換が始まったら閉じる。どの補助にも、変換中は動かないことのテストを付ける（D047）。
- 位置の変換：コアのスパンはUTF-8のバイト位置、CodeMirrorはUTF-16の位置。変換関数は1つだけ作り、そこを必ず通す。サロゲートペア（絵文字など）と全角文字を含むテストを付ける。
- 解析の依頼は、入力が止まってから行う（目安200ミリ秒のデバウンス）。変換中に解析結果で表示が変わらないようにする。
- ハイライトは `packages/lezer-scn` の文法で行う。Lezerは見た目のためだけに使い、意味の判断（台詞かどうか、参照が正しいか）はコアの結果に任せる。

## Lezer文法（packages/lezer-scn）

- 壊れた入力でもハイライトが崩れきらないよう、寛容に書く。行単位で状態が戻るようにする。
- 仕様書の行の種類と記法に対応するトークンを持たせる。トークン名は `docs/glossary.md` の英語名に合わせる。
- `samples/houkago/story/*.scn` を入力にしたテストを置く。

## 設定DBと変数の画面

- フォームで直した値を書くのはコア（`bible::set_field`。直したキーの値だけを差し替え、読み直して確かめる）。フロントでYAMLを組み立てない。
- 入力欄の値は、入力が決まったとき（フォーカスが外れた、変換中でない Enter、選んだ）に書く。変換中は書かない（D049）。
- 型から入力欄を選ぶ、入力を値にする、検索などの判断は `app/src/bible/fields.ts` に置き、テストを付ける。
- 変数の宣言のフォーム（T2-10、D061）は、設定DBと同じ入力欄（`FieldInput`）を `variables/form.ts` の項目の定義で使い、書くのはコア（`set_variable_field`。直した項目の行だけ）。範囲は最小と最大の欄を、両方そろってから `[最小, 最大]` で書く。
- 型定義の編集（`bible/TypeEditor.tsx`）は、項目を `key` で指し、画面が読んだときの型定義の中身を添えて書く（コアの `edit_type`）。項目の型の候補と選択肢の読み方は `bible/typeform.ts` に置き、テストを付ける（`app/test/forms.test.ts`）。Enter の後にフォーカスが外れても、同じ中身を2回書かない。

## プレビュー（app/src/preview/）

- ランタイム（`runtime.ts`）・式の評価（`expr.ts`）・メッセージの組み立て（`text.ts`）・行の探し方（`locate.ts`）は React と DOM に依存させず、テストを付ける。サンプルの各エンディングまで通すテスト（`app/test/preview.test.ts`）は、コアのスナップショット（`crates/scn-core/tests/snapshots/preview__houkago.snap`）を読む。
- 素材のファイルはアセットプロトコル（`core/api.ts` の `assetUrl`）で見せる。許すのは開いた作品の `assets/` だけ。
- エディタとの同期は、カーソルもフォーカスも動かさない行の印（`editor/marker.ts`）で行う。プレビュー中の行は行IDで探し、無ければバイト位置に戻る（D050）。
- カーソル位置から再生（T2-4、D055）は、コアの最短経路（`route`）の選択肢を `runtime.ts` の `replay` で速送りして始める。式の評価や命令の意味を変えたら、コアの `route` も同じに直し、`app/test/route.test.ts`（コアのスナップショット `route__*.snap` を速送りして、同じ変数になることを確かめる）を通す。
- 縦書きビュー（`preview/VerticalView.tsx`）は、コアの通読（`route::read`）を `preview/vertical.ts` で段落・行・ページにする。組み方（折り返し、縦中横、原稿用紙換算）は `vertical.ts` に置き、テストを付ける（`app/test/vertical.test.ts`）。
- プレビューの別ウィンドウ（`preview/PreviewWindow.tsx`。ウィンドウのラベルは `preview`）とメインウィンドウは、Tauri のイベントでだけつなぐ（`core/api.ts` の `sendPreviewLine` など）。ウィンドウどうしで状態を共有しない。

## 環境設定（app/src/preferences/）

- AI の接続先（T3-5、D066）：一覧と書き換えはコア（`ai_connections`・`set_ai_override`・`set_ai_cloud_allowed`・`set_ai_key`・`delete_ai_key`）。キーは入力をコアへ渡すだけで、画面は受け取らない（入力欄は保存したら空にする）。クラウドの「接続を試す」は、送る中身（コアの `test_preview`）を見せてから送る。使う前に足りないもの・タイトルバーの印の判断は `preferences/connection.ts` に置き、テストを付ける（`app/test/preferences.test.ts`）。印は色だけでなく形でも分ける。

- 設定DBの「AIに渡す内容」（T3-6）はコアの `ai::entry_context`（逆引きの `entry_usage` と一緒に受け取る）を `bible/aicontext.ts` で文にする。「＋ 差分を追加」は素材カタログに書く（`add_catalog_entries`）。名前の確かめ方は `bible/fields.ts` の `expressionNameProblem`。どちらもテストを付ける（`app/test/bible.test.ts`）。

## AI実行の画面（app/src/ai/）

- タスクの実行（T3-7、D068）はコア（`ai_prepare`・`ai_run`・`cancel_ai`）。実行と指摘と履歴は App が持つ（画面を切り替えても続く）。結果は回の番号で照らし、取り消した回・前の作品の回の結果は捨てる。クラウドの接続先では送る中身を見せてから送る。
- 提案（T3-8、D069）は `ai/ProposalPanel.tsx` が今の本文と照らし（コアの `view_proposals`）、行ごとに採用・却下する。採用はコア（`adopt_proposals`）が置き換えて保存し、App がエディタの本文を置き換える（名前の変更と同じ扱い）。外部の変更とぶつかっている章には採用しない。フロントで本文を書き換えない。
- 指摘はアプリの中だけに持つ。エディタの印（`editor/findings.ts`）は行末の行IDで引き、本文は変えない。変換中は作り直さず、前の印をずらすだけ。並べ方・持ち方・文は `ai/model.ts` に置き、テストを付ける（`app/test/ai.test.ts`）。

## 検証の画面（app/src/check/）

- 自動巡回と表記の検査（検証の段階2。T3-2・T3-3、D063・D064）は、検証の画面の「走らせる」で、コア（`playthrough` コマンド。`playthrough::stage2`）が行う。保存のたびには走らせない。結果の診断は検証の一覧に足し、本文が変わったら結果を捨てる（位置がずれるため）。結果の見せ方（道の例、変数の値、並べ方）は `check/playthrough.ts` に置き、テストを付ける（`app/test/check.test.ts`）。

## 名前の変更・コマンドパレット・表示の切り替え（T2-8）

- F2 はエディタの keymap で受け、変換中は動かさない。開く前に今の本文をコアへ送り、カーソルのところの名前はコア（`symbol_at`）が探す。置き換えもコア（`apply_rename`）が行い、変わった本文は `replaceText` でエディタに入れる。
- パレット・名前の変更・章の名前の欄は、変換を確定する Enter や矢印では動かさない（`isComposing` か `keyCode 229`）。フォームの暗黙の送信に頼らない。
- 集中モード・タイプライタースクロール・書体は、この端末の localStorage に覚える（読めなくても表示は動く）。タイプライタースクロールは `editor/typewriter.ts`（変換中は動かさない）。
- パレットの絞り込み、W406 の直し方、ルートの文字数の見せ方は `shell/palette.ts`・`editor/quickfix.ts`・`shell/scene.ts` に置き、テストを付ける（`app/test/editor-tools.test.ts`）。

## 企画の画面（app/src/concept/）

- メモと企画書の項目は、設定DBの画面と同じ入力欄（`bible/FieldInput.tsx`、`BodyField`）と書き換え（`set_entry_field` など。直したキーの行だけ）を使う。フロントで YAML を組み立てない。
- 昇格はコア（`promote_idea`）が行う。章にスタブのシーンを足したら、App の `onStoryWritten` でエディタの中身を差し替える（外部の変更の読み込み直しと同じ扱い）。保存していない変更のある章は選ばせない。
- メモの並べ方、絞り込み、昇格の欄の初めの値は `concept/ideas.ts` に置き、テストを付ける（`app/test/ideas.test.ts`）。

## フロー図（xyflow）

- ノードと辺はIRから作る。座標は自動で配置し、人が動かした位置は `layout.json` に保存する（T1-9）。
- 図からの編集（T2-9、D060）は、本文への書き足しだけ（コアの `add_stub`・`add_flow_edge`）。ノードと辺を消す操作は作らない。欄は変換を確定する Enter で足さない。
- スタブ、AI下書き、選択中の見分け方は画面モックに合わせる。
