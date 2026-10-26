#!/usr/bin/env python3
"""Builds the loader ELF corpus for the real-data tests
`crates/yaoplm-formats/tests/loader_elfs.rs` and
`crates/yaoplm-core/tests/loader_corpus.rs` (LIB-1, LDR-4, research/16 §1.6,
§13).

`fetch` downloads every ELF and loader package (7z, zip) of the official
releases of upstream OPL, YAOPL, RiptOPL, wOPL, the MMCE fork, other forks,
NHDDL and Neutrino with `gh release download` into
<out>/<owner>__<repo>/<tag>/ and unpacks packages with 7-Zip into x-<asset>/
next to them. Source, language and debug-symbol archives and assets over
60 MiB are skipped; files already there are kept. Nothing is executed.

`expect` writes <out>/elf-versions.tsv: the version each ELF should carry,
taken from release and file names and from git history, never from reading
the ELF. Values: an exact version, a prefix ending in `*`, `*` for programs
that are not loaders (POPStarter, Ember, Neutrino: checked only for
crashes), and `-` where no version can be read (OPL 0.x: not LZMA-packed,
known by its checksum instead).

Usage:
  python scripts/loader-corpus.py fetch [<out dir>]
  python scripts/loader-corpus.py expect [<out dir>]

The output belongs in the test data folder, never in the repository;
without <out dir> it goes to $YAOPLM_TESTDATA/downloads/loaders. Releases
are open source (AFL-3.0); no game, BIOS or POPS file is fetched (ADR-0007).
"""

from __future__ import annotations

import json
import os
import pathlib
import re
import subprocess
import sys

REPOS = [
    "ps2homebrew/Open-PS2-Loader",
    "belomaxorka/yaopl",
    "NathanNeurotic/Open-PS2-Loader",
    "NathanNeurotic/RiptOPL",
    "ps2homebrew/wOPL",
    "ps2-mmce/Open-PS2-Loader",
    "officialjuicedesigns/OPL-Evolution",
    "ps2max32/Open-PS2-Loader",
    "L10N37/Open-PS2-Loader-Extended-APA",
    "mystyq/Stable-Open-PS2-Loader",
    "pcm720/nhddl",
    "ps2max32/neutrino",
]
WANT = re.compile(r"\.(elf|7z|zip)$", re.I)
SKIP = re.compile(r"(src|source|lang|languages|fonts)", re.I)
MAX_SIZE = 60 << 20
SEVEN_ZIP = r"C:\Program Files\7-Zip\7z.exe"


def gh_json(path: str) -> list:
    out = subprocess.run(["gh", "api", "--paginate", path], capture_output=True, text=True, check=True).stdout
    # --paginate prints one array per page.
    return json.loads("[" + out.strip()[1:-1].replace("]\n[", ",").replace("][", ",") + "]") if out.strip() else []


def fetch(out: pathlib.Path) -> None:
    inventory = []
    for repo in REPOS:
        try:
            releases = gh_json(f"repos/{repo}/releases?per_page=100")
        except subprocess.CalledProcessError as e:
            print(f"!! {repo}: {e.stderr.strip()[:200]}", flush=True)
            continue
        for rel in releases:
            tag = rel["tag_name"]
            for asset in rel.get("assets", []):
                name = asset["name"]
                if not WANT.search(name) or SKIP.search(name) or asset["size"] > MAX_SIZE:
                    continue
                folder = out / repo.replace("/", "__") / re.sub(r"[^A-Za-z0-9._-]", "_", tag)
                target = folder / name
                inventory.append({
                    "repo": repo, "tag": tag, "asset": name, "size": asset["size"],
                    "digest": asset.get("digest"), "published": rel.get("published_at"),
                    "path": target.relative_to(out).as_posix(),
                })
                folder.mkdir(parents=True, exist_ok=True)
                if not target.exists() or target.stat().st_size != asset["size"]:
                    r = subprocess.run(["gh", "release", "download", tag, "-R", repo, "-p", name,
                                        "-D", str(folder), "--clobber"], capture_output=True, text=True)
                    if r.returncode != 0:
                        print(f"!! {repo} {tag} {name}: {r.stderr.strip()[:200]}", flush=True)
                        continue
                print(f"ok {repo} {tag} {name}", flush=True)
                if name.lower().endswith((".7z", ".zip")):
                    dest = folder / f"x-{name}"
                    if not dest.exists():
                        subprocess.run([SEVEN_ZIP, "x", "-y", f"-o{dest}", str(target)], capture_output=True)
    (out / "inventory.json").write_text(json.dumps(inventory, indent=1), encoding="utf-8")
    print("assets:", len(inventory), "bytes:", sum(i["size"] for i in inventory))


