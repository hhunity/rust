#!/bin/bash
# Ubuntu 22.04 (jammy) 上で RadonPy用のconda環境を作り、conda-packで
# 1個のtarballに固める再現スクリプト。
#
# 想定実行環境: Ubuntu 22.04 (jammy) の実機 / VM / chroot / コンテナ
#   (debootstrapでjammyのrootfsを作ってchroot実行してもよい。
#    その場合はネットワーク・DNS・/proc,/sys のbind mountを別途用意すること)
#
# 使い方:
#   sudo bash radonpy-jammy-conda-pack.sh
#
# 出力: $HOME/radonpy_env_jammy22.04.tar.gz
#
# なぜjammyで固めるか:
#   conda-forge製バイナリはlibstdc++等は同梱するが、glibcだけはOS側のものを
#   動的リンクして使う。新しいOS(新しいglibc)で固めた環境を古いOSへ持って
#   いくと `GLIBC_2.XX not found` で動かないことがあるため、配布先と同じ
#   (または配布先以下の)glibcバージョンの環境で固めるのが安全。

set -euxo pipefail

export HOME="${HOME:-/root}"
export PATH="/opt/miniconda3/bin:$PATH"

echo "=== step: download miniconda ==="
if [ ! -f "$HOME/miniconda.sh" ]; then
  curl -fsSL -o "$HOME/miniconda.sh" \
    https://repo.anaconda.com/miniconda/Miniconda3-latest-Linux-x86_64.sh
fi

echo "=== step: install miniconda ==="
if [ ! -d /opt/miniconda3 ]; then
  bash "$HOME/miniconda.sh" -b -p /opt/miniconda3
fi

source /opt/miniconda3/etc/profile.d/conda.sh

conda config --set always_yes true

# defaults (pkgs/main, pkgs/r) はAnaconda社のToS同意が必要になったため使わず、
# conda-forgeのみを使う。
conda config --remove channels defaults || true
conda config --add channels conda-forge
conda config --set channel_priority strict

echo "=== step: create radonpy env (python 3.11) ==="
conda create -n radonpy python=3.11 --override-channels -c conda-forge -y

echo "=== step: install RadonPy dependencies ==="
# psi4は専用チャンネル(psi4)からの提供。RadonPyの推奨インストール手順に準拠。
conda install -n radonpy --override-channels -c conda-forge -c psi4 \
  rdkit psi4 dftd3-python resp mdtraj psutil scipy pandas matplotlib pip lammps -y

echo "=== step: pip install radonpy body ==="
conda run -n radonpy pip install radonpy-pypi

echo "=== step: install conda-pack (in base env) ==="
conda install -n base --override-channels -c conda-forge conda-pack -y

echo "=== step: conda-pack ==="
OUT="$HOME/radonpy_env_jammy22.04.tar.gz"
conda run -n base conda-pack -n radonpy -o "$OUT"

echo "=== DONE ==="
ls -la "$OUT"
echo "オフライン環境側では以下で展開・有効化できる:"
echo "  mkdir radonpy_env && tar xzf $(basename "$OUT") -C radonpy_env"
echo "  source radonpy_env/bin/activate"
echo "  conda-unpack"
