import os, pty, select, subprocess, sys, time, fcntl, termios, struct, re, json, glob

BIN = sys.argv[1]
ARGS = sys.argv[2:]

master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
env = dict(os.environ, TERM="xterm-256color")
p = subprocess.Popen([BIN] + ARGS, stdin=slave, stdout=slave, stderr=slave,
                     env=env, close_fds=True, start_new_session=True)
os.close(slave)

buf = b""

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

ansi = re.compile(rb"\x1b\][^\x07]*\x07|\x1b\[[0-9;?]*[a-zA-Z]|\x1b[()][A-Za-z0-9]|\x1b[=>]")
def text(b):
    return ansi.sub(b"", b).decode("utf-8", "replace")

pump(4.5)
a = text(buf)
buf = b""
os.write(master, b"l")
pump(1.5)
b = text(buf)
buf = b""
os.write(master, b"e")
pump(1.5)
c = text(buf)
buf = b""
os.write(master, b"c")
pump(1.5)
d = text(buf)
buf = b""
os.write(master, b"q")
pump(1.5)
try:
    code = p.wait(timeout=5)
except subprocess.TimeoutExpired:
    p.kill(); code = -9

print(json.dumps({
    "first_screen_excerpt": a[-600:],
    "lang_toggle_excerpt": b[-600:],
    "export_excerpt": c[-400:],
    "copy_excerpt": d[-400:],
    "exit_code": code,
}, ensure_ascii=False))
