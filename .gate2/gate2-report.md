# Gate 2 验收报告（macOS 真机）

- 日期：2026-11-12
- 主机：macOS 26.6.2 (25G83)，Apple Silicon (arm64)，8 核
- 工具链：rustc 1.99.1 / cargo 1.99.1
- 被测产物：`sadp-discover 0.1.0`，`cargo build --release -p sadp-cli`（ad-hoc 签名）
- 清单来源：`docs/MACOS.md` §5

## 结论

12 项清单：**11 项已完成（6、8 由人工验证通过），第 12 项（24h 连续性）本次暂缓**；F1 经人工 80×24 复测撤回，不计为缺陷。

- 未发现崩溃、端口冲突、日志落盘或双栈合并问题。
- **F1 撤回**：人工 80×24 实测无截断；此前「详情首行裁掉型号」的结论来自终端画面抓取/回放方式，属于测量方法误判，已从缺陷列表移除。
- 复核时在 LAN 上发现真机 **DS-2DE4423DW-D/GLT/XM @ 192.168.1.120**（v4，固件 `V7.3 build 241203`）：清单 3/9 已用真机重跑。双栈/仅 v6 场景该设备不回 v6，仍用 `sadp-fake-device` 覆盖。
- 清单要求的 macOS 14/15 与 Intel 机型未覆盖（本机为 macOS 26 / arm64）。

## 结果明细

| # | 检查项 | 结果 | 证据 / 说明 |
|---|---|---|---|
| 1 | 冷启动可用 | ✅ 本机 | `--version` 打印 `sadp-discover 0.1.0`。本机为 ad-hoc 签名，`spctl` 判定 rejected 属预期；Gatekeeper 冷启动与公证链路需 Developer ID 产物，未覆盖 |
| 2 | 网卡枚举 | ✅ | en0（ifindex 11，v4✓ v6✓）、utun5（26）、lo0（1）；JSON 含 netmask/broadcast/joins |
| 3 | 默认发现 | ✅ 真机 | TUI 首帧 13 ms、真机（192.168.1.120）上屏 349 ms，远快于 3 s |
| 4 | 双栈去重 | ✅ | 同一 SN/MAC 的 v4+v6 应答合并为 1 行，`seen_via=both`，`seen_on=[en0, utun5]` |
| 5 | 仅 IPv6 | ✅ | `--family v6` 正常出结果：`seen_via=v6`，`fe80::472:e65:266:b793%11`，无 v4 socket 也不退出 |
| 6 | 中英切换（80×24） | ✅ 人工通过 | 80×24 下中英切换即时生效，页脚/表头/详情完整，无截断（F1 已撤回） |
| 7 | 导出 / 复制 | ✅ | 生成 `sadp-discover-20261112-112720.json`（count=1）；`pbpaste` 得到 SN/MAC/IPv4/Model/Via 五行 |
| 8 | 本地网络权限 | ✅ 人工通过 | 人工在 GUI 首次运行完成允许/拒绝两态验证；拒绝态降级提示已实际触发（0 响应 → macos_local_network） |
| 9 | 路由/多网卡 | ✅ | 三块网卡（en0 / en6 / utun5）均发起探测；en6 为 169.254 链路本地、发送失败仅告警不影响结果；真机从 en0 回包，未漏 |
| 10 | 与系统工具共存 | ✅ | 先占住 37020 再启动：提示「端口 37020 被占用，改用临时端口 55829」，继续扫描，退出码 1 而非崩溃 |
| 11 | 日志路径 | ✅ | 自动创建 `~/Library/Application Support/sadp-discover/logs/sadp-discover.log.2026-11-12`，含 debug 行（socket 端口、轮次结果） |
| 12 | 连续性 | ⏸ 暂缓 | 本机当前为电池供电（60%），跑 24h 必然中途断电，判定无效，本次跳过。采集链路已验证（75 s / 20 s 演练均 PASS：RSS 恒 15.8 MB、FD 13、线程 6、轮次无缺失）；一键启动与判据见 [.gate2/soak/README.md](.gate2/soak/README.md)，接电源后执行 `.gate2/soak/start-24h.sh --wait-ac` 即可 |

## 观察项

### F2. macOS 上「0 设备」必然提示本地网络权限（观察项）

权限正常、网段内也确实没有设备时，仍会打印：

```text
没有收到任何 SADP 响应。macOS 15+ 需要在「系统设置 → 隐私与安全性 → 本地网络」中允许当前终端。
```

原因：`zero_response_hint` 只在「收到过 ProbeMatch」时才算“网络通”([main.rs:221](crates/sadp-cli/src/main.rs#L221))，而空网段同样收不到任何响应。建议把「探测包是否成功发出」也纳入判断（发送失败才提示权限），避免固定误报。

## 环境备注（非产品缺陷）

- 在受限沙箱下运行时，向 239.255.255.250 / ff02::1 发送会得到 `ENETUNREACH (os error 65)`，普通 Python socket 同样复现；放开文件/网络沙箱后组播与 v6 均正常。程序在发送失败时仅告警并继续，行为合理。
- 真机矩阵（IPC/NVR/球机各一、多网段）仍未覆盖：本机只发现 1 台 PTZ（DS-2DE4423DW-D/GLT/XM）。

## 证据文件

- `.gate2/evidence/version.txt`
- `.gate2/evidence/interfaces.txt` / `interfaces.json`
- `.gate2/evidence/default-scan.json`、`.gate2/evidence/v6-only.json`、`.gate2/evidence/en0-only.json`
- ~~`.gate2/evidence/tui-80x24.json`~~（已移除：pyte 取屏回放不可靠，以人工 80×24 实测为准）
- `.gate2/evidence/default-scan.log`
- 复现脚本：`.gate2/pty_tui2.py`、`.gate2/pty_timeline.py`、`.gate2/pty_soak.py`、`.gate2/screen_check.py`、`.gate2/size_check.py`

## 未覆盖项（按优先级）

| 项 | 当前状态 | 触发条件 |
|---|---|---|
| 清单 12（24h 连续性） | ⏸ 暂缓，工具链已就绪 | 接上电源即可：`.gate2/soak/start-24h.sh --wait-ac`，产物落 `.gate2/soak-24h/` |
| macOS 14/15 + Intel | 未覆盖（只有 macOS 26 / arm64 一台） | 换机或 CI |
| 真机矩阵（IPC/NVR/球机） | 只覆盖 1 台球机 | 网段内接入其它型号 |
| F2（0 设备必提示权限） | 观察项，未修 | 需要产品决定：是否把「发送成败」纳入判断 |
