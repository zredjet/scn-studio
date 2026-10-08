# vscode-scn（VS Code 拡張。T3-16）

`.scn` を VS Code で書くための拡張。配布はしない（手元でパッケージにして入れる）。

- ハイライト：`syntaxes/scn.tmLanguage.json`（TextMate の文法。行の種類は `packages/lezer-scn` の文法と同じに分ける。テストが Lezer の期待値と突き合わせる）
- 言語サーバー：`scn lsp`（`crates/scn-lsp`）を起動してつなぐ。診断、補完、定義へ移動、参照、名前の変更、ホバー
- 行ID（行末の `^xxxx`）：薄く（既定）・隠す（カーソルのある行は見せる）・そのまま。コマンド「scn: 行IDの見せ方を切り替える」か設定 `scn.lineIds`

## 設定

| 設定 | 既定 | 意味 |
| --- | --- | --- |
| `scn.path` | `scn` | 言語サーバーに使う `scn` のコマンド。`PATH` に無ければ絶対パス（`cargo install --locked --path crates/scn-cli` で `~/.cargo/bin/scn` に入る。macOS で Dock から開いた VS Code は `PATH` が短いので、絶対パスで書く） |
| `scn.lineIds` | `dim` | 行IDの見せ方（`dim`・`hide`・`show`） |

作品フォルダ（`project.toml` のあるフォルダ）を VS Code のフォルダとして開く。言語サーバーは1つの作品フォルダを扱う。

## 作る・確かめる

```sh
pnpm -C packages/vscode-scn typecheck    # 型検査と、out/extension.cjs へのまとめ（esbuild。scripts/check-all に入っている）
pnpm -C packages/vscode-scn test         # 文法と行IDのテスト（scripts/check-all に入っている）
cargo build -p scn-cli && pnpm -C packages/vscode-scn test:vscode   # 拡張の起動のテスト（VS Code の窓が短い間開く）
```

拡張の起動のテストは、環境変数 `VSCODE_EXECUTABLE`（VS Code の実行ファイル）が無ければ、macOS の `/Applications/Visual Studio Code.app` を使い、それも無ければ安定版をダウンロードする。`scn` は環境変数 `SCN_BIN` か、リポジトリの `target/debug/scn`。

## パッケージにして入れる

```sh
pnpm -C packages/vscode-scn build
cd packages/vscode-scn
npx @vscode/vsce package --no-dependencies --skip-license --allow-missing-repository
code --install-extension vscode-scn-0.1.0.vsix
```

`--no-dependencies` は、依存（`vscode-languageclient`）を `out/extension.cjs` にまとめてあるため。

開発中は、VS Code でこのフォルダを開き、「実行とデバッグ」で拡張の開発ホストを起動してもよい（`launch.json` は置いていない。「拡張機能の開発ホスト」を選ぶ）。
