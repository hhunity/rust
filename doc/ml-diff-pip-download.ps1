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
#       $PythonVersion / $Abi を 311 / cp311 に書き換えること。
#       `conda activate radonpy && python --version` で確認できる。
#
# 実行後、生成される wheels フォルダをUSB等でオフラインのLinux実機へコピーする。
# 実機側では doc/ml-diff-pip-install.sh を使ってオフライン導入する。
#
# 【設計: なぜ1個ずつ--no-depsでダウンロードするのか】
#   以前は「torch scikit-learn tqdm jupyter ...」をまとめて1回の
#   `pip download`に渡し、依存関係解決を丸ごとpipに任せていた。この方式
#   だと、依存の中の1つでも解決できないもの(PEP 508環境マーカーの
#   評価がWindowsとLinuxで食い違う問題。下記「ハマったポイント」参照)が
#   あると、pipはそこでエラーを出して**それ以外の分も含めて何も
#   ダウンロードせずに丸ごと止まってしまう**。1個ダメだと直して再実行、
#   を何度も繰り返す羽目になっていた(実際に発生・報告あり)。
#
#   これを避けるため、$Packagesには「Linux実機向けに必要な
#   全パッケージ名==バージョン」を事前に確定させたリストとして持たせ、
#   1個ずつ`--no-deps`(依存解決なし、そのパッケージ自体のwheelだけを
#   取得)でダウンロードするループに変更した。1個失敗しても
#   ループは止めず、最後に成功/失敗を一覧表示する。これにより
#   1回の実行で「今回何が足りないか」が全部まとめて分かる。
#
#   このリストは、実際にLinux上でtorch/scikit-learn/tqdm/jupyterの
#   フル依存解決を行った結果(131パッケージ)をそのまま書き出したもの。
#   pandas/numpy/rdkit/matplotlibのように既にRadonPy環境にあるものは
#   pipが依存解決時に別途考慮するので、ここに無くても実機側の
#   `pip install`(依存解決あり、doc/ml-diff-pip-install.sh参照)で
#   正しく解決される。
#
# 【差分(追加分)だけダウンロードしたい場合】
#   このスクリプトは`.\wheels`フォルダを毎回消さずに使う。pipは同名の
#   ファイルが既にあればダウンロードし直さず単純にスキップするため、
#   普通に再実行するだけで「前回まだ無かった分だけ」が追加取得される
#   (動作確認済み: 既にあるファイルは `File was already downloaded ...`
#   と表示されてスキップされる)。
#
#   後から新しいパッケージを1つ2つ追加したいだけの場合(上のベース
#   リストに無いもの)は、`-Package`にパッケージ名(カンマ区切りで
#   複数可)を渡す:
#     .\ml-diff-pip-download.ps1 -Package seaborn
#     .\ml-diff-pip-download.ps1 -Package seaborn,plotly
#   ベースのtorch一式は一切ダウンロードせず、指定したパッケージ(通常の
#   依存解決あり。新規パッケージなので--no-depsにはしない)とその新規の
#   依存分だけを同じ `.\wheels` フォルダに追記する。
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
# - 【重要・Windows上で実行すると起きる問題】torch/ipython等が要求する
#   一部の依存(nvidia-cudnn-cu13, nvidia-nccl-cu13, nvidia-cufile,
#   pexpect 等)には、`; platform_system == "Linux"` や
#   `; sys_platform != "win32"` のようなPEP 508環境マーカーが付いており、
#   これは`--platform`(wheelのタグ照合用)では制御できず、**pipを実行
#   している実際のOS**で評価される。そのため、このスクリプトをWindows
#   上で実行すると該当パッケージが"該当なし"としてエラーも出さず静かに
#   スキップされてしまう(実際に複数回発生を確認)。
#   今回、$Packagesの元になった131パッケージ全ての依存関係をPyPIの
#   メタデータで一括スキャンし、このマーカー問題を伴う依存が無いことを
#   確認済み(2026-09-08時点)。--no-depsで1個ずつ明示的に指定している
#   ため、このマーカー問題自体もう起こらない(依存経由でのみ発生する
#   問題であり、トップレベルで明示指定した場合はマーカーは評価されない
#   ため)。
# - このtorchはCUDA 13.0系。doc/Dockerfile.ml-gpu /
#   doc/ml-conda-windows-download.ps1のconda版はCUDA 13.3系で、
#   厳密には別のマイナーバージョンだが、同じCUDA 13系列なので
#   実機のドライバ(13.3対応)でそのまま動くはず。

