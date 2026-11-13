#!/usr/bin/env bash
# 打 .pkg：/Applications/sadp-discover.app + /usr/local/bin/sadp-discover。
# 前提：先跑 bundle.sh；不构建、不签名（签名在 bundle.sh 里做）。
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
cp -R "$app" "$root/Applications/"
cp "$cli" "$root/usr/local/bin/"
xattr -cr "$root" 2>/dev/null || true

final="$SADP_DIST/sadp-discover-${SADP_VERSION}-macos.pkg"
rm -f "$final"
sadp_note "pkgbuild -> ${final}"
pkgbuild --root "$root" \
  --identifier io.github.delta-me13.sadp-discover \
  --version "${SADP_VERSION}" \
  --ownership recommended \
  --install-location / \
  "$final"

# 签名就地替换：产物名不随「是否签名」变化，下游脚本与 CI 只认一个文件名。
if [[ -n ${INSTALLER_IDENTITY:-} ]]; then
  sadp_note "productsign（身份：${INSTALLER_IDENTITY}）"
  productsign --sign "${INSTALLER_IDENTITY}" "$final" "$final.signed"
  mv -f "$final.signed" "$final"
fi

sadp_note "payload"
pkgutil --payload-files "$final"
sadp_note "体积：$(du -h "$final" | cut -f1)"
printf 'packaged: %s\n' "$final"
