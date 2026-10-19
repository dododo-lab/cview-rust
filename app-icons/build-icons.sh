#!/bin/zsh
# 앱 아이콘 만들기 — SVG 원본(icon-light.svg / icon-dark.svg, AppIcon.icon)에서 번들 아이콘을 생성한다.
#  1) 밝은 원본 1024 PNG → `cargo tauri icon` 로 icns·png·ico(기본 아이콘, macOS 25 이하·다른 OS)
#  2) Icon Composer 형식 AppIcon.icon → actool 로 Assets.car(밝은·어두운·틴트 외형) — macOS 26+ 의 Finder·Dock 이
#     시스템 외형에 따라 고른다(Info.plist CFBundleIconName=AppIcon)
#  3) 앱 실행 중 Dock 아이콘·화면 로고용 PNG(밝은·어두운)
# 필요: Xcode(actool), swiftc. 생성물은 crates/cview-app/icons 에 둔다(커밋 대상).
set -euo pipefail
cd "$(dirname "$0")"
out=../crates/cview-app/icons
work=$(mktemp -d)
swiftc -O render.swift -o "$work/render" 2>/dev/null
for v in light dark; do "$work/render" icon-$v.svg "$work/icon-$v-1024.png" 1024 >/dev/null; done
(cd .. && cargo tauri icon "$work/icon-light-1024.png" -o "$work/tauri" >/dev/null)
cp "$work/tauri/32x32.png" "$work/tauri/128x128.png" "$work/tauri/128x128@2x.png" "$work/tauri/icon.png" "$work/tauri/icon.ico" "$work/tauri/icon.icns" "$out/"
mkdir -p "$work/car"
xcrun actool AppIcon.icon --compile "$work/car" --platform macosx --minimum-deployment-target 13.0 \
  --app-icon AppIcon --output-partial-info-plist "$work/partial.plist" >/dev/null
cp "$work/car/Assets.car" "$out/Assets.car"
"$work/render" icon-light.svg "$out/dock-light.png" 512 >/dev/null
"$work/render" icon-dark.svg "$out/dock-dark.png" 512 >/dev/null
"$work/render" tray-template.svg "$out/tray-template.png" 44 >/dev/null
# 화면 안 로고(사이드바·스플래시·정보) — 앱 테마에 맞춰 CSS 가 고른다
mkdir -p ../app-ui/img
"$work/render" icon-light.svg ../app-ui/img/logo-light.png 256 >/dev/null
"$work/render" icon-dark.svg ../app-ui/img/logo-dark.png 256 >/dev/null
rm -rf "$work"
echo "아이콘 생성 완료 → $out"
