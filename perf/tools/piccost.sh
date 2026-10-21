#!/bin/zsh
T=/Volumes/data/project/Rust/CView/target/perf-tools; APP="$T/pic/CView Test.app"
pgrep -f "com.apple.WebKit" | sort > /tmp/cview-webkit-before
: > /tmp/cview-dom.txt; cp $T/js/pic-cost.js /tmp/cview-eval.js; open -g -j -n --env CVIEW_DEBUG_EVAL=/tmp/cview-eval.js "$APP"
for i in $(seq 1 200); do grep -q "^Z done" /tmp/cview-dom.txt && break; sleep 0.5; done
app=$(pgrep -f "CView Test.app/Contents/MacOS/cview-app" | head -1); new=$(comm -13 /tmp/cview-webkit-before <(pgrep -f "com.apple.WebKit" | sort) | tr '\n' ' '); echo "$app $new" > /tmp/pic-pids; echo "pids: $(cat /tmp/pic-pids)"
sleep 5
for mode in off on off on; do
  [[ $mode == on ]] && js='window.__sharpAll(true)' || js='window.__sharpAll(false)'
  : > /tmp/cview-dom.txt; echo "(()=>{ if(location.search) return; const r=$js; fetch('http://127.0.0.1:47999/',{method:'POST',body:'S filters '+r}); fetch('http://127.0.0.1:47999/',{method:'POST',body:'Z done'}); })()" > /tmp/cview-eval.js
  for i in $(seq 1 40); do grep -q "^Z done" /tmp/cview-dom.txt && break; sleep 0.5; done; grep "^S" /tmp/cview-dom.txt | cut -c1-120
  sleep 3; python3 $T/measure.py "sharpen-$mode" 30 /tmp/pic-pids | grep "^{" | python3 -c "import json,sys; d=json.loads(sys.stdin.read()); print(d['label'], 'cpu', d['cpu_pct'], d['cpu_by'])"
done
osascript -e "tell application \"$APP\" to quit" >/dev/null 2>&1; echo DONE
