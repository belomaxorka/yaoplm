#!/usr/bin/env python3
"""Writes the file lists of the picture sources for the coverage test
`crates/yaoplm-api/tests/art_coverage.rs` (ADR-0035, research/14 §6).

Five requests, no pictures:
- xlenore/ps2-covers: the git tree of `main` (one GitHub API request);
- libretro-thumbnails: the directory indexes of Named_Boxarts, Named_Snaps,
  Named_Titles and Named_Logos on thumbnails.libretro.com.

The output, <out>/xlenore_default.txt and <out>/libretro_<folder>.txt, has
one file name per line. OPL Manager's backup is listed by
`scripts/oplm-backup-toc.py`. With --from DIR the libretro indexes are read
from saved HTML files (libretro_<folder>.html) and the xlenore list from
xlenore_default.txt there, instead of the network.

Usage:
  python scripts/art-source-lists.py <out dir> [--from DIR]

The output belongs in the test data folder (D:\\yaoplm-testdata\\art-lists),
never in the repository.
"""

from __future__ import annotations

import html
import json
import re
import sys
import time
import urllib.parse
import urllib.request
from pathlib import Path

USER_AGENT = "YAOPLM/0.1.0 (+https://github.com/belomaxorka/yaoplm)"
FOLDERS = ["Named_Boxarts", "Named_Snaps", "Named_Titles", "Named_Logos"]
LIBRETRO = "https://thumbnails.libretro.com/Sony%20-%20PlayStation%202/{folder}/"
XLENORE = "https://api.github.com/repos/xlenore/ps2-covers/git/trees/main?recursive=1"


def get(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request) as response:
        data = response.read()
    time.sleep(1)
    return data


def libretro_names(index: str) -> list[str]:
    names = set()
    for href in re.findall(r'href="([^"?/][^"]*\.png)"', index):
        names.add(urllib.parse.unquote(html.unescape(href)))
    return sorted(names)


def main() -> None:
    args = sys.argv[1:]
    if not args:
        raise SystemExit(__doc__)
    out = Path(args[0])
    source = Path(args[args.index("--from") + 1]) if "--from" in args else None
    out.mkdir(parents=True, exist_ok=True)
    for folder in FOLDERS:
        if source:
            index = (source / f"libretro_{folder}.html").read_text(encoding="utf-8", errors="replace")
        else:
            index = get(LIBRETRO.format(folder=folder)).decode("utf-8", errors="replace")
        names = libretro_names(index)
        (out / f"libretro_{folder}.txt").write_text("\n".join(names) + "\n", encoding="utf-8")
        print(f"libretro {folder}: {len(names)}")
    if source:
        covers = [line.strip() for line in (source / "xlenore_default.txt").read_text(encoding="utf-8").splitlines()]
    else:
        tree = json.loads(get(XLENORE))
        covers = [
            item["path"].removeprefix("covers/default/")
            for item in tree["tree"]
            if item["path"].startswith("covers/default/") and item["path"].endswith(".jpg")
        ]
    covers = sorted(c for c in covers if c.endswith(".jpg"))
    (out / "xlenore_default.txt").write_text("\n".join(covers) + "\n", encoding="utf-8")
    print(f"xlenore covers/default: {len(covers)}")


if __name__ == "__main__":
    main()
