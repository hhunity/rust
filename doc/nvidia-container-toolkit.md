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

## NVIDIA Container Toolkit本体のURL一覧について

**このリポジトリの自動生成環境では、NVIDIA配布リポジトリ
(`nvidia.github.io`)へのアクセスがプロキシでブロックされており、
`apt-get install --print-uris`を実行してURL一覧を生成することが
できなかった。** 以下のコマンドを、ネットに繋がるUbuntu 22.04環境
(オンラインの間だけのWSLで一度実行するのでもよい)で実行し、
`nvidia-container-urls-jammy.txt`を作成してから、次の「Windows側での
一括ダウンロード」に進むこと。

```
# 1. NVIDIA配布リポジトリのGPGキーとリスト定義を取得
curl -fsSL https://nvidia.github.io/libnvidia-container/gpgkey \
  | gpg --dearmor -o nvidia-container-toolkit-keyring.gpg
curl -s -L https://nvidia.github.io/libnvidia-container/stable/deb/nvidia-container-toolkit.list \
  | sed "s#deb https://#deb [arch=amd64 signed-by=$(pwd)/nvidia-container-toolkit-keyring.gpg] https://#g" \
  | sudo tee /etc/apt/sources.list.d/nvidia-container-toolkit-tmp.list
sudo apt-get update

# 2. パッケージ本体+依存(全てUbuntu標準リポジトリ分も含めて解決される)のURL一覧を生成
apt-get install --print-uris -y -o Dir::State::status=/dev/null \
  nvidia-container-toolkit \
  | grep -oP "(?<=')[^']+(?=')" | grep '^http' | sort -u > nvidia-container-urls-jammy.txt

sudo rm /etc/apt/sources.list.d/nvidia-container-toolkit-tmp.list
```

`nvidia-container-toolkit`パッケージは以下に依存しており
(2026年9月時点)、上記コマンドで生成される一覧にはこれらが
全て含まれるはずなので、個別に意識する必要はない:
- `nvidia-container-toolkit-base`
- `libnvidia-container-tools`
- `libnvidia-container1`

依存はほぼUbuntu標準ライブラリ(`libc6`, `libseccomp2`等)に収まるため、
生成される一覧のほとんどはNVIDIA配布リポジトリ由来の上記4パッケージ
そのものになるはずである。

## Windows側での一括ダウンロード手順

`nvidia-container-urls-jammy.txt`をWindows上の作業フォルダに置き、
PowerShellで一括ダウンロードする(`doc/readme.md`のYocto/Rust向けと
同じパターン):

```powershell
$outDir = ".\nvidia-container-toolkit-offline-debs"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

Get-Content nvidia-container-urls-jammy.txt | ForEach-Object {
    $fileName = Split-Path $_ -Leaf
    Invoke-WebRequest -Uri $_ -OutFile (Join-Path $outDir $fileName)
}
```

`nvidia-container-toolkit-offline-debs`フォルダをUSBメモリ等で
オフラインのWSL(Ubuntu 22.04 jammy)環境へコピーする。

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
