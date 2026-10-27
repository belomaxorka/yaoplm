#!/usr/bin/env python3
"""Builds the bundled title database (META-1, ADR-0024).

Inputs:
  - PCSX2 GameIndex.yaml (GPL-3.0): serial -> name, name-en, region,
    memcardFilters;
  - libretro-database "Sony - PlayStation 2.dat" (CC BY-SA 4.0, Redump
    data): serial -> Redump names and regions, which name the covers in
    libretro-thumbnails.

Outputs, deterministic for the same inputs:
  - data/titles.json.gz: one object per serial, gzip with a zero mtime;
  - data/ATTRIBUTION.md: credits and licences, from data/attribution.json
    plus the versions of the inputs used.

Usage:
  python scripts/build-data.py [--pcsx2 URL|FILE] [--libretro URL|FILE] [--out data]

Needs PyYAML. Network access only to fetch the two inputs; pass local files
to build offline. The script never touches anything outside --out.
"""

from __future__ import annotations

import argparse
import datetime as dt
import gzip
import hashlib
import io
import json
import re
import sys
import urllib.request
from pathlib import Path

try:
    import yaml
except ImportError:  # pragma: no cover - a dev-time dependency
    sys.exit("PyYAML is required: pip install pyyaml")

PCSX2_URL = "https://raw.githubusercontent.com/PCSX2/pcsx2/master/bin/resources/GameIndex.yaml"
LIBRETRO_URL = (
    "https://raw.githubusercontent.com/libretro/libretro-database/master/"
    "metadat/redump/Sony%20-%20PlayStation%202.dat"
)
USER_AGENT = "YAOPLM-build-data (+https://github.com/belomaxorka/yaoplm)"
FORMAT_VERSION = 1

SERIAL_RE = re.compile(r"^([A-Z]{4})-(\d{5})")


def fetch(source: str) -> bytes:
    """Reads a local file or downloads a URL."""
    if source.startswith(("http://", "https://")):
        request = urllib.request.Request(source, headers={"User-Agent": USER_AGENT})
        with urllib.request.urlopen(request, timeout=120) as response:
            return response.read()
    return Path(source).read_bytes()


def lookup_key(serial: str) -> str:
    """`SLES-54793/ANZ` -> `SLES-54793`; other shapes stay as they are.

    Mirrors `yaoplm_formats::serial::lookup_key`.
    """
    serial = serial.strip().upper()
    match = SERIAL_RE.match(serial)
    return f"{match.group(1)}-{match.group(2)}" if match else serial


def load_gameindex(text: str) -> dict[str, dict]:
    loader = getattr(yaml, "CSafeLoader", yaml.SafeLoader)
    data = yaml.load(text, Loader=loader)
    if not isinstance(data, dict):
        raise SystemExit("GameIndex.yaml: expected a mapping at the top level")
    games: dict[str, dict] = {}
    for serial, entry in data.items():
        if not isinstance(entry, dict):
            continue
        record: dict = {}
        name = entry.get("name")
        if isinstance(name, str) and name.strip():
            record["t"] = name.strip()
        name_en = entry.get("name-en")
        if isinstance(name_en, str) and name_en.strip() and name_en.strip() != record.get("t"):
            record["e"] = name_en.strip()
        region = entry.get("region")
        if isinstance(region, str) and region.strip():
            record["r"] = region.strip()
        filters = entry.get("memcardFilters")
        if isinstance(filters, list):
            related = sorted(
                {lookup_key(str(f)) for f in filters if isinstance(f, str) and lookup_key(str(f)) != lookup_key(str(serial))}
            )
            if related:
                record["k"] = related
        games[str(serial).strip().upper()] = record
    return games


def tokenize(text: str):
    """clrmamepro tokens: `(`, `)`, bare words and double-quoted strings."""
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        if c.isspace():
            i += 1
        elif c in "()":
            yield c
            i += 1
        elif c == '"':
            j = i + 1
            out = []
            while j < n and text[j] != '"':
                if text[j] == "\\" and j + 1 < n:
                    j += 1
                out.append(text[j])
                j += 1
            yield "".join(out)
            i = j + 1
        else:
            j = i
            while j < n and not text[j].isspace() and text[j] not in "()":
                j += 1
            yield text[i:j]
            i = j


