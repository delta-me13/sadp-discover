#!/bin/bash
# 清单 12「24h 连续性」启动器：电源门禁 + caffeinate + 自动裁决。
#
#   ./soak/start-24h.sh --wait-ac                 # 挂起，接上电源后自动开跑 24h（推荐）
#   ./soak/start-24h.sh                           # 已接电源时立即开跑
#   ./soak/start-24h.sh --allow-battery --duration 300   # 演练，不要求电源
#   ./soak/start-24h.sh --status                  # 看进度 / 结论
#   ./soak/start-24h.sh --stop                    # 中止（supervisor 优雅退出并写 meta）
#   ./soak/start-24h.sh --analyze                 # 只跑分析
#
# 其它参数：--duration 秒 --interval 秒 --outdir 目录 --fake（用 fake-device 代替真机）
set -uo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT" || exit 1

SUP=".gate2/soak/supervisor.py"
ANA=".gate2/soak/analyze.py"
BIN="./target/release/sadp-discover"
OUTDIR=".gate2/soak-24h"
DURATION=86400
INTERVAL=60
FAKE=0              # 默认 --no-fake：对着真实网段跑
WAIT_AC=0
ALLOW_BATTERY=0
MAX_WAIT_AC=86400
ACTION=start

while [[ $# -gt 0 ]]; do
  case "$1" in
    --duration)      DURATION=$2; shift 2 ;;
    --interval)      INTERVAL=$2; shift 2 ;;
    --outdir)        OUTDIR=$2; shift 2 ;;
    --fake)          FAKE=1; shift ;;
    --no-fake)       FAKE=0; shift ;;
    --wait-ac)       WAIT_AC=1; shift ;;
    --allow-battery) ALLOW_BATTERY=1; shift ;;
    --status)        ACTION=status; shift ;;
    --stop)          ACTION=stop; shift ;;
    --analyze)       ACTION=analyze; shift ;;
    -h|--help)       sed -n '2,13p' "$0"; exit 0 ;;
    *)               echo "未知参数: $1" >&2; exit 2 ;;
  esac
done

RUN_PID="$OUTDIR/run.pid"
RUN_LOG="$OUTDIR/run.log"

say() { printf '[%s] %s\n' "$(date +%H:%M:%S)" "$*"; }
die() { printf '中止: %s\n' "$*" >&2; exit 1; }

ac_online()    { pmset -g ps 2>/dev/null | grep -q "AC Power"; }
battery_desc() { pmset -g batt 2>/dev/null | sed -n '2p' | tr -s ' '; }
alive()        { [[ -f "$1" ]] && kill -0 "$(cat "$1" 2>/dev/null)" 2>/dev/null; }
waiter_pid()   { pgrep -f "start-24h.sh --wait-ac" 2>/dev/null | head -1; }

kill_tree() {   # 先杀子进程（python），再杀 caffeinate 包装
  local pid=$1 p
  for p in $(pgrep -P "$pid" 2>/dev/null); do kill -TERM "$p" 2>/dev/null; done
  kill -TERM "$pid" 2>/dev/null
}

show_verdict() {
  if [[ -f "$OUTDIR/summary.txt" ]]; then
    tail -n 20 "$OUTDIR/summary.txt"
  elif [[ -f "$OUTDIR/verdict.json" ]]; then
    python3 - "$OUTDIR/verdict.json" <<'PY'
import json, sys
v = json.load(open(sys.argv[1]))
print("OVERALL:", "PASS" if v["verdict"] else "FAIL")
for k, ok in v["criteria"].items():
    print("  [%s] %s" % ("PASS" if ok else "FAIL", k))
PY
  else
    echo "(还没有结论)"
  fi
}

# ---------------------------------------------------------------- status
if [[ "$ACTION" == status ]]; then
  wpid=$(waiter_pid)
  if [[ -n "$wpid" ]] && ! alive "$RUN_PID"; then
    echo "挂起中: 等待接电源后自动开跑（launcher pid=$wpid，已等 $(ps -o etime= -p "$wpid" | tr -d ' ')）"
    echo "电源  : $(battery_desc)"
    echo "取消  : $(basename "$0") --stop"
    exit 0
  fi
  if alive "$RUN_PID"; then
    pid=$(cat "$RUN_PID")
    echo "运行中: pid=$pid  已运行 $(ps -o etime= -p "$pid" | tr -d ' ')"
    echo "电源  : $(battery_desc)"
    if [[ -s "$OUTDIR/samples.csv" ]]; then
      total=$DURATION
      [[ -f "$OUTDIR/meta.json" ]] && total=$(python3 -c 'import json,sys; print(int(json.load(open(sys.argv[1]))["duration_s"]))' "$OUTDIR/meta.json" 2>/dev/null || echo "$DURATION")
      awk -F, -v total="$total" 'NR>1 && $1+0>max {max=$1+0; n++; row=$0}
        END {
          if (n == 0) { print "采样  : 尚无数据"; exit }
          printf "采样  : %d 条，进度 %.1f%% (t=%ds / %ds)\n", n, (max/total)*100, max, total
          printf "最新  : rss=%.1fMB fds=%s threads=%s sockets=%s log=%.1fMB\n",
                 $3/1024, $8, $7, $9, $10/1048576
        }' "$OUTDIR/samples.csv"
    fi
    echo "--- run.log 末 3 行 ---"
    tail -n 3 "$RUN_LOG" 2>/dev/null
  else
    echo "未在运行"
    show_verdict
  fi
  exit 0
