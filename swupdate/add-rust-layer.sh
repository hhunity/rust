#!/usr/bin/env bash
# 既にセットアップ済みのgsrd-socfpgaビルド環境に、現行版Rustツールチェーンを
# 使えるようにする meta-lts-mixins(scarthgap/rust)レイヤーを追加する。
#
# 前提: gsrd-socfpgaのルートで
#   . agilex7_dk_si_agf014ea-gsrd-build.sh
#   build_setup
# を実行済みであること(add-swupdate-layer.shと同じタイミングで実行する)。
#
# 背景: scarthgapのoe-coreに標準搭載されているRustは1.59.0と古く、最近のcrateの
# ビルドには不足しがち。meta-lts-mixinsはoe-core masterから現行Rustレシピを
# backportした公式の補完レイヤー。
#
# 注意: これは独自アプリ(meta-myapp)そのものではなく、Rustのビルド基盤を
# 提供するだけのレイヤー。アプリ本体は別途 meta-myapp を用意すること
# (READMEの「GSRDに独自アプリケーションを含める」参照)。
#
# 注意: build_setupを再実行するたびにレイヤーがリセットされるため、
# 毎回このスクリプトも再実行が必要(add-swupdate-layer.shと同じ)。
#
# オフライン環境での動作について: add-swupdate-layer.shと同じ理由で、
# ${WORKSPACE}/meta-lts-mixins はDockerfile.agilex7-dev経由なら
# docker build時(ネットあり)にyocto-agilex7-fetch.shが既にcloneして
# イメージに焼き込み済みのため、ここでのgit cloneは実際にはスキップされ、
# オフラインで問題なく動く。
set -eux

: "${WORKSPACE:?build_setup前提のWORKSPACE変数が未設定。先に <machine>-<image>-build.sh をsourceしてください}"
: "${MACHINE:?}"
: "${IMAGE:?}"

cd "$WORKSPACE"
if [ ! -d meta-lts-mixins ]; then
	git clone -b scarthgap/rust https://git.yoctoproject.org/meta-lts-mixins
fi

cd "$WORKSPACE/$MACHINE-$IMAGE-rootfs"
bitbake-layers add-layer ../meta-lts-mixins

echo -e "\n[INFO] meta-lts-mixins(rust) layer added. Now add your own app layer (meta-myapp)."
