# 01 — Competitive analysis: PC-side managers for Open PS2 Loader (OPL)

Research date: 2026-10-22. Method: WebSearch + WebFetch only (web pages, GitHub pages/API JSON, PyPI, archive.org metadata). No binaries were downloaded or run.

**Legend used throughout**
- **Y** = supported (verified from the tool's own site, README, changelog, release notes or issue tracker)
- **P** = partial / limited (explained in the cell or notes)
- **N** = explicitly not supported, or listed as TODO/WIP by the author
- **—** = not mentioned in any source I could read (treat as "probably not", but **unverified**)
- **?** = conflicting or second-hand information — **unverified**

Access limitations: psx-place.com, ps2-home.com, reddit.com, web.archive.org and bitbuilt.net all returned 403, were unreachable, or were in maintenance during this research, so first-hand forum complaint threads could not be read in full. User-pain evidence below comes from reachable pages (CachyOS forum, Lutris, GitHub issues, lecrabeinfo comments, search-engine snippets of psx-place posts). Snippet-only evidence is marked as such.

---

## 1. OPL Manager (OPLM) — the incumbent

| Field | Value |
|---|---|
| Author | danielb (archive.org uploader `danielb7390`) |
| License | Proprietary freeware, closed source (AlternativeTo: "Proprietary and Free product") |
| Platforms | Windows only. Windows 7 SP1+ and the .NET 6 runtime (x86 or x64) are required. Distributed as a zip (`OPL_Manager_V24.zip`, ~19.7 MB) |
| Latest version | **V24.0, 2024-09-15** (oplmanager.com changelog; lecrabeinfo listing agrees) |
| Status | **No release in ~25 months.** The site is still online. It warns that versions older than V23 can no longer download or upload ART. .NET 6 left Microsoft support in Nov 2024 (general knowledge, not from OPLM's site) |
| Community | Facebook page, a psx-place thread (#19055) and resource (#663), and a ps2-home forum section (f=64) |
| Linux/macOS | Only through Wine. A Lutris installer needs **exactly .NET 6.0.20 Desktop Runtime x86** ("other .NET 6 versions will show an error at launch") plus a manually installed Microsoft Sans Serif font ("without it OPL Manager crashes at startup"). It sets RUNASADMIN and disables auto-update |

### 1.1 What OPLM does (from the full official changelog V1–V24, plus docs)

**Library / games**
- ISO renaming and detection of badly named ISOs at startup (V3). Supports both naming schemes: the "old" `SLUS_123.45.Title.iso` and the "new" `Title.iso` without an ID. The default flipped several times (V18, V19, V20, V21.6). Docs say the old format is required since V19.1 because the new one "can cause problems in OPL".
- Game-ID detection from inside the image via .NET DiscUtils (V15). A "Bad ISOs" tab with DB name lookup (V16).
- **USB Extreme/Advance (ul.cfg)** support (V15). Fixed: a ul.cfg crash (21.4) and UL games with non-standard IDs (V20).
- ISO creation tool "from disc images" (V15). Multi-disc handling and convert-to-ISO without overwriting (21.9). *Which input formats (BIN/CUE, NRG, MDF…) are accepted is **unverified**.*
- **ZSO**: reads ZSO games (22.0). ISO↔ZSO converter, plus **rip CD/DVD directly to ISO/ZSO** (22.1). No CSO, which is correct because OPL does not support CSO.
- MD5 game hasher (V9), Redump links (21.8), a "clean files" tool (21.8).
- Game list filtering and sorting. Double-click launches the game (in PCSX2, V6/V18).
- **CSV game-list export** (V16). V24 added a type column and translated columns.
- **OPL Simulator** (V18): a PC mock-up of the OPL UI with XML themes (only one theme exists). It can launch games in PCSX2.

**ART (cover art)**
- Download of covers and disc icons (V4), batch download (V5), and a **custom ART database on OPLM's own server** (V6). The "download from OPL server" option was removed in V19, so **OPLM's own server is the only source**.
- Art types (docs): ICO 64×64, SCR/SCR2 250×188, BG 640×480, LGO 300×125. PS2: COV 140×200, LAB 18×240, COV2 242×344. PS1: COV 200×200, LAB 12×200, COV2 222×200.
- **Community upload and sharing**: Batch Art Share, screenshot sharing, PS1 ART sharing. Users can report bad art (an explanation is required, V18). Art that misses the guidelines is deleted from the server.
- ART Manager (V15), ART Report, drag-and-drop art out of the manager, batch replace of existing art (21.3), ART folder cleaner, LAB/LOGO art support. V23 enforces 8-bit PNG and flags art that doesn't comply. V24 re-added resize/conversion and moved to 128×128 icons.
- **ART DB backups** are published on archive.org (`OPLM_ART_YYYY_MM`). There are 44 backups from 2018-11 to **2024-09, the last one** (`OPLM_ART_2024_09`, 5.2 GB zip, no license stated). An archive.org search shows no later OPLM_ART item.

**Per-game config, VMC, cheats**
- **CFG Editor** (V6). It grew to cover compatibility modes, DMA, GSM, V-Mode, colours, genres, ELSPA/other ratings, "sports genre", cheat-device fields, character limits, `$GSMFIELDFix` (21.7), and VMC size selection and browsing (21.3). It reads and writes OPL's new **global + per-game settings** with automatic CFG upgrade (21.6). Game CFG "version 4" (V18).
- **VMC creator** (V6). VMC naming fixes followed. Renaming a VCD also renames its VMC folder (21.4).
- **Cheat Editor** (V12) and **PS1 Cheat Editor** (V21).

**PS1 / POPS and APPS**
- POPS ELF and CFG support (V18). POPS moved to VCD files, with a VCD renaming tool, LOGO art and PS1 cheats (V21). POPS folder search at the device root (21.1). VCD names are limited to 32 characters with duplicate-ID handling (21.4/21.5). POPS detection through `conf_apps.cfg` (22.4). *CUE/BIN→VCD conversion inside OPLM is **unverified**.*
- ELF installer tool (V18), app installer device tweaks (21.7), APPS via `title.cfg` (V23).

**Devices**
- **Local PS2 HDD (APA)** support (V16) through bundled **HDL Dump** (selectable versions 86/90/92, V21). A "run as admin" tip for HDL Dump (21.3). Fixed HDD connection with many drives (21.8).
- "HDL server and FTP management (WIP)" (V13) and **FTP sync to PS2** (V15).
- SMB/network share: OPLM works on a local or mapped folder. A dedicated "SMB share" mode is mentioned only in a paywalled French tutorial snippet, so it is **unverified**. Direct support for OPL's NBD server is **unverified**. OPL ≥1.1 replaced the HDL server with NBD, and on Windows an NBD-mounted disk would need the wnbd client.

**Other**
- Multi-language since V12. The changelog names English, Portuguese, Spanish, Chinese, French, Italian, Russian, Bulgarian and Hebrew (at least 9). It shows translation percentages and asks for a language on first run (21.7).
- Auto-update checker (V8). The update check can be disabled (V11).
- **Telemetry**: "anonymous usage data (opt-out)" (V11) and "unique user identifiers" (V20).
- Known friction:
  - Windows 7 needs a manual ISRG Root X1 certificate to reach the server (docs "FixSSL").
  - Older builds needed MSVCR100.dll.
  - HDD mode needs admin rights.
  - One user comment (lecrabeinfo) says >4 GB games split by USBUtil show as 0 bytes in OPLM (anecdotal, **unverified**).

---

## 2. Open-source / alternative PC managers (folder-based: USB, exFAT, MX4SIO, SMB share)

### 2.1 OPL PC Tools (brainstream/OPL-PC-Tools) — the closest OSS desktop rival
- **License** GPL-3.0. **Stack** C++ / Qt (Qt6 builds), LZ4. **Platforms**: Windows (amd64 zip; x86 dropped) and Linux (tar.gz, **AppImage since 4.0**). No macOS build.
- **Latest**: **v4.0, 2026-08-30**. Before that: 3.1 (2025-07-13), 3.0 (2021-04-18), 2.x (2020). Repo created 2017. 227★, 24 forks, 474 commits, 3 open issues.
- **Features**:
  - Library browser with custom images for every art type (v2.0).
  - Automatic cover download for one game or all, from **archive.org** (the `PS2_OPL_ART_kira.7z` pack, not OPLM's DB).
  - Sequential install from CD/DVD discs or ISO/BIN/NRG/ZSO.
  - ISO↔ZSO↔UL conversion and rebuilding an ISO from an installed game.
  - Import from other OPL storages.
  - Editing of standard game settings (v3.0).
  - VMC create/read/write with an explorer (read-only in 3.0, write in 4.0).
  - **Cheats with automatic widescreen-patch download** (4.0).
  - Dual-layer DVD support. Portuguese translation file in the 2.4 assets.
- **Gaps / complaints**:
  - No PS2 HDD access on Linux (#38).
  - Silent failure where only ul.cfg was written (#36).
  - SSL error on cover download under Windows 10 (#31).
  - Users asked for one-click bulk cover download "like OPL Manager" (#30) and for cheat support (#29, later delivered).
  - Flathub request (#41, open).
  - The CachyOS thread (2026-07-12) shows the Linux tarball failing on a `libssl.so.3` / OPENSSL_3.5 symbol. There was no reply.
- No PS1/POPS, no APPS and no CSV export are mentioned.

### 2.2 OrbitPS2 Manager (Luden02/OrbitPS2-Manager; formerly "OrbitOPL Toolbox") — strongest OSS rival, covered in depth

**Identity and status**
- **License** GPL-3.0. **Stack**: Electron + Angular + Tailwind/daisyUI, TypeScript. The main process is split into `src/ipc/*`, `src/services/*`, `src/utils/*` and a ZSO worker pool. Vitest/Jasmine tests exist for the ID resolver, artwork naming, rename, delete and update.
- **Platforms (release assets)**:
  - Windows x64: NSIS setup .exe and portable zip.
  - **macOS arm64 + x64 .dmg**.
  - Linux x64: AppImage, .deb, .rpm, zip. AUR is mentioned in the README.
  - Builds are **unsigned**: the notes give SmartScreen and macOS quarantine-removal instructions, and #23 reported "app is damaged" on macOS.
- **Release history**:
  - v0.2.4-beta (Apr 2026, PS1 hotfix), v1.0.0 (2026-06-16, first stable), v1.0.1, v1.1.0 (2026-06-29).
  - v1.2.2 (2026-07-16), v1.3.0 (2026-08-09).
  - **v1.3.1 "Rebranding!" (2026-08-28, latest stable)**, renamed from OrbitOPL Toolbox.
  - Rolling **Nightly** pre-release on every push to main since #51 (asset date 2026-10-04).
  - Beta "0.1" UI feedback dates to Dec 2025.
- **Repo**: created 2025-08-05. 127★, 11 forks, 6 watchers, ~139 commits, 7 open issues.
- **Last commit 2026-10-22** (`cdd590a` "SP2 DVD rip from disc", by Stefan Adrian Nita). Previous: RiptOPL Hardening #50 by **NathanNeurotic (Ripto, the RiptOPL author)** on 2026-10-09.
- Contributors besides Luden include Stefan Adrian Nita (who did most 1.3.x and rebrand commits), Rafael Pazini, German Emmanuel, dnunezx (the LUNA author) and PixeliGer (theme author). One merged PR is tagged "[codex]", a sign of LLM-assisted contributions.
- **RiptOPL's README recommends OrbitPS2 Manager as its PC companion** (disc import, artwork, ZSO, per-game settings, VMC). In effect it is becoming the "official" manager of the RiptOPL ecosystem.
- The README says it fills the gap that OPLManager lacks macOS/Linux support (seen in a search summary).

**Features as implemented** (README plus source files read: `artwork.service.ts`, `library.service.ts`, `cfg.service.ts`, `vmc.service.ts`, and the repo tree)
- **Library**:
  - Scans `CD`, `DVD`, `VCD`, `POPS` (.iso/.zso/.vcd), `EMBER/games/<folder>/*.cue` (RiptOPL's Ember PS1 core), `ART` (.png/.jpg) and APPS.
  - Grid and list views, search/sort, details view built from the .cfg, stats.
- **UL games**: `getULGames` **parses ul.cfg** (64-byte records: name 0–31, ID 32–46, part count 47, media type at 48 where 0x12 means CD) and sums the `ul.*` part sizes.
  - **It never writes ul.cfg**: no UL creation or splitting, so **FAT32 users cannot install >4 GB games with it**.
- **Import**:
  - PS2 ISO/ZSO.
  - PS2 CD BIN/CUE, with multi-track merge via a `binmerge` port.
  - PS1 CUE/BIN/ZIP → VCD via a `cue2pops` port, with POPStarter (`XX.`/`SB.` prefixes) or RiptOPL/POPSLoader naming. PS1 multi-disc labels and a `ps1-disc-conflicts.json` database.
  - ELF apps (`title.cfg` service).
  - Import from an optical drive (`optical-drive.service`, `cd-import.service`, #49).
  - Queued jobs with progress, and a close guard while importing.
  - The game-ID resolver has regression tests. PS1 IDs come from the PVD.
- **Compression**: ISO↔ZSO (LZ4, multithreaded worker), VCD→BIN/CUE, and optional deletion of the source file.
- **Artwork**:
  - The only source is the GitHub repo `Luden02/psx-ps2-opl-art-database` (a static dump of OPLM's DB), fetched via `raw.githubusercontent.com/.../refs/heads/main/{PS1|PS2}/{ID}/{file}`.
  - Directory listings go through the **unauthenticated GitHub REST API** (`api.github.com/repos/.../contents/...`). HTTP 403 is treated as a rate limit, with no retry, no cache and a 30 s timeout. *The 60 requests/hour limit for unauthenticated calls is general GitHub knowledge, not stated in the code, so bulk runs on a big library are likely to hit it (inference).*
  - Downloads are normalized through `normalizeArtworkPng` to the **8-bit indexed PNG that RiptOPL (and OPL) require**.
  - There is an artwork wizard with loader presets, bulk scopes and "skip existing".
  - RiptOPL naming and dimension standards are implemented (#48/#50): `<ID>_<TYPE>.png` for PS2, `<VCD name>_<TYPE>`, `<Ember folder>_<TYPE>`, `<ELF>.ELF_<TYPE>`. Sizes: ICO 128², PS2 COV 280×400, PS1/APP COV 400×400, plus SCR, BG, LGO, LAB and COV2.
  - The art types downloaded are COV, ICO, SCR and BG, per the README/#45. COV2/LAB/LGO downloads are **unverified**.
- **Per-game CFG**: `CFG/<GAMEID>.cfg` is read as a generic KEY=VALUE map and written back with CRLF. The UI exposes the title, `$Compatibility` modes and `$VMC_0/1` slots. Writing an empty map deletes the file.
  - **No `conf_game.cfg` global settings** and **no backup**. `.cfg.bak` files are skipped.
  - Unknown keys survive only if the UI passes back the whole map (the doc comment claims they do).
- **VMC**: creates **zero-filled** 8/16/32/64 MB `VMC/<name>.bin` files; OPL formats them on first boot. Lists and deletes cards and detects POPStarter `SLOT0/1`. **There is no save browsing**.
- **Maintenance**: invalid-file detection, bulk auto-correct, old/new naming-scheme switch (#10), filename sanitization (#9), and safe delete of a game with its art and launchers.
- **Other**: themes, live logs with verbose mode, update check (ignores pre-releases), custom title bar for Linux tiling window managers, Robocopy unbuffered copy on Windows (#46), and symlinked NAS folder support (RetroNAS, #20).
- **Absent (verified in source/tree: no paths match)**:
  - APA/PFS HDD and NBD.
  - ul.cfg writing.
  - Cheats.
  - SMB client: it only works on a share the OS has already mounted.
  - i18n (UI is English only).
  - CSV export, MD5/Redump, telemetry.

**Issue tracker: complaints and wishes (54 items, read through the API)**
- **Open**:
  - #54 HDD install very slow (9 games/38 GB took more than 1 hour, vs FHDB/HDL Batch Installer), and no speed shown.
  - #53 PSBBN/Luna art.
  - #52 PS1 BIN/CUE without VCD (for Ember).
  - #42 PS1 games tagged as PS2 DVD after conversion.
  - #35 Linux Mint crash when scanning ART; HD art asked for.
  - #31 "Modes Compatibility Database" (community DB of suggested modes per loader, labelled help-wanted, no takers).
- **Declined**:
  - #43 CHD support (wontfix).
  - #24 **web UI for a headless server/NAS** (wontfix).
  - #21 move from Electron to native Rust/Slint (not planned).
- **Fixed but telling**:
  - #4 xlenore covers are 32-bit while OPL needs 8-bit.
  - #6 ZSO flagged invalid.
  - #8 doesn't open on Windows 8.1.
  - #11 macOS SMB share shows no games.
  - #18 imported games not visible.
  - #23 macOS "damaged".
  - #26 portable .exe closes after 4 s on Win10.
  - #28 permission denied on RetroNAS dirs.
  - #36 artwork UX "overcomplicated".
  - #38 POPSTARTER.ELF missing in the Linux RPM.

### 2.3 PyOPLM (edisnord/PyOPLM; PyPI `pyoplm`) — fork of libopl
- **License** GPL-3.0-or-later. **Stack** Python ≥3.10, CLI only. **Platforms**: Linux native. Windows only through Docker (helper repo). BIN/CUE and POPS install is x86_64 Linux only.
- **Latest**: **0.73, 2026-09-29**. Earlier: 0.72 (2026-08-11), 0.71 (2025-12-04), 0.63 (2025-02-07). 19★, 66 commits. Revived in 2025–26. One merged branch is named `claude/dreamy-gates-md733a`, a hint of LLM-assisted maintenance.
- **Features**:
  - Commands: `list / add / delete / rename / fix / init / storage / convert / bintools`.
  - ISO, UL and POPS games. Read/write/fix **ul.cfg**.
  - ISO↔ZSO (0.72). ZSO files must already carry the ID in the filename.
  - Python ports of bchunk, binmerge and cue2pops.
  - Art and titles come from **OPLM's archive.org backups**, either unzipped locally or self-hosted on static storage (e.g. a GCS bucket). The tested backup is `OPLM_ART_2023_07`. Zip-index mode reads straight from the archive.org "view contents" listing.
  - Config lives in `pyoplm.ini` in the OPL directory, or the env var `PYOPLM_OPL_DIR`.
- **N/TODO**: CFG editor and RAWG.io metadata (both TODO). HDD support is "WIP". No VMC, no cheats.

### 2.4 libopl (Nold360/libopl; PyPI `libopl`, command `opl`/"popl")
- **License** GPL-3.0. Python CLI plus a library. Linux only ("port yourself").
- **Status: abandoned.** There are 7 commits, the last on **2019-08-31**. 11★, 3 open issues, no releases.
- Features: add images (optionally split to UL), read/write/merge ul.cfg, download artwork "from open API" (which API is **unverified**), list, init, fix names, recover game IDs from filename or image.
- The author calls it "Very buggy, alpha state code".

### 2.5 OPLNova (WeeXnes) — *audited separately, one-row mention only*
- C# / Avalonia GUI that bills itself as "open-source, cross-platform alternative to OPL Manager". Ships as AppImage + AUR. VMC is "partially experimental". The psx-place thread #50635 is about 3 months old (all from a search snippet).

### 2.6 USB-split / ul.cfg utilities
- **USBUtil 2.0 / 2.2 (closed freeware, Windows)**: the classic tool to split ISOs into USBExtreme `ul.*` parts and maintain `ul.cfg`. The author and release dates are **unverified**. Search results are dominated by SEO spam pages with conflicting claims. The latest named file found is "USBUTIL v2.2 rev1.0 EN" (lecrabeinfo). Now superseded by OPL's direct ISO/exFAT/ZSO support.
- **USB Advance / USB Extreme**: mid-2000s commercial PS2 USB loaders. OPL inherited their split format and `ul.cfg` index. Dates and vendors are **unverified**. The OPL wiki states that ul.cfg gets fragmented on each later install, and recommends installing big games in one session.
- **PS2IsoManager (imadali120)**: MIT. C#/.NET 8 WPF, Windows only. "A modern replacement for USBUtil". 22★, last push 2026-03-03, no releases visible.
  - Features: 1 GiB splitting, ID detection from SYSTEM.CNF, ul.cfg add/delete/rename, themes, drag-drop.
  - Covers come from **xlenore/ps2-covers** and are saved as `ART/{ID}_COV.jpg`. Forum posts say current OPL reads only PNG covers, so the JPG output may be **incompatible**. This is unverified and not tested.
- **Tihwin (developersu)**: GPL-3. Java (jar + Windows exe/NSIS installer). "Sort of ul.cfg manager or USBUtil". Localized into several languages. **v2.6, 2025-12-23**. 52★.
- **POUM (gecm0)**: Tauri v2 (Rust) + React. MIT according to the README (GitHub API shows no license). ISO→USBExtreme, batch import, space check. 5 commits, 0★, no releases.
- **OPLUtil (IsseiYoshida)**: 109★ but only 4 commits, no license, no releases, last update 2021-05. Effectively dead.
- **USB TOOLS (Android app, closed)**: APKMirror listings 10.1.x/10.2.0 include a "PS2 USB Utils – OPL Games Utility" module. It can auto-generate ul.cfg and OPL playlists and import .ul games, over OTG without root. The developer is **unverified**.

### 2.7 Long tail found by GitHub search (mostly 2025–2026, ≤16★, many look LLM-assisted)
- dheison0/ps2-game-manager: Go TUI, MIT, Linux/macOS, 16★, pushed 2026-10-18. Install, rename, remove, cover download, export to ISO, cleanup.
- lucasonline0/opl-linux-toolbox: TS, GPL-3, 2★.
- gpulch/ps2-manager: TS.
- ShaunCarrPHL/PS2FASTLOADER: Rust, MIT.
- JO0812/TinyPS2Manager: Go, "cross-platform".
- logi-26/ps2-game-manager: Java, GPL-3.
- donydaily/PS2OPLManager-cli: Python.
- tekmyster/ps2-opl-ripper: Python, Windows disc ripper plus USB manager.
- Zsubzeroz/PyOPL-Utility: Python, MIT.
- TonyLimaPHB/PopCraft: Python, CHD/CUE/ISO→VCD for POPStarter.
- Informaldehyde-xd/PS2-MANAGER-FOR-ANDROID-: Kotlin, MIT, 0★.
- N0NamedGuy/opl-manager-next: JS, stale since 2023.
- astaroth666/PS2-Cover-Art-: bash script.
- akilluminati47/RIPPS2: a RiptOPL-fork launcher (on-console, not a PC tool).
- **Equiel-1703/ps2-network-manager**: Python GUI that sets up a Samba share for OPL on Linux. MIT, 151★. Shows demand for an easy SMB setup.
- milouk/docker-smb-opl: Dockerized Samba for OPL.
- No browser-based (web) OPL manager was found.

### 2.8 Target loaders a manager must serve (not PC tools, but they set the file formats)
- **OPL upstream (ps2homebrew)**: the last stable is v1.1.0 (2021-09-09). A rolling 1.2.0-beta "latest" build (2026-10-13) adds exFAT, ZSO and 64-fragment tolerance. Its README notes that "RiptOPL is a fork of this project which heavily uses LLM tools".
- **RiptOPL (NathanNeurotic/"Ripto", repo NathanNeurotic/Open-PS2-Loader)**:
  - AFL-3.0, 65★, about 4.2k commits on `rebuild/main`, 16 open issues. Rolling dev builds only ("inceBtion", "Korium" and RetroAchievements kits); no version numbers.
  - Bills itself as an "opinionated … 'definitive build'".
  - Uses the Neutrino core per game for UDPFS/UDPBD; SMB/HTTP stay on the OPL core.
  - Adds an **HTTP** source, internal HDD exFAT (BDMAssault), APA safety rules, favourites, Coverflow/PSBBN-Flow themes, DualSense support and parental locks.
  - **Two PS1 cores**: POPStarter (`*.VCD`) and **Ember** (BIN/CUE game folders under `EMBER/games/`).
  - Separate settings file `settings_riptopl.cfg`. ZSO is supported (but Neutrino cannot read ZSO or UL).
  - Art rules, implemented by OrbitPS2 (#48): **8-bit indexed PNG only**. ICO 128×128, PS2 COV 280×400, PS1/VCD/Ember/APP COV 400×400. Names keyed by disc ID, VCD filename, Ember folder or `ELF.ELF_<TYPE>`.
  - RiptOPL's README lists its recommended PC tools: **OrbitPS2 Manager**, PS2-Servers (Ripto's SMB/UDPFS/UDPBD/HTTP server launcher), PS2RD CHT Manager, OPL PS1 AIO Converter GUI and udpfs-server (Android). It names **LUNA** as the simpler alternative.
- **wOPL / uOPL** (KrahJohlito lineage; ps2homebrew/wOPL 66★): forks with Neutrino-loader, Coverflow and Favourites concepts.
- **NHDDL + Neutrino**:
  - ISO only (no ZSO, no UL) on ATA-exFAT, APA (HDL), MMCE (SD2PSX/MemCard PRO2), MX4SIO, USB/BDM, UDPFS and iLink.
  - Per-game YAML config (`nhddl/<ISO name>.yaml`).
  - OPL-style `ART/<ID>_COV.png` at 140×200. Its README recommends OPL Manager for art.
- **PSBBN / LUNA / HOSDMenu / FHDB**: HDD-OSD/PSBBN front-ends with their own art and icon databases (PSBBN Art DB, HDD-OSD-Icon-Database).
- **POPStarter vs POPSLoader**: different PS1 naming and prefix conventions (`XX.`/`SB.` prefixes, VMC `SLOT0/1`).

### 2.9 Shared data sources the ecosystem relies on
- **OPLM ART DB**: closed server, plus archive.org backups up to 2024-09 (5.2 GB), plus the static GitHub dump `Luden02/psx-ps2-opl-art-database` (29★, "static archive — no updates", no license, "All assets belong to their respective owners").
  - It is used directly or indirectly by OrbitPS2, PyOPLM, PSBBN Definitive (for OPL/NHDDL covers) and HDL Batch Installer (configurable archive.org source "same layout as OPLM backups").
  - The **NHDDL README recommends OPL Manager for getting art**.
- **xlenore/ps2-covers**: ~4.8k★, ~10.2k covers out of ~12.8k serials, used by PCSX2's cover downloader. No license shown.
- **archive.org `PS2_OPL_ART_kira.7z`**: used by OPL PC Tools.
- **PS2-Widescreen/OPL-Widescreen-Cheats**: `.cht` files converted from PCSX2 pnach. Used by OPL PC Tools and HDL Batch Installer. The repo is marked **"CURRENTLY ABANDONED"** and is looking for maintainers.
- **Tom-Bruise/PS2-OPL-CFG-Database**: GPL-3. 13,620 CFG files, with metadata (description, players, genre, date, developer, rating) scraped from ScreenScraper, in EN and FR. Its README tells users to copy the files in with OPL Manager.
- **CosmicScale HDD-OSD-Icon-Database**: used by HDL Batch Installer 3.7 and PSBBN.

---

## 3. PS2 internal HDD (APA/PFS) tools

| Tool | License | Platform / UI | Last release / activity | Notes |
|---|---|---|---|---|
| **hdl-dump** (ps2homebrew) | GPL-2 (COPYING file; GitHub API shows none) | CLI. Linux, macOS, FreeBSD, Windows (MinGW). The Windows GUI build is old and the WineLib build is "not working" | "latest" dev build 2024-06-06. Last numbered release rev47 (2015). Last push 2026-05-19. 153★, 27 open issues | `inject_cd/inject_dvd/install/copy_hdd/inject_mbr/modify_header`, hide/unhide. ZSO needs the original ISO alongside. The UDP server is deprecated in favour of OPL's NBD server. The de-facto engine under most GUIs |
| **pfsshell / pfsfuse** (ps2homebrew) | GPL-2 (libs AFL-2) | CLI. POSIX and Win32. pfsfuse runs on Linux FUSE, macOS macFUSE and Windows Dokany | Last push 2026-06-03, 129★ | Browse, get/put, mkfs, create/remove partitions, mount a PFS partition from a disk, image or **OPL NBD server** |
| **HDL Batch Installer** (israpps) | GPL-3 | **Windows only** (wxWidgets GUI, x64; 32-bit ended at 3.6.0.2) | **v3.7.0 rev3, 2026-03-25**, 328★ | Batch install/extract, auto title. MBR.KELF inject. Mass KELF and icon inject (HDD-OSD-Icon-Database). Downloads art, widescreen cheats and OPL game settings (configurable source incl. archive.org OPLM layout). Mounts PFS via Dokan/dokan2. One-click copy to the OPL partition. HDD-to-HDD transfer. Partition create/delete. HDD format wizard and HDDManager. MD5 check. Translations (template plus Traditional Chinese) |
| **PFS BatchKit Manager** (GDX-X) | none stated | Windows batch scripts | Pushed 2026-10-16, 268★ | PS2 from ISO/BIN-CUE/ZSO/zip/7z/rar. PS1 BIN/CUE→VCD into `__.POPS`. NBD mount workflow (driver, test mode). Auto-detects the OPL resource partition. HDD info. PSBBN/PSX-DESR XMB integration. Up to 2 TB. Users report NBD crashes on newer OPL builds (GBAtemp snippet) |
| **PSBBN Definitive Project** (CosmicScale) | GPL-3 | **Linux shell scripts** (apt/pacman/dnf/emerge/Nix; ARM64 tested). Windows 10/11 through a launcher that sets up WSL | Changelog 2026-09-17, **988★** (largest OSS PS2-HDD project) | Full HDD setup. ISO/ZSO/BIN-CUE, PS1 VCD/BIN-CUE, ELF/PSU. Sync / "add additional" modes. **APA-Jail** (exFAT + PFS on one disk), with exFAT up to 2 TB. OPL or **NHDDL/Neutrino** launcher. Art from the PSBBN Art DB (IGN fallback) and from the **OPLM art DB** for OPL/NHDDL. Generated 3D icons. 7 UI languages |
| **PS2 HDD Manager** (L10N37) | "Other" (custom) | Desktop. Fedora/Linux validated, Windows "experimental" | v0.2.0 (2026-09), 5★ | Format and provision a drive (OPL, FHDB, wLaunchELF…). **Extended APA >2 TiB** (needs a matching custom OPL build; tested with 4 TB / 1,274 games). Persistent transfer queue with a fit warning. Installs art |
| **WinHIIP** | closed freeware | Windows GUI | Old; lecrabeinfo calls it **obsolete** and recommends HDL Dump / HDL Batch Installer | Formats a drive to PFS and copies ISOs for HDLoader/OPL. The 2 TB maximum comes from a secondary source (**unverified**). Author and version **unverified** |
| OPLM "Local PS2 HDD" mode | proprietary | Windows | V24 (2024) | Wraps hdl_dump and needs admin rights |

OPL-side facts that shape HDD support:
- OPL's expansion-bay HDD can be APA+PFS (≤2 TB) **or MBR/GPT with FAT32/exFAT** (1.2.0 beta).
- The OPL partition is `+OPL` by default, overridable via `conf_hdd.cfg`.
- OPL's NBD server exports only the internal HDD. Clients: nbd-client/nbdfuse on Linux/WSL2 and wnbd on Windows. **macOS has no client.**

---

## 4. Feature matrices

### 4.1 Folder-based OPL managers (USB / exFAT / MX4SIO / SMB folder)

| Feature | OPL Manager V24 | OPL PC Tools 4.0 | OrbitPS2 1.3.1 | PyOPLM 0.73 | libopl | PS2IsoManager | Tihwin 2.6 | USBUtil 2.x |
|---|---|---|---|---|---|---|---|---|
| License | Proprietary freeware | GPL-3 | GPL-3 | GPL-3+ | GPL-3 | MIT | GPL-3 | Proprietary (?) |
| Windows | Y (7 SP1+, .NET 6) | Y (amd64) | Y | P (Docker) | N | Y (.NET 8) | Y (exe/jar) | Y |
| Linux | P (Wine, fragile) | Y (AppImage/tar) | Y (AppImage/deb/rpm/AUR) | Y | Y | N | Y (Java) | N |
| macOS | N | — | Y (arm64+x64) | ? (Python, untested) | — | N | Y (Java, per README) | N |
| UI | GUI | GUI (Qt) | GUI (Electron) | CLI | CLI + Python lib | GUI (WPF) | GUI (Java) | GUI |
| Last release | 2024-09-15 | 2026-08-30 | 2026-08-28 (+nightly 2026-10) | 2026-09-29 | commit 2019-08-31 | push 2026-03-03 | 2025-12-23 | ? |
| Add ISO to CD/DVD folders | Y | Y | Y | Y | Y | N (UL only) | N (UL only) | N (UL only) |
| UL split + ul.cfg write | Y | Y | P (parses ul.cfg; **never writes/splits**) | Y (read/write/fix) | Y | Y | Y | Y |
| Game-ID detection from image | Y (DiscUtils) | ? (implied) | Y | Y (not inside ZSO) | Y | Y (SYSTEM.CNF) | ? | ? |
| Rename / naming-scheme fix | Y (old/new format) | ? | Y (switch convention) | Y (`fix`, sanitize) | Y | Y (ul.cfg) | ? | ? |
| ZSO read / ISO↔ZSO | Y | Y | Y (multithreaded) | Y | N | N | N | N |
| CSO | N | N | N | N | N | N | N | N |
| PS2 CD BIN/CUE → ISO | P (formats ?) | Y (+NRG) | Y (multi-track) | Y (bchunk/binmerge ports) | N | N | N | — |
| Rip physical CD/DVD | Y | Y | Y | N | N | N | N | — |
| Restore ISO from UL/ZSO | ? | Y | Y (ZSO→ISO) | Y (convert) | N | — | — | ? |
| Art download | Y (own server) | Y (archive.org pack) | Y (GitHub raw of the OPLM-DB dump via the unauthenticated GitHub API; no fallback) | Y (OPLM backups, self-host) | Y ("open API" ?) | Y (xlenore, COV only, JPG) | N | N |
| Art types (COV/COV2/ICO/LAB/SCR/BG/LGO) | Y (all) | Y (all editable) | P (COV/ICO/SCR/BG; others ?) | Y (whatever is in backup) | ? | P (COV) | N | N |
| Art upload / community DB | Y | N | N | N | N | N | N | N |
| Art validation / resize / 8-bit PNG | Y | — | Y (8-bit normalize; RiptOPL sizes; skip existing) | — | — | — | — | — |
| Per-game CFG editor | Y (most complete) | Y (standard settings) | Y (generic KEY=VALUE; UI: title, `$Compatibility`, `$VMC_*`; no backup) | N (TODO) | N | N | N | N |
| Global settings (conf_game) | Y | — | N (source) | N | N | N | N | N |
| VMC create | Y | Y | Y (zero-filled 8–64 MB; OPL formats on first boot) | N | N | N | N | N |
| VMC content explorer / write | ? | Y | N | N | N | N | N | N |
| Cheats (.cht) | Y (editor, PS1 too) | Y (+auto widescreen) | N | N | N | N | N | N |
| PS1 / POPS (VCD) | Y (VCD, rename, cheats) | N | Y (CUE/BIN/ZIP→VCD, POPStarter/POPSLoader, VCD→BIN/CUE) | Y (cue2pops; x86_64 Linux) | N | N | N | N |
| APPS (ELF, conf_apps/title.cfg) | Y | N | Y (ELF import) | N | N | N | N | N |
| Internal PS2 HDD (APA) | Y (hdl_dump, admin) | N (#38) | N (no HDD/APA code in the tree) | N (WIP) | N | N | N | N |
| SMB / network | ? (folder path; FTP sync) | ? | P (only an OS-mounted share; macOS SMB bug #11 fixed) | Y (SMB folder) | N | N | N | N |
| Import from another OPL storage | P (FTP sync) | Y | — | — | — | — | — | — |
| MD5 / Redump check | Y (MD5 + Redump links) | N | N | N | N | N | N | N |
| CSV / list export | Y | N | N | N (`list` stdout) | N (`list`) | N | N | N |
| UI languages | Y (≥9) | P (pt .qm seen) | N (no i18n in the tree) | N | N | N | Y (several) | ? |
| Batch / queue | Y | Y | Y | Y | P | Y | ? | P |
| Auto-update | Y | Y | Y | N (pip) | N | N | — | N |
| Telemetry | Y (opt-out) | — | — | N | N | — | — | — |
| OPL UI simulator / preview | Y | N | N | N | N | N | N | N |
| Alternate loaders (RiptOPL, POPSLoader, NHDDL) | N | N | Y (RiptOPL art spec, Ember PS1 folders, POPStarter/POPSLoader; no NHDDL YAML) | N | N | N | N | N |
| Headless / web UI | N | N | N (#24 wontfix) | Y (CLI) | Y (CLI) | N | N | N |
| Scriptable CLI / library | N | N | N | Y | Y | N | N | N |

OPLNova (one row, see §2.5): C#/Avalonia GUI, cross-platform (AppImage/AUR, Windows ?), OSS, VMC experimental. Details come from the separate audit.

### 4.2 PS2 HDD (APA/PFS) tools

| Feature | OPLM (HDD mode) | HDL Batch Installer 3.7 | PFS BatchKit Mgr | PSBBN Definitive | PS2 HDD Manager (L10N37) | hdl-dump | pfsshell/pfsfuse | WinHIIP |
|---|---|---|---|---|---|---|---|---|
| License | Proprietary | GPL-3 | none | GPL-3 | Other | GPL-2 | GPL-2 (+AFL) | Proprietary |
| OS | Win | Win | Win | Linux (+WSL launcher) | Linux (Win experimental) | Win/Linux/macOS/BSD | Win/Linux/macOS | Win |
| UI | GUI | GUI | Batch/TUI | Scripts/TUI | GUI | CLI | CLI + FUSE | GUI |
| Install PS2 ISO (HDL) | Y | Y | Y (also zip/7z/rar) | Y (ISO/ZSO/BIN) | Y (queue) | Y | N | Y |
| PS1 VCD to POPS | ? | ? | Y | Y | — | N | P (put files) | N |
| Art | Y (own server) | Y (configurable, archive.org) | P (bundled packs) | Y (PSBBN DB + OPLM DB + IGN) | Y | N | N | N |
| OPL partition file copy (ART/CFG/VMC/CHT) | ? | Y (one click) | Y | Y | Y | N | Y (manual) | N |
| Mount PFS on host | N | Y (Dokan) | Y | — | — | N | Y | N |
| Format / init drive | N | Y (wizard) | Y | Y | Y | P (init) | Y (mkfs) | Y |
| Partition mgmt | N | Y | Y | Y | Y | P | Y | ? |
| NBD (console over LAN) | ? | ? (NBD.zip in repo) | Y | — | — | Y (via NBD block dev) | Y | N |
| >2 TB | N | — | N (≤2 TB) | P (exFAT ≤2 TB) | Y (Extended APA, custom OPL) | — | — | N |
| exFAT + APA hybrid | N | — | N | Y (APA-Jail) | N | N | N | N |
| HDD-OSD/PSBBN icons and integration | N | Y (icons, KELF, MBR) | Y (PSBBN/XMB) | Y | Y (FHDB) | P (MBR inject) | N | N |
| Cheats / game CFG download | Y (editor) | Y (widescreen, settings) | — | Y (widescreen hacks) | Y (OPL settings) | N | N | N |

---

## 5. User complaints / wishlist (evidence-graded)

**Verified (read directly)**
1. **Windows-only, closed source.**
   - CachyOS forum (2026-07-12): a user wants to replace "OPL Manager (a windows package)" on Linux. OPL PC Tools then failed on a libssl/OpenSSL symbol, and nobody replied.
   - OrbitPS2 exists because OPLM lacks macOS/Linux (README, per search summary). OPLNova is pitched the same way.
2. **Wine is fragile.** It needs the exact .NET 6.0.20 x86 runtime plus a font, or OPLM crashes or won't launch (Lutris installer notes).
3. **Stale and EOL runtime.** No release since 2024-09-15, it runs on .NET 6, and Windows 7 needs manual root-cert installs to reach the server (OPLM docs).
4. **Single, closed art server.** Art works only through OPLM's server, and only for builds ≥V23. Public dumps stopped at 2024-09. Other tools patch around this with static mirrors (HDL Batch Installer 3.7 "fixed artwork download with corrected URL" and added an archive.org source; OrbitPS2 had to remap SCR/BG names to stop 404s, PR #45).
5. **Users of OSS rivals ask for OPLM parity**: one-click bulk covers "like OPL Manager" (OPC #30) and cheat support (OPC #29).
6. **Other wants from the OSS issue trackers**:
   - Flatpak/Flathub (OPC #41).
   - CHD import (Orbit #43).
   - PSBBN/Luna art (Orbit #53).
   - PS1 BIN/CUE without VCD conversion for other loaders (Orbit #52).
   - Faster installs with a visible transfer speed (Orbit #54, compared against HDL Batch Installer/FHDB).
   - Correct handling of English-patched ISOs (OPC #39).
7. **Packaging and signing pain** in OSS rivals: unsigned builds (SmartScreen, macOS quarantine), a Linux libssl mismatch, paths with spaces breaking launcher scripts (OPC #37).

**Unverified / second-hand (search snippets of psx-place, GBAtemp, lecrabeinfo comments)**
- psx-place post: "The OPLManager.com website is down, but so are the servers for the ART Download … just times out". The date is unknown.
- GBAtemp: the PFS BatchKit/NBD server crashes on newer OPL builds; a Windows error 1225 when mounting.
- lecrabeinfo comments: >4 GB games split with USBUtil show 0 bytes in OPLM.
- Forum advice that OPL now reads PNG covers only (JPG dropped). If true, it breaks PS2IsoManager's JPG output.
- Reddit r/ps2 and r/ps2homebrew could not be fetched, so no first-hand Reddit evidence is included.

---

## 6. Gaps and opportunities for a new open-source, cross-platform manager

1. **Be the OPLM successor people can actually run everywhere.** No open tool yet matches OPLM's full feature set on Windows, macOS and Linux. Feature depth vs. OPLM:
   - **OrbitPS2 is the one to beat**: best UX, all three OSes, PS1/APPS, ZSO, the RiptOPL ecosystem's endorsement and an active contributor base. Its verified holes are the openings:
     - **no ul.cfg writing / UL splitting**, so no >4 GB games on FAT32;
     - **no APA HDD or NBD**;
     - **no cheats**;
     - **no i18n**;
     - no `conf_game.cfg` global settings and no CFG backups;
     - VMCs are blank files only, with no save management;
     - art depends on a single static GitHub repo through the rate-limited, unauthenticated GitHub API, with no fallback or cache;
     - Electron footprint (#21), unsigned builds, no CLI or headless mode (#24 wontfix);
     - no CSV export, no MD5/Redump.
   - OPL PC Tools has cheats and VMC write, but no macOS, no PS1/APPS and no APA.
   - The others are CLIs or single-purpose tools.
2. **An open, durable art and metadata service.** The whole ecosystem leans on OPLM's closed DB: PSBBN, NHDDL docs, PyOPLM, HDL Batch Installer, OrbitPS2. Public dumps stopped in 2024-09.
   - Keep the OPLM folder layout and naming so existing tools interoperate.
   - Support multiple pluggable sources: OPLM backups, xlenore/ps2-covers, the PSBBN DB, local folders.
   - Add community submissions with moderation, plus a CDN or Git/torrent mirror.
   - Caveat: the art is publishers' copyrighted material and none of the dumps carry a license. Expect legal and DMCA risk, so prefer "bring your own source" and user-configurable mirrors.
3. **First-class internal HDD (APA/PFS) on every OS.** This is today's biggest split: folder managers ignore APA, and APA tools are Windows-only GUIs (HDL Batch Installer, PFS BatchKit) or Linux scripts (PSBBN).
   - Embed or port hdl-dump and pfsshell logic. Both are GPL-2, so check "-or-later" headers before mixing with GPL-3 code (**unverified**).
   - Support raw USB-attached disks plus OPL's NBD server, including a userspace NBD client to cover macOS, which has no NBD client.
   - Support exFAT-on-HDD (OPL 1.2 beta), APA-Jail hybrids and, optionally, Extended APA >2 TB.
4. **Complete CFG editing plus community compatibility data.** Match OPLM's CFG editor: modes, DMA, GSM, `$GSMFIELDFix`, VMC, global vs per-game, unknown-key preservation.
   - Ship or optionally download compatibility presets from community lists.
   - Import metadata (descriptions, genre, players, rating) from the 13,620-file Tom-Bruise CFG DB (GPL-3).
5. **Cheats, maintained.** Only OPLM and OPC handle `.cht`, and the widescreen-cheat repo is abandoned. Offer a cheat editor, auto-download, and possibly pnach→cht conversion from PCSX2 patch sets (feasibility **unverified**).
6. **Multi-loader awareness.** The scene has fragmented:
   - OPL 1.1.0 stable (2021) vs the 1.2.0 beta rolling build.
   - Forks: RiptOPL (which the OPL README notes "heavily uses LLM tools"), wOPL.
   - **NHDDL/Neutrino**: YAML per-game config, ISO only (no ZSO), MMCE per-game cards.
   - POPStarter vs POPSLoader, PSBBN/Luna art.
   - Target profiles that pick folder layout, naming, art specs, compression and config format per loader are not offered by anyone except partially by OrbitPS2 and PSBBN.
7. **Integrity and naming.** Only OPLM has MD5 and Redump links. Opportunities:
   - Built-in Redump DAT matching and bad-dump detection.
   - Canonical renaming, region/serial lookup (e.g. from PCSX2's GameIndex), duplicate detection.
   - Handling of patched or English-translated ISOs.
8. **Storage-aware copying.** OPL needs unfragmented files on USB (≤64 fragments in 1.2 beta, none in older builds), and ul.cfg fragments on each install. Offer:
   - Contiguous-write and fragmentation checks.
   - FAT32/exFAT cluster advice and a "will it fit" planner.
   - Visible transfer speed and fast unbuffered I/O (OrbitPS2 needed Robocopy for this).
9. **Broader imports**: CHD (requested, wontfix in Orbit), zip/7z/rar (PFS BatchKit does this), NRG/MDF/multi-track BIN, direct optical rip, and PS1 multi-disc.
10. **VMC and save management beyond create.** Browse, import and export saves (.psu/.max/.mcs), convert VMC to and from physical memory-card dumps, and handle MMCE per-game cards. Only OPC has a VMC explorer.
11. **Network setup helper.** OPL uses **SMBv1**, which modern Windows, macOS and Samba disable by default. A guided SMB1 share setup or a bundled minimal server would fill a niche; the 151★ Linux-only ps2-network-manager shows demand. SMB1 pain as a user complaint is **unverified**.
12. **Headless CLI + library + GUI from one core, optionally a web UI.** NAS and Raspberry Pi users (RetroNAS issues #20/#28 in OrbitPS2, PyOPLM/libopl demand) can script imports. OrbitPS2 explicitly declined a web UI for headless servers (#24). A GUI on top serves everyone else.
13. **Trust and packaging.**
    - No telemetry, unlike OPLM's opt-out usage data and unique IDs.
    - Signed and notarized builds.
    - Flatpak/Flathub, AppImage, Homebrew cask, winget/Scoop.
    - Self-contained runtime: avoid OPLM's .NET 6 runtime dependency and OPC's system libssl dependency.
14. **Library features OPLM has and others don't**: CSV/HTML export, stats, translated UI (OPLM ≥9 languages), first-run language prompt. Optionally an OPL theme preview (OPLM's "OPL Simulator" is unique, but probably low priority).

---

## 7. Surprising facts
- **OPLM has been frozen for over two years** (V24, 2024-09-15) on an end-of-life .NET 6. Its public art backups also stopped at 2024-09, yet it is still the ecosystem's de-facto art authority (even the NHDDL README tells users to get art with OPLM).
- **OPL's own last stable release is v1.1.0 from 2021-09-09.** exFAT, ZSO and 64-fragment support exist only in the 1.2.0 beta "latest" builds (rolling, last 2026-10-13).
- **2025–2026 boom of OPLM alternatives**:
  - OrbitPS2 (repo created Aug 2025, already 127★, Win/mac/Linux).
  - OPL PC Tools 4.0 (Aug 2026) after a 4-year gap.
  - PyOPLM revived.
  - OPLNova.
  - A dozen tiny repos.
  - Several show signs of LLM-assisted development (PyOPLM merged a `claude/...` branch; OPL's README flags RiptOPL as heavily LLM-developed).
- **PSBBN Definitive Project (988★)** is the most popular OSS PS2-HDD tool and is Linux-first (WSL on Windows). The best APA tooling is not on Windows-only GUIs anymore.
- The OrbitPS2 art source is a public GitHub repo that is "a complete dump of the OPL Manager GameArt Database", with no license. OrbitPS2 fetches it file by file through raw.githubusercontent plus the unauthenticated GitHub contents API, with no cache.
- **RiptOPL and OrbitPS2 are converging.** RiptOPL's README recommends OrbitPS2 as its PC manager, and Ripto himself commits RiptOPL-specific features to OrbitPS2 (#48/#50, art spec with 8-bit PNG, ICO 128², COV 280×400 / 400×400). A new tool must treat RiptOPL as a first-class target or cede that audience.
- OrbitPS2 can parse ul.cfg but never writes it. So the most popular new OSS manager cannot install >4 GB games onto FAT32 drives, which is still the classic USB setup.
- PS2IsoManager saves covers as `.jpg`, which may not display in current OPL (**unverified**).
- macOS has no NBD client according to OPL's README, so even OSS HDD-over-LAN workflows exclude Mac users.

---

## 8. Sources
- OPL Manager home: https://oplmanager.com/
- OPLM changelog: https://oplmanager.com/?changelog
- OPLM docs: https://oplmanager.com/?docs
- OPLM ART DB backups: https://oplmanager.com/?backups
- archive.org OPLM_ART_2024_09: https://archive.org/details/OPLM_ART_2024_09
- archive.org search (OPLM_ART items): https://archive.org/advancedsearch.php?q=OPLM_ART&fl[]=identifier&fl[]=publicdate&fl[]=item_size&sort[]=publicdate+desc&rows=10&output=json
- psx-place OPLM thread (403, snippet only): https://psx-place.com/threads/opl-manager-tool-to-manage-your-games.19055/
- psx-place OPLM resource updates (403, snippet only): https://psx-place.com/resources/opl-manager-tool-to-manage-your-games.663/updates
- lecrabeinfo OPLM download page (V24.0, 2024-09-15): https://lecrabeinfo.net/telecharger/opl-manager
- lecrabeinfo OPL tutorial (partly paywalled; comments): https://lecrabeinfo.net/tutoriels/ps2-installer-et-configurer-open-ps2-loader-opl/
- cdrinfo.pl OPLM thread: https://forum.cdrinfo.pl/f106/ps2-opl-manager-96416-print/
- AlternativeTo OPLM: https://alternativeto.net/software/opl-manager/about
- Lutris OPLM v24 Wine installer: https://lutris.net/games/install/39797/view
- CachyOS forum "OPL Mangaer / OPL PC Tools": https://discuss.cachyos.org/t/opl-mangaer-opl-pc-tools/32817
- psx-place OPL cover-art thread (snippet only): https://psx-place.com/threads/opl-not-showing-cover-art.48498/post-416401
- PyOPLM PyPI: https://pypi.org/project/pyoplm/
- PyOPLM GitHub: https://github.com/edisnord/pyoplm (commits: https://github.com/edisnord/pyoplm/commits/master)
- psx-place PyOPLM thread (snippet): https://www.psx-place.com/threads/pyopl-manager-pyoplm-a-cli-only-open-source-python-app-for-managing-opl-games.41658/
- libopl GitHub: https://github.com/Nold360/libopl (commits: https://github.com/Nold360/libopl/commits/master)
- libopl PyPI: https://pypi.org/project/libopl/
- bitbuilt libopl thread (site in maintenance): https://bitbuilt.net/forums/threads/release-libopl-opl-manager-alternative-python-lib.2916/
- OPLNova psx-place thread (snippet only): https://www.psx-place.com/threads/release-oplnova-a-modern-open-source-cross-platform-alternative-to-opl-manager.50635/
- OPL PC Tools: https://github.com/brainstream/OPL-PC-Tools (releases: https://github.com/brainstream/OPL-PC-Tools/releases; API: https://api.github.com/repos/brainstream/OPL-PC-Tools/releases?per_page=5; issues: https://api.github.com/repos/brainstream/OPL-PC-Tools/issues?state=all&per_page=50)
- OrbitPS2 Manager: https://github.com/Luden02/OrbitPS2-Manager (README: https://raw.githubusercontent.com/Luden02/OrbitPS2-Manager/main/README.md; releases API: https://api.github.com/repos/Luden02/OrbitPS2-Manager/releases?per_page=6; issues API: https://api.github.com/repos/Luden02/OrbitPS2-Manager/issues?state=all&per_page=50; issue #54: https://github.com/Luden02/OrbitPS2-Manager/issues/54)
- OrbitPS2 source files read: https://raw.githubusercontent.com/Luden02/OrbitPS2-Manager/main/src/services/artwork.service.ts , https://raw.githubusercontent.com/Luden02/OrbitPS2-Manager/main/src/services/library.service.ts , https://raw.githubusercontent.com/Luden02/OrbitPS2-Manager/main/src/services/cfg.service.ts , https://raw.githubusercontent.com/Luden02/OrbitPS2-Manager/main/src/services/vmc.service.ts ; tree: https://api.github.com/repos/Luden02/OrbitPS2-Manager/git/trees/main?recursive=1 ; commits: https://api.github.com/repos/Luden02/OrbitPS2-Manager/commits?per_page=15 ; repo meta: https://api.github.com/repos/Luden02/OrbitPS2-Manager ; issues #31/#48: https://github.com/Luden02/OrbitPS2-Manager/issues/31 , https://github.com/Luden02/OrbitPS2-Manager/issues/48
- RiptOPL (NathanNeurotic fork): https://github.com/NathanNeurotic/Open-PS2-Loader ; wOPL: https://github.com/NathanNeurotic/wOPL / ps2homebrew/wOPL (via GitHub topic page)
- OPLM art DB dump: https://github.com/Luden02/psx-ps2-opl-art-database
- POUM: https://github.com/gecm0/POUM
- PS2IsoManager: https://github.com/imadali120/PS2IsoManager
- Tihwin: https://github.com/developersu/Tihwin (releases API: https://api.github.com/repos/developersu/Tihwin/releases?per_page=2)
- OPLUtil: https://github.com/IsseiYoshida/OPLUtil
- dheison0/ps2-game-manager: https://github.com/dheison0/ps2-game-manager
- ps2-network-manager: https://github.com/Equiel-1703/ps2-network-manager
- GitHub search (opl manager ps2): https://api.github.com/search/repositories?q=opl+manager+ps2&sort=stars&per_page=40
- GitHub search (open ps2 loader manager): https://api.github.com/search/repositories?q=%22open+ps2+loader%22+in:description,readme+manager&sort=stars&per_page=40
- GitHub topic opl: https://github.com/topics/opl
- xlenore ps2-covers: https://github.com/xlenore/ps2-covers
- OPL Widescreen Cheats: https://github.com/PS2-Widescreen/OPL-Widescreen-Cheats
- PS2 OPL CFG Database: https://github.com/Tom-Bruise/PS2-OPL-CFG-Database
- Open PS2 Loader repo/README: https://github.com/ps2homebrew/Open-PS2-Loader (releases API: https://api.github.com/repos/ps2homebrew/Open-PS2-Loader/releases?per_page=4)
- OPL wiki USB mode: https://github.com/ps2homebrew/Open-PS2-Loader/wiki/usb-mode
- NHDDL: https://github.com/pcm720/nhddl
- hdl-dump: https://github.com/ps2homebrew/hdl-dump (COPYING: https://raw.githubusercontent.com/ps2homebrew/hdl-dump/master/COPYING; API: https://api.github.com/repos/ps2homebrew/hdl-dump)
- pfsshell: https://github.com/ps2homebrew/pfsshell
- HDL Batch Installer: https://github.com/israpps/HDL-Batch-installer (releases: https://github.com/israpps/HDL-Batch-installer/releases)
- PFS BatchKit Manager: https://github.com/GDX-X/PFS-BatchKit-Manager ; psx-place resource (snippet): https://psx-place.com/resources/pfs-batchkit-manager-2tb-support.1202/updates ; GBAtemp thread (snippet): https://gbatemp.net/threads/pfs-batchkit-manager-ps2-hdd-2tb-support.602462/
- PSBBN Definitive Project: https://github.com/CosmicScale/PSBBN-Definitive-Project ; news: https://www.generationamiga.com/2026/07/04/psbbn-definitive-project-update-brings-major-ps2-hdd-improvements/
- PS2 HDD Manager (L10N37): https://github.com/L10N37/PS2-HDD-Manager
- WinHIIP (secondary): https://lecrabeinfo.net/telecharger/winhiip
- USBUtil (secondary): https://lecrabeinfo.net/telecharger/usbutil/
- USB TOOLS Android (APKMirror, 403 on fetch; snippet only): https://www.apkmirror.com/?p=14609559
- GBAtemp OPL BATCH workflow (snippet): https://gbatemp.net/threads/opl-batch-finalizing-the-installation-of-a-ps2-hdd.556480/
