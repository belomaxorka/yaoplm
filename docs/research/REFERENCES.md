# Reference projects and community sources (2026-10-22)

An index of the code and forums that hold PS2/OPL knowledge outside the official docs: bug fixes, console and firmware quirks, format edge cases. Policy: ADR-0015 and CLAUDE.md rule 6.

**Local clones.**
- Path: `D:\yaoplm-testdata\refs\<owner>__<repo>` (Git Bash: `/d/yaoplm-testdata/refs/...`). The folder sits outside the YAOPLM repo, and nothing from it is ever committed.
- Clones are partial (`git clone --filter=blob:none`). They hold the full commit history; file contents outside the checkout are fetched on demand. `git log -p`, `git blame` and `git show <sha>:<path>` therefore need network access the first time.
- Refresh one clone with `git -C <path> pull`. To refresh all of them (Git Bash):
  `for d in /d/yaoplm-testdata/refs/*/; do git -C "$d" pull --ff-only -q || echo "failed: $d"; done`
- Many repos set `* text=auto` in `.gitattributes`, so on Windows the working tree has CRLF line endings. For byte-exact data (CFG, CHT) read the blob: `git -C <dir> show HEAD:<path>`.
- "Last commit" below is the checked-out HEAD on 2026-10-22, as date plus short SHA. For repos that were not cloned it is the date of the newest commit on the default branch, from the GitHub API.

**Rules when using a reference.**
- Upstream OPL (`ps2homebrew/Open-PS2-Loader`) defines behaviour. Forks and other managers are sources of edge cases; verify their behaviour before relying on it.
- When a YAOPLM rule or fix comes from a reference, cite the repo, commit and file in the code comment and in `docs/research/`, for example: `ps2homebrew/Open-PS2-Loader@b3a21997 src/supportbase.c`.
- Check the licence column before porting anything. AFL code (OPL, ps2sdk, Neutrino) is never copied, only reimplemented from its logic (ADR-0002). Code under a GPL-3-compatible licence may be ported with attribution. "none" means the repo has no licence: read only.
- Licences come from the GitHub API (`.license.spdx_id`). Where the API says `NOASSERTION` or `none`, the LICENSE/COPYING file was read instead and the table says so.

## 1. OPL and forks

