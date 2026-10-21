#!/bin/zsh
T=/Volumes/data/project/Rust/CView/target/perf-tools
rm -rf ~/Library/Application\ Support/app.cview.rust.test ~/Library/Caches/app.cview.rust.test ~/Library/WebKit/app.cview.rust.test ~/Library/HTTPStorages/app.cview.rust.test
for b in base new; do
  rm -rf ~/Library/Application\ Support/app.cview.rust.test ~/Library/Caches/app.cview.rust.test ~/Library/WebKit/app.cview.rust.test ~/Library/HTTPStorages/app.cview.rust.test
  APP="$T/thumb-$b/CView Test.app"; : > /tmp/cview-dom.txt; cp $T/js/thumbs2.js /tmp/cview-eval.js
  open -g -j -n --env CVIEW_DEBUG_EVAL=/tmp/cview-eval.js "$APP"
  for i in $(seq 1 240); do grep -q "^Z done" /tmp/cview-dom.txt && break; sleep 0.5; done
  grep "^S" /tmp/cview-dom.txt | sed "s/^S /$b-처음(2) /"; osascript -e "tell application \"$APP\" to quit" >/dev/null 2>&1; sleep 3
done; echo DONE
