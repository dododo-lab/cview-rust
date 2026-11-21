#!/bin/zsh
# 실제 앱(WKWebView)의 화면별 사용률 측정 — 앱 안 사용률 측정과 같은 표본기(examples/usage_probe.rs)로 밖에서 잰다.
#
# 준비(한 번):
#   CARGO_TARGET_DIR=<target> cargo build --release -p cview-app --example usage_probe
#   CARGO_TARGET_DIR=<target> cargo tauri build --debug --bundles app \
#     --config '{"identifier":"app.cview.rust.perf","productName":"CView Perf","bundle":{"macOS":{"signingIdentity":"-"}}}'
#   (별도 식별자라 설치한 CView·로그인·설정을 건드리지 않는다. 비교할 판은 productName 만 바꿔 같은 식별자로 빌드한다.)
#
# 사용: tools/usage-bench/run.sh <앱 번들> <usage_probe> <결과.jsonl> [시나리오...]
#   시나리오: home · popular · watch · multi4 · multi-bg(멀티라이브 4패인을 뒤에 두고 홈, multi4 뒤에) — 기본은 모두 이 순서로.
#   채널은 공개 인기 라이브 목록(api.chzzk.naver.com)에서 성인 방송이 아닌 상위 4개를 고른다.
#   측정 프로필은 음소거·알림/업데이트 확인 끔으로 시작하고, 공유 데이터 폴더 대신 임시 폴더를 쓴다(CVIEW_APPLICATION_SUPPORT).
#   첫 실행이면 환영 안내가 뜬다 — 한 번 닫아 두면(Esc) 다음부터는 뜨지 않는다.
#   측정 중 앱 창이 앞으로 나온다(딥링크가 창을 앞으로 가져온다). 창이 가려지면 앱이 디코드를 줄여 값이 달라진다.
set -u
APP=$1; PROBE=$2; out=$3; shift 3
if (( $# )); then scenarios=("$@"); else scenarios=(home popular watch multi4 multi-bg); fi
work=${TMPDIR:-/tmp}/cview-usage-bench; mkdir -p $work/support
prof="$HOME/Library/Application Support/app.cview.rust.perf"
if [[ ! -e "$prof/CView_v2.store" ]]; then
  mkdir -p "$prof"
  python3 - "$prof/settings.json" <<'PY'
import json, sys, time
doc = {"schemaVersion": 1, "savedAt": time.time(), "settings": {
    "player": {"startMuted": True, "volumeLevel": 0.0},
    "general": {"notificationsEnabled": False, "autoCheckUpdates": False, "showInMenuBar": False, "restoreWindowOnLaunch": False, "launchAtLogin": False},
    "multiLive": {"multiAudioEnabled": False}}}
open(sys.argv[1], 'w').write(json.dumps(doc, indent=2, sort_keys=True))
PY
fi
chans=($(curl -s --max-time 15 -A "Mozilla/5.0 (Macintosh)" "https://api.chzzk.naver.com/service/v1/lives?size=20&sortType=POPULAR" \
  | python3 -c "import sys,json; print(' '.join([x['channel']['channelId'] for x in json.load(sys.stdin)['content']['data'] if not x.get('adult')][:4]))"))
(( ${#chans} == 4 )) || { echo "인기 라이브를 가져오지 못했습니다"; exit 1; }
: > $out
open -n --env CVIEW_APPLICATION_SUPPORT=$work/support "$APP"; sleep 12
pid=$(pgrep -f "$APP/Contents/MacOS/cview-app" | head -1)
[[ -n $pid ]] || { echo "앱이 뜨지 않았습니다"; exit 1; }
go() { for u in "$@"; do open -a "$APP" "$u"; sleep 3; done; }
measure() { # 이름 초
  $PROBE $pid $2 1000 $1 | grep '^SUMMARY' | sed 's/^SUMMARY //' | tee -a $out | python3 -c "
import sys, json
d = json.load(sys.stdin); g = d['groups']; c = lambda k: g.get(k, {}).get('cpu_avg', 0)
print(f\"{d['label']:<14} 합 {d['total']['cpu_avg']:5.1f}  웹 화면 {c('web'):4.1f}  GPU 처리 {c('gpu'):4.1f}  앱 {c('app'):4.1f}  메모리 {d['total']['mem_mb']:.0f}MB\")"
}
for s in $scenarios; do
  case $s in
    home) go "cview://home"; sleep 20; measure home 40 ;;
    popular) go "cview://popular?tab=live"; sleep 15; measure popular 30 ;;
    watch) go "cview://live?channelId=${chans[1]}"; sleep 20; measure watch 45 ;;
    multi4) go "cview://live?channelId=${chans[2]}&mode=multi" "cview://live?channelId=${chans[3]}&mode=multi" "cview://live?channelId=${chans[4]}&mode=multi" "cview://multilive"; sleep 25; measure multi4 45 ;;
    multi-bg) go "cview://multilive"; sleep 10; go "cview://home"; sleep 15; measure multi-bg 30 ;;
  esac
done
kill $pid
