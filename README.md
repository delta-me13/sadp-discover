# sadp-discover

跨平台的海康（Hikvision）SADP 设备发现工具：**只读**、单文件、中英双语、支持 IPv4/IPv6 双栈。

```text
 sadp-discover   网卡: WLAN   第 3 轮   2 台设备   1.2s
 IP 地址                    MAC                型号            来源   激活
 192.168.1.64               a4:14:37:5f:4c:6d  DS-2CD2T47G2-L  both   ✓
 192.168.1.65               44:19:b6:aa:bb:cc  DS-7608N-K2     v4     ✓
```

## 当前状态

一阶段（Windows CLI/TUI）与二阶段（macOS 适配）已完成，三阶段的 egui 图形界面已可用。
验收清单与进度见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) 与 [docs/MACOS.md](docs/MACOS.md)。

- ✅ **CLI / TUI**：双栈发现、跨栈去重、JSON·CSV、中英双语、日志落盘
- ✅ **macOS**：Gate 2 清单 11/12（24h 连续性待跑），通用二进制脚本
- ✅ **GUI**：`sadp-discover-gui`，表格 / 详情 / 过滤 / 排序 / 复制 / 语言切换 / 日志目录；设置面板可多选网卡、地址族、窗口/周期/端口；导出走系统「另存为」，深/浅主题可切换；整行单击看详情，双击 IP 开设备 Web 后台
- ✅ **Windows 分发**：Inno 安装器（中英双语向导）、MSI（GPO/Intune 静默部署）、便携 zip；x64 与 ARM64（x64 主机交叉编译）各一份
- ✅ **CI**：`dev`（test + e2e + Windows x64/ARM64 打包自检 + 两种安装器的装/卸冒烟）与 `release`（仅 `v*` tag：版本闸门 → Windows `zip`+`setup.exe`+`MSI`（x64/ARM64） / macOS universal `.app`+`.pkg`+`.dmg`+`tar.gz` → GitHub Release）
- 🟡 **真机（Windows）**：Win11 + 球机 DS-2DE4423DW-D/GLT/XM 已通过默认发现、多网卡/VPN 隔离、端口占用回退与双语检查；IPC/NVR、Win10 与防火墙待补，详见 [.gate1/gate1-report.md](.gate1/gate1-report.md)
- ⏳ **待硬件**：IPC/NVR 真机、Win10 主机、IPv6 双栈真机；IPv6 组播地址抓包确认

> **IPv6 说明（重要）**：SADP 的 IPv6 探测组播地址没有公开资料。当前实现按候选组
> `ff02::1` 发送，可用 `--v6-mcast` 覆盖。若抓包证明设备不响应 IPv6 探测，
> 程序仍会展示设备上报的 IPv6 地址（经 IPv4 发现时可见），并如实报告 limitations，
> 不会伪造能力。

## 构建

需要 Rust 1.90+。

