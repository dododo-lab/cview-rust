#!/bin/zsh
# metal-bench/run.sh [기준 git ref=origin/main] [반복=2] [측정 초=12]
#   기준 ref 의 player.html(old/)과 지금 작업 트리의 player.html(new/)을 숨은 WKWebView 로 비교한다.
#   창은 화면 밖에만 뜨고 앱을 활성화하지 않는다 — 보고 있는 화면·포커스를 건드리지 않는다.
set -e
HERE=${0:A:h}; REPO=${HERE:h:h:h}
BASE=${1:-origin/main}; REPEAT=${2:-2}; SECS=${3:-12}
OUT=$REPO/target/perf-tools/metal-bench
mkdir -p $OUT/root/old $OUT/root/new
swiftc -O -o $OUT/bench $HERE/bench.swift -framework AppKit -framework WebKit
[[ -f $OUT/root/clip.mp4 ]] || { swiftc -O -o $OUT/gen-clip $HERE/gen-clip.swift -framework AVFoundation && $OUT/gen-clip $OUT/root/clip.mp4; }
git -C $REPO show $BASE:app-ui/player/player.html > $OUT/root/old/player.html
cp $REPO/app-ui/player/hls.min.js $OUT/root/old/
cp $REPO/app-ui/player/player.html $REPO/app-ui/player/hls.min.js $OUT/root/new/
python3 $HERE/run.py $OUT/root $OUT/bench $REPEAT $SECS | tee $OUT/result-$(date +%Y%m%d-%H%M%S).txt
