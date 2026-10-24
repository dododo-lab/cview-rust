#!/bin/zsh
# macOS·Windows 릴리스 빌드·서명·공증·업데이트 서버 자동 게시 (SYS-26).
#
# 서명: tauri.conf.json 의 bundle.macOS.signingIdentity(Developer ID)로 **모든 빌드(디버그·릴리스)** 를 서명한다.
#   코드 식별(identifier + 팀 ID)이 빌드마다 같아야 키체인의 "항상 허용"이 유지된다 — 애드혹 서명은 빌드마다
#   식별이 바뀌어 실행할 때마다 키체인 암호를 다시 묻는다. 다른 인증서는 APPLE_SIGNING_IDENTITY 로 덮어쓴다.
# 공증 값은 이 스크립트에 적지 않는다 — 실행하는 사람이 환경 변수로 넘긴다(Tauri 번들러가 읽는다).
#   공증(둘 중 하나):
#     APPLE_API_ISSUER · APPLE_API_KEY · APPLE_API_KEY_PATH   (App Store Connect API 키, 권장)
#     APPLE_ID · APPLE_PASSWORD(앱 암호) · APPLE_TEAM_ID
#
# 사용: scripts/release-macos.sh [--dmg [--minor|--major] [--no-notarize]]
#   --dmg 없이: 로컬 서명 빌드만(버전 그대로).
#   --dmg     : 배포 빌드 — 버전과 빌드 번호를 **항상** 올린다(아래 "배포 빌드 순서").
#   공증 프로파일 기본값: cview-notarize (CVIEW_NOTARIZE_PROFILE로 변경 가능).
#   릴리스 설명: CVIEW_RELEASE_NOTES_FILE=<UTF-8 파일> (없으면 Git 변경 목록 사용).
#   --no-notarize: --dmg 와 함께 — 공증만 건너뛰고 나머지(번호 올리기 · Windows 설치 파일 · 커밋 · 태그 · 푸시)는 그대로 한다.
#                  공증 안 된 DMG 는 Gatekeeper 가 막으므로, 그래도 배포하겠다고 명시할 때만 쓴다.
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
#       배포에서는 CVIEW_SKIP_WINDOWS를 허용하지 않는다.
#   5) 커밋·태그 생성 → 양쪽 파일 SSH 전송 → 서버 원자적 게시 → 서버/클라이언트 전체 다운로드 검증 → Git 푸시.
#      게시 중 실패하면 버전·산출물·설명을 보존하고 같은 버전으로 게시를 재시도한다.
#      --no-notarize를 주면 공증만 건너뛰며 릴리스 설명에 명시한다.
set -euo pipefail
cd "$(dirname "$0")/.."
# makensis(Homebrew 3.13)는 로캘이 비어 있으면 bad_alloc 으로 죽는다.
export LANG="${LANG:-en_US.UTF-8}" LC_ALL="${LC_ALL:-en_US.UTF-8}"
[[ "$LANG" == "C" || -z "$LANG" ]] && export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8

CONF=crates/cview-app/tauri.conf.json
# 빌드 산출물 폴더는 cargo 에게 묻는다 — .cargo/config.toml 의 build.target-dir · CARGO_TARGET_DIR 을 따른다(기본 ./target).
TARGET=$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys;print(json.load(sys.stdin)["target_directory"])')
[[ -d "${TARGET:h}" ]] || { echo "✗ 빌드 폴더의 상위 경로가 없습니다(외장 볼륨이 붙어 있나요?): $TARGET"; exit 1; }
echo "빌드 폴더: $TARGET"
dist_build=0
part=patch
no_notarize=0
for arg in "$@"; do
  case "$arg" in
    --dmg) dist_build=1 ;;
    --minor) part=minor ;;
    --major) part=major ;;
    --no-notarize) no_notarize=1 ;;
    *) echo "알 수 없는 옵션: $arg"; exit 2 ;;
  esac
done
if (( no_notarize && ! dist_build )); then
  echo "✗ --no-notarize는 --dmg와 함께 사용합니다."; exit 2
