#!/usr/bin/env python3
"""Writes the table of contents of OPL Manager art backups on archive.org
(ART-5, research/13 §3) for the real-data test
`crates/yaoplm-formats/tests/oplm_backups.rs`.

Only the end of each zip and its central directory are fetched with HTTP
range requests (about 8 MB for a 6 GB backup); the pictures never are.
The output, <out>/<item>.tsv, has one line per entry: name, size,
compressed size, method, CRC-32 and local header offset.

Usage:
  python scripts/oplm-backup-toc.py <out dir> <item>...

Items are named OPLM_ART_YYYY_MM (2018-11 to 2024-09), for example
OPLM_ART_2024_09 and OPLM_ART_2023_07. The output belongs in the test data
folder (D:\\yaoplm-testdata\\oplm-art), never in the repository.
"""

from __future__ import annotations

import io
import json
import sys
import urllib.request
import zipfile
from pathlib import Path

USER_AGENT = "YAOPLM/0.1.0 (+https://github.com/belomaxorka/yaoplm)"


class RangeFile(io.RawIOBase):
    """A remote file read with one range request per read."""

    def __init__(self, url: str) -> None:
        self.url = url
        self.pos = 0
        self.requests = 0
        self.fetched = 0
        request = urllib.request.Request(url, method="HEAD", headers={"User-Agent": USER_AGENT})
        with urllib.request.urlopen(request) as response:
            self.size = int(response.headers["Content-Length"])
            if response.headers.get("Accept-Ranges") != "bytes":
                raise SystemExit(f"{url}: the server does not take range requests")

    def readable(self) -> bool:
        return True

    def seekable(self) -> bool:
        return True

    def tell(self) -> int:
        return self.pos

    def seek(self, offset: int, whence: int = 0) -> int:
        self.pos = {0: offset, 1: self.pos + offset, 2: self.size + offset}[whence]
        return self.pos

    def read(self, n: int = -1) -> bytes:
        if n is None or n < 0:
            n = self.size - self.pos
        if n == 0 or self.pos >= self.size:
            return b""
        end = min(self.size, self.pos + n) - 1
        # The central directory is the largest read: about 10 MB at most.
        if end - self.pos > 64 << 20:
            raise SystemExit(f"refusing to read {end - self.pos} bytes: only the directory is wanted")
        request = urllib.request.Request(
            self.url, headers={"User-Agent": USER_AGENT, "Range": f"bytes={self.pos}-{end}"}
        )
        with urllib.request.urlopen(request) as response:
            if response.status != 206:
                raise SystemExit(f"{self.url}: expected a partial answer, got {response.status}")
            data = response.read()
        self.requests += 1
        self.fetched += len(data)
        self.pos += len(data)
        return data

    def readinto(self, buffer) -> int:
        data = self.read(len(buffer))
        buffer[: len(data)] = data
        return len(data)


def table_of_contents(item: str, out: Path) -> None:
    request = urllib.request.Request(
        f"https://archive.org/metadata/{item}", headers={"User-Agent": USER_AGENT}
    )
    with urllib.request.urlopen(request) as response:
        meta = json.load(response)
    # archive.org/download redirects; its HEAD answer is sometimes a 500,
    # so the item's own server is asked.
    remote = RangeFile(f"https://{meta['server']}{meta['dir']}/{item}.zip")
    entries = zipfile.ZipFile(remote).infolist()
    with open(out / f"{item}.tsv", "w", encoding="utf-8", newline="\n") as table:
        for e in entries:
            table.write(
                f"{e.filename}\t{e.file_size}\t{e.compress_size}\t{e.compress_type}\t{e.CRC:08x}\t{e.header_offset}\n"
            )
    print(
        f"{item}: {len(entries)} entries in a {remote.size}-byte zip; "
        f"{remote.requests} requests, {remote.fetched} bytes fetched"
    )


def main() -> None:
    if len(sys.argv) < 3:
        raise SystemExit(__doc__)
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    for item in sys.argv[2:]:
        table_of_contents(item, out)


if __name__ == "__main__":
    main()
