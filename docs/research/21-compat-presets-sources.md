# 21 - Compatibility presets (CFG-3): what OPL does, where the data is, and where the sources disagree

Date: 2026-10-27. Scope: CFG-3 (v0.3, M6), before any design: the owner's decision of 2026-10-27 is that presets need their own research, a reconciliation of the sources and a check against the OPL sources of every version before a line of code, and a user-facing page in the docs. The plan is `docs/plans/cfg-3-compat-presets.md`. Sources: the local clones from [REFERENCES.md](REFERENCES.md) (upstream OPL `ps2homebrew/Open-PS2-Loader@b3a21997`, tag `v1.1.0`, RiptOPL `NathanNeurotic/Open-PS2-Loader@38ea46ad`, wOPL `@4ae7414`, the MMCE fork `ps2-mmce/Open-PS2-Loader@e209011`, OPL-Evolution `@47119b7`, Neutrino `rickgaiser/neutrino@7be8de2`, NHDDL `@89141d4`, the three CFG corpora, PFS BatchKit), the GitHub API and live requests to OPL-CL on 2026-10-27. Nothing was copied: OPL and its forks are AFL-3.0 (ADR-0002).

Legend: **[verified]** read in the source or fetched today; **[secondary]** another tool's reading; **[unverified]** not reachable or not checked.

## 0. TL;DR

- Upstream OPL has had its own preset mechanism since 0.9.3: "Download Defaults" on a game and "Update game compatibility" in settings fetch a CFG fragment per (game ID, device) from **OPL-CL** (`sx.sytes.net/oplcl`), merge it into `CFG/<ID>.cfg` and mark it `$ConfigSource=2`. The server **answers today** (research/03 took it for dead): HTTP 200, records last modified in 2015, launchers listed up to "OPL v1.0.0". It is private, has no licence and no export.
- The mode bits are stable across upstream 1.1.0 and 1.2 (modes 1–6), but **0x40 and 0x80 changed meaning three times**: "High Module Storage" and "Hide DEV9" until 2018/2019, unused upstream since, then "Neutrino: fix game buffer overrun" (RiptOPL and wOPL, 0x40) and "Slow Reads" (RiptOPL, 0x80). Community CFGs from the 0.9.3 era still carry `$Compatibility=100` (0x64).
- RiptOPL ships a built-in per-title default table (two games) applied only while the game has no `$Compatibility` key, and maps OPL bits to Neutrino `-gc` digits, which are not one-to-one with OPL's modes.
- Machine-readable community data is thin and device-bound: GDX-X's 70 CFGs are HDD only; VTSTech/Tom-Bruise carry bits for 145 of 13,620 games with no device and no provenance; the ps2homebrew list has 98 games as per-report tables with a free-text "required modes" column and MIT licence. The lists disagree in coverage more than in values; the one game checked in four places (Fatal Fury BA2) agrees on mode 6 for HDD and says nothing for USB.
- No PC manager ships presets: OPL Manager edits modes by hand (research/01), PFS BatchKit copies GDX-X's CFG whole, OrbitPS2's "Modes Compatibility Database" issue found no takers.

## 1. What OPL itself does [verified]

### 1.1 The modes and their history

`include/iosupport.h` in `v1.1.0` and in `b3a21997` (1.2 beta) alike: `COMPAT_MODE_1` 0x01 Accurate Reads, `_2` 0x02 Synchronous (alternative data read), `_3` 0x04 Unhook Syscalls, `_4` 0x08 Skip Videos, `_5` 0x10 Emulate DVD-DL, `_6` 0x20 Disable IGR, `_7` 0x40 and `_8` 0x80 "Unused", `COMPAT_MODE_COUNT 6`. The menu labels are the hint strings of `lng_tmpl/_base.yml` (lines 254–264).

History of the two unused bits (`git log -S` on the header):

| Bit | Until | Commit | What it was |
|---|---|---|---|
| 0x40 | 2018-08-09 | `aea05e5b` "Removed High Module Storage mode (mode 7)" | HDD stutter fix of 0.9.x |
| 0x80 | 2019-01-28 | `b7732b25` "CDVDMAN's module ID will no longer be changed to dev9 … removed mode 8. Need For Speed: Underground 2 no longer needs mode 8" | "Hide DEV9 module" |

So a `$Compatibility` value with 0x40 or 0x80 from before 2019 describes a mode that no upstream build since 1.0.0 has, and the same bit means something else in the forks (§2).

### 1.2 The built-in updater: OPL-CL

`include/compatupd.h` (present in `v1.1.0` and 1.2; `src/opl.c:1260-1447`):

