#!/usr/bin/env bash
# 构建 macOS 通用二进制（arm64 + x86_64），可选签名、公证、归档。
#
# 这是**编排入口**：真正的动作在 scripts/macos/ 下按职责拆开，可单独调用。
# 环境变量与产物路径保持向后兼容（CI 与文档都按这个契约使用）。
#
# 用法：
#   ./scripts/build-macos-universal.sh
#   VERSION=0.2.0 ./scripts/build-macos-universal.sh
#   CODESIGN_IDENTITY="Developer ID Application: Foo (TEAMID)" ./scripts/build-macos-universal.sh
#   ./scripts/build-macos-universal.sh --build-only     # 只构建，不签名/公证/打包
#
# 环境变量：
#   VERSION             版本号，默认从 cargo 读取
#   OUT / DIST          输出目录，默认 dist
#   TARGETS             目标架构，默认两个 Apple target
#   PACKAGES            要构建的包，默认 sadp-cli
#   PROFILE             cargo profile，默认 release
#   CODESIGN_IDENTITY   设置后对产物签名（hardened runtime + 时间戳；"-" 为 ad-hoc）
#   NOTARY_PROFILE      设置后提交公证（需先 xcrun notarytool store-credentials）
#   ASC_KEY_PATH/ASC_KEY_ID/ASC_ISSUER  用 App Store Connect API Key 公证（CI 用）
#   SADP_INSTALL_TARGETS=1  缺 target 时允许脚本代为 rustup target add
set -euo pipefail

source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/macos/config.sh"
sadp_init

build_only=0
for arg in "$@"; do
  case "$arg" in
    --build-only) build_only=1 ;;
    -h|--help)
      # 只打印文件头的注释块，不把代码漏进帮助里。
      # 规则顺序有意义：先判断「不是注释就停」，再剥离 # 前缀，
      # 否则 sub() 改掉 $0 后同一条记录会再命中「非注释」规则而提前退出。
      awk 'NR == 1 { next } !/^#/ { exit } { sub(/^# ?/, ""); print }' "${BASH_SOURCE[0]}"
      exit 0
      ;;
    *) sadp_die "未知参数：${arg}（支持 --build-only / --help）" ;;
  esac
done

# 平台门槛只在这一层：lipo 与 codesign 都是 macOS 专有工具。
if [[ "$(uname -s)" != "Darwin" ]]; then
  sadp_die "该脚本只能在 macOS 上运行（需要 lipo）；非 macOS 上可用 TARGETS=<单一 target> 跑 scripts/macos/build.sh"
fi

sadp_note "sadp-discover ${SADP_VERSION}（targets: ${SADP_TARGETS}, profile: ${SADP_PROFILE}）"

# 把解析结果显式交给子脚本：子脚本各自会重新 init，必须看到同一套值，
# 否则 OUT/VERSION/TARGETS 只对编排层生效，产物会落回默认目录。
export DIST="$SADP_DIST"
export TARGETS="$SADP_TARGETS"
export PACKAGES="$SADP_PACKAGES"
export PROFILE="$SADP_PROFILE"
export VERSION="$SADP_VERSION"

bash "$SADP_ROOT/scripts/macos/build.sh"

artifacts=()
for pkg in $SADP_PACKAGES; do
  bin=$(sadp_bin_for_pkg "$pkg") || sadp_die "未知包 $pkg"
  artifacts+=("$SADP_DIST/$bin")
done

if (( build_only )); then
  sadp_note "--build-only：跳过签名、公证与归档"
  exit 0
fi

CODESIGN_IDENTITY="${CODESIGN_IDENTITY:-}" bash "$SADP_ROOT/scripts/macos/sign.sh" "${artifacts[@]}"
bash "$SADP_ROOT/scripts/macos/notarize.sh" "${artifacts[@]}"
bash "$SADP_ROOT/scripts/macos/package.sh" "${artifacts[@]}"

sadp_note "done"
