import os, pty, select, subprocess, sys, time, fcntl, termios, struct, re, json

BIN = sys.argv[1]
ARGS = sys.argv[2:]

master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
env = dict(os.environ, TERM="xterm-256color")
p = subprocess.Popen([BIN] + ARGS, stdin=slave, stdout=slave, stderr=slave,
                     env=env, close_fds=True, start_new_session=True)
os.close(slave)

buf = b""
ansi = re.compile(rb"\x1b\][^\x07]*\x07|\x1b\[[0-9;?]*[a-zA-Z]|\x1b[()][A-Za-z0-9]|\x1b[=>]")

def pump(t):
    global buf
    end = time.time() + t
    while time.time() < end:
        r, _, _ = select.select([master], [], [], 0.1)
        if not r:
            continue
        try:
            chunk = os.read(master, 65536)
        except OSError:
            return
        if not chunk:
            return
        buf += chunk

def take():
    global buf
    t = ansi.sub(b"", buf).decode("utf-8", "replace")
    buf = b""
    return t

def squash(s):
    return s.replace(" ", "").replace("\u3000", "")

def metrics(pid):
    rss = subprocess.run(["ps", "-o", "rss=", "-p", str(pid)],
                         capture_output=True, text=True).stdout.strip()
    fds = subprocess.run(["sh", "-c", "lsof -p %d 2>/dev/null | wc -l" % pid],
                         capture_output=True, text=True).stdout.strip()
    return int(rss or 0), int(fds or 0)

pump(4.0)
zh = take()
m1 = metrics(p.pid)

os.write(master, b"l")
pump(1.5)
en = take()

os.write(master, b"e")
pump(1.5)
exp = take()

os.write(master, b"c")
pump(1.5)
cop = take()

pump(25.0)
m2 = metrics(p.pid)
later = take()

os.write(master, b"q")
pump(2.0)
try:
    code = p.wait(timeout=5)
except subprocess.TimeoutExpired:
    p.kill()
    code = -9

out = {
    "zh_has_header": "网卡" in squash(zh),
    "zh_has_footer": "扫描" in squash(zh) and "退出" in squash(zh),
    "en_has_header": "iface:" in squash(en),
    "en_has_footer": "quit" in squash(en) and "export" in squash(en),
    "export_status_en": "Exportedto" in squash(exp),
    "copy_status_en": "Copiedtoclipboard" in squash(cop),
    "soak_screen_ok": "device" in squash(later) or "设备" in squash(later),
    "rss_kb_start": m1[0], "rss_kb_end": m2[0],
    "fds_start": m1[1], "fds_end": m2[1],
    "exit_code": code,
    "zh_excerpt": squash(zh)[-260:],
    "en_excerpt": squash(en)[-260:],
    "exp_excerpt": squash(exp)[-160:],
    "cop_excerpt": squash(cop)[-160:],
}
print(json.dumps(out, ensure_ascii=False))
