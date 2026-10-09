# scn コマンドの使い方

`scn` は scn-studio のコマンド（CLI）。アプリと同じコア（解析・検証・書き出し）を、ターミナルや AI エージェント（Claude Code など）から使う。アプリで書いても外部のエディタで書いても、作品フォルダのテキストが正なので、どちらからでも同じ結果になる。

この説明は、scn-studio のリポジトリの `docs/guide/scn.md` にあり、`scn new` が作品フォルダに `SCN.md` として置く（ツールの版が上がったら `scn format-doc` で書き直す）。`.scn` の書き方と作品フォルダの規約は `FORMAT.md`（リポジトリでは `docs/spec/scn-format-v0.md` と `docs/spec/project-files.md`）にある。文中の「規約の◯節」は、作品フォルダの規約の節。

## 1. 入れ方

Rust（`cargo`）が要る。scn-studio のリポジトリで次を実行すると、`~/.cargo/bin/scn`（Windows は `%USERPROFILE%\.cargo\bin\scn.exe`）に入る。

```sh
cargo install --locked --path crates/scn-cli
scn --version
```

- `scn: command not found` なら、`~/.cargo/bin` が `PATH` に入っていない。Rust の入れ方の案内（`rustup`）どおりに `PATH` を足すか、絶対パスで呼ぶ。
- リポジトリを更新したら、同じコマンドで入れ直す。入れ直したら、作品フォルダで `scn format-doc` を実行して `FORMAT.md` と `SCN.md` を新しい版にする。
- 入れずに試すときは、リポジトリで `cargo run -p scn-cli -- <コマンド> …` と書く（例：`cargo run -p scn-cli -- check samples/houkago`）。

## 2. 早見表

| したいこと | コマンド |
| --- | --- |
| 新しい作品を始める | `scn new <フォルダ> --title 作品名` |
| 行IDの無い行に行IDを付ける | `scn fmt .` |
| 行IDの無い行があるか確かめる（書き換えない） | `scn fmt --check .` |
| AI が書いた行として行IDを付ける | `scn fmt . --provenance external:claude-code --model <モデル名>` |
| 作品全体を検証する | `scn check` |
| 条件と選択肢を実際にたどって検証する（自動巡回と表記の検査） | `scn check --stage 2` |
| 1つの章だけを解析する | `scn parse story/chapter01.scn` |
| シーンのつながりと変数の読み書きを見る | `scn graph`（`--level chapter`・`--level route`） |
| IR（解析の結果）を JSON で取る | `scn ir . --compact` |
| AI が書いた行と項目の一覧を出す（賞や公募の申告） | `scn provenance`（`-o ai-lines.csv` で CSV） |
| AI タスクが LLM に渡す中身を見る（LLM は呼ばない） | `scn ai context <タスク> --scene <シーンID>` |
| 台本（収録用の表）を書き出す | `scn export script`（`--format xlsx`、`--baseline`） |
| 素材の発注リストを書き出す | `scn export orders` |
| 通読用のテキストを書き出す | `scn export text`（`--route 名前`・`--ending ID`） |
| EPUB・縦書きPDF用の HTML を書き出す | `scn export epub`・`scn export pdf`（`--typeset` で PDF まで） |
| Ren'Py・ティラノスクリプト・宴へ書き出す | `scn export renpy`・`scn export tyrano`・`scn export utage` |
| 自分の書き出し（テンプレートか外部コマンド）を使う | `scn export <プロファイル名>` |
| `FORMAT.md` と `SCN.md` を今の版にする | `scn format-doc`（`--check` で確かめるだけ） |
| エディタ（VS Code など）で診断と補完を使う | `scn lsp`（エディタが起動する） |
| 使い方をその場で見る | `scn --help`、`scn <コマンド> --help`、`scn export <種類> --help` |

## 3. 共通のきまり

### 作品フォルダの指し方

