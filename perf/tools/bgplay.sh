#!/bin/zsh
# bgplay.sh <app 폴더 이름> — 라이브 재생 → 홈으로 이동(뒤에서 소리만) → 15초 안정 → 30초 CPU (로그 끔)
T=/Volumes/data/project/Rust/CView/target/perf-tools; APP=$T/$1/CView.app
C1='["b628d1039a84ecc703804e17acee2eb3"]'
pkill -f "MacOS/cview-app"; sleep 3; pgrep -f "com.apple.WebKit" | sort > /tmp/cview-webkit-before
( env CVIEW_DEBUG_LOG=0 CVIEW_DEBUG_EVAL=/tmp/cview-eval.js nohup $APP/Contents/MacOS/cview-app > /tmp/cview-run.log 2>&1 & ); sleep 15
app=$(pgrep -f "MacOS/cview-app" | head -1); osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $app) to true"
sed "s/__SC__/c/; s/__CH__/$C1/" $T/js/scen.js > $T/js/x.js; $T/ev.sh $T/js/x.js "Z done" 90 >/dev/null; sleep 5
echo "(()=>{ if(location.search) return; window.dispatchEvent(new CustomEvent('cview:navigate',{detail:{route:'home'}})); fetch('http://127.0.0.1:47999/',{method:'POST',body:'Z done'});})()" > $T/js/x.js; $T/ev.sh $T/js/x.js "Z done" 20 >/dev/null; sleep 15
echo "(()=>{ if(location.search) return; const o=[]; for (const f of document.querySelectorAll('iframe')){ let v=null; try{v=f.contentDocument.getElementById('video');}catch{} if(v) o.push(f.id+' playing='+(!v.paused)+' h='+v.videoHeight+' vol='+v.volume+' muted='+v.muted); } fetch('http://127.0.0.1:47999/',{method:'POST',body:'S '+o.join(' | ')}); fetch('http://127.0.0.1:47999/',{method:'POST',body:'Z done'});})()" > $T/js/x.js; $T/ev.sh $T/js/x.js "Z done" 20 | grep "^S"
$T/pids.sh >/dev/null; python3 $T/measure.py "bg-$1" 30 /tmp/cview-pids | grep "^{" | cut -c1-330
