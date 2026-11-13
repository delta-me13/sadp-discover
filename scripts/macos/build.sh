#!/usr/bin/env bash
# 只做「构建 + 合成 + 架构校验」：不签名、不公证、不打包。
#
# 用法：
#   bash scripts/macos/build.sh
#   TARGETS=aarch64-apple-darwin bash scripts/macos/build.sh      # 单架构，跳过 lipo
#   PACKAGES="sadp-cli sadp-gui" bash scripts/macos/build.sh      # 多包
#
# 环境：TARGETS / PACKAGES / PROFILE / DIST / VERSION / SADP_INSTALL_TARGETS
set -euo pipefail

source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/config.sh"
sadp_init

mkdir -p "$SADP_DIST"

# 先整体预检再动手：缺 target 时应该立刻失败，而不是先花几分钟编完一个架构。
for triple in $SADP_TARGETS; do
  sadp_require_target "$triple"
done
if [[ $(printf '%s\n' $SADP_TARGETS | wc -l) -gt 1 ]]; then
  sadp_require_cmd lipo
fi
for pkg in $SADP_PACKAGES; do
  sadp_bin_for_pkg "$pkg" >/dev/null || sadp_die "未知包 ${pkg}（在 scripts/macos/config.sh 的 sadp_bin_for_pkg 里登记）"
done

for pkg in $SADP_PACKAGES; do
  bin=$(sadp_bin_for_pkg "$pkg")

  inputs=()
  for triple in $SADP_TARGETS; do
    sadp_note "cargo build --profile $SADP_PROFILE -p $pkg --target $triple"
    cargo build --profile "$SADP_PROFILE" --target "$triple" -p "$pkg"

    path=$(sadp_bin_path "$triple" "$bin")
    [[ -x $path ]] || sadp_die "构建成功但找不到可执行文件：$path"
    inputs+=("$path")
  done

  out="$SADP_DIST/$bin"
  if (( ${#inputs[@]} > 1 )); then
    sadp_require_cmd lipo
    sadp_note "lipo -create -> $bin"
    lipo -create "${inputs[@]}" -output "$out"
  else
    # 单架构时不做无意义的 lipo：直接落到产物目录，便于本地快速验证。
    cp -f "${inputs[0]}" "$out"
  fi
  chmod +x "$out"

  if command -v lipo >/dev/null 2>&1; then
    lipo -info "$out"
  else
    sadp_warn "没有 lipo，跳过架构校验（非 macOS 上的单架构构建）"
  fi
  printf 'built: %s\n' "$out"
done
