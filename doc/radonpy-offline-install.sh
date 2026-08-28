#!/bin/bash
# doc/radonpy-windows-download.ps1 でWindows側から集めたファイル一式を使い、
# オフラインのUbuntu 22.04(jammy)実機上でRadonPy用conda環境を構築する。
#
# 前提: 以下がこのスクリプトと同じ場所に用意されていること
#   - Miniconda3-latest-Linux-x86_64.sh
#   - pkgs/  (Windows側の <miniconda>\pkgs をコピーしたディレクトリ)
#   - radonpy_wheels/
#
# 使い方:
#   bash radonpy-offline-install.sh
#
# 実行後、`conda activate radonpy` でそのまま使える(ネットワーク不要で
# ここまで構築済み)。

set -euxo pipefail

cd "$(dirname "$0")"

echo "=== step: install miniconda (offline) ==="
if [ ! -d "$HOME/miniconda3" ]; then
  bash ./Miniconda3-latest-Linux-x86_64.sh -b -p "$HOME/miniconda3"
fi

export PATH="$HOME/miniconda3/bin:$PATH"
source "$HOME/miniconda3/etc/profile.d/conda.sh"

echo "=== step: import downloaded package cache ==="
# pkgs/ 直下のパッケージファイル(*.conda / *.tar.bz2)だけでなく、
# pkgs/cache/ 以下のリポジトリメタデータ(repodata)も一緒にコピーしないと
# --offline での依存解決自体ができない点に注意。
cp -an ./pkgs/. "$HOME/miniconda3/pkgs/"

conda config --set always_yes true
conda config --remove channels defaults || true
conda config --add channels conda-forge
conda config --set channel_priority strict

echo "=== step: create radonpy env from local cache only (--offline) ==="
conda create -n radonpy -y --offline --override-channels -c conda-forge -c psi4 \
  python=3.11 rdkit psi4 dftd3-python resp mdtraj psutil scipy pandas matplotlib pip lammps

echo "=== step: install radonpy body (from local wheel, --no-index) ==="
conda run -n radonpy pip install --no-index --find-links=./radonpy_wheels radonpy-pypi

echo "=== DONE ==="
echo "conda activate radonpy  で利用できます"
