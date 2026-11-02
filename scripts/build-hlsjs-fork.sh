#!/usr/bin/env bash
# CView hls.js 포크 빌드.
#
#   third_party/hlsjs/fork.json   업스트림 태그·tarball SHA-256·포크 리비전·빌드 대상·기능 플래그
#   third_party/hlsjs/patches/    우리 패치(NNNN-이름.patch, 번호 순서대로 적용)
#   third_party/hlsjs/contract/   빌드 번들 계약 테스트(node --test)
#
# 업스트림 소스는 저장소에 두지 않는다. fork.json 의 태그 tarball 을 받아 SHA-256 을 확인한 뒤 쓴다.
#
# 사용:
#   bash scripts/build-hlsjs-fork.sh          빌드 → app-ui/player/hls.min.js 와
#                                             third_party/hlsjs/integrity.json 갱신, 계약 테스트 실행
#   bash scripts/build-hlsjs-fork.sh --check  다시 빌드해 커밋된 파일·매니페스트와 바이트 단위로 같은지 검사
#   bash scripts/build-hlsjs-fork.sh --debug  같은 소스·패치·기능 플래그로 압축하지 않은 빌드(+소스맵)를
#                                             <작업 폴더>/debug/ 에 만든다. 앱 리소스·매니페스트는 그대로다.
#
# 작업 폴더: CVIEW_HLSJS_WORK, 없으면 cargo target-dir 아래 hlsjs-fork.
# 버전: 패치가 없고 forkRevision 이 0 이면 업스트림 버전 그대로(공식 릴리스와 바이트 동일),
#       아니면 "<업스트림>-cview.<forkRevision>".
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODE="${1:-build}"
case "$MODE" in build|--check|--debug) ;; *) echo "usage: $0 [--check|--debug]" >&2; exit 2 ;; esac

FORK_DIR="$ROOT/third_party/hlsjs"
PATCHES="$FORK_DIR/patches"
FORK_JSON="$FORK_DIR/fork.json"
MANIFEST="$FORK_DIR/integrity.json"
RESOURCE="$ROOT/app-ui/player/hls.min.js"
RESOURCE_REL="app-ui/player/hls.min.js"

command -v node >/dev/null || { echo "node 가 필요하다(업스트림 .node-version 참고)" >&2; exit 1; }

if [ -z "${CVIEW_HLSJS_WORK:-}" ]; then
    TARGET_DIR="$(cd "$ROOT" && cargo metadata --format-version 1 --no-deps 2>/dev/null \
        | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{try{console.log(JSON.parse(s).target_directory)}catch{}})' || true)"
    CVIEW_HLSJS_WORK="${TARGET_DIR:-$ROOT/target}/hlsjs-fork"
fi
WORK="$CVIEW_HLSJS_WORK"
TREE="$WORK/tree"
DEPS="$WORK/deps"

TAG="$(node -p 'require(process.argv[1]).upstreamTag' "$FORK_JSON")"
TARBALL_SHA="$(node -p 'require(process.argv[1]).upstreamTarballSHA256' "$FORK_JSON")"
REPO_URL="$(node -p 'require(process.argv[1]).upstreamRepository' "$FORK_JSON")"
TARBALL="$WORK/upstream-$TAG.tar.gz"

# 1) 업스트림 원본 — 받은 tarball 은 SHA-256 이 맞을 때만 쓴다
mkdir -p "$WORK" "$DEPS"
if [ ! -f "$TARBALL" ] || [ "$(shasum -a 256 "$TARBALL" | cut -d' ' -f1)" != "$TARBALL_SHA" ]; then
    curl -fsSL -o "$TARBALL.part" "$REPO_URL/archive/refs/tags/$TAG.tar.gz"
    mv "$TARBALL.part" "$TARBALL"
