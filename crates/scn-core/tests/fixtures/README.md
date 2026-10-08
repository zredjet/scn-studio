# fixtureの一覧

`scn-core` のテスト入力。仕様書（`docs/spec/scn-format-v0.md`）の段階0の診断それぞれに、それを出すfixtureを1つ以上置く。中身はT0-3（構文系）とT0-4（意味系）で作る。この一覧はT0-2で作った。

## 約束

- 1つのfixtureでは、1つの構文か1つの診断に絞る（`.claude/rules/spec-and-samples.md`）。
- 異常系は、期待する診断コードをファイル先頭のコメントに書く（例：`// expect: E103`）。複数あれば `// expect: W112 W101` のように並べる。正常系は `// expect:` を書かず、診断が出ないことを確かめる。
- 期待する診断には、そのfixtureで確かめるコードだけを書く。行IDの付け忘れによる I302 が混ざらないよう、I302 を確かめるもの以外では、台詞・地の文・選択肢・`@選択` の行に行IDを付ける。
- 行IDは、fixtureの中でも一意にする。`samples/houkago/` の行IDとは重なってもよい（fixtureは1つずつ解析するため）。
- 構文系のfixtureは、プロジェクトの外の1ファイルとして解析する。台詞の括弧は既定の「」だけになる（仕様書6節）。
- 意味系のfixtureは `sema/` に置き、共通の小さなプロジェクト `project/` の設定DB・変数・素材カタログを使って解析する（D036）。1つのfixtureを、そのプロジェクトの本文の全部として扱う。複数のファイルにまたがるものは、ディレクトリにまとめる（ファイルはパスの辞書順に並べる）。期待する診断は、診断が出るファイルの先頭に書き、ディレクトリ全体で合わせて比べる。
- CRLFのfixture（`ok_crlf.scn`）は、`.gitattributes` でそのファイルだけ `-text` を指定し、LFに変換されないようにする。

## 構文系（T0-3）

### 正常系

