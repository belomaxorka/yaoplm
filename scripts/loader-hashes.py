#!/usr/bin/env python3
"""Appends new loader builds to data/loader-hashes/<history>.txt (SPEC
§5.10.1, ADR-0022, ADR-0040).

Floating tags (`latest`) replace their assets in place, so a build stays
recognisable on a device only if its SHA-256 was written down while it was
published. For every catalogue build with a floating channel this script
lists the releases of its repository that belong to the channel
(`channel.releases`), takes the SHA-256 of the ELF asset from the release's
checksum file (`channel.checksums.sums`) and its GitHub `digest` (they must
agree when both exist), and appends `version sha256 date file` unless the
history has that line. Lines are never changed or removed. No build is
downloaded: only the API answers and the small checksum files.

Usage:
  python scripts/loader-hashes.py                 ask GitHub (GITHUB_TOKEN if set)
  python scripts/loader-hashes.py --check         only validate the history files
  python scripts/loader-hashes.py --releases DIR  offline: DIR/<owner>__<repo>.json is
      the releases list, DIR/<owner>__<repo>/<tag>/<asset> a checksum file

Needs Python 3.11+ (tomllib). `.github/workflows/loader-hashes.yml` runs it
once a day and commits what it appends.
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import os
import re
import sys
import time
import tomllib
import urllib.request
from pathlib import Path

USER_AGENT = "YAOPLM/loader-hashes (+https://github.com/belomaxorka/yaoplm)"
API = "https://api.github.com"
MAX_SUMS = 64 * 1024

# The grammar of crates/yaoplm-core/src/loaders/version.rs.
VERSION = re.compile(r"^v\d+\.\d+\.\d+(-Beta-\d+-[0-9a-fA-F]{7})?(-[A-Za-z0-9]+)*$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")
DATE = re.compile(r"^\d{4}-\d{2}-\d{2}$")


class Failure(Exception):
    """Something a person must look at: the run fails and nothing is written."""


def load_catalogue(data: Path) -> list[dict]:
    index = tomllib.loads((data / "loaders" / "index.toml").read_text(encoding="utf-8"))
    return [
        tomllib.loads((data / "loaders" / f"{build}.toml").read_text(encoding="utf-8"))
        for build in index["builds"]
    ]


def read_history(path: Path) -> list[tuple[str, str, str, str]]:
    """The lines of a history file; Failure on anything the program would refuse."""
    lines = []
    for number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        fields = line.split()
        where = f"{path.name}: line {number}"
        if len(fields) != 4:
            raise Failure(f"{where}: expected `version sha256 date file`")
        version, sha, date, _ = fields
        if not VERSION.match(version):
            raise Failure(f"{where}: {version} is not a version string")
        if not SHA256.match(sha):
            raise Failure(f"{where}: not a lowercase SHA-256")
        if not DATE.match(date):
            raise Failure(f"{where}: not a YYYY-MM-DD date")
        entry = (fields[0], fields[1], fields[2], fields[3])
        if entry in lines:
            raise Failure(f"{where}: repeats an earlier line")
        lines.append(entry)
    return lines


class GitHub:
    """Releases and checksum files, from the API or an offline folder."""

    def __init__(self, offline: Path | None):
        self.offline = offline
        self.token = os.environ.get("GITHUB_TOKEN")
        self.cache: dict[str, list[dict]] = {}
        self.last = 0.0

    def _get(self, url: str, accept: str, limit: int | None = None) -> bytes:
        # At most one request a second (CLAUDE.md rule 3).
        wait = self.last + 1.0 - time.monotonic()
        if wait > 0:
            time.sleep(wait)
        self.last = time.monotonic()
        request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT, "Accept": accept})
        if self.token and url.startswith(API):
            request.add_header("Authorization", f"Bearer {self.token}")
            request.add_header("X-GitHub-Api-Version", "2022-11-28")
        with urllib.request.urlopen(request, timeout=60) as response:
            body = response.read(limit + 1 if limit else -1)
        if limit and len(body) > limit:
            raise Failure(f"{url}: larger than {limit} bytes")
        return body

    def releases(self, repo: str) -> list[dict]:
        if repo not in self.cache:
            if self.offline:
                path = self.offline / f"{repo.replace('/', '__')}.json"
                self.cache[repo] = json.loads(path.read_text(encoding="utf-8")) if path.exists() else []
            else:
                body = self._get(f"{API}/repos/{repo}/releases?per_page=100", "application/vnd.github+json")
                self.cache[repo] = json.loads(body)
        return self.cache[repo]

    def asset_text(self, repo: str, release: dict, asset: dict) -> str:
        if self.offline:
            path = self.offline / repo.replace("/", "__") / release["tag_name"] / asset["name"]
            return path.read_text(encoding="utf-8")
        body = self._get(asset["browser_download_url"], "application/octet-stream", MAX_SUMS)
        return body.decode("utf-8")


def version_of(build: dict, release: dict) -> str | None:
    rule = build["channel"]["version"]
    if rule == "tag":
        candidates = [release.get("tag_name") or ""]
    elif rule == "name":
        candidates = [release.get("name") or ""]
    else:
        prefix, _, suffix = rule.partition("{version}")
        candidates = [
            asset["name"][len(prefix) : len(asset["name"]) - len(suffix)]
            for asset in release.get("assets", [])
            if asset["name"].startswith(prefix)
            and asset["name"].endswith(suffix)
            and len(asset["name"]) > len(prefix) + len(suffix)
        ]
    found = sorted({c for c in candidates if VERSION.match(c)})
    if len(found) > 1:
        raise Failure(f"{build['id']} {release.get('tag_name')}: several versions {found}")
    return found[0] if found else None


def sums_lines(text: str) -> dict[str, str]:
    """`sha256sum` output: `<hex>  <name>` or `<hex> *<name>`."""
    sums = {}
    for line in text.splitlines():
        parts = line.strip().split(None, 1)
        if len(parts) == 2 and SHA256.match(parts[0].lower()):
            sums[parts[1].lstrip("*")] = parts[0].lower()
    return sums


def new_lines(build: dict, github: GitHub) -> list[tuple[str, str, str, str]]:
    channel = build["channel"]
    elf = channel["elf"]
    if not channel.get("floating") or not isinstance(elf, str):
        # Fixed tags never change; an ELF inside an archive has no digest.
        return []
    checksums = channel.get("checksums", {})
    found = []
    for release in github.releases(build["repo"]):
        tag = release.get("tag_name") or ""
        if release.get("draft") or not any(fnmatch.fnmatchcase(tag, p) for p in channel["releases"]):
            continue
        assets = {asset["name"]: asset for asset in release.get("assets", [])}
        asset = assets.get(elf)
        if asset is None:
            print(f"{build['id']} {tag}: no {elf}, skipped", file=sys.stderr)
            continue
        version = version_of(build, release)
        if version is None:
            print(f"{build['id']} {tag}: no version string, skipped", file=sys.stderr)
            continue
        hashes = {}
        digest = asset.get("digest") or ""
        if checksums.get("digest") and digest.startswith("sha256:"):
            hashes["digest"] = digest.removeprefix("sha256:").lower()
        sums_name = checksums.get("sums")
        if sums_name and sums_name in assets:
            listed = sums_lines(github.asset_text(build["repo"], release, assets[sums_name])).get(elf)
            if listed:
                hashes[sums_name] = listed
        if not hashes:
            print(f"{build['id']} {tag}: nothing confirms {elf}, skipped", file=sys.stderr)
            continue
        if len(set(hashes.values())) != 1:
            raise Failure(f"{build['id']} {tag}: {elf} hashes disagree: {hashes}")
        sha = next(iter(hashes.values()))
        if not SHA256.match(sha):
            raise Failure(f"{build['id']} {tag}: {sha} is not a SHA-256")
        date = (asset.get("updated_at") or release.get("published_at") or "")[:10]
        if not DATE.match(date):
            raise Failure(f"{build['id']} {tag}: no date")
        found.append((version, sha, date, elf))
    return found


def append(path: Path, lines: list[tuple[str, str, str, str]]) -> list[tuple[str, str, str, str]]:
    have = read_history(path)
    by_sha = {(sha, name): version for version, sha, _, name in have}
    added = []
    for line in sorted(set(lines), key=lambda l: (l[2], l[0])):
        version, sha, _, name = line
        known = by_sha.get((sha, name))
        if known is not None:
            if known != version:
                print(f"{path.name}: {sha} is already {known}, not {version}", file=sys.stderr)
            continue
        added.append(line)
        by_sha[(sha, name)] = version
    if added:
        text = path.read_text(encoding="utf-8")
        if text and not text.endswith("\n"):
            text += "\n"
        text += "".join(" ".join(line) + "\n" for line in added)
        path.write_text(text, encoding="utf-8", newline="\n")
    return added


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--data", type=Path, default=Path(__file__).resolve().parent.parent / "data")
    parser.add_argument("--check", action="store_true", help="only validate the history files")
    parser.add_argument("--releases", type=Path, help="read releases from this folder, not GitHub")
    args = parser.parse_args(argv)
    try:
        builds = load_catalogue(args.data)
        for path in sorted((args.data / "loader-hashes").glob("*.txt")):
            read_history(path)
        if args.check:
            print("loader hashes: ok")
            return 0
        github = GitHub(args.releases)
        # Every build is read first: a failure leaves every file as it was.
        pending = [(build, new_lines(build, github)) for build in builds]
        for build, lines in pending:
            path = args.data / "loader-hashes" / f"{build['history']}.txt"
            for line in append(path, lines):
                print(f"{path.name}: {' '.join(line)}")
    except Failure as failure:
        print(f"error: {failure}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
