#!/bin/zsh
# macOS 릴리스 빌드·서명·공증 (SYS-26).
#
# 서명·공증 값은 이 스크립트에 적지 않는다 — 실행하는 사람이 환경 변수로 넘긴다(Tauri 번들러가 읽는다).
#   APPLE_SIGNING_IDENTITY  "Developer ID Application: 이름 (TEAMID)"  (없으면 서명 없이 빌드)
#   공증(둘 중 하나):
#     APPLE_API_ISSUER · APPLE_API_KEY · APPLE_API_KEY_PATH   (App Store Connect API 키, 권장)
#     APPLE_ID · APPLE_PASSWORD(앱 암호) · APPLE_TEAM_ID
#
# 사용: scripts/release-macos.sh [--dmg]
set -euo pipefail
cd "$(dirname "$0")/.."

bundles="app"
[[ "${1:-}" == "--dmg" ]] && bundles="app,dmg"

if [[ -z "${APPLE_SIGNING_IDENTITY:-}" ]]; then
  echo "⚠︎ APPLE_SIGNING_IDENTITY 가 없어 서명하지 않고 빌드합니다 (로컬 실행용)."
fi
if [[ -n "${APPLE_SIGNING_IDENTITY:-}" && -z "${APPLE_API_KEY:-}${APPLE_ID:-}" ]]; then
  echo "⚠︎ 공증 자격 증명이 없어 서명만 하고 공증은 건너뜁니다."
fi

(cd crates/cview-app && cargo tauri build --bundles "$bundles")

app="target/release/bundle/macos/CView.app"
[[ -d "$app" ]] || { echo "번들을 찾지 못했습니다: $app"; exit 1; }
echo "── 산출물"
ls -la "$app/Contents/MacOS"
lipo -info "$app/Contents/MacOS/"* || true
if [[ -n "${APPLE_SIGNING_IDENTITY:-}" ]]; then
  echo "── 서명 확인"
  codesign --verify --deep --strict --verbose=2 "$app"
  codesign -dv --verbose=2 "$app" 2>&1 | grep -E "Authority|TeamIdentifier|Runtime" || true
  echo "── Gatekeeper 평가"
  spctl -a -vvv -t exec "$app" || echo "(공증 전이면 거부될 수 있습니다)"
  xcrun stapler validate "$app" || true
fi
echo "완료: $app"
