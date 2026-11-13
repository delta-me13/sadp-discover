# Gate 1 验收报告（Windows 真机）

- 日期：2026-11-13
- 主机：Windows 11 专业工作站版 10.0.26300（Lenovo 82JQ，AMD Ryzen 7 5800H，64 GB）
- 工具链：rustc 1.99.1 / cargo 1.99.1
- 被测产物：`sadp-discover 0.1.0`，`cargo build --release -p sadp-cli`
- 设备：**DS-2DE4423DW-D/GLT/XM @ 192.168.1.120**（固件 V7.3 build 241203，HTTP 80 / SDK 8000，已激活），
  经 `以太网`（192.168.1.1）直连
- 清单来源：`docs/ARCHITECTURE.md` §「验收门槛（Gate 1）」

## 结论

9 项清单：**3 项通过、4 项部分完成、2 项因环境缺失未测**。真机矩阵（IPC/NVR、Win10）、
双栈真机、防火墙三项受硬件与权限限制，本次无法闭环；代码侧可验证的部分全部执行，
并在真机上暴露、修复了 1 个 `--interface` 匹配缺陷（F1）。

- 已通过：多网卡/虚拟网卡隔离（2）、JSON schema 冻结（6）、双语界面（7）。
- 部分完成：真机矩阵（1，仅球机 × Win11）、IPv6-only 降级（4）、24h 连续性（8，本次 25 min 演练）、
  与官方 SADP 共存（9，官方工具未安装，用端口占用做了等效验证）。
- 未测：双栈真机（3，设备无 IPv6）、防火墙首启（5，本机防火墙服务未运行且无管理员权限）。

## 结果明细

| # | 检查项 | 结果 | 证据 / 说明 |
|---|---|---|---|
| 1 | 3 种真机（IPC/NVR/球机）× Win10/Win11，默认参数发现 | ⚠️ 部分 | 只有 1 台球机、只有 Win11。release 默认参数（3 s 窗口）发现 1 台，首包 **73 ms**；TUI 连续运行 25 min，日志记录 463 轮全部 `found=1`（最后约 2 分钟无轮次日志，见 F3）。IPC/NVR 与 Win10 不在现场，不能用假设备冒充 |
| 2 | 双网卡 + 虚拟网卡不漏设备；VPN 未连接不误报 | ✅ | 本机 9 个适配器：以太网、WLAN、2×VMware、Tailscale、vgate0 隧道、回环，另有未连接的 aTrust VPN 与 Not Present 的幽灵适配器。逐网卡扫描只有以太网命中；断开/幽灵适配器在枚举阶段被过滤，不产生幽灵行；多选（`5,21`、`5,5`）仍只有 1 行，`seen_on` 仅 `以太网` |
| 3 | 双栈：同一设备 v4+v6 只显示一行，`Via=both` | ⛔ 未测 | 球机 `ipv6=[]`，以太网链路也只有 IPv4；无法构造真机双栈。等价验证：`sadp-fake-device` 双栈 e2e 通过（发现 1 台），`crates/sadp-core/tests/engine.rs:62` 断言 `SeenVia::Both`；`--family v6` 对球机返回 0 台且不崩溃 |
| 4 | IPv6-only 主机（禁用 IPv4）能启动并完成扫描 | ⚠️ 部分 | 无法真正禁用 IPv4 协议栈（需管理员/虚拟机）。用「v4 bind 到不存在的地址」复现 v4 不可用：进程打印 `v4 disabled` 后继续以 v6 扫描，退出码 1（未发现）而非 2/崩溃。另测 `--family v6` 在 WLAN 上正常完成 |
| 5 | 防火墙开启时首启有提示，允许后有结果 | ⛔ 未测 | 本机 Windows Defender 防火墙服务（mpssvc）**未运行**，策略项 `EnableFirewall=1` 但无实际过滤；当前会话无管理员权限，无法启停服务或加规则。间接验证：空网段（仅 WLAN）扫描时 `sent_packets>0` 只给通用提示「没有收到任何 SADP 响应」，没有误报权限问题 |
| 6 | `--json` schema 冻结 + `insta` 快照 | ✅（有偏差） | `JSON_SCHEMA = "sadp.discover/v1"`，`json_scan_output_matches_frozen_schema` 通过；13 个 CLI 黑盒测试全过。**偏差**：仓库未引入 `insta`，JSON 用字段断言、TUI 用 `TestBackend` 文本断言，没有 `.snap` 文件；见「偏差与观察」 |
| 7 | `--lang en-US` / `zh-CN` 无写死、无截断、key 对齐 | ✅ | key 集合/占位符/无硬编码 4 个测试通过；release 中英文扫描输出的表头、页脚、`Via/Act` 均本地化，无 `tui.*` 原文；TUI 在 80×24 下双语渲染含 IP/型号/MAC，快捷键栏宽度测试 ≤80 列 |
| 8 | 连续扫描 24 h 内存/句柄稳定；`sadp-core` 覆盖率 ≥80% | ⚠️ 部分 | 25 min 演练：工作集 20.8→22.3 MiB（缓升）、私有内存末段两次采样 9.8 / 10.0 MB、句柄 176–178 持平、线程 5–7，无退出；详见下方「8. 连续性」。24 h 未跑，脚本见 `.gate1/soak/start-24h.ps1`。覆盖率沿用 2026-11 记录的 **83.8%**（本次环境未装 `cargo-llvm-cov`，未复测） |
| 9 | 与官方 SADP 同时运行不互踢端口 | ⚠️ 等效验证 | 官方 SADP 未安装。用 Python 普通 socket（不设 `SO_REUSEADDR`）占住 37020 后启动扫描：提示「端口 37020 被占用，改用临时端口 53755」，**仍然发现球机**、退出码 0；反向场景（扫描器先占 37020，再启动第二个扫描器）两者都正常发现设备，无端口错误 |