def parse_blocks(tokens):
    """Yields (block name, {key: value}) for every top-level block; nested
    blocks (`rom ( ... )`) are kept as lists under their key."""
    tokens = list(tokens)
    i = 0

    def block(start: int) -> tuple[dict, int]:
        fields: dict = {}
        j = start
        while j < len(tokens):
            tok = tokens[j]
            if tok == ")":
                return fields, j + 1
            if j + 1 < len(tokens) and tokens[j + 1] == "(":
                inner, j = block(j + 2)
                fields.setdefault(tok, []).append(inner)
            else:
                fields[tok] = tokens[j + 1] if j + 1 < len(tokens) else ""
                j += 2
        return fields, j

    while i < len(tokens):
        name = tokens[i]
        if i + 1 < len(tokens) and tokens[i + 1] == "(":
            fields, i = block(i + 2)
            yield name, fields
        else:
            i += 1


def load_dat(text: str) -> tuple[dict, dict[str, list[list[str]]], dict[str, set[str]]]:
    header: dict = {}
    names: dict[str, list[list[str]]] = {}
    media: dict[str, set[str]] = {}
    for kind, fields in parse_blocks(tokenize(text)):
        if kind == "clrmamepro":
            header = fields
            continue
        if kind != "game":
            continue
        serial = fields.get("serial")
        name = fields.get("name")
        if not serial or not name:
            continue
        region = fields.get("region", "")
        # One DAT entry may carry several serials: "SLUS-20062, SLUS-20063".
        for one in str(serial).split(","):
            key = lookup_key(one)
            if not key:
                continue
            entry = [name, region]
            bucket = names.setdefault(key, [])
            if entry not in bucket:
                bucket.append(entry)
            # Redump dumps a CD as BIN/CUE and a DVD as one ISO (IMP-1): the
            # roms' extensions say which disc the serial is on.
            for rom in fields.get("rom", []):
                rom_name = str(rom.get("name", "")).lower() if isinstance(rom, dict) else ""
                if rom_name.endswith(".iso"):
                    media.setdefault(key, set()).add("DVD")
                elif rom_name.endswith((".cue", ".bin")):
                    media.setdefault(key, set()).add("CD")
    return header, names, media


def build(pcsx2: bytes, libretro: bytes) -> dict:
    games = load_gameindex(pcsx2.decode("utf-8"))
    header, names, media = load_dat(libretro.decode("utf-8", errors="replace"))
    for key, entries in names.items():
        games.setdefault(key, {})["d"] = entries
    # A serial pressed on both kinds of disc (a CD and a DVD release) gets no verdict.
    for key, kinds in media.items():
        if len(kinds) == 1:
            games.setdefault(key, {})["m"] = next(iter(kinds))
    today = dt.datetime.now(dt.timezone.utc).date().isoformat()
    return {
        "format": FORMAT_VERSION,
        "built": today,
        "sources": {
            # The canonical URLs, whatever file was passed: the SHA-256 says
            # which revision of the input this build used.
            "pcsx2": {"url": PCSX2_URL, "sha256": hashlib.sha256(pcsx2).hexdigest()},
            "libretro": {
                "url": LIBRETRO_URL,
                "version": header.get("version", ""),
                "sha256": hashlib.sha256(libretro).hexdigest(),
            },
        },
        "games": dict(sorted(games.items())),
    }


def write_gzip(path: Path, payload: bytes) -> None:
    buffer = io.BytesIO()
    # mtime=0 and no file name: the same input gives the same bytes.
    with gzip.GzipFile(fileobj=buffer, mode="wb", compresslevel=9, mtime=0) as gz:
        gz.write(payload)
    path.write_bytes(buffer.getvalue())


