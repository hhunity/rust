# SWUpdateによるAgilex 7ボードのLinuxアップデート導入メモ

対象ボード: Intel Agilex 7 SoC FPGA(開発キット。カスタムボードの場合は都度読み替え)

## 全体方針

- Agilex 7は2階層の独立した更新機構を持つ想定で進める。
  1. **SDM(Secure Device Manager)レベル**: 電源投入直後にBootROM→SDMがブートイメージ(HPS初段ローダ設定+FPGAビットストリーム)をQSPI等から読み込む。ここの冗長化・フェイルセーフ更新はIntel純正の**RSU(Remote System Update)**が担当し、SWUpdateの管轄外。
  2. **U-Boot/Linuxレベル**: SDM起動完了後の通常のU-Boot→kernel→rootfsのブートチェーン。Cyclone V/Arria10/Stratix10と共通の`u-boot-socfpga`ブランチを使うため、一般的なU-Boot env方式のA/B切替がそのまま使える。
- まずは **Linux本体(kernel+rootfs)のA/B更新をSWUpdateで実現する** ことを目標にする。FPGA(ビットストリーム)の更新は将来フェーズとし、RSUクライアント経由のカスタムハンドラで対応する方針(下記「将来のFPGA更新について」参照)。

## ボード到着前にPCで試せること

Agilex 7実機が無くても、SWUpdate/Yocto周りは以下がPC(x86 Linux/QEMU)だけで練習・検証できる。優先度順。

### A. 最優先: QEMUでSWUpdateのA/Bデモをまるごと動かす

- poky + `meta-swupdate` をclone、`MACHINE = "qemux86-64"` でビルド(標準のA/Bデモ環境が用意されている)
- `runqemu` でブート
- ローカルWebUI(`http://localhost:8080` 等)から `.swu` を適用
- 反対面への切替を確認
- わざと壊れたイメージでbootcountロールバックを確認
- A/B切替の考え方はボード非依存なので、ここで一通り体験しておけば実機到着後はBSP差分だけ乗せ替える形になる

### B. sw-description / .swuパッケージ作成の練習

ボード無関係。UbuntuにSWUpdateをネイティブビルド/インストールし、ダミーファイル(テキストファイルでも可)で以下を試す。

- `sw-description` の書き方
- `.swu` の作成(cpio化)
- 署名(PKCS7/RSA鍵生成→署名→検証)

### C. hawkBitサーバーをDockerでローカル起動

配信管理サーバー側の操作感(デバイス登録、rollout作成、配信状況確認)を先に把握できる。QEMUイメージのSWUpdateを疑似デバイスとして接続すれば、フリート配信のE2Eも実機なしで試せる。

### D. Yoctoビルド環境の準備・下ごしらえ

- `poky`、`meta-intel-fpga`、`meta-swupdate` の取得とレイヤー構成確認
- kernel/rootfsのビルド自体(FPGAビットストリームを除く部分)は`MACHINE=agilex`でも実機接続なしでビルドは通ることが多い(生成物の動作確認だけは実機かQEMUが必要)

### E. U-BootのA/B切替ロジック単体の検証

QEMU上のU-Boot(またはU-Bootのsandboxターゲット)で `bootcount`/`altbootcmd` の環境変数操作だけを先に試す。Agilexでも同じ`u-boot-socfpga`系の仕組みなので、ロジックの理解はそのまま転用できる。

### 実機無しでは検証不可な範囲

FPGA(RSU)側はSDMとのmailbox通信が必要なため、実機無しでの検証はほぼ不可能。ボード到着待ちでよい。

## Phase 1: BSP環境構築

### レイヤー構成(実リポジトリ確認済み)

