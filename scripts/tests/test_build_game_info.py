"""Tests for scripts/build-game-info.py on small fixtures (META-3).

Run: python -m unittest discover -s scripts/tests
"""

from __future__ import annotations

import gzip
import importlib.util
import io
import json
import sys
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent.parent / "build-game-info.py"
spec = importlib.util.spec_from_file_location("build_game_info", SCRIPT)
assert spec is not None and spec.loader is not None
build_game_info = importlib.util.module_from_spec(spec)
sys.modules["build_game_info"] = build_game_info
spec.loader.exec_module(build_game_info)

DAT = """clrmamepro (
\tname "Sony - PlayStation 2"
\tversion "2020.3.19"
)

game (
\tname "Grand Theft Auto III (USA)"
\tserial "SLUS-20062"
\tdescription "never read"
\tdeveloper "DMA Design"
\tpublisher "Rockstar Games"
\treleaseyear "2001"
\treleasemonth "10"
\treleaseday "23"
\tusers "1"
\tgenre "Action"
\trom (
\t\tserial "SLUS-20062"
\t\tname "Grand Theft Auto III (USA).cue"
\t)
)

game (
\tname "Dead or Alive 2 (Europe)"
\tserial "SCES-50003"
\tdeveloper "Team Ninja"
\tpublisher "Tecmo"
\treleaseyear "2000"
\treleasemonth ""
\tusers "4"
\tgenre "Beat'em Up / Fighting"
)

game (
\tname "Twice (Japan)"
\tserial "SLPM-00001, SLPM-00002"
\tdeveloper "Dev"
\tpublisher "Pub"
\treleaseyear "2003"
\treleasemonth "3"
\tusers "0"
\tgenre "Data / Pictures"
)
"""

TITLES = {
    "format": 1,
    "built": "2026-10-27",
    "games": {
        "SLUS-20062": {"t": "Grand Theft Auto III", "r": "NTSC-U"},
        "SLES-50330": {"t": "Grand Theft Auto III", "r": "PAL-M5"},
        "SCES-50003": {"t": "Dead or Alive 2 - Hardcore", "r": "PAL-M5"},
        "SLUS-21331": {"t": "Sonic Riders", "r": "NTSC-U"},
        "SLUS-20930": {"t": "ChoroQ", "r": "NTSC-U"},
        "SCUS-97146": {"t": "Treasure Planet", "r": "NTSC-U"},
        "SLPM-00001": {"t": "Twice"},
        "SLPM-00002": {"t": "Twice"},
    },
}

DUMP = {
    "fetched": "2026-10-27",
    "platform": "Q10680",
    "items": {
        "Q149106": {
            "en": "Grand Theft Auto III",
            "ru": "Grand Theft Auto III",
            "enwiki": "Grand Theft Auto III",
            "ruwiki": "Grand Theft Auto III",
            "genres": ["Q343568", "Q27670585"],
            "developer": ["DMA Design"],
            "publisher": ["Rockstar Games", "Capcom"],
            "released": "2001-10-23",
        },
        "Q683373": {"en": "Sonic Riders", "genres": ["Q860750"], "developer": ["Sonic Team"], "released": "2006-02-21"},
        "Q5105196": {"en": "Choro Q HG 4", "enwiki": "Choro Q HG 4", "psid": ["SLUS-20930"], "genres": ["Q860750"]},
        # Two items with the same title: ambiguous, skipped.
        "Q1": {"en": "Treasure Planet", "released": "2002"},
        "Q2": {"en": "Treasure Planet", "enwiki": "Treasure Planet (video game)"},
        # Not in GameIndex at all.
        "Q3": {"en": "Nothing Here"},
    },
}


def titles_gz() -> bytes:
    buffer = io.BytesIO()
    with gzip.GzipFile(fileobj=buffer, mode="wb", mtime=0) as gz:
        gz.write(json.dumps(TITLES).encode("utf-8"))
    return buffer.getvalue()


