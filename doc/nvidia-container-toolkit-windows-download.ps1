# NVIDIA Container Toolkit (WSL2 Ubuntu 22.04 jammy向け) のdebパッケージ一式を、
# 純粋なWindows環境(Ubuntu/WSLのネット接続不要)だけでダウンロードするスクリプト。
#
# 背景: 通常の手順は「ネットに繋がるUbuntu環境でapt-get install --print-uris
# を実行してURL一覧を作る」だが、WSL自体がオフラインでほかにUbuntu環境も
# 無い場合はこれができない。NVIDIAのAPTリポジトリはaptを経由しなくても、
# リポジトリのインデックスファイル(Packages)を直接HTTPで読めば中身が
# 分かる(プレーンテキストのDebianパッケージインデックス形式)ため、
# Windows(PowerShell)だけで完結できる。
#
# 前提: このスクリプトを実行するWindows機はネットに繋がっている必要がある
#       (WSL側がオフラインでも問題ない。ダウンロードしたファイルをUSB等で
#       WSLへ後から転送する)
#
# 実行後、生成される .\nvidia-container-toolkit-offline-debs\ フォルダを
# USB等でオフラインのWSL(Ubuntu 22.04 jammy)へコピーし、以下でインストール:
#   cd nvidia-container-toolkit-offline-debs
#   sudo apt-get install ./*.deb
#
# 依存関係(libc6, libseccomp2等)はUbuntu 22.04であれば標準で入っているため
# 通常は追加ダウンロード不要。「dependency problems」で足りないと言われた
# 場合は、doc/rust-urls-jammy.txt (Ubuntu標準パッケージのURL一覧、既に
# 生成済み)から該当パッケージを探して追加すること。

$ErrorActionPreference = "Stop"

# NVIDIA Container ToolkitのAPTリポジトリ(amd64向け)。
# https://nvidia.github.io/libnvidia-container/stable/deb/nvidia-container-toolkit.list
# に記載されている実体のリポジトリURL。
$repoBase = "https://nvidia.github.io/libnvidia-container/stable/deb"
$packagesUrl = "$repoBase/amd64/Packages"

# 欲しいパッケージ本体。依存関係(nvidia-container-toolkit-base,
# libnvidia-container-tools, libnvidia-container1)はnvidia-container-toolkit
# パッケージが要求するものなので、念のため全部明示的に対象にしておく
# (Packagesファイル中に無い名前は単に無視される)。
$targets = @(
    "nvidia-container-toolkit",
    "nvidia-container-toolkit-base",
    "libnvidia-container-tools",
    "libnvidia-container1"
)

Write-Host "Fetching package index: $packagesUrl"
$raw = (Invoke-WebRequest -Uri $packagesUrl -UseBasicParsing).Content

# DebianのPackagesファイルは空行区切りで1パッケージ1エントリ。
# 同じ名前で複数バージョンが並んでいることがあるが、通常は新しい版が
# 先に出てくるため最初のヒットを採用する。
$entries = $raw -split "`r?`n`r?`n"

$outDir = ".\nvidia-container-toolkit-offline-debs"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

foreach ($pkgName in $targets) {
    $entry = $entries | Where-Object { $_ -match "(?m)^Package:\s*$([regex]::Escape($pkgName))\s*$" } | Select-Object -First 1
    if (-not $entry) {
        Write-Warning "パッケージが見つかりませんでした(リポジトリ構成が変わった可能性): $pkgName"
        continue
    }
    if ($entry -notmatch "(?m)^Filename:\s*(.+?)\s*$") {
        Write-Warning "Filenameフィールドが見つかりませんでした: $pkgName"
        continue
    }
    $filename = $matches[1] -replace "^\./", ""
    # FilenameはPackagesファイルの場所(amd64/)からの相対パスで書かれている
    $url = "$repoBase/amd64/$filename"
    $outFile = Join-Path $outDir (Split-Path $filename -Leaf)
    Write-Host "Downloading: $url"
    Invoke-WebRequest -Uri $url -OutFile $outFile
}

Write-Host "=== 完了 ==="
Write-Host "以下をUSB等でオフラインのWSL(Ubuntu 22.04 jammy)へコピーしてください:"
Write-Host "  $outDir"
