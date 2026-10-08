# 用語集

日本語の用語と、コード・ファイル・JSONで使う英語名の対応。型名は `PascalCase`、フィールドや関数は `snake_case`（Rust）または `camelCase`（TypeScript）にする。新しい概念を足したら、ここにも足す。

## プロジェクトと企画

| 日本語 | 英語名 | 説明 |
| --- | --- | --- |
| プロジェクト | project | 1作品のフォルダ。`project.toml` を持つ |
| 企画書 | concept | `concept.md`。ログライン、テーマ、エンディング一覧 |
| ログライン | logline | 作品を一文で言ったもの |
| 文体 | style | `style.md`。文体の方針と文体見本 |
| 文体見本 | style sample | 自分で書いた見本の文章。AI生成の文脈に入れる |
| アイデアメモ | idea | `ideas/` の1ファイル |
| 昇格 | promote | メモから設定エントリ・スタブシーン・伏線を作ること |
| 昇格先 | promoted_to / target | メモの `promoted_to`（`scene/<ID>` か `<型>/<ID>`）。解いたものは `IdeaTarget` |

## 設定DB

| 日本語 | 英語名 | 説明 |
| --- | --- | --- |
| 設定DB | bible | 設定の型とエントリの集まり（`bible/`） |
| 型 | entry type | `bible/types/*.yaml`。項目の定義 |
| エントリ | entry | `bible/entries/<型>/<id>.md`。1つの設定 |
| 項目 | field | 型が定める入力欄 |
| 参照 | ref | 他のエントリやシーンを指す項目の値 |
| キャラクター | character | 組み込みの型。話者の解決に使う |
| 呼び名 | alias | キャラクターの別名。話者の解決に使う |
| 欲求・葛藤 | want / conflict | キャラクターの項目。プロット系AIの文脈 |
| 伏線 | foreshadow | 型の1つ。仕込み（setup）と回収（payoff） |
| 項目の型 | field type | `text`・`ref<型ID>`・`list<型>` など（コアの `FieldType`） |
| 参照切れ | broken reference | 参照先の無い参照（W806） |
| 逆引き | usage | エントリが使われている場所（登場シーン、名前の出る行、参照している・されている） |
| フロントマター | front matter | エントリの先頭の `---` で囲んだYAML |
| 本文（エントリの） | body | フロントマターの後ろのMarkdown |
| 紐づく変数 | character variables | キャラクターIDを名前空間にした変数 |
| 型定義の編集 | type edit | 型の項目を足す・直す・並べ替える・消す（`typedef::TypeEdit`。アプリの `TypeEditor`） |

## 構造と本文

