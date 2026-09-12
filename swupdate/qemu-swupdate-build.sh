#!/usr/bin/env bash
# poky + meta-swupdate を qemux86-64 向けにビルドし、A/B更新デモをすぐ試せる状態にする。
# doc/Dockerfile.agilex7-dev のイメージビルド時にコンテナ内で実行される。
# (gsrd-socfpgaとは別の、軽量なQEMU練習用のpoky環境。ビルドまで完了させる。)
set -ex

POKY_BRANCH="${POKY_BRANCH:-scarthgap}"

git clone -b "${POKY_BRANCH}" https://git.yoctoproject.org/poky /workspace/qemu-swupdate/poky
git clone https://github.com/openembedded/meta-openembedded /workspace/qemu-swupdate/meta-openembedded
git clone https://github.com/sbabic/meta-swupdate /workspace/qemu-swupdate/meta-swupdate

cd /workspace/qemu-swupdate
source poky/oe-init-build-env build

bitbake-layers add-layer ../meta-openembedded/meta-oe
bitbake-layers add-layer ../meta-swupdate

{
	echo 'MACHINE = "qemux86-64"'
	echo 'IMAGE_INSTALL:append = " swupdate swupdate-www"'
} >> conf/local.conf

bitbake core-image-full-cmdline
