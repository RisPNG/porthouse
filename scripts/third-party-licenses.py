#!/usr/bin/env python3
"""Writes licenses/rust-crates.txt: the licence and notice files of every Rust crate Porthouse depends on.

Run it from the repository root after changing dependencies. CI runs it with --check.
"""
import hashlib
import json
import pathlib
import subprocess
import sys

OUT = pathlib.Path("licenses/rust-crates.txt")
NAMES = ("LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE")


def render() -> str:
    meta = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--format-version", "1"], text=True))
    crates = sorted((p for p in meta["packages"] if p["source"]), key=lambda p: (p["name"], p["version"]))
    texts: dict[str, list[str]] = {}
    bodies: dict[str, str] = {}
    listing = []
    for p in crates:
        directory = pathlib.Path(p["manifest_path"]).parent
        files = sorted(f for f in directory.iterdir() if f.is_file() and f.name.upper().startswith(NAMES))
        note = "" if files else " (the crate ships no licence file, the standard text of that licence applies)"
        listing.append(f'{p["name"]} {p["version"]}: {p["license"] or "see its files"}{note}')
        for f in files:
            body = f.read_text(encoding="utf-8", errors="replace").strip()
            key = hashlib.sha256(body.encode()).hexdigest()
            bodies[key] = body
            texts.setdefault(key, []).append(f'{p["name"]} {p["version"]}')
    out = ["Rust crates used by Porthouse and their licences", "=" * 48, "", *listing, ""]
    for key in sorted(texts, key=lambda k: texts[k][0]):
        out += ["-" * 78, "Used by: " + ", ".join(texts[key]), "-" * 78, bodies[key], ""]
    return "\n".join(out)


text = render()
if "--check" in sys.argv:
    if OUT.read_text(encoding="utf-8") != text:
        sys.exit("licenses/rust-crates.txt is out of date: run scripts/third-party-licenses.py")
else:
    OUT.write_text(text, encoding="utf-8")
