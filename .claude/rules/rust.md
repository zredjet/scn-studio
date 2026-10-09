---
paths:
  - "crates/**/*.rs"
  - "crates/**/Cargo.toml"
  - "app/src-tauri/**/*.rs"
---

# Rustの約束

## scn-core の構成

解析は段階に分け、各段階の出力を独立にテストできるようにする。

| モジュール | 役割 |
| --- | --- |
| `line` | 1行を分類する（空行、コメント、シーン見出し、ディレクティブ、選択肢、ジャンプ、台詞候補、地の文）。行IDの切り出しもここで行う |
| `syntax` | 行の並びからAST（シーン、ブロック、行）を組み立てる。ブロックの対応が崩れても回復して続ける |
| `inline` | 文中の記法（ルビ、傍点、インライン命令、変数の埋め込み）を解析する |
| `expr` | 条件式と代入式の字句解析と構文解析 |
| `sema` | 話者の解決、シーン参照、変数の型を、設定DBと `variables.yaml` を使って確かめる |
| `ir` | ASTからIR（シーンごとの命令列と、シーン間の辺）を作る。serdeでJSONにする |
| `diag` | 診断（コード、重大度、位置、メッセージ）。コードは仕様書の診断表と一致させる |
| `project` | プロジェクトフォルダの読み込み（`project.toml`、設定DB、変数、`story/` の下（下のフォルダも）の `.scn` 一式）。設定ファイルの誤りは診断にして続ける（E801〜W807）。設定ファイルの先頭の BOM は読み飛ばす（`without_bom`） |
| `index` | 索引（シーン、キャラクターの出現、変数の読み書き、素材の使用、行ID）。IRとプロジェクトの宣言から導く |
| `graph` | 構造の一覧（`scn graph`）。シーン・章・ルートの粒度で、辺と変数の読み書きをまとめる |
| `scaffold` | 作品フォルダのひな形（`scn new`）と、ツールが書く説明（`FORMAT.md`、`SCN.md`。`scn format-doc` が書き直す。`SCN.md` の元は `docs/guide/scn.md`）。設定DBの型のテンプレート集（`TYPE_TEMPLATES`、`templates/types/`。`scn new` が汎用のほかを置き、アプリの「＋ 型」の元にする。T4-9）。ひな形は `templates/` に置き、ビルドのときに埋め込む。型のテンプレートを足したら、読み込みの診断が出ないこととフォームで直せることのテスト（`type_templates_are_readable_and_editable`）を通す |
| `workspace` | 開いている作品フォルダ（アプリ）。保存していない本文とディスクの中身を持ち、保存（行IDを付ける）と、外部の変更の取り込みを決める。設定DBのエントリ・企画書・アイデアメモの書き換えと、メモの昇格（エントリを作る、章の末尾にスタブのシーンを足す）もここ |
| `merge` | 外部の変更とのマージ（アプリ）：元・自分・外の3方向。`.scn` は行IDのある行を行IDで、無い行を中身で対応させる。設定ファイルのテキストは中身だけで。ぶつかりは選ぶまで両方を残す（`.scn` は外の行をコメントに、テキストは `<<<<<<<` の印で）。行IDは書き換えない |
| `flowedit` | フロー図からの編集：引いた辺を、本文にジャンプか選択肢として書き足す中身を作る（人が書いた行は変えない。行き先の決まった終わり方のシーンには足さない）。書くのは `workspace` |
| `rename` | 名前の変更（F2）：章ごとの AST の位置から、シーンID・キャラクターの名前・変数・エントリのIDの現れを集めて置き換える中身と、あわせて書き換える設定ファイルの中身を作る（文字列の置き換えはしない。行ID・台詞の本文・地の文は変えない）。書くのは `workspace` |
| `layout` | フロー図の配置（`layout.json`）の読み書き。壊れていても止めず、読めた分を返す |
| `bible` | 設定DBと変数の画面（アプリ）：項目の型と値の検査（W805・W806）、フロントマターの書き換え（直したキーの値だけを差し替え、読み直して確かめる）、エントリの逆引き、変数の一覧 |
| `vardecl` | 変数の宣言のフォーム（アプリの変数の画面）：`variables.yaml` の1つの宣言の1つの項目の行を差し替える・足す・消す、宣言を足す。読み直して、ほかが変わっていないことを確かめる。書くのは `workspace` |
| `typedef` | 型定義のフォーム（アプリの設定DBの画面）：`bible/types/*.yaml` の1行の `- { … }` の項目を、`key` で指して直す・並べ替える・消す・足す。読み直して確かめる。型を作るときの元（`TypeSource`：空・テンプレート・複製）と、写すときに名前を替えて `builtin` を除く `retitle_type`（T4-9。写した中身を `project::parse_entry_type` で読み直して確かめるのは `workspace::create_type`）。書くのは `workspace` |
| `preview` | 簡易ADVプレビューのデータ（アプリ）：IR全体と、画面・ウィンドウの大きさ、括弧と表示スタイル、キャラクターの名前、変数の初期値、素材のファイル（`assets/` の中にあるものだけ） |
| `script` | 台本の出力（`scn export script`、アプリの「出力」）：IRから台詞ごとの行を作り、CSV（UTF-8 の BOM 付き、CRLF）か Excel 形式（`xlsx`）にする。収録の基準（`export/script-baseline.json`）と比べて、再収録候補を備考に書く |
| `xlsx` | 表を Excel 形式（.xlsx）にする：依存を足さない最小のブック（欄はすべてインライン文字列。シートは1枚か、宴への書き出しは複数 `xlsx_book`）と、圧縮しない ZIP |
| `assets` | 素材カタログ（アプリの素材の画面、`scn export orders`）：カタログに書かれたファイルの読み方（プレビューと書き出しと共通）、素材の一覧（状態、使用回数、初出、使用シーン）、発注リストの CSV、カタログの書き換え（意味名を足す、ファイルを割り当てる。行を足すか1行を差し替え、読み直して確かめる） |
| `ideas` | アイデアメモ（アプリの企画の画面）：`ideas/*.md` の読み方（状態、タグ、昇格先の解決）、新しいメモと、昇格で書く中身（スタブのシーン、伏線の標準の型）。書き換えは `workspace` |
| `concept` | 企画書の画面のデータ：`concept.md` のフロントマターの項目、主要キャラの欲求と葛藤（設定DBから）、企画書のエンディングと本文の `@エンディング` の対応 |
| `plaintext` | プレーンテキスト（`scn export text`、アプリの「出力」）：通読の本文を、小説の形の通読用のテキスト（ルビはなろう・カクヨム記法）にする |
| `route` | 通読（縦書きビュー、プレーンテキスト。道の候補と、道に沿った本文の並び）と、カーソル位置から再生（アプリの F5・「ここから」、プレビューの「最短経路で開始」）：行から始める命令を決め、開始シーンからの最短経路（選んだ選択肢の数がいちばん少ない道）と、その道での変数を求める。式の評価と命令の実行はプレビューのランタイム（TS）と同じ規則で、`app/test/route.test.ts` が突き合わせる。実行器（`Searcher::run`）は自動巡回（`playthrough`）と共通で、`Observer` で入ったシーン・評価した条件・エンディングを見られる。ただし `@呼出` の積み重ねは64段まで（`MAX_CALL_DEPTH`。プレビューのランタイムには上限が無いので、それより深い再帰の先は最短経路で「着けない」になる。D063） |
| `renpy` | Ren'Py への書き出し（`scn export renpy`、アプリの「出力」）：出力プロファイル（`export/renpy.yaml`）を読み、IRから `.rpy` と写す素材を作り、書き出し先の `game/scn/` を作り直す |
| `tyrano` | ティラノスクリプトへの書き出し（`scn export tyrano`、アプリの「出力」。T3-11）：出力プロファイル（`export/tyrano.yaml`）を読み、IRから `.ks` と写す素材と仮素材の SVG を作り、書き出し先の `data/scenario/scn/` と `data/*/scn/` を作り直す。流れはラベルとジャンプだけで書く（`[if]` の中にラベルを置かない）。プロファイルの読み方と素材の集め方は `renpy` のものを使う |
| `utage` | 宴への書き出し（`scn export utage`、アプリの「出力」。T3-12）：出力プロファイル（`export/utage.yaml`）を読み、IRからシナリオのシート（シーンごと）と設定のシート（Character・Texture・Sound・Param）と写す素材と仮素材の PNG を作り、書き出し先をまるごと作り直す（ブックは `xlsx::xlsx_book`）。流れはラベルとジャンプだけで書く |
| `book` | EPUB と縦書きPDF（`scn export epub`・`scn export pdf`、アプリの「出力」。T3-13）：通読（`route::read`）の本文を、EPUB 3（ZIP は `xlsx::Zip`）と、CSS 組版の道具に渡す印刷用の HTML にする。ルビ、傍点、縦中横。PDF はコアで組まず、`pdf_command` を `plugin::typeset` で実行する（T3-14） |
| `plugin` | 書き出しのプラグイン（`scn export <名前>`、アプリの「出力」。T3-14）：出力プロファイル `export/<名前>.yaml` の読み方と一覧、minijinja のテンプレート（渡すもの、フィルタ `plain`、テンプレートのフォルダの中だけを読むローダー）、外部コマンドの実行（`run_command`：シェルを通さない、作品フォルダで、標準入力、`{output}` などの置き換えと `SCN_*`、タイムアウトで止める、標準出力と標準エラーの後ろを取っておく）。テストは自分の実行ファイルを外部コマンドとして呼ぶ（`tests/plugin.rs` の `child_command`） |
| `navigate` | 定義へ移動・参照・ホバー（言語サーバー。T3-15）：本文の位置の名前（`rename::symbol_at`）から、定義の場所（シーンの見出し、設定DBのエントリの `name`、`variables.yaml` の宣言）、すべての現れ（`rename::occurrences`）、ホバーの文（Markdown） |
| `complete` | 補完の候補（言語サーバー。T3-15）：カーソルより前の行の文字から文脈を決める。決まりはアプリの `app/src/editor/complete.ts` と同じ（変えるときは両方を直す） |
| `assist` | 書いている最中の補助（アプリ）：文字数、今のシーンの情報（文脈パネル）、補完の材料。IR・索引・プロジェクトの宣言から導く |
| `check` | 検証の段階1（`scn check`）。行き止まり、到達性、はみ出し、変数の読み書き、企画書のエンディング、伏線。IRと索引から調べ、IRの診断には入れない |
| `playthrough` | 検証の段階2（自動巡回。`scn check --stage 2`、アプリの検証の画面）：開始シーンから選択肢の組み合わせを、`route` と同じ実行器（`Searcher::run` と `Observer`）で実行し、状態ごとにメモ化する。永続の変数は周回する。選べない選択肢、真にならない条件、選べる選択肢が無くなる選択、着かないシーン、仕込みを通らない伏線（W506〜W509・W704）と、カバレッジ、エンディングごとの到達条件、変数ごとの調べた道で出た値（`variables`。始めの値と `@設定` で入れた値を `Observer::assigned` で。流れに関わらない変数は、まとめた道の先の値を数えないので `flow` で分ける。T4-24）。上限に当たったら「無い」と言う診断は出さない |
| `wording` | 表記の検査（検証の段階2。T3-3）：本文の文字から、一人称の表記ゆれ（I204）、避ける書き方（`variants`。I205）、かなだけ違う未登録の用語（I206）。語の区切りを前後の文字で見て、語の一部には出さない。比べるときだけかなをそろえ、本文は変えない。`playthrough::stage2` が自動巡回とまとめる |
| `provenance` | 出自の記録（`ai/provenance.jsonl`。T3-4）：読む（いちばん後ろの記録が今の出自）、書き足す（今の中身の後ろに足すだけ）、保存の前後で直した AI の行（`ai-edited`）、AI の行の一覧と CSV（`scn provenance`、アプリの「出力」）。`scn fmt --provenance` も使う。企画書と設定DBの項目の記録（`file`・`key`、本文は `BODY_KEY`。T4-3）：今の出自（`latest_fields`）、人が直した AI の項目（`edited_ai_fields`。`workspace` の `set_entry_field`・`set_entry_body`・`write_entry_source` が書いた後に記録する）、IDの変更で写す（`moved_field_records`）、項目の一覧（`provenance_field_rows`） |
| `ai` | AIの接続（T3-5。`ai/` の下の `connection`・`client`・`keys`）：接続先の解決（`project.toml` の `[ai.<名前>]` に端末の設定 `ai.toml` を重ねる。ローカルかクラウドか、既定の接続先。タスクの接続先は `task_connection` で、端末の上書き → 定義の `model` → 既定の順。T4-1。文脈の長さと返す分の上限は `Connection::budget` の `TokenBudget` で `context::assemble` に渡す。返す分の上限は端末の上書き → `project.toml` の `output_tokens` → 既定（4分の1、多くて4096）。T4-10）、呼び出し（`complete`。OpenAI 互換と Messages API、SSE、タイムアウト、取り消し、トークン数。考えた中身は本文から除き（`strip_thinking`）、進み具合には `Received::Thinking` で渡す。タスクの実行は `ensure_answer` で答えの無い応答を誤りにする。構造化出力は接続先の `structured_output` と頼みの `schema`（`output_schema`）があるときだけ `response_format` を付ける。T4-2）、接続先に入っているモデルの一覧（`list_models`。解決は model の要らない `resolve_for_models`。ローカルの OpenAI 互換だけ、`<base_url>/models` の `data[].id`。プロキシを通さず、移り先へついて行かない。T4-23）、キーの保管（`KeyStore`。アプリは OS のキーチェーンの `SystemKeys`、テストは `MemoryKeys`）。同期の呼び出し（`reqwest` の `blocking`）なので、非同期の実行環境の中からは直接呼ばない（アプリは `spawn_blocking`）。テストは `tests/ai.rs` の模擬のサーバーで、実際の LLM を呼ばない。AIタスクの定義（`task`。`ai/*.yaml` を `project` が読む）と文脈の組み立て（`context`。節の並べ方、削る順番 `TRIM_ORDER`、シーンの要約のキャッシュ、設定DBのエントリが渡る内容）も `ai/` の下（T3-6）。文脈のスナップショットは `tests/ai_context.rs`。タスクの実行（`run`。T3-7）：指摘の段階だけを実行し、応答から指摘（崩れていても読めた分）とシーンの要約を読み、行IDで位置を引く（`tests/ai_run.rs`）。提案（`proposal`。T3-8）：`ai/proposals/*.md` とアプリの `output: diff` を `Proposal` にそろえ、今の本文と照らし（文字ごとの差分）、採用できる行だけを字下げと行IDを残して置き換える（`apply_proposals`。採用の保存と出自は `workspace::adopt_proposals`。`tests/ai_proposal.rs`） 。生成（`generate`。T3-9）：応答から本文の行を取り、スタブに差し込み（今ある行は変えない、`status: ai-draft`、差し込んだ行にだけ行ID。書くのは `workspace::write_draft`）、AI下書きを省いた IR（`without_ai_drafts`）。自律（`auto`）：章のスタブを展開メモ → 本文 → 点検 → 直す、を反復とトークン数の上限まで（`run_auto`。呼び出しと書き込みは `AutoHost`。`tests/ai_generate.rs`・`ai_auto.rs`）。企画と設定の段（`AutoStages`。T4-6）：前段の企画書・設定と章の後の設定を、組み込みのタスクで頼んで `AutoHost::write_concept`・`write_entries` で書き、段の後の検証を数える（`run_stage`）。企画書と設定DBの下書き（`fielddraft`。T4-4・T4-5）：応答（JSON の対応表）の読み方、空いている項目にだけ書く中身と値のある項目への案（`plan_concept_write`・`plan_entry_write`。共通は `plan_fields`）、新しいエントリの中身（`new_entry_text`）、足りない設定（`missing_entries`。参照切れは `bible::missing_refs` を W806 と共通に）と空いている項目（`empty_fields`）、文脈の節（`types_text`・`missing_text`）、対象のエントリ（`with_entry_target`）。書くのは `workspace::write_concept_draft`・`write_entries_draft`、案の採用は `adopt_field_proposal`（`tests/ai_concept.rs`・`ai_entries.rs`）。文体を必ず入れて削らないのは、本文を書くタスク（出力が `draft`）だけ（`context::writes_body`）。プロットとフローへの支援（`plot`。T3-10）：提案ノードと骨格の応答の読み方、構造の検証の結果の文脈（`structure_text`。コアの計算だけ）、要約のずれ（`summary_drift`）とメタ情報の1行の書き換え、メモの分類案。採用と骨格の書き込みは `workspace::adopt_branch`・`write_skeleton`（フロー図からの編集と同じ書き足しだけ。`tests/ai_plot.rs`） |
| `gitops` | Git の操作（T3-9）：`git` コマンドで、作業ツリーか、今のブランチ、コミットしていない変更、AI のブランチを作る・切り替える・コミットする（`commit_paths` は書いたファイルだけ）・差分・取り込む（ぶつかれば `merge --abort`）・捨てる（`tests/gitops.rs`。`git` が無ければ確かめない） |
| `search` | 作品全体の検索の材料（T4-22）：章ごとの本文の範囲（台詞の括弧の中と地の文。地の文として読む台詞の形の行（閉じ括弧の後ろに文が続く・話者を解決できない）は話者から行の終わりまで。仕様書6節、R222）と行IDの範囲（W303 も）。探すこと自体はアプリが行う |
| `line_id` | 行IDの付与（`scn fmt`、保存のとき）。I302 の出る行の末尾に ` ^ID` を足すだけで、ほかのバイトは変えない |

