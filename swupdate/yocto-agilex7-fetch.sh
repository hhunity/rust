#!/usr/bin/env bash
# gsrd-socfpgaのソース一式(downloads/, sstate_cache/)をbitbakeの正規フェッチャーで
# 取得するだけのスクリプト(コンパイルはしない)。
# doc/Dockerfile.yocto-agilex7-fetch のイメージビルド時にコンテナ内で実行される。
#
# ネット接続がある環境(Windows Docker Desktop等)で `docker build` する際にのみ
# 実行され、生成された downloads/ 一式はイメージのレイヤーに焼き込まれる。
set -ex

POKY_VERSION="${POKY_VERSION:-scarthgap}"
BUILD_SCRIPT="${BUILD_SCRIPT:-agilex7_dk_si_agf014ea-gsrd-build.sh}"

git clone -b "${POKY_VERSION}" https://github.com/altera-fpga/gsrd-socfpga.git /workspace/gsrd-socfpga
cd /workspace/gsrd-socfpga
git submodule update --init -r

source "./${BUILD_SCRIPT}"
build_setup

# meta-swupdateレイヤーも追加(add-swupdate-layer.shと同じ内容をここでも実行)
git clone https://github.com/sbabic/meta-swupdate.git "${WORKSPACE}/meta-swupdate"
cd "${WORKSPACE}/${MACHINE}-${IMAGE}-rootfs"
bitbake-layers add-layer ../meta-swupdate
echo 'IMAGE_INSTALL:append = " swupdate swupdate-www"' >> conf/site.conf

# 現行Rustツールチェーン用のmeta-lts-mixinsレイヤーも追加(add-rust-layer.shと同じ内容)
git clone -b scarthgap/rust https://git.yoctoproject.org/meta-lts-mixins "${WORKSPACE}/meta-lts-mixins"
bitbake-layers add-layer ../meta-lts-mixins

bitbake console-image-minimal gsrd-console-image --runall=fetch
