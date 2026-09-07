
## 機械学習系パッケージ (doc/ml-conda-urls-gpu.txt)

pandas, numpy, scikit-learn, pytorch(GPU/CUDA版), matplotlib, tqdm, rdkit,
jupyter を Ubuntu 22.04 (jammy) にオフラインで入れるための、conda-forge
パッケージ(依存関係含む)のダウンロードURL一覧です。286個、合計約2.9GB。
**この手順は実際にダウンロード→ローカルchannel化→`conda create`→
import・jupyter起動まで動作確認済みです。**

対象環境:
- Ubuntu 22.04 (jammy) / glibc 2.35
- GPU(CUDA)版。CUDAは13.3系(`doc/radonpy-cuda-toolkit-urls.txt`と同じ系統)
  - pytorchはconda-forgeの `cuda130` ビルドが選ばれる
  - CPU版が良い場合は下記生成コマンドから `"pytorch=*=*cuda*"` と
    `"cuda-version=13.3"` の指定を外せばよい(その場合`__cuda`仮想パッケージの
    上書きも不要)
- Python 3.11 (RadonPy側と合わせた)
- `matplotlib`ではなく`matplotlib-base`を使用(理由は下記「ハマりどころ」参照)

### 生成方法

Windows側でも実行できる(Miniforge/Minicondaがインストール済みで、
ネットに繋がっている前提)。`pytorch>=1.11`はGPUドライバの有無を
`__cuda`という仮想パッケージで判定する仕様のため、GPUが無い/Windows上で
解決する場合は `CONDA_OVERRIDE_CUDA` で明示的に「CUDA 13.3が使える」と
偽装してやる必要がある(`CONDA_OVERRIDE_GLIBC`/`CONDA_OVERRIDE_ARCHSPEC`と
同じ考え方)。

```powershell
conda config --remove channels defaults 2>$null
conda config --add channels conda-forge
conda config --set channel_priority strict

$env:CONDA_SUBDIR = "linux-64"
$env:CONDA_OVERRIDE_GLIBC = "2.35"
$env:CONDA_OVERRIDE_ARCHSPEC = "0"
$env:CONDA_OVERRIDE_CUDA = "13.3"     # GPU版が不要ならこの行とpytorch/cuda-versionの指定を外す

conda create -n ml_dl -y --override-channels -c conda-forge --download-only `
  python=3.11 pandas numpy scikit-learn matplotlib-base tqdm rdkit jupyter `
  "pytorch=*=*cuda*" "cuda-version=13.3"
```

このリポジトリの `doc/ml-conda-urls-gpu.txt` は、上記と同じ条件で
`conda create --dry-run --json` を実行し、解決されたパッケージのURLを
抽出したものです(`conda create --download-only` の代わりにURL一覧だけ
欲しい場合はこちらを使う)。

### オフライン環境(Linux)での反映のしかた

conda用パッケージは、ダウンロードしたファイルを置くだけでは
`conda install`から認識されない(パッケージインデックスが必要)。
以下のように**ローカルchannel**として`conda index`を通してから使う。
手元(このリポジトリの検証環境)で実際に動作確認済みの手順:

Windows側のダウンロード時点で、URLの `linux-64`/`noarch` の区分どおりに
自動でフォルダ分けしてくれる `doc/ml-conda-windows-download.ps1` を用意した
(単純にURLを1個ずつ順にInvoke-WebRequestするだけだと、全ファイルが
フラットな1フォルダに落ちてどれがlinux-64向け/noarch向けか分からなくなる
ため)。`doc/ml-conda-urls-gpu.txt` と同じフォルダに置いて実行すると、
`local-channel/linux-64/`・`local-channel/noarch/` に仕分けながら
ダウンロードしてくれる。生成された `local-channel` フォルダを丸ごと
Linux実機へコピーする。