- 作品フォルダは `project.toml` のあるフォルダ。ほとんどのコマンドは、作品フォルダを省くと今のフォルダを使う（`scn check` は `scn check .` と同じ）。
- `scn ir` と `scn fmt` は作品フォルダを省けない（`scn fmt .` のように書く）。`scn parse` は作品フォルダではなく `.scn` のファイルを渡す。`scn new` は作るフォルダを渡す。
- `project.toml` の無いフォルダを渡すと、何もせずに終了コード2で終わる。

### 出力

- 結果（診断、一覧、JSON、`-o -` で選んだ書き出し）は標準出力に、件数のまとめ・知らせ・進み具合は標準エラーに出す。結果だけをファイルやほかのコマンドに渡せる（`scn check --json > diagnostics.json`）。
- `--json` を付けると、結果を JSON で出す（`check`・`parse`・`graph`・`fmt`・`provenance`・`ai context`）。AI エージェントやスクリプトから読むときに使う。
- 書き出す先は `-o <パス>`（`--output <パス>` と同じ）で変えられる。`-o -` で標準出力に書けるのは、ファイル1つを書くもの（`export script`・`export orders`、1つの道を選んだ `export text`・`export epub`・`export pdf`、テンプレートのプラグイン、`provenance`）だけ。`export renpy`・`tyrano`・`utage` と外部コマンドのプラグインの `-o` は書き出し先のフォルダで、`-o -` は `-` という名前のフォルダになる（外部コマンドのプラグインは断る）。
- ファイルのパスは、渡した作品フォルダのパスに続けて出す（`scn check samples/houkago` なら `samples/houkago/story/chapter01.scn:…`、`scn check` なら `story/chapter01.scn:…`）。
- 文字コードは UTF-8。CSV は Excel でそのまま開けるように BOM 付き・改行は CRLF。

### 診断

本文と設定ファイルの問題は、次の1行の形で出る（`scn check` では、行IDのある行の診断の末尾に行IDが付く）。

```
story/chapter01.scn:73:1 W203 メッセージウィンドウ（3行×24字）に収まりません（4行になります） ^k3ff
```

- 位置は `ファイル:行:列`（行も列も1から。列は文字で数える）。
- コードの頭の文字が重さ：`E` はエラー（必ず直す）、`W` は警告（理由を確かめ、直すか残す理由を決める）、`I` は情報（直さなくてよい）。
- コードの意味と直し方は `FORMAT.md` の16節（診断の一覧）にある。
- 解析は1行の誤りで止まらない（壊れたまま書ける）。エラーがあっても、ほかの診断と結果は出る。

### 終了コード

| コード | 意味 |
| --- | --- |
| 0 | 成功（警告と情報だけなら0） |
| 1 | エラー（`E`）の診断がある。`--check` で直すもの（行IDの無い行、古い `FORMAT.md`・`SCN.md`）がある。`ai context` でタスクを実行できない |
| 2 | 使い方の誤り、作品フォルダでない、読めない・書けない、書き出しを断った（消してはいけないフォルダなど）、無いタスクやプロファイル |

設定ファイル（`project.toml`、`variables.yaml`、設定DBなど）の誤りは、エラーの診断（E801〜E803）になるので1。`graph`・`provenance` は診断で終了コードを変えない（作品フォルダでなければ2）。

## 4. 作品フォルダを作る：`scn new`・`scn format-doc`

### `scn new <フォルダ> [--title 作品名]`

新しい作品フォルダを作る。フォルダが無ければ作り、あっても空なら使う（空でなければ何も書かずに終了コード2）。作品名を省くとフォルダ名（`scn new .` なら今のフォルダの名前）。

```sh
scn new my-novel --title 夏の終わり
cd my-novel
scn check
```

作るもの：`project.toml`、`CLAUDE.md`（AI エージェント向けの指示書）、`FORMAT.md`（形式と規約）、`SCN.md`（この説明）、`concept.md`（企画書）、`style.md`（文体）、`variables.yaml`、`story/chapter01.scn`（スタブのシーン1つ）、`bible/types/`（設定DBの型：キャラクター、場所、組織、アイテム、用語、出来事、世界観の決まり、関係、伏線、種族、能力、文化、モチーフ。要らない型はファイルを消してよい）と `bible/entries/character/`、`ai/`（組み込みの AI タスク14個と出自の記録）、`.gitignore`、`.gitattributes`。作った直後の `scn check` は、スタブの行き止まりの警告（W503）が1つだけ出る。