fi

# ---------------------------------------------------------------- stop
if [[ "$ACTION" == stop ]]; then
  if ! alive "$RUN_PID"; then
    wpid=$(waiter_pid)
    [[ -n "$wpid" ]] || die "没有运行中的实例，也没有等待电源的挂起任务"
    say "取消等待电源的挂起任务 pid=$wpid"
    kill_tree "$wpid"
    exit 0
  fi
  pid=$(cat "$RUN_PID")
  say "发送 SIGTERM 给 pid=${pid}（supervisor 会发 q 退出并写 meta）"
  kill_tree "$pid"
  for _ in $(seq 1 40); do alive "$RUN_PID" || break; sleep 1; done
  if alive "$RUN_PID"; then
    say "40s 未退出，强制 KILL"
    kill_tree "$pid"; kill -KILL "$pid" 2>/dev/null
  fi
  rm -f "$RUN_PID"
  say "已停止，跑一次分析"
  python3 "$ANA" "$OUTDIR" > "$OUTDIR/summary.txt" 2>&1
  tail -n 14 "$OUTDIR/summary.txt"
  exit 0
fi

# ---------------------------------------------------------------- analyze
if [[ "$ACTION" == analyze ]]; then
  python3 "$ANA" "$OUTDIR" > "$OUTDIR/summary.txt" 2>&1
  cat "$OUTDIR/summary.txt"
  exit 0
fi

# ---------------------------------------------------------------- start
[[ -f "$SUP" ]] || die "缺少 $SUP"
[[ -x "$BIN" ]] || die "缺少 ${BIN}，先 cargo build --release"
command -v python3 >/dev/null || die "缺少 python3"
alive "$RUN_PID" && die "已有运行中的实例 pid=$(cat "$RUN_PID")，先 --stop"

if pgrep -f "sadp-discover --repeat" >/dev/null; then
  die "检测到另一个 --repeat 扫描进程，先关掉避免抢 37020 端口"
fi

free_mb=$(df -m . | awk 'NR==2 {print $4}')
[[ "$free_mb" -ge 1024 ]] || die "磁盘剩余 ${free_mb}MB，不够写日志"

if ! ac_online; then
  if [[ "$ALLOW_BATTERY" -eq 1 ]]; then
    say "警告: 电池供电（$(battery_desc)），仅供演练，正式 24h 必须接电源"
  elif [[ "$WAIT_AC" -eq 1 ]]; then
    say "当前未接电源（$(battery_desc)），挂起等待，插上电源后自动开始"
    waited=0
    while ! ac_online; do
      sleep 30; waited=$((waited + 30))
      (( waited % 600 == 0 )) && say "仍在等待电源 … 已等 $((waited / 60)) 分钟"
      (( waited > MAX_WAIT_AC )) && die "等待电源超过 $((MAX_WAIT_AC / 3600))h，放弃"
    done
    say "检测到电源，等 60s 确认稳定后开跑"
    sleep 60
    ac_online || die "电源又断开了（插头不稳？），本次不启动，请重跑 --wait-ac"
  else
    die "未接电源：24h 跑到一半关机等于没测。插电后重跑，或加 --wait-ac / --allow-battery"
  fi
fi

mkdir -p "$OUTDIR"
rm -f "$OUTDIR/summary.txt" "$OUTDIR/verdict.json"
: > "$RUN_LOG"

cmd=(caffeinate -is python3 "$SUP" --duration "$DURATION" --interval "$INTERVAL"
     --outdir "$OUTDIR" --bin "$BIN")
[[ "$FAKE" -eq 0 ]] && cmd+=(--no-fake)

say "启动: ${cmd[*]}"
printf '%s\n' "启动: ${cmd[*]}" >> "$RUN_LOG"
"${cmd[@]}" >> "$RUN_LOG" 2>&1 &
child=$!
echo "$child" > "$RUN_PID"

trap 'say "收到中断，转发给 supervisor"; kill_tree "$child"' INT TERM

wait "$child"; rc=$?
rm -f "$RUN_PID"
say "supervisor 退出码 ${rc}，开始分析"
python3 "$ANA" "$OUTDIR" > "$OUTDIR/summary.txt" 2>&1
cat "$OUTDIR/summary.txt"
exit 0