```bash
# 1. (Windows側で ml-conda-windows-download.ps1 を実行済みなら、
#     local-channel/ フォルダをそのままコピーしてきているのでこの手順は不要)

# 2. ローカルchannelとしてインデックスを作る(conda-index が無ければ先に導入)
conda install -n base -y conda-index
conda index local-channel/

# 3. そのローカルchannelだけを使って環境を作成
#    重要: ここで --offline を付けてはいけない。
#    (--offlineを付けるとconda-forgeの「パッチ済みrepodata」ではなく
#     素の生成キャッシュを探しにいってしまい、後述の理由でパッケージが
#     見つからず失敗する。file://ローカルchannelしか指定していなければ
#     --offlineが無くてもネットワークには一切アクセスしない)
export CONDA_OVERRIDE_CUDA=13.3   # GPUドライバが正しく入っていれば本来は自動検出されるが、
                                   # 万一検出に失敗した場合の保険として付けておくと安全
conda create -n ml -y --override-channels \
  -c "file://$(pwd)/local-channel" \
  python=3.11 pandas numpy scikit-learn matplotlib-base tqdm rdkit jupyter pytorch

conda activate ml
python -c "import pandas, numpy, sklearn, matplotlib, rdkit, torch; print(torch.cuda.is_available())"
```

(RadonPy用の`doc/radonpy-windows-download.ps1` + `doc/radonpy-offline-install.sh`
のペアのように、`--download-only`で集めた`pkgs/`ディレクトリを丸ごとコピーして
`--offline`でconda createする方式でも同じ結果になる。そちらは
conda-forge側の「パッチ済みrepodata」もそのままキャッシュに含まれるため
下記のハマりどころが起きにくく、むしろ簡単。URL一覧はダウンロード
マネージャ等でまとめてURLを取得したい場合の代替手段。)

### ハマりどころ: `matplotlib`(フル版)だと依存関係エラーで入らないことがある

`matplotlib`(フル版)は既定でQt系GUIバックエンド(`pyside6`→`qt6-main`→
`xcb-util-wm`→`libxcb`)を道連れにする。ここで問題が起きる:
conda-forge本家は`repodata_patches`で依存関係の矛盾を後から補正しているが、
手元でダウンロード済みファイルに対して`conda index`を実行すると、
その補正が反映されない**素のパッケージ内メタデータ**を使ってしまうため、
`nothing provides libxcb >=1.16,<1.17.0a0 needed by xcb-util-wm-...`
のようなエラーで解決不能になることがある(実際に発生を確認済み)。

対策として、GUI表示を必要としない(Jupyter上で`%matplotlib inline`等を
使う)用途では`matplotlib`の代わりに`matplotlib-base`を使うことでQt系の
依存を丸ごと回避できる。上記のURL一覧・生成コマンドは既にこの対策済み。
どうしてもフル版`matplotlib`が必要な場合は、`--download-only`+`pkgs/`
コピー方式(パッチ済みrepodataキャッシュごと持っていく)を使うこと。

## RadonPy (git) を使うのに必要なファイル (doc/radonpy-files.md)

