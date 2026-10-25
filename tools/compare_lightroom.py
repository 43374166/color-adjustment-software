#!/usr/bin/env python3
"""Compare a rendered image with a Lightroom reference using CIEDE2000.

Usage:
  python tools/compare_lightroom.py REF TEST --out report.json --heatmap heat.png

REF and TEST must be the same scene. Different sizes are resized to REF.
No OpenCV / colour-science dependency: numpy + Pillow only.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import numpy as np
from PIL import Image, ImageOps

# sRGB (IEC 61966-2-1) -> XYZ D65, then XYZ -> CIELAB D65.
RGB_TO_XYZ = np.array(
    [
        [0.4124564, 0.3575761, 0.1804375],
        [0.2126729, 0.7151522, 0.0721750],
        [0.0193339, 0.1191920, 0.9503041],
    ],
    dtype=np.float64,
)
WHITE_D65 = np.array([0.95047, 1.00000, 1.08883], dtype=np.float64)


def srgb_to_linear(rgb: np.ndarray) -> np.ndarray:
    rgb = np.clip(rgb.astype(np.float64), 0.0, 1.0)
    return np.where(rgb <= 0.04045, rgb / 12.92, ((rgb + 0.055) / 1.055) ** 2.4)


def linear_to_lab(linear: np.ndarray) -> np.ndarray:
    xyz = linear @ RGB_TO_XYZ.T
    xyz = xyz / WHITE_D65
    delta = 6.0 / 29.0
    f = np.where(xyz > delta**3, np.cbrt(xyz), xyz / (3 * delta**2) + 4.0 / 29.0)
    return np.stack(
        [116.0 * f[..., 1] - 16.0, 500.0 * (f[..., 0] - f[..., 1]), 200.0 * (f[..., 1] - f[..., 2])],
        axis=-1,
    )


def srgb_to_lab(rgb: np.ndarray) -> np.ndarray:
    return linear_to_lab(srgb_to_linear(rgb))


def delta_e_2000(lab1: np.ndarray, lab2: np.ndarray) -> np.ndarray:
    """CIEDE2000, vectorized; Sharma et al. (2005) formulation."""
    L1, a1, b1 = np.moveaxis(lab1, -1, 0)
    L2, a2, b2 = np.moveaxis(lab2, -1, 0)

    C1 = np.hypot(a1, b1)
    C2 = np.hypot(a2, b2)
    Cbar = 0.5 * (C1 + C2)
    G = 0.5 * (1.0 - np.sqrt(Cbar**7 / (Cbar**7 + 25.0**7)))
    a1p = (1.0 + G) * a1
    a2p = (1.0 + G) * a2
    C1p = np.hypot(a1p, b1)
    C2p = np.hypot(a2p, b2)

    h1p = np.degrees(np.arctan2(b1, a1p)) % 360.0
    h2p = np.degrees(np.arctan2(b2, a2p)) % 360.0
    h1p = np.where(C1p < 1e-12, 0.0, h1p)
    h2p = np.where(C2p < 1e-12, 0.0, h2p)

    dLp = L2 - L1
    dCp = C2p - C1p
    dhp = h2p - h1p
    dhp = np.where(dhp > 180.0, dhp - 360.0, dhp)
    dhp = np.where(dhp < -180.0, dhp + 360.0, dhp)
    dhp = np.where((C1p * C2p) < 1e-12, 0.0, dhp)
    dHp = 2.0 * np.sqrt(C1p * C2p) * np.sin(np.radians(dhp) / 2.0)

    Lbarp = 0.5 * (L1 + L2)
    Cbarp = 0.5 * (C1p + C2p)
    hsum = h1p + h2p
    hdiff = np.abs(h1p - h2p)
    hbarp = np.where(
        (C1p * C2p) < 1e-12,
        hsum,
        np.where(
            hdiff <= 180.0,
            hsum / 2.0,
            np.where(hsum < 360.0, (hsum + 360.0) / 2.0, (hsum - 360.0) / 2.0),
        ),
    )

    T = (
        1.0
        - 0.17 * np.cos(np.radians(hbarp - 30.0))
        + 0.24 * np.cos(np.radians(2.0 * hbarp))
        + 0.32 * np.cos(np.radians(3.0 * hbarp + 6.0))
        - 0.20 * np.cos(np.radians(4.0 * hbarp - 63.0))
    )
    dtheta = 30.0 * np.exp(-(((hbarp - 275.0) / 25.0) ** 2))
    RC = 2.0 * np.sqrt(Cbarp**7 / (Cbarp**7 + 25.0**7))
    SL = 1.0 + (0.015 * (Lbarp - 50.0) ** 2) / np.sqrt(20.0 + (Lbarp - 50.0) ** 2)
    SC = 1.0 + 0.045 * Cbarp
    SH = 1.0 + 0.015 * Cbarp * T
    RT = -np.sin(np.radians(2.0 * dtheta)) * RC

    return np.sqrt(
        (dLp / SL) ** 2
        + (dCp / SC) ** 2
        + (dHp / SH) ** 2
        + RT * (dCp / SC) * (dHp / SH)
    )


def load_rgb(path: Path, max_edge: int = 2048) -> np.ndarray:
    with Image.open(path) as im:
        im = ImageOps.exif_transpose(im).convert("RGB")
        if max_edge > 0 and max(im.size) > max_edge:
            scale = max_edge / max(im.size)
            im = im.resize(
                (max(1, round(im.width * scale)), max(1, round(im.height * scale))),
                Image.Resampling.LANCZOS,
            )
        return np.asarray(im, dtype=np.float32) / 255.0


def masks_for(img: np.ndarray) -> dict[str, np.ndarray]:
    y = (0.299 * img[..., 0] + 0.587 * img[..., 1] + 0.114 * img[..., 2]) * 255.0
    return {
        "all": np.ones(y.shape, dtype=bool),
        "shadows": y < 30.0,
        "midtones": (y >= 30.0) & (y < 200.0),
        "highlights": y >= 200.0,
        # RGB heuristic works better than fixed YCbCr bounds on strongly
        # colour-graded portraits; require red dominance and some luminance.
        "skin": (
            (img[..., 0] > 0.25)
            & (img[..., 0] > img[..., 1] + 0.02)
            & (img[..., 0] > img[..., 2] + 0.04)
            & (img[..., 1] > img[..., 2] * 0.85)
            & (y > 35.0)
        ),
        "saturated": (img.max(axis=-1) - img.min(axis=-1)) > 0.25,
    }


def stats(de: np.ndarray, mask: np.ndarray) -> dict[str, float | int]:
    values = de[mask]
    if values.size == 0:
        return {"pixels": 0}
    return {
        "pixels": int(values.size),
        "mean": float(values.mean()),
        "median": float(np.median(values)),
        "p95": float(np.percentile(values, 95)),
        "p99": float(np.percentile(values, 99)),
        "max": float(values.max()),
        "pct_gt_2": float((values > 2.0).mean() * 100.0),
        "pct_gt_5": float((values > 5.0).mean() * 100.0),
    }


def heatmap(de: np.ndarray, path: Path) -> None:
    # Blue -> green -> yellow -> red; clip at 12 ΔE for readability.
    t = np.clip(de / 12.0, 0.0, 1.0)
    r = np.clip(1.5 * t, 0.0, 1.0)
    g = np.clip(1.5 * (1.0 - np.abs(t - 0.5) * 2.0), 0.0, 1.0)
    b = np.clip(1.5 * (1.0 - t), 0.0, 1.0)
    Image.fromarray((np.stack([r, g, b], axis=-1) * 255).astype(np.uint8)).save(path)


def ci_self_test() -> None:
    cases = [
        ((50.0, 2.6772, -79.7751), (50.0, 0.0, -82.7485), 2.0425),
        ((50.0, 3.1571, -77.2803), (50.0, 0.0, -82.7485), 2.8615),
        ((50.0, 2.8361, -74.0200), (50.0, 0.0, -82.7485), 3.4412),
        ((50.0, -1.3802, -84.2814), (50.0, 0.0, -82.7485), 1.0000),
    ]
    for lab1, lab2, expected in cases:
        got = float(delta_e_2000(np.array(lab1), np.array(lab2)))
        assert abs(got - expected) < 0.01, (lab1, lab2, got, expected)
    white = srgb_to_lab(np.ones((1, 1, 3)))
    assert abs(float(white[0, 0, 0]) - 100.0) < 0.01
    print("CIEDE2000 self-test OK")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("reference", type=Path, nargs="?")
    ap.add_argument("test", type=Path, nargs="?")
    ap.add_argument("--out", type=Path, default=None)
    ap.add_argument("--heatmap", type=Path, default=None)
    ap.add_argument("--max-edge", type=int, default=2048)
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    if args.self_test:
        ci_self_test()
        return

    ref = load_rgb(args.reference, args.max_edge)
    test = load_rgb(args.test, args.max_edge)
    if ref.shape != test.shape:
        test = np.asarray(
            Image.fromarray((np.clip(test, 0, 1) * 255).astype(np.uint8)).resize(
                (ref.shape[1], ref.shape[0]), Image.Resampling.LANCZOS
            ),
            dtype=np.float32,
        ) / 255.0

    ref_lab = srgb_to_lab(ref)
    test_lab = srgb_to_lab(test)
    de = delta_e_2000(ref_lab, test_lab)
    report: dict[str, object] = {
        "reference": str(args.reference),
        "test": str(args.test),
        "size": [int(ref.shape[1]), int(ref.shape[0])],
        "global": stats(de, np.ones(de.shape, dtype=bool)),
        "regions": {name: stats(de, mask) for name, mask in masks_for(ref).items()},
    }
    print(json.dumps(report, indent=2, ensure_ascii=False))
    if args.out:
        args.out.write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")
    if args.heatmap:
        heatmap(de, args.heatmap)


if __name__ == "__main__":
    main()
