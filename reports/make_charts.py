# -*- coding: utf-8 -*-
"""Generate charts for the fdxmp / LightCraft / darktable comparison report.

Pure Pillow: the bundled Python runtime does not include matplotlib.
"""
from __future__ import annotations

import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

OUT = Path(__file__).resolve().parent / "assets"
OUT.mkdir(parents=True, exist_ok=True)

FONT_CN = r"C:\Windows\Fonts\msyh.ttc"
FONT_CN_BOLD = r"C:\Windows\Fonts\msyhbd.ttc"

INK = (30, 41, 59)
MUTED = (100, 116, 139)
GRID = (226, 232, 240)
BG = (255, 255, 255)

AMBER = (217, 119, 6)
AMBER_LIGHT = (253, 230, 200)
TEAL = (15, 118, 110)
TEAL_LIGHT = (204, 251, 241)
SLATE = (51, 65, 85)
SLATE_LIGHT = (203, 213, 225)
RED = (185, 28, 28)


def font(size: int, bold: bool = False) -> ImageFont.FreeTypeFont:
    return ImageFont.truetype(FONT_CN_BOLD if bold else FONT_CN, size)


def text(draw: ImageDraw.ImageDraw, xy, content, size=28, color=INK, bold=False, anchor="la"):
    draw.text(xy, content, font=font(size, bold), fill=color, anchor=anchor)


def rounded_bar(draw: ImageDraw.ImageDraw, box, radius, fill):
    draw.rounded_rectangle(box, radius=radius, fill=fill)


def chart_scale() -> Path:
    W, H = 1600, 900
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    text(d, (80, 55), "代码规模对比", 46, INK, True)
    text(d, (80, 120), "静态统计的源码行数（kLOC，含测试；不同语言不可直接等价比较）", 24, MUTED)

    rows = [
        ("fdxmp · Rust 核心", 34.3, AMBER),
        ("fdxmp · 移动端 UI/桥接", 21.3, AMBER_LIGHT),
        ("LightCraft · Rust", 316.1, TEAL),
        ("darktable · C / C++", 531.9, SLATE),
    ]
    max_v = 600
    left, right = 420, 1460
    top, gap, bar_h = 220, 118, 58

    # grid
    for tick in range(0, 601, 100):
        x = left + (right - left) * tick / max_v
        d.line([(x, 190), (x, 720)], fill=GRID, width=2)
        text(d, (x, 738), f"{tick}", 22, MUTED, anchor="ma")

    for i, (label, value, color) in enumerate(rows):
        y = top + i * gap
        text(d, (left - 32, y + bar_h / 2), label, 27, INK, anchor="rm")
        w = max(4, (right - left) * value / max_v)
        rounded_bar(d, (left, y, left + w, y + bar_h), 12, color)
        value_color = INK if color != AMBER_LIGHT else (146, 64, 14)
        text(d, (left + w + 18, y + bar_h / 2), f"{value:.1f}k", 28, value_color, True, "lm")

    text(d, (80, 835), "口径：统计 *.rs / *.c / *.h 行数；fdxmp Rust 仅 *.rs，移动端为 Swift/Kotlin/ArkTS；"
                       "darktable 不含 rawspeed、LibRaw 等子模块。", 20, MUTED)
    path = OUT / "fig_scale.png"
    img.save(path, quality=95)
    return path