ポリマー物性の全自動計算ライブラリ RadonPy (https://github.com/RadonPy/RadonPy)
を動かすために必要な一式(本体の git clone、依存Conda/PyPIパッケージ、
LAMMPS/Psi4等の外部エンジン、オフライン環境への持っていき方)をまとめたもの。
詳細は `doc/radonpy-files.md` を参照。

## Yoctoビルド用Ubuntu環境 (doc/yocto-urls.txt, doc/yocto-urls-jammy.txt)

Yocto Project (bitbake) をオフラインでビルドできるようにするための、
ビルドホストに必要な apt パッケージ一式(および依存パッケージ)の
ダウンロード URL 一覧です。amd64 向けに以下の2バージョン分を用意しています
(オフライン環境のUbuntuのバージョンと合っていないと、依存パッケージの
バージョン不一致でインストールに失敗するため、対象環境に合わせて使い分けて
ください):

| ファイル | 対象Ubuntuバージョン |
|---|---|
| `doc/yocto-urls.txt` | Ubuntu 24.04 (noble) |
| `doc/yocto-urls-jammy.txt` | Ubuntu 22.04 (jammy) |

対象パッケージ(Yocto公式ドキュメントの "Build Host Packages" 相当、
両バージョン共通)は以下の通りです。
- gawk, wget, git, diffstat, unzip, texinfo (基本ツール)
- build-essential, chrpath, socat, cpio (ビルド・パッケージング関連)
- python3, python3-pip, python3-pexpect, python3-git, python3-jinja2,
  python3-subunit (bitbakeの実行に必要なPython関連)
- xz-utils, zstd, liblz4-tool, lz4, file (アーカイブ/圧縮ツール)
- debianutils, iputils-ping, locales, libacl1 (その他依存)

このうち `chrpath` / `texinfo` / `python3-pip` / `python3-subunit` /
`liblz4-tool` / `lz4` は Ubuntu の `universe` リポジトリに属するため、
オフライン環境側の `/etc/apt/sources.list` (または `.sources`) で
`universe` コンポーネントを有効にしておく必要があります
(パッケージファイル自体はダウンロード済みのものを `dpkg -i` で入れるだけ
なので有効化必須ではありませんが、依存解決のため apt 経由でインストール
する場合は該当行に `universe` を追記してください)。

生成コマンド(`universe` を有効にした対象バージョンの Ubuntu amd64 環境で実行。
noble以外で生成する場合は、`apt-get`のsourcelistを対象コードネームの
リポジトリに向けた上で実行してください):
```
apt-get install --print-uris -y -o Dir::State::status=/dev/null \
  gawk wget git diffstat unzip texinfo build-essential chrpath socat cpio \
  python3 python3-pip python3-pexpect xz-utils debianutils iputils-ping \
  python3-git python3-jinja2 python3-subunit zstd liblz4-tool file locales \
  libacl1 lz4 \
  | grep -oP "(?<=')[^']+(?=')" | grep '^http' | sort -u > yocto-urls.txt
```

`doc/yocto-urls-jammy.txt` はこれと同じパッケージ一式を、jammy(22.04)の
main/universe(+ updates/security)のパッケージインデックスに対して解決した
ものです(パッケージバージョンがnoble版と異なります。例:
binutils 2.42→2.38, python3-pip 24.0→22.0.2, texinfo 7.1→6.8 など)。

### Windows側での一括ダウンロード手順

1. Windows上でPowerShellを開き、対象バージョンのURL一覧
   (`doc/yocto-urls.txt` または `doc/yocto-urls-jammy.txt`) を
   同じフォルダに `yocto-urls.txt` としてコピーする
2. 以下を実行して全 `.deb` をダウンロード:
   ```powershell
   $outDir = ".\yocto-offline-debs"
   New-Item -ItemType Directory -Force -Path $outDir | Out-Null

   Get-Content yocto-urls.txt | ForEach-Object {
       $fileName = Split-Path $_ -Leaf
       Invoke-WebRequest -Uri $_ -OutFile (Join-Path $outDir $fileName)
   }
   ```
3. `yocto-offline-debs` フォルダをUSBメモリ等でオフラインのUbuntu環境へコピー

### オフライン環境(Ubuntu)側でのインストール

```
cd yocto-offline-debs
sudo dpkg -i *.deb
sudo apt-get install -f   # 依存関係の不足があれば解消(ネット不要、同フォルダのdebを使う)
```

`locales` パッケージ導入後は、Yoctoが要求する `en_US.UTF-8` ロケールを
生成しておく:
```
sudo locale-gen en_US.UTF-8
sudo update-locale LC_ALL=en_US.UTF-8 LANG=en_US.UTF-8
```

### 補足: Yocto本体とレシピのソースについて

上記はあくまで**ビルドホスト(Ubuntu)側に必要なツール類**のURL一覧です。
Yoctoのビルドではこれとは別に以下も必要になるため、完全オフラインで
ビルドしたい場合は併せて準備してください:

- **Yocto (poky) 本体**: `git clone https://git.yoctoproject.org/poky` を
  ネットのある端末で実行し、`.git` ごとオフライン環境へコピーする
  (もしくは release tarball を https://downloads.yoctoproject.org/releases/yocto/
  から取得)
- **各レシピが取得するソースアーカイブ (`DL_DIR`)**: ネットに繋がる端末で
  対象ターゲットを一度 `bitbake <target>` してビルドし、生成される
  `downloads/` ディレクトリを丸ごとオフライン環境へコピーして
  `local.conf` の `DL_DIR` に指定する。`bitbake <target> --runall=fetch` を
  使うと実際のビルドをせずにダウンロードだけ済ませられる
- **sstate-cache**: 同様にビルド成果物のキャッシュ (`sstate-cache/`) を
  コピーしておくと、オフライン環境での再ビルドが高速化できる(必須ではない)

## Rust開発環境用 (doc/rust-urls-<コードネーム>.txt)

Rust のコードをオフラインでビルドできるようにするための apt パッケージ一式
(および依存パッケージ)のダウンロード URL 一覧です。**deb パッケージは
Ubuntuのバージョン(コードネーム)ごとに中身もURLも異なるので、
インストール先と同じバージョン向けのファイルを使ってください。**
バージョンが違うファイルを使うと依存関係が噛み合わず「バージョンが合わない」
エラーになります。

現在用意しているファイル:
- `doc/rust-urls-noble.txt` … Ubuntu 24.04 (noble) / amd64 向け
- `doc/rust-urls-jammy.txt` … Ubuntu 22.04 (jammy) / amd64 向け

インストール先のバージョン確認方法:
```
cat /etc/os-release   # または: lsb_release -a
```

### インストール方法の注意: `dpkg -i` ではなく `apt-get install ./*.deb` を使う

ダウンロードした.debをまとめて入れる際、`dpkg -i *.deb` だと依存関係の
解決順序を考慮してくれないため、`dependency problems - leaving unconfigured`
のようなエラーが大量に出ることがある(パッケージが足りていなくても起きるが、
足りていても順序の問題だけで起きる)。代わりに以下を使うこと:
```
cd rust-offline-debs
sudo apt-get install ./*.deb
```
apt がローカルの.debファイルだけを見て正しいインストール順序を自動計算して
くれるため、ネット接続なしでも一発で通る。

対象パッケージ:
- build-essential (gcc, g++, make 等。C ツールチェーンは多くの crate のビルドに必要)
- pkg-config (native crate のビルド設定検出に必要)
- libssl-dev (openssl-sys など TLS を使う crate のビルドに必要)
- ca-certificates (crates.io / GitHub への HTTPS 通信に必要)
- git (cargo の git 依存解決、ソース管理)
- curl (rustup インストーラの取得に必要)
- cmake (一部の native crate のビルドに必要)

生成コマンド(自分の環境がそのままターゲットと同じバージョンの場合):
```
apt-get install --print-uris -y -o Dir::State::status=/dev/null \
  build-essential pkg-config libssl-dev ca-certificates git curl cmake \
  | grep -oP "(?<=')[^']+(?=')" | grep '^http' | sort -u > rust-urls-<コードネーム>.txt
```
(`Dir::State::status=/dev/null` を指定することで、既にインストール済みのパッケージも
含めた完全な依存関係の URL 一覧を出力できます。)

自分の環境と**違う**バージョン向けに生成したい場合(例: noble環境でjammy用を作る)は、
apt の状態を一切変更せず一時的な設定だけでそのバージョンのパッケージ索引を取得できます:
```
mkdir -p /tmp/apt-jammy/lists/partial /tmp/apt-jammy/cache/archives/partial
cat > /tmp/apt-jammy/sources.list <<'EOF'
deb http://archive.ubuntu.com/ubuntu/ jammy main restricted universe multiverse
deb http://archive.ubuntu.com/ubuntu/ jammy-updates main restricted universe multiverse
deb http://security.ubuntu.com/ubuntu/ jammy-security main restricted universe multiverse
EOF

apt-get -o Dir::Etc::SourceList=/tmp/apt-jammy/sources.list \
        -o Dir::Etc::SourceParts=/dev/null \
        -o Dir::State::Lists=/tmp/apt-jammy/lists \
        -o Dir::Cache=/tmp/apt-jammy/cache \
        -o APT::Architecture=amd64 \
        update

apt-get -o Dir::Etc::SourceList=/tmp/apt-jammy/sources.list \
        -o Dir::Etc::SourceParts=/dev/null \
        -o Dir::State::Lists=/tmp/apt-jammy/lists \
        -o Dir::Cache=/tmp/apt-jammy/cache \
        -o Dir::State::status=/dev/null \
        -o APT::Architecture=amd64 \
        install --print-uris -y build-essential pkg-config libssl-dev ca-certificates git curl cmake \
  | grep -oP "(?<=')[^']+(?=')" | grep '^http' | sort -u > rust-urls-jammy.txt
```
`jammy` の部分をターゲットのコードネーム(`focal`, `noble` 等)に置き換えれば
他バージョンにも対応できます。

rustc/cargo 本体は apt ではなく、上記でダウンロードした curl を使って
[rustup](https://rustup.rs/) でインストールすることを推奨します
(バージョン管理がしやすいため)。

## Rust本体を完全オフラインで入れる場合 (doc/rust-toolchain-urls.txt)

インターネットに繋がらない環境にインストールする場合は、rustup経由ではなく
static.rust-lang.org が配布している**単体installer(tarball)** を使うのが簡単です。
これ1ファイルに rustc / cargo / 標準ライブラリ / インストールスクリプト
(`install.sh`) が全部入っているので、rustup 自体は不要です。

手順:
1. ネットワークのある端末で `doc/rust-toolchain-urls.txt` 先頭の
   `rust-<version>-x86_64-unknown-linux-gnu.tar.xz` をダウンロードし、
   sha256sum でハッシュ値を照合する(コメントに記載)
2. オフライン環境に転送し、展開してインストール:
   ```
   tar xf rust-1.98.0-x86_64-unknown-linux-gnu.tar.xz
   cd rust-1.98.0-x86_64-unknown-linux-gnu
   sudo ./install.sh                    # /usr/local に入れる場合
   # もしくは ./install.sh --prefix=$HOME/.local --destdir=  # 非rootの場合
   ```
3. `rustc --version` / `cargo --version` で確認

このリストのバージョン(1.98.0, 2026-08-20時点のstable)は時間が経つと古くなります。
最新化したい場合は `https://static.rust-lang.org/dist/channel-rust-stable.toml`
を取得し、`[pkg.rust.target.x86_64-unknown-linux-gnu]` のURLを参照してください。

## ライブラリ(crate)を集めてWindows→Linuxへ持っていく方法

crates.io上の依存ライブラリ(crate)は個別にURLをダウンロードするのではなく、
cargo標準機能の `cargo vendor` でソースごとまとめて集めるのが基本です。
バイナリではなくソースコード一式なので、Windows上で集めてそのままLinuxへ
コピーすればOKです(クロスコンパイルやOS変換は不要)。

### 手順

1. **Windows側の準備**
   - Rust(cargo)がインストール済みで、かつネットに繋がっている状態が必要
     (vendorする=ダウンロードする作業なので、この時だけはオンラインが必要)
   - 対象プロジェクトの `Cargo.toml`(使うcrate一覧)を用意する

2. **Windows側でvendorを実行**(プロジェクトのルートディレクトリで)
   ```
   mkdir .cargo
   cargo vendor > .cargo/config.toml
   ```
   - `vendor/` フォルダに、依存する全crateのソース一式がダウンロード・展開される
   - `.cargo/config.toml` には、cargoにvendorディレクトリを見に行かせるための
     設定(`source.crates-io` を `vendor/` に差し替える設定)が書き込まれる

3. **Linuxへ転送**
   USBメモリ等で以下をまとめてコピーする(この4点セットで完結):
   - `Cargo.toml`
   - `Cargo.lock`
   - `vendor/`
   - `.cargo/config.toml`

4. **Linux側でビルド**
   ```
   cargo build --offline
   ```
   ネット接続なしでビルドできる。

### 補足

- `Cargo.lock` にはOS条件付き依存(`cfg(windows)` / `cfg(unix)` など)も
  **全プラットフォーム分**記録されるため、Windows上で`cargo vendor`しても
  Linux用の依存(`libc`等)はちゃんと含まれる。クロスコンパイルの心配は不要。
- `openssl-sys` など、ネイティブCライブラリにリンクするcrateはソース自体は
  vendorできるが、リンク先のOS側ライブラリ(`libssl-dev`等)は別途必要。
  これは `doc/rust-urls-<コードネーム>.txt` のaptパッケージ一覧でカバー済み
  (インストール先のUbuntuバージョンに合わせたファイルを使うこと)。
- 依存crateを追加/変更するたびに、Windows側で `Cargo.toml` を編集して
  `cargo vendor` を再実行し、`vendor/` を作り直してLinuxへ再転送する必要がある。
- `cargo vendor` はcargo組み込みのサブコマンドなので追加インストール不要。

Invoke-WebRequest -Uri "http://archive.ubuntu.com/ubuntu/pool/main/g/gcc-13/gcc-13_13.2.0-4ubuntu3_amd64.deb" -OutFile "gcc-13_13.2.0-4ubuntu3_amd64.deb"

---
$outDir = ".\rust-offline-debs"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

Get-Content urls.txt | ForEach-Object {
    $fileName = Split-Path $_ -Leaf
    Invoke-WebRequest -Uri $_ -OutFile (Join-Path $outDir $fileName)
}
---