- Host `sx.sytes.net`, port 80, URI `/oplcl/sync.ashx?code=<startup>&device=<n>`, User-Agent `OPL/<version>`, three retries, a persistent connection.
- `device` is 1 for HDD, 2 for ETH (SMB; the record's mtime is then taken as GMT), 3 for BDM (USB, MX4SIO, iLink: all one device class). Other modes are "unrecognized".
- The response is CFG text; OPL reads it with `configReadBuffer`, **merges** it into the game's CFG (`configMerge`: the file's other keys stay), sets `$ConfigSource=2` (`CONFIG_SOURCE_DLOAD`) and writes the file.
- A game whose CFG says `$ConfigSource=1` (the user saved the settings menu) is **skipped** unless the settings toggle "Overwrites existing game compatibility settings when enabled" is on (`COMPAT_UPD_MODE_UPD_USR`). For a downloaded CFG the request carries If-Modified-Since from the file's mtime (`COMPAT_UPD_MODE_NO_MTIME` off).
- Entry points: per game, the "Download Defaults" button of the compatibility dialog (`COMPAT_DL_DEFAULTS`, `_STR_DL_DEFAULTS`, `src/guigame.c`); for a device, "Update game compatibility" in the network settings (`oplUpdateGameCompat`).

Live check on 2026-10-27 (three GETs with our User-Agent, nothing scraped):

| Request | Answer |
|---|---|
| `code=SLUS_200.62&device=3` | 200, `Content-Disposition: attachment; filename="SLUS_200.62.cfg"`, body `CfgVersion=1` (no modes), Last-Modified 2015-07-12 |
| `code=SLUS_217.23&device=1` (Fatal Fury: Battle Archives Vol. 2, HDD) | 200, `CfgVersion=1` / `$Compatibility=32` |
| `code=SLUS_217.23&device=3` (the same on USB) | 200, `CfgVersion=1` only |
| `code=SLES_511.14&device=3`, the four GT4 Prologue IDs | 404 or empty |

The public page `sx.sytes.net/oplcl/games.aspx` is an ASP.NET list with filters Region, Media (CD, DVD5, DVD9), Launcher ("OPL v1.0.0 (Stable)", "OPL v0.9.3 (Stable)", older marked obsolete), Device (HDD, SMB, USB), VMC and Compatible, behind a login for edits. Server header `Abyss/2.16 Win32`. No licence, no bulk export, no API beyond `sync.ashx`. Whether YAOPLM may make the same per-game GET OPL makes is the owner's call (plan, question 1); scraping the list is out (ADR-0007's spirit).

### 1.3 `$ConfigSource` is the ownership bit

OPL's own save writes 1 when the user touched anything but the VMC slots (research/15 §1.1); the updater writes 2 and later overwrites 2 but not 1. A preset YAOPLM writes therefore chooses between "the user's" (1: OPL's updater leaves it alone) and "downloaded" (2: OPL may replace it on the next update from OPL-CL). RiptOPL has no updater (§2.1), so there the bit only documents provenance.

## 2. The forks and Neutrino

### 2.1 RiptOPL (`NathanNeurotic/Open-PS2-Loader@38ea46ad`) [verified]

