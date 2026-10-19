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
# 사용: scripts/release-macos.sh [--dmg] [--local] [--target <Rust target>]
#   target: aarch64-apple-darwin / x86_64-apple-darwin / universal-apple-darwin
#   공증: CVIEW_NOTARIZE_PROFILE=cview-notarize scripts/release-macos.sh --dmg  (notarytool 프로파일이 있을 때)
set -euo pipefail
cd "$(dirname "$0")/.."

bundles="app"
target=""
local_build=false
while (( $# )); do
  case "$1" in
    --dmg) bundles="app,dmg"; shift ;;
    --local) local_build=true; shift ;;
    --target)
      (( $# >= 2 )) || { echo "--target 값이 필요합니다." >&2; exit 2; }
      target="$2"; shift 2 ;;
    *) echo "알 수 없는 인자: $1" >&2; exit 2 ;;
  esac
done
if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "DMG 릴리즈 빌드는 macOS와 Xcode Command Line Tools가 필요합니다. 현재 환경: $(uname -s)" >&2
  exit 1
fi
if [[ -z "$target" ]]; then
  case "$(uname -m)" in
    arm64) target="aarch64-apple-darwin" ;;
    x86_64) target="x86_64-apple-darwin" ;;
    *) echo "지원하지 않는 호스트 아키텍처입니다." >&2; exit 1 ;;
  esac
fi
case "$target" in
  aarch64-apple-darwin) arch="arm64" ;;
  x86_64-apple-darwin) arch="x64" ;;
  universal-apple-darwin) arch="universal" ;;
  *) echo "지원하지 않는 macOS target: $target" >&2; exit 2 ;;
esac
xcode-select -p >/dev/null
cargo tauri --version >/dev/null

config_args=()
if $local_build; then
  identity="-"
  config_args=(--config '{"bundle":{"macOS":{"signingIdentity":"-"}}}')
  # Local builds must not import certificates or submit to Apple using inherited credentials.
  unset APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID APPLE_API_ISSUER APPLE_API_KEY APPLE_API_KEY_PATH
  unset APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD CVIEW_NOTARIZE_PROFILE
  echo "로컬 테스트 빌드: ad-hoc 서명, Developer ID 인증서·공증 없음"
else
  identity="${APPLE_SIGNING_IDENTITY:-$(python3 -c 'import json;print(json.load(open("crates/cview-app/tauri.conf.json"))["bundle"]["macOS"].get("signingIdentity") or "")')}"
  if [[ -z "$identity" || "$identity" == "-" ]] || ! security find-identity -v -p codesigning | grep -qF "$identity"; then
    echo "✗ Developer ID 서명 인증서를 찾지 못했습니다: '$identity' — 로컬 테스트는 scripts/build-local-macos.sh 를 사용하세요."; exit 1
  fi
  echo "서명 인증서: $identity"
fi
export APPLE_SIGNING_IDENTITY="$identity"
if ! $local_build && [[ -z "${APPLE_API_KEY:-}${APPLE_ID:-}${CVIEW_NOTARIZE_PROFILE:-}" ]]; then
  echo "⚠︎ 공증 자격 증명이 없어 서명만 하고 공증은 건너뜁니다."
fi

build_stamp=$(mktemp -t cview-release.XXXXXX)
trap 'rm -f "$build_stamp" "$build_stamp.notary.json"' EXIT
target_dir="${CARGO_TARGET_DIR:-target}"
export CARGO_TARGET_DIR="${target_dir:A}"
(cd crates/cview-app && cargo tauri build --target "$target" --bundles "$bundles" "${config_args[@]}" -- --locked)

bundle_root="$CARGO_TARGET_DIR/$target/release/bundle"
app="$bundle_root/macos/CView.app"
[[ -d "$app" ]] || { echo "번들을 찾지 못했습니다: $app"; exit 1; }
echo "── 산출물"
ls -la "$app/Contents/MacOS"
lipo -info "$app/Contents/MacOS/"* || true
if true; then
  echo "── 서명 확인"
  codesign --verify --deep --strict --verbose=2 "$app"
  codesign -dv --verbose=2 "$app" 2>&1 | grep -E "Authority|TeamIdentifier|Runtime" || true
  if ! $local_build; then
    echo "── Gatekeeper 평가"
    spctl -a -vvv -t exec "$app" || echo "(공증 전이면 거부될 수 있습니다)"
    xcrun stapler validate "$app" || true
  fi
fi
# 이번 빌드에서 만든 DMG만 배포한다. 없는 경우 예전 파일로 성공 처리하지 않는다.
if [[ "$bundles" == *dmg* ]]; then
  dmgs=()
  for candidate in "$bundle_root"/dmg/*.dmg(N); do
    [[ "$candidate" -nt "$build_stamp" ]] && dmgs+=("$candidate")
  done
  (( ${#dmgs} == 1 )) || { echo "이번 빌드의 DMG가 정확히 하나여야 합니다: ${#dmgs}개" >&2; exit 1; }
  dmg="${dmgs[1]}"
  echo "── DMG 서명·복사"
  hdiutil verify "$dmg"
  if $local_build; then
    codesign --force --sign - "$dmg"
  else
    codesign --force --timestamp --sign "$identity" "$dmg"
  fi
  codesign --verify --strict --verbose=2 "$dmg"
  version=$(python3 -c 'import json;print(json.load(open("crates/cview-app/tauri.conf.json"))["version"])')
  suffix=""
  $local_build && suffix="_local"
  output="dist/CView_${version}_${arch}${suffix}.dmg"
  mkdir -p dist && cp "$dmg" "$output"
  if [[ -n "${CVIEW_NOTARIZE_PROFILE:-}" ]]; then
    echo "── 공증(프로파일 $CVIEW_NOTARIZE_PROFILE)"
    xcrun notarytool submit "$output" --keychain-profile "$CVIEW_NOTARIZE_PROFILE" --wait --output-format json > "$build_stamp.notary.json"
    python3 -c 'import json,sys; result=json.load(open(sys.argv[1])); print("공증 상태:", result.get("status")); sys.exit(0 if result.get("status") == "Accepted" else 1)' "$build_stamp.notary.json"
    xcrun stapler staple "$output"
    xcrun stapler validate "$output"
  fi
  if ! $local_build; then
    spctl -a -vv -t open --context context:primary-signature "$output" || echo "(Gatekeeper 승인 미확인 — 공증 상태를 확인하세요)"
  fi
  shasum -a 256 "$output" | tee "$output.sha256"
  echo "DMG: $output"
fi
echo "완료: $app"
