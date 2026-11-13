#!/usr/bin/env bash
# 只做代码签名：输入必须是已存在的产物，不构建、不打包。
#
# 用法：
#   CODESIGN_IDENTITY="Developer ID Application: ..." bash scripts/macos/sign.sh dist/sadp-discover
#   CODESIGN_IDENTITY=- bash scripts/macos/sign.sh dist/sadp-discover    # ad-hoc，本地验证用
#
# 未设置 CODESIGN_IDENTITY 时跳过（保持「默认不签名」的行为）。
set -euo pipefail

source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/config.sh"
sadp_init

(( $# > 0 )) || sadp_die "用法: sign.sh <file>..."

identity=${CODESIGN_IDENTITY:-}
if [[ -z $identity ]]; then
  sadp_note "未设置 CODESIGN_IDENTITY，跳过签名"
  exit 0
fi

sadp_require_cmd codesign

for target in "$@"; do
  [[ -e $target ]] || sadp_die "不存在：$target"
  if [[ $identity == "-" ]]; then
    # ad-hoc 没有时间戳服务可用，也不能带 --timestamp。
    sadp_note "ad-hoc 签名 $target"
    codesign --force --sign - "$target"
  else
    case "$target" in
      *.dmg)
        # 磁盘镜像是容器而不是可执行文件：hardened runtime 对它没有意义，
        # 带上 --options runtime 只会在 spctl 输出里留一行无关信息。
        sadp_note "签名 ${target}（时间戳）"
        codesign --force --timestamp --sign "$identity" "$target"
        ;;
      *)
        sadp_note "签名 ${target}（hardened runtime + 时间戳）"
        codesign --force --options runtime --timestamp --sign "$identity" "$target"
        ;;
    esac
  fi
  codesign --verify --strict --verbose=2 "$target"
  codesign --verify --strict --verbose=2 "$target"
done
