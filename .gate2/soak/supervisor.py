#!/usr/bin/env python3
"""24h soak supervisor: runs the TUI in a pty, samples RSS/FD/threads, writes CSV.

Usage:
  python3 supervisor.py --duration 86400 --interval 60 --outdir .gate2/soak-24h
"""
import argparse, csv, datetime, glob, json, os, pty, re, select, signal, struct, subprocess, sys, termios, time, fcntl

ANSI = re.compile(rb"\x1b\][^\x07]*\x07|\x1b\[[0-9;?]*[a-zA-Z]|\x1b[()][A-Za-z0-9]|\x1b[=>]")


def iso(ts):
    return datetime.datetime.fromtimestamp(ts).astimezone().isoformat(timespec="seconds")


def sh(cmd):
    try:
        return subprocess.run(["sh", "-c", cmd], capture_output=True, text=True, timeout=15).stdout
    except Exception:
        return ""


def sample(pid, log_glob):
    out = sh("ps -o rss=,vsz=,%%cpu=,etime= -p %d" % pid).split()
    if len(out) < 4:
        return None
    rss, vsz, cpu, etime = out[0], out[1], out[2], out[3]
    threads = max(int(sh("ps -M -p %d | wc -l" % pid).strip() or 1) - 1, 0)
    fds = max(int(sh("lsof -p %d 2>/dev/null | wc -l" % pid).strip() or 1) - 1, 0)
    socks = max(int(sh("lsof -p %d -i 2>/dev/null | wc -l" % pid).strip() or 1) - 1, 0)
    logb = 0
    for f in glob.glob(log_glob):
        try:
            logb += os.path.getsize(f)
        except OSError:
            pass
    return {"rss_kb": int(rss), "vsz_kb": int(vsz), "cpu": float(cpu.replace(",", ".")),
            "etime": etime, "threads": threads, "fds": fds, "sockets": socks, "log_bytes": logb}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--duration", type=int, default=86400)
    ap.add_argument("--interval", type=int, default=60)
    ap.add_argument("--outdir", required=True)
    ap.add_argument("--bin", default="./target/release/sadp-discover")
    ap.add_argument("--fake", default="./target/debug/sadp-fake-device")
    ap.add_argument("--repeat", default="1000")
    ap.add_argument("--no-fake", action="store_true")
    args = ap.parse_args()

    os.makedirs(args.outdir, exist_ok=True)
    t0 = time.time()
    fake = None
    if not args.no_fake:
        fake = subprocess.Popen(
            [args.fake, "--duration", str(args.duration + 900)],
            stdout=open(os.path.join(args.outdir, "fake.log"), "w"),
            stderr=subprocess.STDOUT)

    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
    env = dict(os.environ, TERM="xterm-256color")
    cmd = [args.bin, "--repeat", args.repeat]
    if not args.no_fake:
        cmd += ["--local-port", "0"]
    cmd += ["-vv"]
    proc = subprocess.Popen(cmd, stdin=slave, stdout=slave, stderr=slave,
                            env=env, close_fds=True, start_new_session=True)
    os.close(slave)
    print("scanner pid=%d cmd=%s" % (proc.pid, " ".join(cmd)), flush=True)

    log_glob = os.path.expanduser("~/Library/Application Support/sadp-discover/logs/sadp-discover.log.*")
    csv_path = os.path.join(args.outdir, "samples.csv")
    fields = ["t_s", "time", "rss_kb", "vsz_kb", "cpu", "etime", "threads", "fds", "sockets", "log_bytes"]
    fh = open(csv_path, "w", newline="")
    writer = csv.DictWriter(fh, fieldnames=fields)
    writer.writeheader()

    screen = bytearray()
    next_sample = t0
    exit_code = None
    stopped_reason = "duration"

    def drain(timeout):
        nonlocal screen
        r, _, _ = select.select([master], [], [], timeout)
        if not r:
            return
        try:
            chunk = os.read(master, 1 << 16)
        except OSError:
            return
        if not chunk:
            return
        screen += chunk
        if len(screen) > 400_000:
            del screen[:200_000]

    while time.time() < t0 + args.duration:
        drain(0.2)
        if proc.poll() is not None:
            stopped_reason = "scanner-exited-early"
            exit_code = proc.returncode
            break
        now = time.time()
        if now >= next_sample:
            m = sample(proc.pid, log_glob)
            if m is not None:
                m.update({"t_s": round(now - t0, 1), "time": iso(now)})
                writer.writerow(m)
                fh.flush()
            next_sample = now + args.interval
    else:
        pass

    # graceful quit
    if proc.poll() is None:
        try:
            os.write(master, b"q")
        except OSError:
            pass
        end = time.time() + 15
        while time.time() < end and proc.poll() is None:
            drain(0.2)
        if proc.poll() is None:
            proc.kill()
            stopped_reason = "had-to-kill"
    exit_code = proc.poll() if exit_code is None else exit_code

    if fake is not None and fake.poll() is None:
        fake.terminate()
    fh.close()

    final = ANSI.sub(b"", bytes(screen)).decode("utf-8", "replace")
    with open(os.path.join(args.outdir, "tui-final.txt"), "w") as f:
        f.write(final[-4000:])
    with open(os.path.join(args.outdir, "meta.json"), "w") as f:
        json.dump({"start": iso(t0), "end": iso(time.time()), "duration_s": round(time.time() - t0, 1),
                   "exit_code": exit_code, "stopped_reason": stopped_reason,
                   "repeat_ms": args.repeat, "fake": not args.no_fake}, f, indent=1)
    print(json.dumps({"exit_code": exit_code, "stopped_reason": stopped_reason,
                      "elapsed_s": round(time.time() - t0, 1)}))
    sys.exit(0)


if __name__ == "__main__":
    main()