| ファイル | 内容 |
| --- | --- |
| `ok_minimal_scene.scn` | 見出し、メタ情報、台詞、地の文、ジャンプだけの最小のシーン |
| `ok_chapter_heading.scn` | 先頭の章見出し（前にコメントと空行を置く） |
| `ok_indent.scn` | 行頭の半角空白とタブは読み飛ばし、全角空白は本文に残す |
| `ok_dialogue_display_name.scn` | `灯（？？？）「誰？」` の形の表示名と、空の表示名（`灯（）「…」`、D035） |
| `ok_dialogue_multiline.scn` | 閉じ括弧が次の行以降にある台詞。行IDは最後の行に付ける。途中のコメントの行は台詞に含めない。途中の `# ` の行は台詞の続きで、W112 を出さない（D035） |
| `ok_dialogue_nested_brackets.scn` | 本文の中に同じ種類の括弧がある台詞（入れ子として数える） |
| `ok_inline_markup.scn` | ルビ（`\|`、`｜`、自動ルビ）、傍点、`{w}`、`{p}`、`{se}`、`{speed}`、`{=式}`、`{{` と `}}`、`\|《` と `｜《`（D035）、閉じていない `{` |
| `ok_directives.scn` | 10節のディレクティブすべてを、日本語名と英語名で。引数のキーワードの日英（`左`／`left`、`停止`／`stop`）、`"…"` で囲んだ引数、全角空白の区切り、位置を省いてトランジションを書いた `@立ち絵`、小数の秒 |
| `ok_choice.scn` | 条件・ジャンプ付きの選択肢、中身の無い選択肢、入れ子の選択ブロック、`{w 0.5}` を含む問いかけ文 |
| `ok_if_elif_else.scn` | `@if`、複数の `@elif`、`@else`、`@end` |
| `ok_raw_dedent.scn` | `@raw` の共通の字下げを取り除き、それより深い字下げと空行を保つ。半角空白だけの行は空行として扱う |
| `ok_raw_content.scn` | `@raw` の中の、字下げした `## ` の行、` ^mask` で終わる行、`@選択` や `- ` で始まる行を、解析せずに中身として保つ |
| `ok_escape.scn` | `\@`、`\-`、`\/`、`\#`、`\\` と、台詞の形の前の `\` |
| `ok_blank_lines.scn` | `break` になる空行（本文に挟まれたもの、間にコメントがあるもの）と、ならない空行（演出やブロックの前後） |
| `ok_expr.scn` | 演算子の優先順位、括弧、`not`、文字列、真偽、名前空間の省略、日本語の名前、全角空白の区切り（D035） |
| `ok_line_id_lengths.scn` | 4文字と12文字の行ID。`^_^` と、前に半角空白の無い `^` は本文の一部 |
| `ok_trailing_whitespace.scn` | 行末の半角空白とタブを取り除く（行IDの後ろに空白があっても行IDとして読む）。全角空白は残す（D035） |
| `ok_comment_line_id.scn` | コメントの行からは行IDを切り出さない（W303・E304 を出さない。D035） |
| `ok_meta_comment.scn` | メタ情報の中のコメントの行を読み飛ばす（D035） |
| `ok_bom.scn` | 先頭にBOMのあるファイル |
| `ok_crlf.scn` | 改行がCRLFのファイル |

### 異常系

| ファイル | 内容 | 期待する診断 |
| --- | --- | --- |
| `before_first_scene.scn` | 最初のシーン見出しより前に地の文がある | W101 |
| `meta_unclosed.scn` | メタ情報の `---` が閉じていない（`キー: 値` の形でない行の手前で回復し、続く台詞は本文になる。途中のコメントの行ではメタ情報を終えない）。後ろにメタ情報のあるシーンを置き、その開きの `---` を閉じと取り違えないことも確かめる | E102 |
| `choice_unclosed.scn` | `@選択` が `@end` で閉じていない（次のシーン見出しで回復する） | E103 |
| `if_unclosed.scn` | `@if` が閉じていない（ファイルの終わりで回復する） | E103 |
| `raw_unclosed.scn` | `@raw` が閉じていない（行頭に空白の無い `## ` の行で回復し、そのシーンは正しく読める） | E103 |
| `raw_unclosed_eof.scn` | `@raw` が閉じていないまま、ファイルが終わる | E103 |
| `end_without_block.scn` | 対応するブロックの無い `@end`（無視した行の前後の空行は、段落の区切りにならない） | E104 |
| `else_without_if.scn` | `@if` の無い `@elif` と `@else` | E104 |
| `else_duplicate.scn` | 2つ目の `@else` | E104 |
| `elif_after_else.scn` | `@else` の後の `@elif` | E104 |
| `option_outside_choice.scn` | 選択ブロックの外にある `- ` の行（地の文として扱う） | E105 |
| `choice_no_options.scn` | 選択肢が1つも無い `@選択` | E106 |
| `scene_heading_no_id.scn` | ` #` の無いシーン見出し | W107 |
| `meta_unknown_key.scn` | メタ情報の知らないキー | I109 |
| `meta_status_invalid.scn` | `status` の値が一覧に無い | E110 |
| `scene_id_invalid.scn` | 形の崩れたシーンID（`#帰り道`、`#s-1`、` #` の後が空） | E111 |
| `ending_id_invalid.scn` | 形の崩れたエンディングID（`@エンディング 灯エンド`） | E111 |
| `chapter_heading_not_at_top.scn` | ファイルの途中にある `# ` の行（シーンより後） | W112 |
| `chapter_heading_second.scn` | 先頭の章見出しの後、最初のシーンより前に2つ目の `# ` の行 | W112 W101 |
| `meta_duplicate_key.scn` | メタ情報の同じキーが2回 | W113 |
| `meta_malformed_line.scn` | 閉じたメタ情報の中にある、`キー: 値` の形でない行（`summary 灯が…`） | E114 |
| `dialogue_unclosed_blank.scn` | 閉じ括弧の前に空行が来る台詞 | E201 |
| `dialogue_unclosed_directive.scn` | 閉じ括弧の前にディレクティブが来る台詞 | E201 |
| `dialogue_unclosed_heading.scn` | 閉じ括弧の前にシーン見出しが来る台詞 | E201 |
| `dialogue_unclosed_option.scn` | 選択肢の中身で、閉じ括弧の前に次の選択肢の行が来る台詞（選択ブロックは壊れない） | E201 |
| `dialogue_unclosed_eof.scn` | 閉じ括弧が無いまま、ファイルが終わる台詞 | E201 |
| `line_id_not_allowed.scn` | 行IDを持てない行（シーン見出し、ディレクティブ、ジャンプ、行IDだけの空行、複数行の台詞の途中の行）に行IDがある | W303 |
| `line_id_malformed.scn` | 形の崩れた行ID（`^K3F9`、`^k3`、13文字） | E304 |
| `directive_unknown.scn` | 知らないディレクティブ（`@回想開始`、`@Bgm`） | W401 |
| `directive_arg_count.scn` | 引数の数が合わないディレクティブ（`@背景` に引数なし、`@待機` に引数2つ） | E402 |
| `directive_arg_invalid.scn` | 引数の値が合わないディレクティブ（`@待機 長め`、引数4つの `@立ち絵` の3つ目が位置でない、`"` が閉じていない引数（D035）） | E402 |
| `option_empty_text.scn` | 表示文が空の選択肢（`-` だけ、`- -> s014`、`- if 勇気 >= 2`、`- ^abcd`） | E402 |
| `choice_text_before_option.scn` | `@選択` と最初の選択肢の間にある地の文 | E402 |
| `inline_unknown_command.scn` | 知らないインライン命令（`{shake}`） | W405 |
| `fullwidth_symbol_line.scn` | 行頭の全角の `＠`・`－＞`・`ー＞`・`＞`・`＃＃` で始まる地の文（ディレクティブ・ジャンプ・見出しのつもりの誤入力）。行頭の `\` で止められ、全角空白の後ろなら出さない（D043） | W406 |
| `inline_invalid_value.scn` | 値の合わないインライン命令（`{w 長め}`、`{speed 速すぎ}`） | W405 |
| `expr_syntax_error.scn` | 式の構文エラー（閉じていない括弧、`1日目`、`and` を名前に使う、全角の `”`、全角の数字） | E601 |
| `option_if_in_text.scn` | 表示文に半角の ` if ` を含む選択肢（`- What if I said no`） | E601 |

## 意味系（T0-4、`sema/`）

### 共通のプロジェクト `project/`

- `project.toml`：`format_version = 0`。`[dialogue.brackets]` に「」（`normal`）と『』（`thought`）
- 設定DB：キャラクター3人。1人は名前と呼び名を持つ（話者と名前空間の照合を確かめる）。2人は同じ呼び名を持つ（あいまいな呼び名を確かめる）
- `variables.yaml`：`int`、`bool`、`string`、`enum` の変数を1つずつ以上。キャラクターの名前空間の変数を1つ
- `assets/catalog.yaml`：背景、立ち絵、BGM、SEを少しずつ

### 正常系

| ファイル | 内容 |
| --- | --- |
| `ok_speaker_resolution.scn` | ID、名前、呼び名のそれぞれで書いた話者が、同じキャラクターIDになる |
| `ok_narration_with_quote.scn` | `彼は「行こう」と言った。` のように、閉じ括弧の後に文が続く行が、話者を解決できてもできなくても、診断なしで地の文になる（D036）。閉じ括弧の後ろが変数の埋め込み・ルビ・傍点だけでも、文が続くとみなす（D041） |
| `ok_dialogue_commands_after_close.scn` | 閉じ括弧の後ろがインライン命令（`{p}`・`{w}`・`{se}`・`{speed}`）と空白だけの行が台詞になり、命令が台詞の本文の末尾に付く。複数行の台詞も同じ（D041） |
| `ok_dialogue_bracket_style.scn` | 『』の台詞が、表示スタイル `thought` の台詞になる |
| `ok_dialogue_line_break.scn` | 複数行の台詞の行の境目に、IRで改行の要素が入る（D036） |
| `ok_var_namespace.scn` | `灯.好感度` のようなキャラの名前の名前空間がIDにそろい、名前空間の無い名前に `flag.` が補われる |
| `ok_enum_value.scn` | 列挙の変数への代入と比較を文字列で書く |
| `ok_cross_file_jump/` | 別のファイルのシーンへのジャンプと呼び出し |
| `ok_ending.scn` | `@エンディング` で終わるシーン |
| `ok_jump_branches.scn` | ジャンプの直後に `@else`・`@end`・次の選択肢の行・空行・コメントが来ても、W502 が出ない |
| `ok_assets_known.scn` | カタログにある意味名。トランジションと `@効果` の効果名は、カタログに無くても I403 にならない |

### 異常系

| ファイル | 内容 | 期待する診断 |
| --- | --- | --- |
| `scene_id_duplicate/` | 2つのファイルに同じシーンID（辞書順で後のファイルに出る） | E108 |
| `scene_id_duplicate_in_file.scn` | 1つのファイルに同じシーンID（後の行に出る） | E108 |
| `dialogue_unknown_speaker.scn` | 未登録の話者の台詞（`店員「いらっしゃいませ」`）。閉じ括弧の後ろがインライン命令だけの行も同じ（D041） | W202 |
| `dialogue_unknown_command_after_close.scn` | 閉じ括弧の後ろの知らないインライン命令は文字なので、地の文になる（D041） | W405 |
| `dialogue_ambiguous_alias.scn` | 2人が同じ呼び名を持ち、その呼び名で書いた台詞 | W202 |
| `line_id_duplicate/` | 2つのファイルに同じ行ID | E301 |
| `line_id_duplicate_in_file.scn` | 1つのファイルに同じ行ID | E301 |
| `line_id_missing.scn` | 行IDの無い台詞、地の文、選択肢、`@選択` の行 | I302 |
| `asset_unknown.scn` | 素材カタログに無い背景・立ち絵の差分・BGM・SE・`{se}` の意味名 | I403 |
| `show_unknown_character.scn` | `@立ち絵`・`@退場` のキャラを解決できない（差分の I403 は重ねない） | W404 |
| `jump_unknown_scene.scn` | 存在しないシーンへのジャンプ、選択肢の行のジャンプ、`@呼出` | E501 |
| `unreachable_after_jump.scn` | ジャンプ、`@エンディング`、`@戻る` の後ろの、同じ枝の中の行 | W502 |
| `var_undeclared.scn` | 宣言されていない変数の参照、代入、`+=`（E603・E604 は重ねない） | W602 |
| `var_type_mismatch.scn` | 型の不一致（`bool` に整数、`int` と文字列の比較、列挙の `values` に無い値、条件に整数） | E603 |
| `var_compound_non_int.scn` | `+=`・`-=` を整数以外の変数に使う（E603 は重ねない） | E604 |
| `scene_heading_anonymous.scn` | IDの無いシーンに、IRで仮のIDが付く（D036） | W107 |
| `meta_in_ir.scn` | メタ情報のIRでの形（不正な `status` は `null`、リストでない `tags`、知らないキーは `extra`。D036） | I109 E110 |

## 段階0の診断との対応

仕様書16節の段階0の診断が、すべて上の表にあることを確かめるための一覧。

| 種類 | コード |
| --- | --- |
| 構文系（T0-3） | W101、E102、E103、E104、E105、E106、W107、I109、E110、E111、W112、W113、E114、E201、W303、E304、W401、E402、W405、W406（T1-4）、E601 |
| 意味系（T0-4） | E108、W202、E301、I302、I403、W404、E501、W502、W602、E603、E604 |
| 設定ファイル（T1-3） | E801、E802、E803、W804、W805（T1-10） |

## 設定ファイル（T1-3、`settings/`）

設定ファイル（`project.toml`、`variables.yaml`、`assets/catalog.yaml`、設定DB）と本文のファイルの読み込みの診断（仕様書16節、D042）。fixtureは1つずつが小さなプロジェクトで、期待する診断は `story/a.scn` の1行目の `// expect:` に書く。`expect` の無いものは正常系。

