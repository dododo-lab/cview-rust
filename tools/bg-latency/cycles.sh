#!/bin/zsh
# 사용: cycles.sh <앱> <라벨> <채널> <숨김초...> — 전경 90초 뒤 숨김/복귀(복귀 40초)를 주어진 길이로 되풀이
APP=$1; LABEL=$2; CH=$3; shift 3
OUT=~/bglat/out/$LABEL; rm -rf $OUT; mkdir -p $OUT
LOGDIR=~/Library/Application\ Support/app.cview.rust/logs
pkill -f 'CView.app/Contents/MacOS' 2>/dev/null; sleep 3; rm -f $LOGDIR/debug*.jsonl
ev() { print "$(python3 -c 'import time;print(int(time.time()*1000))') $1" >> $OUT/events.txt; }
launchctl asuser 501 zsh -c "CVIEW_DEBUG_LOG=1 HTTPS_PROXY=http://192.168.64.1:18888 '$APP/Contents/MacOS/cview-app' > $OUT/app.log 2>&1 &"
sleep 15
launchctl asuser 501 zsh -c "'$APP/Contents/MacOS/cview-app' 'cview://live?channelId=$CH' > /dev/null 2>&1 &"
ev launch; sleep 5; PID=$(pgrep -f 'CView.app/Contents/MacOS/cview-app' | head -1)
sleep 90
for h in "$@"; do
  launchctl asuser 501 ~/bglat/vm/appvis hide $PID >/dev/null; ev hide; sleep $h
  launchctl asuser 501 ~/bglat/vm/appvis unhide $PID >/dev/null; ev unhide; sleep 40
done
ev done; cp $LOGDIR/debug*.jsonl $OUT/; pkill -f 'CView.app/Contents/MacOS'; print 완료
