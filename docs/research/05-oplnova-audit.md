# 05 — OPLNova audit (competitor analysis)

Audit date: 2026-10-22. Scope: the psx-place release thread, the OPLNova source (cloned read-only, nothing built or run), its companion art server "OPLNova VAPI", the "RIP" format question, and the user wishlist from the thread.

Legend: **[verified]** = confirmed from source code, git history, or an API response. **[unverified]** = from search-engine snippets or secondary sources only.

---

## 0. Access note: the forum thread

- The psx-place thread (`threads/...50635/`) is behind a **Cloudflare Turnstile "verify you are human" challenge** for every fetcher I tried: WebFetch, curl, the built-in browser pane, and Claude in Chrome. Completing CAPTCHAs is off-limits, so I could not read the live page.
- The Wayback Machine has **no capture** of the thread (it reported "has not archived that URL"). The CDX index was rate-limited or offline. archive.ph is not reachable from this environment.
- So the thread contents below come from **search-engine snippets (several independent queries)**. They agree with each other. Treat them as **[unverified]** until someone opens the thread in a normal browser.
- What the snippets show [unverified]:
  - The thread was created by **WeeXnes** around **2026-07-20** (snippets put it about 93 days old on 2026-10-22). It is in the PS2 section.
  - Snapshots show **0–2 replies**, so it is almost certainly one page; `page-2` most likely does not exist.
  - There is a separate psx-place *resource* listing for OPLNova that shows **v2.0.1** (one snippet dates it Sep 25, 2026). That date is unverified.

---

## 1. OPLNova overview

| Item | Finding |
|---|---|
| Name / history | Started 2025-04-19 as **"PS2_Manager"** (repo `git.weexnes.dev/WeeXnes/ps2_manager`, Forgejo). Renamed **OPLNova** on 2026-04-07 (commit `309da1d`, v1.7.0) [verified, git log] |
| Author | **WeeXnes** (site weexnes.dev; self-hosted OneDev at code.weexnes.dev). Single author: 146 commits by WeeXnes, of which 5 are under the identity "default" (the 2.0.2–2.0.4 macOS work) [verified] |
| License | **GPL-3.0** (`LICENSE`; AUR metadata also says GPL3) [verified]. Mixed messaging: oplnova.xyz footer says "© 2026 WeeXnes. All rights reserved." |
| Language / runtime | **C#**, `TargetFramework net10.0` (`OPLNova/OPLNova.csproj`) [verified] |
| GUI | **Avalonia UI 11.2.7** (Fluent theme, Inter font), compiled bindings. Code-behind style, no MVVM [verified] |
| Dependencies | Avalonia*, `DiscUtils.Iso9660 0.16.13` (ISO reading), vendored `Nocksoft INIFile.cs` (MIT) for settings [verified] |
| Platforms / artifacts | CI (`.onedev-buildspec.yml`) publishes self-contained, single-file, **trimmed** builds: `OPLNova_Linux_x64.AppImage`, `OPLNova_Linux_x64.zip`, `OPLNova_Windows_x64.zip`, `OPLNova_macOS_x64.zip`, `OPLNova_macOS_x64.dmg` (Intel only, ad-hoc signed with rcodesign, not notarized). **AUR**: `oplnova`, `oplnova-git`, auto-bumped from CI [verified]. No ARM builds of any kind |
| Code hosting | `https://code.weexnes.dev/OPLNova` (OneDev 16.7.3). **No GitHub mirror** (the GitHub search API finds nothing for "oplnova") [verified] |
| First commit | 2025-04-19 [verified] |
| First tagged release on current host | **1.6.1, 2025-09-09**. Earlier releases ran on the Forgejo instance from May 2025, when "versioning added" landed [verified] |
| Public announcement | psx-place release thread, about 2026-07-20 [unverified] |
| Latest version | **2.0.4, 2026-10-05** (tags 2.0.2/2.0.3/2.0.4 on the same day, macOS packaging). **2.0.0/2.0.1 on 2026-05-16/17**, commit message "Version 2 Release (AI Driven UI Rework)" [verified] |
| Tags | 1.6.1, 1.6.2, 1.6.3, 1.6.4, 1.7.0, 1.7.1, 1.7.2, 2.0.0, 2.0.1, 2.0.2, 2.0.3, 2.0.4 [verified] |
| Activity | 146 commits total; 0 forks; **0 issues and 0 PRs** visible on OneDev (API and web UI). OneDev has no stars. AUR: **0 votes, popularity 0** for both packages. 37 CI builds [verified, OneDev REST + AUR RPC]. Commit hygiene is poor: many commits are named "f", "rrr", "jj", "g" |
| Size | About **2,400 lines of C#** plus about 630 lines of AXAML. 8 windows/dialogs and 10 core files [verified] |
| Website / community | https://oplnova.xyz (Nuxt site; its TLS certificate had **expired** when WebFetch tried it); docs at `/docs`; Discord `discord.gg/62ffYYqHvf` |