- 設定ファイルを書き換える関数（`bible`・`vardecl`・`typedef`・`assets` のカタログ）は、先頭の BOM を除いて書き換え、BOM を戻す。
- 構文解析は、設定DBに依存させない。台詞かどうかの最終判断は `sema` で行う（仕様書「台詞の判定」）。
- ASTの各ノードはスパン（UTF-8のバイト位置の範囲）を持つ。行番号と列は、必要になったときにスパンから計算する。

## エラーと診断

- `scn-core` のライブラリコードでは `unwrap`、`expect`、`panic!`、添字アクセスによるパニックを使わない。テストコードでは使ってよい。
- 回復できる問題は診断として集め、解析は続ける。回復できない入出力エラーだけを `Result` のエラーで返す（`thiserror` で型を定義する）。
- 新しい診断を追加するときは、仕様書の診断表にコードを足し、そのコードを出すfixtureを `crates/scn-core/tests/fixtures/` に置く。

## テスト

- スナップショットテストに `insta` を使う。入力は `samples/houkago/` と `crates/scn-core/tests/fixtures/*.scn`、出力はAST・IR・診断のJSONまたはデバッグ表示。
- スナップショットの更新は中身を読んで確認してから受け入れる。差分の理由を説明できない更新は受け入れない。
- 任意の入力でパニックしないことを、`proptest` などで確かめる（ランダムな行の組み合わせ、途中で切れた入力）。