def attribution_markdown(attribution: dict, db: dict) -> str:
    lines = [
        "# Attribution",
        "",
        "Data and services YAOPLM relies on (ADR-0007). This file is generated by",
        "`scripts/build-data.py` from `data/attribution.json`; edit the JSON.",
        "",
        "## Bundled data",
        "",
        f"`data/titles.json.gz` (format {db['format']}, built {db['built']}) is derived from:",
        "",
    ]
    sources = db["sources"]
    for entry in attribution["data"]:
        meta = sources.get(entry["id"], {})
        version = f", version {meta['version']}" if meta.get("version") else ""
        lines += [
            f"### {entry['name']}",
            "",
            f"- Source: <{entry['url']}>",
            f"- Licence: {entry['license']} (<{entry['licenseUrl']}>)",
            f"- Credits: {entry['credits']}",
            f"- Fields used: {entry['fields']}",
            f"- Input of this build: <{meta.get('url', '')}>{version}, SHA-256 `{meta.get('sha256', '')}`",
            "",
        ]
    lines += [
        "The derived database keeps the share-alike terms of CC BY-SA 4.0 for the",
        "parts that come from libretro-database and is distributed with YAOPLM under",
        "GPL-3.0-or-later (ADR-0002).",
        "",
    ]
    for entry in attribution.get("other", []):
        lines += [
            f"### {entry['name']}",
            "",
            f"- File: `{entry['file']}`",
            f"- Source: <{entry['url']}> at `{entry['version']}`",
            f"- Licence: {entry['license']} (<{entry['licenseUrl']}>)",
            f"- Credits: {entry['credits']}",
            f"- Used for: {entry['use']}",
            "",
        ]
    lines += [
        "## Picture sources (never bundled)",
        "",
        "Pictures are downloaded only when the user asks for them, cached locally and",
        "written to the user's device. YAOPLM does not redistribute them (ADR-0007,",
        "ADR-0035).",
        "",
    ]
    for entry in attribution["art"]:
        licence = entry["license"] or "no licence stated; used on request only"
        if entry.get("licenseUrl"):
            licence += f" (<{entry['licenseUrl']}>)"
        lines += [
            f"### {entry['name']}",
            "",
            f"- Source: <{entry['url']}>",
            f"- Licence: {licence}",
            f"- Credits: {entry['credits']}",
            f"- Used for: {entry['use']}",
            "",
        ]
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--pcsx2", default=PCSX2_URL, help="GameIndex.yaml URL or file")
    parser.add_argument("--libretro", default=LIBRETRO_URL, help="libretro PS2 DAT URL or file")
    parser.add_argument("--out", default=Path(__file__).resolve().parent.parent / "data", type=Path)
    parser.add_argument(
        "--attribution-only",
        action="store_true",
        help="only rewrite ATTRIBUTION.md from attribution.json and the titles.json.gz there is",
    )
    args = parser.parse_args()

    out: Path = args.out
    out.mkdir(parents=True, exist_ok=True)
    attribution = json.loads((out / "attribution.json").read_text(encoding="utf-8"))
    if args.attribution_only:
        db = json.loads(gzip.decompress((out / "titles.json.gz").read_bytes()))
        (out / "ATTRIBUTION.md").write_text(attribution_markdown(attribution, db), encoding="utf-8", newline="\n")
        print(f"wrote {out / 'ATTRIBUTION.md'}")
        return 0

    print(f"reading {args.pcsx2}")
    pcsx2 = fetch(args.pcsx2)
    print(f"reading {args.libretro}")
    libretro = fetch(args.libretro)
    db = build(pcsx2, libretro)

    payload = json.dumps(db, ensure_ascii=False, separators=(",", ":"), sort_keys=True).encode("utf-8")
    write_gzip(out / "titles.json.gz", payload)
    (out / "ATTRIBUTION.md").write_text(attribution_markdown(attribution, db), encoding="utf-8", newline="\n")

    with_title = sum(1 for g in db["games"].values() if "t" in g)
    with_names = sum(1 for g in db["games"].values() if "d" in g)
    print(
        f"{len(db['games'])} serials ({with_title} with a PCSX2 title, {with_names} with Redump names, "
        f"{sum(1 for g in db['games'].values() if 'm' in g)} with a disc type); "
        f"{len(payload)} bytes of JSON, {(out / 'titles.json.gz').stat().st_size} bytes gzipped"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