**Status:** a small one-person hobby project. It has had bursts of activity: Apr–May 2025 (initial), Aug–Sep 2025, Apr 2026, May 2026 (v2 "AI-driven" UI rework), and Oct 2026 (macOS packaging). There are no users visible on the tracker. Development happens in short sprints.

---

## 2. Source code audit

Clone: a local read-only clone of https://code.weexnes.dev/OPLNova (full history; unshallowed). Nothing was built or executed.

### 2.1 Architecture and module layout

```
OPLNova.sln
OPLNova/
  Program.cs                 Avalonia bootstrap
  App.axaml(.cs)             global exception handler -> ./error_log.txt; shows Setup on first run, else MainWindow
  Setup.axaml(.cs)           first run: API URL (default http://localhost:3000) + library folder picker
  MainWindow.axaml(.cs)      584 lines: list/grid library, search, details pane, VMC + compat toggles, hashes, art carousel
  AddGameWindow.axaml(.cs)   "install" an ISO: shows serial, title, cover; Move/Copy; progress bar
  ConvertGameWindow.axaml(.cs)  BIN -> ISO conversion, then opens AddGameWindow
  BulkScraperWindow.axaml(.cs)  loops all games, downloads missing (or all) art
  VmcCreateDialog.axaml(.cs) "create" 8/16/32/64 MB VMC file
  Core/
    Game.cs        model + ALL business logic: serial read, title lookup, art download/save, copy/install, rename, CFG path
    Config.cs      per-game CFG parse/save ($VMC_0, $VMC_1, $Compatibility, $ConfigSource)
    ConfigUtils.cs settings.ini wrapper (enum-keyed) + vendored Nocksoft INIFile (MIT)
    ISO.cs         SYSTEM.CNF BOOT2/BOOT parsing via DiscUtils CDReader
    IsoConverter.cs  BinType / Iso9660Conv (raw 2352/2336 -> 2048 user data)
    Artwork.cs     enum Front/Back/Disc
    DiscType.cs    CD/DVD enum + icon flags
    Util.cs        file dialog, dir check, size format, VMC file creation, MD5/SHA1
    Console.cs     coloured console + file logger (~/.oplnova/output.log)
    Globals.cs     version const, paths (~/.oplnova/settings.ini), settings enum
  Assets/          icons, AppRun, .desktop, Info.plist, .icns
tools/dmg          a PREBUILT statically-linked x86-64 ELF used by CI to make the DMG (no source, no provenance)
.onedev-buildspec.yml  CI: version/tag job, AUR push, publish linux/win/osx, AppImage, macOS bundle + DMG, zips
```

There are no layers, interfaces, view-models, or DI. All I/O happens in the model constructor (`new Game(path)` opens the ISO, may hit the network, and loads 3 bitmaps).

### 2.2 Feature-by-feature, as implemented in code

