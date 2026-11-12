# 清单 12：「24h 连续性」验证手册

目标：证明 `--repeat 1000` 连续跑 24 小时后，**内存、句柄、线程不增长，扫描不中断、不崩溃**。

## 为什么需要专用脚本

`--repeat` 只在 TUI 模式生效；非交互模式固定只跑一轮。无人值守跑 24 小时需要：

1. 一个**常驻伪终端**（TUI 要求 stdout 是 tty）；
2. 每 60 秒采样 RSS / FD / 线程数；
3. 结束后能自动退出并给出**可判定的结论**，而不是靠人盯屏幕。

`.gate2/soak/supervisor.py` 就是这层壳：它持有 80×24 的 pty，启动 TUI，采样写 CSV，到时发 `q` 退出，再调用 `analyze.py` 出裁决。

## 环境要求

| 项 | 要求 |
|---|---|
| 电源 | **必须接 AC**。24h 跑到一半没电，等于没测（本机当前 61% / 15h，直接跑必失败） |
| 防睡眠 | `caffeinate -is` 包住整条命令；合盖仍会睡，需保持开盖或接外接显示器 |
| 磁盘 | 日志 `-vv` 约 10–30 MB/天；CSV < 1 MB |
| 端口 | 与 fake-device 同机时，扫描器加 `--local-port 0`（脚本已处理） |

## 命令

先做一次 2 分钟的演练（确认采样链路正常）：

```bash
python3 .gate2/soak/supervisor.py --duration 120 --interval 5 --outdir .gate2/soak-rehearsal
python3 .gate2/soak/analyze.py .gate2/soak-rehearsal
```

正式 24 小时（建议 `nohup` 脱离当前 shell）：

```bash
mkdir -p .gate2/soak-24h
nohup caffeinate -is python3 .gate2/soak/supervisor.py \
  --duration 86400 --interval 60 --outdir .gate2/soak-24h \
  > .gate2/soak-24h/run.log 2>&1 &
```

无 fake-device、直接对着真实网段跑（更贴近清单原文）。当前 LAN 上已有一台真机 `DS-2DE4423DW-D/GLT/XM @ 192.168.1.120`，这条命令会直接对着它跑：

```bash
nohup caffeinate -is python3 .gate2/soak/supervisor.py \
  --duration 86400 --interval 60 --outdir .gate2/soak-24h --no-fake \
  > .gate2/soak-24h/run.log 2>&1 &
```

中途查看：

```bash
tail -5 .gate2/soak-24h/samples.csv          # 最近采样
ps -o rss=,etime= -p "$(pgrep -f 'sadp-discover --repeat')"
tail -f .gate2/soak-24h/run.log
```

到点后脚本会自己退出并打印裁决；也可以手动重跑分析：

```bash
python3 .gate2/soak/analyze.py .gate2/soak-24h; echo "exit=$?"
```

## 判定标准（analyze.py 的硬门槛）

| 指标 | 通过条件 | 理由 |
|---|---|---|
| 进程存活 | 跑满时长且 `exit_code=0`，中途未重启 | 崩溃/被 OOM 杀是一票否决 |
| 轮次连续 | 实际轮次 ≥ 预期 90%（`--repeat 1000` 约 86400 轮），相邻轮间隔 ≤ 10 s | 证明扫描没卡死 |
| RSS | 预热后（前 1h）增长 ≤ 10 MB，且拟合斜率 ≤ 1 MB/h | 排除缓慢泄漏；只看终值会被 allocator 噪声误导 |
| FD | 预热后跨度 ≤ 5，末值比首值增长 ≤ 2 | 句柄泄漏通常表现为单调上升 |
| 线程 | 跨度 ≤ 2 | 接收线程应按网卡数固定 |
| 日志 | 无 `ERROR`/panic；总量 ≤ 512 MB | 异常被静默吞掉同样不算通过 |
| 日志轮转 | 跨天产生 `.log.YYYY-MM-DD` 新文件（人工确认） | 防止单文件无限增长 |

## 产物

```text
.gate2/soak-24h/
  samples.csv      # t_s,time,rss_kb,vsz_kb,cpu,etime,threads,fds,sockets,log_bytes
  meta.json        # 起止时间、退出码、停止原因
  tui-final.txt    # 结束时屏幕（ANSI 已剥离）
  verdict.json     # 机器可读裁决
  fake.log         # fake-device 输出（使用 --no-fake 时没有）
  run.log          # nohup 的 stdout/stderr
```

## 常见坑

- **电池供电**：`caffeinate -s` 在电池下不生效，必须插电。
- **合盖睡眠**：`caffeinate` 挡不住合盖，24h 会中断。
- **日志跨天**：0 点会切到新文件，`analyze.py` 会把窗口内的所有日志一起统计。
- **与 fake-device 抢端口**：脚本自动加 `--local-port 0`；手工跑时要自己加。
- **只看 RSS 终值**：RSS 有 allocator 缓存，单点比较容易误判；看预热后的斜率更稳。
- **真机中途离线**：轮次里的 `found` 会掉到 0，不影响连续性判定（判据只看轮次、内存、句柄）。
