
## RadonPy (git) を使うのに必要なファイル (doc/radonpy-files.md)

ポリマー物性の全自動計算ライブラリ RadonPy (https://github.com/RadonPy/RadonPy)
を動かすために必要な一式(本体の git clone、依存Conda/PyPIパッケージ、
LAMMPS/Psi4等の外部エンジン、オフライン環境への持っていき方)をまとめたもの。
詳細は `doc/radonpy-files.md` を参照。

## Yoctoビルド用Ubuntu環境 (doc/yocto-urls.txt)

Ubuntu 24.04 (noble) / amd64 上で Yocto Project (bitbake) をオフラインで
ビルドできるようにするための、ビルドホストに必要な apt パッケージ一式
(および依存パッケージ)のダウンロード URL 一覧です。

対象パッケージ(Yocto公式ドキュメントの "Build Host Packages" 相当):
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

生成コマンド(`universe` を有効にした Ubuntu 24.04 amd64 環境で実行):
```
apt-get install --print-uris -y -o Dir::State::status=/dev/null \
  gawk wget git diffstat unzip texinfo build-essential chrpath socat cpio \
  python3 python3-pip python3-pexpect xz-utils debianutils iputils-ping \
  python3-git python3-jinja2 python3-subunit zstd liblz4-tool file locales \
  libacl1 lz4 \
  | grep -oP "(?<=')[^']+(?=')" | grep '^http' | sort -u > yocto-urls.txt
```

### Windows側での一括ダウンロード手順

1. Windows上でPowerShellを開き、`doc/yocto-urls.txt` を同じフォルダに置く
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

## Rust開発環境用 (doc/rust-urls.txt)

Ubuntu 24.04 (noble) / amd64 上で Rust のコードをオフラインでビルドできるように
するための apt パッケージ一式(および依存パッケージ)のダウンロード URL 一覧です。

対象パッケージ:
- build-essential (gcc, g++, make 等。C ツールチェーンは多くの crate のビルドに必要)
- pkg-config (native crate のビルド設定検出に必要)
- libssl-dev (openssl-sys など TLS を使う crate のビルドに必要)
- ca-certificates (crates.io / GitHub への HTTPS 通信に必要)
- git (cargo の git 依存解決、ソース管理)
- curl (rustup インストーラの取得に必要)
- cmake (一部の native crate のビルドに必要)

生成コマンド:
```
apt-get install --print-uris -y -o Dir::State::status=/dev/null \
  build-essential pkg-config libssl-dev ca-certificates git curl cmake \
  | grep -oP "(?<=')[^']+(?=')" | grep '^http' | sort -u > rust-urls.txt
```
(`Dir::State::status=/dev/null` を指定することで、既にインストール済みのパッケージも
含めた完全な依存関係の URL 一覧を出力できます。)

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
  これは `doc/rust-urls.txt` のaptパッケージ一覧でカバー済み。
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
