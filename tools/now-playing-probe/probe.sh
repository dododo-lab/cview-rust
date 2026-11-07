#!/bin/zsh
# macOS 시스템 '지금 재생 중'(제어 센터·메뉴 막대 섬) 항목을 읽는다 — 앱이 실제로 무엇을 올렸는지 확인용.
# 출력: INFO(제목·아티스트·길이·재생 속도·그림 크기), PLAYING, COMMANDS(지원 명령 번호), CLIENT(올린 앱).
#   명령 번호: 0 재생, 1 일시정지, 2 재생/일시정지, 3 정지, 8·9 빨리 감기, 10·11 되감기,
#              17 앞으로 건너뛰기, 18 뒤로 건너뛰기(섬의 '10초' 버튼), 24 위치 이동.
# macOS 15.4 부터 MediaRemote 는 Apple 서명 프로세스만 읽을 수 있어 /usr/bin/perl 에 라이브러리를 실어 읽는다.
# 비공개 API 라 진단용으로만 쓴다(앱에는 넣지 않는다).
# 사용: tools/now-playing-probe/probe.sh
set -eu
here=${0:A:h}
out=${TMPDIR:-/tmp}/cview-now-playing-probe.dylib
if [[ ! -f $out || $here/probe.m -nt $out ]]; then
  clang -dynamiclib -fobjc-arc -framework Foundation -o "$out" "$here/probe.m"
fi
exec /usr/bin/perl -e 'require DynaLoader; DynaLoader::dl_load_file($ARGV[0]) or die DynaLoader::dl_error()' "$out"
