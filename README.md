# scn-studio（仮称）

分岐する物語（ADV・ビジュアルノベル）のための執筆ツール。Tauri製のデスクトップアプリで、WindowsとmacOSを対象にする。

このフォルダは、Claude Code（Opus 5.5）で開発を始めるためのリポジトリのひな形。コードはまだ無く、設計・仕様・計画・画面モック・サンプル作品が入っている。

## 始め方

1. RustとNode.jsとpnpm、TauriのOS別の前提ソフトを入れる（各公式の最新の安定版）。
2. このフォルダで `git init` し、最初のコミットを作る。
3. `claude` を起動し、`/model` でOpus 5.5を選ぶ。
4. `/phase-task T0-1` と入力する。以降は `docs/plan/phase0.md` の順にタスクを進める。

詳しい進め方（1タスク1セッション、確認の入れ方、人の作業）は `docs/plan/phase0.md` にある。

## 中身

| パス | 内容 |
| --- | --- |
| `CLAUDE.md` | Claude Codeが毎回読む指示書。目的、原則、作業の進め方、止まって確認する場面 |
| `.claude/rules/` | 分野別の約束（Rust、フロントエンド、仕様書とサンプル）。該当ファイルを開いたときに読み込まれる |
| `.claude/skills/phase-task/` | `/phase-task <タスクID>` で、調査から記録・コミットまでを一通り進める手順 |
| `.claude/agents/plan-reviewer.md` | 差分を計画と仕様に照らすレビュー用のサブエージェント |
| `docs/design.md` | 設計検討書（全体設計） |
| `docs/spec/scn-format-v0.md` | `.scn` 形式の仕様（ドラフト。T0-2で確定） |
| `docs/glossary.md` | 用語集（日本語とコード上の名前の対応） |
| `docs/decisions.md` | 決定記録 |
| `docs/plan/phase0.md` | フェーズ0の作業手順 |
| `docs/plan/ime-test.md` | 日本語入力のテスト表 |
| `docs/progress.md` | 進捗ログ（Claude Codeが更新する） |
| `docs/ui/` | 画面の構成、デザイントークン、画面モック（`mockups/index.html`） |
| `samples/houkago/` | サンプル作品「放課後の灯」。パーサの主要なテストデータ |

## 元の資料

設計検討書と画面のデザインは、Claude上の次の資料から書き出した。内容を変えたら、このリポジトリの該当ファイルも更新する。

- 設計検討書：https://claude.ai/code/artifact/cd1b47cd-6fb7-4087-84f6-6021e58a7979
- 画面UIのキャンバス：https://claude.ai/artifact/Hwptrzm5w4rLDCujY1xotj
