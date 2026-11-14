#!/usr/bin/env bash
# 打 .pkg：/Applications/sadp-discover.app + /usr/local/bin/sadp-discover。
# 前提：先跑 bundle.sh；不构建、不签名 app（签名在 bundle.sh 里做）。
#
# 两段式，而不是一步 pkgbuild：
#   pkgbuild    出 component 包（只有 payload 与 --install-location）；
#   productbuild 套上 packaging/macos/installer/Distribution.xml，
#               得到带欢迎页/说明/许可协议/最低系统版本的安装向导。
# 只做 pkgbuild 的话，用户双击看到的是一句「此软件包将安装…」的白板向导。
#
# 环境：
#   INSTALLER_IDENTITY  设置后用 productsign 签 pkg（Developer ID Installer）
# 产物：$DIST/sadp-discover-<version>-macos.pkg
set -euo pipefail

source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/config.sh"
sadp_init

app="$SADP_DIST/sadp-discover.app"
inside="$app/Contents/MacOS/sadp-discover"

[[ -d $app ]] || sadp_die "找不到 ${app}（先 bash scripts/macos/bundle.sh）"
[[ -f $inside ]] || sadp_die "app 里没有 CLI：${inside}"

# /usr/local/bin 的那份直接取自 app 内部，而不是 dist/ 里的原始产物：
# bundle.sh 会对 app 内的副本签名，两者必然逐字节相同，同一个 CDHash 才能让
# .app 的公证票据顺带覆盖 PATH 里的 CLI。从 dist/ 复制则可能拿到未签名的版本。
cli="$inside"

root="$SADP_DIST/pkgroot"
rm -rf "$root"
mkdir -p "$root/Applications" "$root/usr/local/bin"
# 注意 ditto 的语义：目标是已存在的目录时会「并入」而不是「放进去」，
# 所以这里写全路径，不能写成 "$root/Applications/"。
# 用 ditto 而不是 cp -R：bundle 里不该有符号链接，但万一有，cp -R 会把它解引用成副本。
COPYFILE_DISABLE=1 ditto --norsrc "$app" "$root/Applications/sadp-discover.app"
COPYFILE_DISABLE=1 ditto --norsrc "$cli" "$root/usr/local/bin/sadp-discover"
# com.apple.provenance 是内核加的，xattr -d 删不掉（静默无效），别指望这里能清干净；
# pkgbuild 会把它编码成 ._* AppleDouble 条目，属于 payload 清单里的噪音，不影响安装。
xattr -cr "$root" 2>/dev/null || true

sadp_require_cmd pkgbuild
sadp_require_cmd productbuild
work=$(mktemp -d "${TMPDIR:-/tmp}/sadp-pkg.XXXXXX")
trap 'rm -rf "$work"' EXIT

component="sadp-discover-${SADP_VERSION}-component.pkg"
sadp_note "pkgbuild -> ${component}"
pkgbuild --root "$root" \
  --identifier "${SADP_PKG_ID}.pkg" \
  --version "${SADP_VERSION}" \
  --ownership recommended \
  --install-location / \
  "$work/$component"

# payload 清单从 component 上看：product archive 是一层壳，里面才是真的 payload。
sadp_note "component payload"
pkgutil --payload-files "$work/$component"

dist="$work/Distribution.xml"
sed -e "s/__VERSION__/${SADP_VERSION}/g" -e "s/__COMPONENT__/${component}/g" \
  "$SADP_ROOT/packaging/macos/installer/Distribution.xml" > "$dist"

final="$SADP_DIST/sadp-discover-${SADP_VERSION}-macos.pkg"
rm -f "$final"
sadp_note "productbuild -> ${final}"
productbuild --distribution "$dist" \
  --package-path "$work" \
  --resources "$SADP_ROOT/packaging/macos/installer/resources" \
  "$final"

# 签名就地替换：产物名不随「是否签名」变化，下游脚本与 CI 只认一个文件名。
if [[ -n ${INSTALLER_IDENTITY:-} ]]; then
  sadp_note "productsign（身份：${INSTALLER_IDENTITY}）"
  productsign --sign "${INSTALLER_IDENTITY}" "$final" "$final.signed"
  mv -f "$final.signed" "$final"
fi

sadp_note "体积：$(du -h "$final" | cut -f1)"
printf 'packaged: %s\n' "$final"