| フォルダ | 内容 | 期待する診断 |
| --- | --- | --- |
| `ok_builtin_type/` | 型定義に `builtin: character` のある型（フォルダ名が `character` でない）のエントリが話者になる | |
| `ok_brackets_empty/` | 空の `[dialogue.brackets]` は、既定の「」を使う | |
| `toml_broken/` | `project.toml` の構文の誤り。既定の設定（括弧は「」）で読む | E801 |
| `toml_version/` | 対応していない `format_version`。v0として読む | E803 |
| `toml_items/` | `format_version`・`start_scene`・`[dialogue.brackets]` の形の誤り。括弧が1組も読めなければ既定の「」 | E802 |
| `ai_task_items/` | AIタスクの定義（`ai/*.yaml`）の誤り：知らない `level`・`context` の項目、`level` に合わない `output`、`prompt` が無い、`budget`・`max_iterations` の形、表でない定義（E802）、YAML の構文の誤り（E801）。読めた項目で続ける（T3-6、D067） | E801 E802 |
| `toml_ai/` | `[ai.*]` の型の誤り（`model` が文字列でない、`context_tokens` が正の整数でない（T3-6）、接続先が表でない）。その接続先だけを読み飛ばす（T3-5、D066） | E802 |
| `toml_brackets/` | 表示スタイルが英字でない組と、括弧が重なる組を読み飛ばし、読めた組を使う | E802 |
| `toml_dialogue_scalar/` | `dialogue` が表でない。既定の「」を使う | E802 |
| `toml_editor/` | `[editor]` の値が真偽でない。入力補助を切として扱う（D047） | E802 |
| `toml_screen/` | `[screen]` の `width`・`height` が正の整数でない。プレビューは 1280×720 で見せる（T1-11、D050） | E802 |
| `toml_title/` | `title`・`start_scene`・`[message_window]` の形の誤り。本文の読み方は変わらないので、行IDの付与は止めない（D045） | E802 |
| `variables_broken/` | `variables.yaml` の構文の誤り。宣言の無い変数（W602）を出さない | E801 |
| `variables_not_map/` | `variables.yaml` が対応表でない。W602 を出さない | E802 |
| `variables_items/` | 形の合わない変数（型、列挙の値、名前）だけを読み飛ばす。読み飛ばした変数への参照は W602 | E802 W602 |
| `variables_values/` | 宣言の値が型に合わない（`default` の型、列挙の値、`range` の形・向き・型、`default` が範囲の外、`scope`）。宣言は読み飛ばさない（T2-10、D061） | W807 |
| `catalog_broken/` | 素材カタログの構文の誤り。カタログを空として扱う | E801 I403 |
| `catalog_sections/` | 形の合わない節（背景の節、差分）と、文字列にできない意味名だけを読み飛ばす | E802 I403 |
| `bible_type_broken/` | キャラクターの型定義を読めない。フォルダ名 `character` でキャラクターを見分ける | E801 |
| `bible_type_items/` | 型定義の `builtin`・`fields` の誤り | E802 |
| `bible_two_character_types/` | `builtin: character` を2つの型に書くと、最初の型を使う | E802 W202 |
| `bible_entry_broken/` | エントリのフロントマターを読めない。そのエントリを読み飛ばす | E801 W202 |
| `bible_entry_items/` | `name` の無いキャラクターを読み飛ばし、形の合わない `aliases` は呼び名だけを読み飛ばす | E802 W202 |
| `bible_untyped_folder/` | 型定義の無いエントリのフォルダ（`character` の打ち間違い） | W804 W202 |
| `bible_field_types/` | 読めない項目の型（打ち間違い、閉じていない `list<`）の項目の定義を読み飛ばす（T1-10、D049） | E802 |
| `bible_values/` | 項目の型に合わない値（数・真偽・日付・選択肢・複数選択・リスト・参照・文字列）。書いていない値と空の文字列は合う（T1-10、D049） | W805 |
| `story_not_utf8/` | UTF-8でない本文のファイル（`story/b.scn`）を読み飛ばす | E801 |

