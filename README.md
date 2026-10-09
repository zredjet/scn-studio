# scn-studio（仮称）

分岐する物語（ADV・ビジュアルノベル）のための執筆ツール。Tauri製のデスクトップアプリで、WindowsとmacOSを対象にする。

Claude Code（Opus 5.5）で開発している。フェーズ0（形式を固める：形式仕様v0、パーサとIR、ハイライト、アプリの土台、日本語入力のテスト）、フェーズ1（執筆の核：エディタ、設定DB、構造グラフ、検証、簡易ADVプレビュー、台本の出力、CLI）、フェーズ2（制作に使える：素材カタログ、Ren'Py への書き出し、カーソル位置から再生、縦書きビュー、企画の画面、外部エディタとのマージなど）、フェーズ3（支援と拡張：アプリ内のAI、検証の段階2、ティラノスクリプト・宴・EPUB への書き出しとプラグイン、言語サーバーと VS Code 拡張）を終えた。設計検討書の範囲はここまで。その後、人の判断で、フェーズ4（ローカルのLLMと、企画・設定の生成：タスクの接続先と文脈の長さを端末で選ぶ、考えるモデルと構造化出力、企画書と設定の下書き、項目ごとの出自、自律の企画と設定の段。後から `scn` コマンドの説明と、設定DBの既定の型とテンプレート集）を足し、人の確認を除いて終えた。各フェーズの人の確認（2OSの日本語入力、Ren'Py などで遊ぶこと、実際の LLM、VS Code など）と、範囲の外の残りをどうするかは、`docs/review-for-human.md` にまとめて人の判断を待っている。

## 始め方

1. RustとNode.jsとpnpm、TauriのOS別の前提ソフトを入れる（各公式の最新の安定版）。
2. このフォルダで `claude` を起動し、`/model` でOpus 5.5を選ぶ。
3. `docs/progress.md` の「次のタスク」を見て、`/phase-task <タスクID>` と入力する（例：`/phase-task T2-2`）。タスクは `docs/plan/phase2.md` の順に進める。

詳しい進め方（1タスク1セッション、確認の入れ方、人の作業）は `docs/plan/phase0.md`・`docs/plan/phase1.md`・`docs/plan/phase2.md` にある。

## 中身

| パス | 内容 |
| --- | --- |
| `CLAUDE.md` | Claude Codeが毎回読む指示書。目的、原則、作業の進め方、止まって確認する場面 |
| `.claude/rules/` | 分野別の約束（Rust、フロントエンド、仕様書とサンプル）。該当ファイルを開いたときに読み込まれる |
| `.claude/skills/phase-task/` | `/phase-task <タスクID>` で、調査から記録・コミットまでを一通り進める手順 |
| `.claude/agents/plan-reviewer.md` | 差分を計画と仕様に照らすレビュー用のサブエージェント |
| `docs/design.md` | 設計検討書（全体設計） |
| `docs/spec/scn-format-v0.md` | `.scn` 形式の仕様（v0。T0-2で確定） |
| `docs/spec/project-files.md` | 作品フォルダの規約（`.scn` 以外のファイル） |
| `docs/guide/scn.md` | `scn` コマンドの使い方（入れ方、早見表、コマンドごとの説明、つまずいたとき。`scn new` が作品フォルダに `SCN.md` として置く。T4-8） |
| `docs/glossary.md` | 用語集（日本語とコード上の名前の対応） |
| `docs/decisions.md` | 決定記録 |
| `docs/plan/phase0.md` | フェーズ0の作業手順（完了） |
| `docs/plan/phase1.md` | フェーズ1の作業手順（完了。ゲートの人の確認を待っている） |
| `docs/plan/phase2.md` | フェーズ2の作業手順（完了。ゲートの人の確認を待っている） |
| `docs/plan/phase3.md` | フェーズ3の作業手順（人の確認を除いて完了） |
| `docs/plan/phase4.md` | フェーズ4の作業手順（人の判断で足した。D080。人の確認を除いて完了） |
| `docs/plan/design-check.md` | 設計検討書と作ったものの突き合わせ（T3-17） |
| `docs/review-for-human.md` | 人の指示で推奨案で決めたことと、人の環境でしか確かめられないことの一覧（人が戻ったときに確かめる） |
| `docs/plan/ime-test.md` | 日本語入力のテスト表 |
| `docs/progress.md` | 進捗ログ（Claude Codeが更新する） |
| `docs/ui/` | 画面の構成、デザイントークン、画面モック（`mockups/index.html`） |
| `samples/houkago/` | サンプル作品「放課後の灯」。パーサの主要なテストデータ |
| `samples/amayadori/` | フェーズ1のゲートの短編「雨宿りの駅」（T1-13。ツールとClaude Codeで書いた）。プレビューのテストの入力 |

## 元の資料

設計検討書と画面のデザインは、Claude上の次の資料から書き出した。内容を変えたら、このリポジトリの該当ファイルも更新する。

- 設計検討書：https://claude.ai/code/artifact/cd1b47cd-6fb7-4087-84f6-6021e58a7979
- 画面UIのキャンバス：https://claude.ai/artifact/Hwptrzm5w4rLDCujY1xotj
