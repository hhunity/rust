
# RadonPy (git) を使うのに必要なファイル

RadonPy (https://github.com/RadonPy/RadonPy) はポリマー物性の全自動計算を行う
Python製ライブラリ。本体はGit/PyPIで配布されるが、実行には外部の分子動力学(MD)・
量子化学計算(QM)エンジンが別途必要になる。ここではそれらをまとめる。

## 1. RadonPy本体 (git)

```
git clone https://github.com/RadonPy/RadonPy.git
```

オフライン環境へは `.git` ごとコピーするか、`pip download radonpy-pypi` で
取得したwheel/sdistを転送する。

対応Pythonバージョン: 3.9 〜 3.13 (バージョンによって上限が変わるためREADME要確認)。
ライセンスはBSD-3。

## 2. 必須の外部ツール・依存パッケージ

conda (`conda-forge` / `psi4` チャンネル) 経由でのインストールが推奨。

| パッケージ | 用途 |
|---|---|
| rdkit (>=2020.03) | 化学構造処理・ポリマー鎖生成 |
| psi4 (>=1.5) | 量子化学計算(QM、構造最適化・電荷計算) |
| resp | RESP電荷計算 |
| dftd3-python | 分散力補正(DFT-D3) |
| lammps (>=2020.03.03) | 分子動力学(MD)計算本体 |
| mdtraj (>=1.9) | MD軌跡の解析 |
| numpy / scipy | 数値計算 |
| pandas | データ処理 |
| matplotlib | 可視化 |
| psutil | プロセス/リソース監視 |

これはRadonPyリポジトリ同梱の `requirements.txt` の内容と対応:
```
pandas
numpy
scipy
psutil
matplotlib
rdkit>=2020.03
mdtraj>=1.9
lammps>=2020.03.03
```

### オプション(生体高分子: ペプチド・多糖類を扱う場合のみ)

- ambertools
- intermol

## 3. Conda環境ファイル (リポジトリ同梱: yaml/rnpy37.yml, rnpy38.yml, rnpy39.yml)

RadonPyリポジトリの `yaml/` ディレクトリに、Pythonバージョンごとの
Conda環境定義ファイルが同梱されている。チャンネルは `psi4`, `conda-forge`, `defaults`。
これをそのまま使えば依存関係を一括solveできる:
```
conda env create -f yaml/rnpy39.yml
```

## 4. インストール手順(ネットに繋がる環境)

```
conda create -n radonpy python=3.11 -c conda-forge
conda activate radonpy
conda install -c conda-forge -c psi4 rdkit psi4 dftd3-python resp mdtraj psutil scipy pandas matplotlib pip
conda install -c conda-forge lammps
pip install radonpy-pypi
```

LAMMPSを含めてPyPIだけで揃えたい場合(Psi4がインストールできない環境向けの制限構成):
```
pip install radonpy-pypi[lammps]
```

## 5. オフライン環境へ持っていく方法

condaパッケージはaptの `.deb` と違い単純なURL一覧ダウンロードでは揃わないため、
以下のいずれかを使う。

- **conda-pack** (推奨): オンライン環境で作った環境を丸ごと固めてコピー
  ```
  conda install -c conda-forge conda-pack
  conda-pack -n radonpy -o radonpy_env.tar.gz
  ```
  オフライン環境側では展開して `conda-unpack` を実行するだけで使える。

  再現用スクリプト: `doc/radonpy-jammy-conda-pack.sh`
  (Ubuntu 22.04 jammy上でRadonPy用conda環境を作り、conda-packで
  1個のtarballに固めるところまでを自動化したもの)。

  **glibcバージョンに注意**: conda-forge製バイナリはlibstdc++等は
  同梱するが、glibcだけはOS側のものを動的リンクするため、
  新しいOS(新しいglibc)で固めた環境を古いOSへ持っていくと
  `GLIBC_2.XX not found` のようなエラーで動かないことがある。
  配布先の環境と同じ(または配布先以下の)glibcバージョンの
  OS上で `conda-pack` することが望ましい。Dockerが使えない環境では
  `debootstrap` + `chroot` でターゲットOSのrootfsを作りその中で
  実行する方法でも同様の結果が得られる。

- **conda create --download-only**: パッケージ本体のみキャッシュ (`pkgs/`) に
  落として、そのディレクトリごとオフライン環境へ転送し、
  `conda install --offline` でインストールする。

- **RadonPy本体(pip)側**は
  `pip download radonpy-pypi -d ./radonpy_wheels` でwheelを集め、
  オフライン側で
  `pip install --no-index --find-links=./radonpy_wheels radonpy-pypi` する
  (`doc/rust-urls.txt` のvendor手法と同じ考え方)。

## 5.4 Dockerイメージとして固める方法(推奨)

`doc/Dockerfile.radonpy` を使うと、`ubuntu:22.04`(jammy)をベースに
RadonPy実行環境をイメージとしてビルドできる。

これまで検討した方法(conda-pack、純Windownダウンロード)は、
「実機とglibcバージョンを合わせる必要がある」「Windows上での展開で
シンボリックリンクや実行権限が壊れないか未検証」という不確実性が
つきまとっていたが、Dockerイメージ(`docker save`/`docker load`)は
Docker自身がこれらを正しく扱う形式なので、この2つの不確実性が
原理的に発生しない。Macでビルドする場合は必ず
`--platform linux/amd64` を指定すること
(Apple Silicon Macはデフォルトでarm64向けにビルドしてしまうため)。

### 5.4.0 GPU(CUDA)を使う場合

`doc/Dockerfile.radonpy-gpu` を使う。RadonPyのパイプラインでGPU
アクセラレーションが効くのは実質LAMMPS(分子動力学)のみで、
Psi4(量子化学計算)・RDKitはconda-forge版がCPU専用のため恩恵はない。

ホスト側(実際にコンテナを動かすマシン)にNVIDIA製GPU・NVIDIAドライバ・
NVIDIA Container Toolkitが必要で、`docker run --gpus all` を付けて
起動する必要がある。詳細は`doc/Dockerfile.radonpy-gpu`冒頭のコメントを
参照。

`lammps=*=cuda130*` のようにワイルドカードで指定すると、MPIを含まない
`nompi`ビルドが選ばれてしまい、RadonPyが内部で呼び出す`lmp_mpi`という
バイナリが存在せず動かないことを確認済みなので、ビルド文字列まで
含めてMPI+CUDA版を明示的に固定している。CPU版で素の`lammps`を指定すると
mpich版が解決されるため、GPU版もMPI実装を揃えてmpich版を使っている
(openmpi版だとMPI実装ごと丸ごと入れ替わり、差分が大きくなることを確認)。

**既にCPU版を実機で構築済みで、GPU化のために差分だけ追加したい場合**
(コンテナに入って直接`conda install`する場合)は、以下の5パッケージの
URLだけで足りる(python=3.13+psi4=1.10のCPU版が既に入っている前提):

```
https://conda.anaconda.org/conda-forge/linux-64/lammps-2025.07.22-cuda130_py313_h5db5c7c_mpi_mpich_3.conda
https://conda.anaconda.org/conda-forge/noarch/cuda-version-13.3-hcbadf70_3.conda
https://conda.anaconda.org/conda-forge/linux-64/libevent-2.1.12-hf998b51_1.conda
https://conda.anaconda.org/conda-forge/linux-64/ucc-1.8.0-h7a4b9c7_2.conda
https://conda.anaconda.org/conda-forge/linux-64/libpmix-5.0.8-h31fc519_4.conda
```

Windows側でダウンロード後、`-v`マウントしたフォルダ経由でコンテナに渡し、
コンテナ内で以下を実行すればよい:
```
conda install -n radonpy --offline --override-channels -c conda-forge -c psi4 <ダウンロードした*.conda> -y
```
この5個の圧縮ファイルだけで、**ネットワーク接続なしでも**正しく
インストールできることを実際に確認済み(展開済みキャッシュを排除した
クリーンな状態でテスト)。既存のRadonPy(CPU)フル環境をゼロから
`--offline`構築する場合は展開済みディレクトリが必要になる問題が
以前あったが、今回のように「既に動いている環境へ、ファイルパスを
明示して追加install」する場合はこの問題が起きないため。

### 5.4.1 手順まとめ(Mac + GitHub Container Registry、実機にDocker不要)

Dockerは**ビルド用(Mac)にだけ**使い、実機には一切インストールしない構成。
実機側は素のUbuntu 22.04のまま、condaで作った環境をファイルとして
展開するだけで使えるようにする。

**1. Macでイメージをビルド**(Apple Siliconは`--platform`必須)
```bash
docker build --platform linux/amd64 -f doc/Dockerfile.radonpy \
  -t ghcr.io/hhunity/radonpy:latest .
```

**2. GitHub Container Registry (ghcr.io) へpush**
```bash
# GitHubのPersonal Access Token (write:packages 権限) でログイン
echo <YOUR_GITHUB_TOKEN> | docker login ghcr.io -u hhunity --password-stdin
docker push ghcr.io/hhunity/radonpy:latest
```
push後、GitHubの Packages 画面でこのパッケージの可視性を
"Public" に変更しておく(publicならストレージ・帯域無料、
privateだと500MB/月1GBまでの無料枠しかない)。

**3. 中身だけを取り出す(Docker不要、`crane`を使う)**

ネットに繋がる適当な端末(Macでも可)で、`crane`という単体バイナリ
(daemon不要)を使ってイメージの中身をtarとして取り出す:
```bash
brew install crane   # または https://github.com/google/go-containerregistry/releases から取得

# opt/miniforge3 ディレクトリだけを取り出す(base OS部分は不要なので除外)
mkdir -p radonpy-extract
crane export ghcr.io/hhunity/radonpy:latest - \
  | tar -x -C radonpy-extract opt/miniforge3
```

**4. オフライン実機へ転送・配置**

USB等で `radonpy-extract/opt/miniforge3` を実機へコピーし、
**ビルド時と同じパス** `/opt/miniforge3` に配置する
(conda環境内部のスクリプトのshebangがこの絶対パスを直接参照しているため、
別の場所には置けない点に注意):
```bash
sudo mkdir -p /opt/miniforge3
sudo cp -a radonpy-extract/opt/miniforge3/. /opt/miniforge3/
```

**5. 実機での利用**
```bash
export PATH=/opt/miniforge3/envs/radonpy/bin:$PATH
python -c "import radonpy; print('ok')"
# もしくは: /opt/miniforge3/bin/conda run -n radonpy python -c "import radonpy"
```

### 5.4.2 実機でDockerコンテナとして動かす場合

`doc/Dockerfile.radonpy` 冒頭のコメントに、`docker save`/`docker load`や
`docker pull`での手順を記載している。この場合は実機にもDockerが必要。

## 5.5 Windows単体(WSL/Docker不要)で集める方法

Docker/WSLが使えない場合、Windows上のcondaだけで**Linux(linux-64)向けの
パッケージファイルをダウンロードだけ**行うことができる
(`CONDA_SUBDIR=linux-64` + `conda create --download-only`)。
実際の環境構築(link)はターゲットのUbuntu 22.04実機側で`--offline`で行う。

- Windows側: `doc/radonpy-windows-download.ps1`
- オフライン実機側: `doc/radonpy-offline-install.sh`

動作検証済み(RDKit/Psi4/LAMMPSとも実際に実行できることを確認)。

**転送量について**: condaの`--offline`は圧縮された`.conda`ファイルだけでは
不十分で、**展開済みディレクトリ一式**(`<conda>\pkgs\` 配下)を
まるごと転送する必要があり、RadonPyのフルパッケージセットで
**転送量は約7GB**になる。圧縮ファイルのみ(約1.2GB)に縮小する方法
(`conda index`でローカルchannel化 + `CONDA_SOLVER=classic`、または
`conda list --explicit`形式)もいくつか試したが、いずれもcondaの
パッケージキャッシュの仕組み上「展開済みである証拠」がないと
オフラインでは使えないと判定されてしまい、RadonPyのフル依存関係では
安定して動かせなかった。**現状は7GB版が唯一確実に動作する方法。**

**glibcバージョンについて**: Windows上では実機(Ubuntu 22.04 jammy)の
実際のglibcバージョンを自動検出できないため、`CONDA_OVERRIDE_GLIBC`環境変数で
明示的に指定する必要がある(`doc/radonpy-windows-download.ps1`に反映済み)。
これを指定しない場合、開発環境側のOSがjammyより新しいと、より新しいglibcを
前提としたパッケージ(例: `sysroot_linux-64`のバージョン)が選ばれてしまい、
実機のjammyで動かない可能性があることを実際に確認した。

### 5.5.1 Windows側での作業

1. Windows側にconda(Miniconda/Anaconda/Miniforgeいずれでも可、ダウンロード作業に使うだけ)をインストールしておく(ネットに繋がっている状態)
2. `doc/radonpy-windows-download.ps1` をPowerShellで実行
   → カレントディレクトリに `radonpy_wheels\`、
     `Miniforge3-Linux-x86_64.sh` が生成される
3. `conda info --base` で表示されるcondaのインストール先の
   `pkgs\` フォルダ(スクリプト実行後にコンソールへパスが表示される)を
   まるごとコピーしておく

### 5.5.2 Linux実機(オフラインのUbuntu 22.04)への転送・構築

USBメモリ等で以下をこのコピー先へまとめて持っていく(3点セット):

- `Miniforge3-Linux-x86_64.sh`
- Windows側の `<conda>\pkgs\` フォルダ全体 → `pkgs/` という名前で配置
- `radonpy_wheels\` フォルダ

`doc/radonpy-offline-install.sh` と同じ場所に上記3点セットを置いた上で
実行すると、以下がネットワーク不要で自動的に行われる:

```
bash radonpy-offline-install.sh
```

内部でやっていることは:
1. `Miniforge3-Linux-x86_64.sh` からMiniforgeをインストール
   (`$HOME/miniforge3`)
2. 持ち込んだ `pkgs/` の中身を `$HOME/miniforge3/pkgs/` へコピー
   (これでconda側が「もうダウンロード済み」と認識する)
3. `conda create -n radonpy --offline -c conda-forge -c psi4 python=3.11
   rdkit psi4 dftd3-python resp mdtraj psutil scipy pandas matplotlib pip
   lammps` でネットワークなしに環境を構築
4. `pip install --no-index --find-links=./radonpy_wheels radonpy-pypi`
   でRadonPy本体を追加

完了後は通常通り:
```
conda activate radonpy
python -c "import radonpy"
```
で使い始められる。

## 6. まとめ(オフライン転送時に必要な4点セット)

USBメモリ等でまとめてコピーする対象:
- RadonPy本体の `.git` (または wheel/sdist)
- `yaml/rnpyXX.yml` (使うPythonバージョンのもの)
- `conda-pack` で固めた環境 tarball (`radonpy_env.tar.gz`)、
  もしくは `conda create --download-only` の `pkgs/` 一式
- RadonPy本体のwheel (`pip download` した分、conda環境にRadonPy自体が
  含まれない場合)
