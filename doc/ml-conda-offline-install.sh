#!/bin/bash
# doc/ml-conda-windows-download.ps1 でWindows側から集めたファイル一式を使い、
# オフラインのUbuntu 22.04(jammy)実機上でpandas/numpy/scikit-learn/
# pytorch(GPU)/matplotlib/tqdm/rdkit/jupyter用conda環境を構築する。
# doc/radonpy-offline-install.sh と同じ方式。
#
# 前提: 以下がこのスクリプトと同じ場所に用意されていること
#   - Miniforge3-Linux-x86_64.sh
#   - pkgs/  (Windows側の <miniforge3>\pkgs をコピーしたディレクトリ)
#
# 使い方:
#   bash ml-conda-offline-install.sh
#
# 実行後、`conda activate ml` でそのまま使える(ネットワーク不要でここまで
# 構築済み)。GPUを使う場合は対象機にNVIDIAドライバ(CUDA 13.3以上対応)が
# 別途必要。

set -euxo pipefail

cd "$(dirname "$0")"

echo "=== step: install miniforge (offline) ==="
if [ ! -d "$HOME/miniforge3" ]; then
  bash ./Miniforge3-Linux-x86_64.sh -b -p "$HOME/miniforge3"
fi

export PATH="$HOME/miniforge3/bin:$PATH"
source "$HOME/miniforge3/etc/profile.d/conda.sh"

echo "=== step: import downloaded package cache ==="
# pkgs/ 直下のパッケージファイル(*.conda / *.tar.bz2)だけでなく、
# pkgs/cache/ 以下のリポジトリメタデータ(repodata)も一緒にコピーしないと
# --offline での依存解決自体ができない点に注意。
cp -an ./pkgs/. "$HOME/miniforge3/pkgs/"

conda config --set always_yes true
conda config --set channel_priority strict

echo "=== step: create ml env from local cache only (--offline) ==="
conda create -n ml -y --offline --override-channels -c conda-forge \
  python=3.11 pandas numpy scikit-learn matplotlib tqdm rdkit jupyter pytorch

echo "=== DONE ==="
echo "conda activate ml  で利用できます"
