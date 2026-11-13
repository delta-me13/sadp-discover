#!/usr/bin/env bash
# 只做公证与 staple：输入必须是已签名、已存在的产物，不构建、不打包。
#
# 用法：
#   NOTARY_PROFILE=sadp-notary bash scripts/macos/notarize.sh dist/sadp-discover
#   ASC_KEY_PATH=AuthKey_XX.p8 ASC_KEY_ID=XX ASC_ISSUER=... bash scripts/macos/notarize.sh dist/App.app
#
# 两种凭据二选一：钥匙串 profile（本地）或 App Store Connect API Key（CI）。
# 未提供凭据时跳过，保持「默认不公证」的行为。
set -euo pipefail

source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/config.sh"
sadp_init

(( $# > 0 )) || sadp_die "用法: notarize.sh <artifact>..."

creds=()
if [[ -n ${NOTARY_PROFILE:-} ]]; then
  creds=(--keychain-profile "$NOTARY_PROFILE")
elif [[ -n ${ASC_KEY_PATH:-} && -n ${ASC_KEY_ID:-} && -n ${ASC_ISSUER:-} ]]; then
  creds=(--key "$ASC_KEY_PATH" --key-id "$ASC_KEY_ID" --issuer "$ASC_ISSUER")
else
  sadp_note "未设置 NOTARY_PROFILE 或 ASC_KEY_*，跳过公证"
  exit 0
fi

sadp_require_cmd xcrun
mkdir -p "$SADP_DIST"
tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/sadp-notarize.XXXXXX")
trap 'rm -rf "$tmpdir"' EXIT

for artifact in "$@"; do
  [[ -e $artifact ]] || sadp_die "不存在：$artifact"

  # notarytool 只收 zip / dmg / pkg；.app 与裸二进制都要先压成 zip。
  case "$artifact" in
    *.app|*.dmg|*.pkg) submit=$artifact ;;
    *)
      submit="$tmpdir/$(basename "$artifact").zip"
      COPYFILE_DISABLE=1 ditto -c -k --keepParent "$artifact" "$submit"
      ;;
  esac

  sadp_note "提交公证 $(basename "$artifact")"
  output=$(xcrun notarytool submit "$submit" "${creds[@]}" --wait 2>&1) ||
    sadp_die "notarytool 调用失败：$artifact"
  printf '%s\n' "$output"
  printf '%s\n' "$output" | grep -q "status: Accepted" ||
    sadp_die "公证未通过：${artifact}（查看上面的 status / log）"

  case "$artifact" in
    *.app|*.dmg|*.pkg)
      sadp_note "staple $(basename "$artifact")"
      xcrun stapler staple "$artifact"
      ;;
    *)
      # 裸二进制没有可 staple 的容器：Gatekeeper 走在线查票。
      sadp_note "裸二进制无需 staple（Gatekeeper 在线校验 CDHash）"
      ;;
  esac
done