アプリの「新しい作品」も同じものを作る。

### `scn format-doc [作品フォルダ] [--check]`

`FORMAT.md` と `SCN.md` を、今のツールの版で書き直す（同じなら書かない）。この2つは手で直さない。`--check` は書き直さずに確かめ、どちらかが古い（無い）と終了コード1。

## 5. 行IDを付ける：`scn fmt`

```sh
scn fmt <作品フォルダ> [--check] [--json] [--provenance <タスク> --model <モデル名>]
```

行IDの無い行（I302 の出る行）の末尾に ` ^xxxx` を足し、付けた行を `ファイル:行:列 ^ID` の形で出す。ほかの文字は変えない（整形はしない）。行IDは一度付いたら変わらない。ボイス、翻訳、コメント、AI の出自の記録が行IDを使う。

- アプリで保存すると、行IDは自動で付く。外部のエディタや AI エージェントで書いたときに `scn fmt` を使う。
- `--check`：書き換えずに、行IDの無い行を `ファイル:行:列 行IDがありません` の形で出す。あれば終了コード1。
- `--provenance <タスク> --model <モデル名>`：付けた行IDを、AI が書いた行として `ai/provenance.jsonl` の末尾に書き足す。外部のエージェントのタスク名は `external:claude-code` のように書く。人が書いた行に先に行IDを付けておけば（`--provenance` なしの `scn fmt .`）、記録されるのは AI が書いた行だけになる。
- 本文の診断では終了コードを1にしない。`project.toml` の `format_version` か `[dialogue]` を読めなければ、何も書かずに終了コード2（`project.toml` を直してからやり直す）。

## 6. 検証する：`scn check`・`scn parse`

### `scn check [作品フォルダ] [--stage 2] [--json]`

作品全体を検証し、診断をまとめて出す。標準エラーに `エラー 0件、警告 5件、情報 7件` のような件数が出る。

- 段階1（既定）：読み込み・構文・意味の診断（話者、シーンの参照、変数の型、素材、設定DBの値）と、設定DBの参照切れ（W806）、行き止まり（W503）、到達できないシーン（W504）、エンディングに着かない循環（W505）、メッセージウィンドウのはみ出し（W203）、変数の読み書き（W605・W606）、企画書のエンディングと本文の照らし合わせ（W701・W702）、回収の無い伏線（W703）。
- `--stage 2`：段階1に加えて、開始シーンから条件と選択肢を実際にたどる（自動巡回）。決して選べない選択肢（W506）、真にならない条件（W507）、選べる選択肢が無くなる選択（W508）、どう選んでも着かないシーン（W509）、仕込みを通らずに回収へ着ける伏線（W704）。表記の検査も行う（一人称の表記ゆれ I204、避ける書き方 I205、かなだけ違う未登録の用語 I206）。カバレッジとエンディングごとの到達条件（そこまでの選択肢の例と変数の値）を標準エラーに出す（`--json` では `{"diagnostics": …, "playthrough": …}`）。
- 自動巡回は組み合わせが多いと上限で止まる。そのときは「無い」と言い切る診断（W506・W507・W509）を出さない（見つけたことの診断 W508・W704 は出す）。

```
$ scn check --stage 2
story/chapter01.scn:72:1 W506 この選択肢は、条件（勇気 >= 2）が真にならないので選べません ^k3fe
…
自動巡回：状態 3、周回の始め方 1、シーン 11/13、選べた選択肢 3/4
エンディング akari_end（s040）：着く（状態 1。例：選択肢 「すぐに行く」 → 「ごめん、遅くなって」）　akari.好感度 = 3、…
エンディング bad_end（s061）：着かない
```

