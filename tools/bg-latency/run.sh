#!/bin/zsh
# 사용: run.sh <앱 경로> <라벨> <채널> [전경초=180] [숨김초=720] [복귀초=120]
# 앱(디버그 로그 켬)과 웹 기준(Playwright WebKit)을 같은 채널로 함께 틀고, 전경 → 숨김 → 복귀 순서로 잰다.
APP=$1; LABEL=$2; CH=$3; FG=${4:-180}; BG=${5:-720}; RET=${6:-120}
OUT=~/bglat/out/$LABEL; rm -rf $OUT; mkdir -p $OUT
LOGDIR=~/Library/Application\ Support/app.cview.rust/logs
pkill -f 'CView.app/Contents/MacOS' 2>/dev/null; sleep 3
rm -f $LOGDIR/debug*.jsonl
ev() { print "$(python3 -c 'import time;print(int(time.time()*1000))') $1" >> $OUT/events.txt; }
launchctl asuser 501 zsh -c "CVIEW_DEBUG_LOG=1 HTTPS_PROXY=http://192.168.64.1:18888 HTTP_PROXY=http://192.168.64.1:18888 '$APP/Contents/MacOS/cview-app' > $OUT/app.log 2>&1 &"
sleep 15
# 딥링크는 두 번째 실행 인자로 넘긴다(단일 인스턴스가 받아 시청 화면을 연다).
launchctl asuser 501 zsh -c "'$APP/Contents/MacOS/cview-app' 'cview://live?channelId=$CH' > $OUT/app2.log 2>&1 &"
ev launch
(cd ~/bglat/vm && node web-ref.mjs $CH $((FG+BG+RET+30)) $OUT/web.jsonl > $OUT/web.log 2>&1 &)
# CPU 표본(5초)
( end=$(( $(date +%s) + FG + BG + RET + 20 )); while (( $(date +%s) < end )); do
    ts=$(python3 -c 'import time;print(int(time.time()*1000))')
    ps -A -o pid=,%cpu=,rss=,comm= | grep -E 'CView.app/Contents/MacOS/cview-app|com.apple.WebKit' | awk -v ts=$ts '{print ts, $0}' >> $OUT/cpu.txt
    sleep 5; done ) &
sleep 20
PID=$(pgrep -f 'CView.app/Contents/MacOS/cview-app' | head -1)
print "pid=$PID" >> $OUT/events.txt
sleep $FG
launchctl asuser 501 ~/bglat/vm/appvis hide $PID >/dev/null; ev hide; ~/bglat/vm/appvis state $PID >> $OUT/events.txt
sleep $BG
launchctl asuser 501 ~/bglat/vm/appvis unhide $PID >/dev/null; ev unhide; ~/bglat/vm/appvis state $PID >> $OUT/events.txt
sleep $RET
ev done
cp $LOGDIR/debug*.jsonl $OUT/ 2>/dev/null
pkill -f 'CView.app/Contents/MacOS' 2>/dev/null
print "완료 $OUT"
