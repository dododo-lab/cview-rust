#!/bin/zsh
# picft.sh — 선명한 화면·보정 종합 기능 테스트(숨긴 실행, 시험 번들 ID)
T=/Volumes/data/project/Rust/CView/target/perf-tools; APP="$T/pic/CView Test.app"
run(){ : > /tmp/cview-dom.txt; cp $1 /tmp/cview-eval.js; open -g -j -n --env CVIEW_DEBUG_EVAL=/tmp/cview-eval.js "$APP"
  for i in $(seq 1 360); do grep -q "^Z done" /tmp/cview-dom.txt && break; sleep 0.5; done
  grep "^R\|^err\|^S" /tmp/cview-dom.txt; osascript -e "tell application \"$APP\" to quit" >/dev/null 2>&1; sleep 3; }
echo "== A"; run $T/js/pic-a.js
CH=$(grep "^C " /tmp/cview-dom.txt | head -1 | cut -c3-)
echo "== B 다시 실행 ($CH)"; sed "s#__CH__#$CH#" $T/js/pic-b.js > /tmp/pic-b.js; run /tmp/pic-b.js
echo DONE