param(
  # 追加で欲しいパッケージ名(カンマ区切りで複数可)。指定した場合、
  # ベースのtorch一式はダウンロードせず、指定したパッケージのみを
  # .\wheels に追記する(通常の依存解決あり)。
  [string[]]$Package = @()
)

$PythonVersion = "313"
$Abi = "cp313"

New-Item -ItemType Directory -Force -Path .\wheels | Out-Null

# manylinuxのglibcバージョンタグをglibc 2.12〜2.31まで総当たりで列挙
# (配列にして -Args 経由で渡すことで、引用符やエスケープの問題を避ける)
$platformArgs = @()
foreach ($v in 12..31) {
  $platformArgs += "--platform"
  $platformArgs += "manylinux_2_${v}_x86_64"
}
$platformArgs += "--platform", "manylinux2014_x86_64"
$platformArgs += "--platform", "manylinux2010_x86_64"
$platformArgs += "--platform", "manylinux1_x86_64"

$commonArgs = @(
  "download", "--no-cache-dir", "--timeout", "300", "--retries", "8",
  "--only-binary=:all:", "--python-version", $PythonVersion,
  "--implementation", "cp", "--abi", $Abi
) + $platformArgs + @("-d", ".\wheels")

if ($Package.Count -gt 0) {
  # 追加パッケージモード: 指定パッケージだけを通常の依存解決付きで追記
  Write-Host "=== 追加パッケージモード: $($Package -join ', ') ==="
  python -m pip @commonArgs @Package
  if ($LASTEXITCODE -ne 0) {
    Write-Host "=== 失敗しました(終了コード $LASTEXITCODE) ===" -ForegroundColor Red
    exit $LASTEXITCODE
  }
  Write-Host "=== 完了 ==="
  exit 0
}

