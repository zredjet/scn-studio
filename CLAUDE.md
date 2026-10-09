# scn-studio（仮称）

分岐する物語（ADV・ビジュアルノベル）のための執筆ツール。Tauri製のデスクトップアプリで、対象OSはWindowsとmacOS（Linuxは対象外）。シナリオは独自のテキスト形式 `.scn` で書き、設定DB・構造グラフ・プレビュー・エクスポート・AI支援を、同じテキストから導いたデータの上に載せる。

現在は**フェーズ4（ローカルのLLMと、企画・設定の生成）を、人の確認を除いて終えたところ**（T4-7、D081。その後、人の追加の頼みで `scn` コマンドの説明（T4-8）と設定DBの既定の型とテンプレート集（T4-9）を足した。D082。返す分の上限の上書き（T4-10、D083）と、画面の見た目と操作感の点検（T4-11、D084。報告は `docs/ui/audit.md`）と、その直し（T4-12〜T4-14）と Claude Design への頼み（T4-15、`docs/ui/design-brief.md`。D085）も足した。届いた見本 v1 を取り込み、見た目の反映 T4-16〜T4-20 を済ませた（人の確認を除く）。D086。見本の新しい機能のうち人が選んだものと点検の直しを T4-21〜T4-26 で進めている。D087）。設計検討書の範囲はフェーズ3までで（人の確認を除いて完了。T3-17）、フェーズ4は人の相談と判断で足した（D080）。残りは `docs/review-for-human.md` の3節（Y1〜Y9 と、画面の点検の Z1〜Z5）で人の判断を待っている（人の判断が届くまで新しい作業を始めない）。計画は `docs/plan/phase4.md`、進捗は `docs/progress.md`。フェーズ3の人の確認は、届いたときに T3-1 で取り込む。フェーズ3（支援と拡張）の計画は `docs/plan/phase3.md`、設計検討書との突き合わせは `docs/plan/design-check.md`。フェーズ2（制作に使える）の計画は `docs/plan/phase2.md`、フェーズ1（執筆の核）の計画は `docs/plan/phase1.md`、フェーズ0（形式を固める）の計画と記録は `docs/plan/phase0.md` に残している。人の確認を待っていることは `docs/review-for-human.md` にまとめている。

## 作業を始めるとき

- まず `docs/progress.md` を読み、前回の続きから始める。
- 資料は作業に関係する節だけを読む。全部を一度に読み込まない。

| 知りたいこと | 資料 |
| --- | --- |
| `.scn` の文法と意味、診断コード | `docs/spec/scn-format-v0.md` |
| 作品フォルダの `.scn` 以外のファイル（`project.toml`、変数、素材、設定DB、企画書、メモ、出自の記録） | `docs/spec/project-files.md` |
| 日本語の用語とコード上の名前の対応 | `docs/glossary.md` |
| なぜそう決めたか | `docs/decisions.md` |
| 全体設計（データモデル、検証、AI、エクスポート） | `docs/design.md` |
| 画面の構成と見た目 | `docs/ui/README.md`、`docs/ui/tokens.md`、`docs/ui/decisions-v1.md`、`docs/ui/mockups/*.html` |
| `scn` コマンドの使い方（人と AI エージェント向けの説明。作品フォルダの `SCN.md` の元） | `docs/guide/scn.md` |
| 実データの例（パーサの主要なテスト入力） | `samples/houkago/` |

## 設計の原則

変えるときは人に確認する。理由も書いておくので、細部で迷ったらこの理由に沿って判断する。

1. **テキストが正**。フロー図や一覧はすべてテキストから導出し、アプリ独自のDBに真実を持たない。Git・外部エディタ・grep・AIエージェントがそのまま使えるようにするため。
2. **コアはRust、フロントは表示と入力だけ**。ファイルアクセス、解析、検証、AI通信はコアが受け持ち、フロントはTauriコマンド経由で呼ぶ。同じコアをCLIと将来の言語サーバーから使うため。
3. **演出は意味名で書く**（`@背景 屋上/夕方`）。ファイルパスやエンジン固有の命令は、エクスポート時のマッピングで解決する。複数エンジンへ出力するため。
4. **壊れたまま書ける**。パーサはエラー回復を行い、1行の誤りで残りの解析を止めない。未定義の参照や未作成の素材は警告にとどめる。執筆中のテキストは常に不完全だから。
5. **行IDは不変**。一度付いた行ID（`^k3f9`）は、編集・移動・再整形で変えない。ボイス、翻訳、コメント、AIの出自記録がすべて行IDに依存するため。
6. **AIは共作者**。空いている部分（スタブや未作成のルート）には生成を書き込んでよいが、人が書いた行は直接書き換えず、差分の提案として出す。