`gsrd-socfpga`(https://github.com/altera-fpga/gsrd-socfpga)は、以下をsubmoduleとしてまとめて持っている「一括リポジトリ」。個別にcloneする必要はない。

- `poky` … Yoctoビルド本体
- `meta-intel-fpga` … SoCFPGA BSPコアレイヤー(ハードウェア対応の基本部分)
- `meta-intel-fpga-refdes` … SoCFPGA GSRDカスタマイズレイヤー(`meta-intel-fpga`に依存。デモアプリ等)
- `meta-openembedded`, `meta-clang`, `meta-virtualization` … 追加パッケージ群

### 手順

1. リポジトリをclone。`-b`にはpoky/Yoctoのリリースコードネームを指定する(`$POKY_VERSION`はこのブランチ名のプレースホルダで、poky単体を別途cloneする必要はない)。利用可能なブランチ: `kirkstone`, `langdale`, `mickledore`, `nanbield`, `scarthgap`, `styhead`, `walnascar`, `whinlatter`, `wrynose`。特に理由が無ければ `scarthgap` を使う。
   ```bash
   git clone -b scarthgap https://github.com/altera-fpga/gsrd-socfpga.git
   cd gsrd-socfpga
   git submodule update --init -r
   ```
2. ボードに応じたビルドスクリプトをsource(Agilex7 DK-SI-AGF014EAの例。Intel純正開発キット向け。他の対応ボード一覧はリポジトリの`README.md`参照)。
   ```bash
   . agilex7_dk_si_agf014ea-gsrd-build.sh
   ```
3. ビルド環境をセットアップ(内部で`poky/oe-init-build-env`を呼び、`meta-intel-fpga` / `meta-intel-fpga-refdes` / `meta-openembedded`配下等を`bblayers.conf`に自動追加する)。
   ```bash
   build_setup
   ```
4. デフォルトGSRDビルド(`console-image-minimal` と `gsrd-console-image` の2イメージをビルドする)。
   ```bash
   build_default
   ```
   ステップバイステップで進めたい場合は`build_default`の代わりに`bitbake_image`を直接呼ぶ(`build_setup`の後に実行)。

### Apollo Agilex SOM(Terasic製)を使う場合の注意

上記の`gsrd-socfpga`標準スクリプトはIntel純正開発キット向け。Terasic Apollo Agilex SOMのようなサードパーティボードは、DDR構成・pinmux・デバイスツリーがボード固有のため、**Terasicが配布するBSP/Yoctoレイヤー**(Terasic Download Center、またはRocketBoards.orgの`TerasicApolloAgilexRSOM`ページ参照)を別途使う必要がある。`meta-intel-fpga`(SoCコア部分)は共通で使えるが、`meta-intel-fpga-refdes`相当のボード固有部分はTerasic版に差し替える。

## Phase 2: ブートメディア/パーティション設計

4. SWUpdateでA/B管理するのは基本 **kernel + rootfs**(u-boot.itb内のFITイメージやrootfs)。QSPIに置くSDM用ブートイメージ(RSU管轄)とは別パーティション/別メディアに分離するのが無難。
   - dev kit標準構成だとSDカード運用が多いが、実運用ではeMMC+QSPIの組み合わせになることが多いため、量産ボード構成を要確認。
5. kernel_A/rootfs_A, kernel_B/rootfs_B の2面構成でパーティションレイアウトを決定。

```
part1: BOOT(共通, FSBL/u-boot等)
part2: kernel_A + part3: rootfs_A
part4: kernel_B + part5: rootfs_B
part6: data(共通、永続領域)
```

## Phase 3: U-Boot A/B設定

6. `u-boot-socfpga`は`bootcount`/`altbootcmd`をサポートしているため、bootcount方式のfail-safe A/B切替を適用する。
7. boot scriptで現在有効な面のkernel/dtb/rootfsを選択するロジックを実装する。

## Phase 4: SWUpdate導入

8. `meta-swupdate` + `meta-swupdate-boards`(dual-copy方式のリファレンス実装例あり。Beaglebone等向けだが考え方はそのまま流用可)を参考に、Agilex向けに `sw-description` を書き起こす。
9. u-boot.itb(kernel FIT image)とrootfsをそれぞれA/B面に書き込むよう定義する。
10. 署名設定(PKCS7等)。本番運用では署名必須にすべき。

## Phase 5: ローカル検証

11. USB/ネットワーク経由で `.swu` を適用し、以下を確認する。
    - 適用後リブートして反対面から起動しているか
    - わざと壊れたイメージを書いて、bootcount上限超過で自動ロールバックするか(本番前に必ず確認)

## Phase 6: 配信の仕組み(将来)

- 最初はローカル配信で十分。将来フリート管理するならhawkBit連携(SWUpdateはhawkBit対応クライアント同梱)を検討。

## 将来のFPGA更新について

- Agilex 7には「SoC HPS Remote System Update Example」というIntel公式のRSUリファレンスがある。
- FPGA更新フェーズに入ったら、SWUpdateから直接ビットストリームを焼くのではなく、**RSUクライアント(rsu_client等、SDMとのmailbox通信ツール)を呼び出すカスタムハンドラ**を書いて、RSUのマルチイメージ/フェイルオーバー機構に更新イメージを渡す方針とする。
- Zynq系でよく使われる`fpga_manager`(`/sys/class/fpga_manager`)直叩きのアプローチとは異なる点に注意。

## GSRDにSWUpdateを追加する

`meta-swupdate`は通常のYoctoレイヤーなので、`gsrd-socfpga`のビルドにそのまま追加できる。

```bash
# gsrd-socfpgaのルートで、通常通りセットアップ
. agilex7_dk_si_agf014ea-gsrd-build.sh
build_setup

# meta-swupdateレイヤーを追加(swupdate/add-swupdate-layer.sh)
../add-swupdate-layer.sh   # gsrd-socfpga直下に配置した場合のパス例。実際の配置に合わせて調整

bitbake_image
```

`swupdate/add-swupdate-layer.sh` がやること:
- `meta-swupdate`(https://github.com/sbabic/meta-swupdate)をclone
- `bitbake-layers add-layer` でレイヤー追加(`meta-openembedded/meta-oe`は`build_setup`で既に追加済みなので依存関係もOK)
- `IMAGE_INSTALL:append = " swupdate swupdate-www"` を`conf/site.conf`に追記

### 注意: `build_setup`を再実行するたびにレイヤー追加をやり直す必要がある

`build.sh`の`build_setup()`は呼び出すたびに`$MACHINE-$IMAGE-rootfs/conf/`を削除して作り直す(`bblayers.conf`もリセットされる)。そのため、クリーンビルドし直すたびに`add-swupdate-layer.sh`を再実行してレイヤーを足し直す必要がある。恒久的に組み込みたい場合は、`build.sh`の`build_setup()`関数自体に`bitbake-layers add-layer ../meta-swupdate`の行を追記してしまう方が手間が少ない。

### 次にやること(`sw-description`側)

レイヤーを追加しただけではA/B更新は動かない。前述のPhase 3〜4(U-Boot bootcount設定、`sw-description`作成、署名設定)と組み合わせる必要がある。`meta-swupdate`単体は「SWUpdate本体をビルドに含める」ところまでで、A/Bパーティション定義やコピー先の指定は`sw-description`側の作業。

## オフライン環境(ネット未接続のLinux/WSL)でのビルド

Yoctoのビルド(`bitbake`)には大きく3種類のものが必要で、どこまでWindows単体で完結するかが異なる。

| # | 必要なもの | Windows単体(PowerShell/Git)で可能? |
|---|---|---|
| ① | ビルドホスト用aptパッケージ(.deb) | 可能(`doc/yocto-urls.txt`と同じ仕組み) |
| ② | `gsrd-socfpga`本体+submodule一式 | 可能(通常の`git clone`) |
| ③ | 各レシピが要求するソース本体(kernel, u-boot等。bitbakeの`DL_DIR`) | **不可**。`bitbake`自体の実行(=Linux環境)が必要 |

③は`bitbake`の`do_fetch`タスクがレシピの`SRC_URI`を実際に取得する処理そのもので、Windows上で手作業のダウンロードに置き換えるのは非推奨(特にgit系ソースは`DL_DIR`内で特殊な命名規則のbareリポジトリとして保存されるため、手動再現すると壊れやすい)。

### WSLがネットワークポリシーで塞がれている場合: Dockerイメージごと転送する方式

`doc/Dockerfile.ml-gpu`と同じ「Dockerイメージごと転送する」方式が使える。WSL自体のネットワーク設定には一切触れずに済む。

- `swupdate/Dockerfile.yocto-agilex7-fetch` … ①②③を全部イメージに焼き込む(Windows Docker Desktop側で`docker build`する。ここでネットを使う)
- `swupdate/yocto-agilex7-fetch.sh` … コンテナ内で`gsrd-socfpga`をclone→`build_setup`→`bitbake ... --runall=fetch`(フェッチのみ、コンパイルはしない)を行うスクリプト

```powershell
# Windows Docker Desktop側(ネット接続あり)
docker build -f swupdate/Dockerfile.yocto-agilex7-fetch `
  --build-arg POKY_VERSION=scarthgap `
  --build-arg BUILD_SCRIPT=agilex7_dk_si_agf014ea-gsrd-build.sh `
  -t yocto-agilex7-fetch:latest swupdate/
docker save yocto-agilex7-fetch:latest | gzip > yocto-agilex7-fetch.tar.gz
```

USB等でオフライン機(Docker環境)へ転送後:

```bash
docker load < yocto-agilex7-fetch.tar.gz
docker run --rm -it yocto-agilex7-fetch:latest bash
```

コンテナ内(ネット不要。`downloads/`はイメージに焼き込み済み):

```bash
cd /workspace/gsrd-socfpga
source ./agilex7_dk_si_agf014ea-gsrd-build.sh
build_setup
../add-swupdate-layer.sh   # meta-swupdateレイヤーを再度有効化(build_setupのたびに必要)
bitbake_image
```

**注意**: このDockerfile/スクリプトは`gsrd-socfpga/build.sh`の実際の中身を読んで作成したもので、構造面は確認済みだが、`docker build`を最後まで実際に走らせての動作確認はまだ行っていない(フェッチ対象のソース総量が数GB〜十数GBになり、時間もかかるため)。初回はWindows側で`docker build`を実行し、エラーなく完走するか確認すること。

## 参考リンク

- [Building Yocto for FPGA boot first config on Agilex 7 - Intel Community](https://community.intel.com/t5/Intel-SoC-FPGA-Embedded/Building-Yocto-for-FPGA-boot-first-config-on-Agilex-7/td-p/1493563)
- [OpenEmbedded Layer Index - meta-intelfpga](https://layers.openembedded.org/layerindex/branch/master/layer/meta-intelfpga/)
- [Building Yocto or Angstrom for SoCFPGA | RocketBoards.org](https://www.rocketboards.org/foswiki/Documentation/BuildingYoctoOrAngstromForSoCFPGA)
- [SoC HPS Remote System Update Example (Agilex 7) - Altera FPGA Developer Site](https://altera-fpga.github.io/rel-24.3.1/embedded-designs/agilex-7/f-series/soc/rsu/ug-rsu-agx7f-soc/)
- [meta-swupdate-boards examples](https://github.com/sbabic/meta-swupdate-boards)
- [SWUpdate documentation](https://sbabic.github.io/swupdate/swupdate.html)

## 未決事項(要確認)

- 使用予定ボード: Terasic Apollo Agilex SOM。Terasic配布のBSP/Yoctoレイヤーの入手元・ビルド手順を確認する必要あり(Terasic Download Center、RocketBoards.org `TerasicApolloAgilexRSOM` ページ参照)
- 正確な `MACHINE` 名(Terasic版BSPで定義されるもの)
- ブートメディア構成(SDカードのみか、eMMC+QSPIか)
- `swupdate/Dockerfile.yocto-agilex7-fetch` の実ビルド動作確認(Windows Docker Desktop側で未実施)
