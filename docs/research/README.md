# Research (2026-10-22)

Background research done before development started. Facts were checked against primary sources where possible. Anything that could not be confirmed is marked **unverified** inside each report. psx-place.com, reddit and some other forums were behind bot checks, so forum evidence comes mostly from search snippets.

| # | Report | What it covers |
|---|---|---|
| 01 | [competitors.md](01-competitors.md) | OPL Manager, OrbitPS2 Manager, OPL PC Tools, PyOPLM, libopl, ul/split tools, PS2-HDD tools. Feature matrices, user complaints, 14 gaps |
| 02 | [opl-formats.md](02-opl-formats.md) | What OPL reads from storage, from OPL source: folders, ID detection, `ul.cfg` + `USBA_crc32`, CFG keys, ART, CHT, VMC, APPS, ZSO, HDL. Implementation checklist in §14 |
| 03 | [data-sources.md](03-data-sources.md) | Cover-art, metadata, Redump, GameIndex, compatibility lists, cheats. Licences, auth, limits, recommended priority |
| 04 | [stack.md](04-stack.md) | Tauri 2 vs Wails v3 vs Electron vs others, Rust/Go library availability, CI/signing, testing, repo layout, risks |
| 05 | [oplnova-audit.md](05-oplnova-audit.md) | Source audit of OPLNova. "RIP" = RiptOPL art formats and what supporting them takes; §3.4 has the RiptOPL author's art spec verbatim |
| 07 | [loaders-and-boot.md](07-loaders-and-boot.md) | Loader catalogue (OPL, forks, NHDDL/Neutrino), boot setups (FreeMcBoot, PS2BBL, MX4SIO, MMCE emulators), where ELFs go (SPEC LDR-*) |
| — | [REFERENCES.md](REFERENCES.md) | Index of reference repositories (local clones in `D:\yaoplm-testdata\refs\`) and forum threads (ADR-0015) |

Spot-checked by hand against OPL fork source (NathanNeurotic/Open-PS2-Loader @ 2089b79):
- the `USBExtreme_game_entry_t` layout (64 bytes);
- the `USBA_crc32` implementation;
- the `COMPAT_MODE_1..8` definitions. Upstream uses 1–6; 7 and 8 are RiptOPL-only.