## リポジトリ構成

```
CLAUDE.md               この文書
docs/                   設計・仕様・計画・画面モック（.claude/rules/ に分野別の約束）
samples/houkago/        サンプル作品「放課後の灯」（.scn・設定・変数など一式）
samples/amayadori/      フェーズ1のゲートの短編「雨宿りの駅」（T1-13。ツールとClaude Codeで書いた。指示書は CLAUDE.template.md）
crates/scn-core/        パーサ、AST、意味解析、IR、診断
crates/scn-cli/         CLI `scn`
crates/scn-lsp/         言語サーバー（`scn lsp`。T3-15）
packages/lezer-scn/     ハイライト用のLezer文法
packages/vscode-scn/    VS Code拡張（T3-16。配布はしない）
app/                    Tauri＋React（src-tauri/ と src/）
```

まだ存在しないディレクトリは、計画書の該当タスクで作る。

## コマンド

ここには実際に動くコマンドだけを書く。コマンドを足したタスク（T0-3のスナップショット更新、T0-5のCLI、T0-7のアプリ起動など）で、この節に追記する。

- まとめて確認：`scripts/check-all`（macOS）、`powershell -NoProfile -ExecutionPolicy Bypass -File scripts\check-all.ps1`（Windows）。Rustの書式・clippy・テスト、pnpmの依存の導入、Biome、型検査、フロントのテストを順に走らせ、1つでも失敗したらそこで止まる
- Rust：`cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt --all`（確かめるだけなら `cargo fmt --all -- --check`）
- パーサだけ：`cargo test -p scn-core`
  - スナップショット（`crates/scn-core/tests/snapshots/`）が合わないと、テストが失敗し、新しい結果が `*.snap.new` に出る。中身を読んで確かめてから、環境変数 `INSTA_UPDATE=always` を付けて `cargo test -p scn-core` を実行すると受け入れる（`*.snap.new` は消してから）
  - AIタスクの文脈（`tests/ai_context.rs`）は、サンプルの各タスクの組み立てた文脈をスナップショット（`ai_context__*.snap`）で確かめる
  - AIの接続のテスト（`tests/ai.rs`）は、テストの中で立てる模擬のサーバー（OpenAI 互換と Messages API）で確かめ、実際の LLM もキーチェーンも使わない：`cargo test -p scn-core --test ai`。タスクの実行・提案・生成と自律・プロットとフローへの支援も同じく模擬のサーバーで（`--test ai_run`・`ai_proposal`・`ai_generate`・`ai_auto`・`ai_plot`）
  - パニックしないことの検査（proptest）を多く試す：`PROPTEST_CASES=20000 cargo test -p scn-core --release --test robustness`
  - 解析の速さを測る（1万行と10万行の合成データ、`criterion`。10万行のIRのJSONを `target/tmp/ir-100k.json` に書き出す）：`cargo bench -p scn-core --bench analyze`
