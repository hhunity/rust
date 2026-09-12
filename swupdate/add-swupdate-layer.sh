#!/usr/bin/env bash
# 既にセットアップ済みのgsrd-socfpgaビルド環境に meta-swupdate レイヤーを追加する。
#
# 前提: gsrd-socfpgaのルートで
#   . agilex7_dk_si_agf014ea-gsrd-build.sh
#   build_setup
# を実行済みであること(このスクリプトはその直後に実行する)。
#
# 注意: build.sh の build_setup() は呼び出すたびに $MACHINE-$IMAGE-rootfs/conf/ を
# 削除して作り直す(=bblayers.confがリセットされる)。そのため build_setup を
# 再実行するたびに、このスクリプトも再実行してレイヤーを足し直す必要がある。
#
# オフライン環境での動作について: 下のgit cloneは「${WORKSPACE}/meta-swupdate が
# 無ければ」実行される。Dockerfile.agilex7-dev経由で使う場合、そのディレクトリは
# docker build時(ネットあり)に yocto-agilex7-fetch.sh が既にcloneしてイメージに
# 焼き込み済みのため、ここでは実際にはgit cloneは走らずスキップされる
# (=このスクリプト自体はオフラインで問題なく動く)。単体で流用する場合は、
# 事前に同じ場所へmeta-swupdateをcloneしておくこと。
set -eux

: "${WORKSPACE:?build_setup前提のWORKSPACE変数が未設定。先に <machine>-<image>-build.sh をsourceしてください}"
: "${MACHINE:?}"
: "${IMAGE:?}"

POKY_VERSION="${POKY_VERSION:-scarthgap}"

cd "$WORKSPACE"
if [ ! -d meta-swupdate ]; then
	# pokyのブランチ(LAYERSERIES_COMPAT)と合わせないと
	# 「not compatible with the core layer」で失敗するため、必ず-bを指定する
	git clone -b "${POKY_VERSION}" https://github.com/sbabic/meta-swupdate.git
fi

cd "$WORKSPACE/$MACHINE-$IMAGE-rootfs"
bitbake-layers add-layer ../meta-swupdate

# swupdateパッケージ本体とWeb UIを最終イメージに含める
echo 'IMAGE_INSTALL:append = " swupdate swupdate-www"' >> conf/site.conf

echo -e "\n[INFO] meta-swupdate layer added. Proceed with: bitbake_image"
