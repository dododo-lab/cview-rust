#!/bin/zsh
# launch.sh <app-bundle> — 앱 실행(디버그 평가 훅 env), 앱+새 WebKit 도우미 PID 를 /tmp/cview-pids 에
APP=$1
pkill -f "MacOS/cview-app"; sleep 3
before=$(pgrep -f "com.apple.WebKit" | sort); echo $before > /tmp/cview-webkit-before
python3 -c "import time;print(int(time.time()*1000))" > /tmp/cview-launch-ms
(CVIEW_DEBUG_EVAL=/tmp/cview-eval.js RUST_LOG=${RUST_LOG:-info} nohup $APP/Contents/MacOS/cview-app > /tmp/cview-run.log 2>&1 &)
sleep 15
app=$(pgrep -f "$APP/Contents/MacOS/cview-app" | head -1)
after=$(pgrep -f "com.apple.WebKit" | sort)
new=$(comm -13 <(echo $before) <(echo $after) | tr '\n' ' ')
echo "$app $new" > /tmp/cview-pids; cat /tmp/cview-pids
for p in ${=new}; do ps -o pid=,comm= -p $p; done