fi
if (( dist_build )); then
  [[ -z "${CVIEW_SKIP_WINDOWS:-}" ]] || { echo "✗ 배포에는 두 플랫폼 설치 파일이 모두 필요합니다. CVIEW_SKIP_WINDOWS를 해제하세요."; exit 2; }
  export CVIEW_NOTARIZE_PROFILE="${CVIEW_NOTARIZE_PROFILE:-cview-notarize}"
fi

identity="${APPLE_SIGNING_IDENTITY:-$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["bundle"]["macOS"].get("signingIdentity") or "")' "$CONF")}"
if [[ -z "$identity" ]] || ! security find-identity -v -p codesigning | grep -qF "$identity"; then
  echo "✗ 서명 인증서를 찾지 못했습니다: '$identity' — 키체인에 Developer ID 인증서가 있어야 합니다."; exit 1
fi
echo "서명 인증서: $identity"
if (( no_notarize )); then
  echo "⚠︎ --no-notarize — 공증 없이 서명만 해서 배포합니다."
elif [[ -z "${APPLE_API_KEY:-}${APPLE_ID:-}${CVIEW_NOTARIZE_PROFILE:-}" ]]; then
  echo "⚠︎ 공증 자격 증명이 없어 서명만 하고 공증은 건너뜁니다."
fi

BUMPED_FILES=($CONF Cargo.toml Cargo.lock)
bumped=0
revert_bump() {
  if (( bumped )); then
    echo "↩︎ 배포 빌드가 끝나지 않아 버전·빌드 번호를 되돌립니다."
    git restore --source=HEAD --staged --worktree -- $BUMPED_FILES
    bumped=0
  fi
}

