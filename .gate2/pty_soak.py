import os, pty, select, subprocess, sys, time, fcntl, termios, struct, re, json

BIN = sys.argv[1]; ARGS = sys.argv[2:-1]; DURATION = float(sys.argv[-1])
master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
env = dict(os.environ, TERM="xterm-256color")
p = subprocess.Popen([BIN] + ARGS, stdin=slave, stdout=slave, stderr=slave,
                     env=env, close_fds=True, start_new_session=True)
os.close(slave)
def drain():
    while True:
        r, _, _ = select.select([master], [], [], 0)
        if not r: return
        try:
            if not os.read(master, 65536): return
        except OSError:
            return
def metrics():
    rss = subprocess.run(["ps", "-o", "rss=", "-p", str(p.pid)], capture_output=True, text=True).stdout.strip()
    fds = subprocess.run(["sh", "-c", "lsof -p %d 2>/dev/null | wc -l" % p.pid], capture_output=True, text=True).stdout.strip()
    return {"t": round(time.time() - t0, 1), "rss_kb": int(rss or 0), "fds": int(fds or 0)}
t0 = time.time()
samples = []
while time.time() - t0 < DURATION:
    time.sleep(5)
    drain()
    samples.append(metrics())
drain()
os.write(master, b"q")
time.sleep(1)
try:
    code = p.wait(timeout=5)
except subprocess.TimeoutExpired:
    p.kill(); code = -9
print(json.dumps({"samples": samples, "exit_code": code}, ensure_ascii=False))
