# RadonPy用のLinux(linux-64)向けconda/pipパッケージを、純粋なWindows環境
# (WSL/Docker不要)だけでダウンロードするスクリプト。
#
# 前提: Windows に Miniconda/Anaconda がインストール済みで `conda` が使える
#       (ネットに繋がっている環境で実行する)
#
# 実行後、以下をUSB等でオフラインのUbuntu 22.04(jammy)実機へコピーする:
#   - <miniconda>\pkgs\ 以下の *.conda / *.tar.bz2 (このスクリプトでDLしたもの)
#   - Miniconda3-latest-Linux-x86_64.sh
#   - radonpy_wheels\ フォルダ
#
# 実機側では doc/radonpy-offline-install.sh を使ってオフライン構築する。
#
# 仕組み: conda は CONDA_SUBDIR 環境変数でターゲットのプラットフォーム
# (ここでは linux-64) を指定して依存関係を解決・ダウンロードできる。
# --download-only を付けるとファイルをキャッシュに落とすだけでlink/install
# はしない(=Windows上でLinuxバイナリを実行しようとしてエラーになることはない)。
# この組み合わせはLinux上でwin-64パッケージをダウンロードする形で動作検証済み。

$ErrorActionPreference = "Stop"

# defaults(pkgs/main, pkgs/r)はAnaconda社のToS同意が必要になったため使わず、
# conda-forgeのみを使う。
conda config --remove channels defaults 2>$null
conda config --add channels conda-forge
conda config --set channel_priority strict

# ターゲットプラットフォームをlinux-64に固定してダウンロードのみ実行
$env:CONDA_SUBDIR = "linux-64"

# 重要: Windows上ではターゲット(Ubuntu 22.04 jammy)の実際のglibc/CPU情報を
# condaが自動取得できない(Windowsなので当然)。指定しないと、Windows実行時に
# たまたまconda側が想定するデフォルト値(実機より新しいglibcを前提にする等)で
# 依存解決してしまい、選ばれたパッケージが実機のjammy(glibc 2.35系)で
# `GLIBC_2.xx not found` のように動かない可能性がある。
# 必ずjammyの実際のglibcバージョンに合わせて明示指定すること。
$env:CONDA_OVERRIDE_GLIBC = "2.35"
# CPU依存の最適化ビルドを避け、より汎用的なビルドを選ばせることで
# 実機のCPUと異なっていても動く可能性を上げる(0=最適化なしを強制)。
$env:CONDA_OVERRIDE_ARCHSPEC = "0"

conda create -n radonpy_dl -y --override-channels -c conda-forge -c psi4 --download-only `
  python=3.11 rdkit psi4 dftd3-python resp mdtraj psutil scipy pandas matplotlib pip lammps

# 補足: mkl版blasが複数バージョン重複してダウンロードされ肥大化しやすいので、
# 容量を減らしたい場合は下記のようにopenblas版を明示指定するとよい:
#   conda create -n radonpy_dl -y --override-channels -c conda-forge -c psi4 --download-only `
#     "libblas=*=*openblas" python=3.11 rdkit psi4 dftd3-python resp mdtraj psutil scipy pandas matplotlib pip lammps

Remove-Item Env:CONDA_SUBDIR
Remove-Item Env:CONDA_OVERRIDE_GLIBC
Remove-Item Env:CONDA_OVERRIDE_ARCHSPEC

# RadonPy本体(pure Pythonのwheel。プラットフォーム非依存なのでWindows上でDL可)
pip download radonpy-pypi --no-deps -d .\radonpy_wheels

# Miniconda Linuxインストーラ本体も取得しておく
Invoke-WebRequest -Uri "https://repo.anaconda.com/miniconda/Miniconda3-latest-Linux-x86_64.sh" `
  -OutFile ".\Miniconda3-latest-Linux-x86_64.sh"

$pkgsDir = (conda info --base) + "\pkgs"
Write-Host "=== 完了 ==="
Write-Host "以下をオフライン環境へコピーしてください:"
Write-Host "  - $pkgsDir  (中の *.conda 等のパッケージファイルに加えて"
Write-Host "               pkgs\cache\ 以下のrepodataキャッシュも必須。"
Write-Host "               フォルダごとまるごとコピーすること)"
Write-Host "  - .\Miniconda3-latest-Linux-x86_64.sh"
Write-Host "  - .\radonpy_wheels\"
