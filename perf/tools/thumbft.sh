#!/bin/zsh
# thumbft.sh — 썸네일 캐시 종합 기능 테스트(숨긴 실행, 시험 번들 ID)
T=/Volumes/data/project/Rust/CView/target/perf-tools; APP="$T/thumb-new/CView Test.app"
D="$HOME/Library/Application Support/app.cview.rust.test"; IDX="$D/image-cache/index.sqlite"
rm -rf "$D" ~/Library/Caches/app.cview.rust.test ~/Library/WebKit/app.cview.rust.test ~/Library/HTTPStorages/app.cview.rust.test
run(){ js=$1; shift; : > /tmp/cview-dom.txt; cp $js /tmp/cview-eval.js
  open -g -j -n --env CVIEW_DEBUG_EVAL=/tmp/cview-eval.js "$@" "$APP"
  for i in $(seq 1 300); do grep -q "^Z done" /tmp/cview-dom.txt && break; sleep 0.5; done
  grep "^R\|^err" /tmp/cview-dom.txt; osascript -e "tell application \"$APP\" to quit" >/dev/null 2>&1; sleep 3; }
echo "== A 첫 실행"; run $T/js/ft-a.js
grep "^U " /tmp/cview-dom.txt > /tmp/thumb-urls.txt
URLS=$(python3 -c "
import json
print(json.dumps([l.split(' ',2)[1:] for l in open('/tmp/thumb-urls.txt').read().split('\n') if l.strip()]))")
keys(){ python3 - "$1" <<'PY'
import sys,hashlib,urllib.parse
for l in open('/tmp/thumb-urls.txt'):
    p=l.split(' ',2)
    if len(p)<3 or p[1]!=sys.argv[1]: continue
    u=urllib.parse.unquote(p[2].strip().split('localhost/',1)[1]).split('/',1)[1]
    print("'"+hashlib.sha256(u.encode()).hexdigest()+"'")
PY
}
LK=$(keys live | paste -sd, -); AK=$(keys avatar | paste -sd, -)
q(){ sqlite3 "$IDX" "SELECT COUNT(*)||' '||MIN(fetched_at)||' '||MAX(fetched_at) FROM images WHERE key IN ($1)"; }
L1=$(q "$LK"); A1=$(q "$AK"); echo "색인 라이브(개 최소 최대)=$L1 아바타=$A1"
echo "== 50초 기다림(라이브 TTL 45초)"; sleep 50
echo "== B 다시 실행"; sed "s/__MODE__/restart/; s#__URLS__#$URLS#" $T/js/ft-b.js > /tmp/ft-b.js; run /tmp/ft-b.js
L2=$(q "$LK"); A2=$(q "$AK"); echo "색인 라이브=$L2 아바타=$A2"
python3 -c "
l1=list(map(int,'$L1'.split()[1:])); l2=list(map(int,'$L2'.split()[1:])); a1='$A1'; a2='$A2'
print('R', 'PASS' if l2[0]>l1[1] else 'FAIL', '라이브 썸네일 뒤에서 새로 받음 — 받은 시각 최소', l1[0], '→', l2[0])
print('R', 'PASS' if a1==a2 else 'FAIL', '아바타는 신선해 다시 받지 않음 —', a1, '→', a2)"
echo "== C 오프라인(프록시를 닫힌 포트로)"; sed "s/__MODE__/offline/; s#__URLS__#$URLS#" $T/js/ft-b.js > /tmp/ft-b.js; run /tmp/ft-b.js --env HTTPS_PROXY=http://127.0.0.1:9 --env HTTP_PROXY=http://127.0.0.1:9 --env ALL_PROXY=http://127.0.0.1:9
echo "== D 비우기"; sed "s/__MODE__/clear/; s#__URLS__#$URLS#" $T/js/ft-b.js > /tmp/ft-b.js; run /tmp/ft-b.js
echo "blobs: $(/usr/bin/du -sk "$D/image-cache/blobs" | cut -f1)KB, 색인 $(sqlite3 "$IDX" 'SELECT COUNT(*) FROM images')개"
echo DONE