- `COMPAT_MODE_COUNT 8`: 0x40 "Neutrino only: fix game buffer overrun (`-gc=7`)", 0x80 "Slow Reads: Accurate Reads with DVD media at the CD rate (~900 KB/s), OPL core only" (`include/iosupport.h:96-105`).
- **Built-in per-title defaults** `sbTitleCompatDefaults[]` (`src/supportbase.c:798-826`): `SLES_511.14` Pro Evolution Soccer 2 (Europe) → mode 1 (streamed audio breaks without Accurate Reads, upstream issue #1686); `SLUS_217.23` Fatal Fury: Battle Archives Vol. 2 (USA) → mode 6 (black screen with IGR's VBLANK_END handler, upstream #1762, #984). A default applies only while the CFG has no `$Compatibility`; the per-game screen saves an explicit `$Compatibility=0` to turn it off (`sbGetCompatModes`, `guigame.c:1596`). The comment asks that every entry be evidence-backed, because it changes launches for everyone who never opened the game's settings.
- OPL bits → Neutrino: `convertCompatmaskToModes` (`src/system.c:1314-1332`) forwards bits 1, 2, 3, 5 and 7 as the digits `1 2 3 5 7` of `-gc=`; modes 4 (Skip Videos), 6 (IGR) and 8 (Slow Reads) are not passed to Neutrino.
- No `compatupd.h`, no `compatUpdate` in `src/opl.c`: the OPL-CL updater is removed.

### 2.2 wOPL (`ps2homebrew/wOPL@4ae7414`) [verified]

`COMPAT_MODE_COUNT 7`: 0x40 "Patch IOP buffer overrun (bug in the game) by Maximus32" in the OPL core itself, 0x80 unused. No updater. (Its CFG syntax is libconfig, CFG-1.)

### 2.3 The MMCE fork and OPL-Evolution [verified]

`ps2-mmce/Open-PS2-Loader@e209011` and `officialjuicedesigns/OPL-Evolution@47119b7`: upstream modes 1–6, 0x40/0x80 unused, `compatupd.h` and "Download Defaults" kept.

### 2.4 Neutrino and NHDDL [verified text, semantics unverified]

Neutrino's `-gc=<digits>` (README `@7be8de2`): 0 "IOP: Fast reads (sceCdRead)", 1 "dummy", 2 "IOP: Sync reads", 3 "EE: Unhook syscalls", 5 "IOP: Emulate DVD-DL", 7 "IOP: Fix game buffer overrun"; several digits combine (`-gc=23`). RiptOPL's digit `1` for OPL's mode 1 therefore lands on Neutrino's "dummy": Accurate Reads has no Neutrino equivalent, and Neutrino's `0` (fast reads) has no OPL bit. NHDDL keeps per-title Neutrino options in its own files (`nhddl.yaml` title options), not in `CFG/<ID>.cfg`. A preset for a Neutrino launcher is a different record from an OPL one, even for the same game.

## 3. Community sources, checked 2026-10-27

| Source | What it holds | Coverage | Device | Licence | Freshness | Machine-readable | Notes |
|---|---|---|---|---|---|---|---|
| **OPL-CL** `sx.sytes.net/oplcl` (§1.2) | CFG fragments per (ID, device) | unknown; sampled 7 IDs | HDD / SMB / USB | none, private | records 2015; launchers up to OPL 1.0.0 | yes, one GET per game, no export | what OPL's own "Download Defaults" uses |
| **ps2homebrew/Open-PS2-Loader-Compatibility-list** `@3fbbfaf` | per-game Markdown tables: Media, Image format, Loader, Device, **required modes**, VMC, IGR, PADEMU, GSM, Compatible, Tester, Console, Notes; one row per report | **98** game files | per row | **MIT** (GitHub API) | last push 2023-01-06; 31 open issues and PRs | rows parse; "required modes" is free text (`1`, `2, 6`, `1, 2, 3, 5, 6`; 66 of 98 rows empty) | Jekyll site; `script/ProcessData.sh` builds the pages |
| **GDX-X/PS2-OPL-CFG-Compatibility-Database** `@45c7e5b` | ready CFGs `HDD/<ID>.cfg`: `Title`, `$Compatibility`, `Modes=` (informational), `$ConfigSource=1` | **70** files (68 in `Games_List.txt`) | **HDD only** | **none** | last push 2024-07-16 | yes | two pseudo IDs (`SLUD_206.43`, `SLUG_202.73`) for ID conflicts; PFS BatchKit downloads the 7z release and extracts `HDD\<id>.cfg` over the game's CFG (`!PFS-BatchKit-Manager.bat:1472, 1701-1704`), replacing it |
| **VTSTech/PS2-OPL-CFG** `@c669f0c`, fork **Tom-Bruise/PS2-OPL-CFG-Database** `@7dc9204` (GPL-3.0) | 13,620 CFGs with titles and metadata | **145** carry `$Compatibility` ≠ 0: 60 × 32, 45 × 2, 8 × 4, 7 × 1, 6 × 16, 5 × 8, **5 × 100** (0x64 = modes 3, 6 and the old 7), 2 × 3, 1 each of 5, 7, 36, 38 | none stated | 2023-03 / 2021-11 | yes | no provenance per value; the 0x40 bits date them to the 0.9.3 era (OPL-CL, probably) |
| **RiptOPL built-in table** (§2.1) | two games with an issue each | 2 | any | AFL (data, not code: the IDs and bits are facts) | 2026 | yes | the only source that cites evidence per entry |
| **Upstream OPL issues** | reports with modes in the text | 175 issues match "mode 1", "accurate reads" or "compatibility mode" (GitHub search) | per report | — | live | no | evidence for a curated table, not data |
| psx-place "OPL game compatibility lists" thread, Algol's Google Sheet, ConsoleMods wiki | lists linked from the thread (research/03 §4) | — | per device tabs | none stated | — | Sheets CSV export **[untested]** | psx-place is behind a bot check: read through the owner's Chrome only (ADR-0015) **[unverified today]** |
| **OPL Manager** (closed source) | a CFG editor with every mode | — | — | — | — | — | no preset download known (research/01 §OPLM) **[unverified: check in the program]** |

## 4. Where the sources disagree, and what a preset must therefore carry

1. **Device.** OPL-CL keys every record by HDD / SMB / USB and the one sampled game has modes on HDD and none on USB; GDX-X is HDD only; the ps2homebrew list has a Device column per report; the VTSTech bits name no device. Read timing modes (1, 2) and DMA depend on the device. A preset without a device is a guess.
2. **Loader and core.** OPL-CL's launchers stop at 1.0.0; the ps2homebrew list names the build per row; RiptOPL and wOPL give 0x40 a new meaning and RiptOPL forwards only five bits to Neutrino. A value of `100` from 2015 written to a RiptOPL device would switch on "Neutrino: fix buffer overrun" for a game that once needed "High Module Storage". Bits 0x40/0x80 from old data must be dropped, never copied (the CFG-2 rule already keeps them only in the RiptOPL profile).
3. **Ownership.** `$ConfigSource` 1 or 2 decides whether OPL's own updater later overwrites the file (§1.3).
4. **Coverage, not values, is where they differ.** Fatal Fury BA2 (`SLUS_217.23`): OPL-CL HDD 32, OPL-CL USB none, VTSTech 32, RiptOPL default 32 (issue #1762), GDX-X absent, the ps2homebrew list absent. GT4 Prologue (`PBPX_955.23`): GDX-X 32 on HDD, OPL-CL none, VTSTech none. Pro Evolution Soccer 2 (`SLES_511.14`): RiptOPL mode 1 (issue #1686), VTSTech none, OPL-CL none.
5. **Image format and media.** The ps2homebrew list records ISO vs ZSO and CD / DVD5 / DVD9 per report; OPL's bits do not depend on the format except mode 5 (DVD-DL) for split-layer images.
6. **Licences.** MIT (the list), GPL-3.0 (Tom-Bruise) and public facts (bits per ID from issues) can be bundled with attribution in `data/ATTRIBUTION.md`; GDX-X has no licence (read, cite, do not bundle); OPL-CL is a private server (ask or leave to a per-game request the user starts).

A preset record therefore needs at least: game ID, device class (HDD / SMB / BDM), loader family and core (OPL core, Neutrino), the bits in OPL's numbering, the source with its row or issue, the reporter's build and date, and a note. Several records per game are normal; the app shows them, it does not average them.

## 5. What to read in the sources before the design (checklist for the plan)

- Upstream `src/opl.c` `compatUpdate`, `src/guigame.c` "Download Defaults", `include/compatupd.h` in `v1.1.0` and `b3a21997`: done above; re-read the merge rules when writing the ADR.
- The commits `aea05e5b` and `b7732b25` for what modes 7 and 8 did, and `lng_tmpl/_base.yml` for the current hints.
- RiptOPL `sbTitleCompatDefaults`, `sbGetCompatModes`, `convertCompatmaskToModes`, the commit of 2026-10-20 `6f7c7cdd` (Slow Reads); wOPL's 0x40.
- Neutrino's README `-gc` and its source for what `1` does ("dummy") and what the default read mode is **[unverified]**.
- The ps2homebrew list's `script/ProcessData.sh` and `_includes/table_header` for the exact columns; the 31 open issues and PRs for unmerged reports.
- OPL issues #1686, #1762, #984 (RiptOPL's evidence) as the model for an evidence-backed entry.
- The psx-place thread and Algol's sheet through the owner's Chrome; OPL Manager's current CFG editor in the program itself.

## 6. Options for YAOPLM (for the ADR, not decided)

- **A. Bundled, curated table** built by `scripts/build-compat.py` from the MIT list, the GPL-3 CFGs and hand-entered evidence-backed rows (RiptOPL's two, OPL issues), with provenance per row, into `data/compat.json.gz` plus `data/ATTRIBUTION.md`; rebuilt by hand like `data/titles.json.gz` (ADR-0024).
- **B. The same per-game GET OPL makes to OPL-CL**, only when the user asks for that game, with our User-Agent and a cache (ADR-0007 rules), shown as "OPL-CL says …". Needs the owner's decision: a private server with no licence.
- **C. Both**, with OPL-CL as one more source in the same record format.
- Always: the card shows the suggestions with their source, device and build; the user picks; the write is a CFG-2 plan with a backup (CFG-5); bits 0x40/0x80 from old data are dropped; a RiptOPL device gets the mapping of §2.1; a Neutrino launcher gets no OPL bits at all; `$ConfigSource` per the ADR.
- User docs: a page on what the six modes do, where YAOPLM's suggestions come from, why a suggestion may not fit (device, build), and how to turn one off (`$Compatibility=0` in RiptOPL vs removing the key upstream).