def chart_timeline() -> Path:
    W, H = 1600, 820
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    text(d, (80, 50), "三个项目，三个代际", 46, INK, True)
    text(d, (80, 112), "同一类目标（非破坏 RAW 编辑），完全不同的起点、语言与工程阶段", 24, MUTED)

    cards = [
        {
            "x": 80, "color": SLATE, "name": "darktable", "date": "2012-03 创建",
            "age": "14 年", "version": "稳定版 5.6.2",
            "lines": ["C / GTK3 / OpenCL", "531.9k 行 C/H · 97 个模块", "13.3k stars · 1,495 forks"],
            "tag": "成熟专业工作流",
        },
        {
            "x": 560, "color": TEAL, "name": "LightCraft", "date": "2026-09-30 创建",
            "age": "25 天", "version": "v0.13.0",
            "lines": ["纯 Rust / wgpu / egui", "316.1k 行 Rust · 29 个 crate", "14.6k stars · 1,059 open issues"],
            "tag": "现代全流程 alpha",
        },
        {
            "x": 1040, "color": AMBER, "name": "fdxmp", "date": "2026-10-09 创建",
            "age": "15 天", "version": "v1.6.1",
            "lines": ["Rust / wgpu / 四端原生 UI", "34.3k 行 Rust + 21.3k 行移动端", "89 commits · 私有仓库"],
            "tag": "移动端 + AI 修图",
        },
    ]

    card_w, card_h, top = 440, 440, 190
    for card in cards:
        x = card["x"]
        d.rounded_rectangle([x, top, x + card_w, top + card_h], radius=18, fill=(248, 250, 252), outline=(226, 232, 240), width=2)
        d.rounded_rectangle([x, top, x + card_w, top + 12], radius=6, fill=card["color"])
        text(d, (x + 30, top + 34), card["name"], 36, card["color"], True)
        text(d, (x + 30, top + 88), card["date"], 23, MUTED)
        # age badge
        badge_w = 120 if card["age"] == "14 年" else 96
        d.rounded_rectangle([x + card_w - badge_w - 24, top + 40, x + card_w - 24, top + 82], radius=20, fill=card["color"])
        text(d, (x + card_w - badge_w / 2 - 24, top + 61), card["age"], 23, (255, 255, 255), True, "mm")
        text(d, (x + 30, top + 140), card["version"], 27, INK, True)
        for i, line in enumerate(card["lines"]):
            text(d, (x + 30, top + 200 + i * 38), line, 22, INK)
        # tag
        d.rounded_rectangle([x + 30, top + 350, x + card_w - 30, top + 400], radius=10, fill=(255, 255, 255), outline=card["color"], width=2)
        text(d, (x + card_w / 2, top + 375), card["tag"], 23, card["color"], True, "mm")

    # time arrow
    d.line([(110, 720), (1440, 720)], fill=(203, 213, 225), width=6)
    d.polygon([(1440, 720), (1412, 704), (1412, 736)], fill=(203, 213, 225))
    text(d, (110, 748), "2012", 22, MUTED)
    text(d, (1370, 748), "2026", 22, MUTED)
    text(d, (760, 690), "时间 →", 22, MUTED, True, "ma")

    text(d, (80, 780), "数据：GitHub 仓库创建/发布日期、本地 git 历史与 2026-10-25 快照；代码行数含测试。",
         19, MUTED)
    path = OUT / "fig_timeline.png"
    img.save(path, quality=95)
    return path
    path = OUT / "fig_timeline.png"
    img.save(path, quality=95)
    return path


def radar() -> Path:
    W, H = 1500, 1100
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    text(d, (80, 50), "能力雷达（0–5，定性评估）", 46, INK, True)
    text(d, (80, 112), "分数为本报告基于代码、文档与公开数据的评估，不是厂商指标", 24, MUTED)

    axes = ["成熟稳定", "功能广度", "画质深度", "性能", "移动/跨平台", "工程与生态", "许可友好"]
    data = {
        "fdxmp": ([2.0, 3.5, 2.5, 3.5, 4.5, 1.8, 5.0], AMBER),
        "LightCraft": ([2.5, 4.5, 3.5, 4.5, 4.0, 3.8, 5.0], TEAL),
        "darktable": ([5.0, 5.0, 5.0, 3.5, 2.0, 5.0, 1.5], SLATE),
    }

    cx, cy, radius = 760, 610, 330
    n = len(axes)
    angles = [-math.pi / 2 + 2 * math.pi * i / n for i in range(n)]

    for ring in range(1, 6):
        pts = [
            (cx + radius * ring / 5 * math.cos(a), cy + radius * ring / 5 * math.sin(a))
            for a in angles
        ]
        d.polygon(pts, outline=GRID)
        if ring in (1, 2, 3, 4, 5):
            text(d, (cx + 8, cy - radius * ring / 5 + 2), str(ring), 18, MUTED)

    for i, (a, label) in enumerate(zip(angles, axes)):
        x = cx + radius * 1.17 * math.cos(a)
        y = cy + radius * 1.17 * math.sin(a)
        anchor = "mm"
        if abs(math.cos(a)) > 0.8:
            anchor = "lm" if math.cos(a) > 0 else "rm"
        elif math.sin(a) < -0.5:
            anchor = "mb"
        elif math.sin(a) > 0.5:
            anchor = "mt"
        text(d, (x, y), label, 25, INK, True, anchor)
        d.line([(cx, cy), (cx + radius * math.cos(a), cy + radius * math.sin(a))], fill=GRID, width=2)

    overlay = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    od = ImageDraw.Draw(overlay)
    for name, (values, color) in data.items():
        pts = [
            (cx + radius * v / 5 * math.cos(a), cy + radius * v / 5 * math.sin(a))
            for v, a in zip(values, angles)
        ]
        od.polygon(pts, fill=color + (42,), outline=color + (255,))
        od.line(pts + [pts[0]], fill=color + (255,), width=4)
        for p in pts:
            od.ellipse([(p[0] - 5, p[1] - 5), (p[0] + 5, p[1] + 5)], fill=color + (255,))
    img = Image.alpha_composite(img.convert("RGBA"), overlay).convert("RGB")
    d = ImageDraw.Draw(img)

    # legend
    lx, ly = 150, 950
    for i, (name, (values, color)) in enumerate(data.items()):
        y = ly + i * 42
        d.rounded_rectangle([lx, y, lx + 28, y + 16], radius=4, fill=color)
        text(d, (lx + 40, y - 5), name, 22, INK)

    path = OUT / "fig_radar.png"
    img.save(path, quality=95)
    return path


if __name__ == "__main__":
    for p in (chart_scale(), chart_timeline(), radar()):
        print(p)
