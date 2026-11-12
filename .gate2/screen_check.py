import os, pty, select, subprocess, sys, time, fcntl, termios, struct, json
import pyte
# 注意：pyte 回放对 CJK 宽字符/行尾单元格可能失真（F1 误判即源于此）。
# 判定 UI 是否截断请以真实终端人工复核为准。

BIN = sys.argv[1]; ARGS = sys.argv[2:]
master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
env = dict(os.environ, TERM="xterm-256color")
p = subprocess.Popen([BIN] + ARGS, stdin=slave, stdout=slave, stderr=slave,
                     env=env, close_fds=True, start_new_session=True)
os.close(slave)
scr = pyte.Screen(80, 24)
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

def screen_lines():
    return ["".join(scr.buffer[y][x].data for x in range(scr.columns)).rstrip()
            for y in range(scr.lines)]

pump(5.0)
zh = screen_lines()
os.write(master, b"l")
pump(1.5)
en = screen_lines()
os.write(master, b"q")
time.sleep(0.5)
try: code = p.wait(timeout=5)
except subprocess.TimeoutExpired: p.kill(); code = -9

def has(lines, *needles):
    return any(all(n in l for n in needles) for l in lines)

out = {
  "zh": zh, "en": en, "exit_code": code,
  "zh_footer_full": has(zh, "[s]", "扫描", "退出"),
  "en_footer_full": has(en, "[s]", "scan", "quit"),
  "zh_header": has(zh, "网卡"),
  "en_header": has(en, "iface:"),
  "device_row": has(zh, "a4:14:37:5f:4c:6d", "DS-2CD2T47G2-L"),
  "max_line_len_zh": max(len(l) for l in zh),
  "max_line_len_en": max(len(l) for l in en),
}
print(json.dumps(out, ensure_ascii=False, indent=1))
