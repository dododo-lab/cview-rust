#!/bin/zsh
# VFI 개발 계획(docs/research/vfi-dev-plan.md) 실험 재현: 1단계(보호 장치·서킷브레이커·RIFE GPU 시간),
# 3단계(RIFE 학습 가중치 정합·화질, 가중치가 있을 때만).
# 사용: tools/frame-interp-lab/run-plan.sh [결과폴더]
#   결과폴더를 주면 절마다 로그 파일을 남긴다(없으면 화면에만). 앱 빌드·배포와 무관하다.
#   전부 명령줄 도구이며 창을 띄우지 않는다. 걸리는 시간: M1 Max 약 8분.
set -euo pipefail
HERE=${0:A:h}
OUT=${CVIEW_TARGET:-/Volumes/data/project/Rust/CView/target}/frame-interp-lab
[[ -d ${OUT:h} ]] || OUT=$HERE/.build
mkdir -p $OUT
LOG=${1:-}
[[ -n $LOG ]] && mkdir -p $LOG

# RIFE 가중치(rife-export.py 출력)가 있는 곳. <폴더>/rife-4.25/export, <폴더>/rife-4.25-lite/export
RW=${CVIEW_RIFE_WEIGHTS:-/Volumes/ssd2/cview-vfi/weights}
FW=(-framework Foundation -framework AVFoundation -framework VideoToolbox -framework CoreVideo -framework CoreMedia
    -framework CoreText -framework CoreGraphics -framework ImageIO -framework UniformTypeIdentifiers)
MW=(-framework Foundation -framework Metal -framework MetalPerformanceShaders -framework MetalPerformanceShadersGraph)
clang -O2 -fobjc-arc -Wno-deprecated-declarations -Wno-gnu-folding-constant -mmacosx-version-min=26.0 $FW $MW $HERE/bench.m -o $OUT/bench
clang -O2 -fobjc-arc -mmacosx-version-min=26.0 $MW $HERE/rife-mpsgraph.m -o $OUT/rife-mpsgraph
clang -O2 -fobjc-arc -mmacosx-version-min=26.0 $MW $HERE/mps-ops-micro.m -o $OUT/mps-ops-micro
clang -O2 -fobjc-arc -mmacosx-version-min=26.0 $MW $HERE/mpp-conv-micro.m -o $OUT/mpp-conv-micro

sec() { echo; echo "## $1"; }
q() { "$@" 2>/dev/null | /usr/bin/grep -v '^# 지원'; echo; }
log() { local name=$1; shift; if [[ -n $LOG ]]; then { "$@" } 2>&1 | tee $LOG/$name.txt; else "$@"; fi; }

guards() {
  sec "보호 장치: 합성 장면(장면 전환 포함), VT 저지연 보간 + cut/static/conf"
  for s in 1280x720 1920x1080; do
    q $OUT/bench --algo lowlat --size $s --frames 90 --guards --cuts 15
    q $OUT/bench --algo lowlat --size $s --frames 90 --guards
    q $OUT/bench --algo lowlat --size $s --in-fps 60 --frames 120 --guards --cuts 30
  done
  echo "### 비교: conf 를 이동 하나로만 판단(--g-conf-layers 1), 정지 판정 4장 고정(--g-static-k 4)"
  q $OUT/bench --algo lowlat --size 1920x1080 --frames 90 --guards --g-conf-layers 1
  q $OUT/bench --algo lowlat --size 1920x1080 --in-fps 60 --frames 120 --guards --cuts 30 --g-static-k 4
}

realtime() {
  sec "실시간 모사(절대 시간표, 비동기 1건) + 마감 서킷브레이커"
  for b in off v1 on; do q $OUT/bench --rt 30 --size 1280x720 --in-fps 60 --breaker $b; done
  for b in off on; do q $OUT/bench --rt 20 --size 1920x1080 --in-fps 30 --breaker $b; done
  echo "### 720p 60→120, 다른 프로세스가 1080p 보간으로 ANE 를 계속 점유하는 동안"
  for b in off on; do
    $OUT/bench --algo lowlat --size 1920x1080 --frames 1000000 --loop 34 >/dev/null 2>&1 &
    local hog=$!
    sleep 3
    q $OUT/bench --rt 30 --size 1280x720 --in-fps 60 --breaker $b
    wait $hog || true
  done
}

