#!/usr/bin/env bash
# 构建 macOS 通用二进制（arm64 + x86_64）。
#
# 必须在 macOS 上运行：lipo / codesign / notarytool 都是 Xcode 命令行工具的一部分。
#
# 用法：
#   ./scripts/build-macos-universal.sh
#   VERSION=0.2.0 ./scripts/build-macos-universal.sh
#   CODESIGN_IDENTITY="Developer ID Application: Foo (TEAMID)" ./scripts/build-macos-universal.sh
#
# 环境变量：
#   VERSION             版本号，默认从 Cargo.toml 读取
#   OUT                 输出目录，默认 dist
#   CODESIGN_IDENTITY   设置后对产物签名（hardened runtime + 时间戳）
#   NOTARY_PROFILE      设置后提交公证（需先 xcrun notarytool store-credentials）
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "error: 该脚本只能在 macOS 上运行（需要 lipo）" >&2
  exit 1
fi

VERSION="${VERSION:-$(awk -F'"' '/^version = /{print $2; exit}' Cargo.toml)}"
OUT="${OUT:-dist}"
TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
BIN="$OUT/sadp-discover"

echo "==> building sadp-discover $VERSION (universal)"
mkdir -p "$OUT"

for target in "${TARGETS[@]}"; do
  rustup target add "$target" >/dev/null 2>&1 || true
  echo "==> cargo build --release --target $target"
  cargo build --release -p sadp-cli --target "$target"
done

echo "==> lipo"
lipo -create \
  "target/aarch64-apple-darwin/release/sadp-discover" \
  "target/x86_64-apple-darwin/release/sadp-discover" \
  -output "$BIN"
chmod +x "$BIN"
lipo -info "$BIN"

if [[ -n "${CODESIGN_IDENTITY:-}" ]]; then
  echo "==> codesign ($CODESIGN_IDENTITY)"
  codesign --force --options runtime --timestamp \
    --sign "$CODESIGN_IDENTITY" "$BIN"
  codesign --verify --verbose=2 "$BIN"
else
  echo "==> 跳过签名（未设置 CODESIGN_IDENTITY）"
fi

ARCHIVE="$OUT/sadp-discover-$VERSION-macos-universal.tar.gz"
tar -czf "$ARCHIVE" -C "$OUT" sadp-discover
echo "==> wrote $ARCHIVE"

if [[ -n "${NOTARY_PROFILE:-}" ]]; then
  echo "==> notarize"
  # notarytool 接受 zip；注意 stapler 只对 .app/.dmg/.pkg 有效，
  # 裸命令行二进制分发时 Gatekeeper 走在线校验，不需要 staple。
  ditto -c -k --keepParent "$BIN" "$OUT/sadp-discover-notarize.zip"
  xcrun notarytool submit "$OUT/sadp-discover-notarize.zip" \
    --keychain-profile "$NOTARY_PROFILE" --wait
  rm -f "$OUT/sadp-discover-notarize.zip"
else
  echo "==> 跳过公证（未设置 NOTARY_PROFILE）"
fi

echo "==> done"
