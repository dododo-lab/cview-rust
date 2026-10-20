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
# 사용: scripts/release-macos.sh [--dmg [--minor|--major]]
#   --dmg 없이: 로컬 서명 빌드만(버전 그대로).
#   --dmg     : 배포 빌드 — 버전과 빌드 번호를 **항상** 올린다(아래 "배포 빌드 순서").
#   공증: CVIEW_NOTARIZE_PROFILE=cview-notarize scripts/release-macos.sh --dmg  (notarytool 프로파일이 있을 때)
#
# 버전의 기준은 하나다: crates/cview-app/tauri.conf.json
#   "version"                    → CFBundleShortVersionString · 앱 정보 · 업데이트 비교 (Cargo 작업공간 버전도 같게 맞춘다)
#   "bundle.macOS.bundleVersion" → CFBundleVersion(빌드 번호, 단조 증가 정수)
#
# 배포 빌드 순서(--dmg) — 반쯤 올린 상태가 남지 않게:
#   1) 작업 트리가 깨끗해야 한다(되돌리기가 안전하도록).
#   2) 검사(fmt · clippy -D warnings · 전체 테스트 · node --check)를 **올리기 전에** 통과해야 한다.
#   3) 버전 patch+1(--minor/--major 면 그 자리 +1, 아래 자리 0)·빌드 번호 +1.
#   4) 빌드 → DMG 서명 → 공증(Accepted 확인) → 스테이플 → 확인.
#      여기서 하나라도 실패하면 올린 파일을 git 으로 되돌리고 끝낸다(trap).
#   4b) 같은 버전의 Windows 설치 파일(NSIS, x64)을 이 Mac 에서 교차 빌드한다(x86_64-pc-windows-gnu · mingw-w64 · makensis).
#       → dist/CView_X.Y.Z_x64-setup.exe. 코드 서명 인증서가 없어 서명되지 않는다(SmartScreen 경고).
#       도구가 없으면(rustup target add x86_64-pc-windows-gnu · brew install mingw-w64 nsis) 실패로 보고 되돌린다.
#       CVIEW_SKIP_WINDOWS=1 이면 건너뛴다.
#   5) 성공하면 "release: vX.Y.Z (build N)" 커밋과 주석 태그 vX.Y.Z 를 만들고, origin 이 있으면 푸시한다.
#      공증 프로파일 없이(서명만) 돌리면 4)의 서명 DMG 까지만 만들고 번호는 되돌린다 — 공증 안 된 DMG 에 번호를 쓰지 않는다.
set -euo pipefail
cd "$(dirname "$0")/.."
# makensis(Homebrew 3.13)는 로캘이 비어 있으면 bad_alloc 으로 죽는다.
export LANG="${LANG:-en_US.UTF-8}" LC_ALL="${LC_ALL:-en_US.UTF-8}"
[[ "$LANG" == "C" || -z "$LANG" ]] && export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8

CONF=crates/cview-app/tauri.conf.json
dist_build=0
part=patch
for arg in "$@"; do
  case "$arg" in
    --dmg) dist_build=1 ;;
    --minor) part=minor ;;
    --major) part=major ;;
    *) echo "알 수 없는 옵션: $arg"; exit 2 ;;
  esac
done

identity="${APPLE_SIGNING_IDENTITY:-$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["bundle"]["macOS"].get("signingIdentity") or "")' "$CONF")}"
if [[ -z "$identity" ]] || ! security find-identity -v -p codesigning | grep -qF "$identity"; then
  echo "✗ 서명 인증서를 찾지 못했습니다: '$identity' — 키체인에 Developer ID 인증서가 있어야 합니다."; exit 1
fi
echo "서명 인증서: $identity"
if [[ -z "${APPLE_API_KEY:-}${APPLE_ID:-}${CVIEW_NOTARIZE_PROFILE:-}" ]]; then
  echo "⚠︎ 공증 자격 증명이 없어 서명만 하고 공증은 건너뜁니다."
fi

BUMPED_FILES=($CONF Cargo.toml Cargo.lock)
bumped=0
revert_bump() {
  if (( bumped )); then
    echo "↩︎ 배포 빌드가 끝나지 않아 버전·빌드 번호를 되돌립니다."
    git checkout -- $BUMPED_FILES
    bumped=0
  fi
}

