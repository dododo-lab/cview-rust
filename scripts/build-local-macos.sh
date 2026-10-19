#!/bin/zsh
# Build a local-test DMG without an Apple Developer account or GitHub Actions.
set -euo pipefail
cd "$(dirname "$0")/.."

target=""
mode_args=(--local)
while (( $# )); do
  case "$1" in
    --signed) mode_args=(); shift ;;
    --help|-h)
      echo "사용: scripts/build-local-macos.sh [--signed] [--target aarch64-apple-darwin|x86_64-apple-darwin|universal-apple-darwin]"
      echo "Rust·Tauri CLI와 필요한 target을 준비하고 dist/에 로컬 테스트용 DMG를 생성합니다."
      echo "--signed: 로컬 테스트 대신 Developer ID 인증서로 배포용 빌드 (공증 설정은 RELEASE.md 참고)."
      exit 0 ;;
    --target)
      (( $# >= 2 )) || { echo "--target 값이 필요합니다." >&2; exit 2; }
      target="$2"; shift 2 ;;
    *) echo "알 수 없는 인자: $1" >&2; exit 2 ;;
  esac
done
if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "로컬 DMG 빌드에는 macOS 13 이상이 필요합니다." >&2
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
  aarch64-apple-darwin|x86_64-apple-darwin) targets=("$target") ;;
  universal-apple-darwin) targets=(aarch64-apple-darwin x86_64-apple-darwin) ;;
  *) echo "지원하지 않는 macOS target: $target" >&2; exit 2 ;;
esac
if ! xcode-select -p >/dev/null 2>&1 || ! xcrun --find clang >/dev/null 2>&1; then
  echo "먼저 xcode-select --install 을 실행해 Command Line Tools 설치를 마친 뒤 다시 실행하세요." >&2
  exit 1
fi
if ! command -v python3 >/dev/null 2>&1; then
  echo "Python 3가 필요합니다. Xcode Command Line Tools 설치를 확인하세요." >&2
  exit 1
fi
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
if ! command -v rustup >/dev/null 2>&1; then
  installer=$(mktemp -t cview-rustup.XXXXXX)
  trap 'rm -f "$installer"' EXIT
  curl --proto '=https' --tlsv1.2 --fail --silent --show-error https://sh.rustup.rs -o "$installer"
  sh "$installer" -y --profile minimal --no-modify-path --default-toolchain none
  rm -f "$installer"
fi
# The repository's rust-toolchain.toml selects and installs the pinned toolchain.
rustup show active-toolchain
rustup target add "${targets[@]}"
cli_version="$(cargo tauri --version 2>/dev/null || true)"
if [[ "$cli_version" != *" 2."* ]]; then
  cargo install tauri-cli --version '^2' --locked
fi
exec zsh scripts/release-macos.sh "${mode_args[@]}" --dmg --target "$target"