- 開発用CLI `scn`（`crates/scn-cli`）：`cargo run -p scn-cli -- parse <file.scn>`（診断を `ファイル:行:列 コード メッセージ` で出す。`--json` でJSON）／`cargo run -p scn-cli -- ir <project-dir> [--compact]`（IRをJSONで出す。`--compact` は整形しない。例：`cargo run -p scn-cli -- ir samples/houkago`）
  - 終了コードは、成功が0、エラー（E）の診断があれば1、使い方の誤り・`project.toml` の無いフォルダ・フォルダを読めないなどの入出力の誤りは2。設定ファイルの誤り（読めないものを含む）はエラーの診断（E801〜E803）になるので1
  - `parse` は、上のディレクトリに `project.toml` があればプロジェクト全体で意味解析を行い、設定ファイルの診断と、そのファイルの診断を出す（ほかの章の診断は出さない）。無ければ構文解析だけを行う
  - `cargo run -p scn-cli -- fmt <project-dir> [--provenance <タスク> --model <モデル名>]`：行IDの無い行（I302）に行IDを付け、付けた行を `ファイル:行:列 ^ID` で出す（`--json` でJSON）。`--provenance` は付けた行IDを AI が書いた行として `ai/provenance.jsonl` に書き足す。`--check` は書き換えずに、付ける行があれば終了コード1。本文の診断では1にしない。`project.toml` の `format_version` か `[dialogue]` を読めなければ（`project.toml` そのものを読めないときを含む）、何もせずに終了コード2
  - `cargo run -p scn-cli -- check [project-dir] [--stage 2]`：プロジェクト全体を検証し（読み込み・構文解析・意味解析と、検証の段階1：行き止まり、到達できないシーン、はみ出し、変数の読み書き、企画書のエンディング、伏線）、診断を `ファイル:行:列 コード メッセージ ^行ID` で出す（`--json` でJSON）。フォルダを省くと今のフォルダ。`--stage 2` は検証の段階2（自動巡回：選べない選択肢、真にならない条件、着かないシーン、仕込みを通らない伏線。表記の検査：一人称と用語の表記ゆれ、かなだけ違う未登録の用語）も行い、カバレッジとエンディングごとの到達条件と、変数ごとの調べた道で出た値を標準エラーに出す（`--json` では `{"diagnostics": …, "playthrough": …}`）。終了コードは `parse` と同じ
  - `cargo run -p scn-cli -- provenance [project-dir] [--json] [-o 書き出し先]`：AI が書いた行（出自の記録で `ai`・`ai-edited` の行）と、企画書と設定DBの AI の項目（T4-3）の一覧と数を出す。`-o` で CSV（UTF-8 の BOM 付き、CRLF）に書く。終了コードは0（作品フォルダでなければ2）
  - `cargo run -p scn-cli -- ai context <タスク> [project-dir] [--scene ID] [--route 名前] [--tokens N] [--output-tokens N]`：AIタスクの定義（`ai/<タスク>.yaml`）から組み立てた文脈（LLM に渡す指示と依頼）を出す（`--route` はルートの骨格の対象のルート）。LLM は呼ばない。入れた節・削った節・知らせは標準エラーに出す（`--json` でJSON）。`--tokens` は文脈の長さ（省くとタスクの接続先の `context_tokens`）、`--output-tokens` は返す分の上限（省くと接続先の `output_tokens`、無ければ文脈の長さの4分の1、多くて4096。T4-10）。実行できない（シーンが要る、入りきらない、返す分の上限が文脈の長さ以上）ときは終了コード1、無いタスクは2
  - `cargo run -p scn-cli -- graph [project-dir] [--level scene|chapter|route]`：シーン、辺、条件、変数の読み書きを、粒度ごとに一覧で出す（`--json` でJSON）。辺の文脈は、いちばん内側のもの（選択肢、条件、`@else` の枝）だけ。診断は出さず、終了コードは0（作品フォルダでなければ2）
  - `cargo run -p scn-cli -- new <dir> [--title 作品名]`：新しい作品フォルダを作る（無いフォルダか空のフォルダに。`project.toml`、スタブの章、`CLAUDE.md`、`FORMAT.md`、`SCN.md`、設定DBの13の型など）。ひな形は `crates/scn-core/templates/`（設定DBの型のテンプレートは `templates/types/`。T4-9）
  - `cargo run -p scn-cli -- format-doc [project-dir]`：作品フォルダの `FORMAT.md`（仕様書の1〜16節と作品フォルダの規約）と `SCN.md`（`docs/guide/scn.md` のコマンドの使い方）を、今のツールの版で書き直す。`--check` は書き直さずに、どちらかが古ければ終了コード1
  - コマンドやオプションを足したら、`docs/guide/scn.md` にも書く（`cargo test -p scn-cli` が、すべてのコマンドと長いオプションが説明にあることを確かめる）
  - `cargo run -p scn-cli -- export script [project-dir] [--format csv|xlsx] [--baseline] [--no-ai-drafts] [-o 書き出し先]`（`--no-ai-drafts` は AI下書きのシーンの台詞と地の文を省く。`export text`・`export renpy`・`export tyrano`・`export utage` にもある）：台本（行ID、シーン、キャラ、台詞、直前の地の文、ボイスファイル名、備考）を CSV（UTF-8 の BOM 付き、CRLF）か Excel 形式（`--format xlsx`。欄はすべて文字列）で書き出す。既定の書き出し先は作品フォルダの `export/script.csv`（Excel 形式は `export/script.xlsx`）、`-o -` で標準出力。`--baseline` で書き出した台本を収録の基準（`export/script-baseline.json`）として記録し、基準があれば、変わった台詞の備考に「再収録候補」と書いて数を標準エラーに出す。行IDの無い台詞の数も標準エラーに出す。終了コードは `ir` と同じ（エラーの診断があれば1）
  - `cargo run -p scn-cli -- export orders [project-dir] [-o 書き出し先]`：発注リスト（まだファイルの無い素材の表。種別、名前、キャラ、差分、使用回数、初出、使用シーン、備考）を CSV（UTF-8 の BOM 付き、CRLF）で書き出す。既定の書き出し先は作品フォルダの `export/orders.csv`、`-o -` で標準出力。終了コードは `export script` と同じ
  - `cargo run -p scn-cli -- export text [project-dir] [--route 名前 | --ending ID] [--no-notes] [-o 書き出し先]`：道（共通・ルート・エンディング）を一本道にした通読用のテキスト（話者名なし、ルビは `｜親文字《読み》`）を書き出す。`--route` も `--ending` も無ければ、共通とすべてのルートを作品フォルダの `export/text/<道の名前>.txt` に書く。`-o -` で標準出力（1つの道のときだけ）。ルートの無い作品は「通し」として書く。まとめて書くとき、着けない道は標準エラーに知らせて書かない。終了コードは `export script` と同じで、加えて `--route`・`--ending` で指定した道が無い・着けないときは2
  - `cargo run -p scn-cli -- export renpy [project-dir] [-o 書き出し先]`：出力プロファイル（`export/renpy.yaml`）に従って Ren'Py のスクリプトを書き出す。既定の書き出し先はプロファイルの `output`（既定は作品フォルダの `export/renpy`）で、その `game/scn/` だけを作り直す。知らせとプロファイルの誤りは標準エラーに出す。終了コードは `export script` と同じ（`game/scn/` が書き出したものでなければ消さずに2）
  - `cargo run -p scn-cli -- export tyrano [project-dir] [--no-ai-drafts] [-o 書き出し先]`：出力プロファイル（`export/tyrano.yaml`）に従ってティラノスクリプトのシナリオを書き出す。既定の書き出し先はプロファイルの `output`（既定は作品フォルダの `export/tyrano`）で、その `data/scenario/scn/` と素材の `data/{bgimage,fgimage,bgm,sound}/scn/` だけを作り直す（`first: true` なら `data/scenario/first.ks` も）。終了コードは `export renpy` と同じ（作り直すフォルダが書き出したものでなければ消さずに2）
  - `cargo run -p scn-cli -- export utage [project-dir] [--no-ai-drafts] [-o 書き出し先]`：出力プロファイル（`export/utage.yaml`）に従って、宴のシナリオ（`<book>.xlsx`。開始のシート `scn_start` とシーンごとのシート）と設定（`<book>_settings.xlsx`。Character・Texture・Sound・Param・Layer）と素材（`Texture/`・`Sound/`）を書き出す。既定の書き出し先はプロファイルの `output`（既定は作品フォルダの `export/utage`）で、まるごと作り直す。終了コードは `export renpy` と同じ（書き出し先が書き出したものでなく空でもなければ、消さずに2）
  - `cargo run -p scn-cli -- export epub|pdf [project-dir] [--route 名前 | --ending ID] [--horizontal] [--no-notes] [--no-ai-drafts] [-o 書き出し先]`：道（`export text` と同じ選び方）ごとに、EPUB 3（`export/epub/<道の名前>.epub`）か、縦書きPDFを組むための印刷用の HTML（`export/pdf/<道の名前>.html`。PDF は Vivliostyle などで組む）を書き出す。出力プロファイルは `export/book.yaml`。`-o` は1つの道のときだけ（`-o -` は標準出力）。`pdf --typeset` は、印刷用の HTML を書いた後に `export/book.yaml` の `pdf_command` で PDF を組む（無ければ2）。終了コードは `export text` と同じ
  - `cargo run -p scn-cli -- export <プロファイル名> [project-dir] [-o 書き出し先] [--no-ai-drafts]`：作品フォルダの出力プロファイル `export/<名前>.yaml` のプラグインで書き出す（`template:` は minijinja のテンプレートで1つのファイルを、`command:` は IR の JSON を標準入力で渡して外部コマンドを作品フォルダで実行する。仕様は `docs/spec/project-files.md` の15節）。既定の書き出し先は `export/<名前>/` の中。無い名前・プロファイルとテンプレートの誤り・コマンドの失敗は2
  - `cargo run -p scn-cli -- lsp`：言語サーバー（LSP）を標準入出力で動かす（VS Code 拡張などのエディタから使う。診断（読み込み・構文・意味と検証の段階1）、補完、定義へ移動、参照、名前の変更、ホバー。作品フォルダは初めに渡されたフォルダか、開いた文書から上へ探した `project.toml` のあるフォルダ。位置は UTF-16）
  - 言語サーバーのテストだけ：`cargo test -p scn-lsp`（テストの中で JSON-RPC の要求をサービスに渡す）
  - CLIのテストだけ：`cargo test -p scn-cli`
  - `scn` をパスの通った場所（`~/.cargo/bin`）に入れる：`cargo install --locked --path crates/scn-cli`（作品フォルダで `scn` として使うとき）
