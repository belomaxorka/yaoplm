"""Tests of scripts/loader-hashes.py on a copy of the catalogue and canned
release lists: `python -m unittest discover -s scripts -p "test_*.py"`."""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import shutil
import tempfile
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
DATA = SCRIPTS.parent / "data"

spec = importlib.util.spec_from_file_location("loader_hashes", SCRIPTS / "loader-hashes.py")
assert spec and spec.loader
loader_hashes = importlib.util.module_from_spec(spec)
spec.loader.exec_module(loader_hashes)

NEW_BETA = "1" * 64
NEW_YAOPL = "2" * 64


def asset(name: str, digest: str | None = None, updated: str = "2026-10-26T04:00:00Z") -> dict:
    return {
        "name": name,
        "digest": f"sha256:{digest}" if digest else None,
        "updated_at": updated,
        "browser_download_url": f"https://example.invalid/{name}",
    }


class LoaderHashes(unittest.TestCase):
    def setUp(self) -> None:
        self.dir = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.dir)
        self.data = self.dir / "data"
        shutil.copytree(DATA / "loaders", self.data / "loaders")
        shutil.copytree(DATA / "loader-hashes", self.data / "loader-hashes")
        self.releases = self.dir / "releases"
        self.releases.mkdir()

    def write_releases(self, repo: str, releases: list[dict]) -> None:
        (self.releases / f"{repo.replace('/', '__')}.json").write_text(json.dumps(releases), encoding="utf-8")

    def write_sums(self, repo: str, tag: str, text: str) -> None:
        folder = self.releases / repo.replace("/", "__") / tag
        folder.mkdir(parents=True, exist_ok=True)
        (folder / "SHA256SUMS").write_text(text, encoding="utf-8")

    def run_script(self, *args: str) -> tuple[int, str, str]:
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = loader_hashes.main(["--data", str(self.data), *args])
        return code, out.getvalue(), err.getvalue()

    def history(self, name: str) -> list[str]:
        text = (self.data / "loader-hashes" / f"{name}.txt").read_text(encoding="utf-8")
        return [line for line in text.splitlines() if line and not line.startswith("#")]

    def upstream(self, elf_digest: str = NEW_BETA) -> None:
        version = "v1.2.0-Beta-2280-0123abc"
        self.write_releases(
            "ps2homebrew/Open-PS2-Loader",
            [
                {
                    "tag_name": "latest",
                    "name": "latest",
                    "assets": [
                        asset("OPNPS2LD.ELF", elf_digest),
                        asset("OPNPS2LD.7z", "a" * 64),
                        asset(f"OPNPS2LD-{version}.7z", "a" * 64),
                        asset(f"OPNPS2LD-VARIANTS-{version}.7z", "b" * 64),
                        asset(f"OPNPS2LD-LANGS-{version}.7z", "c" * 64),
                    ],
                },
                {"tag_name": "v1.1.0", "name": "v1.1.0", "assets": [asset("OPNPS2LD-v1.1.0.7z")]},
            ],
        )

    def yaopl(self, sums_sha: str = NEW_YAOPL, digest: str | None = NEW_YAOPL) -> None:
        version = "v1.2.0-Beta-2380-89abcde-yaopl"
        self.write_releases(
            "belomaxorka/yaopl",
            [
                {
                    "tag_name": "latest",
                    "name": version,
                    "assets": [asset("OPNPS2LD.ELF", digest), asset("SHA256SUMS", "d" * 64)],
                },
                # Inherited upstream tags are not YAOPL releases.
                {"tag_name": "v1.1.0", "name": "v1.1.0", "assets": [asset("OPNPS2LD.ELF", "e" * 64)]},
            ],
        )
        self.write_sums("belomaxorka/yaopl", "latest", f"{sums_sha}  OPNPS2LD.ELF\n{'f' * 64}  OPNPS2LD-{version}.7z\n")

    def test_appends_new_builds_once(self) -> None:
        self.upstream()
        self.yaopl()
        stable_before = self.history("opl-stable")
        code, out, err = self.run_script("--releases", str(self.releases))
        self.assertEqual(code, 0, err)
        self.assertEqual(self.history("opl-beta")[-1], f"v1.2.0-Beta-2280-0123abc {NEW_BETA} 2026-10-26 OPNPS2LD.ELF")
        self.assertEqual(self.history("yaopl")[-1], f"v1.2.0-Beta-2380-89abcde-yaopl {NEW_YAOPL} 2026-10-26 OPNPS2LD.ELF")
        self.assertEqual(self.history("opl-stable"), stable_before)
        self.assertEqual(out.count("\n"), 2, out)
        # The old lines stay, in order, and a second run adds nothing.
        self.assertEqual(len(self.history("opl-beta")), 2)
        code, out, _ = self.run_script("--releases", str(self.releases))
        self.assertEqual((code, out), (0, ""))
        self.assertEqual(len(self.history("opl-beta")), 2)

    def test_disagreeing_hashes_stop_the_run(self) -> None:
        self.upstream()
        self.yaopl(sums_sha="3" * 64)
        before = self.history("opl-beta")
        code, _, err = self.run_script("--releases", str(self.releases))
        self.assertEqual(code, 1)
        self.assertIn("disagree", err)
        # Nothing was written, not even the upstream line.
        self.assertEqual(self.history("opl-beta"), before)

    def test_sums_alone_are_enough(self) -> None:
        self.yaopl(digest=None)
        code, _, err = self.run_script("--releases", str(self.releases))
        self.assertEqual(code, 0, err)
        self.assertTrue(self.history("yaopl")[-1].startswith("v1.2.0-Beta-2380-89abcde-yaopl 2222"))

    def test_a_known_build_is_not_added_again(self) -> None:
        # The build already in the history, as GitHub reports it today.
        self.upstream(elf_digest="4b6d36eff917bbeb3d860fea7ca0b3dd38502c4f5b008803a1edf3f0edf12290")
        code, out, err = self.run_script("--releases", str(self.releases))
        self.assertEqual(code, 0, err)
        self.assertEqual(out, "")
        self.assertIn("already v1.2.0-Beta-2271-b3a2199", err)

    def test_check_refuses_a_broken_file(self) -> None:
        self.assertEqual(self.run_script("--check")[0], 0)
        path = self.data / "loader-hashes" / "opl-beta.txt"
        path.write_text(path.read_text(encoding="utf-8") + "latest abc 2026 OPNPS2LD.ELF\n", encoding="utf-8")
        code, _, err = self.run_script("--check")
        self.assertEqual(code, 1)
        self.assertIn("opl-beta.txt: line", err)

    def test_the_repository_files_pass(self) -> None:
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = loader_hashes.main(["--check"])
        self.assertEqual((code, out.getvalue().strip()), (0, "loader hashes: ok"))


if __name__ == "__main__":
    unittest.main()