| Area | Implemented? | Details (code) |
|---|---|---|
| **Game import** | Yes, one file at a time | File picker accepts `*.iso`, `*.bin` (`Globals.FileDialogTypes`). `.bin` goes to the converter first. **No multi-select, drag-and-drop, folder scan, or batch import.** Install = buffered 1 MiB copy with progress (`Game.CopyIsoWithProgressAsync`), then for "Move" a `File.Delete` of the source (`Game.Install`) |
| **Library scan** | Yes | Non-recursive `*.iso` in `<lib>/DVD` and `<lib>/CD` (`MainWindow.FetchGamesFromLibrary`). Every ISO is opened to read SYSTEM.CNF and 3 PNGs are decoded per game, **synchronously on the UI thread**. Ignores `.zso`, `ul.*`, VCD, APPS |
| **ID detection** | Yes, basic | `ISO.GetSerial`: DiscUtils `CDReader`, reads `SYSTEM.CNF`, takes the first line starting with `BOOT2` or `BOOT`, and returns the text between the first `\` and `;` (for example `SLUS_200.02`). Returns `null` when there is no SYSTEM.CNF; nothing downstream handles null. No PS1/PS2 distinction (it also accepts `BOOT`). No ELF-header fallback and no handling of boot paths in subdirectories |
| **CD vs DVD** | Heuristic | `file size < 1,000,000,000 bytes` = CD, else DVD (`Game` ctor). This puts small DVD images in `CD/`. It does not check the image's actual medium |
| **Renaming** | Yes | Install writes `<ID>.<Title>.iso` (the classic OPL name). Title comes from the API and is **truncated to 32 chars**. "Save" in the details pane renames via `Game.ChangeName`, which **always moves into `DVD/`, even for CD games**, and does not update `GamePath`. **No sanitising of illegal filename characters** (`:` `?` `/` and so on) from API titles. Reading names back uses `ParseFormattedFilename` = `split('.')[^2]`, which **breaks on titles containing dots** |
| **New OPL naming (title-only filenames)** | No | — |
| **ul.cfg / USBExtreme split (>4 GB on FAT32)** | **No** | Nothing. The BIN converter only *throws* if the output exceeds 4 GB on FAT32 |
| **Per-game CFG editing** | Partial | `Core/Config.cs` parses `$Compatibility`, `$VMC_0`, `$VMC_1`, `$ConfigSource`. **`SaveConfig()` rewrites the whole file with only those 4 keys**, which wipes `$DNAS`, `$AltStartup`, GSM, cheats, PADEMU, `Title`/`Description` metadata, Neutrino/RiptOPL keys, and so on. Worse, `SetShowcaseArea` programmatically sets the VMC combo boxes and toggles, which fires their change handlers and **calls `SaveConfig()` on games the user merely selects**. Browsing the library can therefore silently strip CFG files (found by reading the code, not runtime-tested) |
| **Compatibility modes** | Partial | Six unlabeled toggles "Mode 1..6" mapped to bits 0–5. There are no descriptions; in OPL they are Accurate Reads, Synchronous, Unhook Syscalls, Skip Videos, Emulate DVD-DL, Disable IGR. **Bits 6–7 are dropped on save** (RiptOPL uses 0x40/0x80) |
| **VMC** | Partial / broken create | Assigns slot 1/2 from files in `<lib>/VMC` (names without extension). "Create VMC" (`Util.CreateVmcFile`) just does `FileStream.SetLength(size)`, which gives an **all-zero, unformatted file**, not a formatted PS2 memory-card image (no superblock, FAT, or ECC). The README itself says VMCs must be created in OPL |
| **Art download** | Yes | Front `_COV.png`, back `_COV2.png`, disc `_ICO.png` from the configured API (see 2.3). Saved to `<lib>/ART/` **rescaled to 353×500 (COV, COV2) and 353×353 (ICO)**, which are non-standard sizes larger than OPL's usual 140×200 / 242×344 / 64×64. Manual override: click the cover to choose a PNG/JPG |
| **Bulk scraper** | Yes (v2) | `BulkScraperWindow`: iterates all games, "overwrite existing" checkbox, log pane. Runs one game at a time, sequentially |
| **Other art types (LAB/BG/SCR/LGO)** | No | — |
| **Metadata sources** | Title only | `GET {api}/search/{ID}` returns `{id,title}`. No genre, region, year, description, or players, and nothing is written to CFG |
| **Cheats (CHT)** | No | — |
| **POPS / PS1** | No | A PS1 `.bin` would be converted and installed into `CD/` as if it were a PS2 game |
| **APPS** | No | — |
| **HDD / APA / HDL** | No | — |
| **SMB / network** | No | (It only works on whatever folder you point it at; there is no SMB client) |
| **ZSO / CSO** | No | — |
| **BIN/CUE conversion** | Yes, simplistic | `Core/IsoConverter.cs`: detects Mode 1 vs Mode 2 by the sync header and byte 15. Treats the file as a single track of 2352- (or else 2336-) byte sectors and strips to 2048. **Ignores the .cue**: no multi-track handling (CDDA tracks get converted as garbage data), no pregap/INDEX handling, no multi-bin. Output goes next to the source and can overwrite an existing `.iso`. The free-space check uses `DriveInfo.TotalSize` instead of `AvailableFreeSpace` (bug). The code style (`(FileStream) null`, `num1`, `numArray`, "BytesWritted", `#nullable disable`) **looks decompiled (ILSpy-style) from an unnamed third-party .NET assembly. Its provenance is unknown, so treat this code as unsafe to reuse** (assessment) |
| **Hashing / verification** | Display only | MD5 and SHA-1 computed in two parallel BackgroundWorkers, so the file is read twice at once. **No Redump/DAT comparison** and no CRC32 |
| **Delete / uninstall** | Dangerous | The `Delete` key or the "Uninstall Game" menu item calls `File.Delete` on the ISO **with no confirmation**. CFG and ART are left behind |
| **i18n** | No | All strings are hard-coded English in AXAML and C# |
| **Settings** | Minimal | `~/.oplnova/settings.ini` with keys `library_path`, `preferred_installation_method`, `api_url`. Log at `~/.oplnova/output.log`. Crash log `error_log.txt` goes to the **current working directory** |
| **Updater** | No | No in-app update check. The commit "added version check" (`7f01a92`) is a CI tag check. Updates come through AUR or manual download |
| **UI extras** | Yes | List/grid toggle, search by name/ID (Ctrl+F), F5 refresh, totals (count and size), front/back/disc art carousel, dark "Obsidian" theme |