- Ren'Py で確かめる（macOS、Ren'Py の SDK が要る。`scripts/check-all` には入らない）：`scripts/renpy-check [SDKのフォルダ]`（既定は環境変数 `RENPY_SDK` か `/Applications/renpy-8.5.2-sdk`）。両サンプルを `target/renpy-check/` に書き出し、SDK の lint と、Ren'Py の testcase（`scripts/renpy/*.rpy`）で全エンディングまで自動で遊ぶ。ゲームの窓が短い間開く
- フロント：`pnpm install` / `pnpm lint`（Biomeの書式と静的検査）/ `pnpm fmt`（Biomeで整形と安全な修正）/ `pnpm typecheck` / `pnpm test`
  - Biomeはルートで全体を一度に検査する（`docs/` と `samples/` は対象外）。各パッケージには `lint` を置かず、`typecheck` と `test` のスクリプトを置く。ルートの `pnpm typecheck` と `pnpm test` が、それを持つパッケージをすべて走らせる
- Lezer文法（`packages/lezer-scn`）：`pnpm -C packages/lezer-scn test`（文法から構文解析器を作り直してから、`node --test` でテスト）／`pnpm -C packages/lezer-scn build`（構文解析器を作るだけ）
  - 作った `src/parser.ts`・`src/parser.terms.ts` はコミットしない（`typecheck` と `test` が先に作る）
  - サンプルの行の種類の期待値（`test/expected/*.txt`）を作り直す：`UPDATE_EXPECTED=1 pnpm -C packages/lezer-scn test`。差分を読んで確かめてから受け入れる（VS Code拡張の文法のテストも、この期待値と突き合わせる）
