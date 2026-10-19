#!/bin/zsh
# macOS 릴리스 빌드·서명·공증 (SYS-26).
#
# 서명: tauri.conf.json 의 bundle.macOS.signingIdentity(Developer ID)로 **모든 빌드(디버그·릴리스)** 를 서명한다.
#   코드 식별(identifier + 팀 ID)이 빌드마다 같아야 키체인의 "항상 허용"이 유지된다 — 애드혹 서명은 빌드마다
#   식별이 바뀌어 실행할 때마다 키체인 암호를 다시 묻는다. 다른 인증서는 APPLE_SIGNING_IDENTITY 로 덮어쓴다.
# 공증 값은 이 스크립트에 적지 않는다 — 실행하는 사람이 환경 변수로 넘긴다(Tauri 번들러가 읽는다).
#   공증(둘 중 하나):
#     APPLE_API_ISSUER · APPLE_API_KEY · APPLE_API_KEY_PATH   (App Store Connect API 키, 권장)
#     APPLE_ID · APPLE_PASSWORD(앱 암호) · APPLE_TEAM_ID
#
# 사용: scripts/release-macos.sh [--dmg]
set -euo pipefail
cd "$(dirname "$0")/.."

bundles="app"
[[ "${1:-}" == "--dmg" ]] && bundles="app,dmg"

identity="${APPLE_SIGNING_IDENTITY:-$(python3 -c 'import json;print(json.load(open("crates/cview-app/tauri.conf.json"))["bundle"]["macOS"].get("signingIdentity") or "")')}"
if [[ -z "$identity" ]] || ! security find-identity -v -p codesigning | grep -qF "$identity"; then
  echo "✗ 서명 인증서를 찾지 못했습니다: '$identity' — 키체인에 Developer ID 인증서가 있어야 합니다."; exit 1
fi
echo "서명 인증서: $identity"
if [[ -z "${APPLE_API_KEY:-}${APPLE_ID:-}" ]]; then
  echo "⚠︎ 공증 자격 증명이 없어 서명만 하고 공증은 건너뜁니다."
fi

(cd crates/cview-app && cargo tauri build --bundles "$bundles")

app="target/release/bundle/macos/CView.app"
[[ -d "$app" ]] || { echo "번들을 찾지 못했습니다: $app"; exit 1; }
echo "── 산출물"
ls -la "$app/Contents/MacOS"
lipo -info "$app/Contents/MacOS/"* || true
if true; then
  echo "── 서명 확인"
  codesign --verify --deep --strict --verbose=2 "$app"
  codesign -dv --verbose=2 "$app" 2>&1 | grep -E "Authority|TeamIdentifier|Runtime" || true
  echo "── Gatekeeper 평가"
  spctl -a -vvv -t exec "$app" || echo "(공증 전이면 거부될 수 있습니다)"
  xcrun stapler validate "$app" || true
fi
echo "완료: $app"
