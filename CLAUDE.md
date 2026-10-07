# scn-studio（仮称）

分岐する物語（ADV・ビジュアルノベル）のための執筆ツール。Tauri製のデスクトップアプリで、対象OSはWindowsとmacOS（Linuxは対象外）。シナリオは独自のテキスト形式 `.scn` で書き、設定DB・構造グラフ・プレビュー・エクスポート・AI支援を、同じテキストから導いたデータの上に載せる。

現在は**フェーズ1（執筆の核）**。計画は `docs/plan/phase1.md`、進捗は `docs/progress.md`。フェーズ0（形式を固める）の計画と記録は `docs/plan/phase0.md` に残している。

## 作業を始めるとき

- まず `docs/progress.md` を読み、前回の続きから始める。
- 資料は作業に関係する節だけを読む。全部を一度に読み込まない。

| 知りたいこと | 資料 |
| --- | --- |
| `.scn` の文法と意味、診断コード | `docs/spec/scn-format-v0.md` |
| 日本語の用語とコード上の名前の対応 | `docs/glossary.md` |
| なぜそう決めたか | `docs/decisions.md` |
| 全体設計（データモデル、検証、AI、エクスポート） | `docs/design.md` |
| 画面の構成と見た目 | `docs/ui/README.md`、`docs/ui/mockups/*.html` |
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
crates/scn-core/        パーサ、AST、意味解析、IR、診断
crates/scn-cli/         CLI `scn`
packages/lezer-scn/     ハイライト用のLezer文法
app/                    Tauri＋React（src-tauri/ と src/）
```

まだ存在しないディレクトリは、計画書の該当タスクで作る。

## コマンド

ここには実際に動くコマンドだけを書く。コマンドを足したタスク（T0-3のスナップショット更新、T0-5のCLI、T0-7のアプリ起動など）で、この節に追記する。

- まとめて確認：`scripts/check-all`（macOS）、`powershell -NoProfile -ExecutionPolicy Bypass -File scripts\check-all.ps1`（Windows）。Rustの書式・clippy・テスト、pnpmの依存の導入、Biome、型検査、フロントのテストを順に走らせ、1つでも失敗したらそこで止まる
- Rust：`cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt --all`（確かめるだけなら `cargo fmt --all -- --check`）
- パーサだけ：`cargo test -p scn-core`
  - スナップショット（`crates/scn-core/tests/snapshots/`）が合わないと、テストが失敗し、新しい結果が `*.snap.new` に出る。中身を読んで確かめてから、環境変数 `INSTA_UPDATE=always` を付けて `cargo test -p scn-core` を実行すると受け入れる（`*.snap.new` は消してから）
  - パニックしないことの検査（proptest）を多く試す：`PROPTEST_CASES=20000 cargo test -p scn-core --release --test robustness`
  - 解析の速さを測る（1万行と10万行の合成データ、`criterion`。10万行のIRのJSONを `target/tmp/ir-100k.json` に書き出す）：`cargo bench -p scn-core --bench analyze`
- 開発用CLI `scn`（`crates/scn-cli`）：`cargo run -p scn-cli -- parse <file.scn>`（診断を `ファイル:行:列 コード メッセージ` で出す。`--json` でJSON）／`cargo run -p scn-cli -- ir <project-dir>`（IRをJSONで出す。例：`cargo run -p scn-cli -- ir samples/houkago`）
  - 終了コードは、成功が0、エラー（E）の診断があれば1、使い方の誤り・`project.toml` の無いフォルダ・フォルダを読めないなどの入出力の誤りは2。設定ファイルの誤り（読めないものを含む）はエラーの診断（E801〜E803）になるので1
  - `parse` は、上のディレクトリに `project.toml` があればプロジェクト全体で意味解析を行い、設定ファイルの診断と、そのファイルの診断を出す（ほかの章の診断は出さない）。無ければ構文解析だけを行う
  - `cargo run -p scn-cli -- fmt <project-dir>`：行IDの無い行（I302）に行IDを付け、付けた行を `ファイル:行:列 ^ID` で出す（`--json` でJSON）。`--check` は書き換えずに、付ける行があれば終了コード1。本文の診断では1にしない。`project.toml` にエラーがあれば、何もせずに終了コード2
  - CLIのテストだけ：`cargo test -p scn-cli`
  - `scn` をパスの通った場所（`~/.cargo/bin`）に入れる：`cargo install --path crates/scn-cli`（作品フォルダで `scn` として使うとき）
- フロント：`pnpm install` / `pnpm lint`（Biomeの書式と静的検査）/ `pnpm fmt`（Biomeで整形と安全な修正）/ `pnpm typecheck` / `pnpm test`
  - Biomeはルートで全体を一度に検査する（`docs/` と `samples/` は対象外）。各パッケージには `lint` を置かず、`typecheck` と `test` のスクリプトを置く。ルートの `pnpm typecheck` と `pnpm test` が、それを持つパッケージをすべて走らせる
- Lezer文法（`packages/lezer-scn`）：`pnpm -C packages/lezer-scn test`（文法から構文解析器を作り直してから、`node --test` でテスト）／`pnpm -C packages/lezer-scn build`（構文解析器を作るだけ）
  - 作った `src/parser.ts`・`src/parser.terms.ts` はコミットしない（`typecheck` と `test` が先に作る）
  - サンプルの行の種類の期待値（`test/expected/*.txt`）を作り直す：`UPDATE_EXPECTED=1 pnpm -C packages/lezer-scn test`。差分を読んで確かめてから受け入れる
- アプリ（`app/`、Tauri＋React）：`pnpm -C app tauri dev`（開発用に起動する。デバッグビルドでは起動時に `samples/houkago` を開く）／`pnpm -C app test`（位置の変換とグラフの配置のテスト）／`pnpm -C app build`（フロントのビルド）
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
