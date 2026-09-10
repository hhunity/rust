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

1. `meta-intel-fpga`(a.k.a `meta-intelfpga`)レイヤーを取得し、Yoctoにレイヤー追加。
2. `MACHINE = "agilex"` を設定(開発キット向けの具体的なmachine名は要確認。例: `agilex_socdk`系)。
3. まずは素の `core-image-minimal` をビルドしてSDブート/起動確認。
   - GSRD構成では `u-boot.itb`, `Image`, `socfpga_agilex_socdk.dtb`, `ghrd.core.rbf` が生成される。
   - ここまでは、FPGA抜きで「Linuxがとりあえず起動する」状態を作るのが最初のゴール。

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

## 参考リンク

- [Building Yocto for FPGA boot first config on Agilex 7 - Intel Community](https://community.intel.com/t5/Intel-SoC-FPGA-Embedded/Building-Yocto-for-FPGA-boot-first-config-on-Agilex-7/td-p/1493563)
- [OpenEmbedded Layer Index - meta-intelfpga](https://layers.openembedded.org/layerindex/branch/master/layer/meta-intelfpga/)
- [Building Yocto or Angstrom for SoCFPGA | RocketBoards.org](https://www.rocketboards.org/foswiki/Documentation/BuildingYoctoOrAngstromForSoCFPGA)
- [SoC HPS Remote System Update Example (Agilex 7) - Altera FPGA Developer Site](https://altera-fpga.github.io/rel-24.3.1/embedded-designs/agilex-7/f-series/soc/rsu/ug-rsu-agx7f-soc/)
- [meta-swupdate-boards examples](https://github.com/sbabic/meta-swupdate-boards)
- [SWUpdate documentation](https://sbabic.github.io/swupdate/swupdate.html)

## 未決事項(要確認)

- 開発キットか、カスタムボードか
- 正確な `MACHINE` 名
- ブートメディア構成(SDカードのみか、eMMC+QSPIか)
