import os, pty, select, subprocess, sys, time, fcntl, termios, struct, json
import pyte
# 注意：pyte 回放对 CJK 宽字符/行尾单元格可能失真（F1 误判即源于此）。
# 判定 UI 是否截断请以真实终端人工复核为准。

BIN = sys.argv[1]; W = int(sys.argv[2]); H = int(sys.argv[3]); ARGS = sys.argv[4:]
master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", H, W, 0, 0))
env = dict(os.environ, TERM="xterm-256color")
p = subprocess.Popen([BIN] + ARGS, stdin=slave, stdout=slave, stderr=slave,
                     env=env, close_fds=True, start_new_session=True)
os.close(slave)
scr = pyte.Screen(W, H)
stream = pyte.Stream(scr)
def pump(t):
    end = time.time() + t
    while time.time() < end:
        r, _, _ = select.select([master], [], [], 0.05)
        if r:
            try: chunk = os.read(master, 65536)
            except OSError: return
            if not chunk: return
            stream.feed(chunk.decode("utf-8", "replace"))
def lines():
    return ["".join(scr.buffer[y][x].data for x in range(scr.columns)).rstrip()
            for y in range(scr.lines)]
pump(5.0)
zh = lines()
os.write(master, b"q"); time.sleep(0.5)
try: code = p.wait(timeout=5)
except subprocess.TimeoutExpired: p.kill(); code = -9
print(json.dumps({"w": W, "h": H, "exit": code,
                  "has_model_value": any("DS-2CD2T47G2-L" in l for l in zh),
                  "detail_lines": [l for l in zh if "型号" in l or "Model" in l]}, ensure_ascii=False))