## 逐项说明

### 1. 真机矩阵

现场只有一台球机，直连主机网口：

```text
192.168.1.120  04:ee:cd:7e:d0:32  DS-2DE4423DW-D/GLT/XM  ...  v4  ✓
```

- 默认参数 + 默认端口：`sadp-discover --no-tui`（3 s 窗口）1 台，`first_seen_ago_ms` 推算首包 **73 ms**；
- TUI 以 `--repeat 3000` 连续跑，日志每轮 `round finished ... found=1`；
- `--json` 中 `dhcp=false`、`activated=true`、`http_port=80`、`sdk_port=8000` 与设备 Web 页（HTTP 200）一致；
- 设备 `model` 上报为数字 `196607`，展示层回退到 `description`（既有行为，符合预期）。

缺口：IPC、NVR 与 Win10 主机不在现场；Gate 1 要求 3 类 × 2 平台共 6 种组合，本次只覆盖 1 种。

### 2. 多网卡与虚拟网卡

`interfaces` 枚举到 7 个可用适配器，另有 3 个被正确跳过：

| 适配器 | 状态 | 结果 |
|---|---|---|
| 以太网（5） | Up | 发现球机，`seen_on=[以太网]` |
| WLAN（21） | Up | 0 台，无幽灵行 |
| VMware VMnet1 / VMnet8（2/17） | Up | 0 台，无幽灵行（等价于 Hyper-V 虚拟网卡场景） |
| Tailscale（43） | Up | 0 台，v4/v6 组播均加入成功 |
| vgate0（13） | Up | 0 台 |
| 回环（1） | Up | 0 台；修复前会误扫全网卡，见 F1 |
| aTrust VPN（26） | Disconnected | 枚举阶段过滤；`-i 26` 报「no interface matched」而不是误报设备 |
| 蓝牙 PAN（25） | Disconnected | 同上 |
| 以太网 2（6） | Not Present | 枚举阶段过滤，不产生幽灵行 |

多选 `-i 5,21`、`-i 5,5`、`-i 以太网,Tailscale` 都只返回 1 台，未出现重复行。

### 3. 双栈

球机在 SADP 响应里没有任何 IPv6 地址，主机以太网也没有 IPv6 地址（WLAN 有全局 v6，但与设备不同链路），
因此无法做「同一台设备 v4/v6 各回一包」的真机验证。

已有等价覆盖：

- `sadp-fake-device` 双栈 e2e：`cargo test -p sadp-fake-device -- --ignored` 通过（1 台）；
- `crates/sadp-core/tests/engine.rs:62`：同一 SN 的 v4+v6 事件合并后 `SeenVia::Both`；
- 真机上 `--family v6` 返回 0 台 + 通用提示，进程正常退出。

### 4. IPv6-only 主机

无法真正禁用 IPv4 协议栈（需要管理员改网卡绑定或开虚拟机），改用等价故障注入：

```text
! bind 203.0.113.1:37020 failed ... falling back to ephemeral port
! v4 disabled: socket error: bind 203.0.113.1:0 ...
未发现设备
没有收到任何 SADP 响应。...
exit=1
```