class LibretroFacts(unittest.TestCase):
    def test_fields_and_dates(self) -> None:
        header, facts = build_game_info.load_libretro(DAT)
        self.assertEqual(header["version"], "2020.3.19")
        gta = facts["SLUS-20062"]
        self.assertEqual(
            gta,
            {
                "developer": "DMA Design",
                "publisher": "Rockstar Games",
                "released": "2001-10-23",
                "players": 1,
                "genres": ["action"],
            },
        )
        self.assertNotIn("description", gta)
        doa = facts["SCES-50003"]
        self.assertEqual(doa["released"], "2000", "an empty month stops the date at the year")
        self.assertEqual(doa["genres"], ["beat-em-up", "fighting"])
        self.assertEqual(doa["players"], 4)

    def test_serial_lists_and_zero_players(self) -> None:
        _, facts = build_game_info.load_libretro(DAT)
        self.assertEqual(facts["SLPM-00001"], facts["SLPM-00002"])
        self.assertNotIn("players", facts["SLPM-00001"], "0 players is no information")
        self.assertEqual(facts["SLPM-00001"]["genres"], ["other"])
        self.assertEqual(facts["SLPM-00001"]["released"], "2003-03")

    def test_unknown_genre_fails_the_build(self) -> None:
        with self.assertRaises(build_game_info.GenreError):
            build_game_info.libretro_genres("Action / Knitting")
        for phrase in build_game_info.LIBRETRO_GENRES.values():
            self.assertIn(phrase, build_game_info.GENRES)
        for code in build_game_info.WIKIDATA_GENRES.values():
            self.assertIn(code, build_game_info.GENRES)


class WikidataMatching(unittest.TestCase):
    def test_titles_normalise(self) -> None:
        n = build_game_info.normalise_title
        self.assertEqual(n("Grand Theft Auto III"), "grand theft auto 3")
        self.assertEqual(n("The Legend of Spyro: A New Beginning"), "legend of spyro a new beginning")
        self.assertEqual(n("Pokémon & Friends [Greatest Hits] (USA)"), "pokemon and friends")
        self.assertEqual(n("Tak - The Great Juju Challenge"), "tak the great juju challenge")
        self.assertEqual(n("Disney's Treasure Planet"), "disneys treasure planet")

    def test_match_by_id_and_title(self) -> None:
        matched, stats = build_game_info.match_wikidata(DUMP, TITLES["games"])
        self.assertEqual(stats, {"byId": 1, "byTitle": 2, "ambiguous": 2, "unmatched": 1}, "both items with the shared title count as ambiguous")
        gta = matched["SLUS-20062"]
        self.assertEqual(gta["qid"], "Q149106")
        self.assertEqual(gta["match"], "title")
        self.assertEqual(gta["genres"], ["action-adventure"], "a theme item is dropped")
        self.assertEqual(gta["publisher"], ["Rockstar Games", "Capcom"])
        self.assertEqual(gta["ruwiki"], "Grand Theft Auto III")
        self.assertEqual(matched["SLES-50330"]["qid"], "Q149106", "every serial with the title")
        self.assertEqual(matched["SLUS-20930"]["match"], "id")
        self.assertNotIn("SCUS-97146", matched, "two items share the title")
        self.assertNotIn("SCES-50003", matched)

    def test_dates(self) -> None:
        n = build_game_info.normalise_date
        self.assertEqual(n("+2001-10-23T00:00:00Z", 11), "2001-10-23")
        self.assertEqual(n("+2001-10-00T00:00:00Z", 10), "2001-10")
        self.assertEqual(n("+2001-00-00T00:00:00Z", 9), "2001")
        self.assertEqual(n("+2001-10-23T00:00:00Z", 9), "2001", "precision wins over the digits")
        self.assertIsNone(n("junk", 11))


class Build(unittest.TestCase):
    def test_deterministic(self) -> None:
        a = build_game_info.build(DAT.encode("utf-8"), DUMP, titles_gz())
        b = build_game_info.build(DAT.encode("utf-8"), DUMP, titles_gz())
        for db in (a, b):
            db["built"] = "fixed"
        self.assertEqual(a, b)
        self.assertEqual(a["format"], 1)
        self.assertEqual(a["genres"], build_game_info.GENRES)
        self.assertEqual(list(a["games"]), sorted(a["games"]))
        self.assertEqual(a["games"]["SLUS-20062"]["facts"]["developer"], "DMA Design")
        self.assertEqual(a["games"]["SLUS-20062"]["wikidata"]["qid"], "Q149106")
        self.assertEqual(a["sources"]["wikidata"]["items"], 6)
        self.assertEqual(a["sources"]["libretro-developer"]["version"], "2020.3.19")
        # Without a Wikidata dump the table still builds.
        c = build_game_info.build(DAT.encode("utf-8"), None, titles_gz())
        self.assertNotIn("wikidata", c["sources"])
        self.assertNotIn("wikidata", c["games"]["SLUS-20062"])


if __name__ == "__main__":
    unittest.main()
