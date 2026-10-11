#!/usr/bin/env python3
"""从 assets/图标.png 生成 iOS 应用图标（去掉外圈白底）。

背景是接近纯白的画布，图标本体（浅色圆角卡片 + 阴影）只占中间约 826×826；
这里自动找出非白内容的包围盒，裁掉白边后缩放到 1024×1024，输出：

- assets/icon-1024.png                     （处理后的主文件，RGB 无 alpha）
- platforms/ios/Fdxmp/Assets.xcassets/AppIcon.appiconset/icon-1024.png

ios AppIcon 用单尺寸（1024）即可，其余尺寸由 Xcode 生成。
依赖：Pillow（本机 pip 已装）。
"""

from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "assets" / "图标.png"
MASTER = ROOT / "assets" / "icon-1024.png"
APPICON = (
    ROOT
    / "platforms"
    / "ios"
    / "Fdxmp"
    / "Assets.xcassets"
    / "AppIcon.appiconset"
    / "icon-1024.png"
)

# 外圈白底约为 254；小于该阈值的内容视为图标本体（含阴影）。
WHITE_THRESHOLD = 246


def find_content_box(image: Image.Image) -> tuple[int, int, int, int]:
    """返回非白内容的包围盒（left, top, right, bottom；右 / 下为开区间）。"""
    width, height = image.size
    pixels = image.load()
    min_x, min_y, max_x, max_y = width, height, -1, -1
    for y in range(height):
        for x in range(width):
            r, g, b = pixels[x, y]
            if min(r, g, b) <= WHITE_THRESHOLD:
                if x < min_x:
                    min_x = x
                if y < min_y:
                    min_y = y
                if x > max_x:
                    max_x = x
                if y > max_y:
                    max_y = y
    if max_x < 0 or max_y < 0:
        raise SystemExit("没有找到非白内容，检查源图")
    return min_x, min_y, max_x + 1, max_y + 1


def main() -> None:
    source = Image.open(SOURCE).convert("RGB")
    left, top, right, bottom = find_content_box(source)
    box_width = right - left
    box_height = bottom - top
    print(f"源图 {source.size}，内容包围盒 ({left}, {top}, {right}, {bottom}) {box_width}×{box_height}")

    # 裁掉白边 → 正方形 → 1024。
    icon = source.crop((left, top, right, bottom))
    icon = icon.resize((1024, 1024), Image.LANCZOS)
    MASTER.parent.mkdir(parents=True, exist_ok=True)
    APPICON.parent.mkdir(parents=True, exist_ok=True)
    # AppIcon 不允许透明通道：直接存 RGB。
    icon.save(MASTER, optimize=True)
    icon.save(APPICON, optimize=True)
    print(f"已生成：{MASTER}")
    print(f"已生成：{APPICON}")


if __name__ == "__main__":
    main()