| Project | Licence | Last commit | Useful for | Local |
|---|---|---|---|---|
| [ps2homebrew/Open-PS2-Loader](https://github.com/ps2homebrew/Open-PS2-Loader) (upstream) | AFL-3.0 | 2026-10-12 `b3a21997` | The reference for every format rule. `src/supportbase.c`: ISO name rules, ul.cfg read/rebuild, ZSO probe, ul part names `ul.%08X.%s.%02x`. `src/system.c`: `USBA_crc32`. `include/supportbase.h`: `USBExtreme_game_entry_t`. `src/config.c`: CFG parser/writer. `src/ps2cnf.c`: SYSTEM.CNF. `src/bdmsupport.c`, `src/ethsupport.c`, `src/hddsupport.c`: per-device paths. `src/textures.c`, `src/themes.c`: art. `src/cheatman.c`, `include/iosupport.h`, `modules/isofs/zso.c`, `modules/iopcore/common/cdvd_config.h`. PC tools: `pc/ziso.py`, `pc/iso2opl`, `pc/opl2iso`, `pc/genvmc`. Also `CHANGELOG`, 476 open issues, Discussions | yes |
| [Open-PS2-Loader wiki](https://github.com/ps2homebrew/Open-PS2-Loader/wiki) (`.wiki.git`) | — | 2022-05-11 `4ccdb3f3` | `usb-mode.md` (ul.cfg fragmentation, USB advice), `Home.md` | yes (`ps2homebrew__Open-PS2-Loader.wiki`) |
| [NathanNeurotic/Open-PS2-Loader](https://github.com/NathanNeurotic/Open-PS2-Loader) (RiptOPL "Legacy Edition", branch `rebuild/main`) | AFL-3.0 | 2026-10-22 `40c52ceb` | GitHub fork of ps2-mmce/Open-PS2-Loader, about 1,976 commits ahead of upstream. RiptOPL-only rules: `settings_riptopl.cfg` (`src/config.c`), art tar/cache (`src/tar.c`, `src/texcache.c`), `EMBER/`, `POPS/`, MMCE (`src/mmcesupport.c`). `docs/` covers GAME-FORMATS, EXFAT, VMC, VCD, MX4SIO, APA-SAFETY, COMPRESSED-IMAGES and UNICODE-GAME-TITLES. `notes/` has dated hardware audits; `pc/apa-recovery`. Upstream's README says the fork is heavily LLM-assisted, so verify before trusting | yes. The docs site is cloned separately (gh-pages branch, shallow, by a parallel session) as `NathanNeurotic__Open-PS2-Loader__gh-pages` |
| [NathanNeurotic/RiptOPL](https://github.com/NathanNeurotic/RiptOPL) (branch `rebuild/main`) | AFL-3.0 | 2026-10-20 `6edea174` | The C++/ps2build migration of RiptOPL. Not a GitHub fork, but it shares upstream history. `src/sources/*.cpp` (e.g. `mmcesupport.cpp`), `docs/UPSTREAM-PARITY-2026-10-12.md`, `docs/MIGRATION-CHECKPOINT-2026-10-12.md` | yes |
| [ps2-mmce/Open-PS2-Loader](https://github.com/ps2-mmce/Open-PS2-Loader) (the MMCE fork) | AFL-3.0 | `master` 2025-01-22 `e2090114`; `mmce_support` 2025-11-24 `e554796` | The owner is the GitHub org `ps2-mmce`. **MMCE code exists only on branch `mmce_support`** (28 ahead and 53 behind upstream); `master` has none. Files: `src/mmcesupport.c`, `include/mmcesupport.h`, `modules/iopcore/cdvdman/device-mmce.c`, `modules/mcemu/device-mmce.c`, `modules/iopcore/common/mmcedrv_config.h`. Read with `git show origin/mmce_support:<path>` | yes (`master` checked out) |
| [ps2homebrew/wOPL](https://github.com/ps2homebrew/wOPL) (branch `wOPL-base`) | AFL-3.0 | 2026-09-13 `4ae74148` | Continues KrahJohlito's uOPL. **MMCE support since 2026-02-28** (`0c9d14a`, credited to qnox32, rickgaiser and bbsan2k): `src/mmcesupport.c`, `device-mmce.c`. Also art `.tar` archives (`src/tar.c`), favourites, Coverflow, Neutrino for MMCE | yes |
| [officialjuicedesigns/OPL-Evolution](https://github.com/officialjuicedesigns/OPL-Evolution) (branch `evo-development`) | AFL-3.0 | 2026-07-25 `47119b71` | Experimental fork, v0.27.0-alpha: unified PS2/PS1 UI, per-game VMC assignment, VMC creation from 8 to 64 MB. See `docs/OPL_EVOLUTION_ARCHITECTURE.md`. Found by GitHub search; not in research/01 | yes |
| [NathanNeurotic/wOPL](https://github.com/NathanNeurotic/wOPL) | AFL-3.0 | 2026-04-26 | Fork of ps2homebrew/wOPL with 0 commits ahead and 155 behind, so nothing unique | no |
| [slimpuggamer/uOPL](https://github.com/slimpuggamer/uOPL) | AFL-3.0 | 2025-03-01 | Mirror of KrahJohlito's uOPL. The original repo no longer exists (KrahJohlito has no uOPL repo); wOPL supersedes it | no |
| [akilluminati47/RIPPS2](https://github.com/akilluminati47/RIPPS2) | none | 2026-10-22 | On-console front end based on RiptOPL (RiptOPL calls it its "daughter project"). HEAD is about 94 MiB, mostly binaries | no |

## 2. Other loaders, launchers and firmware

| Project | Licence | Last commit | Useful for | Local |
|---|---|---|---|---|
| [ps2max32/neutrino](https://github.com/ps2max32/neutrino) (formerly rickgaiser/neutrino; the old URL redirects) | AFL-3.0 | 2026-08-31 `7be8de27` | The device emulator behind NHDDL and RiptOPL's per-game core. `ee/loader/config/*.toml` (backing-store configs, e.g. `bsd-mmce.toml`), `iop/cdvdman_emu`, `iop/mc_emu`, `iop/hdlfs`, `iop/bdfs`. The repo moved to the `ps2max32` org (created 2026-09-14) | yes. A parallel session also cloned it as `rickgaiser__neutrino` (same HEAD) |
| [ps2-mmce/neutrino](https://github.com/ps2-mmce/neutrino) | AFL-3.0 | 2024-12-06 `30508edb` | Stale fork: 0 ahead, 203 behind | yes (parallel session) |
| [pcm720/nhddl](https://github.com/pcm720/nhddl) (only branch: `legacy`) | AFL-3.0 | 2026-04-13 `89141d47` | NHDDL launcher. `src/devices/*.c` (MMCE `mmce.c`, BDM, APA, UDPFS), `include/devices/title_id.h`, per-game YAML in `src/options.c` and `examples/nhddl.yaml`, `iop/mmceman` submodule | yes |
| [pcm720/OSDMenu](https://github.com/pcm720/OSDMenu) | AFL-3.0 (`common/`, `launcher/`, `mbr/`); FMCB licence (`patcher/`); the API shows none | 2026-07-31 `a2b847a2` | OSDMenu and HOSDMenu: OSDSYS/HDD-OSD patches based on FMCB 1.8. `patcher/README.md` (`OSDMENU.CNF`), `launcher/` | yes (parallel session) |
| [ps2-mmce/mmceman](https://github.com/ps2-mmce/mmceman) | MIT | 2026-08-01 `de28936f` | The MMCE IOP driver. `docs/MMCE CMD Protocol.md`, `docs/MMCE FS Protocol.md`, `mmceman/src/mmce_cmds.c`, `mmce_fs.c`, `mmcedrv.c` | yes |
| [sd2psx/firmware](https://github.com/sd2psx/firmware) (original SD2PSX) | GPL-3.0 | 2025-12-08 `2408cef5` | RP2040 memory-card emulator: `src/ps2/ps2_cardman.c`, `ps2_memory_card.c`, `src/ps1/` | yes |
| [sd2psXtd/firmware](https://github.com/sd2psXtd/firmware) | GPL-3.0 | 2026-07-08 `8a20bd99` | Fork chain sd2psx/firmware → bbsan2k/sd2psx_firmware → sd2psXtd/firmware. The card side of MMCE (`src/ps2/mmceman/*`), game ID → card mapping (`src/game_db/game_db.c`, `database/parse_GameDB.py`), history tracker | yes |
| [sd2psXtd/sd2psXtd.github.io](https://github.com/sd2psXtd/sd2psXtd.github.io) | MIT | 2026-08-01 `7a204d98` | Docs site source (`index.md`, `homebrew.md`, `sd_builder/`, `memcard_browser/`) | yes (parallel session, 101 MB) |
| [bbsan2k/sd2psx_firmware](https://github.com/bbsan2k/sd2psx_firmware) | GPL-3.0 | 2025-05-28 | Intermediate fork | no |
| [sd2psXtd/gameDB](https://github.com/sd2psXtd/gameDB) | none (API) | 2025-01-22 | Builds minimal game databases for SD2PSX | no |
| [israpps/PlayStation2-Basic-BootLoader](https://github.com/israpps/PlayStation2-Basic-BootLoader) (PS2BBL) | GPL-3.0 | 2026-02-11 `d2e81fa1` | FMCB alternative. Config search order is in `include/common.h` (`mc?:/SYS-CONF/PS2BBL.INI`, `mass:/PS2BBL/CONFIG.INI`, `hdd0:__sysconf:pfs:/PS2BBL/CONFIG.INI`). `src/main.c` handles the `LK_<key>_E#` launch keys. Also `src/OSDConfig.c`, `src/ps1.c` and `src/ps2.c` (disc boot) | yes |
| [TnA-Plastic/FreeMcBoot](https://github.com/TnA-Plastic/FreeMcBoot) | Free McBoot License (custom, "All Rights Reserved"; the API says NOASSERTION) | 2021-07-11 `2e4eef9a` | Source of FMCB **v1.8b** (2008, Neme & jimmikaelkael): `main.c`, `CNF/` (FREEMCB.CNF), launchers. The FMCB 1.9xx core is not public | yes |
| [israpps/FreeMcBoot-Installer](https://github.com/israpps/FreeMcBoot-Installer) | none | 2025-03-28 `ac53a47a` | Installers for FMCB 1.953–1.966: `installer/` source plus packaged binaries | yes (parallel session, 252 MB) |
| [ps2homebrew/wLaunchELF](https://github.com/ps2homebrew/wLaunchELF) | AFL-2.0 (LICENSE; the API says NOASSERTION) | 2026-10-12 `57c20494` | File manager and ELF launcher; device path conventions | yes |
| [israpps/wLaunchELF_ISR](https://github.com/israpps/wLaunchELF_ISR) | none | 2026-01-14 `74550d86` | israpps' wLaunchELF fork | yes (parallel session) |
| [CTurt/FreeDVDBoot](https://github.com/CTurt/FreeDVDBoot) | none | 2020-09-06 `ba7fa5bb` | DVD-player exploit. Only relevant to boot-setup docs | yes (parallel session) |
| [NathanNeurotic/POPSTARTERINFO](https://github.com/NathanNeurotic/POPSTARTERINFO) | none | 2026-06-22 `109b4b9e` | Source of nathanneurotic.github.io/POPSTARTERINFO (`docs/`, `data/`, `research/`). POPStarter conventions: `XX.`/`SB.` prefixes, VMC slots, cheats. POPStarter itself is closed source | yes |

## 3. PC managers

### 3.1 Folder-based (USB, exFAT, MX4SIO, SMB)

| Project | Licence | Last commit | Useful for | Local |
|---|---|---|---|---|
| [Luden02/OrbitPS2-Manager](https://github.com/Luden02/OrbitPS2-Manager) | GPL-3.0 | 2026-10-22 `cdd590ae` | Electron/TS. `src/services/library.service.ts` (parses ul.cfg), `cfg.service.ts`, `artwork.service.ts`, `artwork-filenames.ts`, `src/utils/png-artwork.ts` (8-bit PNG, RiptOPL names and sizes), `game-id-resolver.service.ts` (with tests), `zso.service.ts` plus `src/workers/zso.worker.ts`, `src/utils/cue2pops.ts`, `binmerge.ts`, `vmc.service.ts`, `src/data/ps1-disc-conflicts.json`. Issues #31, #48, #54 | yes |
| [brainstream/OPL-PC-Tools](https://github.com/brainstream/OPL-PC-Tools) | GPL-3.0 | 2026-08-30 `2291b5a9` | C++/Qt. `src/OplPcTools/UlConfigGameStorage.cpp` (ul.cfg read/write, its own CRC port), `UlConfigGameInstaller.cpp`, `Device/Zso.cpp`, `ZisoDeviceSource.cpp`, `BinCueDeviceSource.cpp`, `IsoRestorer.cpp`, `MemoryCard/` (PS2 MC filesystem read/write), `GameConfiguration.cpp`, `GameCheatManager.cpp`. ImHex patterns for the binary formats: `imhex/ulconfig.hexpat`, `vmc.hexpat`, `ziso.hexpat` | yes |
| [edisnord/PyOPLM](https://github.com/edisnord/PyOPLM) | GPL-3.0 | 2026-09-29 `a8e3ceb1` | Python CLI, fork of libopl. `pyoplm/ul.py`, `common.py` (`usba_crc32`), `zso.py`, `game.py`, `storage.py`, `bchunk.py`, `binmerge.py`, `cue2pops_basic_conversion.py`, `test_storage_bugs.py` | yes |
| [Nold360/libopl](https://github.com/Nold360/libopl) | GPL-3.0 | 2019-08-31 `fd6a1f86` | `libopl/ul.py`, `common.py`, `game.py`, `artwork.py`. Abandoned | yes |
| [WeeXnes/OPLNova](https://code.weexnes.dev/OPLNova) (OneDev) | GPL-3.0 (LICENSE) | 2026-10-05 `7c307572` | C#/Avalonia. `OPLNova/Core/ISO.cs`, `IsoConverter.cs`, `Artwork.cs`, `Config.cs`. Audited in research/05. OneDev does not support partial clone, so this is a full clone | yes |
| [WeeXnes/oplnova_vapi](https://code.weexnes.dev/oplnova_vapi) (OneDev) | none | 2026-10-06 `4806a791` | OPLNova's title/art API server, written in V (`api.v`, `setup_db.sh`) | yes |
| [imadali120/PS2IsoManager](https://github.com/imadali120/PS2IsoManager) | MIT | 2026-03-03 `b1a5d16f` | C#/WPF. `Services/OplCrc32.cs`, `UlCfgService.cs`, `IsoSplitterService.cs`, `Iso9660Reader.cs` | yes |
| [developersu/Tihwin](https://github.com/developersu/Tihwin) | GPL-3.0 | 2026-10-05 `39386f25` | Java. `tihwin/ul/UlMaker.java`, `UlConfiguration.java` (CRC), `src/test/java/UlCfgTests.java` | yes |
| [gecm0/POUM](https://github.com/gecm0/POUM) | none in the API; README says MIT (**unverified**) | 2026-01-04 `bffd600a` | Tauri 2 + React, the same stack as YAOPLM. `src-tauri/src/core/crc.rs`, `ul_config.rs`, `splitter.rs`, `iso.rs`, `src-tauri/src/fs/opl_structure.rs` | yes |

Long tail from research/01 §2.7. Mostly small and 2025–26, many look LLM-assisted. All cloned except PopCraft (see the PopCraft row).

| Project | Licence | Last commit | Notes |
|---|---|---|---|
| [dheison0/ps2-game-manager](https://github.com/dheison0/ps2-game-manager) | MIT | 2026-10-18 `f92dba17` | Go TUI, `manager/game.go` |
| [lucasonline0/opl-linux-toolbox](https://github.com/lucasonline0/opl-linux-toolbox) | GPL-3.0 | 2026-09-01 `3a776d14` | Electron/Angular. `src/services/safe-import.service.ts`, `library.service.ts`; ships an `iso2opl` binary |
| [gpulch/ps2-manager](https://github.com/gpulch/ps2-manager) | none | 2026-02-10 `a0a245f8` | TS. `docs/REMOTE_SOURCES.md` |
| [ShaunCarrPHL/PS2FASTLOADER](https://github.com/ShaunCarrPHL/PS2FASTLOADER) | MIT | 2026-07-11 `c47be7ba` | Tauri/Rust. `desktop/src-tauri/src/game_ops.rs`; bundles mymc |
| [JO0812/TinyPS2Manager](https://github.com/JO0812/TinyPS2Manager) | none | 2026-09-29 `b1b10794` | Go. `internal/oplfs/tree.go`, `internal/invariants/` |
| [logi-26/ps2-game-manager](https://github.com/logi-26/ps2-game-manager) | GPL-3.0 | 2026-09-10 `75291ccb` | Java. `IsoSplitter.java`, `IsoMerger.java`, `PopsGameManager.java` |
| [donydaily/PS2OPLManager-cli](https://github.com/donydaily/PS2OPLManager-cli) | none | 2025-12-25 `f47d7ff1` | Python, `ulcfg.py` |
| [tekmyster/ps2-opl-ripper](https://github.com/tekmyster/ps2-opl-ripper) | GPL-3.0 | 2026-09-19 `627a6a12` | Python disc ripper. Vendors FATtools, a FAT/exFAT implementation in Python (`src/FATtools/`) |
| [Zsubzeroz/PyOPL-Utility](https://github.com/Zsubzeroz/PyOPL-Utility) | MIT | 2026-05-06 `9f773e01` | Python, `core/splitter.py` |
| [Informaldehyde-xd/PS2-MANAGER-FOR-ANDROID-](https://github.com/Informaldehyde-xd/PS2-MANAGER-FOR-ANDROID-) | MIT | 2026-09-18 `de472a7f` | Kotlin, `app/src/main/java/com/ps2manager/app/data/UlConfig.kt` |
| [N0NamedGuy/opl-manager-next](https://github.com/N0NamedGuy/opl-manager-next) | none | 2020-02-14 `30dfbd30` | JS, stale |
| [IsseiYoshida/OPLUtil](https://github.com/IsseiYoshida/OPLUtil) | none | 2021-05-11 `9c563355` | 4 commits |
| [astaroth666/PS2-Cover-Art-](https://github.com/astaroth666/PS2-Cover-Art-) | none | 2026-06-19 `c15d0295` | Bash script that pulls art from an archive |
| [shaanhomebrew-cloud/OPL-PS1-AIO-Converter-GUI](https://github.com/shaanhomebrew-cloud/OPL-PS1-AIO-Converter-GUI) | none | 2026-08-19 `b25d20b3` | README only; distributed through releases |
| [TonyLimaPHB/PopCraft](https://github.com/TonyLimaPHB/PopCraft) | MIT | 2025-09-26 | Python PS1 → VCD. **Not cloned:** the repo ships Sony `POPS.ELF`, `POPS_IOX.PAK` and a 512 KiB `BIOS.BIN` (ADR-0007). A clone was made by mistake and deleted |

### 3.2 Internal HDD (APA/PFS)

| Project | Licence | Last commit | Useful for | Local |
|---|---|---|---|---|
| [israpps/HDL-Batch-installer](https://github.com/israpps/HDL-Batch-installer) | GPL-3.0 | 2026-03-25 `a724a26d` | wxWidgets C++, in `HDL-Batch-installer-SRC/`: `ArtMan.cpp`, `gamename/` (title DB), `HDDManager.cpp`, `NDBMan.cpp` (NBD), `PFSShell.cpp`, `MD5Man.cpp` | yes (158 MB) |
| [GDX-X/PFS-BatchKit-Manager](https://github.com/GDX-X/PFS-BatchKit-Manager) | none | 2026-10-16 `eb77f51a` | Windows batch: `PFS-BatchKit-Manager/!PFS-BatchKit-Manager.bat`, `BAT/*.BAT`. Bundles tools and `BAT/Dats/PS2/RedumpPS2.dat` | yes (259 MB) |
| [CosmicScale/PSBBN-Definitive-Project](https://github.com/CosmicScale/PSBBN-Definitive-Project) | GPL-3.0 | 2026-10-20 `59992562` | Linux shell. `scripts/Game-Installer.sh` (ISO/ZSO/VCD install, art, APA-Jail exFAT+APA), `scripts/HOSDMenu-Installer.sh`, `PSBBN-Definitive-Patch.sh` | yes |
| [L10N37/PS2-HDD-Manager](https://github.com/L10N37/PS2-HDD-Manager) | GPL-3.0-or-later (LICENSE; the API says NOASSERTION) | 2026-09-12 `1a30bc4b` | C++/Qt. `src/core/Ps2Apa.cpp`, `OplConfig.cpp`, `FhdbConfig.cpp`; `FOUR_TB_DESIGN.md` (Extended APA >2 TiB); `docs/INSTALLED_TITLE_AND_PC_GAMEID.md` | yes |

## 4. PS2 tools and SDK

| Project | Licence | Last commit | Useful for | Local |
|---|---|---|---|---|
| [ps2dev/ps2sdk](https://github.com/ps2dev/ps2sdk) | AFL-2.0 (LICENSE; the API says NOASSERTION) | 2026-10-12 `329e2bb4` | `iop/fs/bdm`, `iop/fs/bdmfs_fatfs` (exFAT via FatFs), `iop/hdd/apa`, `libapa`, `pfs`, `apa-gpt`, `iop/sio/mx4sio_bd`, `iop/cdvd/*`, `common/include/libcdvd-common.h`, `download_dependencies.sh` | yes |
| [fjtrujy/FatFs](https://github.com/fjtrujy/FatFs) | FatFs licence (ChaN, BSD-style; the API says NOASSERTION) | `main` 2024-04-18 `cac33149` | The FatFs that PS2SDK uses. Branch `iop-r0.16`: `source/include/ffconf.h` (exFAT, LFN, sector settings). Also branches `iop-r0.14b`, `iop-r0.15`, `iop-r0.16a` | yes |
| [ps2homebrew/hdl-dump](https://github.com/ps2homebrew/hdl-dump) | GPL-2.0 (COPYING; the API says none) | 2024-06-06 `32c296c6` | `hdl.c` (HDL install, header), `apa.c`, `isofs.c`, `iin_*.c` (image readers), `ps2_hdd.h`. [AKuHAK/hdl-dump](https://github.com/AKuHAK/hdl-dump) is identical to it, so it is not cloned | yes |
| [ps2homebrew/pfsshell](https://github.com/ps2homebrew/pfsshell) | GPL-2.0; AFL-2.0 for the libs | 2026-04-10 `8c92467b` | `src/shell.c`, `src/pfsd.c`, `subprojects/apa`, `pfs`, `hdlfs`; pfsfuse | yes |
| [unknownbrackets/maxcso](https://github.com/unknownbrackets/maxcso) | ISC | 2024-01-25 `961f232c` | **`README_ZSO.md` (the ZSO format spec)**, `README_CSO.md`, `src/` | yes |
| [Adubbz/mymcplusplus](https://github.com/Adubbz/mymcplusplus) | GPL-3.0 | 2026-07-28 `3247c9c0` | PS2 memory-card filesystem: `mymcplusplus/ps2mc.py`, `ps2mc_ecc.py`, `ps2mc_dir.py`, save formats, `test/` | yes |
| [bucanero/ps2vmc-tool](https://github.com/bucanero/ps2vmc-tool) | GPL-3.0 | 2026-09-27 `32a02451` | C. `include/mcio.h`, `ps1card.h`, `ps2blank.h`. Sample VMC images in `samples/` (`oplcard.bin`, 16/32 MB cards, PS1 card) | yes (76 MB) |
| [Al-Azif/ps-disc-detect](https://github.com/Al-Azif/ps-disc-detect) | GPL-3.0 | 2026-10-09 `0869fd11` | C++ PS1/PS2 serial detection: `include/disc_serial.h`, `detect_disc.h` | yes |
| [ErikAndren/cue2pops-mac](https://github.com/ErikAndren/cue2pops-mac) | none | 2015-11-04 `608aebf7` | `cue2pops.c`, `PATCHES.TXT` | yes |
| [tallero/cue2pops-linux](https://github.com/tallero/cue2pops-linux) | none | 2018-01-04 `541863ad` | `cue2pops.c` (Linux port) | yes |
| [NathanNeurotic/PS2-Servers](https://github.com/NathanNeurotic/PS2-Servers) | AFL-3.0 | 2026-10-20 `42c0613b` | LAN servers that RiptOPL recommends: `smbv1_server/`, `udpbd_server/`, `udpfs_server/`, `http_server/` | yes |
| [Equiel-1703/ps2-network-manager](https://github.com/Equiel-1703/ps2-network-manager) | MIT | 2025-03-22 `c11aa07a` | Python GUI that sets up Samba for OPL on Linux (SMBv1 settings) | yes |
| [milouk/docker-smb-opl](https://github.com/milouk/docker-smb-opl) | none | 2021-09-11 `1d8a8799` | Dockerised Samba for OPL | yes |
| [ps2-mmce/ps2sdk](https://github.com/ps2-mmce/ps2sdk) | NOASSERTION (fork of ps2sdk, AFL-2.0) | 2025-07-09 | ps2sdk fork used by the MMCE work | no |
| [saildot4k/ATA-Assault](https://github.com/saildot4k/ATA-Assault) | NOASSERTION | 2026-06-23 | exFAT HDD support for closed old homebrew | no |

## 5. Data sources

| Project | Licence | Last commit | Useful for | Local |
|---|---|---|---|---|
| [ps2homebrew/Open-PS2-Loader-Compatibility-list](https://github.com/ps2homebrew/Open-PS2-Loader-Compatibility-list) | MIT | 2022-12-16 `3fbbfafb` | `List/<letter>/<ID>.md`: per-game modes | yes |
| [VTSTech/PS2-OPL-CFG](https://github.com/VTSTech/PS2-OPL-CFG) (formerly Veritas83/PS2-OPL-CFG) | GPL-3.0 | 2023-03-01 `c669f0c7` | `CFG/*.cfg`: real-world per-game CFG files, a corpus for parser/round-trip tests | yes |
| [Tom-Bruise/PS2-OPL-CFG-Database](https://github.com/Tom-Bruise/PS2-OPL-CFG-Database) | GPL-3.0 | 2021-11-22 `7dc9204a` | `CFG_en/`, `CFG_fr/`: about 13.6k CFG files with metadata keys. A round-trip corpus for "preserve unknown keys and order" | yes |
| [GDX-X/PS2-OPL-CFG-Compatibility-Database](https://github.com/GDX-X/PS2-OPL-CFG-Compatibility-Database) | none | 2024-07-17 `45c7e5b5` | `Games_List.txt`, `HDD/` | yes |
| [PS2-Widescreen/OPL-Widescreen-Cheats](https://github.com/PS2-Widescreen/OPL-Widescreen-Cheats) | none | 2026-10-14 `837624eb` | `CHT/*.cht`, `BROKEN/`, `MISSING_MASTERCODE.TSV`: a corpus for the CHT parser | yes |
| [root670/CheatDevicePS2](https://github.com/root670/CheatDevicePS2) (archived) | GPL-3.0 | 2020-07-06 `7d9c525a` | `CheatDatabase.txt` | yes |
| [PCSX2/pcsx2_patches](https://github.com/PCSX2/pcsx2_patches) | none | 2026-10-21 `8e658bcb` | `patches/*.pnach` (widescreen, no-interlace) for pnach → cht conversion. Small (14 MB), unlike PCSX2 itself | yes |
| [PCSX2/pcsx2](https://github.com/PCSX2/pcsx2) | GPL-3.0 | 2026-10-22 | `bin/resources/GameIndex.yaml`, `pcsx2/Elfheader.cpp` (ELF CRC). Huge; fetch single files raw | no |
| [KrystianLesniak/pcsx2_patches_list](https://github.com/KrystianLesniak/pcsx2_patches_list) | MIT | 2026-10-22 | Generated list of pcsx2_patches | no |
| [xlenore/ps2-covers](https://github.com/xlenore/ps2-covers) | none | 2026-09-27 | `covers/default/<serial>.jpg`, `covers/3d/`. About 8.5 GB | no |
| [libretro-thumbnails/libretro-thumbnails](https://github.com/libretro-thumbnails/libretro-thumbnails) | none | 2026-09-06 | Hub of per-system submodules | no |
| [libretro-thumbnails/Sony_-_PlayStation_2](https://github.com/libretro-thumbnails/Sony_-_PlayStation_2) | none | 2026-10-18 | `Named_Boxarts/` etc. About 6.3 GB | no |
| [libretro/libretro-database](https://github.com/libretro/libretro-database) | CC-BY-SA-4.0 | 2026-10-10 | `metadat/redump/Sony - PlayStation 2.dat`. About 0.8 GB | no |
| [Luden02/psx-ps2-opl-art-database](https://github.com/Luden02/psx-ps2-opl-art-database) | none ("assets belong to their respective owners") | 2025-08-14 | Static dump of the OPLM art DB, about 4 GB. Reference only; see ADR-0007 and research/03 | no |
| [CosmicScale/HDD-OSD-Icon-Database](https://github.com/CosmicScale/HDD-OSD-Icon-Database) | none | 2026-10-16 | HDD-OSD/PSBBN icons, about 130 MB | no |

Deliberately excluded:
- **Sony POPS/BIOS binaries (ADR-0007):** AnimMouse/POPS-binaries and TonyLimaPHB/PopCraft. Neither is cloned.
- **Generic libraries and frameworks** named in research/04 (Tauri, Wails, go-yaml, go-diskfs, go-smb2, sevenzip, iso9660, etc.): not PS2 references, so they are not indexed.

## 6. Forums and community

psx-place.com sits behind a Cloudflare bot check: automated fetches get 403. Do not try to bypass it. Read threads through the user's Chrome (ADR-0015). The sources were the OPL README, research/01–05 and web search on 2026-10-22. Search-only finds are marked as such; none of these threads were opened.

**psx-place: OPL core**
- OPL forum section (linked from the OPL README): https://www.psx-place.com/forums/open-ps2-loader-opl.77/
- OPL game bug reports (OPL README): https://www.psx-place.com/threads/open-ps2-loader-game-bug-reports.19401/
- OPL game compatibility lists (research/03): https://psx-place.com/threads/open-ps2-loader-game-compatibility-lists.19037/
- [Testers Needed] OPL with ZSO support (research/02): https://psx-place.com/threads/testers-needed-open-ps2-loader-with-zso-support-compressed-iso.37223/
- [Testers Needed] OPL with exFAT support, files above 4 GB on USB (search): https://psx-place.com/threads/testers-needed-open-ps2-loader-with-exfat-support-files-above-4gb-on-usb.37455/
- [Testers Needed] OPL internal exFAT 2TB+ HDD and multi-BDM devices (search): https://psx-place.com/threads/testers-needed-opl-internal-exfat-2tb-hdd-and-multi-bdm-devices.40018/
- OPL not showing cover art (research/01): https://psx-place.com/threads/opl-not-showing-cover-art.48498/
- Tags: https://psx-place.com/tags/openps2loader/ , https://psx-place.com/tags/neutrino/ , https://psx-place.com/tags/uopl/

**psx-place: MX4SIO and memory-card emulators**
- [MX4SIO/SIO2SD] SD card adapter and SD driver (search): https://psx-place.com/threads/mx4sio-sio2sd-sd-card-adapter-and-sd-driver-for-the-ps2-sio2-interface.29210/
- OPL MX4SIO: only MX4SIO or USB (search; device lists are separate per device): https://psx-place.com/threads/opl-mx4sio-only-mx4sio-or-usb.46927/
- OPL 1.2.0 MX4SIO not being read (search): https://psx-place.com/threads/opl-1-2-0-mx4sio-not-being-read.49591/
- MX4SIO tag: https://psx-place.com/tags/mx4sio/
- MMCE (SD2PSX, MemCard PRO 2): no psx-place thread found (**unverified**). Off-site guide with OPL MMCE settings: https://retrorgb.com/?p=48205

**psx-place: forks and managers**
- RiptOPL thread: https://www.psx-place.com/threads/fork-riptopl-customized-open-ps2-loader.50327/ ; resource: https://www.psx-place.com/resources/fork-riptopl-customized-open-ps2-loader.1711/
- uOPL "[POC/Outdated]" thread (search): https://psx-place.com/threads/poc-outdated-unofficial-open-ps2-loader-uopl.45661/
- RIPPS2 thread: https://www.psx-place.com/threads/ripps2-a-ps2-front-end-in-one-elf-launch-disc-storage-memory-files.51316/
- OPLNova release thread (the RiptOPL art spec is in post 427638; see research/05 §3.4): https://www.psx-place.com/threads/release-oplnova-a-modern-open-source-cross-platform-alternative-to-opl-manager.50635/
- OrbitPS2 Manager thread: https://www.psx-place.com/threads/orbitps2-manager-by-luden02.51158/ ; resource: https://www.psx-place.com/resources/orbitps2-manager-by-luden02.1789/
- OPL Manager thread: https://psx-place.com/threads/opl-manager-tool-to-manage-your-games.19055/ ; resource: https://psx-place.com/resources/opl-manager-tool-to-manage-your-games.663/ ; tag: https://psx-place.com/tags/opl-manager/
- PyOPLM thread: https://www.psx-place.com/threads/pyopl-manager-pyoplm-a-cli-only-open-source-python-app-for-managing-opl-games.41658/
- PFS BatchKit Manager resource: https://psx-place.com/resources/pfs-batchkit-manager-2tb-support.1202/
- Tools resource category: https://www.psx-place.com/resources/categories/tools.24/

**GitHub trackers**
- OPL issues: https://github.com/ps2homebrew/Open-PS2-Loader/issues ; Discussions (enabled): https://github.com/ps2homebrew/Open-PS2-Loader/discussions
- RiptOPL issues: https://github.com/NathanNeurotic/Open-PS2-Loader/issues ; OrbitPS2: https://github.com/Luden02/OrbitPS2-Manager/issues ; NHDDL: https://github.com/pcm720/nhddl/issues ; Neutrino: https://github.com/ps2max32/neutrino/issues ; PSBBN: https://github.com/CosmicScale/PSBBN-Definitive-Project/issues ; sd2psXtd: https://github.com/sd2psXtd/firmware/issues

**Docs sites**
- OPL: https://ps2homebrew.github.io/Open-PS2-Loader/ ; RiptOPL: https://nathanneurotic.github.io/Open-PS2-Loader/ ; POPStarter info: https://nathanneurotic.github.io/POPSTARTERINFO/ ; PS2-Servers: https://nathanneurotic.github.io/PS2-Servers/ ; sd2psXtd: https://sd2psxtd.github.io
- ConsoleMods OPL wiki (403 for bots): https://consolemods.org/wiki/PS2:Open_PS2_Loader_(OPL)

**Other forums and chat**
- ps2-home.com forum: http://www.ps2-home.com/forum/ (**unverified**, not reached). Research/01 mentions an OPL Manager section (`viewforum.php?f=64`), and a psx-place tag page links an OPLM cover-art tutorial at `viewtopic.php?p=15806` (both **unverified**).
- Reddit: https://www.reddit.com/r/ps2homebrew/ and https://www.reddit.com/r/ps2/ (**unverified**; reddit blocks automated fetches, and search did not confirm r/ps2homebrew)
- OPL Discord, invite from the OPL README: https://discord.gg/CVFUa9xh6B (**unverified** that it is still valid)
- GBAtemp: PFS BatchKit https://gbatemp.net/threads/pfs-batchkit-manager-ps2-hdd-2tb-support.602462/ ; OPL BATCH HDD workflow https://gbatemp.net/threads/opl-batch-finalizing-the-installation-of-a-ps2-hdd.556480/
- BitBuilt libopl thread (site was in maintenance): https://bitbuilt.net/forums/threads/release-libopl-opl-manager-alternative-python-lib.2916/
- cdrinfo.pl OPL Manager thread (changelog mirror): https://forum.cdrinfo.pl/f106/ps2-opl-manager-96416-print/

## 7. Findings made while building this index

- **MMCE support (corrects research/02 §2, which names only ps2-mmce):**
  - Upstream OPL has no MMCE code at `b3a21997`.
  - ps2-mmce/Open-PS2-Loader has MMCE only on branch `mmce_support`.
  - wOPL merged MMCE on 2026-02-28 (`0c9d14a`).
  - RiptOPL (forked from ps2-mmce) has it in both repos.
  - Neutrino has `bsd-mmce.toml` and NHDDL has `src/devices/mmce.c`.
  - The protocol is documented in ps2-mmce/mmceman `docs/`; the card side is sd2psXtd/firmware `src/ps2/mmceman/`.
- **ul part-number formatting differs between tools.** Upstream OPL uses `ul.%08X.%s.%02x` (`src/supportbase.c`, lines 439/726/806). Other tools differ:

  | Tool | Part-number format |
  |---|---|
  | OPL's own `pc/iso2opl/src/iso2opl.c` | decimal `%02d` |
  | OPL-PC-Tools `UlConfigGameStorage.cpp:246` | decimal |
  | POUM `games.rs` | decimal `{:02}` |
  | PS2IsoManager | uppercase hex `{part:X2}` |
  | PyOPLM `common.py:51` | uppercase hex |

  The variants agree only for parts 00–09, and a DVD-9 needs at most 8 parts, so the difference is latent.
- **PyOPLM CRC padding (apparent bug, not tested).** `pyoplm/common.py:49` and `ul.py:193/211` format the CRC with `hex(...)[2:]` and no zero-padding to 8 digits. Only `game.py:150` pads, with `zfill(8)`. A title whose CRC is below 0x10000000 would get a 7-digit name that OPL does not find. Follow OPL's `%08X`.
- **Ports of `USBA_crc32` to cross-check against OPL `src/system.c` (none verified here):**
  - OPL-PC-Tools: `UlConfigGameStorage.cpp:79`
  - PS2IsoManager: `OplCrc32.cs`
  - POUM: `core/crc.rs`
  - Tihwin: `UlConfiguration.java`
  - PyOPLM: `common.py:141`
  - libopl: `common.py:98`
  - iso2opl: `iso2opl.c:48`
- **Repo moves and renames:**
  - Neutrino moved to the `ps2max32` org.
  - Veritas83/PS2-OPL-CFG is now VTSTech/PS2-OPL-CFG.
  - NHDDL's only branch is `legacy`.
  - RiptOPL has two repos: a C "Legacy Edition" (a fork of ps2-mmce) and a C++ "PS2Build Edition".
