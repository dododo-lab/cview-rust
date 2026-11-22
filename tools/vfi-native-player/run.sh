#!/bin/zsh
# vfi-player 시나리오 실행. 사용법: run.sh <새 결과 폴더> [절 ...]
#   절: sizes file bipbop mux-ll discont chzzk chzzk1080 long   (생략하면 long 을 뺀 전부, 약 36분)
# 창을 띄우지 않는다(window 출력은 사용자가 직접 실행). VT 세션은 샌드박스 밖에서 실행해야 시작된다.
# 치지직은 로그인 없이 공개 방송(성인·유료·암호화 제외)만 본다. 재생 주소의 토큰은 기록하지 않는다.
set -uo pipefail
HERE=${0:A:h}
OUT=${1:?결과 폴더를 지정하세요}
shift
SECTIONS=("$@"); (( ${#SECTIONS} )) || SECTIONS=(sizes file bipbop mux-ll discont chzzk chzzk1080)
BIN=$(zsh $HERE/build.sh) || exit 1
mkdir -p $OUT
FIX=$OUT/fixture-720p60.mp4
BIPBOP=https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_fmp4/master.m3u8
MUX_LL=https://stream.mux.com/v69RSHhFelSm4701snP22dYz2jICy4E4FUyk02rW4gxRM.m3u8
MUX_DISCONT=https://test-streams.mux.dev/dai-discontinuity-deltatre/manifest.m3u8

# 표준 오류(시스템 프레임워크의 기록)는 따로 남기고, 진행 줄만 화면에 보인다.
# VFI_PROGRESS_LOG 를 주면 절마다 시작·끝을 그 파일에 한 줄씩 적는다(진행 콘솔용).
note() { [[ -n ${VFI_PROGRESS_LOG:-} ]] && echo "$(date +%H:%M:%S) $*" >> $VFI_PROGRESS_LOG; return 0; }
run() {
  local name=$1; shift
  echo "=== $name: $* ==="
  note "$name 시작"
  $BIN "$@" --json $OUT/$name.json --csv $OUT/$name.csv 2>$OUT/$name.stderr | tee $OUT/$name.txt | grep --line-buffered -v "^  · "
  note "$name 끝(종료 코드 ${pipestatus[1]}): $(grep -m1 '^요약' $OUT/$name.txt 2>/dev/null | cut -c1-160)"
}

for s in $SECTIONS; do
  case $s in
    sizes) $BIN --probe-sizes 2>/dev/null | tee $OUT/sizes.txt ;;
    file)
      if [[ ! -f $FIX ]]; then
        $BIN --generate $FIX --gen-size 1280x720 --gen-fps 60 --seconds 120 2>$OUT/generate.stderr || { echo "시험 영상을 만들지 못했습니다(generate.stderr)"; exit 1; }
      fi
      run file-base   --file $FIX --seconds 60 --interp off --guards none --seek-every 20
      run file-guards --file $FIX --seconds 60 --interp off --seek-every 20
      run file-interp --file $FIX --seconds 60 --seek-every 20
      run file-render --file $FIX --seconds 60 --seek-every 20 --sink renderer
      run file-nobrk  --file $FIX --seconds 60 --seek-every 20 --breaker off
      run file-inject --file $FIX --seconds 60 --seek-every 20 --inject-delay 10:150 --sink renderer ;;   # VT 완료 10%를 0~150ms 늦춰 순서·세대 처리 검증
    bipbop)   # 720p60 → 432p30 → 720p60 화질·프레임률 전환 + 탐색
      run bipbop --url $BIPBOP --seconds 120 --switch "40:432,80:720" --seek-every 30 --forward-buffer 6 --sink renderer ;;
    mux-ll)   # 공개 LL-HLS 라이브(오디오 분리)
      run mux-ll --url $MUX_LL --seconds 120 --max-height 720 ;;
    discont)  # EXT-X-DISCONTINUITY 4개
      run discont --url $MUX_DISCONT --seconds 150 --max-height 720 ;;
    chzzk)    # 치지직 공개 방송 720p60, 10분
      run chzzk --chzzk popular --seconds 600 --max-height 720 --sink renderer ;;
    chzzk1080) # 1080p60: 이 Mac 에서는 VT 가 따라가지 못해 브레이커가 끄는지 본다
      run chzzk1080 --chzzk popular --seconds 180 --max-height 1080 ;;
    long)     # 1시간 연속(C8)
      run long --chzzk popular --seconds 3600 --max-height 720 --sink renderer ;;
    *) echo "알 수 없는 절: $s" ;;
  esac
done
echo "결과: $OUT"
