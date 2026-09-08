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
#   矛盾(ResolutionImpossible)を起こすことがある。バージョン固定が必須。
#   以下ではtorch==2.14.0(CUDA 13.0系ランタイム同梱、"+cu130"ビルド)に
#   固定している。実機のドライバがCUDA 12.x世代までしか対応していない
#   場合は "torch==2.10.0" (CUDA 12.8系)に変更するとよい。
# - torch本体・依存パッケージ群でmanylinuxのタグがバラバラ(パッケージ
#   ごとにビルド時のglibcバージョンをそのままタグにしているため、
#   manylinux_2_17/2_18/2_24/2_25/2_26/2_27/2_28や旧来のmanylinux1/2010/
#   2014などが混在する)。pipは--platformで指定した文字列と完全一致する
#   タグしか受け付けない(自動的な下位互換の判定はクロス指定時は
#   働かない)ため、--platformを glibc 2.12〜2.31 まで総当たりで
#   列挙している。1つだけ指定すると解決できないパッケージが出てエラーに
#   なることを確認済み。
# - このtorchはCUDA 13.0系。doc/Dockerfile.ml-gpu /
#   doc/ml-conda-windows-download.ps1のconda版はCUDA 13.3系で、
#   厳密には別のマイナーバージョンだが、同じCUDA 13系列なので
#   実機のドライバ(13.3対応)でそのまま動くはず。
# - 【重要・Windows上で実行すると起きる問題】torchのCUDA関連依存
#   (nvidia-cudnn-cu13, nvidia-nccl-cu13 等)には、torch側のメタデータで
#   `; platform_system == "Linux"` という条件(PEP 508環境マーカー)が
#   付いている。この条件は`--platform`(wheelのタグ照合用)では制御でき
#   ず、**pipを実行している実際のOS**で評価される。そのため、この
#   スクリプトをWindows上で実行すると`platform_system`が`"Windows"`と
#   評価され、cudnn/nccl等のCUDAライブラリ一式が"該当なし"として
#   静かに(エラーも出さずに)スキップされてしまう。実際に発生を確認済み
#   (torch本体の.whl(約550MB)だけ落ちて、残りが落ちない)。
#   対策として、これらのパッケージをtorchの依存経由ではなく
#   **明示的に個別指定**することで回避する(明示指定した場合はマーカー
#   条件によるフィルタが適用されないため)。バージョンはtorch==2.14.0が
#   要求する値と完全に一致させる必要がある(異なると依存関係エラーに
#   なる)。torchのバージョンを変更する場合は、対応するこれらのバージョンも
#   `pip download --no-deps torch==<version>` 等で事前に確認し直すこと。
# - 【上記の亜種・実機で発生を確認】`cuda-toolkit[cufile]`のextra経由で
#   入る`nvidia-cufile`は、cuda-toolkit自身のメタデータ内で
#   `sys_platform == "linux"`という条件が付いており、**Windows版の
#   ビルドがそもそも存在しない**(GPUDirect StorageはLinux専用機能の
#   ため)。cuda-toolkit全体を明示指定してもextra内部のこの条件は
#   別扱いで評価されるため、Windows上ではnvidia-cufileだけが同様に
#   スキップされ、Linux実機側で
#   `Could not find a version that satisfies the requirement
#   nvidia-cufile==1.15.1.6`になることを確認した。これも
#   `nvidia-cufile`自体を明示指定することで回避する。

$ErrorActionPreference = "Stop"

New-Item -ItemType Directory -Force -Path .\wheels | Out-Null

# manylinuxのglibcバージョンタグをglibc 2.12〜2.31まで総当たりで列挙
# (配列にして -Args 経由で渡すことで、引用符やエスケープの問題を避ける)
$pipArgs = @(
  "download", "--no-cache-dir", "--timeout", "300", "--retries", "8",
  "--only-binary=:all:", "--python-version", "313",
  "--implementation", "cp", "--abi", "cp313"
)
foreach ($v in 12..31) {
  $pipArgs += "--platform"
  $pipArgs += "manylinux_2_${v}_x86_64"
}
$pipArgs += "--platform", "manylinux2014_x86_64"
$pipArgs += "--platform", "manylinux2010_x86_64"
$pipArgs += "--platform", "manylinux1_x86_64"
$pipArgs += "-d", ".\wheels"
$pipArgs += "torch==2.14.0", "scikit-learn", "tqdm", "jupyter"
# torchのLinux限定・CUDA関連依存を明示指定(Windows上でのマーカー
# フィルタ問題の回避。バージョンはtorch==2.14.0の要求値と一致させている)
$pipArgs += "cuda-toolkit[cublas,cudart,cufft,cufile,cupti,curand,cusolver,cusparse,nvjitlink,nvrtc,nvtx]==13.0.3"
$pipArgs += "cuda-bindings==13.3.1"
$pipArgs += "nvidia-cudnn-cu13==9.24.0.43"
$pipArgs += "nvidia-cusparselt-cu13==0.8.1"
$pipArgs += "nvidia-nccl-cu13==2.30.7"
$pipArgs += "nvidia-nvshmem-cu13==3.4.5"
$pipArgs += "triton==3.8.0"
$pipArgs += "nvidia-cufile==1.15.1.6"

python -m pip @pipArgs

Write-Host "=== 完了 ==="
Write-Host ".\wheels フォルダをUSB等でLinux実機へコピーしてください"
