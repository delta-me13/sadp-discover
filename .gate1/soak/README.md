# 24h 连续性压测（Windows 真机）

对应 Gate 1 第 8 条：连续扫描 24 小时，内存 / 句柄稳定。
`.gate2/soak/` 是 macOS 版本；本目录是 Windows 版本，判据一致。

## 前提

- 已接通电源（脚本会检查 `Win32_Battery.BatteryStatus`，非 2 = 交流电时直接退出）。
- 网段内至少有一台真机，否则测的是空轮询，价值有限。
- 使用默认端口 37020、默认参数，TUI 每 3 s 开一轮。

## 运行

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .gate1/soak/start-24h.ps1
# 可选：-Hours 0.5 先做半小时演练
```

产物：

- `soak-<时间戳>.csv`：每 60 s 一行 `timestamp,working_set_bytes,handles,threads,cpu_seconds`
- `soak-<时间戳>.summary.txt`：最小/最大驻留内存、句柄、线程，以及是否中途退出

## 判据

- 进程不退出、无 OOM；
- 驻留内存无持续单调上升（首尾相差 < 2 MB 视为稳定）；
- 句柄数首尾相差 ≤ 5；
- 线程数不持续增长（扫描线程会随轮次启停，允许小幅波动）。

TUI 窗口以最小化方式启动，不要关闭；结束后脚本会自行终止。

## 已验证范围

2026-11-13 在 Windows 11 / Ryzen 7 5800H 上做过 25 分钟演练：工作集 20.8 → 22.3 MiB（缓慢上升）、
私有内存 9.8–10.0 MB（基本不变）、句柄 176–178 持平、线程 5–7 波动，无退出。25 分钟不足以判定 24 h 稳定性，
正式验收需用本脚本跑满 24 h，详见 `../gate1-report.md` 第 8 项。

CSV 列：`timestamp,working_set_bytes,private_bytes,handles,threads,cpu_seconds`。
