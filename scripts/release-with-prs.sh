#!/bin/zsh
# PR 병합 → 작업 보관 → main 갱신 → 배포 릴리스 → 작업 복원을 한 번에 (release-macos.sh --dmg 래퍼).
#
# 사용: scripts/release-with-prs.sh [PR번호 ...] [-- release-macos.sh 추가 옵션]
#   예: scripts/release-with-prs.sh 33
#       scripts/release-with-prs.sh 33 35 -- --minor
#       scripts/release-with-prs.sh              # 병합할 PR 없이 main 최신으로 릴리스
#   CVIEW_NOTARIZE_PROFILE · CVIEW_RELEASE_NOTES_FILE 은 그대로 release-macos.sh 에 넘어간다.
#
# 순서:
#   1) main 브랜치인지 확인. .idea/ 는 .git/info/exclude 에 넣어(로컬 전용) IDE 가 다시 만들어도 깨끗함 검사를 막지 않게 한다.
#   2) 각 PR: 열려 있는 PR 만 draft 해제 → squash 병합(이미 병합된 PR 은 건너뜀).
#   3) 커밋 안 된 작업(추적 안 된 파일 포함)을 stash 로 보관 → git pull --ff-only origin main.
#   4) scripts/release-macos.sh --dmg 실행.
#   5) 성공·실패와 관계없이 보관한 작업을 되돌린다. 릴리스가 작업 트리를 더럽힌 채 끝났으면 되돌리지 않고 안내만 한다.
# 스크립트 전체를 함수로 감싸 둔다 — 실행 중 stash·pull 로 이 파일이 바뀌어도 이미 읽은 내용으로 끝까지 돈다.
set -euo pipefail

main() {
  local -a prs release_args
  while (( $# )); do
    case "$1" in
      --) shift; release_args=("$@"); break ;;
      <->) prs+=("$1"); shift ;;
      *) echo "✗ PR 번호(숫자)만 받습니다. release-macos.sh 옵션은 -- 뒤에 넣으세요: $1"; exit 2 ;;
    esac
  done
  for a in $release_args; do
    [[ "$a" == --no-notarize ]] && { echo "✗ --no-notarize는 이 스크립트로 넘기지 않습니다. 필요하면 release-macos.sh를 직접 실행하세요."; exit 2; }
  done

  [[ "$(git rev-parse --abbrev-ref HEAD)" == main ]] || { echo "✗ main 브랜치에서 실행하세요."; exit 1; }
  local exclude=.git/info/exclude
  grep -qxF '.idea/' "$exclude" 2>/dev/null || echo '.idea/' >> "$exclude"

  local state draft
  for pr in $prs; do
    read -r state draft < <(gh pr view "$pr" --json state,isDraft --jq '"\(.state) \(.isDraft)"')
    case "$state" in
      MERGED) echo "── PR #$pr 이미 병합됨, 건너뜀" ;;
      OPEN)
        echo "── PR #$pr 병합"
        [[ "$draft" == true ]] && gh pr ready "$pr"
        gh pr merge "$pr" --squash ;;
      *) echo "✗ PR #$pr 상태가 $state 입니다 — 병합할 수 없습니다."; exit 1 ;;
    esac
  done

  local stashed=0
  if [[ -n "$(git status --porcelain)" ]]; then
    echo "── 커밋 안 된 작업 보관"
    git status --short
    git stash push -u -m "release-with-prs $(date +%Y%m%d-%H%M%S)"
    stashed=1
  fi

  restore_wip() {
    (( stashed )) || return 0
    stashed=0
    if [[ -z "$(git status --porcelain)" ]]; then
      echo "── 보관한 작업 되돌리기"
      git stash pop || { echo "⚠︎ stash pop 충돌 — 'git stash list'의 맨 위 항목을 직접 확인하세요."; return 1; }
    else
      echo "⚠︎ 작업 트리가 깨끗하지 않아 보관한 작업을 되돌리지 않았습니다. 정리 후 'git stash pop'을 실행하세요."
      git status --short
    fi
  }
  trap 'restore_wip; exit 130' INT TERM

  # if·|| 안의 명령은 set -e 로 끊기지 않는다 — 실패해도 아래에서 작업을 되돌린다.
  local rc=0
  echo "── main 갱신"
  if git pull --ff-only origin main; then
    echo "── 배포 릴리스"
    scripts/release-macos.sh --dmg $release_args || rc=$?
  else
    rc=$?
  fi
  trap - INT TERM
  restore_wip || { (( rc )) || rc=1; }
  return $rc
}

# 함수 안의 $0 은 함수 이름이므로 저장소 루트로는 여기서 옮긴다.
cd "${0:A:h}/.."
main "$@"
