#!/usr/bin/env python3
"""Batch CIEDE2000 comparison of two folders (same stems, different extensions)."""

from __future__ import annotations

import argparse
import csv
import json
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
from compare_lightroom import delta_e_2000, load_rgb, masks_for, srgb_to_lab, stats


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("reference_dir", type=Path)
    ap.add_argument("test_dir", type=Path)
    ap.add_argument("out_dir", type=Path)
    ap.add_argument("--max-edge", type=int, default=2048)
    args = ap.parse_args()
    args.out_dir.mkdir(parents=True, exist_ok=True)

    refs = {p.stem: p for p in args.reference_dir.iterdir() if p.suffix.lower() in {".jpg", ".jpeg", ".png", ".tif", ".tiff"}}
    tests = {p.stem: p for p in args.test_dir.iterdir() if p.suffix.lower() in {".jpg", ".jpeg", ".png", ".tif", ".tiff"}}
    rows = []
    for stem in sorted(refs.keys() & tests.keys()):
        ref = load_rgb(refs[stem], args.max_edge)
        test = load_rgb(tests[stem], args.max_edge)
        if ref.shape != test.shape:
            from PIL import Image

            test = np.asarray(
                Image.fromarray((np.clip(test, 0, 1) * 255).astype(np.uint8)).resize(
                    (ref.shape[1], ref.shape[0]), Image.Resampling.LANCZOS
                ),
                dtype=np.float32,
            ) / 255.0
        de = delta_e_2000(srgb_to_lab(ref), srgb_to_lab(test))
        report = {
            "stem": stem,
            "reference": str(refs[stem]),
            "test": str(tests[stem]),
            "size": [int(ref.shape[1]), int(ref.shape[0])],
            "global": stats(de, np.ones(de.shape, dtype=bool)),
            "regions": {name: stats(de, mask) for name, mask in masks_for(ref).items()},
        }
        (args.out_dir / f"{stem}.json").write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")
        rows.append(
            {
                "stem": stem,
                "global_mean": report["global"]["mean"],
                "global_median": report["global"]["median"],
                "global_p95": report["global"]["p95"],
                **{f"{k}_mean": v.get("mean", "") for k, v in report["regions"].items()},
            }
        )
        print(
            f"{stem:12s} mean={report['global']['mean']:6.2f} "
            f"median={report['global']['median']:6.2f} p95={report['global']['p95']:6.2f} "
            f"skin={report['regions']['skin'].get('mean', float('nan')):5.2f}"
        )

    if not rows:
        raise SystemExit("no matching pairs")
    summary = {
        "pairs": len(rows),
        "global_mean": float(np.mean([r["global_mean"] for r in rows])),
        "global_median": float(np.mean([r["global_median"] for r in rows])),
        "global_p95": float(np.mean([r["global_p95"] for r in rows])),
        "regions": {
            start: float(np.mean([r[f"{start}_mean"] for r in rows if r[f"{start}_mean"] != ""]))
            for start in ["shadows", "midtones", "highlights", "skin", "saturated"]
        },
    }
    (args.out_dir / "summary.json").write_text(json.dumps(summary, indent=2, ensure_ascii=False), encoding="utf-8")
    with (args.out_dir / "summary.csv").open("w", newline="", encoding="utf-8") as fh:
        writer = csv.DictWriter(fh, fieldnames=rows[0].keys())
        writer.writeheader()
        writer.writerows(rows)
    print(json.dumps(summary, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
