#!/usr/bin/env python3
"""生成应用图标：assets/sadp-discover.png / .icns（macOS）/ .ico（Windows）。

图标语义：一块雷达屏上扫出一台设备——与「SADP 发现」对应。
所有尺寸都由一个 1024pt 的母版缩放，母版本身按 4x 超采样绘制后降采样，
避免 16pt 下出现锯齿。

用法（仓库根目录）：
    python3 packaging/make-icon.py
需要 Pillow；.icns 额外需要 macOS 自带的 iconutil（其它平台跳过这一步，
.ico 不受影响）。
"""

import shutil
import subprocess
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "assets"
SCALE = 4
SIZE = 1024
S = SIZE * SCALE

BG_TOP = (22, 50, 74)
BG_BOTTOM = (8, 14, 26)
RING = (74, 214, 196)
SWEEP = (86, 232, 210)
DEVICE = (240, 252, 250)
MARGIN = 100
RADIUS = 186


def lerp(a, b, t):
    return tuple(round(x + (y - x) * t) for x, y in zip(a, b))


def squircle_mask(size_px, margin, radius):
    """size_px 是像素尺寸；margin/radius 用 1024pt 的设计值，这里统一乘 SCALE。"""
    mask = Image.new("L", (size_px, size_px), 0)
    draw = ImageDraw.Draw(mask)
    draw.rounded_rectangle(
        (
            margin * SCALE,
            margin * SCALE,
            size_px - margin * SCALE,
            size_px - margin * SCALE,
        ),
        radius=radius * SCALE,
        fill=255,
    )
    return mask


def gradient(size, scale):
    img = Image.new("RGB", (1, size * scale))
    for y in range(size * scale):
        img.putpixel((0, y), lerp(BG_TOP, BG_BOTTOM, y / (size * scale - 1)))
    return img.resize((size * scale, size * scale))


def draw_icon():
    canvas = Image.new("RGBA", (S, S), (0, 0, 0, 0))

    # 投影：让图标在浅色背景上也有轮廓
    shadow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    sd = ImageDraw.Draw(shadow)
    pad = MARGIN * SCALE
    sd.rounded_rectangle(
        (pad, pad + 14 * SCALE, S - pad, S - pad + 14 * SCALE),
        radius=RADIUS * SCALE,
        fill=(0, 0, 0, 90),
    )
    canvas.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(24 * SCALE)))

    body = gradient(SIZE, SCALE).convert("RGBA")
    canvas.paste(body, (0, 0), squircle_mask(S, MARGIN, RADIUS))

    cx, cy = S // 2, S // 2 + 22 * SCALE

    # 雷达环：越外越淡
    rings = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    rd = ImageDraw.Draw(rings)
    for i, r in enumerate((92, 168, 244, 320)):
        alpha = 150 - i * 26
        width = max(2, int((5 - i) * SCALE))
        rd.ellipse(
            (cx - r * SCALE, cy - r * SCALE, cx + r * SCALE, cy + r * SCALE),
            outline=(*RING, alpha),
            width=width,
        )
    canvas.alpha_composite(rings)

    # 扫描扇区：用若干段递减透明度模拟拖尾
    sweep = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    sw = ImageDraw.Draw(sweep)
    steps = 18
    for i in range(steps):
        start = -90 + i * (70 / steps)
        alpha = int(110 * (1 - i / steps) ** 1.6)
        sw.pieslice(
            (cx - 320 * SCALE, cy - 320 * SCALE, cx + 320 * SCALE, cy + 320 * SCALE),
            start=start,
            end=start + 70 / steps + 0.6,
            fill=(*SWEEP, alpha),
        )
    canvas.alpha_composite(sweep.filter(ImageFilter.GaussianBlur(1.2 * SCALE)))

    # 折角上的设备点：三台设备，越远越暗
    dots = ((196, -34, 1.0), (248, 26, 0.75), (150, 42, 0.6))
    for dist, deg, strength in dots:
        import math

        rad = math.radians(deg)
        x = cx + math.cos(rad) * dist * SCALE
        y = cy + math.sin(rad) * dist * SCALE
        glow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
        gd = ImageDraw.Draw(glow)
        gd.ellipse(
            (x - 26 * SCALE, y - 26 * SCALE, x + 26 * SCALE, y + 26 * SCALE),
            fill=(*SWEEP, int(70 * strength)),
        )
        canvas.alpha_composite(glow.filter(ImageFilter.GaussianBlur(10 * SCALE)))
        dot = Image.new("RGBA", (S, S), (0, 0, 0, 0))
        dd = ImageDraw.Draw(dot)
        r = 13 * SCALE
        dd.ellipse((x - r, y - r, x + r, y + r), fill=(*DEVICE, int(255 * strength)))
        canvas.alpha_composite(dot)

    # 原点
    origin = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    od = ImageDraw.Draw(origin)
    od.ellipse(
        (cx - 9 * SCALE, cy - 9 * SCALE, cx + 9 * SCALE, cy + 9 * SCALE),
        fill=(*DEVICE, 255),
    )
    canvas.alpha_composite(origin)

    return canvas.resize((SIZE, SIZE), Image.LANCZOS)


# Windows 的 .ico 只需要这些尺寸：16/32 用于列表，48 用于桌面，256 用于大图标视图。
ICO_SIZES = (16, 24, 32, 48, 64, 128, 256)

ICONSET = [
    (16, "icon_16x16.png"),
    (32, "icon_16x16@2x.png"),
    (32, "icon_32x32.png"),
    (64, "icon_32x32@2x.png"),
    (128, "icon_128x128.png"),
    (256, "icon_128x128@2x.png"),
    (256, "icon_256x256.png"),
    (512, "icon_256x256@2x.png"),
    (512, "icon_512x512.png"),
    (1024, "icon_512x512@2x.png"),
]


def main():
    ASSETS.mkdir(parents=True, exist_ok=True)
    master = draw_icon()
    png = ASSETS / "sadp-discover.png"
    master.save(png)
    print(f"wrote {png}")

    ico = ASSETS / "sadp-discover.ico"
    master.save(ico, sizes=[(px, px) for px in ICO_SIZES])
    print(f"wrote {ico}")

    iconset = ROOT / "packaging" / "AppIcon.iconset"
    if iconset.exists():
        shutil.rmtree(iconset)
    iconset.mkdir(parents=True)
    for px, name in ICONSET:
        master.resize((px, px), Image.LANCZOS).save(iconset / name)

    if shutil.which("iconutil") is None:
        print("iconutil 不存在（非 macOS），跳过 .icns 生成", file=sys.stderr)
        return
    icns = ASSETS / "sadp-discover.icns"
    subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(icns)], check=True)
    print(f"wrote {icns}")


if __name__ == "__main__":
    main()
