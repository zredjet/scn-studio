# ゲートの短編「雨宿りの駅」

フェーズ1のゲート（`docs/plan/phase1.md`）の短編。T1-13で、Claude Code がツールの CLI（`scn new`・`check`・`fmt`・`graph`・`export script`）と作品フォルダの手順（`CLAUDE.template.md`）どおりに書いた（D052）。全シーンが AI下書き（`status: ai-draft`）で、出自を `ai/provenance.jsonl` に記録している。

プレビューのランタイムのテスト（`app/test/preview.test.ts`）の入力にも使う。選択肢のすべての組み合わせで、3つのエンディング（`bus_end`・`stay_end`・`rain_end`）すべてに着く。

| ファイル | 内容 |
| --- | --- |
| `story/chapter01.scn` | 改札前から雨脚まで。選択肢2つ（`s02`、`s06` は条件つきの枝を含む）、`@呼出` で構内放送（`s90`） |
| `story/chapter02.scn` | 夜明け前の分岐（`@if`／`@elif`／`@else`）と3つのエンディング、呼び出して使う構内放送 |
| `concept.md` / `style.md` | 企画書（エンディング3つ）と文体 |
| `bible/` | 設定DB（キャラクター3人） |
| `variables.yaml` | 変数4つ |
| `assets/catalog.yaml` | 素材カタログ（すべて仮素材） |
| `ai/provenance.jsonl` | 出自の記録 |
| `ai/*.yaml` | 組み込みのAIタスクの定義（9つ。`scn new` のひな形と同じ。T3-6） |
| `CLAUDE.template.md` | 作品フォルダに置くAIエージェント向けの指示書（作品では `CLAUDE.md` にする）。ツールのリポジトリでは、開発作業中に読み込まれないよう、この名前で置いている（`scn new` が作ったまま） |

`scn new` が作る `FORMAT.md` は置いていない（ツールの版が上がると古くなるため。作品フォルダで `scn format-doc` を実行すると作れる）。

`scn check samples/amayadori` は、エラー・警告・情報とも0件。
