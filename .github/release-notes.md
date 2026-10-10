分岐する物語（ADV・ビジュアルノベル）のための執筆ツール scn-studio の {{VERSION}} です。開発中の版で、名前は仮です。使い方は [README](https://github.com/zredjet/scn-studio#readme) と [アプリの使い方](https://github.com/zredjet/scn-studio/blob/main/docs/guide/app.md) を見てください。

> [!IMPORTANT]
> **インストーラーはなく、署名もしていません。** ZIP を展開してそのまま使います。初めて開くときに、OS が「開発元を確認できない」と警告します。開き方は下の「macOS で開く」「Windows で開く」を見てください。

## ダウンロード

| ファイル | 中身 |
| --- | --- |
| `scn-studio_{{VERSION}}_macos_arm64.zip` | アプリ（macOS、Apple Silicon。Intel の Mac では動きません） |
| `scn-studio_{{VERSION}}_windows_x64.zip` | アプリ（Windows、x64） |
| `scn-studio_{{VERSION}}_windows_arm64.zip` | アプリ（Windows、ARM64） |
| `scn_{{VERSION}}_macos_arm64.zip`・`scn_{{VERSION}}_windows_x64.zip`・`scn_{{VERSION}}_windows_arm64.zip` | コマンド `scn`（検証・行IDの付与・書き出しなど。[使い方](https://github.com/zredjet/scn-studio/blob/main/docs/guide/scn.md)） |
| `scn-studio-vscode_{{VERSION}}.vsix` | VS Code 拡張（ハイライトと言語サーバー） |
| `SHA256SUMS.txt` | 各ファイルの SHA-256 |

Windows の x64 と ARM64 は、「設定 → システム → バージョン情報」の「システムの種類」で分かります（「ARM ベース プロセッサ」なら ARM64）。

## macOS で開く

1. ZIP を展開し、`scn-studio.app` を「アプリケーション」フォルダに移します（移さなくても動きます）。
2. `scn-studio.app` をダブルクリックします。「"scn-studio" は開いていません」（「Apple は…マルウェアが含まれていないことを検証できませんでした」）と出たら、「完了」を押して閉じます。
3. 「システム設定 → プライバシーとセキュリティ」を開き、下の方にある「"scn-studio" は…ブロックされました」の横の「このまま開く」を押します。パスワードを入れ、出てきた確認で「このまま開く」を押します。
4. 2回目からは、ふつうにダブルクリックで開けます。

「壊れているため開けません」と出たときや、上の手順で開けないときは、ターミナルで次を実行してから開きます（ダウンロードした印を外します）。

```sh
xattr -dr com.apple.quarantine /Applications/scn-studio.app
```

コマンド `scn` は、展開したフォルダで `xattr -d com.apple.quarantine scn` を実行してから、`PATH` の通ったフォルダ（`~/.local/bin` など）に置きます。macOS 11 以降の Apple Silicon の Mac で動きます。

## Windows で開く

1. （任意）ZIP を右クリック →「プロパティ」→ 下の「セキュリティ: 許可する」にチェックを入れて「OK」。展開したファイルの警告が減ります。
2. ZIP を展開し、`scn-studio.exe` をダブルクリックします。
3. 「Windows によって PC が保護されました」と出たら、「詳細情報」を押し、「実行」を押します。

- 画面の表示に Microsoft Edge WebView2 ランタイムを使います。Windows 11 には入っています。Windows 10 で起動しないときは、Microsoft の「WebView2 ランタイム（Evergreen）」を入れてください。
- 「スマート アプリ コントロール」が有効な Windows 11 では、署名の無いアプリは起動できません。
- コマンド `scn.exe` は、`PATH` の通ったフォルダに置くか、フォルダごと `PATH` に足します。
- Windows では、アプリの操作と日本語入力をまだ実機で確かめていません。困ったことがあれば Issue で知らせてください。

## VS Code 拡張

```sh
code --install-extension scn-studio-vscode_{{VERSION}}.vsix
```

言語サーバーにコマンド `scn` を使います。`scn` が `PATH` に無ければ、設定 `scn.path` に絶対パスを書きます。

## はじめて使うとき

- サンプル作品「放課後の灯」は、このリリースの「Source code」の ZIP の `samples/houkago/` にあります。アプリの「作品フォルダを開く」で開けます。
- 新しい作品は、アプリの「新しく作る」か `scn new <フォルダ>` で作ります。
- AI の支援は使わなくてもかまいません。使うときは、作品フォルダの `project.toml` に接続先を書き、アプリの「環境設定 → AIの接続」でキーや Claude Code（CLI）を設定します。

## 制限

- 自動更新はありません。新しい版は、このページから同じ手順で入れ直します。
- 書体は同梱していません。OS の明朝体とゴシック体を使います。
