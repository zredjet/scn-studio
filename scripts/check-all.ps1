# Rustとフロントエンドの検査をすべて走らせる（Windows用。macOSでは check-all を使う）。
# 1つでも失敗したら、そこで止まり、0以外の終了コードを返す。
# Windows PowerShell 5.1 で日本語を正しく読めるよう、このファイルはBOM付きのUTF-8で保存する。
#Requires -Version 5.1

# $ErrorActionPreference = 'Stop' は使わない。Windows PowerShell 5.1 では、
# 標準エラーをリダイレクトしたときにcargoの進捗表示まで失敗として扱われるため。
# 成否は各コマンドの終了コードで判定する。
Set-Location (Join-Path $PSScriptRoot '..') -ErrorAction Stop

function Invoke-Step {
    param(
        [string]$Name,
        [string]$Command,
        [string[]]$Arguments
    )
    Write-Host ''
    Write-Host "==> ${Name}"
    if (-not (Get-Command $Command -ErrorAction SilentlyContinue)) {
        Write-Host "失敗しました：${Name}（${Command} が見つかりません）"
        exit 1
    }
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        Write-Host "失敗しました：${Name}（終了コード ${LASTEXITCODE}）"
        exit $LASTEXITCODE
    }
}

Invoke-Step 'Rust：書式（cargo fmt --check）' 'cargo' @('fmt', '--all', '--', '--check')
Invoke-Step 'Rust：静的検査（clippy）' 'cargo' @('clippy', '--workspace', '--all-targets', '--', '-D', 'warnings')
Invoke-Step 'Rust：テスト（失敗しても残りのテストを走らせ、まとめて知らせる）' 'cargo' @('test', '--workspace', '--no-fail-fast')
Invoke-Step 'pnpm：依存の導入（lockfileは変えない）' 'pnpm' @('install', '--frozen-lockfile')
Invoke-Step 'フロント：書式と静的検査（Biome）' 'pnpm' @('run', 'lint')
Invoke-Step 'フロント：型検査（typecheck のあるパッケージ）' 'pnpm' @('run', 'typecheck')
Invoke-Step 'フロント：テスト（test のあるパッケージ）' 'pnpm' @('run', 'test')

Write-Host ''
Write-Host 'すべての検査に成功しました。'
