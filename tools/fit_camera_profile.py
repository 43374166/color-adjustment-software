#!/usr/bin/env python3
"""Fit a provisional camera look from paired RAW renders and camera JPEGs.

This is a *proxy* calibration: target is the camera's embedded JPEG, not
Lightroom. It is used only to validate the CameraProfile plumbing.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import numpy as np
from PIL import Image, ImageFilter, ImageOps

RGB_TO_XYZ = np.array(
    [
        [0.4124564, 0.3575761, 0.1804375],
        [0.2126729, 0.7151522, 0.0721750],
        [0.0193339, 0.1191920, 0.9503041],
    ]
)
WHITE = np.array([0.95047, 1.0, 1.08883])


def srgb_to_linear(x):
    x = np.clip(x, 0, 1)
    return np.where(x <= 0.04045, x / 12.92, ((x + 0.055) / 1.055) ** 2.4)


def linear_to_srgb(x):
    x = np.clip(x, 0, 1)
    return np.where(x <= 0.0031308, x * 12.92, 1.055 * x ** (1 / 2.4) - 0.055)


def linear_to_lab(x):
    xyz = x @ RGB_TO_XYZ.T / WHITE
    d = 6 / 29
    f = np.where(xyz > d**3, np.cbrt(xyz), xyz / (3 * d**2) + 4 / 29)
    return np.stack([116 * f[..., 1] - 16, 500 * (f[..., 0] - f[..., 1]), 200 * (f[..., 1] - f[..., 2])], -1)


def delta_e_2000(lab1, lab2):
    # compact CIEDE2000 implementation; see tools/compare_lightroom.py
    L1, a1, b1 = np.moveaxis(lab1, -1, 0)
    L2, a2, b2 = np.moveaxis(lab2, -1, 0)
    C1, C2 = np.hypot(a1, b1), np.hypot(a2, b2)
    Cbar = 0.5 * (C1 + C2)
    G = 0.5 * (1 - np.sqrt(Cbar**7 / (Cbar**7 + 25**7)))
    a1p, a2p = (1 + G) * a1, (1 + G) * a2
    C1p, C2p = np.hypot(a1p, b1), np.hypot(a2p, b2)
    h1p = np.degrees(np.arctan2(b1, a1p)) % 360
    h2p = np.degrees(np.arctan2(b2, a2p)) % 360
    dh = h2p - h1p
    dh = np.where(dh > 180, dh - 360, dh)
    dh = np.where(dh < -180, dh + 360, dh)
    dH = 2 * np.sqrt(C1p * C2p) * np.sin(np.radians(dh) / 2)
    Lbar = 0.5 * (L1 + L2)
    Cbarp = 0.5 * (C1p + C2p)
    hsum = h1p + h2p
    hbar = np.where(
        np.abs(h1p - h2p) <= 180,
        hsum / 2,
        np.where(hsum < 360, (hsum + 360) / 2, (hsum - 360) / 2),
    )
    T = (
        1
        - 0.17 * np.cos(np.radians(hbar - 30))
        + 0.24 * np.cos(np.radians(2 * hbar))
        + 0.32 * np.cos(np.radians(3 * hbar + 6))
        - 0.20 * np.cos(np.radians(4 * hbar - 63))
    )
    dtheta = 30 * np.exp(-(((hbar - 275) / 25) ** 2))
    RC = 2 * np.sqrt(Cbarp**7 / (Cbarp**7 + 25**7))
    SL = 1 + 0.015 * (Lbar - 50) ** 2 / np.sqrt(20 + (Lbar - 50) ** 2)
    SC = 1 + 0.045 * Cbarp
    SH = 1 + 0.015 * Cbarp * T
    RT = -np.sin(np.radians(2 * dtheta)) * RC
    return np.sqrt(
        ((L2 - L1) / SL) ** 2
        + ((C2p - C1p) / SC) ** 2
        + (dH / SH) ** 2
        + RT * ((C2p - C1p) / SC) * (dH / SH)
    )


def load(path: Path, size=512) -> np.ndarray:
    with Image.open(path) as im:
        im = ImageOps.exif_transpose(im).convert("RGB").filter(ImageFilter.GaussianBlur(1.0))
        scale = size / max(im.size)
        im = im.resize((round(im.width * scale), round(im.height * scale)), Image.Resampling.LANCZOS)
        return np.asarray(im, np.float32) / 255


def orient(test: np.ndarray, ref: np.ndarray) -> np.ndarray:
    """Camera embedded JPEG can be 90° rotated relative to the RAW render."""
    if (test.shape[0] > test.shape[1]) != (ref.shape[0] > ref.shape[1]):
        return np.rot90(test, 1)
    return test


def align(test: np.ndarray, ref: np.ndarray) -> np.ndarray:
    """Align test to ref by integer translation (phase correlation)."""
    h = min(test.shape[0], ref.shape[0])
    w = min(test.shape[1], ref.shape[1])
    test, ref = test[:h, :w], ref[:h, :w]
    a = test.mean(2)
    b = ref.mean(2)
    win = np.outer(np.hanning(h), np.hanning(w))
    A = np.fft.fft2((a - a.mean()) * win)
    B = np.fft.fft2((b - b.mean()) * win)
    R = A * np.conj(B)
    R /= np.maximum(np.abs(R), 1e-9)
    corr = np.abs(np.fft.ifft2(R))
    dy, dx = np.unravel_index(np.argmax(corr), corr.shape)
    if dy > h // 2:
        dy -= h
    if dx > w // 2:
        dx -= w
    shifted = np.roll(test, (dy, dx), axis=(0, 1))
    # Crop the wrapped border after translation.
    m = 8
    return shifted[m : h - m, m : w - m]


def fit_matrix(x, y):
    # y = x @ M.T, no bias: a global affine camera correction.
    M, *_ = np.linalg.lstsq(x, y, rcond=None)
    return M


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--raw-dir", type=Path, default=Path(r"D:\color-adjustment-software\.tmp\camera-pairs\raw"))
    ap.add_argument("--cam-dir", type=Path, default=Path(r"D:\color-adjustment-software\.tmp\camera-pairs\camera"))
    ap.add_argument("--out", type=Path, default=Path(r"D:\color-adjustment-software\.tmp\camera-pairs\fit.json"))
    args = ap.parse_args()
    pairs = []
    for raw_path in sorted(args.raw_dir.glob("*.png")):
        cam_path = args.cam_dir / (raw_path.stem + ".jpg")
        if cam_path.exists():
            pairs.append((raw_path.stem, raw_path, cam_path))
    print("pairs", len(pairs))
    rng = np.random.default_rng(1)
    idx = rng.permutation(len(pairs))
    train = [pairs[i] for i in idx[: max(1, len(pairs) - 2)]]
    test = [pairs[i] for i in idx[max(1, len(pairs) - 2) :]]

    def gather(items):
        xs, ys = [], []
        for _, rp, cp in items:
            r = load(rp)
            c = align(orient(load(cp), r), r)
            h, w = c.shape[:2]
            r = r[8 : 8 + h, 8 : 8 + w]
            r = srgb_to_linear(r)
            c = srgb_to_linear(c)
            xs.append(r.reshape(-1, 3))
            ys.append(c.reshape(-1, 3))
        return np.concatenate(xs), np.concatenate(ys)

    x, y = gather(train)
    mask = (x.max(1) < 0.98) & (x.min(1) > 0.002) & (y.max(1) < 0.98) & (y.min(1) > 0.002)
    x, y = x[mask], y[mask]
    M = fit_matrix(x, y)
    print("matrix", M)

    def apply_profile(rgb):
        lin = srgb_to_linear(rgb)
        pred = np.clip(lin @ M, 0, 1)
        return linear_to_srgb(pred)

    for name, items in [("train", train), ("test", test)]:
        des = []
        for _, rp, cp in items:
            r = load(rp)
            c = align(orient(load(cp), r), r)
            h, w = c.shape[:2]
            r = r[8 : 8 + h, 8 : 8 + w]
            before = delta_e_2000(linear_to_lab(srgb_to_linear(c)), linear_to_lab(srgb_to_linear(r)))
            after_rgb = apply_profile(r)
            after = delta_e_2000(linear_to_lab(srgb_to_linear(c)), linear_to_lab(srgb_to_linear(after_rgb)))
            des.append((before.mean(), after.mean()))
        arr = np.array(des)
        print(name, "before", arr[:, 0].mean(), "after", arr[:, 1].mean(), "per-image", arr.round(2).tolist())

    args.out.write_text(
        json.dumps(
            {
                "matrix": M.T.tolist(),
                "bias": [0.0, 0.0, 0.0],
                "curve": [[0.0, 1.0, 0.0, 0.0] for _ in range(3)],
            },
            indent=2,
        ),
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()
