#!/usr/bin/env python3
"""Set Lightroom develop settings in a *copy* of a catalog.

The catalog is a SQLite database, but Adobe does not document this as a public
API. Use only on a throwaway catalog copy, never the live catalog.

Example:
  python tools/lightroom_catalog_settings.py ^
    .tmp/lr-catalog/Lightroom Catalog.lrcat ^
    --folder "D:/PS/2023-9-10/" ^
    --set Exposure2012=1 --set Contrast2012=50 --set Saturation=50
"""

from __future__ import annotations

import argparse
import sqlite3
from pathlib import Path


def _find_value_end(text: str, start: int) -> int:
    depth = 0
    in_string = False
    quote = ""
    i = start
    while i < len(text):
        ch = text[i]
        if in_string:
            if ch == "\\":
                i += 2
                continue
            if ch == quote:
                in_string = False
            i += 1
            continue
        if ch in "\"'":
            in_string = True
            quote = ch
        elif ch in "{([":
            depth += 1
        elif ch in "})]":
            if depth == 0:
                return i
            depth -= 1
        elif ch == "," and depth == 0:
            return i
        i += 1
    return i


def set_top_level(text: str, key: str, value: str) -> str:
    """Update a top-level Lua-table key, leaving nested masks/gradients alone."""
    start = text.find("{")
    if start < 0:
        return f"s = {{\n{key} = {value},\n}}\n"
    depth = 0
    i = start
    in_string = False
    quote = ""
    while i < len(text):
        ch = text[i]
        if in_string:
            if ch == "\\":
                i += 2
                continue
            if ch == quote:
                in_string = False
            i += 1
            continue
        if ch in "\"'":
            in_string = True
            quote = ch
            i += 1
            continue
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                prefix = text[:i].rstrip()
                comma = "" if prefix.endswith(",") else ","
                return prefix + f"{comma}\n{key} = {value},\n" + text[i:]
        elif depth == 1 and (ch.isalpha() or ch == "_"):
            j = i
            while j < len(text) and (text[j].isalnum() or text[j] == "_"):
                j += 1
            name = text[i:j]
            k = j
            while k < len(text) and text[k].isspace():
                k += 1
            if name == key and k < len(text) and text[k] == "=":
                vstart = k + 1
                while vstart < len(text) and text[vstart].isspace():
                    vstart += 1
                vend = _find_value_end(text, vstart)
                return text[:vstart] + value + text[vend:]
            i = j
            continue
        i += 1
    return text + f"\n{key} = {value},\n"


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("catalog", type=Path)
    ap.add_argument("--folder", required=True, help="pathFromRoot, e.g. 2023-9-10/")
    ap.add_argument("--root", default="D:/PS/")
    ap.add_argument("--set", action="append", default=[], metavar="KEY=VALUE")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()
    if not args.catalog.exists():
        raise SystemExit(f"catalog not found: {args.catalog}")
    values = {}
    for item in args.set:
        key, value = item.split("=", 1)
        values[key] = value
    if not values:
        raise SystemExit("at least one --set KEY=VALUE is required")

    con = sqlite3.connect(args.catalog)
    con.text_factory = lambda b: b.decode("utf-8", "replace")
    q = """
    select i.id_local, d.id_local, d.text
    from Adobe_images i
    join AgLibraryFile f on f.id_local = i.rootFile
    join AgLibraryFolder fo on fo.id_local = f.folder
    join AgLibraryRootFolder r on r.id_local = fo.rootFolder
    left join Adobe_imageDevelopSettings d on d.image = i.id_local
    where r.absolutePath = ? and fo.pathFromRoot = ?
    """
    rows = list(con.execute(q, (args.root, args.folder)))
    print(f"{len(rows)} photos in {args.root}{args.folder}")
    for image_id, settings_id, text in rows:
        text = text or "s = {\n}\n"
        for key, value in values.items():
            text = set_top_level(text, key, value)
        if args.dry_run:
            continue
        if settings_id is None:
            con.execute(
                "insert into Adobe_imageDevelopSettings(image, text, hasDevelopAdjustments) "
                "values (?, ?, 1)",
                (image_id, text),
            )
        else:
            con.execute(
                "update Adobe_imageDevelopSettings set text = ?, hasDevelopAdjustments = 1 "
                "where id_local = ?",
                (text, settings_id),
            )
    if args.dry_run:
        print("dry run; no changes written")
    else:
        con.commit()
        print(f"updated {len(rows)} photos")


if __name__ == "__main__":
    main()