### 2.3 Art and metadata source (important)

- **Client side** (`Core/Game.cs`): `{api_url}/search/{GameID}` for the title, and `{api_url}/art/{GameID}_COV.png`, `_COV2.png`, `_ICO.png` for images. **No API key, no auth, plain HTTP allowed.**
- **History of the endpoint** [verified, git log]:
  - 2025-04-19 initial: GitHub `xlenore/ps2-covers` (`https://github.com/xlenore/ps2-covers/blob/main/covers/default/{id}.jpg?raw=true`).
  - 2025-04-22: `http://localhost:3000/...` (dev).
  - 2025-09-09 → 2026-04-09: hard-coded `https://ps2api.weexnes.dev/` (author-hosted).
  - 2026-04-09 (v1.7.1): made configurable (`api_url` in settings.ini).
  - 2026-05-17 (v2.0.1): **the setup wizard defaults to `http://localhost:3000`**, with a tooltip telling users to self-host "OPLNova VAPI".
- **The hosted endpoint is dead**: `https://ps2api.weexnes.dev/search/...` and `/art/...` return **HTTP 200 with an empty body** (Caddy, Content-Length 0) [verified 2026-10-22]. So **out of the box, a new install gets no titles and no art** unless the user self-hosts the VAPI server.
- **OPLNova VAPI** (`https://code.weexnes.dev/oplnova_vapi`, cloned locally, read-only):
  - About 180 lines of **V language** (`api.v`, `veb` framework), port 3000.
  - `/search/:id` normalises the ID (`SLUS_200.02` becomes `SLUS-20002`) and looks it up in `db/PS2_LIST.txt` (`ID Title` lines).
  - `/art/:file` serves `db/PS2/<file>`, with fallbacks: png↔jpg, then normalised↔legacy ID forms. It has a basic `..` traversal guard.
  - `setup_db.sh` downloads and unzips **`OPLM_ART_2024_09.zip`**, which is the **OPL Manager artwork database backup** (the README says "from Archive.org"). The URL is **blank in the repo** (`URL=""`), so the setup cannot be reproduced as published.
  - `contact.html` credits **oplmanager.com "Artwork Database Backups"** and **xlenore/ps2-covers**.
  - `v.mod` says MIT but there is **no LICENSE file**.
- Takeaway for us: OPLNova has no art source of its own. It is a thin client over a re-hosted **OPL Manager art dump**, the same data as GitHub `Luden02/psx-ps2-opl-art-database`, which OrbitPS2 Manager pulls from `raw.githubusercontent.com` and the GitHub contents API. The artwork's licensing is murky either way (third-party box art).

### 2.4 Code quality, tests, notable bugs

