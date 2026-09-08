#!/bin/bash
# doc/ml-diff-pip-download.ps1 でWindows側から集めたwheel一式を使い、
# 既に構築済みのRadonPy用conda環境(env: radonpy)に、
# scikit-learn / tqdm / jupyter / torch だけを追加でオフライン導入する。
#
# 前提:
#   - env: radonpy が既に構築済みであること(doc/Dockerfile.radonpy方式=
#     python=3.13 でも、doc/radonpy-offline-install.sh方式=python=3.11 でも
#     どちらでもよいが、wheels/がそのPythonバージョン向けに正しく
#     ダウンロードされていること。doc/ml-diff-pip-download.ps1側の
#     コメント参照)
#   - このスクリプトと同じ場所に wheels/ (Windows側の.\wheelsをコピーしたもの)
#     があること
#
# 使い方:
#   bash ml-diff-pip-install.sh
#
# pip経由でのインストールのため、condaが管理しているpsi4/lammps/mkl/numpy/
# scipy等の既存パッケージには基本的に触れない。ただし jupyter が要求する
# pyzmq / ipykernel はバージョン制約次第でpipが新しいものに引き上げて
# しまうことがあり、その場合pipホイール版のpyzmq(libzmqを静的バンドル)が
# conda-forge版を上書きし、Jupyterカーネルがcell実行時にZMQ通信で
# ハングする不具合を引き起こすことが判明している(素のpythonは正常動作)。
# そのため最後にpyzmq/ipykernelをconda版へ強制的に戻す。

set -euxo pipefail

cd "$(dirname "$0")"

export PATH="$HOME/miniforge3/bin:$PATH"
source "$HOME/miniforge3/etc/profile.d/conda.sh"

echo "=== step: install diff packages via pip (--no-index, offline) ==="
conda run -n radonpy pip install --no-index --find-links=./wheels \
  scikit-learn tqdm jupyter torch

echo "=== step: restore conda-managed pyzmq/ipykernel if pip overwrote them ==="
conda run -n radonpy pip uninstall -y pyzmq ipykernel
conda install -n radonpy pyzmq ipykernel --offline --force-reinstall -y

echo "=== DONE ==="
echo "conda activate radonpy の中で import pandas, sklearn, torch 等が使えます"