```bash
# Windows（MSVC）：只出 exe
cargo build --release -p sadp-cli
# 产物：target/release/sadp-discover.exe

# Windows 分发包（zip + Inno 安装器 + MSI，需在 Windows 上运行）
# 依赖：Inno Setup 6（中文向导用仓库自带的 languages\ChineseSimplified.isl）、
#       WiX Toolset 3.14（MSI）；GitHub 的 windows-latest 镜像两者都预装。
pwsh -File scripts/build-windows.ps1
# 产物：dist/sadp-discover-<version>-windows-x86_64.zip
#       dist/windows/sadp-discover-<version>-windows-x86_64-setup.exe
#       dist/windows/sadp-discover-<version>-windows-x86_64.msi
# 只要 zip：pwsh -File scripts/build-windows.ps1 -SkipInstaller -SkipMsi

# Windows ARM64（在 x64 主机上交叉编译）
# 需要 VS 组件 Microsoft.VisualStudio.Component.VC.Tools.ARM64；
# 没有该组件时可设 SADP_XWIN_SPLAT 指向 xwin splat 目录（见 docs/WINDOWS-PACKAGING.md）。
pwsh -File scripts/build-windows.ps1 -Arch arm64
# 产物后缀换成 -windows-arm64（zip / setup.exe / msi 各一份）

# macOS 通用二进制（arm64 + x86_64，需在 Mac 上运行）
bash scripts/build-macos-universal.sh
# 产物：dist/sadp-discover-<version>-macos-universal.tar.gz
# 只构建、不签名不打包：bash scripts/build-macos-universal.sh --build-only
# 单架构快速验证（缺另一个 target 时）：TARGETS=aarch64-apple-darwin bash scripts/macos/build.sh

# macOS 分发包（.app + .pkg，CLI 同时装到 /usr/local/bin）
PACKAGES=sadp-gui TARGETS="aarch64-apple-darwin x86_64-apple-darwin" \
  bash scripts/macos/build.sh          # GUI 通用二进制
bash scripts/macos/bundle.sh && bash scripts/macos/pkg.sh && bash scripts/macos/dmg.sh
# 产物：dist/sadp-discover.app、dist/sadp-discover-<version>-macos.pkg、
#       dist/sadp-discover-<version>-macos.dmg
# 默认 ad-hoc 签名；正式分发需 CODESIGN_IDENTITY + notarize.sh

# 图形界面（三阶段，egui）
cargo build --release -p sadp-gui
# 产物：target/release/sadp-discover-gui
```

macOS 的权限、签名与公证说明见 [docs/MACOS.md](docs/MACOS.md)；
分发用 `.pkg`（装 `/Applications/sadp-discover.app` + `/usr/local/bin/sadp-discover`）的方案见
[docs/MACOS-PACKAGING.md](docs/MACOS-PACKAGING.md)。
Windows 的产物布局、安装器的注册表/PATH 行为、exe 内嵌资源见
[docs/WINDOWS-PACKAGING.md](docs/WINDOWS-PACKAGING.md)。

## 发布

CI 分成两条工作流，互不干扰：

| 工作流 | 触发 | 做什么 |
|---|---|---|
| `dev` | push `main` / PR | `test`（fmt、clippy、Intel 交叉检查、单元测试）+ `e2e`（Windows 假设备）；push 到 `main` 时额外跑 `package`（Windows 打包 + 装/卸一遍），不产出对外发布的产物 |
| `release` | **仅 `v*` tag**；另可手动试跑 | 版本闸门 → cargo-deny → Windows `zip`+`setup.exe` / macOS universal `.app`+`.pkg`+`.dmg`+`tar.gz` → GitHub Release；试跑只构建并上传 artifact，不创建 Release |

```bash
git tag v0.1.0 && git push origin v0.1.0
```

tag 必须等于 `cargo pkgid -p sadp-cli` 的版本，否则 `release` 在版本闸门就失败；
带连字符的 tag（`v1.2.3-rc1`）自动按 prerelease 发布。

不想先打 tag、或还没有签名证书时，可以先手动试跑同一条流水线：

```bash
gh workflow run release.yml -f version=0.1.0-rc.1 -f dry_run=true
gh run watch
```

试跑同样过版本闸门（`version` 必须等于 workspace 版本），产物在 run 页面的 Artifacts 里，
不会出现在 Releases；没有 secrets 时按「Windows 未签名 + macOS ad-hoc」构建。

Release 页面不是手写的，`publish` 用 `scripts/release/notes.sh` 生成正文：

- 标题固定 `sadp-discover <version>`（`<version>` 就是 `cargo pkgid -p sadp-cli` 的值）；
- 正文 = `CHANGELOG.md` 里 `## [<version>]` 段落 + 按真实附件生成的下载表 + 校验 / 未签名说明；
- 没有该版本段落时回退到 GitHub 自动 notes，并在 run 里留一条 warning；
- 带 `-` 的 tag 自动勾 prerelease；附件固定是 9 个产物 + `SHA256SUMS.txt`。

想先看正文长什么样，在本地跑（第二个参数给产物目录，下载表就按真实文件生成）：

```bash
bash scripts/release/notes.sh 0.1.0-rc.1
```

## 安装与卸载

### Windows

