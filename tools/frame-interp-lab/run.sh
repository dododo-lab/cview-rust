#!/bin/zsh
# 프레임 보간 연구 도구 빌드·측정. 앱 빌드·배포와 무관하다.
# 사용: tools/frame-interp-lab/run.sh [샘플.mp4]
#   샘플을 주지 않으면 합성 장면만 잰다. 공개 1080p60 샘플 예:
#   curl -r 0-23454228 -o /tmp/bipbop.mp4 \
#     https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_fmp4/v9/main.mp4
set -euo pipefail
HERE=${0:A:h}
OUT=${CVIEW_TARGET:-/Volumes/data/project/Rust/CView/target}/frame-interp-lab
[[ -d ${OUT:h} ]] || OUT=$HERE/.build
mkdir -p $OUT

clang -O2 -fobjc-arc -Wno-deprecated-declarations -mmacosx-version-min=26.0 \
  -framework Foundation -framework AVFoundation -framework VideoToolbox -framework CoreVideo \
  -framework CoreMedia -framework CoreText -framework CoreGraphics -framework ImageIO \
  -framework UniformTypeIdentifiers $HERE/bench.m -o $OUT/bench
swiftc -O $HERE/run-webgpu.swift -o $OUT/run-webgpu

q() { $OUT/bench "$@" 2>/dev/null | /usr/bin/grep -v '^# 지원'; echo; }

echo "## VT 저지연 보간, 합성 장면, 30→60"
for s in 640x360 960x540 1280x720 1920x1080; do q --algo lowlat --size $s --frames 90; done
echo "## VT 저지연 보간, 60→120 (720p)"
q --algo lowlat --size 1280x720 --in-fps 60 --frames 120
echo "## VT 저지연 보간, 1080p 를 720p 로 줄여 보간"
q --algo lowlat --proc-size 1280x720 --frames 90
echo "## VT 저지연 보간, 1080p 비동기 처리량"
for n in 1 2 4; do q --algo lowlat --frames 60 --inflight $n; done
echo "## VT 프레임률 변환(오프라인용), 1080p"
q --algo frc --frames 30
if [[ $# -ge 1 ]]; then
  echo "## 실제 영상: $1"
  q --source $1 --algo lowlat --frames 150
  q --source $1 --algo lowlat --max-width 1280 --frames 150
fi
echo "## WebView(WKWebView) 안 WebGPU 블록 매칭"
for p in "w=1920&h=1080&r=48" "w=1920&h=1080&r=24" "w=1280&h=720&r=32"; do $OUT/run-webgpu $HERE/webgpu-bench.html $p; done