## 検証の段階1（T1-4、`check/`）

`scn check` がプロジェクト全体で行う検証（仕様書16節の「段階」が1の診断、D043）。fixtureは1つずつが小さなプロジェクトで、期待する診断（読み込み・構文解析・意味解析の診断を含む）は `story/a.scn` の1行目の `// expect:` に書く。`expect` の無いものは正常系。

| フォルダ | 内容 | 期待する診断 |
| --- | --- | --- |
| `ok_flow/` | 出口のある経路だけ：選択肢のどれにも出口がある、`@else` まで出口のある `@if`、`@呼出` と `@戻る`（呼ばれる側がジャンプを挟んで `@戻る` に着く場合を含む） | |
| `dead_end/` | 出口の無い経路：本文の無いシーン、`@else` の無い `@if`、出口の無い選択肢、`@呼出` だけで終わるシーン | W503 |
| `unreachable/` | 開始シーン（`start_scene`）から着けないシーン。最初のシーンでも、開始シーンでなければ数える | W504 |
| `unreachable_no_start/` | `start_scene` が無ければ、最初のシーンを開始シーンにする | W504 |
| `start_missing/` | `start_scene` のシーンが無い（`project.toml` に E501） | E501 |
| `cycle/` | どのエンディングにも着かない循環と、呼び出しを挟んだ循環。行き止まりに続くだけのシーンと、`@戻る` で終える呼ばれる側には出さない | W503 W505 |
| `variables/` | 書き込むだけの変数、読まれるのに書かれない変数。`persistent` の変数には W606 を出さない | W605 W606 |
| `overflow/` | メッセージウィンドウ（2行×10字）からのはみ出し。話者名と括弧、ルビの読み、インライン命令は数えない。半角は0.5。`{p}` でページを改める | W203 |
| `no_window/` | `[message_window]` が無ければ、はみ出しを調べない | |
| `concept/` | 企画書のエンディングが本文に無い、本文のエンディングが企画書に無い | W701 W702 |
| `concept_without_endings/` | 企画書に `endings` が無ければ調べない | |
| `concept_broken/` | 企画書のフロントマターを読めない（W701・W702 は調べない） | E801 |
| `concept_items/` | `endings` の形の合わない要素を読み飛ばし、読めたもので調べる | E802 W701 W702 |
| `foreshadow/` | 回収（`payoff`）が無い、空、無いシーンを指す伏線。回収は W703 だけで調べ、W805・W806 は出さない | W703 |
| `references/` | 参照先の無い設定DBの参照：無いエントリ、無いシーン、リストの中の無い参照、型の定義の無い型への参照（T1-10、D049） | W806 |