v4 不可用时进程没有退出，而是降级为 v6 继续扫描——正是清单第 4 条要验证的行为；
`--family v6 -i 21`（WLAN，有全局 IPv6）同样能正常完成一轮。

局限：没有覆盖「AF_INET socket 创建即失败」（如内核禁用 IPv4）的路径，只覆盖了 bind 失败。

### 5. 防火墙

```text
mpssvc (Windows Defender Firewall)  Stopped
HKLM\...\FirewallPolicy\StandardProfile\EnableFirewall = 1
```

服务未运行，实际没有过滤；无管理员权限，无法启动服务或添加放行/阻止规则，因此
「首次运行提示 → 允许 → 有结果」这条链路没有真实执行。

间接验证了提示逻辑：在没有任何设备的 WLAN 上扫描，`sent_packets>0`，输出通用提示
「没有收到任何 SADP 响应。请确认设备与主机在同一二层网段，且防火墙放行 UDP 37020」，
没有把「没人回」误判成权限问题（macOS 的同类误报已在 Gate 2 修掉）。

### 6. JSON schema

- `sadp.discover/v1` 由 `sadp_core::JSON_SCHEMA` 常量冻结，字段只增不删；
- `crates/sadp-cli/tests/cli.rs` 的 `json_scan_output_matches_frozen_schema` 校验
  `schema/tool/devices/port/generated_at`；
- `interfaces --json` 使用独立 schema `sadp.interfaces/v1`。

偏差：架构文档把 `insta` 列为快照测试选型，但仓库从未引入该依赖，也没有 `.snap` 文件。
目前 TUI 用 `TestBackend` 断言文本内容，JSON 用字段断言，能防住本次涉及的回归，
但不是文档写的那种快照测试。建议二选一：补 `insta`（TUI 80×24 双语 + JSON 顶层结构），
或把文档改成「断言测试」。

### 7. 双语

- `cargo test -p sadp-core --test i18n_keys`：4 个测试（key 集合一致、占位符一致、全部可解析、无硬编码）；
- release 中英文输出：表头、页脚、单位全部本地化，没有回退成 key；
- `renders_device_and_help_in_both_languages`：80×24 两种语言都能看到 IP/型号/MAC；
- `help_bar_fits_eighty_columns`：两种语言快捷键栏 ≤80 列（英文恰好 80）。

### 8. 连续性（25 min 演练）

- 方式：TUI 默认参数 + 默认端口 37020，`--repeat 3000`，每 30 s 采样一次进程指标；
- 结果：50 个样本（首尾 24m32s，30 s 间隔）；工作集 **20.81 → 22.28 MiB**（最小/最大同首尾），
  私有内存末段两次采样 **9.8 / 10.0 MB**（基本不变；CSV 未记录该列），句柄 **178 → 178**（区间 176–178），线程 **5–7**；进程未退出、无 OOM；
- 日志：10:47–10:59 的干净窗口 250 轮全部 `found=1`，无 `recv failed`/`send failed`；60 s 实测 20 轮，符合 3 s 一轮；
  整个进程共记录 463 轮，最后一轮在 10:59:29，之后到 11:01:31 终止前约 2 分钟没有新的 `round finished`（见 F3）；
- 判读：句柄与线程平稳；工作集约 1.5 MiB/25 min 缓慢上升，私有内存不变，暂时分不清是分配器/工作集缓存还是泄漏，
  25 min 不足以替代 24 h 结论；
- 24 h 未跑：需要连续 24 h 占用桌面会话，脚本与判据已备（`.gate1/soak/`），接电源后可直接执行。

### 9. 与官方 SADP 共存

官方 SADP 未安装，做两个等效场景：

1. **端口被别的进程占用**：Python 普通 socket（不设 `SO_REUSEADDR`）绑定 `0.0.0.0:37020`，
   扫描器启动时 `bind` 返回 `os error 10013`，自动回退临时端口 53755，**仍发现设备**，退出码 0；
2. **两个扫描器共存**：TUI 已占 37020 时再启动一个 CLI 扫描，`SO_REUSEADDR` 允许绑定，
   第二个进程也发现设备，没有端口报错或互踢。

局限：没有官方 SADP 的真实 socket 选项可对照（它是否设 `SO_REUSEADDR`、是否会主动抢占端口未知）。

## 全量测试与静态检查

2026-11-13 本机（release 构建 + debug 测试）：

