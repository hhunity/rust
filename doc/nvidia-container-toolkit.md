# WSL2 + NVIDIA Container Toolkit をオフラインで導入する (doc/nvidia-container-toolkit.md)

Windows上のWSL2 (Ubuntu 22.04 jammy) で、GPU対応のDockerコンテナ
(`docker run --gpus all ...`)を動かせるようにするための一式。
対象環境にネット接続がない前提で、**必要なファイルをすべてWindows側で
先にダウンロードしておき、USB等でオフラインのWSL環境へ持ち込む**手順を
まとめる。

## 全体構成(3層)

GPU対応DockerをWSL2で動かすには、以下の3つが別々に必要になる。
どれか1つでも欠けると動かないので注意。

| 層 | 何を | どこに入れるか |
|---|---|---|
| 1. GPUドライバ | NVIDIA製Windowsドライバ(WSL向けCUDA対応を内蔵) | **Windows側**(WSL内にはインストールしない) |
| 2. コンテナランタイム | Docker Engine (docker-ce等) | WSL内 (Ubuntu 22.04 jammy) |
| 3. GPU連携ツール | NVIDIA Container Toolkit | WSL内 (Ubuntu 22.04 jammy) |

### 層1: WindowsのGPUドライバ について

WSL2でGPUを使う場合、**GPUドライバはWindows側にのみ入れる**。
WSL(Linux)側で`nvidia-driver-XXX`のような通常のLinux用ドライバを
別途入れてはいけない(Windows側のドライバがWSL越しに透過的に見える
仕組みになっており、二重に入れると衝突する)。

- 511.65 (2022年3月) 以降のGeForce/Studio/Quadroドライバであれば
  WSL2のCUDAサポートが標準で入っている。既にWindows側でGPUを
  使っている(ゲームや通常の描画用にドライバが入っている)なら、
  多くの場合そのままで追加作業不要。
- 未導入・更新したい場合の入手先(バージョンが頻繁に変わるため
  固定URLではなくランディングページを案内する):
  - https://www.nvidia.com/en-us/geforce/drivers/ (GeForce/Studio)
  - https://www.nvidia.com/Download/index.aspx (Quadro/RTX Workstation等、型番指定検索)
- 導入後、WSL内で `nvidia-smi` を実行できればOK(WSL内に別途
  nvidia-smiをインストールする必要はなく、Windows側ドライバの
  ものがそのまま見える)。

### 層2・層3: WSL内(Ubuntu 22.04 jammy)に入れるdebパッケージ

`doc/docker-urls-jammy.txt` … Docker Engine一式(docker-ce, docker-ce-cli,
containerd.io, docker-buildx-plugin, docker-compose-plugin と依存パッケージ、
計186ファイル)。archive.ubuntu.com由来のパッケージとdownload.docker.com
由来のパッケージが混在している。

生成コマンド(自環境がjammyの場合。違うバージョンから生成する場合は
`doc/readme.md`の「自分の環境と違うバージョン向けに生成したい場合」の
節を参照し、sources.listをjammy向けに差し替えて同様に実行する):
```
curl -fsSL https://download.docker.com/linux/ubuntu/gpg -o docker.asc
gpg --dearmor -o docker.gpg docker.asc
echo "deb [arch=amd64 signed-by=$(pwd)/docker.gpg] https://download.docker.com/linux/ubuntu jammy stable" \
  | sudo tee /etc/apt/sources.list.d/docker-tmp.list
sudo apt-get update

apt-get install --print-uris -y -o Dir::State::status=/dev/null \
  docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin \
  | grep -oP "(?<=')[^']+(?=')" | grep '^http' | sort -u > docker-urls-jammy.txt

sudo rm /etc/apt/sources.list.d/docker-tmp.list
```

**NVIDIA Container Toolkit本体のURL一覧については、このリポジトリでは
まだ生成できていない。** NVIDIA配布リポジトリ(`nvidia.github.io`)への
アクセスが本セッションの実行環境からブロックされていたため
(社内/開発環境のプロキシ制限)、`apt-get install --print-uris`を
このリポジトリの自動化からは実行できなかった。**ネットに繋がる
Ubuntu 22.04環境(Windows機のWSLでもよい。オンラインの間に一度だけ
実行すればよい)で以下を実行し、`nvidia-container-urls-jammy.txt`を
生成してから、このファイルと同じ手順でWindows側へ持っていくこと。**

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

## Windows側での一括ダウンロード手順

`doc/docker-urls-jammy.txt`と、上記手順で別途生成した
`nvidia-container-urls-jammy.txt`の両方に対して、それぞれ実行する
(`doc/readme.md`のYocto/Rust向けと同じPowerShellパターン):

```powershell
$outDir = ".\nvidia-wsl-offline-debs"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

# docker-urls-jammy.txt と nvidia-container-urls-jammy.txt の中身を
# 1つのテキストファイル(urls.txt)にまとめてから流し込んでもよい
Get-Content urls.txt | ForEach-Object {
    $fileName = Split-Path $_ -Leaf
    Invoke-WebRequest -Uri $_ -OutFile (Join-Path $outDir $fileName)
}
```

`nvidia-wsl-offline-debs`フォルダをUSBメモリ等でオフラインのWSL
(Ubuntu 22.04 jammy)環境へコピーする。

## オフライン環境(WSL内)側でのインストール

`dpkg -i *.deb`だと依存関係の解決順序でエラーになることがあるため、
`doc/readme.md`のRust向け手順と同じく`apt-get install ./*.deb`を使う:

```
cd nvidia-wsl-offline-debs
sudo apt-get install ./*.deb
```

Docker Engineインストール後、Dockerグループへの追加とサービス起動
(WSLでsystemdが有効な場合。`/etc/wsl.conf`の`[boot] systemd=true`で
有効化していない場合は`sudo service docker start`を使う):
```
sudo usermod -aG docker $USER   # 再ログイン(wsl --shutdown後の再起動)が必要
sudo systemctl enable --now docker
```

NVIDIA Container Toolkitのインストール後、Docker daemonにNVIDIA
ランタイムを認識させる設定(`/etc/docker/daemon.json`を書き換え、
dockerを再起動する):
```
sudo nvidia-ctk runtime configure --runtime=docker
sudo systemctl restart docker
```

## 動作確認

```
nvidia-smi                                          # WindowsのGPUドライバがWSLから見えるか
docker run --rm --gpus all nvidia/cuda:12.6.0-base-ubuntu22.04 nvidia-smi
```

2つ目のコマンドがコンテナ内から`nvidia-smi`の出力を表示できれば、
WSL2 + Docker + NVIDIA Container Toolkitの一式が正しく機能している。
(このコマンド自体はDocker Hubからイメージを取得するためネット接続が
必要。完全オフラインで検証したい場合は、ネットのある環境で
`docker pull nvidia/cuda:12.6.0-base-ubuntu22.04 && docker save ...`し、
`doc/Dockerfile.radonpy`のコメントにある`docker save`/`docker load`と
同じ要領でイメージ自体もUSB等で持ち込むこと)