- VS Code拡張（`packages/vscode-scn`）：`pnpm -C packages/vscode-scn typecheck`（型検査と、esbuild で `out/extension.cjs` にまとめる）／`pnpm -C packages/vscode-scn test`（TextMate の文法（サンプルの全行の行の種類が Lezer の期待値と同じ、文中の記法と式の scope）と行IDの見つけ方のテスト）。どちらも `scripts/check-all` に入っている
  - 拡張の起動のテスト（`scripts/check-all` には入らない。VS Code の窓が短い間開く）：`cargo build -p scn-cli && pnpm -C packages/vscode-scn test:vscode`（VS Code は環境変数 `VSCODE_EXECUTABLE`、無ければ macOS の `/Applications/Visual Studio Code.app`、それも無ければダウンロード。`scn` は `SCN_BIN` か `target/debug/scn`）
  - パッケージにして入れる手順は `packages/vscode-scn/README.md`（`npx @vscode/vsce package --no-dependencies …`）
- アプリ（`app/`、Tauri＋React）：`pnpm -C app tauri dev`（開発用に起動する。デバッグビルドでは起動時に `samples/houkago` を開く）／`pnpm -C app test`（位置の変換、グラフの配置、エディタ、置き換え、執筆表示、行IDを守る扱い、入力補助、検証の画面、設定DBの画面、変数と型定義のフォーム、素材カタログの画面、企画の画面、マージの画面、名前の変更とパレットなどエディタの残り、環境設定の AI の接続先の表示、AI実行の画面、プレビューのランタイムと縦書きの組み方のテスト。プレビューと「ここから再生」のテストはコアのスナップショット（`crates/scn-core/tests/snapshots/preview__*.snap`・`route__*.snap`）を読むので、`cargo test -p scn-core` の後に走らせる）／`pnpm -C app build`（フロントのビルド）
  - 開発用の環境変数：`SCN_DEV_PROJECT=<作品フォルダ>`（起動時に開く）、`SCN_MEASURE_IPC=1`（開いた後に、入力が止まったときの受け渡しの時間を10回測ってターミナルに書く）。解析の速さを見るときは、Rustをリリースビルドにする `pnpm -C app tauri dev --release` で測る
  - Rust側（`app/src-tauri`、パッケージ名 `scn-app`）はCargoのワークスペースに入っているので、`cargo test --workspace`・`cargo clippy --workspace` の対象になる
  - フロントの `dev`・`build`・`typecheck` は、先に `packages/lezer-scn` の文法から構文解析器を作る