### `scn parse <ファイル.scn> [--json]`

1つの章だけを解析する。ファイルが作品フォルダの中（上のフォルダに `project.toml` がある）なら、作品全体で意味解析を行い、設定ファイルの診断と、そのファイルの診断を出す（ほかの章の診断は出さない）。作品フォルダの外なら、構文の診断だけを出す。検証の段階1（行き止まりなど）は行わない（`scn check` で）。

## 7. 構造を見る：`scn graph`・`scn ir`

### `scn graph [作品フォルダ] [--level scene|chapter|route] [--json]`

シーンのつながり（辺）、辺の条件と選択肢、変数の読み書きを、粒度ごとに一覧で出す。分岐を足す前に作品の形を確かめるのに使う。辺の文脈は、いちばん内側のもの（選択肢、条件、`@else` の枝）だけを書く。

```
$ scn graph
s012 放課後の屋上（story/chapter01.scn、ルート 共通、draft）
  -> s014（選択肢「黙ってうなずく」 if 勇気 >= 2）
  -> s013a（条件 灯.好感度 >= 3）
  -> s013b（@else の枝）
  読む：akari.好感度、flag.勇気
  書く：akari.好感度
$ scn graph --level route
ルート 灯（シーン s013a、s014、s020、s021、s022、s040）
  @エンディング akari_end
  書く：akari.転校済み、flag.ルート
```

- `scene`（既定）：シーンごと。見出しに題名、章のファイル、ルート、状態（`status`）。
- `chapter`：章（ファイル）ごと。章の外へ出る辺だけ。
- `route`：ルート（シーンのメタ情報の `route`）ごと。ルートの外へ出る辺と、着くエンディング。

### `scn ir <作品フォルダ> [--compact]`

作品全体の IR（シーンごとの命令列と、シーン間の辺、診断）を JSON で標準出力に出す。自分でツールを書くとき、書き出しのプラグイン（9節）と同じ材料を見たいときに使う。`--compact` は字下げと改行を入れない（入れ子の深い本文でも大きくならない）。IR の形は `FORMAT.md` には入っていない（リポジトリの `docs/spec/scn-format-v0.md` の17節）。

## 8. AI：`scn provenance`・`scn ai context`

### `scn provenance [作品フォルダ] [--json] [-o 書き出し先]`

AI が書いた行（出自の記録で `ai` か、AI が書いた後に人が直した `ai-edited` の行）と、AI が書いた企画書・設定DBの項目の一覧を出す。賞や公募で AI の使用を申告するときに使う。標準エラーに数が出る。

```
$ scn provenance
story/chapter01.scn:63:1 ^k3fa ai-edited draft_stub [モデル名] 2026-10-06T21:10:00+09:00
（標準エラー）AIの行 1（ai 0、ai-edited 1）／行IDのある行 27
```

`-o ai-lines.csv` で CSV（UTF-8 の BOM 付き、CRLF）に書く（`-o -` で標準出力）。出自の記録（`ai/provenance.jsonl`）は手で直さない（規約の9節）。

### `scn ai context <タスク> [作品フォルダ] [--scene ID] [--route 名前] [--tokens N] [--json]`

AI タスクの定義（`ai/<タスク>.yaml`）から組み立てた、LLM に渡す中身（指示と依頼）を出す。LLM は呼ばない（呼ぶのはアプリの「AI」の画面）。タスクの定義を直したときや、ローカルの LLM の文脈の長さに入るかを確かめるときに使う。入れた節・削った節・知らせは標準エラーに出る。

- `--scene`：今のシーン（文脈に今のシーンや前のシーンの要約を入れるタスクで要る）。
- `--route`：対象のルート（ルートの骨格のタスクで）。
- `--tokens`：文脈の長さ（省くとタスクの接続先の `context_tokens`）。長さの4分の1（4096まで）を答えの分に取っておき、残りに入らない節を決まった順で削る。
- 終了コード：実行できない（シーンが要るのに無い、削っても入りきらない）と1、無いタスクは2。

