#!/bin/zsh
# ev.sh script.js "marker" [timeout_s] — 앱의 모든 창에서 JS 실행, marker 줄이 올 때까지 출력 대기
OUT=/tmp/cview-dom.txt; : > $OUT; cp $1 /tmp/cview-eval.js
t=${3:-120}; for i in $(seq 1 $((t*2))); do grep -q "^$2" $OUT 2>/dev/null && break; sleep 0.5; done; cat $OUT
