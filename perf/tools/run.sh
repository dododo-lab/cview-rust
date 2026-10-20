#!/bin/zsh
# run.sh <tag> <scenario> [channel ids json] — 시나리오 준비 후 60초 측정
T=/Volumes/data/project/Rust/CView/target/perf-tools; sc=$2; ch=${3:-[]}
sed "s/__SC__/$sc/; s/__CH__/$ch/" $T/js/scen.js > $T/js/scx.js
app=$(pgrep -f "MacOS/cview-app" | head -1); osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $app) to true"
$T/ev.sh $T/js/scx.js "Z done" 90 | grep "^S\|err"
$T/ev.sh $T/js/vis.js "Z done" 20 | grep "^S"
if [[ $sc == f ]]; then app=$(pgrep -f "MacOS/cview-app" | head -1); osascript -e "tell application \"System Events\" to set visible of (first process whose unix id is $app) to false"; sleep 20; fi
$T/pids.sh >/dev/null
python3 $T/measure.py "$1-$sc" 60 /tmp/cview-pids | tee -a $T/$1.jsonl
if [[ $sc == f ]]; then app=$(pgrep -f "MacOS/cview-app" | head -1); osascript -e "tell application \"System Events\" to set visible of (first process whose unix id is $app) to true"; fi
