#!/usr/bin/env bash
# 只做归档与校验和：输入必须是已构建好的产物，不构建、不签名。
#
# 用法：bash scripts/macos/package.sh dist/sadp-discover
# 产物：$DIST/sadp-discover-<version>-macos-universal.tar.gz 与同名 .sha256
set -euo pipefail

source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/config.sh"
sadp_init

(( $# > 0 )) || sadp_die "用法: package.sh <binary>..."

stage=$(mktemp -d "${TMPDIR:-/tmp}/sadp-package.XXXXXX")
trap 'rm -rf "$stage"' EXIT

for artifact in "$@"; do
  [[ -f $artifact ]] || sadp_die "不存在：$artifact"
  # 统一收进 staging 再打包：避免把产物目录里的中间文件一起卷进归档，
  # 也顺手绕开 "tar -C 不能跨目录" 的麻烦。
  cp -f "$artifact" "$stage/"
done

archive="$SADP_DIST/$(sadp_tar_name)"
rm -f "$archive" "$archive.sha256"

# 两点是为了「同样的输入 -> 同样的 sha256」，方便发布物比对：
#   gzip -n            不写归档时间戳；
#   --no-xattrs        macOS 的 com.apple.provenance 等扩展属性不进 pax 头；
#   COPYFILE_DISABLE   不生成 ._ AppleDouble 文件，弄脏 Linux/Windows 解压结果。
( cd "$stage" && COPYFILE_DISABLE=1 tar --no-xattrs -cf - -- * | gzip -n > "$archive" )
(cd "$SADP_DIST" && shasum -a 256 "$(basename "$archive")" > "$(basename "$archive").sha256")

printf 'archived: %s\n' "$archive"
cat "$archive.sha256"
