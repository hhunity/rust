# pandas, numpy, scikit-learn, pytorch(GPU/CUDA版), matplotlib, tqdm, rdkit,
# jupyter を、純粋なWindows環境(WSL/Docker不要)だけでダウンロードする
# スクリプト。doc/radonpy-windows-download.ps1 と同じ方式。
#
# 前提: Windows に Miniconda/Anaconda がインストール済みで `conda` が使える
#       (ネットに繋がっている環境で実行する)
#
# 実行後、以下をUSB等でオフラインのUbuntu 22.04(jammy)実機へコピーする:
#   - <miniconda>\pkgs\ 以下の *.conda / *.tar.bz2 と pkgs\cache\ (repodataキャッシュ)
#   - Miniforge3-Linux-x86_64.sh
#
# 実機側では doc/ml-conda-offline-install.sh を使ってオフライン構築する。
# (doc/ml-conda-urls-gpu.txt はダウンロードマネージャ等でURLを直接扱いたい
#  場合の代替手段。手作業でのフォルダ振り分けやmatplotlib-base化の回避策が
#  必要になるので、通常はこちらのスクリプトを使う方が簡単)

$ErrorActionPreference = "Stop"

conda config --remove channels defaults 2>$null
conda config --add channels conda-forge
conda config --set channel_priority strict

# ターゲットプラットフォームをlinux-64に固定してダウンロードのみ実行
$env:CONDA_SUBDIR = "linux-64"

# Windows上ではターゲット(Ubuntu 22.04 jammy)の実際のglibc/CPU情報を
# condaが自動取得できないため、明示的に指定する(doc/radonpy-windows-download.ps1
# と同じ理由)。
$env:CONDA_OVERRIDE_GLIBC = "2.35"
$env:CONDA_OVERRIDE_ARCHSPEC = "0"

# pytorch>=1.11はGPUドライバの有無を`__cuda`という仮想パッケージで判定する
# 仕様のため、GPUの無いWindows上で解決させるには明示的に「使える」と
# 教えてやる必要がある。CPU版で良い場合はこの行と、下のconda createの
# "pytorch=*=*cuda*" "cuda-version=13.3" の指定を外せばよい。
$env:CONDA_OVERRIDE_CUDA = "13.3"

conda create -n ml_dl -y --override-channels -c conda-forge --download-only `
  python=3.11 pandas numpy scikit-learn matplotlib tqdm rdkit jupyter `
  "pytorch=*=*cuda*" "cuda-version=13.3"

Remove-Item Env:CONDA_SUBDIR
Remove-Item Env:CONDA_OVERRIDE_GLIBC
Remove-Item Env:CONDA_OVERRIDE_ARCHSPEC
Remove-Item Env:CONDA_OVERRIDE_CUDA

# 実機にインストールするのはMiniforge(conda-forge専用ディストリビューション)。
Invoke-WebRequest -Uri "https://github.com/conda-forge/miniforge/releases/latest/download/Miniforge3-Linux-x86_64.sh" `
  -OutFile ".\Miniforge3-Linux-x86_64.sh"

$pkgsDir = (conda info --base) + "\pkgs"
Write-Host "=== 完了 ==="
Write-Host "以下をオフライン環境へコピーしてください:"
Write-Host "  - $pkgsDir  (中の*.conda等のパッケージファイルに加えて"
Write-Host "               pkgs\cache\ 以下のrepodataキャッシュも必須。"
Write-Host "               フォルダごとまるごとコピーすること)"
Write-Host "  - .\Miniforge3-Linux-x86_64.sh"