## 行IDの付与（T1-2、`fmt/`）

`scn fmt` の付け方（仕様書9節、D041）を確かめる入力。`*.expected` は、付けた後の本文の形で、`{0}`・`{1}`… が付けた行IDに置き換わる（`{{`・`}}` は波括弧そのもの）。改行・BOM・行末の空白を確かめるので、`.gitattributes` で変換と空白の検査を止めている。

| ファイル | 内容 |
| --- | --- |
| `placement.scn` | 付ける場所：BOM、CRLF、行末の半角空白とタブ、行末の全角空白、絵文字、複数行の台詞、閉じ括弧の後ろのインライン命令、`@選択` と選択肢の行、改行の無い最後の行。形の合わない候補のある行（E304）、コメントの行、行IDのある行、見出しに付いた行ID（W303）には付けない |

## `story/` の下のフォルダ（T2-10、`subfolders/`）

- `subfolders/`：`story/prologue.scn`・`story/part1/ch01.scn`・`story/part2/ch01.scn`（同じファイル名）と、読まない隠しフォルダ `story/.trash/`。パスの順に読み、IDの無いシーンの仮のIDは `_anon_part1_ch01_1` のように重ならない（`tests/analyze.rs` の `subfolders_fixture`。D061）。

## 検証の段階2（T3-2、`playthrough/`）

