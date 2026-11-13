# macOS 适配（二阶段）

> 状态：代码已就绪，两个 Apple target 的交叉编译检查通过。Gate 2 已在 macOS 26 / Apple Silicon
> 真机通过 11/12 项（第 12 项 24h 连续性因电池供电暂缓）；macOS 14/15 与 Intel 机型仍未覆盖。
> 证据与结论见 [.gate2/gate2-report.md](../.gate2/gate2-report.md)。

## 1. 平台差异一览

| 项目 | macOS 上的处理 | 需要平台专用代码吗 |
|---|---|---|
| 网卡枚举 | `if-addrs` 走 `getifaddrs` + `if_nametoindex`，`index` 字段可用 | 否 |
| 组播出口 | 每地址族一个 socket，发送前 `IP_MULTICAST_IF`（v4 用接口地址，v6 用 ifindex） | 否 |
| 子网广播 | 用接口的子网定向广播地址，`SO_BROADCAST` 已开 | 否 |
| 绑定 37020 | 按配置绑定；被占用则退临时端口并提示 | 否 |
| 链路本地 IPv6 | 依赖 `recvfrom` 带回的 `scope_id`；缺失时用来源网卡 ifindex 补 | 否 |
| 剪贴板 | `pbcopy` | 是（`clipboard.rs`，已有） |
| 日志/配置路径 | `~/Library/Application Support/sadp-discover/`（`dirs` 自动处理） | 否 |
| 终端 | crossterm / ratatui 原生支持 | 否 |

### 为什么不需要 `IP_BOUND_IF`

架构文档最初把 `IP_BOUND_IF` 列为 macOS 风险项，理由是 socket2 的 `bind_device` 只在 Linux 可用。
实际实现没有采用「把 socket 绑到具体网卡」的方案：**绑定到具体地址的 socket 收不到广播与组播**，
而我们把出口控制放在发送侧的 `IP_MULTICAST_IF`（v4 传接口地址、v6 传 ifindex），
接收侧始终是通配地址。这个方案在两个平台上语义一致，因此不需要 `IP_BOUND_IF`，
也不需要 `libc` 的 `setsockopt` 平台分支。

如果将来需要「只从某块网卡收包」，再考虑 `IP_BOUND_IF`，并接受它会改变广播/组播的接收行为。

## 2. 构建

### 交叉编译检查（可在 Windows/Linux 上做）

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo check --workspace --target aarch64-apple-darwin
cargo check --workspace --target x86_64-apple-darwin
```

`cargo check` 不链接，因此不需要 macOS SDK；它能验证所有平台条件编译与依赖是否可用。

### 单架构构建（在 Mac 上）

```bash
cargo build --release -p sadp-cli --target aarch64-apple-darwin   # Apple Silicon
cargo build --release -p sadp-cli --target x86_64-apple-darwin    # Intel
```

### 通用二进制

```bash
bash scripts/build-macos-universal.sh
# 产物：dist/sadp-discover-<version>-macos-universal.tar.gz
```

脚本会：安装两个 target → 分别构建 → `lipo -create` 合成 → 校验架构 →
（可选）`codesign` →（可选）`notarytool` 公证。

## 3. 权限

### 本地网络权限（macOS 15+）

macOS 15 起，访问局域网需要用户授权：

- **命令行**：权限授予**终端 App**（Terminal / iTerm / VS Code），`sadp-discover` 继承终端的授权；
- 首次发包时系统弹窗，拒绝后不会再有提示；
- 恢复路径：**系统设置 → 隐私与安全性 → 本地网络**，打开对应终端；
- 部分版本需要重启终端进程才生效。

**症状**：程序正常启动、正常绑定端口，但一个响应都收不到，`sadp-discover` 会打印：

```
没有收到任何 SADP 响应。macOS 15+ 需要在「系统设置 → 隐私与安全性 → 本地网络」中允许当前终端。
```

这条提示只在**一个探测包都没发出去**时触发（`error.hint.macos_local_network`），
即 `Transport::sent_packets() == 0`——权限被拒时发包会直接失败，这才是可靠信号。
如果探测包发出去了、只是没人回应（空网段、设备离线、跨 VLAN、防火墙丢包），
程序给出的是通用提示 `error.hint.no_sadp_packets`，不会把锅甩给本地网络权限。

### 应用防火墙

macOS 的入站防火墙按**进程**放行。如果开启了「防火墙 → 阻止所有传入连接」，
需要在「防火墙选项」里允许终端；UDP 37020 的组播/广播回包才能进来。

### 自查命令

```bash
sadp-discover interfaces            # 看网卡、ifindex、组播加入结果
sadp-discover -vv --save-raw ./raw  # 原始报文 + 解析失败原因
tail -f ~/Library/Application\ Support/sadp-discover/logs/sadp-discover.log.*
```

## 4. 签名与公证

仅 Developer ID 分发需要；本地自用可跳过。

```bash
# 1) 签名（hardened runtime + 时间戳，公证的前提）
codesign --force --options runtime --timestamp \
  --sign "Developer ID Application: Your Name (TEAMID)" dist/sadp-discover

