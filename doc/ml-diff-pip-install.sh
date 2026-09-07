#!/bin/bash
# doc/ml-diff-pip-download.ps1 でWindows側から集めたwheel一式を使い、
# 既に構築済みのRadonPy用conda環境(env: radonpy)に、
# scikit-learn / tqdm / jupyter / torch だけを追加でオフライン導入する。
#
# 前提:
#   - doc/radonpy-offline-install.sh で env: radonpy が既に構築済みであること
#   - このスクリプトと同じ場所に wheels/ (Windows側の.\wheelsをコピーしたもの)
#     があること
#
# 使い方:
#   bash ml-diff-pip-install.sh
#
# pip経由でのインストールのため、condaが管理しているpsi4/lammps/mkl/numpy/
# scipy等の既存パッケージには一切触れない(pipは対象環境で条件を満たす
# パッケージが既にあればそれをそのまま使い、上書きしない)。

set -euxo pipefail

cd "$(dirname "$0")"

export PATH="$HOME/miniforge3/bin:$PATH"
source "$HOME/miniforge3/etc/profile.d/conda.sh"

echo "=== step: install diff packages via pip (--no-index, offline) ==="
conda run -n radonpy pip install --no-index --find-links=./wheels \
  scikit-learn tqdm jupyter torch

echo "=== DONE ==="
echo "conda activate radonpy の中で import pandas, sklearn, torch 等が使えます"