- **Tests:** none. **CI:** packaging only (no build-and-test, lint, or analyzers). CI downloads `appimagetool` "continuous" and an `rcodesign` tarball **without checksum pinning**, runs jobs on the host (`runInContainer: false`), and uses a **committed opaque ELF `tools/dmg`**. That is a supply-chain smell.
- **Quality:** a god-class model (`Game`) doing network, disk, and imaging work. Sync-over-async (`GetAwaiter().GetResult()`) on the UI thread. A new `HttpClient` per request. Catch-all exception swallowing. Logs say "downloaded successfully" even after a failure. `async void` installs. Nullable is enabled but ignored. Imports are unused (VisualBasic CompilerServices, Tmds.DBus, JS interop). The 2.0 UI was by the author's own label "AI Driven". Commit messages are mostly meaningless.
- **Bug list** (from code reading; not runtime-verified):
  1. Selecting a game rewrites its CFG through the change handlers, and `SaveConfig` drops every key except 4. **Data loss** (`MainWindow.SetShowcaseArea` + `Config.SaveConfig`).
  2. The Delete key permanently deletes the ISO without confirmation (`MainWindow_OnKeyDown` → `Game.Uninstall`).
  3. `ChangeName` always targets `DVD/`, so CD games get moved into the wrong folder. `GamePath` is not updated.
  4. No filename sanitising, so titles with `:`, `?`, `"`, `/` fail on Windows/FAT32/exFAT.
  5. `ParseFormattedFilename` returns the wrong title for titles containing `.`.
  6. A null GameID (no SYSTEM.CNF) throws a NullReferenceException in the search filter and produces CFG/ART names like `.cfg` and `_COV.png`.
  7. CD/DVD is decided by a 1 GB size threshold.
  8. The BIN converter ignores the CUE (multi-track and pregap), uses the TotalSize free-space check, and overwrites an existing ISO.
  9. "Create VMC" makes an unformatted zero-filled file.
  10. Compat bits 6–7 are lost on save.
  11. Art requests run on the UI thread with the default 100 s HttpClient timeout. An unreachable API can freeze the Add dialog for minutes.
  12. Scanning a library whose drive is unplugged throws from `Directory.GetFiles` (unhandled).
  13. `error_log.txt` is written to the CWD, which may be read-only (AppImage, Program Files).
  14. Art is upscaled to 353×500 PNG RGBA, which wastes PS2 memory and load time compared with OPL-standard sizes.

### 2.5 What is worth borrowing (and licence implications)

OPLNova is **GPL-3.0**. Copying any of its code would force our program to be GPL-3.0 (or compatible). Ideas are free to reuse.

- **Ideas worth taking:**
  - (a) Decouple metadata and art behind a tiny **self-hostable HTTP service** with **ID normalisation** (`SLUS_200.02` ⇄ `SLUS-20002`) and **png/jpg fallbacks**. Good for mirrors and offline packs.
  - (b) A **bulk scraper** with "only missing / overwrite" and a per-game log.
  - (c) A **Move vs Copy** install preference.
  - (d) **Sync-header based sector-mode detection** for raw BINs: check the 12-byte sync, then byte 15 = mode → user-data offset 16 or 24.
  - (e) Distribution recipe: **self-contained single-file trimmed .NET publish**, AppImage, AUR packages auto-bumped from CI, and a cross-built macOS bundle with `rcodesign`.
- **Do not copy:**
  - `IsoConverter.cs` has unclear provenance (likely decompiled).
  - The art DB has unclear rights.
  - `INIFile.cs` is MIT (Nocksoft) and reusable on its own terms if ever needed, though a modern INI library is better.
- **What it teaches by omission:** our differentiators should be:
  - correct CFG round-tripping (preserve unknown keys);
  - UL/USBExtreme, ZSO, POPS/VCD, APPS, HDD/APA, SMB, and cheats;
  - Redump verification;
  - proper VMC formatting;
  - safe destructive actions;
  - batch import;
  - async I/O;
  - i18n;
  - a working default art source.

### 2.6 Nearby competitors seen during research (for context)

| Project | Notes |
|---|---|
| **OrbitPS2 Manager** (`Luden02/OrbitPS2-Manager`) | Electron + Angular + TypeScript, **GPL-3.0**, **127★**, pushed 2026-10-22. Covers PS2 ISO/ZSO conversion; PS1 CUE/BIN/ZIP→VCD with a POPStarter or **RiptOPL/POPSLoader** launcher style; APPS; per-game CFG; VMC creation; UL recognition (per the ConsoleMods wiki); and an update check. Art from GitHub `Luden02/psx-ps2-opl-art-database`. It already has **RiptOPL art support**, which OPLNova lacks [verified via its README/source]. **This is the stronger direct competitor, not OPLNova** |
| Others found via GitHub search (not audited) | `lucasonline0/opl-linux-toolbox`, `JO0812/TinyPS2Manager`, `gpulch/ps2-manager`, `gecm0/POUM`, `developersu/Tihwin` (ul.cfg / split tool), `L10N37/PS2-HDD-Manager` (APA/PFS >2 TiB), `N0NamedGuy/opl-manager-next` (stale 2023), `tekmyster/ps2-opl-ripper` (disc ripper + OPL USB manager) |

---

## 3. The "RIP" format

### 3.1 What the thread comment says [unverified: search snippets, consistent across 3 queries]

The comment comes from **NathanNeurotic**, the developer of the RiptOPL fork:

> "Would love added support for RiptOPL's art formats." (NathanNeurotic, psx-place OPLNova thread)