- 版の固定：Rustは `rust-toolchain.toml`、pnpmとBiomeは `package.json`（`packageManager` と `devDependencies`）

## 作業の進め方

- 1セッションで1タスク（例：T0-3）を扱う。タスクの中身と完了条件は計画書にある。`/phase-task T0-3` で一連の手順を呼び出せる。
- 複数のファイルにまたがる変更や、設計判断を含む変更は、先に計画を立ててから実装する。一文で説明できる小さな修正は、そのまま進めてよい。
- 検証できる形で進める。パーサの変更は、先にfixtureとテストを書いてから実装する。
- 完了を報告するときは根拠を添える。実行したコマンド、テスト件数、警告の有無を書く。実行していない確認を「確認済み」と書かない。
- タスクを閉じる前に、`plan-reviewer` サブエージェントで差分を計画と仕様に照らす。指摘のうち、正しさと完了条件に関わるものだけを直す。
- タスクの区切りで `docs/progress.md` を更新し、コミットする。
- 下の「止まって確認する場面」に当たらない限り、人の返事を待たずに最後まで進めてよい。

## 止まって確認する場面

当たったら作業を止め、選択肢と推奨案を示して人に聞く（AskUserQuestionが使える）。

- `.scn` の文法や意味を変える、または仕様書に書かれていない解釈を決める
- 設計の原則、または `docs/decisions.md` の決定と食い違う
- 大きな依存を加える（UIコンポーネント集、状態管理、パーサ生成器など）。小さく定番のcrateやパッケージは、理由を `docs/progress.md` に書けば加えてよい
- テストを通すために `samples/` の作品データを書き換えたくなった
- 現在のフェーズより先の機能が必要に見える
- 人の環境でしか確かめられない（IMEの挙動、実機での見た目など）

決めたことは `docs/decisions.md` に追記する（番号、決定、理由、影響）。

## コードの約束

- 識別子は英語で、`docs/glossary.md` の対応に合わせる。コメントとドキュメントは日本語でよい。
- テキストはUTF-8。全角空白（U+3000）は本文の一部として扱う。行頭の半角空白とタブは、整形用として無視する。文字列を黙って正規化（NFC化、記号の置き換え）しない。
- 位置情報は、コアではUTF-8のバイト位置で持つ。CodeMirrorはUTF-16の位置を使うので、変換はフロントとの境界の1か所だけで行う。
- 言語ごとの細かい約束は `.claude/rules/` にあり、該当ファイルを開くと読み込まれる。

## 範囲の規律

- 現在のフェーズの範囲だけを作る。計画書の「やらないこと」に挙げたものは作らない。
- 将来のための抽象化、設定項目、拡張点を先回りして作らない。必要になったフェーズで作る。
- 1つの変更に、関係のない整形やリネームを混ぜない。

## コミットと報告

- コミットメッセージは日本語で、先頭に種別とタスクIDを付ける。例：`feat(T0-3): 選択肢ブロックの解析を追加`
- 人への報告と質問は日本語で行う。
- 会話を要約（コンパクト）するときは、作業中のタスクID、変更したファイル、未解決の問題、検証に使うコマンドを必ず残す。