- `cargo test --workspace`：**119 passed / 0 failed / 1 ignored**（组播 e2e 需真实网卡），合计 120；
- `cargo test -p sadp-fake-device --test e2e -- --ignored`：1 passed；
- `cargo clippy --workspace --all-targets -- -D warnings`：通过；
- `cargo fmt --all -- --check`：通过；
- `cargo build --release -p sadp-cli`：通过（本次真机证据均来自该产物）。

## 偏差与观察

### F1. `--interface` 纯数字同时按索引和 IPv4 前缀匹配（已修复）

**症状**：`-i 1`（回环）在真机上返回了以太网上的球机，`seen_on=[以太网]`。

**根因**：`matches_selector` 先按索引匹配 `1`，未命中后继续走「IPv4 字符串前缀」分支；
`192.168.1.120`、`10.x`、`100.x` 都以 `1` 开头，于是回环扫描把所有网卡都选中了。
这也解释了调试日志里本地各网卡地址都在回环扫描时收到自己发出的 Probe。

**修法**：纯数字只按索引解释，完整 IP 只做精确匹配，仅非数字前缀允许模糊匹配。

**验证**：`selector_matches_name_index_and_address` 新增断言（`-i 1` 不匹配 192.168.1.10）；
修复后 `-i 1` 返回 0 台，`-i 5`/`-i 以太网`/`-i 192.168.1.` 仍返回球机。

### F2. 未连接适配器按索引选择时报「no interface matched」（观察，未改）

`-i 26`（aTrust，Disconnected）返回 `网络错误：no usable interface for no interface matched ["26"]`。
行为正确（不误报设备），但提示像「写错了名字」。可考虑改成「适配器未连接/不可用」。
本次不改，避免扩大范围。

### F3. 25 min 压测末尾 2 分钟轮次日志停止（观察，未定位）

进程从 10:36:16 跑到 11:01:31，日志最后一条 `round finished` 是 10:59:29 的 round=463。
之后 2 分钟进程仍在（句柄 178、线程 5、CPU 从 10.89 s 微增到 10.94 s），但日志不再出现轮次行，
工作集也停在 23,363,584 字节不再变化。

可能原因：tracing 的 non-blocking worker 停摆/缓冲丢失，或扫描线程阻塞在向 UI 发送事件
（`tx.send` 背压）。当前证据分不清两者；下次 24h 压测应同时记录 TUI 轮次计数或日志文件 mtime，
并在复现时抓 stderr。**不影响已记录的 463 轮结果**，但 24h 结论必须排除该停摆。

### 环境说明（非产品缺陷）

- 主机同时运行 Tailscale、vgate0、VMware 虚拟网卡；默认 `Auto` 会逐网卡发包，属预期。
- `sadp-discover` 为 debug 构建时启动约 0.2 s；本次正式证据使用 release 构建。
- 本机未安装官方 SADP，无法验证其端口选项。

## 未覆盖项与补测条件

| 项 | 状态 | 补测条件 |
|---|---|---|
| IPC / NVR 真机 | 未测 | 接入任一 IPC/NVR（同二层），跑 `scan --no-tui` 与 TUI |
| Win10 主机 | 未测 | 任一 Win10 机器，重复第 1、2、7 项 |
| 真机双栈 | 未测 | 设备开启 IPv6 + 主机同链路有 IPv6（链路本地即可），看 `Via=both` |
| 防火墙首启 | 未测 | 管理员权限 + 启用 mpssvc，首次运行看提示，放行后复测 |
| 24 h 连续性 | 未跑 | 接电源执行 `.gate1/soak/start-24h.ps1` |
| 覆盖率复测 | 未复测 | `cargo install cargo-llvm-cov` 后 `cargo llvm-cov -p sadp-core --summary-only` |
| 官方 SADP 共存 | 等效验证 | 安装官方 SADP，二者同时运行观察端口与结果 |

## 证据文件

- `.gate1/evidence/host.txt`、`adapters.txt`、`interfaces.txt`、`interfaces.json`、`toolchain.txt`
- `.gate1/evidence/default-scan.txt` / `default-scan.json`、`scan-en.txt`、`scan-zh.txt`
- `.gate1/evidence/per-interface-summary.txt`、`multi-selector-summary.txt`
- `.gate1/evidence/port-occupied.json` / `port-occupied.err.txt`
- `.gate1/evidence/v4-bind-fail.txt`、`v6-only-wlan.txt`、`v6-only-no-ipv6-iface.txt`
- `.gate1/evidence/raw/`（真机 ProbeMatch 原文）
- `.gate1/soak/soak-37020.csv`（25 min、50 个采样点）
- 复跑：`.gate1/soak/README.md`