VERSION_IN_NAME = re.compile(
    r"(v\d+\.\d+(?:\.\d+)?(?:-rc\d+)?(?:-Beta-\d+-[0-9a-f]{7}(?:-yaopl)?|-beta-\d+|-\d+(?=[-.]|$)|-unofficial-[0-9a-f]{7})?)",
    re.I,
)
# Builds whose names do not carry the version: from the clones' git history
# (rev-list --count + 2, research/16 §3, §6) and the release names.
KNOWN = {
    ("ps2-mmce__Open-PS2-Loader", "mmce-beta1"): "v1.2.0-Beta-2127-5962294",
    ("ps2-mmce__Open-PS2-Loader", "OPL-MMCE-beta-2"): "v1.2.0-Beta-2216-bb415d2",
    ("officialjuicedesigns__OPL-Evolution", "v0.27.0-alpha"): "v1.2.0-Beta-2245-3e3f34e*",
    ("L10N37__Open-PS2-Loader-Extended-APA", "v1.2.0-Beta-2273-Extended-APA-1"): "v1.2.0-Beta-2273*",
    # KrahJohlito's last uOPL builds: "unofficial" builds of 1.2.0.
    ("ps2homebrew__wOPL", "beta"): "v1.2.0-unofficial-*",
    # The release is named "Double Unofficial OPL v1.2-beta-468".
    ("ps2homebrew__wOPL", "latest"): "v1.2-beta-468",
}


def sibling_version(folder: pathlib.Path) -> str | None:
    """The version in the name of an archive or ELF next to a bare ELF."""
    for item in sorted(folder.iterdir()):
        m = VERSION_IN_NAME.search(item.name.removeprefix("x-"))
        if m:
            return m.group(1)
    return None


def expected(root: pathlib.Path, rel: str, path: pathlib.Path) -> str:
    repo, tag = rel.split("/")[:2]
    low = rel.lower()
    name = path.name
    if re.search(r"/(pops|ember)/", low) or "/neutrino/" in low or name.lower() in ("ee_core.elf", "neutrino.elf"):
        return "*"
    if repo == "ps2max32__neutrino":
        return "*"
    if repo == "ps2homebrew__Open-PS2-Loader" and tag.startswith("0."):
        return "-"
    if (repo, tag) in KNOWN:
        return KNOWN[(repo, tag)]
    if repo == "pcm720__nhddl":
        # A tag like mmce-hdl-4 is no version.
        return tag + ("*" if "standalone" in name else "") if tag.startswith("v") else "-"
    if repo.startswith("NathanNeurotic__"):
        m = re.search(r"Beta-(\d+)", rel) or next(
            (m for s in path.parent.iterdir() if (m := re.search(r"Beta-(\d+)", s.name))), None
        )
        # PS2DEV "pinned" builds come from a shallow clone: their number is 3.
        if m is None or "PINNED" in rel:
            return "v1.2.0-Beta-*"
        return f"v1.2.0-Beta-{m.group(1)}*"
    if repo == "mystyq__Stable-Open-PS2-Loader":
        return "v1.2.0-Beta-*"
    m = VERSION_IN_NAME.search(name)
    if m:
        return m.group(1)
    tag_folder = root / repo / tag
    for folder in [path.parent, *path.parents]:
        if folder == tag_folder.parent:
            break
        m = VERSION_IN_NAME.search(folder.name)
        if m:
            return m.group(1)
        found = sibling_version(folder)
        if found:
            return found
    return "?"


def expect(root: pathlib.Path) -> None:
    rows = []
    for path in sorted(root.rglob("*")):
        if path.is_file() and path.suffix.lower() == ".elf":
            rel = path.relative_to(root).as_posix()
            rows.append(f"{rel}\t{expected(root, rel, path)}")
    (root / "elf-versions.tsv").write_text(
        "# Expected version strings of the loader ELFs here, from release and file names and git\n"
        "# history (scripts/loader-corpus.py expect), never from the ELFs themselves.\n"
        + "\n".join(rows) + "\n",
        encoding="utf-8",
    )
    print(len(rows), "rows;", sum(r.endswith("\t?") for r in rows), "unknown")


def testdata(*parts: str) -> pathlib.Path:
    """The default output folder inside the test data folder."""
    root = os.environ.get("YAOPLM_TESTDATA")
    if not root:
        raise SystemExit("Give the output folder or set YAOPLM_TESTDATA (scripts/testdata.ps1).")
    return pathlib.Path(root, *parts)


def main() -> None:
    if len(sys.argv) not in (2, 3) or sys.argv[1] not in ("fetch", "expect"):
        sys.exit(__doc__)
    out = pathlib.Path(sys.argv[2]) if len(sys.argv) == 3 else testdata("downloads", "loaders")
    out.mkdir(parents=True, exist_ok=True)
    fetch(out) if sys.argv[1] == "fetch" else expect(out)


if __name__ == "__main__":
    main()