# 2) 公证（凭据先存进钥匙串）
xcrun notarytool store-credentials sadp-notary \
  --apple-id you@example.com --team-id TEAMID --password app-specific-password

ditto -c -k --keepParent dist/sadp-discover dist/notarize.zip
xcrun notarytool submit dist/notarize.zip --keychain-profile sadp-notary --wait
```

说明：

- `stapler` 只对 `.app` / `.dmg` / `.pkg` 生效；**裸命令行二进制无法 staple**，
  Gatekeeper 通过在线查询公证记录放行，首次运行需要联网。
- 从浏览器/邮件下载的压缩包会带 `com.apple.quarantine`，未公证时用户可手动解除：
  `xattr -d com.apple.quarantine ./sadp-discover`。
- 三阶段 GUI 打包成 `.app` 后，需要在 `Info.plist` 里加
  `NSLocalNetworkUsageDescription`（描述为什么要访问本地网络），否则 macOS 15 上不会弹出授权框。

## 5. Gate 2 验收清单

需要在 macOS 14 与 15、Apple Silicon 与 Intel 各一台机器上执行。

| # | 检查项 | 命令 / 操作 | 预期 |
|---|---|---|---|
| 1 | 冷启动可用 | `./sadp-discover --version` | 打印版本，无 Gatekeeper 拦截 |
| 2 | 网卡枚举 | `./sadp-discover interfaces` | 列出 `en0` 等，ifindex 非 0，组播 `v4✓ v6✓` |
| 3 | 默认发现 | `./sadp-discover` | TUI 出现在真实设备，3 秒内出首屏 |
| 4 | 双栈去重 | 设备同时有 v4/v6 | 只一行，`via=both` |
| 5 | 仅 IPv6 | `--family v6` | 只发 v6，不因 v4 失败退出 |
| 6 | 中英切换 | TUI 按 `L` | 立即切换，无截断（80×24） |
| 7 | 导出/复制 | TUI 按 `e` / `c` | 生成 JSON；`pbcopy` 生效，可粘贴 |
| 8 | 本地网络权限 | 首次运行 | 弹窗；允许后有结果；拒绝时出现权限提示 |
| 9 | 路由/多网卡 | 同时开 Wi-Fi 与 USB 网卡 | 两块网卡都发，不漏设备 |
| 10 | 与系统工具共存 | 同时开其他用 37020 的程序 | 绑定失败退临时端口并提示，不崩溃 |
| 11 | 日志路径 | `-vv` 运行后查看 | `~/Library/Application Support/sadp-discover/logs/` 有日志 |
| 12 | 连续性 | `--repeat 1000` 跑 24h | 内存/句柄稳定 |

## 6. 已知限制

- 本仓库的 CI 在 `macos-15` 与 `macos-latest`（Apple Silicon）上跑构建与单元测试，并额外交叉检查 `x86_64-apple-darwin`；Intel 的运行期行为**仍需真机**。`macos-14` 镜像已于 2026-10 被 GitHub 退役，任务会被直接取消。
- 组播相关的端到端用例（`sadp-fake-device -- --ignored`）**只跑在 Windows runner 上**：macOS 15+ 的本地网络权限在托管 runner 上无人授权，回包收不到；真机 macOS 上该用例可通过，属于环境限制。
- 官方 SADP 工具没有 macOS 版，因此不存在「抢 37020」的兼容性场景；但仍保留端口回退逻辑。
- macOS 上 Wi-Fi 与有线同时接入同一网段时，同一设备可能从两个接口各回一包；
  去重逻辑会合并为一行，`seen_on` 会列出两个网卡。
- 未适配 App Sandbox；若将来上架 Mac App Store，需要
  `com.apple.developer.networking.multicast` 权限与相应 entitlement。