`scn check --stage 2` の自動巡回（仕様書16節の「段階」が2の診断、D063）。fixtureは1つずつが小さなプロジェクトで、期待する診断（段階0と1を含む）は `story/a.scn` の1行目の `// expect:` に書く。`expect` の無いものは正常系。テスト（`tests/playthrough.rs`）は、診断と自動巡回の結果（カバレッジ、エンディングごとの到達条件）をスナップショットにする。

| フォルダ | 内容 | 期待する診断 |
| --- | --- | --- |
| `ok_playthrough/` | どの条件も真になり、どの選択肢も選べる。永続の変数はエンディングで書いた値からもう一度始めるので、2周目にだけ選べる選択肢も選べる | |
| `unselectable/` | 着いても条件が真にならず、決して選べない選択肢（行き先はほかの選択肢からも着く） | W506 |
| `never_true/` | 評価されても真にならない `@if`・`@elif` の条件。評価されない条件には出さない | W507 |
| `stuck/` | 条件のせいで、選べる選択肢が1つも無くなる道がある（別の道では選べる） | W508 |
| `unreached_scene/` | 段階1では着けるが、条件のせいで着かないシーン。着いたシーンからの辺が実行されないシーンにだけ出す | W507 W509 |
| `foreshadow_bypass/` | 仕込みを通らずに回収へ着く道がある伏線。仕込みを必ず通る伏線には出さない | W704 |
| `foreshadow_replay/` | 回収へは、前の周で仕込みを見て立てた永続の変数が無いと行けない。前の周で通った仕込みも通ったに数えるので出さない。2周目でしか着かないエンディングの道の例は、前の周の道から（〔周回〕の印） | |
| `broken_condition/` | 構文の崩れた条件（E601）は偽として実行するが、W506・W507 は重ねて出さない | E601 |

