# 清单 12：「24h 连续性」验证手册

目标：证明 `--repeat 1000` 连续跑 24 小时后，**内存、句柄、线程不增长，扫描不中断、不崩溃**。

> 本次 Gate 2 验收**暂缓执行**（机器电池供电，跑到一半断电等于没测），工具链已就绪：接上电源后执行
> `.gate2/soak/start-24h.sh --wait-ac` 即可，判定与产物见下文。

## 为什么需要专用脚本

`--repeat` 只在 TUI 模式生效；非交互模式固定只跑一轮。无人值守跑 24 小时需要：

1. 一个**常驻伪终端**（TUI 要求 stdout 是 tty）；
2. 每 60 秒采样 RSS / FD / 线程数；
3. 结束后能自动退出并给出**可判定的结论**，而不是靠人盯屏幕。

`supervisor.py` 就是这层壳：持有 80×24 的 pty，启动 TUI，采样写 CSV，到时发 `q` 退出；
`analyze.py` 负责出裁决；`start-24h.sh` 在外面再包一层（电源门禁 + caffeinate + 自动分析）。

## 一键启动（推荐）

~~~bash
# 挂起：检测到接上电源后自动开跑 24h（插上电后等 60s 确认，然后启动）
nohup .gate2/soak/start-24h.sh --wait-ac > .gate2/soak-24h.launch.log 2>&1 &

# 已接电源时直接开跑
.gate2/soak/start-24h.sh

# 看进度 / 结论
.gate2/soak/start-24h.sh --status

# 中止（supervisor 收到 SIGTERM → 发 q 退出 → 写 meta → 自动分析；中止必然判 FAIL）
.gate2/soak/start-24h.sh --stop

# 只重跑分析
.gate2/soak/start-24h.sh --analyze
~~~

其它参数：`--duration 秒`（默认 86400）、`--interval 秒`（默认 60）、`--outdir 目录`
（默认 `.gate2/soak-24h`）、`--fake`（用 fake-device 代替真机）、`--allow-battery`（演练用，跳过电源门禁）。

启动器动手前会检查：release 二进制存在、磁盘 ≥ 1 GB、没有另一个 `--repeat` 扫描进程在跑、
没有残留实例；不满足直接拒绝，不会留下半截任务。

真机模式（默认）不加 `--local-port`：37020 被占用时 CLI 会自动挑临时端口，与本场景一致。
若确实需要固定端口，直接调 `supervisor.py --local-port 0`。

## 直接调用 supervisor（调试用）

~~~bash
python3 .gate2/soak/supervisor.py --duration 75 --interval 5 --outdir .gate2/soak-rehearsal --no-fake
python3 .gate2/soak/analyze.py .gate2/soak-rehearsal
~~~

`--no-fake` 时直接对着当前网段跑。LAN 上有一台真机 `DS-2DE4423DW-D/GLT/XM @ 192.168.1.120`；
它中途下线只会让 `found` 掉到 0，不影响连续性判定。

## 环境要求

| 项 | 要求 |
|---|---|
| 电源 | **必须接 AC**。启动器默认拒绝在电池上开跑（`--wait-ac` 会挂起等电源） |
| 防睡眠 | 启动器已用 `caffeinate -is` 包住进程；合盖仍会睡，需保持开盖或接外接显示器 |
| 磁盘 | `-vv` 日志约 10–30 MB/天；CSV < 1 MB |
| 端口 | 与 fake-device 同机时脚本自动加 `--local-port 0` |

## 判定标准（analyze.py 的硬门槛）

| 指标 | 通过条件 | 理由 |
|---|---|---|
| 进程存活 | 跑满时长且 `exit_code ∈ {0,1}`、`stopped_reason=duration` | 崩溃/被 OOM 杀/人工中止是一票否决 |
| 轮次连续 | 实际轮次 ≥ 预期 90%，相邻轮间隔 ≤ 10 s | 证明扫描没卡死 |
| RSS | 预热后（前 1h）增长 ≤ 10 MB，且拟合斜率 ≤ 1 MB/h | 排除缓慢泄漏；只看终值会被 allocator 噪声误导 |
| FD | 预热后跨度 ≤ 5，末值比首值增长 ≤ 2 | 句柄泄漏通常表现为单调上升 |
| 线程 | 跨度 ≤ 2 | 接收线程应按网卡数固定 |
| 日志 | 无 `ERROR`/panic；总量 ≤ 512 MB | 异常被静默吞掉同样不算通过 |
| 日志轮转 | 跨天产生 `.log.YYYY-MM-DD` 新文件（人工确认） | 防止单文件无限增长 |

## 产物

~~~text
.gate2/soak-24h/
  samples.csv      # t_s,time,rss_kb,vsz_kb,cpu,etime,threads,fds,sockets,log_bytes
  meta.json        # 起止时间、退出码、停止原因
  tui-final.txt    # 结束时屏幕（ANSI 已剥离）
  verdict.json     # 机器可读裁决
  summary.txt      # analyze.py 的人类可读输出
  run.log          # supervisor 的 stdout（每 10 个采样一行心跳）
  run.pid          # 运行中的 caffeinate 包装进程
~~~

## 演练记录

| 时长 | 结果 | 关键数字 |
|---|---|---|
| 90 s（interval 5 s） | PASS | RSS 恒 15.9 MB，FD 13，线程 6，90 轮，最大间隔 1.0 s |
| 75 s（--no-fake，走启动器） | PASS | RSS 15.8 MB，FD 13，线程 6，75 轮，日志 0.1 MB |
| 20 s（走启动器） | PASS | 同上，验证端到端（含自动分析） |
| 53 s + SIGTERM | FAIL（预期） | `stopped_reason=signal-SIGTERM`，证明中止路径能写 meta 并判 FAIL |

## 常见坑

- **电池供电**：`caffeinate -s` 在电池下不生效，必须插电。
- **合盖睡眠**：`caffeinate` 挡不住合盖，24h 会中断。
- **日志跨天**：0 点会切到新文件，`analyze.py` 会把窗口内的所有日志一起统计。
- **与 fake-device 抢端口**：脚本自动加 `--local-port 0`；手工跑时要自己加。
- **只看 RSS 终值**：RSS 有 allocator 缓存，单点比较容易误判；看预热后的斜率更稳。
- **真机中途离线**：轮次里的 `found` 会掉到 0，不影响连续性判定（判据只看轮次、内存、句柄）。
