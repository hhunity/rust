# doc/ml-conda-urls-gpu.txt のURL一覧を、URLに含まれる linux-64/noarch の
# 区分どおりに自動でフォルダ分けしながらダウンロードするスクリプト。
#
# 使い方: このファイルと同じ場所に ml-conda-urls-gpu.txt を置いて実行
#   PS> .\ml-conda-windows-download.ps1
#
# 実行後、生成される local-channel フォルダを丸ごとUSB等でLinux実機へ
# コピーし、doc/readme.md の「オフライン環境(Linux)での反映のしかた」
# の手順を続ける。

$ErrorActionPreference = "Stop"

$urlFile = Join-Path $PSScriptRoot "ml-conda-urls-gpu.txt"
$outDir  = Join-Path $PSScriptRoot "local-channel"

New-Item -ItemType Directory -Force -Path (Join-Path $outDir "linux-64") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $outDir "noarch")  | Out-Null

$urls = Get-Content $urlFile | Where-Object { $_ -match '^https?://' }
$total = $urls.Count
$i = 0

foreach ($url in $urls) {
    $i++
    if ($url -match '/linux-64/') {
        $subdir = "linux-64"
    } elseif ($url -match '/noarch/') {
        $subdir = "noarch"
    } else {
        Write-Warning "linux-64/noarchのどちらか判定できないのでスキップ: $url"
        continue
    }

    $fileName = Split-Path $url -Leaf
    $dest = Join-Path (Join-Path $outDir $subdir) $fileName

    if (Test-Path $dest) {
        Write-Host "[$i/$total] skip (already exists): $fileName"
        continue
    }

    Write-Host "[$i/$total] downloading ($subdir): $fileName"
    Invoke-WebRequest -Uri $url -OutFile $dest
}

Write-Host "=== 完了 ==="
Write-Host "$outDir フォルダをUSB等でLinux実機へコピーしてください"