| 产物 | 装什么 | 适合 |
|---|---|---|
| `…-setup.exe` | `%LOCALAPPDATA%\Programs\sadp-discover` 下的两个 exe，开始菜单快捷方式，CLI 加进用户 PATH；向导为中英双语 | **推荐**：不需要管理员权限，卸载走「设置 → 应用」 |
| `…-windows-x86_64.msi` | `%ProgramFiles%\sadp-discover`，两个 exe + 开始菜单快捷方式，CLI 加进**系统** PATH | 企业批量部署（GPO / Intune / SCCM），`/qn` 静默；安装/卸载需要管理员 |
| `…-windows-x86_64.zip` | 同样这几个文件，解压即用 | 便携、免安装、丢进 U 盘 |
| `…-windows-arm64.zip` / `-arm64-setup.exe` / `-arm64.msi` | 同上，ARM64 原生 | Windows on ARM（x64 仿真包也能跑，但原生更快） |

```powershell
# 安装器：默认就装到当前用户，不弹 UAC；/VERYSILENT 用于脚本与批量部署
.\sadp-discover-<version>-windows-x86_64-setup.exe
.\sadp-discover-<version>-windows-x86_64-setup.exe /VERYSILENT /SUPPRESSMSGBOXES

# MSI：按机器静默安装 / 卸载（在管理员终端里执行，企业部署用）
msiexec /i .\sadp-discover-<version>-windows-x86_64.msi /qn
msiexec /x .\sadp-discover-<version>-windows-x86_64.msi /qn

# 便携包
Expand-Archive sadp-discover-<version>-windows-x86_64.zip -DestinationPath .
.\sadp-discover-<version>-windows-x86_64\sadp-discover-gui.exe
```

> **两种安装器装在不同位置**：Inno 是「按用户」（`%LOCALAPPDATA%` + 用户 PATH，不需要管理员），
> MSI 是「按机器」（`%ProgramFiles%` + 系统 PATH，需要管理员）。同一台机器上建议只装一种；
> 两者开始菜单项同名。ARM64 包只能装在 ARM64 Windows 上。

装完**新开一个命令行窗口**才看得到 PATH 里的 `sadp-discover`。首次扫描时 Windows
防火墙会问是否允许访问网络，请勾选「专用网络」并允许——拒绝之后设备回包会被丢掉，
界面上就是「一台设备都没有」。

卸载：`设置 → 应用 → 已安装的应用` 里找到 sadp-discover，或运行安装目录下的
`unins000.exe`；PATH 里的条目会一并删除。

### macOS

macOS 有两个产物，按需求选一个：

| 产物 | 装什么 | 适合 |
|---|---|---|
| `.pkg` | `/Applications/sadp-discover.app` + `/usr/local/bin/sadp-discover` | **推荐**：双击是 GUI，终端里直接有命令 |
| `.dmg` | 只有 `.app`（拖进 Applications 完成安装） | 只需要 GUI、不想动系统目录 |

```bash
# .pkg：一次装好 app 与 PATH 里的 CLI（带安装向导：欢迎页、说明、许可协议、macOS 11+ 检查）
sudo installer -pkg sadp-discover-<version>-macos.pkg -target /

# .dmg：打开后把 sadp-discover.app 拖进 Applications
open sadp-discover-<version>-macos.dmg
```

`.dmg` **不会**把 `sadp-discover` 放进 PATH。想从 dmg 装完又要在终端里用，二选一：

```bash
# 软链到 app 内的那份 CLI（与 app 同版本、同一个可执行文件）
sudo ln -sf /Applications/sadp-discover.app/Contents/MacOS/sadp-discover /usr/local/bin/sadp-discover

# 或者用 tar.gz 里独立的那份
tar -xzf sadp-discover-<version>-macos-universal.tar.gz -C /usr/local/bin
```

卸载（`.pkg` 没有 uninstall 钩子，三条命令足够）：

```bash
sudo rm -rf /Applications/sadp-discover.app
sudo rm -f  /usr/local/bin/sadp-discover
rm -rf ~/Library/Application\ Support/sadp-discover   # GUI 的日志目录
```

