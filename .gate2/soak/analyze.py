#!/usr/bin/env python3
"""Analyse soak output: verdict on 24h continuity (RSS / FD / threads / rounds / logs)."""
import csv, datetime, glob, json, os, re, sys

SPARK = "▁▂▃▄▅▆▇█"


def spark(vals, width=48):
    if not vals:
        return ""
    if len(vals) > width:
        step = len(vals) / width
        vals = [vals[int(i * step)] for i in range(width)]
    lo, hi = min(vals), max(vals)
    if hi == lo:
        return SPARK[0] * len(vals)
    return "".join(SPARK[min(int((v - lo) / (hi - lo) * (len(SPARK) - 1)), len(SPARK) - 1)] for v in vals)


def slope_per_hour(points):
    # points: [(t_s, value)]
    if len(points) < 2:
        return 0.0
    xs = [p[0] / 3600.0 for p in points]
    ys = [p[1] for p in points]
    mx, my = sum(xs) / len(xs), sum(ys) / len(ys)
    den = sum((x - mx) ** 2 for x in xs)
    if den == 0:
        return 0.0
    return sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / den


def parse_logs(log_glob, start, end):
    rounds = []  # (ts, round_no, found)
    errors = []
    total_bytes = 0
    pat = re.compile(r"^(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d+)Z\s+\w+\s+\S+:\s+round finished round=(\d+) found=(\d+)")
    errpat = re.compile(r"\bERROR\b|\bpanic\b|\bPANIC\b")
    for f in sorted(glob.glob(log_glob)):
        try:
            if os.path.getmtime(f) < start - 7200:
                continue
            data = open(f, "r", errors="replace").read()
        except OSError:
            continue
        total_bytes += len(data)
        for line in data.splitlines():
            if errpat.search(line):
                errors.append(line.strip())
            m = pat.match(line)
            if m:
                ts = datetime.datetime.strptime(m.group(1), "%Y-%m-%dT%H:%M:%S.%f").replace(
                    tzinfo=datetime.timezone.utc)
                if start - 5 <= ts.timestamp() <= end + 5:
                    rounds.append((ts.timestamp(), int(m.group(2)), int(m.group(3))))
    rounds.sort()
    gaps = [(rounds[i + 1][0] - rounds[i][0]) for i in range(len(rounds) - 1)]
    return {"count": len(rounds), "rounds": rounds, "gaps": gaps, "errors": errors, "log_bytes": total_bytes}


def main():
    outdir = sys.argv[1]
    meta = json.load(open(os.path.join(outdir, "meta.json")))
    rows = list(csv.DictReader(open(os.path.join(outdir, "samples.csv"))))
    if not rows:
        print("no samples"); sys.exit(2)
    duration = float(meta["duration_s"])
    start = datetime.datetime.fromisoformat(meta["start"]).timestamp()
    end = datetime.datetime.fromisoformat(meta["end"]).timestamp()

    warm = min(3600.0, max(10.0, duration * 0.1))
    post = [r for r in rows if float(r["t_s"]) >= warm] or rows
    rss = [(float(r["t_s"]), int(r["rss_kb"])) for r in post]
    fds = [int(r["fds"]) for r in post]
    thr = [int(r["threads"]) for r in post]
    rss_growth = rss[-1][1] - rss[0][1]
    rss_slope = slope_per_hour(rss)
    fd_first, fd_last = fds[0], fds[-1]
    fd_span = max(fds) - min(fds)
    thr_span = max(thr) - min(thr)

    log_glob = os.path.expanduser("~/Library/Application Support/sadp-discover/logs/sadp-discover.log.*")
    lg = parse_logs(log_glob, start, end)
    repeat_ms = int(meta.get("repeat_ms", 1000))
    expected = duration / (repeat_ms / 1000.0)
    max_gap = max(lg["gaps"]) if lg["gaps"] else None

    crit = {
        # exit 1 只是「没发现设备」，仍是干净退出；崩溃/被 kill 才是失败。
        "process_alive": meta["exit_code"] in (0, 1) and meta["stopped_reason"] == "duration",
        "rounds_continue": lg["count"] >= 0.9 * expected and (max_gap is None or max_gap <= max(10, repeat_ms / 1000.0 * 3)),
        "rss_stable": rss_growth <= 10240 and rss_slope <= 1024,
        "fds_stable": fd_span <= 5 and (fd_last - fd_first) <= 2,
        "threads_stable": thr_span <= 2,
        "no_errors_in_log": len(lg["errors"]) == 0,
        "log_bounded": lg["log_bytes"] <= 512 * 1024 * 1024,
    }
    verdict = all(crit.values())

    print("== soak verdict ==")
    print("duration      : %.0f s (%.2f h)" % (duration, duration / 3600))
    print("exit code     : %s (%s)" % (meta["exit_code"], meta["stopped_reason"]))
    print("samples       : %d every %.0fs" % (len(rows), duration / max(len(rows), 1)))
    print("rss   growth  : %+d KB (%.1f -> %.1f MB), slope %+.0f KB/h" % (
        rss_growth, rss[0][1] / 1024, rss[-1][1] / 1024, rss_slope))
    print("rss   trend   : %s" % spark([v for _, v in rss]))
    print("fds   min/max : %d / %d (first %d, last %d)" % (min(fds), max(fds), fd_first, fd_last))
    print("fds   trend   : %s" % spark(fds))
    print("threads       : %d..%d" % (min(thr), max(thr)))
    print("rounds        : %d (expected ~%.0f), max gap %ss" % (
        lg["count"], expected, ("%.1f" % max_gap) if max_gap is not None else "n/a"))
    print("log bytes     : %.1f MB, error lines %d" % (lg["log_bytes"] / 1048576, len(lg["errors"])))
    print()
    for k, v in crit.items():
        print("  [%s] %s" % ("PASS" if v else "FAIL", k))
    print()
    print("OVERALL:", "PASS" if verdict else "FAIL")
    json.dump({"verdict": verdict, "criteria": crit,
               "rss_growth_kb": rss_growth, "rss_slope_kb_per_h": round(rss_slope, 1),
               "fd_min": min(fds), "fd_max": max(fds), "fd_first": fd_first, "fd_last": fd_last,
               "threads": [min(thr), max(thr)],
               "rounds": lg["count"], "expected_rounds": round(expected),
               "max_gap_s": max_gap, "log_bytes": lg["log_bytes"],
               "errors": lg["errors"][:10]},
              open(os.path.join(outdir, "verdict.json"), "w"), indent=1)
    sys.exit(0 if verdict else 1)


if __name__ == "__main__":
    main()
