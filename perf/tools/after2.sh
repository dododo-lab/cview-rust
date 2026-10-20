#!/bin/zsh
T=/Volumes/data/project/Rust/CView/target/perf-tools; APP=$T/dbg3/CView.app; OUT=$T/after-2.txt; : > $OUT
LOGF="$HOME/Library/Application Support/app.cview.rust/logs/debug.jsonl"
C1='["b628d1039a84ecc703804e17acee2eb3"]'; C4='["65c3035bdc598c81f15a8fe0e958b3ce","e14259667937be6e561d28ab156718be","6bd0bb97d31365e7834d8113bb01d889","55e243bd868e55adf3524c85f8db51b5"]'
log(){ echo "$*" | tee -a $OUT; }
front(){ app=$(pgrep -f "MacOS/cview-app" | head -1); osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $app) to true"; swift $T/warp.swift; }
rss(){ $T/pids.sh >/dev/null; ps -o rss= -p $(tr ' ' ',' < /tmp/cview-pids | sed 's/,$//') | awk '{s+=$1} END {print int(s/1024)}'; }
launch(){ pkill -f "MacOS/cview-app"; sleep 3; pgrep -f "com.apple.WebKit" | sort > /tmp/cview-webkit-before; python3 -c "import time;print(int(time.time()*1000))" > /tmp/cview-launch-ms; ( env $1 CVIEW_DEBUG_EVAL=/tmp/cview-eval.js nohup $APP/Contents/MacOS/cview-app > /tmp/cview-run.log 2>&1 & ); sleep 15; front; }
CH=b628d1039a84ecc703804e17acee2eb3
# A) 시작 ×3 (로그 켬)
for i in 2 3; do launch CVIEW_DEBUG_LOG=1; sed "s/__LAUNCH__/$(cat /tmp/cview-launch-ms)/; s/'__CH__'/'$CH'/" $T/js/startup.js > $T/js/x.js; $T/ev.sh $T/js/x.js "Z done" 60 | grep "^S" | while read -r l; do log "run$i $l"; done; done
# B) 로그 끔 — CPU·스크롤·채팅·메모리
launch CVIEW_DEBUG_LOG=0
DUR=30 $T/run.sh after2 a | grep "^{" >> $OUT
for r in following popular categories; do sed "s/__R__/$r/" $T/js/scroll.js > $T/js/x.js; $T/ev.sh $T/js/x.js "Z done" 40 | grep "^S" | while read -r l; do log "$l"; done; done
DUR=30 $T/run.sh after2 c $C1 | grep "^{" >> $OUT
sed "s/__R__/chat/" $T/js/scroll.js > $T/js/x.js; $T/ev.sh $T/js/x.js "Z done" 40 | grep "^S" | while read -r l; do log "$l"; done
sed "s/__RATES__/25,100,400/" $T/js/chatburst.js > $T/js/x.js; $T/ev.sh $T/js/x.js "Z done" 60 | grep "^S" | while read -r l; do log "$l"; done
line="S mem live_rss_mb"; for m in 0 1 2; do line="$line $(rss)"; [ $m -lt 2 ] && sleep 60; done; log "$line"
echo "(async()=>{ if(location.search) return; const I=window.__TAURI__.core.invoke; const v=document.getElementById('viewer'); if(!v.hidden) document.getElementById('viewer-close').click(); await I('multilive_stop_all'); window.dispatchEvent(new CustomEvent('cview:navigate',{detail:{route:'home'}})); fetch('http://127.0.0.1:47999/',{method:'POST',body:'Z done'});})()" > $T/js/x.js; $T/ev.sh $T/js/x.js "Z done" 20 >/dev/null; sleep 30; log "S mem after_close_30s_mb $(rss)"
DUR=30 $T/run.sh after2 e $C4 | grep "^{" >> $OUT
# C) 로그 켬 — 요청 수(대기 2분 + 시청 2분)
launch CVIEW_DEBUG_LOG=1
echo "(()=>{ if(location.search) return; window.dispatchEvent(new CustomEvent('cview:navigate',{detail:{route:'home'}})); fetch('http://127.0.0.1:47999/',{method:'POST',body:'Z done'});})()" > $T/js/x.js; $T/ev.sh $T/js/x.js "Z done" 20 >/dev/null
s1=$(date -u +%Y-%m-%dT%H:%M:%S); sleep 60; e1=$(date -u +%Y-%m-%dT%H:%M:%S)
sed "s/__SC__/c/; s/__CH__/$C1/" $T/js/scen.js > $T/js/x.js; $T/ev.sh $T/js/x.js "Z done" 90 >/dev/null
s2=$(date -u +%Y-%m-%dT%H:%M:%S); sleep 60; e2=$(date -u +%Y-%m-%dT%H:%M:%S)
sleep 2; log "WIN idle $s1 $e1"; log "WIN watch $s2 $e2"
log "DONE"