組み込みのタスク（`scn new` が `ai/` に置く。定義は直してよく、自分のタスクも足せる。規約の9節）：

| タスク | 中身 | 既定の接続先 |
| --- | --- | --- |
| `scene_summary` | シーン要約 | local |
| `tone_check` | 口調チェック | local |
| `wording_check` | 表記ゆれと誤字 | local |
| `setting_check` | 設定矛盾チェック | local |
| `rephrase` | 言い換え候補（行の差分の提案） | local |
| `thin_branches` | 分岐の薄さの指摘 | cloud |
| `branch_ideas` | 分岐案を出す | cloud |
| `concept_questions` | 企画書とメモへの問い返し | cloud |
| `idea_sort` | メモの分類案 | cloud |
| `route_skeleton` | ルート骨格を作る | cloud |
| `draft_stub` | スタブの本文を下書き | cloud |
| `draft_chapter` | 章を下書きまで仕上げる | cloud |
| `draft_concept` | 企画書の下書き | cloud |
| `draft_entries` | 設定の下書き | cloud |

接続先（`local`・`cloud`）は `project.toml` の `[ai.<名前>]` で決める（規約の2節）。この端末だけの接続先の上書きとキーは、アプリの環境設定で行う。

## 9. 書き出す：`scn export`

どの書き出しも、作品フォルダの `export/` の下に書くのが既定で、書くたびに作り直す（書き出したものは手で直さない）。エラーの診断があっても書き出し、終了コード1（台本・発注リスト・Ren'Py・ティラノスクリプト・宴は、標準エラーにエラーの数も知らせる）。`--no-ai-drafts` を付けると、AI下書き（`status: ai-draft`）のシーンの台詞と地の文を省く（流れは残し、省いたことを地の文で示す。台本・テキスト・EPUB・PDF・Ren'Py・ティラノスクリプト・宴・プラグイン）。

### 台本：`scn export script [作品フォルダ] [--format csv|xlsx] [--baseline] [-o 書き出し先]`

収録と校正の表。列は、行ID、シーン、キャラ、台詞、直前の地の文、ボイスファイル名（`キャラID_行ID`）、備考。既定は `export/script.csv`（`--format xlsx` なら `export/script.xlsx`。欄はすべて文字列なので、行IDが数や日付に化けない）。`-o -` で標準出力。

- 行IDの無い台詞は、ボイスファイル名を空にして数を知らせる（先に `scn fmt .`）。
- `--baseline`：書き出した台本を収録の基準（`export/script-baseline.json`）として記録する。次からの書き出しで、基準の後に変わった台詞の備考に「再収録候補」と書き、変わった・足した・無くなった台詞の数を標準エラーに出す。収録に出すときに付ける。

### 発注リスト：`scn export orders [作品フォルダ] [-o 書き出し先]`

まだファイルの無い素材（仮素材と、カタログに無い素材）の表。種別、名前、キャラ、差分、使用回数、初出、使用シーン、備考。既定は `export/orders.csv`。

### 通読用のテキスト：`scn export text [作品フォルダ] [--route 名前 | --ending ID] [--no-notes] [-o 書き出し先]`

道（共通・ルート・エンディングまで）を一本道にした小説の形のテキスト。話者名は書かず、ルビは `｜親文字《読み》`（なろう・カクヨムの記法）。選んだ選択肢を `〔選択：…〕`、エンディングを `〔エンディング：…〕` で注記する（`--no-notes` で書かない）。

- `--route` も `--ending` も無ければ、共通（ルートの無い作品は「通し」）とすべてのルートを `export/text/<道の名前>.txt` に書く。着けない道は知らせて書かない。
- `--route 名前`・`--ending ID` で1つの道を選ぶ（無い・着けないと終了コード2）。`-o` はこのときだけ使える。

### EPUB と縦書きPDF：`scn export epub|pdf [作品フォルダ] [--route 名前 | --ending ID] [--horizontal] [--no-notes] [-o 書き出し先]`