Summary: he asks OPLNova to support **RiptOPL's artwork conventions**. He notes that **OrbitPS2 Manager already added this**, describing it as essentially a checkbox plus different file-naming schemes.

So "RIP / RIP.OPL / OPL RIP" is almost certainly **"RiptOPL"**. It is **not a disc-image or ripping format**: it is an **OPL fork** whose *artwork layout and requirements* differ from stock OPL.

### 3.2 What RiptOPL is [verified: GitHub + cloned source of NathanNeurotic/Open-PS2-Loader]

- **RiptOPL** by NathanNeurotic is a downstream fork of Open PS2 Loader. The main repo is `NathanNeurotic/Open-PS2-Loader` ("RiptOPL Legacy Edition", forked from `ps2-mmce/Open-PS2-Loader`): **65★, 5 forks, AFL-3.0**, extremely active (pushed 2026-10-22, rolling releases). There is also a new `NathanNeurotic/RiptOPL` ("C++ PS2Build Edition", 4★, created 2026-09-17). Its docs are at https://nathanneurotic.github.io/Open-PS2-Loader/ and there is a psx-place "[FORK] RiptOPL" resource and thread. The README says it relies heavily on LLM tooling [unverified snippet].
- Features include:
  - Coverflow and "PSBBN Flow" cover-art themes, Favorites, and per-game Neutrino launching;
  - **Ember** (a second PS1 core) alongside POPSTARTER;
  - exFAT on the internal HDD, RetroAchievements (dev builds), and DualSense support;
  - its own master settings file `settings_riptopl.cfg`, while sharing `CFG/`, `ART/`, `VMC/` and the other folders with OPL.
- A related project is **RIPPS2** (`akilluminati47/RIPPS2`), a RiptOPL-derived single-ELF front-end. The name is similar, but the comment is about RiptOPL.
- **Mainstream or niche?** Niche but rising. Upstream OPL (`ps2homebrew/Open-PS2-Loader`) has **3,082★**, against RiptOPL's 65★. RiptOPL is visible on psx-place, and at least one manager (OrbitPS2) already targets it.

### 3.3 Structure of "RiptOPL art formats" (what a manager must do)

From the RiptOPL source (`src/tar.c`, `src/texcache.c`, `src/bdmsupport.c`, `docs/THEME_ENGINE.md`, `docs/VCD.md`) and OrbitPS2's implementation (`src/utils/png-artwork.ts`, `angular/src/app/shared/utils/ps1-artwork-identities.ts`, PR NathanNeurotic/OrbitPS2-Manager-for-PR#1):

1. **Location and naming:** always `ART/<identity>_<SUFFIX>.png` at the device's OPL root.
   - The extension must be lower-case `.png`, because the PS2 HDD's PFS is case-sensitive.
   - Suffixes in use: `COV`, `COV2`, `ICO`, `LAB`, `BG`, `SCR`, `SCR2`, `LGO`, plus `DISC`. OrbitPS2 also mentions a `COV3` secondary-cover slot.
2. **Identity rules (the main difference from stock OPL):**
   - **PS2:** the startup/disc ID (`SLUS_123.45`), same as OPL.
   - **PS1 VCD:** the **VCD filename stem**, *not* the serial. A serial-named file is accepted only if the VCD name itself begins with that serial.
   - **Ember PS1 games:** the **game-folder name** under `EMBER/games/<folder>/`.
   - **Apps:** the full ELF filename (per OrbitPS2; not cross-checked in RiptOPL code).
