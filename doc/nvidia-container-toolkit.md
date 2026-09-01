# WSL2向け NVIDIA Container Toolkit をオフラインで導入する (doc/nvidia-container-toolkit.md)

Windows上のWSL2 (Ubuntu 22.04 jammy) に、GPU対応Dockerに必要な
**NVIDIA Container Toolkit本体**をオフラインで導入するための手順。
Docker Engine自体(Docker Desktop、またはWSL内に別途入れたdocker-ce)は
既に用意済みである前提で、NVIDIA Container Toolkitのdebパッケージと
その依存関係のみを対象にする。

## 前提: WindowsのGPUドライバ

NVIDIA Container ToolkitはGPUドライバそのものではない。WSL2でGPUを
使うには、**GPUドライバはWindows側にのみ入れる**(WSL内には
`nvidia-driver-XXX`のような通常のLinux用ドライバを別途入れてはいけない。
Windows側のドライバがWSL越しに透過的に見える仕組みのため、二重に
入れると衝突する)。

- 511.65 (2022年3月) 以降のGeForce/Studio/Quadroドライバであれば
  WSL2のCUDAサポートが標準で入っている。既にWindows側でGPUを使って
  いるなら、多くの場合そのままで追加作業不要。
- 未導入・更新したい場合の入手先(バージョンが頻繁に変わるため
  固定URLではなくランディングページを案内する):
  - https://www.nvidia.com/en-us/geforce/drivers/ (GeForce/Studio)
  - https://www.nvidia.com/Download/index.aspx (Quadro/RTX Workstation等)
- 導入後、WSL内で`nvidia-smi`を実行できればOK(WSL内に別途
  nvidia-smiをインストールする必要はなく、Windows側ドライバのものが
  そのまま見える)。

## Windows側での一括ダウンロード手順 (doc/nvidia-container-toolkit-windows-download.ps1)

WSL自体がオフラインで、他にネットに繋がるUbuntu環境も無い前提のため、
「Ubuntu環境で`apt-get install --print-uris`を実行してURL一覧を作る」
という通常のやり方(`doc/rust-urls-*.txt`等と同じ方式)は使えない。

代わりに、NVIDIAのAPTリポジトリが公開しているパッケージインデックス
ファイル(`Packages`。Debianパッケージリポジトリの標準フォーマットの
プレーンテキストで、`apt`を経由しなくても中身が読める)を直接HTTPで
取得し、そこから必要な4パッケージ

- `nvidia-container-toolkit`
- `nvidia-container-toolkit-base`
- `libnvidia-container-tools`
- `libnvidia-container1`

のダウンロードURLを組み立てる`doc/nvidia-container-toolkit-windows-download.ps1`
を用意した。**Ubuntu/WSL環境は一切不要で、ネットに繋がるWindows機だけ
で完結する**:

```powershell
.\nvidia-container-toolkit-windows-download.ps1
```

実行すると`.\nvidia-container-toolkit-offline-debs\`に4つの`.deb`が
ダウンロードされる。このフォルダをUSBメモリ等でオフラインのWSL
(Ubuntu 22.04 jammy)環境へコピーする。

**注意: このスクリプトはNVIDIA配布リポジトリ(`nvidia.github.io`)への
アクセスが本リポジトリの開発環境からブロックされていたため、実機での
動作確認ができていない。** 想定しているリポジトリ構成(flat repository
形式、`https://nvidia.github.io/libnvidia-container/stable/deb/amd64/Packages`
にインデックスがある)と実際が異なっていた場合、「パッケージが
見つかりませんでした」という警告が出るか、ダウンロードが404で失敗する。
その場合は以下の手動フォールバックを使うこと:

1. ブラウザで https://nvidia.github.io/libnvidia-container/stable/deb/nvidia-container-toolkit.list
   を開き、実際の`deb`行に書かれているリポジトリのベースURLを確認する
2. `<そのベースURL>/Packages` (アーキテクチャ別ディレクトリが挟まる
   構成なら `<ベースURL>/amd64/Packages` や `<ベースURL>/binary-amd64/Packages`
   等)をブラウザで直接開き、`Package: nvidia-container-toolkit` /
   `Package: nvidia-container-toolkit-base` / `Package: libnvidia-container-tools` /
   `Package: libnvidia-container1` の各ブロックを探す
3. 各ブロックの`Filename:`の値を`<ベースURL>/<Filename>`と組み合わせた
   URLをブラウザで直接開けばダウンロードできる

なお依存関係(`libc6`, `libseccomp2`等)はUbuntu 22.04であれば標準で
入っているため、通常はこの4パッケージ以外に追加ダウンロードは不要。
`apt-get install ./*.deb`で「dependency problems」と言われた場合のみ、
`doc/rust-urls-jammy.txt`(Ubuntu標準パッケージのURL一覧、既に生成済み)
から該当パッケージを探して追加すること。

## オフライン環境(WSL内)側でのインストール

`dpkg -i *.deb`だと依存関係の解決順序でエラーになることがあるため、
`doc/readme.md`のRust向け手順と同じく`apt-get install ./*.deb`を使う:

```
cd nvidia-container-toolkit-offline-debs
sudo apt-get install ./*.deb
```

インストール後、既存のDocker daemonにNVIDIAランタイムを認識させる
設定を行う(`/etc/docker/daemon.json`を書き換え、dockerを再起動する):
```
sudo nvidia-ctk runtime configure --runtime=docker
sudo systemctl restart docker   # WSLでsystemd無効の場合: sudo service docker restart
```

## 動作確認

```
nvidia-smi                                          # WindowsのGPUドライバがWSLから見えるか
docker run --rm --gpus all nvidia/cuda:12.6.0-base-ubuntu22.04 nvidia-smi
```

2つ目のコマンドがコンテナ内から`nvidia-smi`の出力を表示できれば、
NVIDIA Container Toolkitが正しく機能している。(このコマンド自体は
Docker Hubからイメージを取得するためネット接続が必要。完全オフラインで
検証したい場合は、ネットのある環境で`docker pull ... && docker save ...`
し、`doc/Dockerfile.radonpy`のコメントにある`docker save`/`docker load`
と同じ要領でイメージ自体もUSB等で持ち込むこと)
