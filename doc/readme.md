
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
