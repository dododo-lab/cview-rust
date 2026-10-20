#!/bin/zsh
# suite2.sh <tag> <app-bundle> — 확장 측정(perf/baseline-2.md). 결과는 $T/<tag>-2.txt
T=/Volumes/data/project/Rust/CView/target/perf-tools; tag=$1; APP=$2; OUT=$T/$tag-2.txt; : > $OUT
C1='["b628d1039a84ecc703804e17acee2eb3"]'
log(){ echo "$*" | tee -a $OUT; }
front(){ app=$(pgrep -f "MacOS/cview-app" | head -1); osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $app) to true"; swift $T/warp.swift; }
rss(){ $T/pids.sh >/dev/null; ps -o rss= -p $(tr ' ' ',' < /tmp/cview-pids | sed 's/,$//') | awk '{s+=$1} END {print int(s/1024)}'; }
# 1) 시작 + 첫 라이브 프레임 (3회)
for i in 1 2 3; do
  RUST_LOG=info $T/launch.sh $APP >/dev/null; front
  CH=$(python3 -c "import json;print(json.loads('$C1')[0])")
  sed "s/__LAUNCH__/$(cat /tmp/cview-launch-ms)/; s/'__CH__'/'$CH'/" $T/js/startup.js > $T/js/x.js
  $T/ev.sh $T/js/x.js "Z done" 60 | grep "^S" | while read -r l; do log "run$i $l"; done
done
# 2) 시나리오 a~f (기존 도구, 같은 채널)
$T/suite.sh $tag $APP >/dev/null 2>&1; cat $T/$tag.jsonl >> $OUT
# 3) 스크롤 프레임 시간 + 채팅 처리량 + 네트워크(시청 중)
RUST_LOG=info,cview_networking=debug $T/launch.sh $APP >/dev/null; front
for r in following popular categories; do sed "s/__R__/$r/" $T/js/scroll.js > $T/js/x.js; $T/ev.sh $T/js/x.js "Z done" 40 | grep "^S" | while read -r l; do log "$l"; done; done
# 대기 중 요청 수(홈 120초)
echo "(()=>{ if(location.search) return; window.dispatchEvent(new CustomEvent('cview:navigate',{detail:{route:'home'}})); fetch('http://127.0.0.1:47999/',{method:'POST',body:'Z done'});})()" > $T/js/x.js; $T/ev.sh $T/js/x.js "Z done" 20 >/dev/null
a=$(grep -c "Request:" /tmp/cview-run.log); sleep 120; b=$(grep -c "Request:" /tmp/cview-run.log); log "S net idle_requests_per_min=$(( (b-a)/2 ))"
grep "Request:" /tmp/cview-run.log | tail -400 | sed -E 's/.*Request: [A-Z]+ ([^ ]+).*/\1/' | sort | uniq -c | sort -rn | head -8 | while read -r l; do log "  idle-path $l"; done
sed "s/__SC__/c/; s/__CH__/$C1/" $T/js/scen.js > $T/js/x.js; $T/ev.sh $T/js/x.js "Z done" 90 >/dev/null
a=$(grep -c "Request:" /tmp/cview-run.log); sleep 120; b=$(grep -c "Request:" /tmp/cview-run.log); log "S net watching_requests_per_min=$(( (b-a)/2 ))"
grep "Request:" /tmp/cview-run.log | tail -300 | sed -E 's/.*Request: [A-Z]+ ([^ ]+).*/\1/' | sort | uniq -c | sort -rn | head -8 | while read -r l; do log "  watch-path $l"; done
sed "s/__R__/chat/" $T/js/scroll.js > $T/js/x.js; $T/ev.sh $T/js/x.js "Z done" 40 | grep "^S" | while read -r l; do log "$l"; done
sed "s/__RATES__/25,50,100,200,400/" $T/js/chatburst.js > $T/js/x.js; $T/ev.sh $T/js/x.js "Z done" 90 | grep "^S" | while read -r l; do log "$l"; done
# 4) 메모리 — 10분 라이브, 닫은 뒤 60초
RUST_LOG=info $T/launch.sh $APP >/dev/null; front
sed "s/__SC__/c/; s/__CH__/$C1/" $T/js/scen.js > $T/js/x.js; $T/ev.sh $T/js/x.js "Z done" 90 >/dev/null
line="S mem live_rss_mb"; for m in 0 1 2 3 4 5 6 7 8 9 10; do line="$line $(rss)"; [ $m -lt 10 ] && sleep 60; done; log "$line"
echo "(async()=>{ if(location.search) return; const I=window.__TAURI__.core.invoke; const v=document.getElementById('viewer'); if(!v.hidden) document.getElementById('viewer-close').click(); await I('multilive_stop_all'); window.dispatchEvent(new CustomEvent('cview:navigate',{detail:{route:'home'}})); fetch('http://127.0.0.1:47999/',{method:'POST',body:'Z done'});})()" > $T/js/x.js; $T/ev.sh $T/js/x.js "Z done" 20 >/dev/null
sleep 60; log "S mem after_close_60s_mb $(rss)"
# 5) 멀티채팅 3채널 (g) — 새로 띄워 복원 경로로
RUST_LOG=info $T/launch.sh $APP >/dev/null; front
CH3='["65c3035bdc598c81f15a8fe0e958b3ce","e14259667937be6e561d28ab156718be","6bd0bb97d31365e7834d8113bb01d889"]'
sed "s/__CH3__/$CH3/" $T/js/gscen.js > $T/js/x.js; $T/ev.sh $T/js/x.js "Z done" 60 | grep "^S" | while read -r l; do log "$l"; done
$T/pids.sh >/dev/null; python3 $T/measure.py "$tag-g" 60 /tmp/cview-pids | tee -a $OUT >/dev/null
log "DONE"
