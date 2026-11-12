import os, pty, select, subprocess, time, fcntl, termios, struct, re

master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
env = dict(os.environ, TERM="xterm-256color")
p = subprocess.Popen(["./target/release/sadp-discover", "--local-port", "0", "--timeout", "3000"],
                     stdin=slave, stdout=slave, stderr=slave, env=env,
                     close_fds=True, start_new_session=True)
os.close(slave)
buf = b""
end = time.time() + 5
while time.time() < end:
    r, _, _ = select.select([master], [], [], 0.1)
    if r:
        try: c = os.read(master, 65536)
        except OSError: break
        if not c: break
        buf += c
os.write(master, b"q"); time.sleep(0.5)
try: p.wait(timeout=5)
except Exception: p.kill()

label = "序列号".encode()
print("序列号 occurrences:", buf.count(label))
for m in re.finditer(re.escape(label), buf):
    i = m.start()
    print("--- 序列号 at", i, "---")
    print(repr(buf[i-20:i+200]))
print("---- model contexts ----")
for m in re.finditer(b"DS-2CD2T47G2-L", buf):
    i = m.start()
    print("--- model at", i, "---")
    print(repr(buf[max(0,i-160):i+80]))
