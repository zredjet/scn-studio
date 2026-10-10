# 開発への参加

scn-studio に興味を持ってくれてありがとう。不具合の報告、提案、プルリクエストを歓迎する。やりとりは日本語で行う（英語でもよい）。

## Issue

- 不具合は、OS（macOS か Windows と版）、アプリか `scn` か、再現する手順、期待した結果と実際の結果を書く。作品のテキストが要るときは、問題の出る最小の `.scn` を添える（`samples/houkago` で再現できればなおよい）。
- 機能の提案は、困っている場面と、ほしい動きを書く。設計の原則（下）に関わるものは、先に Issue で相談してほしい。

## 開発の環境

[README の「ソースから動かす」](README.md#ソースから動かす) の道具を入れてから、次を実行する。

```bash
pnpm install
```

```bash
scripts/check-all
```

Windows では次を使う。

```bash
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\check-all.ps1
```

`check-all` は、Rust の書式（`cargo fmt`）・静的検査（`cargo clippy`）・テストと、フロントの Biome・型検査・テストを順に走らせ、1つでも失敗したら止まる。プルリクエストでは GitHub Actions が macOS と Windows で同じものを走らせる。

よく使うコマンド：

| コマンド | 内容 |
| --- | --- |
| `pnpm -C app tauri dev` | アプリを開発用に起動する（`samples/houkago` を開く） |
| `cargo test -p scn-core` | コアのテストだけ（スナップショットは `insta`） |
| `pnpm -C app test` | フロントのテストだけ（`cargo test -p scn-core` の後に走らせる） |
| `cargo run -p scn-cli -- check samples/houkago` | 入れずに `scn` を試す |
| `pnpm fmt` / `cargo fmt --all` | 書式を整える |

コマンドの一覧と細かい約束は [`CLAUDE.md`](CLAUDE.md) の「コマンド」にある。

## リポジトリの構成

| パス | 内容 |
| --- | --- |
| `crates/scn-core/` | コア（Rust）：パーサ、AST、意味解析、IR、診断、検証、書き出し、AI の接続。ファイルの読み書きと解析はすべてここ |
| `crates/scn-cli/` | コマンド `scn` |
| `crates/scn-lsp/` | 言語サーバー（`scn lsp`） |
| `app/` | デスクトップアプリ（Tauri の `src-tauri/` と、React の `src/`）。フロントは表示と入力だけで、コアを Tauri のコマンドで呼ぶ |
| `packages/lezer-scn/` | ハイライト用の Lezer 文法（CodeMirror 6） |
| `packages/vscode-scn/` | VS Code 拡張 |
| `samples/` | サンプル作品（パーサとアプリのテストの入力） |
| `docs/` | 仕様、設計、決定の記録、計画、画面の見本 |
| `scripts/` | まとめての検査、Ren'Py での確認 |

## 設計の原則

変える前に Issue で相談してほしい。細部で迷ったら、この理由に沿って決める。

1. **テキストが正**。フロー図や一覧はすべてテキストから導出し、アプリ独自のDBに真実を持たない。
2. **コアは Rust、フロントは表示と入力だけ**。同じコアを CLI と言語サーバーから使うため。
3. **演出は意味名で書く**（`@背景 屋上/夕方`）。ファイルパスやエンジン固有の命令は、書き出しのときに決める。
4. **壊れたまま書ける**。1行の誤りで残りの解析を止めない。未定義の参照や未作成の素材は警告にとどめる。
5. **行IDは不変**。一度付いた行IDは、編集・移動・整形で変えない。
6. **AI は共作者**。人が書いた行は直接書き換えず、差分の提案として出す。

## 変更の作り方

- 1つのプルリクエストでは1つのことを変える。関係の無い整形や名前の変更を混ぜない。
- `.scn` の文法や意味を変えるときは、[`docs/spec/scn-format-v0.md`](docs/spec/scn-format-v0.md) を先に直す（仕様書が正で、パーサは仕様書に従う）。新しい診断コードには、仕様書の表と、それを出す fixture（`crates/scn-core/tests/fixtures/`）を足す。
- パーサの変更は、先に fixture とテストを書いてから実装する。スナップショットが変わったら、中身を読んで理由を説明できるものだけを受け入れる。
- テストを通すためにサンプル作品（`samples/`）を書き換えない。サンプルには、わざと残している警告がある（[`samples/houkago/README.md`](samples/houkago/README.md)）。
- 日本語入力の変換中に動く処理（補完、整形、保存、本文の置き換え）は、変換が終わってから行う。
- 識別子は英語で、[`docs/glossary.md`](docs/glossary.md) の対応に合わせる。コメントとドキュメントは日本語でよい。
- 分野ごとの細かい約束は `.claude/rules/`（Rust、フロントエンド、仕様書とサンプル）にある。
- 決めたことは [`docs/decisions.md`](docs/decisions.md) に、番号・決定・理由・影響を書き足す。

## コミット

コミットメッセージは日本語で、先頭に種別を付ける（`feat`・`fix`・`docs`・`test`・`refactor`・`ci` など）。計画書のタスクに当たるものはタスクIDも付ける。

```text
feat(T4-30): 画面の倍率を環境設定で変えられるようにする
fix: 縦書きの通読で、ルビの後ろの改行が消える
```

## リリースの作り方

1. 版を上げる：`Cargo.toml` の `[workspace.package]`、`app/src-tauri/tauri.conf.json`、`app/package.json`・`packages/lezer-scn/package.json`・`packages/vscode-scn/package.json` の `version` をそろえる（ずれていると、リリースの CI が止まる）。
2. `scripts/check-all` を通して（`Cargo.lock` の版も書き換わる）、`Cargo.lock` と一緒にコミットし、`main` に push する（リリースの CI は `--locked` で組むので、`Cargo.lock` が古いと止まる）。
3. タグを付けて push する。

```bash
git tag v0.1.0
```

```bash
git push origin v0.1.0
```

4. GitHub Actions（`.github/workflows/release.yml`）が、macOS（Apple Silicon）と Windows（x64・ARM64）のアプリとコマンド `scn`、VS Code 拡張を組み、ハッシュ（`SHA256SUMS.txt`）と説明（`.github/release-notes.md`）を付けて、**下書き**のリリースを作る。
5. リリースの頁で中身を確かめてから「Publish release」を押す。

インストーラーと署名は無い（macOS のアプリは ad-hoc の署名だけ）。組むところだけを試すときは、Actions の「Release」を手動で動かす（リリースは作らず、組んだものを成果物に置く）。

## Claude Code での開発

このリポジトリは Claude Code で開発している。[`CLAUDE.md`](CLAUDE.md) は Claude Code が毎回読む指示書で、人が読んでもこのプロジェクトの約束の要約になっている。

- `.claude/rules/`：分野ごとの約束（該当のファイルを開いたときに読み込まれる）
- `.claude/skills/phase-task/`：`/phase-task <タスクID>` で、計画書のタスクを調査から記録・コミットまで進める手順
- `.claude/agents/plan-reviewer.md`：差分を計画書と仕様に照らすレビュー用のサブエージェント
- [`docs/progress.md`](docs/progress.md)：進捗の記録。[`docs/review-for-human.md`](docs/review-for-human.md)：人の判断と確認を待っていること

## ライセンス

寄稿したものは、このリポジトリと同じ [MIT ライセンス](LICENSE) で配布される。
