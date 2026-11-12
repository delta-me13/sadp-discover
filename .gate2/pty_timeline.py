import os, pty, select, subprocess, sys, time, fcntl, termios, struct, re, json

BIN = sys.argv[1]; ARGS = sys.argv[2:]
master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
env = dict(os.environ, TERM="xterm-256color")
t0 = time.time()
p = subprocess.Popen([BIN] + ARGS, stdin=slave, stdout=slave, stderr=slave,
                     env=env, close_fds=True, start_new_session=True)
os.close(slave)
ansi = re.compile(rb"\x1b\][^\x07]*\x07|\x1b\[[0-9;?]*[a-zA-Z]|\x1b[()][A-Za-z0-9]|\x1b[=>]")
buf = b""; t_title = t_dev = None; seen = ""
CHECK_MAC = os.environ.get("CHECK_MAC", "a4:14:37:5f:4c:6d").lower()
def squash(s): return s.replace(" ", "")
deadline = time.time() + 10
while time.time() < deadline:
    r, _, _ = select.select([master], [], [], 0.05)
    if r:
        try: chunk = os.read(master, 65536)
        except OSError: break
        if not chunk: break
        buf += chunk
    txt = squash(ansi.sub(b"", buf).decode("utf-8", "replace"))
    seen = txt
    if t_title is None and "sadp-discover" in txt: t_title = time.time() - t0
    if t_dev is None and CHECK_MAC in txt.lower():
        t_dev = time.time() - t0
        break
time.sleep(0.5)
r, _, _ = select.select([master], [], [], 0)
if r:
    try: buf += os.read(master, 65536)
    except OSError: pass
final = squash(ansi.sub(b"", buf).decode("utf-8", "replace"))
os.write(master, b"q")
code = None
end = time.time() + 8
while time.time() < end:
    code = p.poll()
    if code is not None: break
    time.sleep(0.05)
if code is None: p.kill()
print(json.dumps({"t_first_frame_s": round(t_title,3) if t_title else None,
                  "t_first_device_s": round(t_dev,3) if t_dev else None,
                  "device_on_screen": CHECK_MAC in final.lower(),
                  "final_excerpt": final[-220:],
                  "exit_code": code}, ensure_ascii=False))