fi
GOT_SHA="$(shasum -a 256 "$TARBALL" | cut -d' ' -f1)"
[ "$GOT_SHA" = "$TARBALL_SHA" ] || { echo "업스트림 tarball SHA-256 불일치: $GOT_SHA (기대 $TARBALL_SHA)" >&2; exit 1; }

# 2) 원본 풀기 + 패치 적용
rm -rf "$TREE"
mkdir -p "$TREE"
tar -xzf "$TARBALL" -C "$TREE" --strip-components=1
# 작업 트리가 다른 git 저장소 안에 있으면 `git apply` 가 경로를 바깥 저장소 기준으로 해석해 패치를
# **조용히 건너뛴다**(2026-10-04 실제로 겪음). 독립 저장소로 만든다. 패치 작성용 기준 커밋도 겸한다.
git -C "$TREE" init -q
printf 'node_modules\ndist\n' > "$TREE/.git/info/exclude"
git -C "$TREE" add -A
git -C "$TREE" -c user.name=cview-fork -c user.email=fork@invalid commit -qm "upstream $TAG"
PATCH_LIST=()
while IFS= read -r patch; do
    [ -n "$patch" ] || continue
    git -C "$TREE" apply --check --whitespace=nowarn "$patch" \
        || { echo "패치 적용 실패: $patch" >&2; exit 1; }
    git -C "$TREE" apply --whitespace=nowarn "$patch"
    PATCH_LIST+=("$(basename "$patch")")
done < <(find "$PATCHES" -maxdepth 1 -name '*.patch' | sort)
# 적용 결과 검증 — 패치가 있는데 트리가 그대로면 실패로 본다.
if [ "${#PATCH_LIST[@]}" -gt 0 ] && git -C "$TREE" diff --quiet HEAD -- && [ -z "$(git -C "$TREE" ls-files --others --exclude-standard)" ]; then
    echo "패치 ${#PATCH_LIST[@]}개를 적용했는데 소스가 바뀌지 않았다" >&2; exit 1
fi