3. **Dimensions** (OrbitPS2's "RiptOPL" normalisation table):

   | System | COV | ICO | SCR / SCR2 | BG | LGO | LAB | COV2 |
   |---|---|---|---|---|---|---|---|
   | PS2 | 280×400 | 128×128 | 250×188 | 640×480 | 300×125 | 18×240 | 242×344 |
   | PS1 | 400×400 | 128×128 | 250×188 | 640×480 | 300×125 | 12×200 | 222×200 |

4. **Pixel format:** OrbitPS2 enforces **8-bit indexed (palette) PNG** "for RiptOPL compatibility". RiptOPL's texture loader uploads paletted PNGs as CLUT textures (`src/textures.c`), which saves PS2 VRAM and RAM. Whether RiptOPL *rejects* RGBA PNGs is **unverified**; treat indexed as strongly recommended. NathanNeurotic's PS2-Servers tool has a "Repair ART" action that converts legacy JPG/BMP/WebP art to 8-bit indexed PNG [unverified snippet].
5. **Optional archive `ART/art.tar`:**
   - The format is plain uncompressed **ustar** with `<ID>_<SUFFIX>.png` members. It was ported from **wOPL** (author mystyq) and is compatible with wOPL/sOPL art packs.
   - Packs may be **concatenated** (covers + BG + SCR joined with `cat` / `copy /b`). RiptOPL reads past mid-file EOF markers, like GNU tar's `--ignore-zeros`.
   - RiptOPL writes a sidecar index `art_cache.bin` next to the archive, keyed on the tar size.
   - Use of the archive is opt-in (`gEnableArtTar`, default off). Loose files are the fallback.
   - Analogous `CHT/cht.tar` (`<startup>.cht` members) is supported, and the code defines `CFG/cfg.tar`.
6. **CFG compatibility:** RiptOPL adds per-game keys such as:
   - `$CoreLoader`, `$NeutrinoArgs`, `$NeutrinoVideo`, `$NeutrinoGsmComp`, `$NeutrinoBsdfs`;
   - `$Ember*`, `$VMCDisable_N`, `$CheatSel*`;
   - compat bits **0x40 (Neutrino buffer-overrun fix)** and **0x80 (Slow Reads)**.

   A manager must **round-trip unknown keys and bits untouched**.
7. **PS1 layout:** "launcher-free" VCDs live in `POPS/` **at the device root**, even when PS2 games sit in a subfolder. There are no per-game `XX.<name>.ELF` launchers in APPS. Ember games go in `EMBER/games/<name>/` (CUE/BIN, optional `discs.txt` for multi-disc).

**Effort to support:** low to moderate.
- An "Art profile: OPL / RiptOPL" toggle that changes the resize targets, palette quantisation, and identity mapping for PS1/Ember/apps.
- An optional `art.tar` packer.
- A guarantee that CFG writes preserve unknown keys. That last item is needed anyway.
- PS1 "launcher style: POPStarter vs RiptOPL/POPSLoader" if we do PS1 at all.

### 3.4 Other interpretations considered

- **RIPPS2** (the RiptOPL-based front-end): related, but the comment names RiptOPL explicitly.
- **"Rips"** in the old scene sense (PS2 ISOs with FMV/audio stripped to fit a CD-R): there is no evidence of this in the thread, and OPL needs nothing special for such images. They are ordinary ISOs, so this is unlikely.
- **"RIP" as in someone deceased**: appears in unrelated psx-place threads, not relevant.

---

## 4. User wishlist and bug reports from the thread

From the visible content [unverified, search snippets only; the thread itself was not readable]:

- **Request (NathanNeurotic):** support **RiptOPL's art formats**, "just a checkbox and different naming schemes", as OrbitPS2 Manager already does.
- **Original post:** the author invites feature requests and issue reports via the repo and Discord. He frames the motivation as **OPL Manager being closed-source and awkward on Linux and Steam Deck under Wine**, which implies demand for a native Linux / Steam Deck tool. He lists a dark UI, grid/list views, search, art display, the bulk scraper, the BIN→ISO converter, and experimental VMC support (VMCs still created in OPL).
- No other replies, bug reports, or requests surfaced. Reply count in snapshots: 0–2.
- **Signals from elsewhere (indirect wishlist):** OPLNova's tracker has 0 issues. Gaps that users of competing tools clearly want, judging by what OrbitPS2 and RiptOPL ship:
  - ZSO conversion;
  - PS1 VCD with launcher-style choice;
  - APPS;
  - UL/split games;
  - VMC creation;
  - macOS (OPLNova added Intel-only macOS in 2.0.2–2.0.4);
  - a working default art source.

---

## 5. Bottom line

- OPLNova is a **small, early, single-author GPL-3.0 C#/Avalonia app**. It covers a narrow happy path: one ISO at a time → `ID.Title.iso` in `CD/`/`DVD/`, plus front/back/disc art and VMC/compat fields in CFG.
- It **lacks most OPL Manager features**: UL, ZSO, POPS, APPS, HDD, SMB, cheats, verification, i18n, updater.
- It has **data-loss bugs** (CFG rewriting, delete without confirmation). Its **art pipeline does not work out of the box**: the hosted API is empty, the default is localhost, and self-hosting needs an unpublished OPL Manager art zip.
- **No tests, no users on the tracker, 0 AUR votes.**
- It is not a serious threat. The more capable open-source rival is **OrbitPS2 Manager** (Electron, GPL-3.0, 127★, already RiptOPL-aware).
- The "RIP" request = **RiptOPL artwork conventions**: PS1/Ember identity naming, specific dimensions, indexed PNG, and the optional `ART/art.tar`. It is cheap to support and makes a good differentiator to add early.

---

## Sources

### Forum / web
- psx-place release thread (Cloudflare-blocked; read via search snippets): https://www.psx-place.com/threads/release-oplnova-a-modern-open-source-cross-platform-alternative-to-opl-manager.50635/
- psx-place Tools resources (OPLNova v2.0.1 listing, unverified): https://www.psx-place.com/resources/categories/tools.24/
- OPLNova site and docs: https://oplnova.xyz/ , https://oplnova.xyz/docs (TLS cert expired at fetch time; fetched with `curl -k`)
- OPLNova repo (OneDev): https://code.weexnes.dev/OPLNova ; builds: https://code.weexnes.dev/OPLNova/~builds ; issues: https://code.weexnes.dev/OPLNova/~issues
- OneDev REST used: `https://code.weexnes.dev/~api/projects`, `~api/builds`, `~api/issues`, `~api/pulls` (query `"Project" is "OPLNova"`)
- OPLNova VAPI: https://code.weexnes.dev/oplnova_vapi
- Dead hosted API (checked 2026-10-22): https://ps2api.weexnes.dev/
- AUR RPC: https://aur.archlinux.org/rpc/v5/info?arg[]=oplnova&arg[]=oplnova-git
- RiptOPL: https://github.com/NathanNeurotic/Open-PS2-Loader , https://github.com/NathanNeurotic/RiptOPL , docs https://nathanneurotic.github.io/Open-PS2-Loader/
- psx-place RiptOPL fork: https://www.psx-place.com/resources/fork-riptopl-customized-open-ps2-loader.1711/ , https://www.psx-place.com/threads/fork-riptopl-customized-open-ps2-loader.50327/
- RIPPS2: https://github.com/akilluminati47/RIPPS2 , https://www.psx-place.com/threads/ripps2-a-ps2-front-end-in-one-elf-launch-disc-storage-memory-files.51316/
- OrbitPS2 Manager: https://github.com/Luden02/OrbitPS2-Manager ; RiptOPL PR: https://github.com/NathanNeurotic/OrbitPS2-Manager-for-PR/pull/1 ; psx-place: https://www.psx-place.com/resources/orbitps2-manager-by-luden02.1789/
- Art DB dump: https://github.com/Luden02/psx-ps2-opl-art-database ; xlenore covers: https://github.com/xlenore/ps2-covers ; OPL Manager art backups: https://oplmanager.com/site/?backups
- Upstream OPL: https://github.com/ps2homebrew/Open-PS2-Loader

### Repo file paths (local clones, read-only)
- OPLNova (https://code.weexnes.dev/OPLNova):
  - `OPLNova\Core\Game.cs` (serial, title/art API, install, rename, art save sizes)
  - `OPLNova\Core\Config.cs` (CFG parse/save, compat bits)
  - `OPLNova\Core\ISO.cs` (SYSTEM.CNF parse)
  - `OPLNova\Core\IsoConverter.cs` (BIN→ISO)
  - `OPLNova\Core\Util.cs` (VMC create, MD5/SHA1)
  - `OPLNova\Core\Globals.cs`, `OPLNova\Core\ConfigUtils.cs` (settings)
  - `OPLNova\MainWindow.axaml.cs` (scan, delete, CFG handlers), `OPLNova\Setup.axaml` (API URL default/tooltip)
  - `OPLNova\BulkScraperWindow.axaml.cs`, `OPLNova\VmcCreateDialog.axaml.cs`, `OPLNova\ConvertGameWindow.axaml.cs`, `OPLNova\AddGameWindow.axaml.cs`
  - `OPLNova\OPLNova.csproj`, `.onedev-buildspec.yml`, `tools\dmg`, `LICENSE`, `README.md`
- OPLNova VAPI (https://code.weexnes.dev/oplnova_vapi): `api.v`, `setup_db.sh`, `contact.html`, `v.mod`, `README.md`
- RiptOPL (https://github.com/NathanNeurotic/Open-PS2-Loader): `src\tar.c`, `src\texcache.c`, `src\bdmsupport.c`, `src\textures.c`, `include\config.h`, `include\iosupport.h`, `docs\THEME_ENGINE.md`, `docs\VCD.md`, `README.md`
- OrbitPS2 (read via GitHub API): `src/utils/png-artwork.ts`, `angular/src/app/shared/utils/ps1-artwork-identities.ts`, `src/services/artwork.service.ts`, `docs/folder-layout.md`, `docs/guide.md`