## 性能

- 目安：1万行の `.scn` を、リリースビルドで100ミリ秒以内に解析する。T0-4でベンチマーク（`criterion`）を置き、結果を `docs/progress.md` に記録する。
- 打鍵ごとの再解析は、全体の再解析にする（D040、D042）。10万行の合成データで解析は約48ミリ秒で、時間の多くはIRのJSONの大きさ（約11MB）とフロントでの読み込みにかかる。差分更新は作らず、アプリへ渡すものを表示に要る分に絞る（T1-6）。

## IRのJSON

- フィールド名は `snake_case`、列挙は `{"kind": "say", ...}` のようなタグ付きで出す。
- IRには `ir_version` を持たせる。形が変わったら上げ、`docs/spec/scn-format-v0.md` のIRの節も更新する。

## Tauri側（app/src-tauri）

- コマンドは薄く保ち、処理は `scn-core` に置く。コマンドの入出力は、IRや診断の型をそのまま直列化する。
- ファイルの読み書きはコア側のコマンドだけで行い、フロントには任意のパスを読ませない。
- AI のキーはフロントに返さない（`ai_connections` はあるかどうかだけ）。キーとクラウドの許可は、今の作品フォルダの接続先の行き先にだけ書く（T3-5）。
- 自律の実行中（`AppState.auto`。T3-9）は、本文を書き換えるコマンドの初めで `refuse_during_auto` を呼んで断る（ブランチならどの章も、直接なら書く章だけ）。本文を書き換えるコマンドを足したら、ここにも足す。企画と設定の段を選んだ自律の間（T4-6）は、企画書と設定DBのエントリを書き換えるコマンドの初めで `refuse_settings_during_auto` を呼んで断る（アイデアメモのパスは断らない）。
- 言語サーバー（`crates/scn-lsp`。T3-15）も Tauri と同じく薄く保つ：LSP の形との受け渡しと、位置の変換（`position`。UTF-8 のバイト位置と LSP の UTF-16 の位置の変換はここだけ）。処理は `scn-core` に置く。
- OS のファイルの画面で開く（素材の画面の `asset_location`。T3-1、R191）のは、作品フォルダの `assets/` の中だけ（場所の決め方と外の判断はコアの `assets::asset_location`）。開くのは OS のコマンド（macOS は `open`、Windows は `explorer`）で、プラグインを足さない。素材のフォルダそのもの以外は選んだ状態で見せる（macOS で `.app` のような束を起動しない）。
- 作品フォルダのコマンド（書き出しのプラグインの `command`、`export/book.yaml` の `pdf_command`。T3-14）は、`command_approved` でこの端末で確かめたかを見て、確かめていなければ実行せずに `needs_approval` を返す（画面が人に確かめて `approve` で呼び直す）。確かめた記録はアプリの設定のフォルダの `export-commands.json` だけに書く。実行は `spawn_blocking` で、ワークスペースのロックを持ったまま待たない。