# 3) 버전·기능 플래그
BUILD_VERSION="$(node -e '
const f = require(process.argv[1]); const n = Number(process.argv[2]);
console.log(f.forkRevision === 0 && n === 0 ? f.upstreamVersion : `${f.upstreamVersion}-cview.${f.forkRevision}`);
' "$FORK_JSON" "${#PATCH_LIST[@]}")"
CONFIG_TYPE="$(node -p 'require(process.argv[1]).configType' "$FORK_JSON")"
[ "$MODE" = "--debug" ] && CONFIG_TYPE="full"
node -e '
const fs = require("fs"); const p = JSON.parse(fs.readFileSync(process.argv[1]));
p.version = process.argv[2]; fs.writeFileSync(process.argv[1], JSON.stringify(p, null, 2) + "\n");
' "$TREE/package.json" "$BUILD_VERSION"
FEATURE_ENV=()
while IFS= read -r line; do [ -n "$line" ] && FEATURE_ENV+=("$line"); done < <(node -e '
const f = require(process.argv[1]).features || {};
for (const [k, v] of Object.entries(f)) console.log(`${k}=${v}`);
' "$FORK_JSON")

# 4) 의존성 — 잠금 파일 해시가 같으면 재사용
LOCK_HASH="$(shasum -a 256 "$TREE/package-lock.json" | cut -d' ' -f1)"
if [ ! -f "$DEPS/.lock-sha256" ] || [ "$(cat "$DEPS/.lock-sha256")" != "$LOCK_HASH" ] || [ ! -d "$DEPS/node_modules" ]; then
    rm -rf "$DEPS/node_modules"
    git -C "$TREE" show HEAD:package.json > "$DEPS/package.json"
    cp "$TREE/package-lock.json" "$DEPS/"
    (cd "$DEPS" && PUPPETEER_SKIP_DOWNLOAD=1 CHROMEDRIVER_SKIP_DOWNLOAD=true \
        npm ci --ignore-scripts --no-audit --no-fund --loglevel=error)
    echo "$LOCK_HASH" > "$DEPS/.lock-sha256"
fi
ln -sfn "$DEPS/node_modules" "$TREE/node_modules"

# 5) 빌드
(cd "$TREE" && env ${FEATURE_ENV[@]+"${FEATURE_ENV[@]}"} npx --no-install rollup --config --configType "$CONFIG_TYPE" >/dev/null)
if [ "$MODE" = "--debug" ]; then
    rm -rf "$WORK/debug" && mkdir -p "$WORK/debug"
    cp "$TREE/dist/hls.js" "$TREE/dist/hls.js.map" "$WORK/debug/"
    echo "hls.js fork $BUILD_VERSION debug build → $WORK/debug/hls.js (+ hls.js.map, patches ${#PATCH_LIST[@]})"
    exit 0
fi
OUTPUT="$TREE/dist/hls.min.js"
[ -f "$OUTPUT" ] || { echo "빌드 결과가 없다: $OUTPUT" >&2; exit 1; }
BUILT_SHA="$(shasum -a 256 "$OUTPUT" | cut -d' ' -f1)"
PATCH_SHA="$(cat /dev/null $(find "$PATCHES" -maxdepth 1 -name '*.patch' | sort) | shasum -a 256 | cut -d' ' -f1)"

if [ "$MODE" = "--check" ]; then
    COMMITTED_SHA="$(shasum -a 256 "$RESOURCE" | cut -d' ' -f1)"
    MANIFEST_SHA="$(node -p 'require(process.argv[1]).sha256' "$MANIFEST")"
    MANIFEST_VERSION="$(node -p 'require(process.argv[1]).version' "$MANIFEST")"
    if [ "$BUILT_SHA" != "$COMMITTED_SHA" ] || [ "$BUILT_SHA" != "$MANIFEST_SHA" ] || [ "$BUILD_VERSION" != "$MANIFEST_VERSION" ]; then
        echo "hls.js 포크 재현 빌드 불일치" >&2
        echo "  built     $BUILT_SHA ($BUILD_VERSION)" >&2
        echo "  committed $COMMITTED_SHA" >&2
        echo "  manifest  $MANIFEST_SHA ($MANIFEST_VERSION)" >&2
        exit 1
    fi
    echo "hls.js fork $BUILD_VERSION reproducible build verified ($BUILT_SHA, patches ${#PATCH_LIST[@]})"
    exit 0
fi

cp "$OUTPUT" "$RESOURCE"
node -e '
const fs = require("fs");
const [manifestPath, forkPath, version, file, sha, patchSha, patchesJson] = process.argv.slice(1);
const fork = JSON.parse(fs.readFileSync(forkPath));
const m = {
  version, file, sha256: sha,
  source: `${fork.upstreamRepository}/releases/tag/${fork.upstreamTag}`,
  upstreamTag: fork.upstreamTag, upstreamTarballSHA256: fork.upstreamTarballSHA256,
  forkRevision: fork.forkRevision, configType: fork.configType, features: fork.features || {},
  patches: JSON.parse(patchesJson), patchesSHA256: patchSha,
  build: "bash scripts/build-hlsjs-fork.sh"
};
fs.writeFileSync(manifestPath, JSON.stringify(m, null, 2) + "\n");
' "$MANIFEST" "$FORK_JSON" "$BUILD_VERSION" "$RESOURCE_REL" "$BUILT_SHA" "$PATCH_SHA" "$(node -e 'console.log(JSON.stringify(process.argv.slice(1)))' ${PATCH_LIST[@]+"${PATCH_LIST[@]}"})"
echo "hls.js fork $BUILD_VERSION → $RESOURCE ($BUILT_SHA, patches ${#PATCH_LIST[@]})"

# 6) 계약 테스트 — 새 번들이 패치 동작과 치지직 재생목록 형태를 그대로 지키는지
node --test --test-reporter=dot "$FORK_DIR"/contract/*.test.cjs
