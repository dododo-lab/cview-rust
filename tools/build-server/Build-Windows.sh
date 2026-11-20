#!/bin/bash
# Linux MSVC cross-build. Native Windows execution is a separate check.
set -euo pipefail

root=$1
shift
tools=$(cd "$(dirname "$0")" && pwd)
export PATH="$tools/bin:$HOME/.cargo/bin:$PATH"
export XWIN_CACHE_DIR="$root/xwin-cache"
export CARGO_BUILD_JOBS=3
export CARGO_TERM_COLOR=never
export CI=true
# Tauri disables the UCRT static default library and cargo-xwin disables the
# import library for crt-static. Pass the static archive as an explicit input.
crt_flags='-C target-feature=+crt-static -C link-arg=libucrt.lib'
export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS="$crt_flags"
export CARGO_TARGET_AARCH64_PC_WINDOWS_MSVC_RUSTFLAGS="$crt_flags"

if [[ $1 == --check-only ]]; then
    test -r "$tools/pack_windows_tests.py"
    for tool in cargo cargo-xwin clang-cl llvm-ml llvm-rc lld-link makensis; do
        command -v "$tool" >/dev/null || { echo "Missing $tool" >&2; exit 1; }
    done
    # Tauri 2.12 queries the tray library on the Linux host even for NSIS.
    pkg-config --exists ayatana-appindicator3-0.1 || { echo 'Install libayatana-appindicator3-dev' >&2; exit 1; }
    installed=$(rustup target list --installed)
    for target in x86_64-pc-windows-msvc aarch64-pc-windows-msvc; do
        grep -Fxq "$target" <<< "$installed" || { echo "Missing $target" >&2; exit 1; }
    done
    cargo tauri --version
    printf '%s\n' 'Linux MSVC cross-build for x64 and ARM64'
    exit
fi

source=$1
lane=$2
target_dir=$3
artifacts=$4
version=$5
mode=$6
export CARGO_TARGET_DIR="$target_dir"
cd "$source"

case "$lane" in
    check)
        cargo fmt --all --check
        cargo xwin clippy --workspace --all-targets --locked --target x86_64-pc-windows-msvc -- -D warnings
        if [[ $mode == windows-release ]]; then
            messages="$(dirname "$artifacts")/x64-test-build.ndjson"
            cargo xwin test --workspace --no-run --locked --target x86_64-pc-windows-msvc \
                --message-format=json-render-diagnostics > "$messages" || { tail -15 "$messages"; exit 1; }
            python3 "$tools/pack_windows_tests.py" "$messages" "$artifacts/x64-tests.zip"
        fi
        exit
        ;;
    x64) target=x86_64-pc-windows-msvc ;;
    arm64)
        target=aarch64-pc-windows-msvc
        # ring's ARM64 fallback invokes the GNU-style clang driver. Translate
        # the MSVC-style system include switches inherited from cargo-xwin.
        shim="$target_dir/clang-driver"
        mkdir -p "$shim"
        cat > "$shim/clang" <<'EOF'
#!/bin/bash
if [[ ${0##*/} == clang-cl ]]; then
    exec /usr/bin/clang-cl "$@"
fi
args=()
for arg in "$@"; do
    [[ $arg != /imsvc ]] || arg=-isystem
    args+=("$arg")
done
exec /usr/bin/clang "${args[@]}"
EOF
        chmod 755 "$shim/clang"
        export PATH="$shim:$PATH"
        cargo xwin clippy -p cview-app --all-targets --locked --target "$target" -- -D warnings
        ;;
    *) echo "Unknown build lane: $lane" >&2; exit 1 ;;
esac

bundle="$target_dir/$target/release/bundle/nsis"
if [[ -d "$bundle" ]]; then
    find "$bundle" -maxdepth 1 -type f -name '*.exe' -delete
fi
cd crates/cview-app
cargo tauri build --ci --runner cargo-xwin --target "$target" --bundles nsis -- --locked
shopt -s nullglob
installers=("$bundle"/*.exe)
if [[ ${#installers[@]} -ne 1 ]]; then
    echo "Expected one $lane NSIS installer" >&2
    exit 1
fi
mkdir -p "$artifacts"
cp --update=none-fail "${installers[0]}" "$artifacts/CView_${version}_$lane-setup.exe"
