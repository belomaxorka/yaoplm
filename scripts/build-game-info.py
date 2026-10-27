#!/usr/bin/env python3
"""Builds the bundled game info table (META-3, ADR-0050).

Inputs:
  - libretro-database `metadat/developer/Sony - PlayStation 2.dat`
    (CC BY-SA 4.0; a scrape of PSXDataCenter by robloach's scraper):
    developer, publisher, release date, player count and genre per serial.
    Its descriptions are uncredited copy and are never read (research/23
    §2.4);
  - a Wikidata extract (CC0) of every item with platform PlayStation 2,
    fetched by this script through SPARQL (`--wikidata fetch`) and saved
    for offline builds (`--wikidata-dump`): labels in `en` and `ru`,
    genres, developers, publishers, the earliest publication date, the
    PlayStation ID where there is one, and the `en`/`ru` Wikipedia article
    titles;
  - `data/titles.json.gz` (ADR-0024): the serial -> title table a Wikidata
    item is matched against.

Outputs, deterministic for the same inputs:
  - data/game-info.json.gz: one record per serial, gzip with a zero mtime;
  - data/ATTRIBUTION.md is rewritten by `scripts/build-data.py
    --attribution-only`, which reads this file's `sources`.

Usage:
  python scripts/build-game-info.py [--libretro URL|FILE]
      [--wikidata fetch|FILE] [--wikidata-dump FILE] [--titles FILE] [--out data]

Network access only to fetch the inputs; pass local files to build offline.
The script never touches anything outside --out and --wikidata-dump.
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
import time
import unicodedata
import urllib.parse
import urllib.request
from pathlib import Path

LIBRETRO_URL = (
    "https://raw.githubusercontent.com/libretro/libretro-database/master/"
    "metadat/developer/Sony%20-%20PlayStation%202.dat"
)
SPARQL_URL = "https://query.wikidata.org/sparql"
USER_AGENT = "YAOPLM-build-game-info (+https://github.com/belomaxorka/yaoplm)"
FORMAT_VERSION = 1
PS2 = "Q10680"

SERIAL_RE = re.compile(r"^([A-Z]{4})-(\d{5})")

# The genre vocabulary (ADR-0050): one short list of codes with `en`/`ru`
# labels in the i18n catalogues (`genre.<code>`). Every libretro phrase must
# map here (a new one fails the build); Wikidata genres map where they fit.
GENRES = [
    "action",
    "action-adventure",
    "adventure",
    "platform",
    "shooter",
    "fighting",
    "beat-em-up",
    "racing",
    "sports",
    "rpg",
    "strategy",
    "simulation",
    "puzzle",
    "music",
    "party",
    "board",
    "card",
    "casino",
    "mahjong",
    "quiz",
    "horror",
    "visual-novel",
    "educational",
    "pinball",
    "other",
]

# libretro's `genre` is one phrase, several of them joined by " / " (and once
# by ", "). Each token maps to one code.
LIBRETRO_GENRES = {
    "Action": "action",
    "Action role-playing": "rpg",
    "Adventure": "adventure",
    "Basketball": "sports",
    "Beat 'em up": "beat-em-up",
    "Beat'em Up": "beat-em-up",
    "Board": "board",
    "Boxing": "sports",
    "Breakout": "puzzle",
    "Card": "card",
    "Chess": "board",
    "Dancing": "music",
    "Data": "other",
    "Educational": "educational",
    "Fighting": "fighting",
    "Flight Simulator": "simulation",
    "Football": "sports",
    "Gambling": "casino",
    "Gun": "shooter",
    "Hunting and Fishing": "sports",
    "Interactive Movie": "adventure",
    "Mahjong": "mahjong",
    "Mini Games": "party",
    "Music": "music",
    "Party": "party",
    "Pictures": "other",
    "Pinball": "pinball",
    "Platform": "platform",
    "Puzzle": "puzzle",
    "Quiz": "quiz",
    "Racing": "racing",
    "Rhythm": "music",
    "Role-playing (RPG)": "rpg",
    "Shogi": "board",
    "Shooter": "shooter",
    "Simulation": "simulation",
    "Sing": "music",
    "Sports": "sports",
    "Strategy": "strategy",
    "Survival Horror": "horror",
    "Tennis": "sports",
    "Visual Novel": "visual-novel",
    "Wrestling": "sports",
    "hack and slash": "action",
}

# Wikidata genre items (P136) of PS2 games that fit the vocabulary; the
# long tail (themes such as "science fiction video game") is left out.
WIKIDATA_GENRES = {
    "Q343568": "action-adventure",
    "Q860750": "racing",
    "Q689445": "visual-novel",
    "Q270948": "action",
    "Q846224": "fighting",
    "Q744038": "rpg",
    "Q868217": "sports",
    "Q828322": "platform",
    "Q380266": "shooter",
    "Q584105": "music",
    "Q185029": "shooter",
    "Q23916": "adventure",
    "Q1478420": "sports",
    "Q1422746": "rpg",
    "Q1044478": "shooter",
    "Q54767": "puzzle",
    "Q63915027": "sports",
    "Q401831": "beat-em-up",
    "Q63915391": "sports",
    "Q1163960": "action",
    "Q1529437": "rpg",
    "Q1610017": "simulation",
    "Q92608103": "music",
    "Q7888616": "party",
    "Q1339223": "simulation",
    "Q62019045": "sports",
    "Q2070892": "action",
    "Q333967": "horror",
    "Q858523": "action",
    "Q472055": "strategy",
    "Q71474750": "sports",
    "Q2632782": "music",
    "Q1199309": "simulation",
    "Q105221690": "sports",
    "Q208189": "strategy",
    "Q4282636": "shooter",
    "Q130270628": "fighting",
    "Q3965952": "shooter",
    "Q1417032": "racing",
    "Q61793886": "sports",
    "Q2016457": "sports",
    "Q4740553": "simulation",
    "Q71474253": "sports",
    "Q3177953": "sports",
    "Q603481": "simulation",
    "Q108611897": "sports",
    "Q2176159": "strategy",
    "Q1260861": "shooter",
    "Q5923834": "rpg",
    "Q221181": "simulation",
    "Q2313710": "strategy",
    "Q13717398": "puzzle",
    "Q60256879": "sports",
    "Q11338014": "horror",
    "Q1198141": "simulation",
    "Q1224999": "educational",
    "Q3177954": "pinball",
}


def fetch(source: str) -> bytes:
    """Reads a local file or downloads a URL."""
    if source.startswith(("http://", "https://")):
        request = urllib.request.Request(source, headers={"User-Agent": USER_AGENT})
        with urllib.request.urlopen(request, timeout=120) as response:
            return response.read()
    return Path(source).read_bytes()


def lookup_key(serial: str) -> str:
    """`SLES-54793/ANZ` -> `SLES-54793`, `SLUS_200.62` -> `SLUS-20062`.

    Mirrors `yaoplm_formats::serial::lookup_key` for the spellings the
    inputs use.
    """
    serial = serial.strip().upper().replace("_", "-").replace(".", "")
    match = SERIAL_RE.match(serial)
    return f"{match.group(1)}-{match.group(2)}" if match else serial


# --- libretro DAT -----------------------------------------------------------


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


class GenreError(ValueError):
    pass


def libretro_genres(phrase: str) -> list[str]:
    """`Beat'em Up / Fighting` -> `["beat-em-up", "fighting"]`, in order,
    without repeats. An unknown token is an error: the vocabulary is kept
    by hand."""
    codes: list[str] = []
    for token in re.split(r"\s*(?:/|,)\s*", phrase.strip()):
        if not token:
            continue
        code = LIBRETRO_GENRES.get(token)
        if code is None:
            raise GenreError(f"unknown libretro genre {token!r} in {phrase!r}")
        if code not in codes:
            codes.append(code)
    return codes


def clean(value: str) -> str:
    return " ".join(value.split())


def load_libretro(text: str) -> tuple[dict, dict[str, dict]]:
    """Facts per serial. A serial listed twice keeps its first record."""
    header: dict = {}
    facts: dict[str, dict] = {}
    for kind, fields in parse_blocks(tokenize(text)):
        if kind == "clrmamepro":
            header = fields
            continue
        if kind != "game":
            continue
        serial = fields.get("serial")
        if not isinstance(serial, str) or not serial.strip():
            continue
        record: dict = {}
        developer = clean(str(fields.get("developer", "")))
        if developer:
            record["developer"] = developer
        publisher = clean(str(fields.get("publisher", "")))
        if publisher:
            record["publisher"] = publisher
        year = str(fields.get("releaseyear", "")).strip()
        if re.fullmatch(r"\d{4}", year):
            date = year
            month = str(fields.get("releasemonth", "")).strip()
            if re.fullmatch(r"\d{1,2}", month) and 1 <= int(month) <= 12:
                date += f"-{int(month):02d}"
                day = str(fields.get("releaseday", "")).strip()
                if re.fullmatch(r"\d{1,2}", day) and 1 <= int(day) <= 31:
                    date += f"-{int(day):02d}"
            record["released"] = date
        users = str(fields.get("users", "")).strip()
        if re.fullmatch(r"\d+", users) and int(users) > 0:
            record["players"] = int(users)
        genre = str(fields.get("genre", "")).strip()
        if genre:
            codes = libretro_genres(genre)
            if codes:
                record["genres"] = codes
        if not record:
            continue
        for one in serial.split(","):
            key = lookup_key(one)
            if key and key not in facts:
                facts[key] = record
    return header, facts


# --- Wikidata ---------------------------------------------------------------


def sparql(query: str) -> list[dict]:
    url = SPARQL_URL + "?" + urllib.parse.urlencode({"query": query, "format": "json"})
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT, "Accept": "application/sparql-results+json"})
    for attempt in range(4):
        try:
            with urllib.request.urlopen(request, timeout=300) as response:
                payload = json.load(response)
            return [{k: v["value"] for k, v in row.items()} for row in payload["results"]["bindings"]]
        except Exception as error:  # noqa: BLE001 - retried, then reported
            if attempt == 3:
                raise
            print(f"  retry after {error}", file=sys.stderr)
            time.sleep(10 * (attempt + 1))
    return []


def fetch_wikidata() -> dict:
    """Every item with platform PS2, one query per property, as a plain
    dictionary that `--wikidata FILE` reads back."""
    items: dict[str, dict] = {}

    def item(qid: str) -> dict:
        return items.setdefault(qid, {})

    print("  labels and sitelinks")
    # Wikidata keeps a name that is the same in every language as a `mul`
    # label, often without an `en` or `ru` one: `mul` is the fallback.
    rows = sparql(
        f"""SELECT ?g ?en ?ru ?mul ?enwiki ?ruwiki WHERE {{
          ?g wdt:P400 wd:{PS2} .
          OPTIONAL {{ ?g rdfs:label ?en FILTER(LANG(?en)="en") }}
          OPTIONAL {{ ?g rdfs:label ?ru FILTER(LANG(?ru)="ru") }}
          OPTIONAL {{ ?g rdfs:label ?mul FILTER(LANG(?mul)="mul") }}
          OPTIONAL {{ ?enwiki schema:about ?g ; schema:isPartOf <https://en.wikipedia.org/> }}
          OPTIONAL {{ ?ruwiki schema:about ?g ; schema:isPartOf <https://ru.wikipedia.org/> }}
        }}"""
    )
    for row in rows:
        qid = row["g"].rsplit("/", 1)[1]
        entry = item(qid)
        for key in ("en", "ru"):
            value = row.get(key) or row.get("mul")
            if value:
                entry[key] = value
        for key in ("enwiki", "ruwiki"):
            if row.get(key):
                entry[key] = urllib.parse.unquote(row[key].rsplit("/wiki/", 1)[1]).replace("_", " ")
    time.sleep(2)
    for prop, key, labelled in [("P136", "genres", False), ("P178", "developer", True), ("P123", "publisher", True)]:
        print(f"  {key}")
        if labelled:
            query = f"""SELECT ?g ?v ?l ?m WHERE {{ ?g wdt:P400 wd:{PS2} ; wdt:{prop} ?v .
              OPTIONAL {{ ?v rdfs:label ?l FILTER(LANG(?l)="en") }}
              OPTIONAL {{ ?v rdfs:label ?m FILTER(LANG(?m)="mul") }} }}"""
        else:
            query = f"""SELECT ?g ?v WHERE {{ ?g wdt:P400 wd:{PS2} ; wdt:{prop} ?v . }}"""
        for row in sparql(query):
            qid = row["g"].rsplit("/", 1)[1]
            value = row["v"].rsplit("/", 1)[1]
            if labelled:
                value = row.get("l") or row.get("m") or value
            bucket = item(qid).setdefault(key, [])
            if value not in bucket:
                bucket.append(value)
        time.sleep(2)
    print("  publication dates")
    for row in sparql(
        f"""SELECT ?g ?d ?p WHERE {{ ?g wdt:P400 wd:{PS2} ; p:P577/psv:P577 ?node .
          ?node wikibase:timeValue ?d ; wikibase:timePrecision ?p . }}"""
    ):
        qid = row["g"].rsplit("/", 1)[1]
        date = normalise_date(row["d"], int(row["p"]))
        if date and (item(qid).get("released") is None or date < item(qid)["released"]):
            item(qid)["released"] = date
    time.sleep(2)
    print("  PlayStation IDs")
    for row in sparql(f"""SELECT ?g ?v WHERE {{ ?g wdt:P400 wd:{PS2} ; wdt:P2606 ?v . }}"""):
        qid = row["g"].rsplit("/", 1)[1]
        bucket = item(qid).setdefault("psid", [])
        if row["v"] not in bucket:
            bucket.append(row["v"])
    return {
        "fetched": dt.datetime.now(dt.timezone.utc).date().isoformat(),
        "platform": PS2,
        "items": dict(sorted(items.items())),
    }


def normalise_date(value: str, precision: int) -> str | None:
    """`+2001-10-23T00:00:00Z` with Wikidata's precision (9 year, 10 month,
    11 day) -> `2001-10-23`, `2001-10` or `2001`."""
    match = re.match(r"^\+?(\d{4})-(\d{2})-(\d{2})", value)
    if not match:
        return None
    year, month, day = match.groups()
    if year == "0000":
        return None
    if precision >= 11 and month != "00" and day != "00":
        return f"{year}-{month}-{day}"
    if precision >= 10 and month != "00":
        return f"{year}-{month}"
    return year


# --- matching ---------------------------------------------------------------

ROMAN = {"ii": "2", "iii": "3", "iv": "4", "v": "5", "vi": "6", "vii": "7", "viii": "8", "ix": "9", "x": "10"}


def normalise_title(title: str) -> str:
    """A key for matching titles across sources: lower case, no accents,
    apostrophes dropped (`Disney's` -> `disneys`), other punctuation as
    spaces, `&` as `and`, roman numerals as digits, no articles at the
    start, no bracketed tags."""
    text = unicodedata.normalize("NFKD", title)
    text = "".join(c for c in text if not unicodedata.combining(c))
    text = re.sub(r"\([^)]*\)|\[[^\]]*\]", " ", text)
    text = text.lower().replace("&", " and ").replace("'", "").replace("’", "")
    text = re.sub(r"[^a-z0-9]+", " ", text)
    words = [ROMAN.get(w, w) for w in text.split()]
    if words and words[0] in ("the", "a", "an"):
        words = words[1:]
    return " ".join(words)


def title_index(titles: dict[str, dict]) -> dict[str, list[str]]:
    """Normalised title -> serials, from GameIndex's `name` and `name-en`."""
    index: dict[str, list[str]] = {}
    for serial, entry in titles.items():
        for key in ("t", "e"):
            name = entry.get(key)
            if isinstance(name, str):
                norm = normalise_title(name)
                if norm:
                    index.setdefault(norm, []).append(serial)
    return index


def match_wikidata(dump: dict, titles: dict[str, dict]) -> tuple[dict[str, dict], dict]:
    """Serial -> Wikidata record. A PlayStation ID wins; otherwise the
    item's `en` label (or `en` article title) must equal exactly one
    Wikidata item's title among the PS2 items, and that title must be in
    GameIndex. Ambiguous titles are skipped and counted."""
    index = title_index(titles)
    by_norm: dict[str, list[str]] = {}
    for qid, entry in dump["items"].items():
        keys = {normalise_title(entry[k]) for k in ("en", "enwiki") if entry.get(k)}
        for norm in keys:
            if norm:
                by_norm.setdefault(norm, []).append(qid)
    out: dict[str, dict] = {}
    stats = {"byId": 0, "byTitle": 0, "ambiguous": 0, "unmatched": 0}

    def record(qid: str, entry: dict, method: str) -> dict:
        rec: dict = {"qid": qid, "match": method}
        for key in ("en", "ru", "enwiki", "ruwiki", "released"):
            if entry.get(key):
                rec[key] = entry[key]
        genres: list[str] = []
        for g in entry.get("genres", []):
            code = WIKIDATA_GENRES.get(g)
            if code and code not in genres:
                genres.append(code)
        if genres:
            rec["genres"] = genres
        for key in ("developer", "publisher"):
            if entry.get(key):
                rec[key] = entry[key]
        return rec

    for qid, entry in sorted(dump["items"].items()):
        matched = False
        for psid in entry.get("psid", []):
            serial = lookup_key(psid)
            if serial in titles and serial not in out:
                out[serial] = record(qid, entry, "id")
                matched = True
        if matched:
            stats["byId"] += 1
            continue
        keys = {normalise_title(entry[k]) for k in ("en", "enwiki") if entry.get(k)}
        serials: list[str] = []
        ambiguous = False
        for norm in keys:
            if not norm or norm not in index:
                continue
            if len(by_norm.get(norm, [])) > 1:
                ambiguous = True
                continue
            serials += index[norm]
        if ambiguous and not serials:
            stats["ambiguous"] += 1
            continue
        if not serials:
            stats["unmatched"] += 1
            continue
        for serial in serials:
            if serial not in out:
                out[serial] = record(qid, entry, "title")
        stats["byTitle"] += 1
    return out, stats


# --- build ------------------------------------------------------------------


def build(libretro: bytes, dump: dict | None, titles_gz: bytes) -> dict:
    header, facts = load_libretro(libretro.decode("utf-8", errors="replace"))
    titles = json.loads(gzip.decompress(titles_gz))["games"]
    games: dict[str, dict] = {}
    for serial, record in facts.items():
        games.setdefault(serial, {})["facts"] = dict(record)
    stats: dict = {}
    if dump is not None:
        matched, stats = match_wikidata(dump, titles)
        for serial, record in matched.items():
            games.setdefault(serial, {})["wikidata"] = record
    today = dt.datetime.now(dt.timezone.utc).date().isoformat()
    sources = {
        "libretro-developer": {
            "url": LIBRETRO_URL,
            "version": header.get("version", ""),
            "sha256": hashlib.sha256(libretro).hexdigest(),
        }
    }
    if dump is not None:
        sources["wikidata"] = {
            "url": SPARQL_URL,
            "version": dump.get("fetched", ""),
            "sha256": hashlib.sha256(
                json.dumps(dump, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")
            ).hexdigest(),
            "items": len(dump.get("items", {})),
            "matched": stats,
        }
    return {
        "format": FORMAT_VERSION,
        "built": today,
        "genres": GENRES,
        "sources": sources,
        "games": dict(sorted(games.items())),
    }


def write_gzip(path: Path, payload: bytes) -> None:
    buffer = io.BytesIO()
    # mtime=0 and no file name: the same input gives the same bytes.
    with gzip.GzipFile(fileobj=buffer, mode="wb", compresslevel=9, mtime=0) as gz:
        gz.write(payload)
    path.write_bytes(buffer.getvalue())


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--libretro", default=LIBRETRO_URL, help="libretro developer DAT URL or file")
    parser.add_argument(
        "--wikidata",
        default="fetch",
        help="`fetch` (SPARQL, then saved to --wikidata-dump), a saved dump, or `none`",
    )
    parser.add_argument(
        "--wikidata-dump",
        default=root / "target" / "game-info" / "wikidata-ps2.json",
        type=Path,
        help="where a fetched extract is saved",
    )
    parser.add_argument("--titles", default=root / "data" / "titles.json.gz", type=Path)
    parser.add_argument("--out", default=root / "data", type=Path)
    args = parser.parse_args()

    print(f"reading {args.libretro}")
    libretro = fetch(args.libretro)
    dump: dict | None
    if args.wikidata == "none":
        dump = None
    elif args.wikidata == "fetch":
        print("fetching Wikidata")
        dump = fetch_wikidata()
        args.wikidata_dump.parent.mkdir(parents=True, exist_ok=True)
        args.wikidata_dump.write_text(
            json.dumps(dump, ensure_ascii=False, sort_keys=True, indent=1), encoding="utf-8", newline="\n"
        )
        print(f"saved {args.wikidata_dump} ({len(dump['items'])} items)")
    else:
        print(f"reading {args.wikidata}")
        dump = json.loads(Path(args.wikidata).read_text(encoding="utf-8"))
    titles_gz = args.titles.read_bytes()

    db = build(libretro, dump, titles_gz)
    out: Path = args.out
    out.mkdir(parents=True, exist_ok=True)
    payload = json.dumps(db, ensure_ascii=False, separators=(",", ":"), sort_keys=True).encode("utf-8")
    write_gzip(out / "game-info.json.gz", payload)

    games = db["games"]
    facts = sum(1 for g in games.values() if "facts" in g)
    wikidata = sum(1 for g in games.values() if "wikidata" in g)
    print(
        f"{len(games)} serials ({facts} with libretro facts, {wikidata} with a Wikidata item; "
        f"matching: {db['sources'].get('wikidata', {}).get('matched', {})}); "
        f"{len(payload)} bytes of JSON, {(out / 'game-info.json.gz').stat().st_size} bytes gzipped"
    )
    print("now run: python scripts/build-data.py --attribution-only")
    return 0


if __name__ == "__main__":
    sys.exit(main())