macOS 15+ 的「本地网络」授权是记在系统设置里的，不随 app 删除而消失；
要让新装的版本重新弹一次授权，去 **系统设置 → 隐私与安全性 → 本地网络** 里删掉
`sadp-discover` 这一条。

### 代码签名（当前未签名）

仓库目前**没有** Windows Authenticode 证书，也没有 Apple Developer ID 证书，所以
CI 产物是未签名的：Windows 会报 SmartScreen「未知发布者」，macOS 的 `.app` 只有
ad-hoc 签名，Gatekeeper 会拦首次打开。**这是已知状态，不是包损坏。**

- **Windows**：SmartScreen 弹窗 →「更多信息」→「仍要运行」；从 zip 解压的 exe 若被
  标记为「来自 Internet」，可先 `Unblock-File .\<文件名>`。
- **macOS**：右键点 `.app` →「打开」→ 再确认一次；被拦的 `.pkg` 去
  「系统设置 → 隐私与安全性」点「仍要打开」。命令行可用
  `xattr -dr com.apple.quarantine /Applications/sadp-discover.app` 清隔离属性。
- **校验下载**：`Get-FileHash -Algorithm SHA256 <文件>`（Windows）或
  `shasum -a 256 <文件>`（macOS），与 Release 里的 `SHA256SUMS.txt` 对照。
  ad-hoc 签名只保证包内自洽，**不证明发布者身份**，SHA-256 与 HTTPS 下载渠道才是
  完整性依据。

接入证书后流水线会在不改打包脚本的前提下自动签名/公证（secrets 名见
[docs/MACOS-PACKAGING.md](docs/MACOS-PACKAGING.md) §8 与
[docs/WINDOWS-PACKAGING.md](docs/WINDOWS-PACKAGING.md) §6）。

## 用法

```bash
sadp-discover                       # 交互式 TUI，单轮 3 秒
sadp-discover --repeat 1000         # 每秒持续扫描
sadp-discover --json                # 机器可读（schema sadp.discover/v1）
sadp-discover --csv -o out.csv      # 导出 CSV
sadp-discover --family v6 --no-tui  # 仅 IPv6，纯文本
sadp-discover --passive             # 只监听设备主动通告
sadp-discover interfaces            # 网卡列表 + 每个地址族的组播加入结果
sadp-discover completions powershell
sadp-discover logs                  # 打开日志文件夹（资源管理器 / Finder）

# 图形界面：同一套扫描引擎与事件契约，窗口里看表格/详情
sadp-discover-gui                    # 启动即扫描，每秒一轮；单击行看详情，双击 IP 开 Web 后台
sadp-discover-gui --lang en-US       # 指定界面语言
sadp-discover-gui --local-port 0     # 与官方 SADP 同机时避开 37020
```

常用选项：

| 选项 | 说明 |
|---|---|
| `--timeout <MS>` | 单轮收包窗口，默认 3000 |
| `--repeat <MS>` | 每 N 毫秒开始新一轮；短于 `--timeout` 时自动收窄窗口 |
| `--port <PORT>` | 探测目标端口，默认 37020 |
| `--local-port <PORT>` | 本地绑定端口；`0` = 临时端口（与官方 SADP 同机运行时避免抢端口） |
| `--interface <NAME>` | 只用指定网卡（名称/索引/地址，逗号分隔） |
| `--family v4\|v6\|both` | 地址族 |
| `--v6-mcast <ADDR>` | IPv6 组播组，默认 `ff02::1` |
| `--stale <SECS>` | 超过 N 秒未再出现的设备从结果移除 |
| `--save-raw <DIR>` | 原始报文落盘，便于排障 |
| `--lang en-US\|zh-CN` | 界面语言 |

TUI 快捷键：`S` 立即扫描（保留结果）、`空格` 暂停/继续、`R` 清空结果并重扫、`/` 过滤、`E` 导出、`C` 复制、`T` 换排序列、`D` 换排序方向、`L` 切换语言、`O` 打开日志文件夹、`Q` 退出；字母键大小写不敏感。

### 退出码

