# 既にRadonPy用のconda環境(env: radonpy, doc/Dockerfile.radonpy方式 =
# Python 3.13)が構築済みの実機に対して、
# scikit-learn / tqdm / jupyter / torch(PyPI版、CUDA同梱wheel)
# だけを"差分"として追加したい場合のダウンロードスクリプト。
#
# なぜこの方式が必要か:
#   pandas/numpy/rdkit/matplotlibはdoc/Dockerfile.radonpy(または
#   doc/radonpy-offline-install.sh)で既に入っている。残り
#   (scikit-learn/tqdm/jupyter/torch)をcondaで追加しようとすると、
#   GPU版pytorchが要求する新しいmklにpsi4/lammpsも巻き込まれて
#   バージョンアップ・再ビルドされてしまう(実機相当の環境で確認済み)。
#   pipでインストールすればcondaの依存関係解決を経由しないため、
#   psi4/lammps/mkl/numpy/scipy等の既存condaパッケージには一切触れずに
#   追加できる(実際にpip install前後でconda list出力が完全一致することを
#   python=3.13 + psi4=1.10 の環境で確認済み)。
#
# 前提: Windows に Python(3.13)がインストール済みで、ネットに繋がっている
#       (Miniconda/Anacondaのpythonでもよい)。
#       実機のradonpy環境がpython=3.11で構築されている場合(古い
#       doc/radonpy-offline-install.sh方式)は、下記の
#       --python-version / --abi を 311 / cp311 に書き換えること。
#       `conda activate radonpy && python --version` で確認できる。
#
# 実行後、生成される wheels フォルダをUSB等でオフラインのLinux実機へコピーする。
# 実機側では doc/ml-diff-pip-install.sh を使ってオフライン導入する。
#
# 注意点(ハマったポイント):
# - torchのバージョンを明示指定しないと、pipが全バージョンを総当たりして
#   矛盾(ResolutionImpossible)を起こすことがある(新しめのtorchが要求する
#   nvidia-cudnn-cu13が現時点ではPyPIにプレースホルダーしか存在しない等)。
#   このため以下ではtorchのバージョンを2.10.0に固定している
#   (CUDA 12.8系ランタイムを同梱)。より新しいtorchを試したい場合は
#   バージョン番号を変えてよいが、その場合`nvidia-cudnn-cu13`等の
#   実体が存在するか事前に確認すること。
# - torch本体は新しめのmanylinux_2_28タグでビルドされているが、依存する
#   nvidia-*-cu12パッケージ群は古いmanylinux2014/manylinux_2_17/
#   manylinux_2_27等バラバラのタグを使っているため、--platformは
#   複数列挙する必要がある(1つだけ指定すると解決できるパッケージが
#   見つからずエラーになる)。
# - doc/Dockerfile.ml-gpu / doc/ml-conda-windows-download.ps1のconda版
#   はCUDA 13.3系だが、こちらのpip版はCUDA 12.8系。系統が異なる点に注意
#   (実機のNVIDIAドライバは新しいCUDAランタイムにも後方互換があるため、
#   13.3対応ドライバならこちらも動作するはず)。

$ErrorActionPreference = "Stop"

New-Item -ItemType Directory -Force -Path .\wheels | Out-Null

python -m pip download --no-cache-dir --timeout 300 --retries 8 `
  --only-binary=:all: --python-version 313 --implementation cp --abi cp313 `
  --platform manylinux_2_28_x86_64 --platform manylinux_2_27_x86_64 `
  --platform manylinux_2_26_x86_64 --platform manylinux_2_24_x86_64 `
  --platform manylinux_2_17_x86_64 --platform manylinux2014_x86_64 `
  --platform manylinux2010_x86_64 --platform manylinux1_x86_64 `
  --platform manylinux_2_12_x86_64 `
  -d .\wheels `
  "torch==2.10.0" scikit-learn tqdm jupyter

Write-Host "=== 完了 ==="
Write-Host ".\wheels フォルダをUSB等でLinux実機へコピーしてください"
