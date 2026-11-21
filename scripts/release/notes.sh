#!/usr/bin/env bash
# 生成 GitHub Release 的正文。
#
# 约定（README「发布」一节）：Release 页面的正文 = CHANGELOG 里该版本的段落
# + 从实际产物清单生成的下载表 + 校验方式 + 未签名说明。
# 页面不再手写：手写过一次 0.1.0-rc.1，正文与 CHANGELOG 脱节，也没法在
# 下一次发版时复用。这里让「文字」和「产物」都来自可复现的输入。
#
# 用法：scripts/release/notes.sh <version> [asset-dir]
#   version     不带 v 的版本号，必须与 CHANGELOG 的 `## [x.y.z]` 段落一致
#   asset-dir   发布产物所在目录；给了就按真实文件生成下载表，不给就省略该节
#
# 退出码：0 正常；2 用法/环境错误；3 CHANGELOG 里没有该版本段落（调用方回退到
# `gh release create --generate-notes`）。
set -euo pipefail

die() { printf 'notes.sh: %s\n' "$*" >&2; exit 2; }

[ $# -ge 1 ] || die '用法：notes.sh <version> [asset-dir]'

version=${1#v}
asset_dir=${2:-}
# Git Bash / Windows 传进来的可能是 C:\\...，反斜杠在 glob 里是转义符，
# 会让 `"$asset_dir"/*` 一个文件都匹配不到。统一换成正斜杠。
asset_dir=${asset_dir//\\//}

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
changelog="$root/CHANGELOG.md"
[ -f "$changelog" ] || die "找不到 $changelog"

# 取 `## [x.y.z]` 到下一个 `## [` 之间的内容。按行首前缀匹配而不是正则，
# 免得版本号里的 `.` 被当成通配符（0.1.0 会匹配到 0x1y0）。
header="## [$version]"
section=$(
  awk -v h="$header" '
    substr($0, 1, length(h)) == h &&
      (length($0) == length(h) || substr($0, length(h) + 1, 1) == " ") { inside = 1; next }
    inside && substr($0, 1, 4) == "## [" { exit }
    inside { print }
  ' "$changelog"
)

if [ -z "$(printf '%s' "$section" | tr -d '[:space:]')" ]; then
  printf 'notes.sh: CHANGELOG.md 里没有 [%s] 段落\n' "$version" >&2
  exit 3
fi

# 去掉段落首尾的空行，其余保持原样（段落里的 `### Added` 等直接成为正文小节）。
section=$(printf '%s\n' "$section" | awk 'NF { started = 1 } started { print }')

describe_asset() {
  case "$1" in
    *-macos.pkg)
      echo 'macOS 安装包：`/Applications/sadp-discover.app` + `/usr/local/bin/sadp-discover`'
      ;;
    *-macos.dmg)
      echo 'macOS 磁盘映像：拖进「应用程序」即可，内含同一份已签名 app'
      ;;
    *-macos-universal.tar.gz)
      echo 'macOS CLI 单文件包：给 Homebrew / 脚本安装用'
      ;;
    *-windows-x86_64.zip)
      echo 'Windows x64 便携包：解压即用，免安装'
      ;;
    *-windows-x86_64-setup.exe)
      echo 'Windows x64 安装器：按用户装到 `%LOCALAPPDATA%`，不需要管理员，中英双语向导'
      ;;
    *-windows-x86_64.msi)
      echo 'Windows x64 MSI：按机器装到 `%ProgramFiles%` + 系统 PATH，`/qn` 静默，装/卸需要管理员'
      ;;
    *-windows-arm64.zip)
      echo 'Windows ARM64 便携包：解压即用，免安装'
      ;;
    *-windows-arm64-setup.exe)
      echo 'Windows ARM64 安装器：按用户安装，中英双语向导'
      ;;
    *-windows-arm64.msi)
      echo 'Windows ARM64 MSI：按机器安装，`/qn` 静默，装/卸需要管理员'
      ;;
    *)
      echo '未在模板里登记，见 README 的「安装与卸载」'
      ;;
  esac
}

has_arm64=0
if [ -n "$asset_dir" ]; then
  for f in "$asset_dir"/*arm64*; do
    [ -e "$f" ] && { has_arm64=1; break; }
  done
fi

# 带 `-` 的版本是预发布（与 release.yml 里 --prerelease 的判定同一条规则）。
if [ "${version#*-}" != "$version" ]; then
  printf '> 预发布 `%s`：接口与产物可能继续变动，正式版以 `%s` 为准。\n\n' \
    "$version" "${version%%-*}"
fi

cat <<EOF
## 变更

$section

## 下载
EOF

if [ -n "$asset_dir" ]; then
  [ -d "$asset_dir" ] || die "产物目录不存在：$asset_dir"
  rows=0
  printf '\n| 文件 | 说明 |\n|---|---|\n'
  for f in "$asset_dir"/*; do
    [ -f "$f" ] || continue
    name=$(basename "$f")
    [ "$name" = 'SHA256SUMS.txt' ] && continue
    printf '| `%s` | %s |\n' "$name" "$(describe_asset "$name")"
    rows=$((rows + 1))
  done
  [ "$rows" -gt 0 ] || die "产物目录里没有任何文件：$asset_dir"
  printf '\n'
else
  printf '\n产物清单以 Release 附件为准。\n\n'
fi

cat <<'EOF'
## 校验

`SHA256SUMS.txt` 覆盖上面所有文件：

```bash
sha256sum -c SHA256SUMS.txt              # macOS / Linux
Get-FileHash -Algorithm SHA256 <文件>      # Windows，与 SHA256SUMS.txt 对照
```

## 未签名产物

没有代码签名证书，第一次运行会被系统拦一下：

- **Windows**：SmartScreen 提示「未知发布者」→「更多信息」→「仍要运行」。
- **macOS**：ad-hoc 签名、未公证 → 右键「打开」，或
  `xattr -dr com.apple.quarantine /Applications/sadp-discover.app`。

签名只证明发布者身份，完整性以 `SHA256SUMS.txt` 与 HTTPS 下载渠道为准。
EOF

if [ "$has_arm64" = 1 ]; then
  cat <<'EOF'

## ARM64

`-windows-arm64` 的产物在 x64 主机上交叉编译：CI 校验了 PE 架构（`0xAA64`）与打包布局，
没有 ARM64 真机的运行冒烟。
EOF
fi