| 码 | 含义 |
|---|---|
| 0 | 发现设备 |
| 1 | 未发现设备 |
| 2 | 参数错误或网络错误 |
| 3 | 权限被拒（防火墙 / 提权） |

### JSON schema

`--json` 输出 `sadp.discover/v1`：字段只增不删，字段名不随语言变化。

```json
{
  "schema": "sadp.discover/v1",
  "tool": "sadp-discover",
  "version": "0.1.0",
  "generated_at": "2026-11-10T14:42:25Z",
  "port": 37020,
  "families": ["v4", "v6"],
  "rounds": 1,
  "count": 1,
  "devices": [
    {
      "sn": "DS-2CD2T47G2-L20210618AAWRL12345678",
      "mac": "a4:14:37:5f:4c:6d",
      "ipv4": "192.168.1.64",
      "ipv6": [{ "address": "fe80::21b:2bff:fe3c:4d5e", "scope_id": 22, "global": false }],
      "seen_via": "both",
      "seen_on": ["WLAN"],
      "first_seen_ago_ms": 120,
      "last_seen_ago_ms": 40
    }
  ]
}
```

## 架构

```text
sadp-cli ──▶ sadp-core ──▶ sadp-net
sadp-gui ──▶ sadp-core ──▶ sadp-net      （三阶段）
```

| crate | 职责 |
|---|---|
| `sadp-net` | 网卡枚举、双栈 UDP、组播/广播、接收线程（socket 平台差异集中在这里） |
| `sadp-core` | 协议编解码、设备模型与去重合并、扫描引擎、视图逻辑、i18n、系统工具调用（打开 URL / 目录） |
| `sadp-cli` | 参数解析、TUI、JSON/CSV 输出、日志、剪贴板 |
| `sadp-fake-device` | 本地假设备，无真机时做端到端验证 |

一次扫描的数据流：

```text
Probe ── v4 组播 239.255.255.250 / 各网卡子网广播
       └─ v6 链路本地组播 ff02::1（候选，待验证）
   │
   ▼ sadp-net   每族一个 socket；逐网卡设置 IP_MULTICAST_IF；接收线程 → channel
   ▼ sadp-core  ProbeMatch → DeviceStore（SN→MAC→IP 去重、逐字段合并）→ ScanEvent
   ▼ sadp-cli   TUI / JSON / CSV / 纯文本
```

设计要点：

- **同步线程而非 async**：小包低频，`Transport` trait 保留了替换空间。
- **去重键优先级**：`SN` → `MAC` → `IPv4` → 全局 `IPv6`；同一设备经 v4/v6 各回一包时只显示一行，`via=both`。
- **绑定 37020 失败自动退临时端口**，并提示用户；`--local-port 0` 可主动指定。
- **只读**：任何代码路径都不向设备发送写指令。

## 开发

```bash
cargo test --workspace                        # 单元 / 集成测试
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check                              # 依赖许可 / 漏洞 / 来源
cargo llvm-cov -p sadp-core --summary-only    # 覆盖率（当前行覆盖 83.8%）
```

无真机时的端到端验证（需要组播可用的真实网卡）：

```bash
cargo run -p sadp-fake-device -- --duration 15 &
cargo run -p sadp-cli -- --no-tui --local-port 0 --timeout 2000
# 或跑带 #[ignore] 的自动测试
cargo test -p sadp-fake-device -- --ignored
```

更多设计细节见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)（含协议事实、风险登记与 ADR）。

## 已知限制

- IPv6 探测组地址未经验证（见上）。
- 跨三层网段无法发现：设备只监听组播/广播，单播到设备 37020 通常被拒绝。
- 响应可被伪造，结果仅用于发现与展示，不可作为安全决策依据。
- 没有 SN / MAC / 地址的报文会被丢弃，不会显示成空白行。
- Windows 首次运行可能触发防火墙提示，需手动允许。
- 配置文件的读写尚未实现（当前只有命令行与环境变量）。

## 许可证

MIT OR Apache-2.0，见 [LICENSE-MIT](LICENSE-MIT) 与 [LICENSE-APACHE](LICENSE-APACHE)。