道の選び方は `export text` と同じ。設定は `export/book.yaml`（規約の14節）。縦書きか横書きかは `export/book.yaml` の `writing`（無ければ縦書き）で、`--horizontal` を付けると横書き（`epub`・`pdf` のどちらも）。

- `epub`：EPUB 3 を `export/epub/<道の名前>.epub` に。
- `pdf`：縦書きPDFを組むための印刷用の HTML を `export/pdf/<道の名前>.html` に。PDF は Vivliostyle などの CSS 組版の道具で組む。`--typeset` を付けると、HTML を書いた後に `export/book.yaml` の `pdf_command`（例：`npx @vivliostyle/cli build {input} -o {output}`）を実行して PDF まで作る（`pdf_command` が無ければ2）。

### ゲームエンジン：`scn export renpy|tyrano|utage [作品フォルダ] [-o 書き出し先]`

演出の意味名（`@背景 屋上/夕方` など）は、素材カタログと出力プロファイルで各エンジンのファイルと命令にする。カタログにファイルの無い素材は仮素材で書き出す。

| 書き出し | 出力プロファイル | 既定の書き出し先 | 作り直すところ |
| --- | --- | --- | --- |
| `renpy` | `export/renpy.yaml`（規約の11節） | `export/renpy` | `game/scn/` だけ |
| `tyrano` | `export/tyrano.yaml`（12節） | `export/tyrano` | `data/scenario/scn/` と `data/{bgimage,fgimage,bgm,sound}/scn/`（`first: true` なら `data/scenario/first.ks` も） |
| `utage` | `export/utage.yaml`（13節） | `export/utage` | 書き出し先のまるごと（シナリオと設定の Excel のブックと素材） |

- Ren'Py・ティラノスクリプトの既存のゲームのプロジェクトに書き出すときは `-o <ゲームのフォルダ>`（またはプロファイルの `output`）。scn が作り直すのは上の表のところだけで、ほかのファイルには触らない。宴は書き出し先をまるごと作り直すので、ゲームのプロジェクトの外のフォルダに書き出し、Unity の宴のプロジェクトへ取り込む。
- 作り直すフォルダが scn の書き出したものでない（書き出したときに付ける印が無い。宴は空でもない）と、消さずに終了コード2で止まる（人のファイルを消さないため）。
- Ren'Py で遊んで確かめるには、書き出し先を Ren'Py のランチャーで開く。

### 自分の書き出し：`scn export <プロファイル名> [作品フォルダ] [-o 書き出し先] [--no-ai-drafts]`

作品フォルダの `export/<名前>.yaml` に `template:`（minijinja のテンプレート）か `command:`（外部コマンド）を書くと、その名前で書き出せる（規約の15節）。

```yaml
# export/cast.yaml：台詞の一覧を CSV にするテンプレート
template: tools/cast.csv.j2
```

```yaml
# export/mygame.yaml：IR の JSON を標準入力で受け取る外部コマンド
command: python3 tools/my_export.py --out {output}
```

- テンプレートには IR・作品の情報・キャラクター・通読の道が渡る。外部コマンドは作品フォルダで実行し、標準入力に IR の JSON（`scn ir --compact` と同じ）を渡す。シェルは通さない。
- 無い名前を指定すると、プラグインのプロファイルの名前を並べて終了コード2。プロファイルやテンプレートの誤り、コマンドの失敗も2。
- CLI は外部コマンドを確かめずに実行する（アプリは初めて実行する前に人に見せて確かめる）。人から受け取った作品フォルダでは、先に `export/*.yaml` の `command` を読む。

## 10. エディタから使う：`scn lsp`

言語サーバー（LSP）を標準入出力で動かす。人が直接は実行せず、エディタが起動する。診断（読み込み・構文・意味と検証の段階1）、補完、定義へ移動、参照、名前の変更、ホバー。作品フォルダは、エディタが初めに渡したフォルダか、開いた文書から上へ探した `project.toml` のあるフォルダ。