## 書き出しのプラグイン（T3-14、`plugin/`）

`tests/plugin.rs` が使うテンプレート。サンプルの写しに置いて、両サンプルを書き出したスナップショットを取る。

| ファイル | 内容 |
| --- | --- |
| `scenes.md.j2` | シーンごとの台詞の一覧（キャラクターの名前、表示スタイルの括弧、`plain("narou")`、行ID）と、道ごとのシーンの数（`roads`）。プロファイルの決まっていないキー（`heading`）も読む |

## 表記の検査（T3-3、`wording/`）

`scn check --stage 2` の表記の検査（仕様書16節の I204〜I206、D064）。fixtureは1つずつが小さなプロジェクトで、期待する診断は `story/a.scn` の1行目の `// expect:` に書く。設定DBは、どのfixtureも同じ（灯：一人称 `わたし`、呼び名 `あかり`、避ける書き方 `燈`。遥人：一人称 `ぼく`、呼び名 `ハルト`。志乃：呼び名 `しの`。組織 北風高校：避ける書き方 `北風高`）。

| フォルダ | 内容 | 期待する診断 |
| --- | --- | --- |
| `ok_wording/` | 一人称は設定DBどおり。語の一部（`私立`・`ボクシング`・`下僕`）、ルビで読みを示した一人称（`｜私《わたくし》`）、登録した書き方（字下げの後ろ、伸ばした `ハルトーっ`）、ひらがなだけの書き方（`はると`）、名前の中の避ける書き方（`北風高校`）、別の語（`シノブ` と呼び名 `しの`）には出さない | |
| `first_person/` | 台詞の一人称の書き方が、設定DBの一人称と違う（`わたし` と `私`、`ぼく` と `僕`）。位置は語の区切りに合う現れ（`私立` の `私` ではない）。表示名の中は見ない。地の文には出さない | I204 |
| `variants/` | 設定DBの避ける書き方（`variants`）が、台詞・地の文・選択肢の表示文にある。台詞の形の地の文（閉じ括弧の後ろに文が続く）は括弧の外も調べる。名前の中の現れ（`北風高校`）には出さない | I205 |
| `kana_variant/` | 設定DBの名前・呼び名と、かなの書き方だけが違う、カタカナを含む書き方（`あかり` と `アカリ`、`しの` と `シノ`）。字下げの後ろでも1つだけ | I206 |
