#!/bin/zsh
# thumbcmp2.sh — 렌더링과 상관없는 측정(숨긴 창): 화면 밖 라이브 51~62번 썸네일+아바타를 new Image().decode() 로 동시에
T=/Volumes/data/project/Rust/CView/target/perf-tools
reset(){ rm -rf ~/Library/Application\ Support/app.cview.rust.test ~/Library/Caches/app.cview.rust.test ~/Library/WebKit/app.cview.rust.test ~/Library/HTTPStorages/app.cview.rust.test; }
run(){ APP="$T/thumb-$1/CView Test.app"; : > /tmp/cview-dom.txt; cp $T/js/thumbs2.js /tmp/cview-eval.js
  open -g -j -n --env CVIEW_DEBUG_EVAL=/tmp/cview-eval.js "$APP"
  for i in $(seq 1 240); do grep -q "^Z done" /tmp/cview-dom.txt && break; sleep 0.5; done
  grep "^S" /tmp/cview-dom.txt | sed "s/^S /$1-$2 /"; osascript -e "tell application \"$APP\" to quit" >/dev/null 2>&1; sleep 3; }
for b in base new; do reset; run $b 처음; run $b 다시실행; done
echo DONE