rife() {
  sec "RIFE 4.25 / 4.25.lite IFNet 복제본(무작위 가중치) — MPSGraph GPU 시간"
  for a in "" "--fast phase" "--fast 3x3"; do
    for s in 1280x720 1920x1080; do q $OUT/rife-mpsgraph --size $s --iters 50 ${=a}; done
  done
  for a in "--variant lite" "--scale 0.5" "--variant lite --scale 0.5" "--warp32" "--dtype f32"; do
    q $OUT/rife-mpsgraph --size 1920x1080 --iters 50 --fast 3x3 ${=a}
  done
  sec "MPSGraph 연산별 1080p 미세 측정"
  q $OUT/mps-ops-micro
  sec "Metal 4 MPP tensor_ops::convolution2d vs MPSGraph (값 정합 확인 포함)"
  q env MPP_CPU_CHECK=1 $OUT/mpp-conv-micro --iters 5
  q $OUT/mpp-conv-micro
}

rifeq() {
  local w=$RW/rife-4.25/export wl=$RW/rife-4.25-lite/export
  if [[ ! -f $w/weights.bin || ! -f $wl/weights.bin ]]; then echo "RIFE 가중치 없음($RW) — rife-export.py 로 만든 뒤 다시 실행"; return 0; fi
  sec "RIFE 학습 가중치: CPU 기준 구현과 정합"
  q $OUT/rife-mpsgraph --check --weights $w
  q $OUT/rife-mpsgraph --check --weights $w --scale 0.5
  q $OUT/rife-mpsgraph --check --weights $wl --variant lite
  q $OUT/rife-mpsgraph --check --weights $wl --variant lite --scale 0.5 --size 512x256
  sec "RIFE 학습 가중치: 1080p GPU 시간(FP32, 빠른 구성 3x3)"
  for a in "--weights $w" "--weights $w --scale 0.5" "--weights $wl --variant lite"; do
    q $OUT/rife-mpsgraph --size 1920x1080 --iters 50 --fast 3x3 --dtype f32 ${=a}
  done
  sec "RIFE 학습 가중치: 합성 장면 화질(FP32, 빠른 구성 3x3) + 보호 장치"
  for s in 1280x720 1920x1080; do
    for m in "--rife-weights $w" "--rife-weights $wl --rife-variant lite"; do
      q $OUT/bench --algo rife ${=m} --size $s --frames 90 --guards
      q $OUT/bench --algo rife ${=m} --size $s --frames 90 --guards --cuts 15
      q $OUT/bench --algo rife ${=m} --size $s --in-fps 60 --frames 120 --guards --cuts 30
    done
  done
  echo "### 1080p 흐름 절반 해상도(scale 0.5)"
  q $OUT/bench --algo rife --rife-weights $w --rife-scale 0.5 --size 1920x1080 --frames 90 --guards
  q $OUT/bench --algo rife --rife-weights $w --rife-scale 0.5 --size 1920x1080 --frames 90 --guards --cuts 15
  q $OUT/bench --algo rife --rife-weights $w --rife-scale 0.5 --size 1920x1080 --in-fps 60 --frames 120 --guards --cuts 30
  echo "### 입력 채널 순서 BGR(학습 데이터가 cv2 로 읽혔을 가능성 확인)"
  q $OUT/bench --algo rife --rife-weights $w --rife-bgr --size 1920x1080 --frames 90 --guards
}

# 절 하나만: run-plan.sh <결과폴더> guards|realtime|rife|rifeq
if [[ $# -ge 2 ]]; then for p in ${@:2}; do log $p $p; done; exit 0; fi
log guards guards
log realtime realtime
log rife rife
log rifeq rifeq
