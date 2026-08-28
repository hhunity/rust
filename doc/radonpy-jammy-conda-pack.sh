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
export PATH="/opt/miniforge3/bin:$PATH"

# condaディストリビューションはMiniconda(Anaconda社製)ではなく
# Miniforge(conda-forgeプロジェクト製)を使う。Minicondaはデフォルトで
# Anaconda社のdefaultsチャンネル(pkgs/main, pkgs/r)を前提にしており、
# これは利用規約上、一定規模以上の商用利用で有料ライセンスが必要になった。
# Miniforgeは最初からconda-forgeのみの構成なのでこの問題がそもそも発生しない。

echo "=== step: download miniforge ==="
if [ ! -f "$HOME/miniforge.sh" ]; then
  curl -fsSL -o "$HOME/miniforge.sh" \
    https://github.com/conda-forge/miniforge/releases/latest/download/Miniforge3-Linux-x86_64.sh
fi

echo "=== step: install miniforge ==="
if [ ! -d /opt/miniforge3 ]; then
  bash "$HOME/miniforge.sh" -b -p /opt/miniforge3
fi

source /opt/miniforge3/etc/profile.d/conda.sh

conda config --set always_yes true
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
