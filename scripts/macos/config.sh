#!/usr/bin/env bash
# macOS 打包流水线的共享配置与工具函数（被 source，不单独执行）。
#
# 这一层只回答「事实」：版本号从哪来、产物放哪、有哪些 target、包与二进制怎么对应。
# 「动作」拆到 build / sign / notarize / package 四个脚本里，彼此不互相调用：
#   - 换签名方式不会碰到构建逻辑；
#   - 换输出布局只改这个文件；
#   - 每个脚本都能单独跑（对已有产物补签名、重打包），不需要从头构建。

set -euo pipefail

# --- 单一事实来源 ---------------------------------------------------------
# 包名与产出的二进制名只在这里出现；新增 GUI 打包时补一行 case 即可，
# build.sh / package.sh 不需要跟着改。
SADP_PACKAGES_DEFAULT="sadp-cli"
SADP_TARGETS_DEFAULT="aarch64-apple-darwin x86_64-apple-darwin"

sadp_bin_for_pkg() {
  case "$1" in
    sadp-cli) printf 'sadp-discover\n' ;;
    sadp-gui) printf 'sadp-discover-gui\n' ;;
    *) return 1 ;;
  esac
}

# --- 输出 -----------------------------------------------------------------
# 注意：变量紧挨中文标点时必须写成 ${var}。UTF-8 locale 下 bash 会把多字节字符
# 并进变量名，裸写 $triple（ 会得到 "triple<乱码>: unbound variable"。
sadp_note() { printf '==> %s\n' "$*"; }
sadp_warn() { printf 'warning: %s\n' "$*" >&2; }
sadp_die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

sadp_require_cmd() {
  command -v "$1" >/dev/null 2>&1 || sadp_die "缺少命令 $1，请安装 Xcode Command Line Tools（xcode-select --install）"
}

# --- 事实查询 -------------------------------------------------------------
# cargo 自己管 target 目录：尊重 CARGO_TARGET_DIR，也尊重 .cargo/config 里的设置。
sadp_target_dir() {
  if [[ -n ${CARGO_TARGET_DIR:-} ]]; then
    printf '%s\n' "$CARGO_TARGET_DIR"
    return 0
  fi
  local meta dir
  meta=$(cargo metadata --no-deps --format-version 1 2>/dev/null) || return 1
  dir=$(printf '%s\n' "$meta" | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p' | head -1)
  [[ -n $dir ]] || return 1
  printf '%s\n' "$dir"
}

# 版本号取自 cargo 本身，而不是逐行扫描 Cargo.toml 的文本格式。
sadp_manifest_version() {
  local out
  out=$(cargo pkgid -p "${SADP_CLI_PKG:-sadp-cli}" 2>/dev/null) || return 1
  out=$(printf '%s\n' "$out" | sed -e 's/.*[#@]//')
  [[ $out =~ ^[0-9]+\.[0-9]+\.[0-9]+ ]] || return 1
  printf '%s\n' "$out"
}

# cargo 的 profile 名与目录名不一致（dev -> debug），在这里统一掉。
sadp_profile_dir() {
  case "$1" in
    dev) printf 'debug\n' ;;
    *) printf '%s\n' "$1" ;;
  esac
}

sadp_bin_path() {
  printf '%s/%s/%s/%s\n' "$SADP_TARGET_DIR" "$1" "$SADP_PROFILE_DIR" "$2"
}

sadp_tar_name() {
  printf 'sadp-discover-%s-macos-universal.tar.gz\n' "$SADP_VERSION"
}

# target 的 rust-std 是否真的存在。缺了就直接给出可操作命令，
# 而不是让 cargo 抛一句难懂的 "can't find crate for core"。
sadp_require_target() {
  local triple=$1 libdir
  libdir=$(rustc --print target-libdir --target "$triple" 2>/dev/null) || true
  if [[ -n $libdir && -d $libdir ]]; then
    return 0
  fi
  if [[ ${SADP_INSTALL_TARGETS:-0} == 1 ]]; then
    sadp_note "安装 target $triple"
    rustup target add "$triple" || sadp_die "rustup target add $triple 失败"
    return 0
  fi
  sadp_die "target $triple 的 rust-std 未安装。先运行：rustup target add ${triple}（或设 SADP_INSTALL_TARGETS=1 让脚本代装）"
}

# --- 初始化 ---------------------------------------------------------------
# 只读环境、不产生副作用（不 mkdir、不 touch），这样在 CI 的只读 checkout 上也能安全调用。
sadp_init() {
  local here
  here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
  SADP_ROOT=$(cd -- "$here/../.." && pwd)
  cd "$SADP_ROOT"

  SADP_CLI_PKG=sadp-cli
  # 两个 slice 必须声明同一个最低系统版本：x86_64 的 rustc 默认是 10.12，
  # 而 aarch64 是 11.0，不统一会让 Intel slice 的 LC_BUILD_VERSION 写着 10.12，
  # 与 Info.plist 的 LSMinimumSystemVersion 对不上。
  export MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-11.0}
  SADP_PROFILE=${PROFILE:-release}
  SADP_PROFILE_DIR=$(sadp_profile_dir "$SADP_PROFILE")
  SADP_TARGETS=${TARGETS:-$SADP_TARGETS_DEFAULT}
  SADP_PACKAGES=${PACKAGES:-$SADP_PACKAGES_DEFAULT}
  SADP_DIST=${DIST:-${OUT:-$SADP_ROOT/dist}}

  SADP_TARGET_DIR=$(sadp_target_dir) ||
    sadp_die "无法确定 cargo 的 target 目录（cargo metadata 失败）"

  if [[ -n ${VERSION:-} ]]; then
    SADP_VERSION=$VERSION
    # VERSION 只影响产物命名，不会改二进制里内嵌的版本（那是 Cargo.toml 决定的）。
    # 两者不一致时提醒一次，避免发布包叫 1.0.0、--version 却报 0.1.0。
    local manifest
    manifest=$(sadp_manifest_version) || manifest=""
    if [[ -n $manifest && $VERSION != "$manifest" && -z ${SADP_VERSION_WARNED:-} ]]; then
      sadp_warn "VERSION=${VERSION} 只影响产物命名；二进制内嵌版本仍是 ${manifest}（由 Cargo.toml 决定）"
      export SADP_VERSION_WARNED=1
    fi
  else
    SADP_VERSION=$(sadp_manifest_version) ||
      sadp_die "无法从 cargo 读取版本号，请显式设置 VERSION=<x.y.z>"
  fi
}
