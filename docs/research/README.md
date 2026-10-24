# Research (2026-10-22)

Background research done before development started. Facts were checked against primary sources where possible. Anything that could not be confirmed is marked **unverified** inside each report. psx-place.com, reddit and some other forums were behind bot checks, so forum evidence comes mostly from search snippets.

| # | Report | What it covers |
|---|---|---|
| 01 | [competitors.md](01-competitors.md) | OPL Manager, OrbitPS2 Manager, OPL PC Tools, PyOPLM, libopl, ul/split tools, PS2-HDD tools. Feature matrices, user complaints, 14 gaps |
| 02 | [opl-formats.md](02-opl-formats.md) | What OPL reads from storage, from OPL source: folders, ID detection, `ul.cfg` + `USBA_crc32`, CFG keys, ART, CHT, VMC, APPS, ZSO, HDL. Implementation checklist in §14 |
| 03 | [data-sources.md](03-data-sources.md) | Cover-art, metadata, Redump, GameIndex, compatibility lists, cheats. Licences, auth, limits, recommended priority |
| 04 | [stack.md](04-stack.md) | Tauri 2 vs Wails v3 vs Electron vs others, Rust/Go library availability, CI/signing, testing, repo layout, risks |
| 05 | [oplnova-audit.md](05-oplnova-audit.md) | Source audit of OPLNova. "RIP" = RiptOPL art formats and what supporting them takes; §3.4 has the RiptOPL author's art spec verbatim |
| 06 | [device-formatting.md](06-device-formatting.md) | Preparing USB sticks and SD cards for OPL, MX4SIO and memory card emulators: filesystems, partitioning, 512-byte sectors, cluster sizes, per-OS device access, own Rust formatter, safety pipeline (SPEC DEV-2, DEV-3) |
| 07 | [loaders-and-boot.md](07-loaders-and-boot.md) | Loader catalogue (OPL, forks, NHDDL/Neutrino), boot setups (FreeMcBoot, PS2BBL, MX4SIO, MMCE emulators), where ELFs go (SPEC LDR-*) |
| 08 | [format-verification.md](08-format-verification.md) | M1 implementation notes: forks agree on the core formats, new OPL edge cases (SYSTEM.CNF, isofs, `ul.cfg`, CFG comments, CHT quirks, ZSO limits), `USBA_crc32` vectors checked against OPL's C code, `ziso.py` interop, corpus results. Corrects 02 §6.1 |
| 09 | [import-prior-art.md](09-import-prior-art.md) | M2 prior art: how other tools write `ul` games and `ul.cfg`, BIN/CUE to ISO (offsets, refusals, lone BIN), CD/DVD detection and what the media byte changes in OPL, rename/delete/move and the files keyed by the game ID, write safety. Corrects 02 §4.4 |
| 10 | [ui-prior-art.md](10-ui-prior-art.md) | M3 prior art: jobs panel, device picker, import plan, rename/delete dialogs, library grid; what was taken from OrbitPS2 Manager, POUM, OPL PC Tools and what was not repeated from OPLNova |
| 11 | [art-prior-art.md](11-art-prior-art.md) | M4 prior art: bulk scopes and "skip existing" (Orbit), download hygiene and health repair (Toolbox), mirrors (PyOPLM), resize rules (OPL PC Tools, OPL itself); GitHub-API listings, single unlicensed databases and indexed-only PNGs not repeated |
| 12 | [dump-prior-art.md](12-dump-prior-art.md) | M5 prior art: the Redump DAT (URL, User-Agent, no validators, serial shapes in the PS2 DAT), why CD ISOs never match, ZSO order and worst-case align (Orbit), `ul.cfg` records sharing chunks and OPL's `sbDelete`, the 1 GiB part rule, archives streamed instead of unpacked, CHD layout and parents |
| 13 | [art-types-and-sets-prior-art.md](13-art-types-and-sets-prior-art.md) | M5 prior art: picture types OPL reads and the ICO drawn at 128×128, open sources per type, xlenore 3-D covers as a COV variant, LGO fitted not stretched, OPL Manager backup layout (numbered SCR and BG, mixed jpg/png), RiptOPL and wOPL `art.tar` readers, covers and dump checks while games are added |
| 15 | [cfg-vmc-prior-art.md](15-cfg-vmc-prior-art.md) | M5 prior art for CFG-2 and VMC-1: what OPL's game menu writes and removes, the launch rule for `...Source` keys, 1.1.0 vs 1.2, RiptOPL's modes 7 and 8, other managers' editors and their pitfalls, the CFG corpora; genvmc's card layout field by field, cross-checks against `pc/genvmc`, mymcplusplus and genvmc samples, VMC names and groups, OPL's checks and menu quirks, counting a file's runs on Windows |
| 14 | [art-sources.md](14-art-sources.md) | A network source for every picture type: OPL Manager's archive.org backup read by range requests (index, CRC, region duplicates), ScreenScraper behind a developer key, the region policy, the order per type, sources not taken, coverage measured over the title database |
| 16 | [loader-signs.md](16-loader-signs.md) | Which loader build a device is for (LIB-1, LDR-4): upstream OPL as a ladder of beta numbers (keys, ZSO, exFAT, fragments, JPG/BMP, name length, VMC groups), the `games.bin` record size, the rewritten upstream history, YAOPL, the MMCE fork, RiptOPL, wOPL, OPL-Evolution, NHDDL and Neutrino; ELF names, packing and hashes; signs ranked with false positives, conflict rules, how to keep the signs current, a device test matrix |
| 17 | [loader-install-prior-art.md](17-loader-install-prior-art.md) | M5 prior art for the loader installer (LDR-1…LDR-7, BOOT-1): how TinyPS2Manager, PS2 HDD Manager, PFS BatchKit, PSBBN and the FMCB installer install and update loaders (no backups, no manifests, no version detection), the stock FMCB and PS2BBL paths, RiptOPL's and YAOPL's release processes, upstream release facts and archive layouts, evidence for the presets, language packs, starter settings, MX4SIO and foreign ELFs |
| — | [REFERENCES.md](REFERENCES.md) | Index of reference repositories (local clones in `D:\yaoplm-testdata\refs\`) and forum threads (ADR-0015) |

Spot-checked by hand against OPL fork source (NathanNeurotic/Open-PS2-Loader @ 2089b79):
- the `USBExtreme_game_entry_t` layout (64 bytes);
- the `USBA_crc32` implementation;
- the `COMPAT_MODE_1..8` definitions. Upstream uses 1–6; 7 and 8 are RiptOPL-only.