# 通常モード: Linux実機向けに必要な全パッケージ(torch/scikit-learn/
# tqdm/jupyterのフル依存解決結果、131個)を1個ずつ--no-depsで取得する。
# 失敗してもループを止めず、最後に一覧表示する。
$Packages = @(
  "torch==2.14.0",
  "scikit-learn==1.9.0",
  "tqdm==4.70.0",
  "jupyter==1.1.1",
  "anyio==4.15.1",
  "argon2-cffi==25.1.0",
  "argon2-cffi-bindings==26.1.0",
  "arrow==1.4.0",
  "asttokens==3.0.2",
  "async-lru==2.3.0",
  "attrs==26.1.0",
  "babel==2.18.0",
  "beautifulsoup4==4.15.0",
  "bleach==6.4.0",
  "certifi==2026.7.22",
  "cffi==2.1.1",
  "charset-normalizer==3.5.1",
  "cloudpickle==3.1.2",
  "comm==0.2.3",
  "cuda-bindings==13.3.1",
  "cuda-pathfinder==1.8.1",
  "cuda-toolkit==13.0.3.0",
  "debugpy==1.8.21",
  "defusedxml==0.7.1",
  "executing==2.2.1",
  "fastjsonschema==2.22.2",
  "filelock==3.32.5",
  "fqdn==1.5.1",
  "fsspec==2026.7.0",
  "h11==0.16.0",
  "httpcore==1.0.9",
  "httpx==0.28.1",
  "idna==3.19",
  "ipykernel==7.3.0",
  "ipython==9.17.1",
  "ipython-pygments-lexers==1.1.1",
  "ipywidgets==8.1.9",
  "isoduration==20.11.0",
  "jedi==0.20.0",
  "jinja2==3.1.6",
  "joblib==1.6.0",
  "json5==0.15.0",
  "jsonpointer==3.1.1",
  "jsonschema==4.26.0",
  "jsonschema-specifications==2025.9.1",
  "jupyter-builder==1.2.3",
  "jupyter-client==8.10.0",
  "jupyter-console==6.6.3",
  "jupyter-core==5.9.1",
  "jupyter-events==0.12.1",
  "jupyter-lsp==2.3.1",
  "jupyter-server==2.21.0",
  "jupyter-server-terminals==0.5.4",
  "jupyterlab==4.6.3",
  "jupyterlab-pygments==0.3.0",
  "jupyterlab-server==2.28.0",
  "jupyterlab-widgets==3.0.17",
  "lark==1.3.1",
  "markupsafe==3.0.3",
  "matplotlib-inline==0.2.2",
  "mistune==3.3.4",
  "mpmath==1.3.0",
  "narwhals==2.25.0",
  "nbclient==0.11.0",
  "nbconvert==7.17.1",
  "nbformat==5.11.1",
  "nest-asyncio2==1.7.2",
  "networkx==3.6.1",
  "notebook==7.6.2",
  "notebook-shim==0.2.4",
  "numpy==2.5.3",
  "nvidia-cublas==13.1.1.3",
  "nvidia-cuda-cupti==13.0.85",
  "nvidia-cuda-nvrtc==13.0.88",
  "nvidia-cuda-runtime==13.0.96",
  "nvidia-cudnn-cu13==9.24.0.43",
  "nvidia-cufft==12.0.0.61",
  "nvidia-cufile==1.15.1.6",
  "nvidia-curand==10.4.0.35",
  "nvidia-cusolver==12.0.4.66",
  "nvidia-cusparse==12.6.3.3",
  "nvidia-cusparselt-cu13==0.8.1",
  "nvidia-nccl-cu13==2.30.7",
  "nvidia-nvjitlink==13.3.33",
  "nvidia-nvshmem-cu13==3.4.5",
  "nvidia-nvtx==13.0.85",
  "overrides==7.7.0",
  "packaging==26.3",
  "pandocfilters==1.5.1",
  "parso==0.8.7",
  "pexpect==4.9.0",
  "platformdirs==4.11.7",
  "prometheus-client==0.26.0",
  "prompt-toolkit==3.0.53",
  "psutil==7.2.2",
  "ptyprocess==0.7.0",
  "pure-eval==0.2.3",
  "pycparser==3.0",
  "pygments==2.21.0",
  "python-dateutil==2.9.0.post0",
  "python-json-logger==4.2.0",
  "pyyaml==6.0.3",
  "pyzmq==27.2.0",
  "referencing==0.37.0",
  "requests==2.34.2",
  "rfc3339-validator==0.1.4",
  "rfc3986-validator==0.1.1",
  "rfc3987-syntax==1.1.0",
  "rpds-py==2026.6.3",
  "scipy==1.18.1",
  "send2trash==2.1.0",
  "setuptools==84.0.0",
  "six==1.17.0",
  "soupsieve==2.9.2",
  "stack-data==0.6.3",
  "sympy==1.14.0",
  "terminado==0.18.1",
  "threadpoolctl==3.6.0",
  "tinycss2==1.5.1",
  "tornado==6.5.8",
  "traitlets==5.16.1",
  "triton==3.8.0",
  "typing-extensions==4.16.0",
  "tzdata==2026.3",
  "uri-template==1.3.0",
  "urllib3==2.7.0",
  "wcwidth==0.8.3",
  "webcolors==25.10.0",
  "webencodings==0.6.1",
  "websocket-client==1.9.2",
  "widgetsnbextension==4.0.16"
)

$failures = @()
$i = 0
foreach ($pkg in $Packages) {
  $i++
  Write-Host "[$i/$($Packages.Count)] $pkg"
  python -m pip @commonArgs --no-deps $pkg
  if ($LASTEXITCODE -ne 0) {
    Write-Host "  -> 失敗(終了コード $LASTEXITCODE)" -ForegroundColor Red
    $failures += $pkg
  }
}

Write-Host ""
Write-Host "=== 完了: $($Packages.Count - $failures.Count)/$($Packages.Count) 成功 ==="
if ($failures.Count -gt 0) {
  Write-Host "以下が失敗しました:" -ForegroundColor Red
  $failures | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
  Write-Host "上記を貼って報告してください。" -ForegroundColor Red
} else {
  Write-Host ".\wheels フォルダをUSB等でLinux実機へコピーしてください"
}