if (( dist_build )); then
  if [[ -n "$(git status --porcelain)" ]]; then
    echo "✗ 작업 트리가 깨끗하지 않습니다 — 배포 빌드는 커밋된 코드로만 만듭니다."; git status --short; exit 1
  fi
  echo "── 업데이트 서버·인증 사전 확인"
  next_version=$(python3 - "$CONF" "$part" <<'PY'
import json, sys
major, minor, patch = map(int, json.load(open(sys.argv[1]))['version'].split('.'))
if sys.argv[2] == 'major': major, minor, patch = major + 1, 0, 0
elif sys.argv[2] == 'minor': minor, patch = minor + 1, 0
else: patch += 1
print(f'{major}.{minor}.{patch}')
PY
)
  python3 scripts/publish-update.py --preflight --version "$next_version"
  if (( ! no_notarize )); then
    xcrun notarytool history --keychain-profile "$CVIEW_NOTARIZE_PROFILE" --output-format json >/dev/null
  fi
  if [[ -n "${CVIEW_RELEASE_NOTES_FILE:-}" && ! -s "$CVIEW_RELEASE_NOTES_FILE" ]]; then
    echo "✗ 릴리스 설명 파일이 없거나 비어 있습니다: $CVIEW_RELEASE_NOTES_FILE"; exit 1
  fi
  echo "── 검사(올리기 전에)"
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -q -- -D warnings
  # Windows 판도 만들므로 Windows 타깃 clippy 도(타깃이 있을 때).
  if [[ -z "${CVIEW_SKIP_WINDOWS:-}" ]] && rustup target list --installed 2>/dev/null | grep -q x86_64-pc-windows-gnu; then
    cargo clippy -p cview-app --target x86_64-pc-windows-gnu -q -- -D warnings
  fi
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
  rm -f "$TARGET"/release/bundle/dmg/*.dmg(N) dist/CView_${version}_arm64.dmg(N)
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
    rm -f "$TARGET"/release/bundle/macos/rw.*.dmg(N)
    # CI=true → tauri 가 bundle_dmg.sh 에 --skip-jenkins 를 넘긴다: Finder 를 열어 아이콘을 배치하는 AppleScript 를 건너뛰어
    # 만드는 동안 화면에 창이 뜨지 않는다(사용자 요청 2026-10-21). 임시 이미지는 원래 -noautoopen -nobrowse 로 붙는다.
    # 대신 창 크기·아이콘 위치 같은 꾸밈은 없다 — Applications 바로가기(끌어다 설치)는 그대로 들어간다.
    if (cd crates/cview-app && CI=true cargo tauri bundle --bundles app,dmg); then dmg_ok=1; break; fi
    echo "⚠︎ DMG 만들기 실패($attempt/3) — 잠시 뒤 다시 시도합니다."
    sleep 5
  done
  (( dmg_ok )) || { echo "✗ DMG 를 만들지 못했습니다"; exit 1; }
fi

app="$TARGET/release/bundle/macos/CView.app"
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
  dmg=$(ls -t "$TARGET"/release/bundle/dmg/*.dmg(N) 2>/dev/null | head -1 || true)
  [[ -n "$dmg" ]] || { echo "✗ DMG 를 찾지 못했습니다"; exit 1; }
  out="dist/CView_${version}_arm64.dmg"
  echo "── DMG 서명·복사 → $out"
  codesign --force --timestamp --sign "$identity" "$dmg"
  mkdir -p dist && cp "$dmg" "$out"
  if (( no_notarize )); then
    # --no-notarize: 일부러 공증 없이 배포한다(서명만, Gatekeeper 가 막는다). 번호·커밋·태그·Windows 설치 파일은 공증한 배포와 같다.
    echo "⚠︎ --no-notarize — 공증 없이 서명만 한 DMG 로 배포합니다: $out"
    shasum -a 256 "$out"
    stat -f "   %z bytes" "$out"
    notarize_note="공증 안 함(--no-notarize) — 서명만."
  elif [[ -z "${CVIEW_NOTARIZE_PROFILE:-}" ]]; then
    echo "⚠︎ 공증 프로파일이 없어 서명만 한 DMG 입니다(배포용 아님) — 번호는 되돌립니다: $out"
    exit 0
  else
    echo "── 공증(프로파일 $CVIEW_NOTARIZE_PROFILE)"
    # notarytool 은 결과가 Invalid 여도 0 으로 끝나므로 상태를 직접 확인한다.
    # notarytool --wait 는 기다리는 중 스스로 죽는다(2026-10-21 SIGBUS — 문자열 포맷 중 스택 가드, 애플 도구 쪽; 0.1.8 은 3번 모두).
    # 그래서 올리기만 하고(--no-wait, 바로 끝남) 상태는 짧은 info 호출로 15초마다 묻는다 — 한 번 죽어도 다음 물음이 이어간다.
    # submit 도 올린 뒤 ID 를 찍기 전에 죽을 때가 있다(2026-10-21 0.1.10 — 애플에는 들어감). 그러면 history 에서
    # 이 파일 이름으로 이번 실행 뒤에 들어간 제출을 찾아 이어 간다(다시 내면 같은 DMG 가 두 번 올라간다).
    sub_id=""
    submit_since=$(date -u +%Y-%m-%dT%H:%M:%S)
    for attempt in 1 2 3; do
      if submitted=$(xcrun notarytool submit "$out" --keychain-profile "$CVIEW_NOTARIZE_PROFILE" --no-wait --no-progress --output-format json); then
        sub_id=$(python3 -c 'import json,sys;print(json.loads(sys.argv[1]).get("id",""))' "$submitted" 2>/dev/null || true)
        [[ -n "$sub_id" ]] && break
      fi
      sleep 5
      sub_id=$(xcrun notarytool history --keychain-profile "$CVIEW_NOTARIZE_PROFILE" --output-format json 2>/dev/null | python3 -c '
import json, sys
name, since = sys.argv[1], sys.argv[2]
subs = [s for s in json.load(sys.stdin).get("history", []) if s.get("name") == name and s.get("createdDate", "")[:19] >= since]
print(max(subs, key=lambda s: s["createdDate"])["id"] if subs else "")' "$(basename "$out")" "$submit_since" 2>/dev/null || true)
      if [[ -n "$sub_id" ]]; then echo "   submit 은 죽었지만 제출은 들어갔습니다 — history 에서 찾음"; break; fi
      echo "⚠︎ 공증 제출 실패($attempt/3) — 다시 냅니다"
      sleep 5
    done
    [[ -n "$sub_id" ]] || { echo "✗ 공증 제출을 하지 못했습니다"; exit 1; }
    echo "   제출 $sub_id — 결과를 기다립니다"
    result=""
    sub_status="In Progress"
    for poll in $(seq 1 120); do
      sleep 15
      if result=$(xcrun notarytool info "$sub_id" --keychain-profile "$CVIEW_NOTARIZE_PROFILE" --output-format json 2>/dev/null); then
        sub_status=$(python3 -c 'import json,sys;print(json.loads(sys.argv[1]).get("status",""))' "$result" 2>/dev/null || echo "")
        [[ "$sub_status" == "In Progress" || -z "$sub_status" ]] || break
      fi
    done
    echo "$result"
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
    notarize_note="공증 제출 $sub_id — Accepted."
  fi
  if [[ -z "${CVIEW_SKIP_WINDOWS:-}" ]]; then
    echo "── Windows 설치 파일(x64, 교차 빌드)"
    win_out="dist/CView_${version}_x64-setup.exe"
    rm -f "$TARGET"/x86_64-pc-windows-gnu/release/bundle/nsis/*.exe(N) "$win_out"
    (cd crates/cview-app && cargo tauri build --target x86_64-pc-windows-gnu --bundles nsis)
    win_exe=$(ls -t "$TARGET"/x86_64-pc-windows-gnu/release/bundle/nsis/*.exe(N) | head -1)
    [[ -n "$win_exe" ]] || { echo "✗ Windows 설치 파일을 찾지 못했습니다"; exit 1; }
    cp "$win_exe" "$win_out"
    shasum -a 256 "$win_out"
    stat -f "   %z bytes" "$win_out"
  fi
  notes_out="dist/CView_${version}_notes.txt"
  python3 - "$version" "$build" "$notes_out" "${CVIEW_RELEASE_NOTES_FILE:-}" "$no_notarize" <<'PY'
from pathlib import Path
import subprocess, sys
version, build, output, source, unnotarized = sys.argv[1:]
if source:
    body = Path(source).read_text(encoding='utf-8').strip()
else:
    tag = subprocess.run(['git', 'describe', '--tags', '--abbrev=0'], capture_output=True, text=True)
    revision = tag.stdout.strip() + '..HEAD' if tag.returncode == 0 else 'HEAD'
    body = subprocess.check_output(['git', 'log', revision, '--format=- %s', '-30'], text=True).strip()
    body = body or '새 배포 빌드입니다.'
mac = '공증 안 함(--no-notarize), Developer ID 서명만 적용.' if unnotarized == '1' else 'Developer ID 서명 및 Apple 공증·스테이플 완료.'
notes = (f'CView {version} (빌드 {build})\n\n{body}\n\nmacOS: Apple Silicon(arm64), {mac}\n'
         'Windows: x64 NSIS 설치 파일. 코드 서명 인증서가 없어 미서명이며 SmartScreen 경고가 표시될 수 있습니다.\n'
         '업데이트 확인과 다운로드에는 Tailscale 연결이 필요합니다.\n')
Path(output).write_text(notes, encoding='utf-8')
PY
  echo "── 커밋·태그 v$version (빌드 $build)"
  git add $BUMPED_FILES
  git commit -q -m "release: v$version (build $build)" -m "$notarize_note dist/CView_${version}_arm64.dmg"
  # A committed release may be published even if the SSH response is lost.
  # Preserve its version and artifacts; retry publishing instead of rolling back.
  bumped=0
  trap - EXIT INT TERM
  git tag -a "v$version" -m "CView v$version (build $build)"
  echo "── 업데이트 서버 업로드·게시·다운로드 검증"
  publish_flags=()
  (( no_notarize )) && publish_flags+=(--allow-unnotarized)
  if ! python3 scripts/publish-update.py --version "$version" --notes-file "$notes_out" "${publish_flags[@]}"; then
    echo "✗ 게시·검증 미완료. 새 버전을 만들지 말고 같은 파일로 재시도하세요:"
    echo "python3 scripts/publish-update.py --version $version --notes-file $notes_out ${publish_flags[*]}"
    echo "검증 성공 후 git push origin HEAD 및 git push origin v$version 을 실행하세요."
    exit 1
  fi
  if git remote get-url origin >/dev/null 2>&1; then
    git push -q origin HEAD
    git push -q origin "v$version"
    echo "   푸시: $(git rev-parse --short HEAD) + 태그 v$version"
  fi
fi
echo "완료: $app"