if (( dist_build )); then
  if [[ -n "$(git status --porcelain)" ]]; then
    echo "✗ 작업 트리가 깨끗하지 않습니다 — 배포 빌드는 커밋된 코드로만 만듭니다."; git status --short; exit 1
  fi
  echo "── 검사(올리기 전에)"
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -q -- -D warnings
  cargo test --workspace -q 2>&1 | grep -E "^test result|FAILED|panicked" \
    | awk '{p+=$4; f+=$6} /FAILED|panicked/ {print} END {print "   테스트 통과", p, "실패", f; exit (f > 0)}'
  for f in app-ui/js/*.js; do node --check "$f"; done
  echo "── 버전·빌드 번호 올리기($part)"
  trap revert_bump EXIT INT TERM
  bumped=1
  read -r version build < <(python3 - "$CONF" "$part" <<'PY'
import json, re, sys
conf, part = sys.argv[1], sys.argv[2]
d = json.load(open(conf))
major, minor, patch = (int(x) for x in d["version"].split("."))
if part == "major": major, minor, patch = major + 1, 0, 0
elif part == "minor": minor, patch = minor + 1, 0
else: patch += 1
version = f"{major}.{minor}.{patch}"
try: build = int(d["bundle"]["macOS"].get("bundleVersion") or 0) + 1
except ValueError: build = 1
# 들여쓰기·키 순서를 그대로 두려고 텍스트에서 바꾼다.
s = open(conf).read()
s = re.sub(r'("version":\s*")[^"]+(")', rf'\g<1>{version}\2', s, count=1)
s = re.sub(r'("bundleVersion":\s*")[^"]*(")', rf'\g<1>{build}\2', s, count=1)
assert json.loads(s)["version"] == version and json.loads(s)["bundle"]["macOS"]["bundleVersion"] == str(build)
open(conf, "w").write(s)
c = open("Cargo.toml").read()
c2 = re.sub(r'(\[workspace\.package\][^\[]*?\nversion\s*=\s*")[^"]+(")', rf'\g<1>{version}\2', c, count=1, flags=re.S)
assert c2 != c, "Cargo.toml 작업공간 버전을 찾지 못했습니다"
open("Cargo.toml", "w").write(c2)
print(version, build)
PY
)
  echo "   → v$version (빌드 $build)"
  # 예전 DMG 를 다시 쓰지 않게 지운다.
  rm -f target/release/bundle/dmg/*.dmg(N) dist/CView_${version}_arm64.dmg(N)
fi

(cd crates/cview-app && cargo tauri build --bundles app)
if (( dist_build )); then
  # DMG 만들기는 임시 이미지를 떼어 낼 때 "리소스 사용 중"(새 볼륨을 Spotlight·Finder 가 잠깐 잡음)으로 가끔 실패한다.
  # 남은 임시 이미지를 강제로 떼고 몇 번 다시 시도한다(컴파일은 위에서 끝났으므로 bundle 만 다시 — dmg 만 묶으면 tauri 가 .app 을 지우므로 app,dmg 로).
  dmg_ok=0
  for attempt in 1 2 3; do
    for dev in $(hdiutil info -plist | python3 -c '
import plistlib, sys
for img in plistlib.loads(sys.stdin.buffer.read()).get("images", []):
    if "/rw." in img.get("image-path", "") and "CView_" in img.get("image-path", ""):
        ents = [e["dev-entry"] for e in img.get("system-entities", []) if "dev-entry" in e]
        if ents: print(min(ents, key=len))'); do
      hdiutil detach "$dev" -force >/dev/null 2>&1 || true
    done
    rm -f target/release/bundle/macos/rw.*.dmg(N)
    if (cd crates/cview-app && cargo tauri bundle --bundles app,dmg); then dmg_ok=1; break; fi
    echo "⚠︎ DMG 만들기 실패($attempt/3) — 잠시 뒤 다시 시도합니다."
    sleep 5
  done
  (( dmg_ok )) || { echo "✗ DMG 를 만들지 못했습니다"; exit 1; }
fi

app="target/release/bundle/macos/CView.app"
[[ -d "$app" ]] || { echo "번들을 찾지 못했습니다: $app"; exit 1; }
echo "── 산출물"
ls -la "$app/Contents/MacOS"
lipo -info "$app/Contents/MacOS/"* || true
echo "   CFBundleShortVersionString $(plutil -extract CFBundleShortVersionString raw "$app/Contents/Info.plist")"
echo "   CFBundleVersion $(plutil -extract CFBundleVersion raw "$app/Contents/Info.plist")"
echo "── 서명 확인"
codesign --verify --deep --strict --verbose=2 "$app"
codesign -dv --verbose=2 "$app" 2>&1 | grep -E "Authority|TeamIdentifier|Runtime" || true
echo "── Gatekeeper 평가"
spctl -a -vvv -t exec "$app" || echo "(공증 전이면 거부될 수 있습니다)"
xcrun stapler validate "$app" || true

if (( dist_build )); then
  dmg=$(ls -t target/release/bundle/dmg/*.dmg(N) 2>/dev/null | head -1 || true)
  [[ -n "$dmg" ]] || { echo "✗ DMG 를 찾지 못했습니다"; exit 1; }
  out="dist/CView_${version}_arm64.dmg"
  echo "── DMG 서명·복사 → $out"
  codesign --force --timestamp --sign "$identity" "$dmg"
  mkdir -p dist && cp "$dmg" "$out"
  if [[ -z "${CVIEW_NOTARIZE_PROFILE:-}" ]]; then
    echo "⚠︎ 공증 프로파일이 없어 서명만 한 DMG 입니다(배포용 아님) — 번호는 되돌립니다: $out"
    exit 0
  fi
  echo "── 공증(프로파일 $CVIEW_NOTARIZE_PROFILE)"
  # notarytool 은 결과가 Invalid 여도 0 으로 끝나므로 상태를 직접 확인한다.
  result=$(xcrun notarytool submit "$out" --keychain-profile "$CVIEW_NOTARIZE_PROFILE" --wait --output-format json)
  echo "$result"
  sub_status=$(python3 -c 'import json,sys;print(json.loads(sys.argv[1]).get("status",""))' "$result")
  sub_id=$(python3 -c 'import json,sys;print(json.loads(sys.argv[1]).get("id",""))' "$result")
  if [[ "$sub_status" != "Accepted" ]]; then
    echo "✗ 공증 결과: $sub_status — 로그:"
    xcrun notarytool log "$sub_id" --keychain-profile "$CVIEW_NOTARIZE_PROFILE" || true
    exit 1
  fi
  xcrun stapler staple "$out"
  xcrun stapler validate "$out"
  spctl -a -vv -t install "$out"
  shasum -a 256 "$out"
  stat -f "   %z bytes" "$out"
  if [[ -z "${CVIEW_SKIP_WINDOWS:-}" ]]; then
    echo "── Windows 설치 파일(x64, 교차 빌드)"
    win_out="dist/CView_${version}_x64-setup.exe"
    rm -f target/x86_64-pc-windows-gnu/release/bundle/nsis/*.exe(N) "$win_out"
    (cd crates/cview-app && cargo tauri build --target x86_64-pc-windows-gnu --bundles nsis)
    win_exe=$(ls -t target/x86_64-pc-windows-gnu/release/bundle/nsis/*.exe(N) | head -1)
    [[ -n "$win_exe" ]] || { echo "✗ Windows 설치 파일을 찾지 못했습니다"; exit 1; }
    cp "$win_exe" "$win_out"
    shasum -a 256 "$win_out"
    stat -f "   %z bytes" "$win_out"
  fi
  echo "── 커밋·태그 v$version (빌드 $build)"
  git add $BUMPED_FILES
  git commit -q -m "release: v$version (build $build)" -m "공증 제출 $sub_id — Accepted. dist/CView_${version}_arm64.dmg"
  git tag -a "v$version" -m "CView v$version (build $build)"
  bumped=0
  trap - EXIT INT TERM
  if git remote get-url origin >/dev/null 2>&1; then
    git push -q origin HEAD
    git push -q origin "v$version"
    echo "   푸시: $(git rev-parse --short HEAD) + 태그 v$version"
  fi
fi
echo "완료: $app"
