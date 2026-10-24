# 17 — Loader install and update: prior art

Date: 2026-10-25. Scope: milestone M5, loader installer (SPEC LDR-1…LDR-4, LDR-6, LDR-7, BOOT-1, DEV-4). Goal: how neighbouring tools actually download, install, update and replace PS2 loaders, and the current release facts our catalogue (`data/loaders/<id>.toml`) depends on. research/07 (catalogue, boot paths, configs, proposal) and research/16 (signs per build) are background and are not repeated here.

Sources (local clones under `D:\yaoplm-testdata\refs\`, `git pull --ff-only` on 2026-10-25; nothing built or run; no release asset downloaded, only `gh api` metadata):

| Tool / build | Clone | HEAD |
|---|---|---|
| TinyPS2Manager (Go) | `JO0812__TinyPS2Manager` | `b1b1079` (2026-09-29) |
| OrbitPS2 Manager | `Luden02__OrbitPS2-Manager` | `cdd590a` (2026-10-22) |
| PS2 HDD Manager | `L10N37__PS2-HDD-Manager` | `1a30bc4` (2026-09-12, v0.2.0) |
| FreeMcBoot-Installer | `israpps__FreeMcBoot-Installer` | `ac53a47` (2025-03-28) |
| PS2BBL | `israpps__PlayStation2-Basic-BootLoader` | `d2e81fa` (2026-02-11) |
| OSDMenu | `pcm720__OSDMenu` | `a2b847a` (2026-07-31) |
| RiptOPL C ("Legacy Edition"), `rebuild/main` | `NathanNeurotic__Open-PS2-Loader` | `55261de1` (2026-10-23) |
| RiptOPL C++ ("PS2Build Edition") | `NathanNeurotic__RiptOPL` | `6edea174` (2026-10-20) |
| RiptOPL docs site (`gh-pages`) | `NathanNeurotic__Open-PS2-Loader__gh-pages` | `20d2113` (2026-10-23) |
| OPL upstream | `ps2homebrew__Open-PS2-Loader` | `b3a21997` (2026-10-12) |
| YAOPL (owner's fork, working tree only, not fetched) | `C:\Users\Admin\Projects\Open-PS2-Loader` | `33a2b19a` (2026-10-23) = remote `master` per `gh api repos/belomaxorka/yaopl/commits/master` |
| PSBBN Definitive | `CosmicScale__PSBBN-Definitive-Project` | `6c7c122` (2026-10-23) |
| PFS BatchKit Manager | `GDX-X__PFS-BatchKit-Manager` | `eb77f51` (2026-10-16) |
| OPL Linux Toolbox | `lucasonline0__opl-linux-toolbox` | `3a776d1` (2026-09-01) |
| OPLNova, PyOPLM, PS2 Manager for Android, gpulch ps2-manager | `WeeXnes__OPLNova` `7c30757`, `edisnord__PyOPLM` `a8e3ceb`, `Informaldehyde-xd__PS2-MANAGER-FOR-ANDROID-` `de472a7`, `gpulch__ps2-manager` `a0a245f` | — |

Release facts: `gh api repos/<o>/<r>/releases[/tags/<t>]` and `git/ref/tags/<t>` on 2026-10-25.

Tags: **[V]** verified in code, CI config or a GitHub API response (file or endpoint named); **[R]** README, docs site or release text; **[D]** deduced from verified facts; **[U]** unverified.

Licences: the tools above are AFL-3.0, GPL-3.0, MIT or unlicensed; nothing here is copied code, only behaviour (ADR-0002).

---

## 0. Key findings

1. **No neighbouring tool backs up the ELF it replaces.** Every installer overwrites: TinyPS2Manager by temp file + rename, PS2 HDD Manager and PFS BatchKit through `pfsshell put` (BatchKit deletes first), PSBBN by `rsync` when the MD5 differs, the FMCB installer with `O_TRUNC`. The only guards are by ownership: PS2 HDD Manager updates OPL only in the layout it created itself and disables the option elsewhere; OrbitPS2 refuses to import an app into an existing folder (§1–§4, §8).
2. **Nobody records an install manifest on the device.** TinyPS2Manager keeps `tag, asset, digest, flavour, elfPath, elfSize, stagedAt` in its own SQLite; PS2 HDD Manager writes a `MANIFEST.txt` of downloads on the PC. The only on-device manifest in the set is FMCB's own `SYS-CONF/FMCBUINST.dat` (legacy `uninstall.dat`), versioned, which the installer uses to refuse a second multi-install (§1, §3, §4.1).
3. **Nobody detects the installed loader version.** No tool reads an installed ELF to tell its version; "update" means "the digest or MD5 differs" (PS2 HDD Manager, PSBBN) or is not offered at all (TinyPS2Manager) (§9 g).
4. **The stock FMCB installer writes OPL v1.0.0 to the memory card.** `installer_res/<1953…1966>/INSTALL/APPS/OPNPS2LD.ELF` has SHA-256 `4b7a8190…a0d1`, which is `OPNPS2LD-v1.0.0.ELF` in our `data/loader-hashes/opl-stable.txt`. All five stock `FREEMCB.CNF` files (1.953, 1.963–1.966) try `mass:/UPDT/OPL.ELF` before that card copy (§4.1).
5. **Package layouts drift under consumers.** TinyPS2Manager looks for `APP_RIPTOPL-<FLAVOUR>/RIPTOPL.ELF` at the zip root. RiptOPL moved those folders under `APPS/` on 2026-09-20 (`d91ffcae`), and its packager now rejects root-level app folders. So TinyPS2Manager cannot stage from the current `rolling` package [V code + CI; the failure itself is D]. Our catalogue needs member paths verified against real listings, and the installer needs a clear "layout changed" error (§1, §5).
6. **Release metadata is unreliable for "which build, which is newer".**
   - Upstream `latest` is named `latest`; its text has no version. The version appears only in asset names. `target_commitish` is `master`; the tag ref gives the commit.
   - RiptOPL `rolling` reports `target_commitish` `05297663` (2026-09-13), but its tag points to `55261de1`.
   - The C++ edition was published 2026-10-16, yet its assets were replaced on 2026-10-20.
   - The `current-fan-favorite` "snapshot" was published 2026-09-04, and its assets were re-uploaded on 2026-09-30.
   Use the tag ref, asset `updated_at`, digests and the version string in asset or member names (§5–§7).
7. **Checksums files:**
   - YAOPL `SHA256SUMS` lists 4 files in `sha256sum` format.
   - RiptOPL C++ `SHA256SUMS.txt` lists 5 files, `BUILD-MANIFEST.json` among them.
   - RiptOPL C `rolling` computes one and then deletes it from the release on purpose; only `rolling-ra` keeps one.
   - Upstream has none.
   - GitHub `digest` is present on every current asset of all four (§5–§7).
8. **Starter settings: two tools seed `conf_opl.cfg`, both on the HDD, neither checks the memory card.**
   - PS2 HDD Manager replaces the file with a preset, pre-ticked.
   - PSBBN deletes its own keys and appends new values (LF).
   - OSDMenu's MBR installer writes its config only when none exists.
   - RiptOPL's docs say: back up settings and do not overwrite them with package defaults (§3, §4.3, §8, §9 c).

---

## 1. TinyPS2Manager (`JO0812/TinyPS2Manager`, Go)

All [V] at `b1b1079` unless tagged. Files: `internal/riptopl/riptopl.go`, `doc.go`, `riptopl_test.go`, `internal/api/prepare.go`, `internal/api/enrich.go`, `riptopl.go` (CLI), `internal/transfer/disk.go`, `OPL-Backup-Manager-SPEC.md` §2.7.

| Step | Behaviour |
|---|---|
| Source | Hard-coded repo `NathanNeurotic/Open-PS2-Loader`. Only RiptOPL, no other loader. Default tag `current-fan-favorite`, alternative `rolling` (`riptopl.go:22-29`) |
| Release lookup | `GET https://api.github.com/repos/<repo>/releases/tags/<tag>`, `Accept: application/vnd.github+json`. No User-Agent of its own (Go default), no token, no ETag or cache of the JSON (`riptopl.go:114-151`) |
| Asset pick | First asset matching `^RIPTOPL-v.*\.zip$` whose upper-cased name lacks `DEBUG`, `VARIANTS`, `LANGS`, `-RA.`, `-SRC` (`riptopl.go:41-54`). On `current-fan-favorite` both `…-3390-Korium.zip` and `…-3390.zip` match. The API lists Korium first, so the Korium theme package would be taken [D] |
| Verify | A digest is **required**: `parseDigest` rejects missing or malformed `digest`, so assets without one cannot be installed. Streams to a temp file while hashing; checks byte count = `size` and SHA-256 = digest (`riptopl.go:153-231`) |
| Cache | None. OS temp file, removed after staging (`prepare.go:313`). Every install downloads the 14–16 MB package again |
| Extract | Only `<FLAVOUR>/RIPTOPL.ELF`, by exact name at the **zip root**, in the order `APP_RIPTOPL-PS2DEVPINNED` > `-OFFICIALPINNED` > `-PS2DEVROLLING` > `-OFFICIALROLLING`. Size cap 64 MiB, zip-slip guard (`riptopl.go:244-313`). The test fixture uses the same root layout (`riptopl_test.go:20-42`) |
| Install path | `<root>/<BDM prefix>/APPS/APP_RIPTOPL-<FLAVOUR>/RIPTOPL.ELF`. **Only the ELF**: no `title.cfg`, icons, `ART/`, `POPS/`, `EMBER/`, `neutrino/`. The spec says `APPS/RIPTOPL/RIPTOPL.ELF` instead (spec §2.7 vs code) |
| Existing file | `CopyToDest` writes `.oplbm.*` in the target folder, `fsync`, then `os.Rename` over the target, which replaces it on Windows and POSIX. No check of what was there, no backup (`disk.go:152-213`). By contrast, art and cheats never overwrite a hand-made file (`internal/art/art.go:327-340`) |
| Plan → execute | The preview shows tag, asset, URL, size and digest. Execute calls `Resolve` again and installs whatever the tag holds then; the previewed digest is not passed through (`prepare.go:210-233` vs `300-333`). On a rolling tag the confirmed build and the installed build can differ [D] |
| Record | SQLite state key `loader.<dest path>` = JSON `{tag, asset, digest, flavour, elfPath, elfSize, stagedAt}` (`prepare.go:322-330`). Nothing is written on the device |
| Detect / update | None: no reading of an installed ELF, no comparison, no update offer |
| Settings, LNG, `title.cfg` | None written. Instead a first-boot checklist (`riptopl.go:326-333`): launch the ELF, enable devices in Game Sources, pick a start mode, save (which writes `settings_riptopl.cfg` and the folders), press L3 for PS1 |

**Current RiptOPL packages do not match Tiny's extractor** [V CI, D outcome]:
- Since `d91ffcae` (2026-09-20, "Fix release app layout…") the zip holds `APPS/APP_RIPTOPL/` and `APPS/APP_RIPTOPL-<FLAVOUR>/` (`rolling-release.yml:700-787`, zip command at `:984-988`).
- `package_release_app.py verify` rejects any `APP_RIPTOPL*/` at the root (`:285-290`).
- Before that commit the flavour folders sat at the root (`git show d91ffcae^:…rolling-release.yml:891-894`).
- Whether the `current-fan-favorite` zips, re-uploaded 2026-09-30, use the old or the new layout is [U].

## 2. OrbitPS2 Manager (`Luden02/OrbitPS2-Manager`)

All [V] at `cdd590a`.
- **No loader download, install or update.** No `OPNPS2LD`/`RIPTOPL.ELF` handling in `src/`. `api.github.com` appears only for its own update check (`src/services/update.service.ts`) and the art database (`artwork.service.ts:226`).
- **Self-update check**: `GET …/releases?per_page=10` with `User-Agent: OrbitPS2-Manager`. It compares semver core numbers, ignores pre-release suffixes `alpha|beta|rc|release-candidate|indev|nightly` and a bare `nightly` tag (`update.service.ts:20-60`). This is how a manager handles its own versioned releases; it does not fit rolling loader tags.
- **Device preparation**: `checkOplStructure` / `createOplFolders` over `APPS ART CD CFG DVD POPS VCD VMC`. It creates only folders the user picks from that list (`library.service.ts:26-79`). No `LNG`, `THM`, `CHT`.
- **App import** (any ELF): it refuses when `APPS/<sanitised title>/` already exists. Otherwise it copies the ELF and writes `title.cfg` as `title=<t>\nboot=<elf>\n` (LF) (`apps.service.ts:175-206`).
- It ships `POPSTARTER.ELF` in its own `assets/` and copies it into PS1 launcher folders (`ps1-import.service.ts:57-60`, `234-238`).
- Issues (53 issues and PRs, `gh api …/issues?state=all`): none about installing or updating OPL or RiptOPL. The related ones cover art and naming for RiptOPL (#39, #48), "RiptOPL Hardening" (PR #50) and "slow installation into HDD" (#54).

## 3. PS2 HDD Manager (`L10N37/PS2-HDD-Manager`)

All [V] at `1a30bc4`: `fetch_payloads.sh`, `PROVISIONING.md` [R], `src/ui/MainWindow.cpp`, `src/core/OplConfig.cpp`, `src/formatter_main.cpp`, `src/core/Ps2HddFormat.cpp`.

| Step | Behaviour |
|---|---|
| Sources | Upstream `ps2homebrew/Open-PS2-Loader` tag `latest`, exact asset `OPNPS2LD.ELF`. It deliberately avoids `/releases/latest` because that endpoint is stable-only [R `PROVISIONING.md`]. For >2 TiB disks: `L10N37/Open-PS2-Loader-Extended-APA` tag `v1.2.0-Beta-2273-Extended-APA-1`, asset `OPNPS2LD-Extended-APA-v1.2.0-Beta-2273.ELF`, **SHA-256 pinned in the script** (`aa7d6765…a244`) and checked after the digest check. A copy is also committed in the repo (`payload/banked-opl/`) |
| Lookup and verify | `curl` releases-by-tag; asset by exact name or prefix. The download goes to `<asset>.part`. **Size always**, `digest` when it starts with `sha256:`; then rename (`fetch_payloads.sh:52-118`) |
| Cache | `~/.cache/ps2-hdd-manager/downloads/<owner_repo>/<tag>/<asset>`. A cache hit needs the same size and digest as the current API entry, so a re-published rolling tag triggers a new download |
| Record | `payload/runtime/MANIFEST.txt` on the PC: `repo \| tag \| release \| published \| asset \| sha256` (append). Nothing on the HDD |
| Install | `hdd0:PP.FHDB.APPS:pfs:/OPL/OPNPS2LD.ELF` through `pfsshell put` (overwrite), plus a second copy `/OPL/IGR.ELF` with `exit_path=hdd0:PP.FHDB.APPS:pfs:/OPL/IGR.ELF`, and `__common/OPL/conf_hdd.cfg` with `hdd_partition=PP.FHDB.APPS` (`Ps2HddFormat.cpp:128,241-245`; `PROVISIONING.md`) |
| Update | "Install / Update OPL Apps…" (`MainWindow.cpp:5290-5330`): the OPL checkbox is enabled **only** when the OPL partition is its own `PP.FHDB.APPS`. Otherwise its tooltip says the manager will not guess where the booted OPL ELF lives. No version check: it always installs the current `latest` |
| Starter settings | "Apply … recommended OPL settings (replace conf_opl.cfg)", **pre-ticked**. It writes a whole new file: `hdd_mode=2`, `default_device=6`, `app_mode=2\|0`, `usb_mode=0`, `eth_mode=0`, `enable_coverart`, `enable_delete_rename`, `hdd_game_list_cache`, `autorefresh`, `autosort`, `remember_last=0`, `autostart_last=0`, `exit_path`, LF, with two `#` comment lines (`OplConfig.cpp:8-31`). A separate single-key path keeps every other line (`SetKey`, `formatter_main.cpp:2858-2905`). No warning that `mc?:OPL/conf_opl.cfg` would take precedence [V absence] |
| Version / title doc | `docs/INSTALLED_TITLE_AND_PC_GAMEID.md` covers HDL game titles, not loaders. Its relevant pattern: check the target and current state, write, re-read and verify, and stop a batch at the first failure [R] |
| Credits | `THIRD_PARTY_NOTICES.md` credits upstream OPL and the Extended APA fork on the PC side. No licence file is put on the HDD |

## 4. Launchers and their installers

### 4.1 FreeMcBoot-Installer (`israpps/FreeMcBoot-Installer`)

All [V] at `ac53a47`.
- **Existing files**:
  - `CopyFiles` opens every target with `O_WRONLY | O_CREAT | O_TRUNC` (`installer/system.c:1778-1830`), so it overwrites, with no backup.
  - Before copying, `CleanupTarget` deletes the four `B?EXEC-SYSTEM` folders (`system.c:753-767`).
  - The only existing-file prompt asks "Existing FMCB CNF found! Do you want to keep it?" (`lang.c:9-10`, `menu.c:474-481`). "Yes" skips copying `FREEMCB.CNF`.
- **Already-installed / multi-install**:
  - `HasOldMultiInstall` looks for `mc?:SYS-CONF/FMCBUINST.dat`, then the legacy `uninstall.dat` (`system.c:1922-1945`).
  - A normal install or cleanup is refused while a multi-install exists ("Multi-installation detected! Please uninstall first.", `system.c:2350-2357`).
  - Multi-install writes that uninstall file (`GenerateUninstallFile`, `system.c:2289`). The file is versioned (`FMCBUninstallFileVersion`, `system.c:2114`).
  - No detection of the installed FMCB version; only the installer's own version is shown (`menu.c:371-373`).
  - Full card dump and restore are separate menu items (`lang.c`, "will be dumped" / "restored").
- **The OPL it installs**: `installer_res/<1953|1963|1964|1965|1966>/INSTALL/APPS/OPNPS2LD.ELF`, all 1,175,272 B. SHA-256 `4b7a81900e7521588ec428f8093d7fc1eea3ae7c1653c71d082610aa7356a0d1` = **OPL v1.0.0** (`data/loader-hashes/opl-stable.txt`). It is copied with `APPS.icn` and `icon.sys` to `mc?:/APPS/`.
- **Stock OPL entries** (refreshes research/07 §2.2):

  | `FREEMCB.CNF` | `path1/2/3_OSDSYS_ITEM_3` ("Open PS2 Loader") | `LK_L1_E1..E3` |
  |---|---|---|
  | 1.953 | `mass:/UPDT/OPL.ELF`, `mc?:/APPS/OPNPS2LD.ELF`, `mc?:/OPL/OPNPS2LD.ELF` | `mass:/OPL.ELF`, `mc?:/OPL/OPNPS2LD.ELF`, `mc?:/APPS/OPNPS2LD.ELF` |
  | 1.963, 1.964, 1.966 | same as 1.953 | `mass:/BOOT/HDLOADER.ELF`, `mc?:/BOOT/HDLOADER.ELF`, `mc?:/B?DATA-SYSTEM/HDLOADER.ELF` |
  | 1.965 | same as 1.953 | same as 1.953 |
  | `MASS/FREEMCB.CNF` (FMCB on USB) | item 4: `mass:/APPS/OPNPS2LD_LATEST.ELF`, `hdd0:+OPL:pfs:/APPS/OPL/OPNPS2LD_LATEST.ELF`; items 5–15: `mass:/APPS/OPL<ver>/OPL<ver>.ELF` (110 … 05) | `mass:/APPS/OPL.ELF`, `mc?:/APPS/OPNPS2LD.ELF`, `hdd0:+OPL:pfs:/APPS/OPL/OPNPS2LD.ELF` |

  research/07 §2.9 gave 1.953 only the L1 path; it has item 3 too. **`mass:/UPDT/OPL.ELF` is path1 of the OPL item in all five stock configs.**

### 4.2 PS2BBL (`israpps/PlayStation2-Basic-BootLoader`)

All [V] at `d2e81fa`.
- **No installer in this repo.**
  - The CI builds `PS2BBL_OpenTuna_Installer.ELF` from `israpps/Funtuna-Fork` (branch `PS2BBL`). It copies `COMPRESSED_PS2BBL.ELF` → `BOOT/BOOT.ELF` and `PS2BBL.INI` → `SYS-CONF/` (`.github/workflows/CI.yml:140-170`).
  - How that installer treats existing files is [U] (not cloned). KELFBinder, the other route, is [U] too.
- **Stock INI** (`release/SYS-CONF/PS2BBL.INI`, one file for all build variants): `LK_AUTO_E1..E3 = mass:/APPS/OPNPS2LD.ELF, mc?:/APPS/OPNPS2LD.ELF, mc?:/OPL/OPNPS2LD.ELF` (unchanged since research/07).
- **Built-in fallback when no config is found** (`include/common.h:126-142`):
  - AUTO: `mc?:/BOOT/ULE.ELF`, `mc?:/APPS/ULE/ELF`, `mass:/BOOT/BOOT.ELF`;
  - **L1: `mc?:/OPL/OPNPS2LD.ELF`, `mc?:/APPS/OPNPS2LD/ELF` (a typo for `.ELF`), `mass:/PS2BBL/OPNPS2LD.ELF`**;
  - R1: `mass:/RESCUE.ELF`, …
- **MX4SIO build**:
  - `massX:` is replaced by the first `mass0..4:` whose BDM driver name is `sdc` (`src/main.c:480-485`, `559-578`);
  - config `massX:/PS2BBL/CONFIG.INI` (`include/common.h:33-34`);
  - plain `mass:` paths in the stock INI go to whatever unit is `mass0` [D];
  - MX4SIO and MMCE builds are mutually exclusive (`Makefile:85-105`).
- The release 7z also holds `LICENSE.TXT` and `README.md` (`CI.yml:80-81`).

### 4.3 OSDMenu (`pcm720/OSDMenu`)

All [V] at `a2b847a`.
- `examples/OSDMENU.CNF` still has **no OPL entry**: items are "Launch Disc" (`cdrom`), "wLaunchELF" (`mc?:/BOOT/BOOT.ELF`, `mmce?:/apps/wLaunchELF.elf`, `usb:/apps/wLaunchELF.elf`) and Shutdown.
- `examples/OSDMBR.CNF` boots NHDDL: `hdd0:__common:pfs:/Apps/APP_NHDDL/nhddl.elf`, `mc?:/APP_NHDDL/nhddl.elf`.
- The MBR installer copies its config to `hdd0:__sysconf/osdmenu/OSDMBR.CNF` **only if the file does not exist** (`utils/installer/src/main.c:172-199`; README).

## 5. RiptOPL release process

### 5.1 C edition (`NathanNeurotic/Open-PS2-Loader`, `rebuild/main` @`55261de1`)

All [V] unless tagged: `.github/workflows/rolling-release.yml`, `release-normalize.yml`, `.github/scripts/package_release_app.py`, `asset_version.sh`.

**Flavours.** The same commit is built with four SDK containers:
- `PS2DEVPINNED` (ps2dev by digest);
- `OFFICIALPINNED` (ps2homebrew by digest);
- `PS2DEVROLLING` (required; its failure fails the run);
- `OFFICIALROLLING`.

**One-click `RIPTOPL.ELF`.** It is always the `OFFICIALROLLING` ELF, by the owner's decision of 2026-09-10 (`:636-680`). Its name and URL never change. When that flavour fails to build, the release ships without `RIPTOPL.ELF`.

**Asset names.**
- `ASSET_VERSION` is the build version without its trailing 7-hex hash, e.g. `v1.2.0-Beta-4257` (`asset_version.sh`).
- Zips: `RIPTOPL-<ASSET_VERSION>.zip`, `-src.zip`, `RIPTOPL-LANGS-…zip`, `RIPTOPL-VARIANTS-…zip`, `RIPTOPL-DEBUG-…zip`. Korium: `…-Korium` suffix, separate release `rolling-korium`.
- `APP_RIPTOPL.psu`.

**Package zip** (built at `:690-988`):

```
APPS/APP_RIPTOPL/                  RIPTOPL.ELF (OFFICIALROLLING, else first of PS2DEVPINNED, OFFICIALPINNED, PS2DEVROLLING),
                                   title.cfg, APPINFO.PBT, bgm.ogg, icon.sys, list.icn, copy.icn, del.icn
APPS/APP_RIPTOPL-<FLAVOUR>/        the same set per flavour (4 folders, each best-effort except PS2DEVROLLING)
ART/RIPTOPL.ELF_<BG|COV|COV2|ICO|LAB|LGO|SCR|SCR2>.png
APP_RIPTOPL.psu                    the app folder as a memory-card save
POPS/                              POPSTARTER.ELF, KELF, TROJAN_7.BIN, IGR TM2s, CHEATS.TXT, BDMA pairs
                                   usbd/usbhdfsd.irx.<usbexfat|mx4sio|mmce|ata|ilink|udpfs>, IPCONFIG.DAT,
                                   UDPFSCONFIG.DAT, "POPSTARTER VERSIONS/<MAIN|DEBUG|USBDELAY…>/POPSTARTER.ELF"
EMBER/                             ember.elf, LICENSE-BETA.txt (hard requirement), "Ember Official Webpage.url",
                                   games/Per-Game-Folder/GAME FILES; never bios.bin
neutrino/                          latest rickgaiser/neutrino pre-release, extracted, + config/bsd-udpfsbd.toml,
                                   RiptOPL's IEEE1394_bd_mini.irx and iLinkman.irx
PS2-Servers.url, android-udpfs-server.url, OrbitPS2-Manager.url, OPL-PS1-AIO-Converter-GUI.url, PS2RD-CHT-Manager.url
```

The package carries **no `LICENSE`/`CREDITS` of RiptOPL itself**; the zip command lists only the entries above.

**`title.cfg` as staged** (`package_release_app.py:113-126, 228-236`): it is the `Example/…/title.cfg` template with these fields replaced:
- `title=[APP] Open PS2 Loader: RiptOPL`;
- `boot=RIPTOPL.ELF`;
- `Title=<title> <ASSET_VERSION> <FLAVOUR>`;
- **`Version=<ASSET_VERSION> <FLAVOUR>`**, e.g. `Version=v1.2.0-Beta-4257 OFFICIALROLLING`;
- `Release=Rolling|Tagged`;
- `Description=Open PS2 Loader for PS2 and PS3`;
- `Notes=Companion folders are supplied in the release ZIP`.

Other keys stay from the template (`CfgVersion=8`, `$ConfigSource=1`, `Developer=`, `Source=`…). The line endings are LF. The template's own `Version=RiptOpen PS2 Loader v1.2.0 OFFICIAL ROLLING`, quoted in research/07 §1.3, is not what a release carries.

**Checksums.**
- CI writes `SHA256SUMS.txt` (sha256sum format) over `*.ELF *.psu *-src.zip` (`:1025-1037`).
- `release-normalize.yml` then **deletes** `SHA256SUMS.txt`, `*MANIFEST*.txt`, `DETAILED_CHANGELOG` and every versioned ELF from the public release. It keeps only `RIPTOPL.ELF` and `RIPTOPL-RA.ELF` (`:153-172`, verified at `:187-215`).
- So `rolling` and `rolling-korium` carry no checksum file, only GitHub `digest`s. `rolling-ra` keeps `SHA256SUMS.txt` (808 B) and `RA-RELEASE.json`.

**Publishing.** The release is edited in place: upload first, then delete stale assets (`:1071-1130`). The release id stays the same (`rolling` = 388169586). `target_commitish` is not refreshed:
- `rolling` reports `05297663` (a 2026-09-13 merge);
- `git/ref/tags/rolling` → `55261de1`, the built commit;
- the body header names `55261de1` and `v1.2.0-Beta-4257-55261de`.

**Release body** (about 5.5 KB):
- a bold title with the build type;
- a commit link and the full version;
- `<details>` "Changelog" with the last 10 commits;
- `<details>` "Downloads", one line per asset: the full package for a first install, `RIPTOPL.ELF` as the update that replaces the existing ELF, PSU, DEBUG, LANGS…

**Current assets** (`gh api`, 2026-10-25; digests shortened, they change several times a day):

| Release | Asset | Size | `digest` (prefix) | `updated_at` |
|---|---|---|---|---|
| `rolling` (pub 2026-10-24T01:22:39Z) | `RIPTOPL.ELF` | 1,560,916 | `639868819b55` | 2026-10-24T01:22:30Z |
| | `RIPTOPL-v1.2.0-Beta-4257.zip` | 14,609,991 | `d09458db626d` | 01:23:19Z |
| | `APP_RIPTOPL.psu` | 1,980,928 | `cade2f39250f` | 01:22:26Z |
| | `RIPTOPL-LANGS-v1.2.0-Beta-4257.zip` | 6,987,322 | `4eaa6bb4d1c3` | 01:22:27Z |
| | `-VARIANTS-` 51,243,641 / `-DEBUG-` 68,803,743 / `-src` 23,145,965 | | | |
| `rolling-korium` (pub 01:27:12Z) | `RIPTOPL.ELF` 1,828,244; `RIPTOPL-v1.2.0-Beta-4257-Korium.zip` 16,211,811; same set with `-Korium` | | | |
| `rolling-ra` (pre-release) | `RIPTOPL-RA-{inceBtion,Korium}.ELF`, `APP_RIPTOPL-RA-*.psu`, `RIPTOPL-RetroAchievements-*-v1.2.0-Beta-4257.zip`, `RA-RELEASE.json`, `SHA256SUMS.txt` | | | 2026-10-24T01:28Z |
| `current-fan-favorite` (pub **2026-09-04**) | `RIPTOPL-v1.2.0-Beta-3390[-Korium].zip` and LANGS, VARIANTS, DEBUG, RA zips; **no loose ELF, no PSU** | | | **2026-09-30** |

`immutable: false` on all of them.

### 5.2 C++ edition (`NathanNeurotic/RiptOPL` @`6edea174`)

[V `tools/build_edition.py:19-23, 90-125`, `tools/package_edition.py:20-100`, gh api]
- **Releases**: `rolling-korium`, `rolling-incebtion`, `rolling-ra`, all non-pre-release.
  - Asset names carry **no version**: `RIPTOPL.ELF` (RA: `RIPTOPL-RA.ELF`), `APP_RIPTOPL.psu`, `RIPTOPL-<Korium|IncebtIon|RetroAchievements>.zip`, `RIPTOPL-src.zip`, `BUILD-MANIFEST.json`, `SHA256SUMS.txt`.
  - Published 2026-10-16, assets replaced 2026-10-20.
- **`SHA256SUMS.txt`**: `<hex>  <name>` lines for every other file in the output folder, sorted. That is 5 files, `BUILD-MANIFEST.json` included (413 B = 5 × 67 + name lengths).
- **`BUILD-MANIFEST.json` fields**:
  - `repository`, `source_commit`, `dirty`, `edition`;
  - the edition options `title`, `tag`, `theme`, `ra`;
  - `sdk` (`ps2build`), `compiler`, `configuration`, `ps2build_version`, `components`, `definition_sha256`;
  - `elf_sha256`, `version` (the `asm/opl_version.h` line), `run_url`;
  - added by packaging: `neutrino_archive {filename, sha256}` and `kit_files {path: sha256}` for every file in the kit.
- **Kit** = the C layout plus `LICENSE`, `CREDITS`, `README.md` at the root. `title.cfg` uses `title=RiptOPL <Edition>`, flavour `PS2BUILD`.
- The packager refuses dirty trees, LLVM builds and an ELF that no longer matches the manifest.

### 5.3 What the install docs tell users (docs site @`20d2113`)

All [R]: `install.html`, `releases.html`, `languages.html`.
- **First install**:
  - download the package;
  - copy `APPS/APP_RIPTOPL/` (or import the PSU), **or** take `APPS/APP_RIPTOPL-PS2DEVPINNED/RIPTOPL.ELF`, "#1 recommended";
  - point the launcher at it; FMCB conventions are `mass:/APPS/RIPTOPL/RIPTOPL.ELF` or `mc0:/RIPTOPL/RIPTOPL.ELF`, and the ELF file name does not matter;
  - `neutrino/` goes to `mc?:/neutrino/` or, for MMCE GameID setups, `mmceN:/neutrino/`.
- **Update**:
  - skip the package; download the loose `RIPTOPL.ELF` (OFFICIALROLLING) and overwrite the existing ELF. `POPS/`, `EMBER/`, `neutrino/` and settings stay;
  - use the full package for a pinned flavour, missing companions, or announced core or module updates. Replacing only the ELF does not update Neutrino or Ember;
  - back up configuration and save cards first, and do not overwrite them with package defaults.
- **Settings**:
  - `settings_riptopl.cfg` lives in the launch folder;
  - `conf_opl.cfg` is read once as a seed when no RiptOPL settings exist, so swapping `OPNPS2LD.ELF` for `RIPTOPL.ELF` keeps the user's devices and options;
  - a fresh install starts with every device Off.
- **Languages**: `RIPTOPL-LANGS-*.zip` has versioned per-language subfolders. Copy the files of one subfolder next to the ELF, into `LNG/` next to it, or into the legacy device `LNG/`. The lookup order is CWD → CWD/LNG → memory-card OPL folder and device `LNG/`.
- **Finding the build**:
  - zip names omit the hash and the SDK; the full id (`…-<hash>-<FLAVOUR>`) is in the release notes and the loader's About;
  - Beta = commit count + 2;
  - the docs admit that one Beta-4224 kit's PS2DEVPINNED loader reported "Beta-3" because of a shallow checkout (`releases.html`, "Finding the exact build").
- **Pulling**: `gh release download rolling …`. Every prior run's assets are deleted before the new set is uploaded.

## 6. YAOPL release process (`belomaxorka/yaopl`)

All [V]: working tree `33a2b19a` = remote `master`; `FORK.md` blob `d61999c7` is identical locally and remotely (`gh api …/contents/FORK.md`).
- **CI** (`.github/workflows/compilation.yml:181-241`):
  - job `release` runs on `master` (and on `v*` tags, upstream's path);
  - a fork-only step publishes only if `git ls-remote origin refs/heads/master` still equals `GITHUB_SHA`;
  - it then runs `sha256sum OPNPS2LD.ELF OPNPS2LD-<ver>.7z OPNPS2LD-VARIANTS-<ver>.7z OPNPS2LD-LANGS-<ver>.7z > SHA256SUMS`;
  - `gh release delete latest --cleanup-tag`, then `git push --delete origin latest`;
  - `gh release create latest <files> SHA256SUMS --prerelease --target $GITHUB_SHA --title <OPL_VERSION> --notes-file notes.md`.
- **Assets** (2026-10-23T07:19:25Z, all `digest` set, `immutable: false`):

  | Asset | Size | `digest` |
  |---|---|---|
  | `OPNPS2LD.ELF` | 1,377,492 | `sha256:aabb85cc5b3f2194ccb3f7e0488b48a49fcd5197be8e5936d7e3e27b9489e4fa` |
  | `OPNPS2LD-v1.2.0-Beta-2370-33a2b19-yaopl.7z` | 1,425,784 | `sha256:44a17a6f8104094d37ae24746e02f3715ad08ffc470b4a0ea3ea3eabce1e812b` |
  | `OPNPS2LD-VARIANTS-v1.2.0-Beta-2370-33a2b19-yaopl.7z` | 4,657,771 | `sha256:212a39bd977d4edf79a091f84edfdf7188cb1af690a842e5a3cd04c8afd9435e` |
  | `OPNPS2LD-LANGS-v1.2.0-Beta-2370-33a2b19-yaopl.7z` | 3,411,527 | `sha256:11931222d672a617aa8d5af30d4e5d856851260884e989f897eea4c590744955` |
  | `SHA256SUMS` | 421 | `sha256:2ce2564f1d59d44dbbb5457c09eb012d5c2b8d1753b7ddf39562c0ae359e8f0c` |

- **`SHA256SUMS` format**:
  - `sha256sum` text mode, `<64 hex><2 spaces><name>\n`, four lines for the four files above, itself excluded;
  - 421 B = (64 + 2 + 1) × 4 + 12 + 42 + 51 + 48 [V arithmetic];
  - `FORK.md` tells users to run `sha256sum -c SHA256SUMS` or `Get-FileHash`.
- **Not there**:
  - un-versioned `OPNPS2LD.7z` copies (upstream has them);
  - `HASHES.txt`;
  - versioned or immutable releases: the only release is `latest`, deleted and recreated on every push, so the release id and tag move each time.
  - `FORK.md`: "Only the build of the current tip of `master` is published."
- **Release body, verbatim**:

  ```
  Automatic YAOPL (Yet Another Open PS2 Loader) build of v1.2.0-Beta-2370-33a2b19-yaopl from 33a2b19a8afa37b50015eeb1414f575b2270ac7d.

  Not an official OPL build: see FORK.md for what differs from upstream OPL.
  SHA-256 checksums of the files are in SHA256SUMS.
  ```

  The release `name` is the full version string; the tag ref = `33a2b19a`.
- **Archive contents**: the CI copies `INFO/` (`DETAILED_CHANGELOG`, `CREDITS`, `LICENSE`, `README.md`) into `OPNPS2LD/` and `OPNPS2LD-VARIANTS/` before packing (`:205-216`, artifact `INFO` at `:53-62`).

## 7. Upstream OPL release facts (`ps2homebrew/Open-PS2-Loader`)

### 7.1 `latest`

[V gh api `releases/tags/latest`]
- id 411255901; `prerelease: true`; `name: latest`; created 2026-10-13T04:04:53Z, **published 2026-10-13T04:07:00Z**;
- `target_commitish: master`; `git/ref/tags/latest` → `b3a21997`; `immutable: false`; author `github-actions[bot]`.

| Asset | Size | `digest` | `content_type` | `updated_at` |
|---|---|---|---|---|
| `OPNPS2LD.ELF` | 1,361,812 | `sha256:4b6d36eff917bbeb3d860fea7ca0b3dd38502c4f5b008803a1edf3f0edf12290` | application/octet-stream | 2026-10-13T04:07:08Z |
| `OPNPS2LD.7z` | 1,406,597 | `sha256:5785f268cdc165a625856f9a1a750eb69a969bb422b044deb33257738d7c9266` | application/octet-stream | 04:07:05Z |
| `OPNPS2LD-v1.2.0-Beta-2271-b3a2199.7z` | 1,406,597 | same as `OPNPS2LD.7z` | application/octet-stream | 04:07:02Z |
| `OPNPS2LD-VARIANTS.7z` | 4,617,101 | `sha256:edf1ee00289b09a297d0ada26829cdc26caff51b6e0ba3dae1be652a8d8bd47d` | application/octet-stream | 04:07:06Z |
| `OPNPS2LD-VARIANTS-v1.2.0-Beta-2271-b3a2199.7z` | 4,617,101 | same as `OPNPS2LD-VARIANTS.7z` | application/octet-stream | 04:07:03Z |
| `OPNPS2LD-LANGS.7z` | 3,404,283 | `sha256:ad83741ae803c1eb79d545e3684425074f28b8c6c1f6625ebc83cab447905ac8` | application/octet-stream | 04:07:07Z |
| `OPNPS2LD-LANGS-v1.2.0-Beta-2271-b3a2199.7z` | 3,404,283 | same as `OPNPS2LD-LANGS.7z` | application/octet-stream | 04:07:04Z |

**Body**: 570 characters, 5 lines. A `## Documentation` heading and four commit bullets (author link, commit link), the last being `b3a2199` "Add note about different proejcts". **No version string anywhere in the release metadata except the asset names.**

### 7.2 `v1.1.0`

[V gh api `releases/tags/v1.1.0`]
- id 49303113; not a pre-release; created 2021-09-09T18:59:41Z, published 19:08:06Z; `target_commitish: master`.

| Asset | Size | `digest` | `content_type` | `updated_at` |
|---|---|---|---|---|
| `OPNPS2LD-v1.1.0.7z` | 1,238,940 | null | application/octet-stream | 2021-09-09T19:08:07Z |
| `OPNPS2LD.7z` | 1,238,940 | null | application/octet-stream | 2021-09-09T19:08:08Z |
| `OPNPS2LD-VARIANTS-v1.1.0.7z` | 1,473,070 | null | application/octet-stream | 2021-09-09T19:08:08Z |
| `OPNPS2LD-VARIANTS.7z` | 1,473,070 | null | application/octet-stream | 2021-09-09T19:08:09Z |
| `OPNPS2LD-LANGUAGES-AND-FONTS-v1.1.0.7z` | 2,511,479 | null | application/octet-stream | **2021-09-10T17:34:26Z** (added a day later) |

**Body**: 19,767 characters, 153 lines. It opens with "New in OPL v1.1.0": GCC 11 toolchain, NBD server, a new USB mass-storage driver, experimental iLink and MX4SIO, HDD settings partition, OPL Launcher for HDD-OSD, reorganised GUI settings. Then "Known regressions since v1.0.0" (first item: movie playback hang on HDD, #466) and the commit list.

### 7.3 Upstream CI packaging

[V `.github/workflows/compilation.yml` @`b3a21997`, `lng_pack.sh`]
- **Licence files**:
  - the `build` job uploads artifact `INFO` = `DETAILED_CHANGELOG`, `CREDITS`, `LICENSE`, `README.md` (`:36-45`);
  - `release` copies `INFO/*` into `OPNPS2LD/` (the main ELF `OPNPS2LD-<ver>.ELF`) and `OPNPS2LD-VARIANTS/` (`:187-188`);
  - so **`OPNPS2LD.7z` and `OPNPS2LD-VARIANTS.7z` hold `LICENSE`/`CREDITS`/`README.md`/`DETAILED_CHANGELOG`; the bare `OPNPS2LD.ELF` and the LANGS archive do not.**
- **Archive layout** (closed by listing the downloaded release files on 2026-10-25, with the owner's go-ahead; nothing from them is in the repository) [V]:
  - `OPNPS2LD.7z` of `latest` holds `OPNPS2LD\OPNPS2LD-v1.2.0-Beta-2271-b3a2199.ELF` (1,361,812 B, SHA-256 equal to the bare `OPNPS2LD.ELF` asset) plus `OPNPS2LD\CREDITS`, `DETAILED_CHANGELOG`, `LICENSE`, `README.md`. So members **do** carry the `OPNPS2LD/` folder; research/07 §1.2 ("flat") is wrong.
  - `OPNPS2LD-v1.1.0.7z` (1,238,940 B, SHA-256 `869b51f2c9c442c79f9545a57d65ed1a28a47b19fb82ac7d307ad6cb30ddd4ed`) has the same layout; its `OPNPS2LD\OPNPS2LD-v1.1.0.ELF` (1,206,808 B) hashes to `dd6036a0…9520`, the `v1.1.0` line of `data/loader-hashes/opl-stable.txt`.
  - `OPNPS2LD-LANGS.7z` of `latest` holds one member, `OPNPS2LD-LANGS\OPNPS2LD-LANGS-v1.2.0-Beta-2271-b3a2199.zip` (6,382,114 B). The zip has `README` and one folder per language, `<Lang>-<version>\lang_<Lang>.lng` plus `font_<Lang>.ttf` or `.otf` (Russian: `lang_Russian.lng` 12,056 B, `font_Russian.ttf` 29,764 B). YAOPL's `OPNPS2LD-LANGS-<version>.7z` is built the same way.
  - `OPNPS2LD-LANGUAGES-AND-FONTS-v1.1.0.7z` (2,511,479 B, SHA-256 `10d93f39353a870103bcd9b94b226ff51cf0cf540a16d279bcee8ee342a8af68`) is **not** nested: `OPNPS2LD-LANGUAGES-AND-FONTS-v1.1.0\<Lang>\lang_<Lang>.lng` and fonts, plus `Readme.txt`, and two English folders (`English (template)`, `English (for custom font users)`).
  - So an installer finds languages by the file names `lang_<Lang>.lng` and `font_<Lang>.ttf|otf` at any depth, one nested zip deep, rather than by fixed member paths.
- **Languages**:
  - `lng_pack.sh` builds `OPNPS2LD-LANGS-<ver>.zip` with one folder per language, `<Lang>-<OPL_VERSION>/lang_<Lang>.lng`, plus `font_<Lang>.ttf|otf` when `lng_src/thirdparty/` has one, and a `README` (`lng_pack.sh:29-75`);
  - the CI then wraps that zip in `OPNPS2LD-LANGS-<ver>.7z` (`:103-114`, `:191`), so it is a **zip inside a 7z**;
  - the README tells users to put the `.lng` and font file "into your OPL directory of your memory card", not to rename them, then pick the language in Display settings and save.
- **Checksums**: **no `SHA256SUMS`**; only GitHub `digest`. The un-versioned names are byte copies (`cp -f`, `:192-195`), so their digests are equal.
- The release step is `marvinpinto-action-automatic-releases` with `automatic_release_tag: latest`, which recreates the release (new id each build) [V action use; recreation D from the id and `created_at`].

## 8. Other managers and scripts (sweep)

`rg -il "OPNPS2LD|releases/latest|api\.github\.com"` over every clone (112 files in 25 repos). Loader sources and their CIs aside, the hits are:

| Repo | Loader download or install? | How |
|---|---|---|
| `GDX-X/PFS-BatchKit-Manager` (`eb77f51`) | **Yes**, OPL for HDD | `wget` of `…/releases/download/latest/OPNPS2LD.7z`, no checksum check. Extracts with 7-Zip and moves `TMP\OPNPS2LD\*` into `APPS\Open PS2 Loader Latest\`; if no ELF results, it falls back to a bundled "Open PS2 Loader Stable" from `APPS.zip`. It renames the ELF to `OPNPS2LD.ELF`. On HDD it writes `rm OPNPS2LD.ELF` then `put OPNPS2LD.ELF` through pfsshell: delete first, no backup (`BAT/APPS.BAT:653-695`, `!PFS-BatchKit-Manager.bat:5057-5063, 8043-8049, 10132-10147`) [V] |
| `CosmicScale/PSBBN-Definitive-Project` (`6c7c122`) | **Yes**, bundled | `scripts/assets/OPL/OPNPS2LD.ELF` (10,361,324 B, unpacked; `version.txt` = `OPNPS2LD-v1.2.0-Beta-2245-3e3f34e-PADEMU=1-EXTRA_FEATURES=0`), `assets/NHDDL/nhddl.elf`. `update_apps` copies to `__system/launcher/` when the MD5s differ, so the bundled build always wins and can downgrade. Neutrino compares `version.txt` with `sort -V`. It copies 8 bundled languages and fonts to `<OPL>/LNG/` and merges `conf_opl.cfg` (deletes its 7 keys, appends `enable_coverart=1 default_device=0 usb_mode=2 app_mode=2 enable_bdm_hdd=1 language_text=… swap_select_btn=…`) (`scripts/Game-Installer.sh:882-930, 3961-3983, 4062-4064`) [V] |
| `lucasonline0/opl-linux-toolbox` | No (self-update only) | `releases/latest`, verifies its own package against the release's `SHA256SUMS` (`src/services/linux-updater.service.ts:15, 101-121`) [V] |
| `Luden02/OrbitPS2-Manager` | No | §2 |
| `brainstream/OPL-PC-Tools` (`2291b5a`), `logi-26/ps2-game-manager` (`75291cc`) | No (self-update only) | `src/OplPcTools/Updater.cpp`; `api/app/github.py` with a cache and a fixture dir for tests [V] |
| `ShaunCarrPHL/PS2FASTLOADER` (`c47be7b`) | No (OPL **themes**) | GitHub repository search, then `releases/latest`, picks a zip asset (`desktop/src-tauri/src/themes.rs:537-545`) [V] |
| `Informaldehyde-xd/PS2-MANAGER-FOR-ANDROID-`, `WeeXnes/OPLNova`, `edisnord/PyOPLM`, `gpulch/ps2-manager` | No | Only art-database or README hits [V] |
| `JO0812/TinyPS2Manager`, `L10N37/PS2-HDD-Manager` | Yes | §1, §3 |

---

## 9. Evidence for the product decisions

Statements of what the evidence suggests; the decisions are the owner's.

### (a) Default boot-method preset on a USB stick

| Evidence | Tag |
|---|---|
| `mass:/UPDT/OPL.ELF` is path1 of the stock OPL item in **all five** FMCB installer configs (1.953, 1.963–1.966). `mass:/OPL.ELF` (`LK_L1_E1`) exists only in 1.953 and 1.965 | [V] §4.1 |
| The FMCB installer puts **OPL 1.0.0** on the card at `mc?:/APPS/OPNPS2LD.ELF`. A stick with `UPDT/OPL.ELF` overrides it whenever the stick is inserted, without touching the card | [V] hash; override D |
| PS2BBL stock INI: `LK_AUTO_E1 = mass:/APPS/OPNPS2LD.ELF`, started with no key press. Without an INI, L1 tries `mass:/PS2BBL/OPNPS2LD.ELF` | [V] §4.2 |
| FMCB-on-USB package: `mass:/APPS/OPL.ELF`, `mass:/APPS/OPNPS2LD_LATEST.ELF` | [V] |
| RiptOPL docs: `mass:/APPS/RIPTOPL/RIPTOPL.ELF` by convention; the package app `APPS/APP_RIPTOPL/` | [R] |
| Managers: TinyPS2Manager `APPS/APP_RIPTOPL-<FLAVOUR>/RIPTOPL.ELF` (no `title.cfg`); BatchKit `APPS/Open PS2 Loader Latest/` (app style). Nobody targets `UPDT/` | [V] |
| FMCB's stock `mass:` driver is FAT32-only | research/07 §2.1 [V configs; U] |

Suggests:
- `UPDT/OPL.ELF` is the one USB path that every stock FMCB 1.95x/1.96x reaches. It fits the owner's FMCB setup and replaces the 1.0.0 that the FMCB installer leaves on the card.
- `APPS/OPNPS2LD.ELF` is the PS2BBL equivalent.
- Neither path is listed on OPL's own Apps page (no `title.cfg`), and no other tool uses `UPDT/`, so the plan should say why that folder exists.

### (b) Language packs (`LNG/`) and fonts

| Evidence | Tag |
|---|---|
| Upstream LANGS is a zip inside a 7z, with versioned per-language subfolders. Its README says to copy into the memory card's OPL folder | [V] §7.3 |
| OPL reads `mc?:OPL/*.lng` at start (`src/lang.c:170-175`) and `<prefix>LNG/*.lng` of each BDM device only when that device is enabled and scanned (`src/bdmsupport.c:244-248`). Fonts are `font_<Name>.ttf\|otf` in the same folder (`lang.c:36-50`). Files in subfolders are skipped | [V]; research/16 §1.6 |
| The chosen language is saved as `language_text` in `conf_opl.cfg` (`lang.c:153-167`) | [V] |
| PSBBN copies 8 languages and fonts to the OPL partition `LNG/` and seeds `language_text` | [V] §8 |
| RiptOPL: CWD → CWD/LNG → legacy `LNG/`; LANGS per release (7.0 MB) | [R] §5.3 |
| TinyPS2Manager, OrbitPS2, PS2 HDD Manager, BatchKit: no LNG install | [V absence] |

Suggests:
- Optional, off by default, the user picking languages.
- Flatten the folders into `LNG/` (or beside the ELF for RiptOPL).
- Warn that on USB the language appears only after the device is enabled, unless `language_text` and device keys come from the same seeded config.
- A Russian UI user is the obvious case: `lang_Russian.lng` + `font_Russian.ttf` exist (PSBBN bundles both) [V file list].

### (c) Starter settings (`conf_opl.cfg` on the device)

| Evidence | Tag |
|---|---|
| PS2 HDD Manager writes a **replacing** preset on the APA OPL partition, pre-ticked, LF, two `#` lines; single-key merge elsewhere. No check of the memory card | [V] §3 |
| PSBBN merges by deleting its keys and appending (LF), on the HDD OPL partition | [V] §8 |
| OSDMenu MBR installer writes its config only if absent | [V] §4.3 |
| TinyPS2Manager seeds nothing and shows a first-boot checklist | [V] §1 |
| RiptOPL docs: back up settings; do not overwrite them with package defaults. RiptOPL reads `conf_opl.cfg` once as a seed | [R] §5.3 |
| OPL reads `mc?:OPL/conf_opl.cfg` before any device copy, and forces `usb_mode=2` when the config came from BDM | research/07 §3.3 [V] |
| OPL keeps unknown keys and writes CRLF; `configRead` skips lines without `=`, so `#` comments with no `=` are dropped [D from `src/config.c:468-500`] | [V]/[D] |

What breaks [D]:
- A replacing preset loses the user's keys (language, theme, network), so merge, never replace.
- A device copy is silently ignored on FMCB setups whose card already has `mc0:/OPL/conf_opl.cfg`.
- Line endings matter only for byte-equality with OPL's own saves.

Suggests: write only when absent, or merge keys and keep the rest. Before offering it, check whether the profile reads settings from the device at all (RiptOPL beside the ELF: yes; upstream on an FMCB setup: probably not).

### (d) MX4SIO installs

| Evidence | Tag |
|---|---|
| No tool in this set installs to MX4SIO or has an MX4SIO preset | [V absence] |
| PS2BBL MX4SIO build: `massX:` resolves to the BDM unit with driver `sdc`; config `massX:/PS2BBL/CONFIG.INI`; the stock INI's plain `mass:` paths reach MX4SIO only if it is `mass0` | [V]/[D] §4.2 |
| OSDMenu understands `mx4sio:` | research/07 §2.1 [V] |
| FMCB cannot read MX4SIO | research/07 [U] |
| OPL's Apps page lists `<usb_prefix>/APPS/<app>/` with `title.cfg` on any enabled BDM unit, MX4SIO included | [D] research/02 §11, research/07 §3.2 |

Suggests:
- There is no stock path. The fitting options are app style (`APPS/<app>/` + `title.cfg`) plus a BOOT-1 line for PS2BBL-MX4SIO (`massX:/APPS/<app>/<ELF>`) or OSDMenu (`mx4sio:/…`).
- `APPS/OPNPS2LD.ELF` works with the stock PS2BBL INI only by unit order [U]. Console test needed.

### (e) A foreign ELF already sits at the install path

| Evidence | Tag |
|---|---|
| Overwrite without backup: TinyPS2Manager (rename over), PS2 HDD Manager (`put`), BatchKit (`rm` + `put`), PSBBN (MD5 differs → copy; can downgrade), FMCB installer (`O_TRUNC`, writes OPL 1.0.0 over whatever was there) | [V] |
| Refuse by ownership: PS2 HDD Manager updates OPL only in its own `PP.FHDB.APPS` layout; OrbitPS2 refuses an existing app folder | [V] |
| Prompt: only FMCB's "keep existing CNF?", for config, not binaries | [V] |
| RiptOPL docs: updating = overwrite the ELF, after backing up settings and saves | [R] |
| TinyPS2Manager confirms one digest and installs another on a rolling tag | [V]/[D] §1 |

Suggests:
- Prior art offers no ELF backup and no "unknown file" handling; SPEC §5.10's `.yaoplm/backup/loader/` would be new.
- The closest safe patterns are "update only what we installed or recognise by hash" and "pin the digest the user confirmed into the plan, re-check it after download".
- An unrecognised file: show size and SHA-256 in the plan; back up or rename it, never delete it.

### (f) `LICENSE`/`CREDITS` next to the ELF

| Evidence | Tag |
|---|---|
| Upstream and YAOPL: `LICENSE`, `CREDITS`, `README.md`, `DETAILED_CHANGELOG` exist only inside `OPNPS2LD[-<ver>].7z` (and VARIANTS). The bare `OPNPS2LD.ELF` asset comes alone | [V] §6, §7.3 |
| RiptOPL C package: no RiptOPL `LICENSE`/`CREDITS`; `EMBER/LICENSE-BETA.txt` is mandatory (CI fails without it). C++ kit: `LICENSE`, `CREDITS`, `README.md` at the root | [V] §5 |
| PS2BBL 7z: `LICENSE.TXT`, `README.md` | [V] §4.2 |
| No manager copies a licence file to the device; PS2 HDD Manager credits OPL on the PC side only | [V absence] |

Suggests:
- Copying them means downloading the 7z (1.4 MB) instead of, or as well as, the bare ELF; upstream does not offer them separately.
- For RiptOPL C, only Ember's licence is available, and it must travel with `EMBER/` if we install the full package.

### (g) Deciding "newer" for rolling builds

| Evidence | Tag |
|---|---|
| Upstream `latest`: name `latest`, body without version, `target_commitish: master`; the version only in versioned asset names; the release is recreated each build | [V] §7.1 |
| YAOPL: name = full version, body = full SHA, release and tag recreated each push | [V] §6 |
| RiptOPL C: release edited in place (stable id); `target_commitish` stale (`05297663` vs tag `55261de1`); version in zip names without hash; one-click `RIPTOPL.ELF` has no version in its name; Beta = commit count + 2 and was once wrong ("Beta-3") | [V]/[R] §5 |
| RiptOPL C++: no version in asset names; `published_at` 4 days older than the assets; `BUILD-MANIFEST.json` has `version`, `source_commit`, `elf_sha256` | [V] §5.2 |
| `current-fan-favorite`: published 2026-09-04, assets re-uploaded 2026-09-30 | [V] |
| Managers: PS2 HDD Manager re-downloads when size or digest differ (no ordering); PSBBN replaces when the MD5 differs (bundled wins); PSBBN's Neutrino uses `sort -V` on `version.txt`; OrbitPS2 and opl-linux-toolbox use semver on their own tags | [V] |
| Beta numbers are per repository and reused across forks; upstream rewrote history after v1.1.0 | research/16 §0.7, §1.1 |

Suggests:
- A digest says "different", never "newer".
- Within one repo and channel, the revision number in the version string is the usable ordering key. Take it from versioned asset names, archive members, `title.cfg` `Version=` or our own hash history; only YAOPL puts it in the release name.
- Use the tag ref (not `target_commitish`) for the commit, and asset `updated_at` (not `published_at`) for the date.
- An older revision or an equal number with a different digest is a "different build", not an update.

---

## 10. Corrections and open checks for research/07 and research/16

1. research/07 §1.3: the real `title.cfg` `Version=` is `<ASSET_VERSION> <FLAVOUR>` (`v1.2.0-Beta-4257 OFFICIALROLLING`). The quoted `RiptOpen PS2 Loader v1.2.0 OFFICIAL ROLLING` is the template's [V §5.1].
2. research/07 §1.3: the package layout is `APPS/APP_RIPTOPL[-<FLAVOUR>]/` since 2026-09-20. The RiptOPL C package has no `LICENSE`/`CREDITS`. The C `rolling` release has no `SHA256SUMS` because CI deletes it [V §5.1].
3. research/07 §2.2/§2.9: FMCB 1.953 has the `UPDT/OPL.ELF` item too; 1.965 has `LK_L1 = mass:/OPL.ELF`. The FMCB installer ships OPL 1.0.0 [V §4.1].
4. research/07 §2.4: PS2BBL's no-config fallback L1 includes `mass:/PS2BBL/OPNPS2LD.ELF` [V §4.2].
5. research/07 §1.2, research/16 §1.6: the members of `OPNPS2LD.7z` sit under an `OPNPS2LD/` folder, in 1.1.0 and in `latest` alike; the language archives differ between 1.1.0 (folders) and the betas (a zip inside the 7z) [V §7.3, closed 2026-10-25].
6. research/07 §1.11: YAOPL's state is unchanged as of 2026-10-25. One pre-release `latest`, no versioned releases, no `HASHES.txt`, `immutable: false`.

---

## Sources

**Code and CI** (clones under `D:\yaoplm-testdata\refs\`, commits in the table at the top):
- TinyPS2Manager: `internal/riptopl/{riptopl.go,doc.go,riptopl_test.go}`, `internal/api/{prepare.go,enrich.go}`, `riptopl.go`, `internal/transfer/disk.go`, `internal/art/art.go`, `OPL-Backup-Manager-SPEC.md` §2.7
- OrbitPS2 Manager: `src/services/{update,library,apps,ps1-import,artwork}.service.ts`
- PS2 HDD Manager: `fetch_payloads.sh`, `PROVISIONING.md`, `docs/INSTALLED_TITLE_AND_PC_GAMEID.md`, `src/core/OplConfig.cpp`, `src/core/Ps2HddFormat.cpp`, `src/formatter_main.cpp`, `src/ui/MainWindow.cpp`, `THIRD_PARTY_NOTICES.md`, `payload/banked-opl/SHA256.txt`
- FreeMcBoot-Installer: `installer/{system.c,menu.c,lang.c}`, `installer_res/{1953,1963,1964,1965,1966}/INSTALL/SYS-CONF/FREEMCB.CNF`, `installer_res/*/INSTALL/APPS/OPNPS2LD.ELF` (hashed locally), `MASS/FREEMCB.CNF`
- PS2BBL: `release/SYS-CONF/PS2BBL.INI`, `include/common.h`, `src/main.c`, `Makefile`, `.github/workflows/CI.yml`
- OSDMenu: `examples/OSDMENU.CNF`, `examples/OSDMBR.CNF`, `utils/installer/{README.md,src/main.c}`
- RiptOPL C: `.github/workflows/{rolling-release,release-normalize}.yml`, `.github/scripts/{package_release_app,asset_version}.py|sh`, `Example/APP_RIPTOPL-OFFICIALROLLING-VERSION.ZIP/APPS/APP_RIPTOPL/title.cfg`; commit `d91ffcae` (2026-09-20) and its parent `91072b10`
- RiptOPL C++: `tools/{build_edition,package_edition,publish_editions}.py`
- RiptOPL docs (`gh-pages` @`20d2113`): `install.html`, `releases.html`, `languages.html`, `usb.html`
- OPL upstream: `.github/workflows/compilation.yml`, `lng_pack.sh`, `src/lang.c`, `src/bdmsupport.c`, `src/config.c`
- YAOPL: `.github/workflows/compilation.yml`, `FORK.md` (working tree `33a2b19a`)
- PSBBN: `scripts/Game-Installer.sh`, `scripts/assets/{OPL,NHDDL}/`, `scripts/assets/database/AppDB.csv`
- PFS BatchKit Manager: `PFS-BatchKit-Manager/BAT/APPS.BAT`, `PFS-BatchKit-Manager/!PFS-BatchKit-Manager.bat`
- opl-linux-toolbox: `src/services/linux-updater.service.ts`; OPL-PC-Tools `src/OplPcTools/Updater.cpp`; ps2-game-manager `api/app/github.py`; PS2FASTLOADER `desktop/src-tauri/src/themes.rs`
- YAOPLM: `data/loader-hashes/opl-stable.txt`, `docs/SPEC.md` §5.10

**GitHub API** (2026-10-25):
- `repos/ps2homebrew/Open-PS2-Loader/releases/tags/{latest,v1.1.0}` and `git/ref/tags/latest`;
- `repos/belomaxorka/yaopl/{releases,commits/master,contents/FORK.md,git/ref/tags/latest}`;
- `repos/NathanNeurotic/Open-PS2-Loader/{releases,releases/tags/rolling,git/ref/tags/rolling}`;
- `repos/NathanNeurotic/RiptOPL/releases`;
- `repos/Luden02/OrbitPS2-Manager/issues?state=all`.