VS Code では scn-studio の拡張（リポジトリの `packages/vscode-scn`）を入れ、作品フォルダを VS Code のフォルダとして開く。`scn` が `PATH` に無いとき（macOS で Dock から開いた VS Code など）は、設定 `scn.path` に絶対パス（`~/.cargo/bin/scn` を展開したもの）を書く。

## 11. よくある流れ

### 外部のエディタや AI エージェントで書く

```sh
scn fmt --check .     # 人が書いた行に行IDが無いか
scn fmt .             # あれば先に付ける（人の行として）
# …書く（スタブ → 展開メモ → 本文）…
scn fmt . --provenance external:claude-code --model <モデル名>   # 自分が書いた行に行IDを付けて記録
scn check             # E は直す。W は理由を確かめる
scn check --stage 2   # 分岐を足したら、条件と選択肢をたどって確かめる
scn graph --level route
```

作品フォルダの `CLAUDE.md` に、AI エージェント向けの同じ手順と、書いてよいところ・書き換えないところがある。

### 収録に出す

```sh
scn fmt .                                       # 台詞に行IDを付ける（ボイスファイル名に使う）
scn check                                       # E が無いことを確かめる
scn export script --format xlsx --baseline      # 台本を書き、収録の基準を記録する
# …直した後…
scn export script --format xlsx                 # 変わった台詞の備考に「再収録候補」
```

### 読んでもらう・遊んでもらう

```sh
scn export text --route 灯 -o -        # 1つのルートを通読用のテキストで（標準出力）
scn export epub                        # 道ごとの EPUB
scn export renpy -o ~/renpy/mygame     # Ren'Py のプロジェクトへ
```

### Git と一緒に使う

- `scn new` の `.gitignore` は `.cache/` を Git に入れない。`export/` に書き出したものを Git に入れるかは作品ごとに決める（収録の基準 `export/script-baseline.json` はチームで共有するなら入れる）。
- コミットの前に `scn fmt --check .` と `scn check` を走らせると、行IDの付け忘れとエラーを防げる（Git のフックや CI で使える。終了コードで分かる）。

## 12. つまずいたとき

| 出たこと | わけと直し方 |
| --- | --- |
| `scn: command not found` | `~/.cargo/bin` が `PATH` に無い。1節 |
| 終了コード2と「…/project.toml を読めません」 | 渡したフォルダが作品フォルダでない。作品フォルダで実行するか、パスを渡す |
| `scn fmt` が何もせずに終了コード2 | `project.toml` の `format_version` か `[dialogue]` が読めない。`scn check` で E801〜E803 を見て直す |
| `scn new` が「空のフォルダではありません」 | 作品フォルダは無いフォルダか空のフォルダにだけ作る |
| `scn format-doc --check` が1 | ツールを入れ直した後で `FORMAT.md`・`SCN.md` が古い。`scn format-doc` を実行する |
| 台本のボイスファイル名が空 | 行IDの無い台詞。`scn fmt .` で付ける |
| `export renpy`・`tyrano`・`utage` が書き出し先を消さずに2 | 作り直すフォルダ（9節の表）が scn の書き出したものでない。`renpy`・`tyrano` は、作り直すフォルダ（`game/scn/` など）が無いか scn の書いたものなら、既存のゲームのフォルダにも書き出せる。`utage` は空のフォルダか、前に scn が書き出したフォルダを指す |
| `export text --route` が2 | そのルートが無いか、どの道でも着けない。`scn graph --level route` と `scn check --stage 2` で確かめる |
| `ai context` が1 | `--scene` が要るタスクか、文脈の長さに入りきらない。`--scene` を付けるか、`--tokens` に使う LLM の文脈の長さを渡して確かめる（CLI はアプリの環境設定の上書きを読まず、`project.toml` の `context_tokens` を使う） |
| CSV を Excel で開くと行IDが数になる | CSV は書き換えずに書いている。`--format xlsx` で書き出すか、取り込みで列を文字列にする |
| 診断の意味が分からない | `FORMAT.md` の16節（診断の一覧）。コードで探す |
