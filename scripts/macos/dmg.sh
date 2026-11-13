#!/usr/bin/env bash
# 打 .dmg：把已组装好的 .app 放进压缩磁盘镜像，并附一个 /Applications 快捷方式。
# 前提：先跑 bundle.sh（app 已签名；要离线可用的还需先 staple），不构建、不签 app 本身。
#
# 用法：
#   bash scripts/macos/dmg.sh                                 # 默认 dist/sadp-discover.app
#   bash scripts/macos/dmg.sh dist/other.app                  # 换一个 app
#
# 环境：
#   CODESIGN_IDENTITY  设置后对 dmg 本身签名（Developer ID Application；"-" 视为不签）
#   SADP_DMG_FORMAT    镜像格式，默认 UDZO（zlib 压缩、只读）；要调试可设 UDRW
# 产物：$DIST/sadp-discover-<version>-macos.dmg
#
# 注意：dmg 里放的是 .app 的副本，所以必须在 app 完成签名（和 staple）之后再做，
# 否则断网安装的用户拿到的是没有离线票据的 app。
set -euo pipefail

source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/config.sh"
sadp_init

app=${1:-$SADP_DIST/sadp-discover.app}
[[ -d $app ]] || sadp_die "找不到 ${app}（先 bash scripts/macos/bundle.sh）"

sadp_require_cmd hdiutil
sadp_require_cmd ditto

final="$SADP_DIST/sadp-discover-${SADP_VERSION}-macos.dmg"
stage=$(mktemp -d "${TMPDIR:-/tmp}/sadp-dmg.XXXXXX")
# 挂载点也放在临时目录里，避免 /Volumes 里出现半截镜像干扰手动排查。
mnt="$stage/.mnt"
trap 'hdiutil detach "$mnt" >/dev/null 2>&1 || true; rm -rf "$stage"' EXIT

# ditto 而不是 cp -R：签名与公证依赖的元数据、扩展属性要原样过去。
COPYFILE_DISABLE=1 ditto "$app" "$stage/$(basename "$app")"
# 装进去的快捷方式：用户把 app 拖进它就完成安装，不用记住 /Applications 在哪。
ln -s /Applications "$stage/Applications"
# 别把 ._* 与 Finder 的 .DS_Store 带进镜像（会让镜像看起来"有垃圾文件"）。
COPYFILE_DISABLE=1 find "$stage" -name '.DS_Store' -delete 2>/dev/null || true
xattr -cr "$stage" 2>/dev/null || true

rm -f "$final"
sadp_note "hdiutil create -> ${final}"
# -quiet 只压掉进度条；失败照样以非 0 退出。
hdiutil create \
  -volname "sadp-discover ${SADP_VERSION}" \
  -srcfolder "$stage" \
  -fs HFS+ \
  -format "${SADP_DMG_FORMAT:-UDZO}" \
  -ov -quiet \
  "$final"

if [[ -n ${CODESIGN_IDENTITY:-} && ${CODESIGN_IDENTITY} != "-" ]]; then
  sadp_note "签名 dmg（身份：${CODESIGN_IDENTITY}）"
  # dmg 是容器不是可执行文件，hardened runtime（--options runtime）对它没有意义，
  # 所以这里不走 sign.sh 的那条分支。
  codesign --force --timestamp --sign "${CODESIGN_IDENTITY}" "$final"
  codesign --verify --verbose=2 "$final"
fi

hdiutil verify "$final" >/dev/null
sadp_note "dmg 体积：$(du -h "$final" | cut -f1)"
sadp_note "镜像内容"
hdiutil attach -nobrowse -readonly -mountpoint "$mnt" -noautoopen "$final" >/dev/null
ls -1 "$mnt"
hdiutil detach "$mnt" >/dev/null 2>&1 || true
printf 'dmg: %s\n' "$final"
