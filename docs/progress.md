# 進捗ログ

Claude Codeは、作業を始めるときにこのファイルを読み、タスクの区切りで更新する。新しい記録を上に足す。

## 現在の状態

- フェーズ：0（形式を固める）
- 作業中のタスク：なし
- 次のタスク：T0-2 形式仕様v0の確定
- 人の判断待ち：なし
- 別途対応：Windowsで `scripts/check-all.ps1` を実行し、成功するか確かめる（T0-1の完了条件のうち、Windows側）。Windowsの環境を用意しにくいため、T0-1とは切り離して後で行う（2026-10-07、人の判断）

## フェーズ1へ（フェーズ0では作らないが、気づいたこと）

- （まだない）

## 記録

### 2026-10-07 T0-1 完了（Windowsでの実行の確認を除く）

- やったこと
  - Cargoのワークスペース。メンバーは `crates/scn-core`（ライブラリ）と `crates/scn-cli`（実行ファイル名は `scn`）。どちらも中身は空の最小構成
  - `rust-toolchain.toml`、`rustfmt.toml`。editionは2024（Rust 1.99.0で選べる最新）
  - pnpmのワークスペース（`packages/*` と `app`、まだ空）、ルートの `package.json`、`biome.json`
  - `scripts/check-all`（macOS）と `scripts/check-all.ps1`（Windows、BOM付きUTF-8）
  - `.gitattributes`（改行をLFにそろえる）
  - `CLAUDE.md` のコマンドの節を、実際に動くコマンドだけに書き直した
  - それまでコミットが無かったので、準備資料を先に最初のコミット（`chore: 準備資料を追加`）にした
- 実行したコマンドと結果
  - `scripts/check-all`：成功（終了コード0）。Rustのテストは0件（まだテストする中身が無い）、clippyの警告0件、Biomeは2ファイル（`package.json`、`biome.json`）を検査して指摘0件、型検査とフロントのテストは対象のパッケージが0件
  - 作業用の複製で、書式・clippy・Rustのテスト・Biome・型検査・フロントのテストをそれぞれわざと壊し、その段階で止まって0以外の終了コードになることを確かめた（型検査とフロントのテストは、仮のパッケージを置いて確かめた）
  - `scn-core` の禁止lint：作業用の複製で、ライブラリに `unwrap`・添字アクセス・文字列のスライスを書くとclippyが失敗し、`#[cfg(test)]` の中では失敗しないことを確かめた
  - `CLAUDE.md` のコマンドの節に書いたコマンドを1つずつ実行し、すべて終了コード0
  - `scripts/check-all.ps1` は、このMacにPowerShellが無いため実行していない
- 決めたこと
  - TypeScriptの整形と静的検査はBiome 2.5.15。理由：整形、静的検査、importの整理が1つのツールと1つの設定で済み、ESLintとPrettierを組み合わせたときのような設定の食い違いが起きない。依存が少なく速い。ルートの `biome.json` で全体を一度に検査し、各パッケージには `lint` を置かない。`docs/`（画面モックのHTML）と `samples/`（作品データ）は対象から外す。版が上がると整形の結果が変わるので、版は固定する（`-E`）
  - Biomeの警告もエラーとして扱う（`biome check --error-on-warnings`）。Rustの `-D warnings` とそろえ、`.claude/rules/frontend.md` の「`any` を使わない」を検査で守らせるため（Biomeの `noExplicitAny` は既定で警告）。`plan-reviewer` の指摘で足した
  - Rustは1.99.0に固定（この日の最新の安定版）。理由：clippyの版によって警告が変わり、`-D warnings` の結果が人や環境でずれるのを防ぐ。上げるときは `rust-toolchain.toml` を変え、check-allが通ることを確かめる
  - pnpmは `package.json` の `packageManager` で12.9.1に固定（この日の最新の安定版）。別の版のpnpmから実行しても、この版に切り替わる
  - `scn-core` のライブラリのコードでは、`unwrap`・`expect`・`panic!`・添字アクセス・文字列のスライスをclippyで禁止した（`lib.rs` の先頭。テストは対象外）。`.claude/rules/rust.md` の約束を、検査で守らせるため
  - `.gitattributes` で改行をLFにそろえる。理由：Windowsで取り出しても、rustfmtとBiomeの書式の検査と、バイト位置を含むスナップショットの結果が変わらないようにする
  - check-allは `pnpm install --frozen-lockfile` も行う。新しい環境でもそのまま走らせられ、lockfileの更新忘れも見つけられるため
- 加えた依存：`@biomejs/biome`（開発用、理由は上のとおり）。crateは加えていない
- 残った課題
  - Windowsで `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\check-all.ps1` を実行し、成功するかを人が確かめる
    - 2026-10-07：Windowsの環境を用意しにくいため、別途対応とする（人の判断）。T0-7とT0-8、フェーズ0のゲート2もWindowsでの確認を含むので、それまでに環境が要る
  - T0-3への申し送り：仕様（1節）ではCRLFも受け付ける。CRLFのfixtureを置くときは、`.gitattributes` でそのファイルだけ `-text` を指定する（指定しないとLFに変換される）
- 次のタスク：T0-2

### 2026-10-07 準備

- 設計検討書、形式仕様v0のドラフト、用語集、決定記録、画面モック、サンプル作品を用意した。
- 次は T0-1。`/phase-task T0-1` で始める。

<!--
記録の書き方の例：

### 2026-10-08 T0-1 完了
- やったこと：Cargoとpnpmのワークスペース、check-all スクリプト
- 実行したコマンドと結果：scripts/check-all 成功（Rustテスト 2件、警告 0）
- 決めたこと：TypeScriptの検査は Biome（理由：整形と検査が1つで済む）
- 残った課題：なし
- 次のタスク：T0-2
-->
