#!/bin/zsh
# vfi-player 빌드. 출력: ${CVIEW_TARGET:-/Volumes/data/project/Rust/CView/target}/vfi-native-player/vfi-player
# (외장 드라이브가 없으면 이 폴더의 .build)
set -euo pipefail
HERE=${0:A:h}
BASE=${CVIEW_TARGET:-/Volumes/data/project/Rust/CView/target}
[[ -d ${BASE:h} ]] || BASE=$HERE/.build
OUT=${1:-$BASE/vfi-native-player}
mkdir -p $OUT
xcrun clang -O2 -fobjc-arc -Wall -Wno-unused-variable -Wno-unused-function -mmacosx-version-min=26.0 \
  -framework Foundation -framework AppKit -framework AVFoundation -framework VideoToolbox \
  -framework CoreMedia -framework CoreVideo -framework CoreText -framework CoreGraphics -framework QuartzCore \
  $HERE/vfi-player.m -o $OUT/vfi-player
echo $OUT/vfi-player
