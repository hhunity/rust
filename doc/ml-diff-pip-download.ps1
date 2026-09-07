# 既にRadonPy用のconda環境(env: radonpy)が構築済みの実機に対して、
# scikit-learn / tqdm / jupyter / torch(GPU非対応、PyPI版CUDA同梱wheel)
# だけを"差分"として追加したい場合のダウンロードスクリプト。
#
# なぜこの方式が必要か:
#   pandas/numpy/rdkit/matplotlibはdoc/radonpy-offline-install.shで既に
#   入っている。残り(scikit-learn/tqdm/jupyter/torch)をcondaで追加しようと
#   すると、GPU版pytorchが要求する新しいmklにpsi4/lammpsも巻き込まれて
#   バージョンアップ・再ビルドされてしまう(実機で確認済み)。
#   pipでインストールすればcondaの依存関係解決を経由しないため、
#   psi4/lammps/mkl/numpy/scipy等の既存condaパッケージには一切触れずに
#   追加できる(実際にpip install前後でconda list出力が完全一致することを
#   確認済み)。
#
# 前提: Windows に Python(3.11)がインストール済みで、ネットに繋がっている
#       (Miniconda/Anacondaのpythonでもよい)
#
# 実行後、生成される wheels フォルダをUSB等でオフラインのLinux実機へコピーする。
# 実機側では doc/ml-diff-pip-install.sh を使ってオフライン導入する。
#
# 注意: このtorchはPyPI版(CUDA 12.4系ランタイムをnvidia-*-cu12パッケージとして
# 同梱)。doc/Dockerfile.ml-gpu / doc/ml-conda-windows-download.ps1のconda版
# (CUDA 13.3系)とはCUDAバージョン系統が異なる点に注意。実機のNVIDIAドライバが
# CUDA 12.4以上に対応していれば問題なく動く(ドライバは基本的に新しいCUDAランタイム
# にも後方互換があるため、13.3対応ドライバならこちらも動作するはず)。

$ErrorActionPreference = "Stop"

New-Item -ItemType Directory -Force -Path .\wheels | Out-Null

# Linux(manylinux)/Python 3.11向けのwheelだけを対象に、依存関係も含めて
# まとめてダウンロードする(--platformでWindows上からでもLinux用を取得できる)。
python -m pip download --no-cache-dir --timeout 120 --retries 5 `
  --only-binary=:all: --python-version 311 --implementation cp --abi cp311 `
  --platform manylinux_2_17_x86_64 --platform manylinux2014_x86_64 `
  -d .\wheels `
  scikit-learn tqdm jupyter torch

Write-Host "=== 完了 ==="
Write-Host ".\wheels フォルダをUSB等でLinux実機へコピーしてください"
