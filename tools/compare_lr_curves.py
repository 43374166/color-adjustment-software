#!/usr/bin/env python3
"""Compare the curve-panel reference set against fdxmp renders.

Layout mirrors tools/compare_lr_basic.py:
  <lr-root>/<image>_<control>/<original-name>
  <fd-root>/<image>_<control>.png

Usage:
  python tools/compare_lr_curves.py --lr-root .tmp/lr-ref-curves \
      --fd-root .tmp/fdxmp-ref-curves --out reports/lr_curves_compare.csv
"""

from __future__ import annotations

import argparse
import csv
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageOps

sys.path.insert(0, str(Path(__file__).resolve().parent))
from compare_lightroom import delta_e_2000, srgb_to_lab  # noqa: E402

IMAGES = {"landscape": "1.jpg", "portrait": "heng.jpg"}
CONTROLS = ["base"]
for _name in ("shadows", "darks", "lights", "highlights"):
    for _value in (-100, -50, 50, 100):
        CONTROLS.append(
            f"param_{_name}_m{abs(_value)}" if _value < 0 else f"param_{_name}_p{_value}"
        )
CONTROLS += [
    "point_s",
    "point_shadow_lift",
    "point_highlight_roll",
    "red_lift",
    "green_lift",
    "blue_lift",
]


def load(path: Path, size: tuple[int, int] | None = None) -> np.ndarray:
    with Image.open(path) as im:
        im = ImageOps.exif_transpose(im).convert("RGB")
        if size is not None and im.size != size:
            im = im.resize(size, Image.Resampling.LANCZOS)
        return np.asarray(im, dtype=np.float32) / 255.0


def luma(rgb: np.ndarray) -> np.ndarray:
    return 0.2126 * rgb[..., 0] + 0.7152 * rgb[..., 1] + 0.0722 * rgb[..., 2]


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--lr-root", type=Path, required=True)
    ap.add_argument("--fd-root", type=Path, required=True)
    ap.add_argument("--out", type=Path, required=True)
    args = ap.parse_args()

    rows = []
    for image, original_name in IMAGES.items():
        fd_base = load(args.fd_root / f"{image}_base.png")
        size = (fd_base.shape[1], fd_base.shape[0])
        lr_base = load(args.lr_root / f"{image}_base" / original_name, size)
        lb = luma(lr_base)
        fb = luma(fd_base)

        for control in CONTROLS:
            lr = (
                lr_base
                if control == "base"
                else load(args.lr_root / f"{image}_{control}" / original_name, size)
            )
            fd = (
                fd_base
                if control == "base"
                else load(args.fd_root / f"{image}_{control}.png")
            )
            if lr.shape != fd.shape:
                raise SystemExit(f"size mismatch {image}/{control}: {lr.shape} vs {fd.shape}")

            de = delta_e_2000(srgb_to_lab(lr), srgb_to_lab(fd))
            if control == "base":
                response_mae = response_bias = 0.0
                lr_strength = fd_strength = 0.0
                channel_mae = 0.0
            else:
                lr_response = luma(lr) - lb
                fd_response = luma(fd) - fb
                response_mae = float(np.mean(np.abs(lr_response - fd_response)) * 255.0)
                response_bias = float(np.mean(lr_response - fd_response) * 255.0)
                lr_strength = float(np.mean(np.abs(lr_response)) * 255.0)
                fd_strength = float(np.mean(np.abs(fd_response)) * 255.0)
                channel_mae = float(np.mean(np.abs((lr - lr_base) - (fd - fd_base))) * 255.0)

            rows.append(
                {
                    "image": image,
                    "control": control,
                    "de_mean": float(de.mean()),
                    "de_p95": float(np.percentile(de, 95.0)),
                    "response_mae": response_mae,
                    "response_bias": response_bias,
                    "lr_strength": lr_strength,
                    "fd_strength": fd_strength,
                    "channel_response_mae": channel_mae,
                }
            )

    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("w", newline="", encoding="utf-8") as file:
        writer = csv.DictWriter(file, fieldnames=list(rows[0].keys()))
        writer.writeheader()
        writer.writerows(rows)

    print(
        f"{'control':<26} {'dE':>6} {'resp_MAE':>9} {'bias':>7} "
        f"{'chan_MAE':>9} {'LR/FD':>12}"
    )
    for control in CONTROLS:
        subset = [row for row in rows if row["control"] == control]
        print(
            f"{control:<26} "
            f"{np.mean([row['de_mean'] for row in subset]):6.2f} "
            f"{np.mean([row['response_mae'] for row in subset]):9.2f} "
            f"{np.mean([row['response_bias'] for row in subset]):+7.2f} "
            f"{np.mean([row['channel_response_mae'] for row in subset]):9.2f} "
            f"{np.mean([row['lr_strength'] for row in subset]):5.1f}/"
            f"{np.mean([row['fd_strength'] for row in subset]):5.1f}"
        )
    print(f"\nwrote {args.out}")


if __name__ == "__main__":
    main()
