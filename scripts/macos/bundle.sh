#!/usr/bin/env bash
# 组装 .app：Info.plist + 图标 + 两个二进制 + 代码签名。不构建、不打 pkg。
#
# 用法：
#   bash scripts/macos/bundle.sh
#   SADP_GUI_BIN=target/aarch64-apple-darwin/release/sadp-discover-gui \
#   SADP_CLI_BIN=dist/sadp-discover bash scripts/macos/bundle.sh
#
# 环境：
#   CODESIGN_IDENTITY  签名身份，默认 "-"（ad-hoc）。arm64 上未签名的 app 会被内核直接杀掉，
#                      所以这里默认就签，不能留空。
#   BUILD_NUMBER       CFBundleVersion，默认 1（CI 用 run number）
# 产物：$DIST/sadp-discover.app
set -euo pipefail

source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/config.sh"
sadp_init

host=$(rustc -vV | sed -n 's/^host: //p')
# 优先用 dist/ 里已经 lipo 过的通用 GUI（`PACKAGES=sadp-gui` 跑 build.sh 的产物），
# 没有才退回宿主架构的单架构构建，避免打包时误取 arm64-only 的 GUI。
if [[ -z ${SADP_GUI_BIN:-} && -f "$SADP_DIST/sadp-discover-gui" ]]; then
  SADP_GUI_BIN="$SADP_DIST/sadp-discover-gui"
fi
gui=${SADP_GUI_BIN:-$SADP_TARGET_DIR/$host/release/sadp-discover-gui}
cli=${SADP_CLI_BIN:-$SADP_DIST/sadp-discover}
app="$SADP_DIST/sadp-discover.app"
icon="$SADP_ROOT/assets/sadp-discover.icns"

[[ -f $gui ]] || sadp_die "找不到 GUI 二进制：${gui}（先 cargo build --release -p sadp-gui）"
[[ -f $cli ]] || sadp_die "找不到 CLI 二进制：${cli}（先 bash scripts/macos/build.sh）"
[[ -f $icon ]] || sadp_die "缺少图标：assets/sadp-discover.icns（运行 python3 packaging/macos/make-icon.py）"

sadp_note "组装 ${app}"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
install -m 755 "$gui" "$app/Contents/MacOS/sadp-discover-gui"
install -m 755 "$cli" "$app/Contents/MacOS/sadp-discover"
install -m 644 "$SADP_ROOT/packaging/macos/Info.plist" "$app/Contents/Info.plist"
sed -i '' -e "s/__VERSION__/${SADP_VERSION}/g" -e "s/__BUILD__/${BUILD_NUMBER:-1}/g" "$app/Contents/Info.plist"
printf 'APPL????' > "$app/Contents/PkgInfo"
install -m 644 "$icon" "$app/Contents/Resources/sadp-discover.icns"
plutil -lint "$app/Contents/Info.plist"

identity=${CODESIGN_IDENTITY:--}
sign() {
  if [[ $identity == "-" ]]; then
    codesign --force --sign - "$1"
  else
    codesign --force --options runtime --timestamp --sign "$identity" "$1"
  fi
}
# 先内后外：嵌套的可执行文件先签，最后签 bundle 本身。
sadp_note "签名（身份：${identity}）"
sign "$app/Contents/MacOS/sadp-discover"
sign "$app/Contents/MacOS/sadp-discover-gui"
sign "$app"
codesign --verify --deep --strict --verbose=2 "$app"

sadp_note "架构"
file "$app/Contents/MacOS/sadp-discover" "$app/Contents/MacOS/sadp-discover-gui"
# CLI 与 GUI 架构不一致时给出警告：这样打出来的包在一半机器上「能装但一半功能起不来」。
cli_archs=$(lipo -archs "$app/Contents/MacOS/sadp-discover")
gui_archs=$(lipo -archs "$app/Contents/MacOS/sadp-discover-gui")
if [[ $cli_archs != "$gui_archs" ]]; then
  sadp_warn "CLI（${cli_archs}）与 GUI（${gui_archs}）架构不一致"
fi
sadp_note "app 体积：$(du -sh "$app" | cut -f1)"
printf 'bundled: %s\n' "$app"