| 日本語 | 英語名 | 説明 |
| --- | --- | --- |
| 章 | chapter | 1つの `.scn` ファイル |
| 章見出し | chapter heading | `# ` の行 |
| シーン | scene | `## タイトル #id` で始まる単位 |
| シーンID | scene id | `s012` など |
| メタ情報 | scene meta | 見出し直後の `---` ブロック |
| 状態 | status | `draft` / `writing` / `done` / `ai-draft` |
| スタブ | stub | 本文に台詞・地の文が無いシーン（プロットカード） |
| ルート | route | シーンのまとまり（`route:`） |
| 構造グラフ | story graph | シーンをノード、ジャンプなどを辺にしたグラフ |
| 辺 | edge | シーン間のつながり |
| 本文 | body | シーンの中の行の並び |
| 行 | line | `.scn` の1行 |
| 行ID | line id | `^k3f9`。行の不変の識別子 |
| 台詞 | dialogue | `話者「本文」` |
| 台詞候補 | dialogue candidate | 構文解析の段階で台詞の形に見える行 |
| 話者 | speaker | 台詞を話すキャラクター（IDに解決する） |
| 表示名 | display name | `灯（？？？）` の上書き名 |
| 表示スタイル | dialogue style | 括弧の種類ごとの台詞の見せ方（`normal`、`thought` など）。`project.toml` の `[dialogue.brackets]` で決める |
| 地の文 | narration | 台詞でも命令でもない本文 |
| エスケープ | escape | 行頭の `\`。取り除いたうえで、その行を地の文として分類させる |
| 段落の区切り | break | 空行から作る区切り |
| ディレクティブ | directive | `@` で始まる行 |
| ブロック | block | `@選択`・`@if`・`@raw` から `@end` まで |
| 選択ブロック | choice | `@選択` のブロック |
| 問いかけ文 | prompt | `@選択` の後ろの文 |
| 選択肢 | option | `- ` の行 |
| 条件分岐 | conditional (if) | `@if` のブロック |
| 条件 | cond | 選択肢や分岐の式 |
| ジャンプ | jump | `-> id` |
| 呼び出し・戻る | call / return | `@呼出` と `@戻る` |
| エンディング | ending | `@エンディング id` |
| 生コード | raw | `@raw` のブロック |
| 文中の記法 | inline markup | ルビ、傍点、インライン命令など |
| ルビ | ruby | `\|親文字《るび》`（base / reading） |
| 傍点 | emphasis dots | `《《強調》》` |
| インライン命令 | inline command | `{w 0.5}` など |
| 変数の埋め込み | interpolation | `{=式}` |
| 改行（台詞の） | line break | 複数行の台詞の、行の境目。IRの `line_break` |

## エディタ

| 日本語 | 英語名 | 説明 |
| --- | --- | --- |
| 執筆表示 | writing view | 記号を畳んで見せる表示（D046） |
| ソース表示 | source view | 生のテキストと行番号を見せる表示 |
| 入力補助 | input assist | 括弧の補い、正規化、段落頭の字下げ（`typing.ts`、D047） |
| 補完 | completion | 候補の窓。キャラ名、命令、引数、シーンID（`complete.ts`） |
| 正規化 | normalize | `...` を `……` にするなど（`[editor]` の `normalize_ellipsis`） |
| 段落頭の字下げ | narration indent | 地の文の行頭の全角空白（`indent_narration`） |
| 文脈パネル | context panel | エディタの右。今のシーンの登場キャラ、変数、素材、問題 |
| 今のシーン | current scene | カーソルのあるシーン |
| 表示する文字数 | display chars | ルビは親文字、記法と空白を除いて1字ずつ数えた字数（`SceneSummary.chars`） |
| クイックオープン | quick open | シーンをIDと題名で探して開く（Mod-P） |
| 定義へ移る | go to definition | シーンIDからシーンへ、話者名からキャラクターへ（F12、Mod-クリック） |
| 名前の変更 | rename | シーンID・キャラクターの名前・変数・エントリのIDを全体で置き換える（F2。`rename`、D059） |
| コマンドパレット | command palette | コマンドを名前で探して行う（Mod-Shift-P） |
| 集中モード | focus mode | ほかのペインを隠してエディタだけにする（Mod-Shift-F） |
| タイプライタースクロール | typewriter scroll | カーソルの行を縦の真ん中に置く |
| マージ | merge | 保存していない変更と外部の変更を、元・自分・外の3方向で1つにする（`merge`、D058） |
| 元・自分・外 | base / mine / theirs | マージの3つの中身：読み込んだときのディスクの中身、保存していない変更、外で変わった中身 |
| ぶつかり | conflict | 自分と外が同じところを違うように変えたもの（`Hunk::Conflict`）。自分・外・両方から選ぶ |

## 変数と検証

| 日本語 | 英語名 | 説明 |
| --- | --- | --- |
| 変数 | variable | `variables.yaml` で宣言する |
| 名前空間 | namespace | `akari.好感度` の `akari` |
| スコープ | scope | `save`（セーブ単位）/ `persistent`（周回をまたぐ） |
| 宣言 | declaration | `variables.yaml` の1つの変数の定義（型、値、既定値、範囲、スコープ、説明。フォームは `vardecl`） |
| 範囲 | range | 整数の変数の `[最小, 最大]` |
| 式 | expr | 条件式 |
| 代入 | assignment | `@設定` |
| 構文木 | AST | パーサの出力 |
| 中間表現 | IR | 全機能の共通入力 |
| 診断 | diagnostic | コード、重大度、位置、メッセージ |
| 重大度 | severity | `error` / `warning` / `info` |
| 位置 | span | UTF-8のバイト位置の範囲 |
| 検証 | check | プロジェクト全体の検査（CLIの `check`） |
| 索引 | index | IRから導く一覧。シーン、キャラクターの出現、変数の読み書き、素材の使用、行ID |
| 到達性 | reachability | 開始シーンからたどり着けるか |
| 行き止まり | dead end | 出口の無い経路 |
| 自動巡回 | auto playthrough | 選択肢の組み合わせを実際に実行する検証（検証の段階2。`playthrough`、`scn check --stage 2`） |
| カバレッジ | coverage | 自動巡回で着いたシーンと選べた選択肢の数（`playthrough::Coverage`） |
| 到達条件 | ending reach | エンディングに着くか、道の例、着いたときの変数の値（`playthrough::EndingReach`） |
| 表記ゆれ | wording variant | 同じ語の書き方の違い（一人称の `私` と `わたし`、避ける書き方 `variants`。I204・I205。`wording`） |
| 避ける書き方 | variants | 設定DBのエントリの `variants`。本文にあれば表記ゆれとして知らせる |
| 未登録の用語 | unregistered term | 設定DBの名前・呼び名と、かなの書き方だけが違う書き方（I206） |
| 周回 | replay | 永続の変数の値を引き継いで、もう一度始めること。自動巡回は、エンディングで書いた永続の変数の値から周回する |

## 素材・プレビュー・出力

| 日本語 | 英語名 | 説明 |
| --- | --- | --- |
| 素材 | asset | 背景、立ち絵、BGM、SE |
| 素材カタログ | asset catalog | `assets/catalog.yaml` |
| 意味名 | semantic name | `屋上/夕方` のような素材の名前 |
| 仮素材 | placeholder asset | ファイルの無い意味名 |
| 差分 | expression | 立ち絵の表情の違い |
| 位置 | position | `left` / `center` / `right` |
| 発注リスト | asset order list | まだファイルの無い素材の一覧（`export/orders.csv`。`scn export orders`、`assets::orders`） |
| 素材の状態 | asset state | ファイルあり（`file`）・仮素材（`placeholder`）・未登録（`unregistered`）。`assets::AssetState` |
| 未登録の素材 | unregistered asset | 本文で使われているが、カタログに無い意味名（I403） |
| プレビュー | preview | ADVプレイヤーと縦書きビュー |
| ランタイム | runtime | IRを解釈してプレビューを進めるもの（`app/src/preview/runtime.ts`） |
| 舞台 | stage | プレビューの背景・立ち絵・BGMの今の状態 |
| 止まり | stop | ランタイムが止まるところ（台詞・地の文、選択肢、待ち、エンディング、行き止まりなど） |
| メッセージウィンドウ | message window | 台詞と地の文を出す枠（`[message_window]`） |
| 画面の大きさ | screen size | プレビューの画面（`[screen]`） |
| バックログ | backlog | これまでに出た台詞・地の文と選んだ選択肢 |
| デバッグの欄 | debug panel | 変数の編集、ジャンプ、自動選択、バックログ |
| 自動選択 | auto choice | 選択肢を1番目かランダムで選ぶ |
| トランジション | transition | 背景や立ち絵の切り替え方（プレビューはフェードだけ再現） |
| エクスポート | export | エンジン用ファイルなどへの書き出し |
| 出力プロファイル | export profile | `export/*.yaml`（Ren'Py は `export/renpy.yaml`、`RenpyProfile`） |
| Ren'Py への書き出し | Ren'Py export | `scn export renpy`。書き出し先の `game/scn/` を作り直す（`renpy::export_renpy`） |
| 書き出し先 | output | 書き出すフォルダ（Ren'Py は既定で `export/renpy`） |
| 通読 | reading | 道を一本道にした本文の並び（`route::read`）。縦書きビューとプレーンテキストに使う |
| 道（通読の） | reading target | 共通のみ・ルート・エンディング・選んだ道（`route::ReadingTarget`） |
| 縦書きビュー | vertical view | プレビューの縦書きの表示（`preview/VerticalView.tsx`、組み方は `vertical.ts`） |
| プレーンテキスト | plain text | 通読用のテキスト（`export/text/`。`scn export text`、`plaintext`） |
| 原稿用紙換算 | manuscript sheets | 本文を20字×20行で数えた枚数 |
| 台本 | script sheet | 収録・校正用の表（`export/script.csv`、Excel 形式は `export/script.xlsx`。`scn export script`） |
| 台本の形式 | script format | `csv` か `xlsx`（`script::ScriptFormat`） |
| 収録の基準 | baseline | 収録に出した台本の、行IDごとの話者と台詞のハッシュ（`export/script-baseline.json`） |
| 再収録候補 | retake | 収録の基準の後に話者か台詞が変わった台詞（`script::Retakes`） |
| ボイスファイル名 | voice file name | `キャラID_行ID`（拡張子なし） |
| 直前の地の文 | preceding narration | 台本の列。同じシーンで、その台詞より前のいちばん近い地の文 |

## AI

| 日本語 | 英語名 | 説明 |
| --- | --- | --- |
| AIタスク | ai task | `ai/*.yaml` の1定義 |
| 関わり方の段階 | level | `review` / `propose` / `generate` / `auto` |
| 指摘・提案・生成・自律 | review / propose / generate / auto | 段階の名前 |
| 提案ノード | proposal node | フロー図上の未採用の仮ノード。AI の分岐案（`ai::BranchProposal`：足す選択肢1つと行き先、足すスタブ） |
| ルートの骨格 | route skeleton | エンディングとキャラの葛藤から AI が作る、スタブのシーンの並び（`ai::SkeletonScene`）。生成の段階で今のシーンを文脈に入れないタスク |
| 構造の検証の結果 | structure | 文脈の項目。コアが計算したルートの分量、エンディングへの到達、結果が同じになる選択、流れと伏線の診断 |
| 同期 | sync | シーン要約とメタ情報の summary のずれ（`ai::SummaryDrift`）を知らせ、summary に書く |
| 問い返し | questions | 企画書とメモの空白・矛盾を、作者への質問として返す（`concept_questions`） |
| 分類案 | idea suggestion | アイデアメモの状態とタグの案（`output: ideas`、`ai::IdeaSuggestion`） |
| AI下書き | ai draft | 状態が `ai-draft` のシーン |
| 出自 | provenance | `ai` / `ai-edited` / `human`。`ai/provenance.jsonl` の、行IDごとのいちばん後ろの記録（`provenance`） |
| 出自の記録 | provenance record | `ai/provenance.jsonl` の1行（`provenance::Record`）。書き足すだけで、消さない |
| 展開メモ | beats | シーンの流れのメモ。スタブと本文の間の段 |
| 書き込む範囲 | scope | `empty_only` など。AIが書いてよい範囲 |
| AI実行 | ai run | 1回のタスク実行 |
| 接続先 | connection | `project.toml` の `[ai.<名前>]`（`project::AiConnectionDecl`）と、端末の上書きを重ねて呼び出せる形にしたもの（`ai::Connection`） |
| 接続の種類 | provider | `openai-compatible`（OpenAI 互換）/ `anthropic`（Messages API） |
| 行き先 | endpoint | `<provider> <base_url>`。キーとクラウドへの送信の許可の単位 |
| 端末の設定 | machine settings | アプリの設定のフォルダの `ai.toml`（`ai::MachineSettings`）。URL とモデルの上書き、クラウドの許可 |
| 既定の接続先 | default connection | `[ai.local]`。無ければ最初のローカルの接続先 |
| キー | api key | OS のキーチェーンに置く（`ai::KeyStore`） |
| タスクの定義 | task definition | `ai/*.yaml` の1つ（`ai::AiTask`）。名前はファイル名 |
| 文脈 | context | LLM に渡す指示と依頼（`ai::Assembled`）。タスクの `context` の項目から組み立てる |
| 文脈の項目 | context item | `scene`・`characters`・`terms` など（`ai::ContextItem`） |
| 文脈の長さ | context tokens | 接続先の `context_tokens`。返す分を除いたものが文脈の上限（`ai::context_limit`） |
| 削る順番 | trim order | 文脈が上限に入らないときに節を削る順（`ai::TRIM_ORDER`） |
| 要約のキャッシュ | summary cache | `.cache/summaries.json`（`ai::SummaryCache`）。シーンの本文のハッシュと一緒に置く |
| 提案（行ごとの） | proposal | 行ID・今の文・案・理由（`ai::Proposal`）。アプリの `output: diff` と `ai/proposals/*.md` |
| 下書き（生成の） | draft | スタブのシーンに差し込む AI の本文（`ai::generated_body`・`Workspace::write_draft`）。シーンは `ai-draft` になる |
| 点検（自律の） | inspection | 自律で書き込む前に、`check` と指摘のタスクで下書きを確かめること |
| AIのブランチ | ai branch | 自律の大きな実行を書く Git のブランチ（`ai/<タスク>-<日時>`。`gitops`） |
| 採用 | adopt | 提案を人が選んで本文に当てる（`workspace::adopt_proposals`）。行IDは変えず、出自を `ai` と記録する |
| 指摘（AIの） | finding | AI のタスクが返した、行IDつきの指摘（`ai::Finding`）。アプリの中だけに持つ |
| 実行の結果 | run outcome | 1回の実行の指摘・要約・読めなかった行・トークン数（`ai::RunOutcome`） |
| AIに渡す内容 | entry context | 設定DBのエントリが、どのタスクにどの部分で渡るか（`ai::EntryContext`） |
