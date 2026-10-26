# 07 — Loaders and boot setups: installing OPL (or a fork) onto a device

Research date: 2026-10-22. Goal: let YAOPLM install a chosen loader (upstream OPL or a fork) onto a device and prepare that device for the user's boot setup. The product owner launches OPL from a FreeMcBoot (FMCB) memory card and also owns an MMCE-type "smart" memory card with a microSD slot.

How this was researched:
- `gh api` for repo metadata and releases (assets, sizes and `digest`).
- Shallow clones under `$YAOPLM_TESTDATA\refs\<owner>__<repo>` (outside the repo) to read code. Nothing was built or run, and no release binary was downloaded.
- WebSearch for forum knowledge. psx-place.com, consolemods.org and playstationdev.wiki return bot checks or 403, so they appear only as search snippets.

Clones and the commits read:

| Clone | HEAD |
|---|---|
| `ps2homebrew__Open-PS2-Loader` (upstream) | `b3a2199`, 2026-10-12 |
| `NathanNeurotic__Open-PS2-Loader` (RiptOPL, branch `rebuild/main`) | `40c52ce`, 2026-10-22 |
| `NathanNeurotic__Open-PS2-Loader__gh-pages` (RiptOPL docs site) | `32cd00f`, 2026-10-21 |
| `ps2-mmce__Open-PS2-Loader` (branch `mmce_support` fetched) | `e554796`, 2025-11-24 |
| `ps2homebrew__wOPL` | `4ae7414`, 2026-09-13 |
| `pcm720__nhddl` | `89141d4` = v1.2.2 |
| `Luden02__OrbitPS2-Manager` | `cdd590a`, 2026-10-22 |

Also cloned: `rickgaiser__neutrino`, `ps2-mmce__mmceman`, `ps2-mmce__neutrino`, `sd2psXtd__firmware`, `sd2psXtd__sd2psXtd.github.io`, `sd2psx__firmware`, `israpps__FreeMcBoot-Installer`, `israpps__PlayStation2-Basic-BootLoader`, `pcm720__OSDMenu`, `ps2homebrew__wLaunchELF`, `israpps__wLaunchELF_ISR`, `CTurt__FreeDVDBoot`.

Verification tags (same scheme as research/02):

| Tag | Meaning |
|---|---|
| **[V]** | Verified by reading source code, CI config or a GitHub API response (file or endpoint named) |
| **[D]** | Derived: reasoned from verified facts. Not tested on hardware |
| **[R]** | From an official README, docs site or vendor manual, not from code |
| **[U]** | Unverified: forum posts, search snippets, third-party tools |

---

## 0. Key findings (what changes our SPEC)

1. **Only forks support MMCE; upstream OPL does not.**
   - Upstream OPL has no MMCE code at all, and feature request #1598 was closed as a duplicate [V].
   - The MMCE OPL fork is `ps2-mmce/Open-PS2-Loader`, branch `mmce_support`. Its only builds are two prereleases, the last on 2025-01-15. The branch was last touched on 2025-11-24 [V].
   - Maintained loaders with MMCE: RiptOPL, wOPL and NHDDL+Neutrino [V].
   - So "MMCE storage" is a property of the **loader profile**, not of the device.
2. **MMCE layout = USB layout.**
   - The SD card holds the usual `CD/ DVD/ ART/ CFG/ VMC/ CHT/ THM/ LNG/ APPS/`, at the SD root or under `mmce_prefix` [V].
   - The card firmware owns the filesystem, so there is no 64-fragment limit. exFAT works [V]/[R].
   - Multi-part `ul` games likely cannot stream on MMCE: the in-game driver gets one file handle [D].
   - The card's own folders must never be touched:
     - SD2PSX/sd2psXtd: `.sd2psx/`, `MemoryCards/`;
     - MemCard PRO 2: `PS1/`, `PS2/`, `mcp2.bin`, `os/`.
3. **The MMCE fork keeps `conf_opl.cfg` on the memory card, not on the SD card.** For an MMCE card that is the emulated boot-card image (`.mcd`) [V]. RiptOPL instead keeps `settings_riptopl.cfg` **next to its ELF**, and can also find it at the `mmceN:/` root [V]. A PC tool can therefore preconfigure RiptOPL, but not the MMCE fork, without editing a memory-card image.
4. **RiptOPL art is not "8-bit indexed only".**
   - RiptOPL's PNG loader accepts RGB, RGBA, 4/8-bit palette and grey (promoted) [V `src/textures.c`].
   - It **rejects 16-bit-per-channel PNGs**; upstream silently strips them to 8-bit [V].
   - RiptOPL's own release art is 8-bit **RGBA** at **standard** sizes: `RIPTOPL.ELF_COV.png` is 140×200 and `_ICO` is 64×64 [V].
   - The "8-bit indexed" rule and the doubled sizes come from **OrbitPS2 Manager's pipeline**: it can resize palette images but cannot quantise.
   - So SPEC open question 1 is answered: **indexed is recommended, not required. Bit depth ≤8 is required.**
5. **"Only COV and ICO use doubled dimensions"** means that, against OPL Manager's standard sizes (research/02 §7), only COV and ICO are 2×:
   - COV 140×200 → 280×400;
   - PS1 COV 200×200 → 400×400;
   - ICO 64×64 → 128×128.
   Every other type keeps the OPL Manager size. The doubling serves RiptOPL's Coverflow and PSBBN-Flow themes, which draw an enlarged centre cover, and its disc-art (`ICO`) [D].
6. **Art keys confirmed in RiptOPL code:**
   - PS2 = game ID;
   - VCD = filename without `.VCD`;
   - Ember = folder name under `EMBER/games/`;
   - app = ELF file name **with** `.ELF`.
   RiptOPL's own docs contradict themselves on Ember (one page says "PS1 disc ID") and on a VCD disc-ID art fallback. The art code has neither (§4).
7. **wOPL changed its config format (new).** Since 2026-06-20 (PR #286) wOPL stores settings **and per-game `CFG/<ID>.cfg` in libconfig syntax**, plus a `CFG/<ID>.info` sidecar.
   - Its GUI has a batch tool that rewrites existing key=value CFG files, keeping `.bak` copies [V].
   - A device shared between OPL and current wOPL can end up with CFG files that OPL cannot parse.
   - CFG-1 must detect this format and never "repair" it.
8. **Install paths for OPL are by convention, not standard.**
   - FMCB 1.966's stock menu item tries `mass:/UPDT/OPL.ELF` first, then `mc?:/APPS/OPNPS2LD.ELF`, then `mc?:/OPL/OPNPS2LD.ELF` [V].
   - PS2BBL's stock config tries `mass:/APPS/OPNPS2LD.ELF` first, then the same two memory-card paths [V].
   - So with stock configs YAOPLM can update OPL on a **USB stick** without touching the memory card.
9. **FMCB cannot read MMCE, MX4SIO or exFAT** in its stock form [U, consistent with all its configs].
   - To launch from those devices you need PS2BBL (MMCE or MX4SIO build), OSDMenu, or an OPL ELF inside the emulated boot card.
   - wLaunchELF_ISR builds can browse MMCE and MX4SIO.
10. **Every target loader publishes SHA-256 `digest`s** for current assets in the GitHub API [V].
    - Upstream OPL `latest`, RiptOPL, wOPL, NHDDL, Neutrino, PS2BBL `latest` and OSDMenu all have them.
    - Old assets have `null`: OPL v1.1.0 and the MMCE fork's betas.
    - Rolling tags are re-published in place, so pin by digest, never by tag.
11. **Neither OPL nor RiptOPL ELFs show their version in plain text.** Both are packed with ps2-packer (LZMA by default) [V].
    - Identify them by SHA-256 against known release digests.
    - Also use the RiptOPL app's `title.cfg` `Version=`, release file names, and our own install manifest.
12. **Licences allow on-demand download, not bundling by default.**
    - OPL, RiptOPL, wOPL, NHDDL and Neutrino are AFL-3.0. PS2BBL is GPL-3.0.
    - FMCB, FreeMcBoot-Installer, FreeDVDBoot, POPSTARTER and Ember are proprietary or unlicensed.
    - Download from upstream at runtime, show licence and credits, never mirror (ADR-0007 style).

---

## 1. Loader catalogue

### 1.1 Overview

| Loader | Repo | Licence | Channel and cadence | Main release assets | `digest` | Settings file(s) |
|---|---|---|---|---|---|---|
| **OPL 1.2.0 beta** | `ps2homebrew/Open-PS2-Loader` (3,082★) | AFL-3.0 | Pre-release tag `latest`, re-published on every push to `master` (last 2026-10-13, `v1.2.0-Beta-2271-b3a2199`) | `OPNPS2LD.ELF`, `OPNPS2LD.7z`, `OPNPS2LD-VARIANTS.7z`, `OPNPS2LD-LANGS.7z`, plus versioned copies of the three `.7z` | sha256 on all | `conf_opl.cfg`, `conf_last.cfg`, `conf_apps.cfg`, `conf_network.cfg`, `conf_game.cfg`; APA `__common/OPL/conf_hdd.cfg` |
| **OPL 1.1.0** | same | AFL-3.0 | Stable, 2021-09-09, frozen | `OPNPS2LD-v1.1.0.7z` (= `OPNPS2LD.7z`), `OPNPS2LD-VARIANTS*.7z`, `OPNPS2LD-LANGUAGES-AND-FONTS-v1.1.0.7z` | `null` | same |
| **RiptOPL** | `NathanNeurotic/Open-PS2-Loader` (66★; fork of ps2-mmce fork) | AFL-3.0 | Non-pre-release rolling tags rebuilt on every push: `rolling` (inceBtion theme), `rolling-korium`, `rolling-ra` (RetroAchievements, pre-release), snapshot `current-fan-favorite` (2026-09-04). Several builds per day | `RIPTOPL.ELF`, `APP_RIPTOPL.psu`, `RIPTOPL-v1.2.0-Beta-4221.zip` (package), `-VARIANTS-`, `-LANGS-`, `-DEBUG-`, `-src.zip`; `-Korium` suffixed set | sha256 on all; RA also ships `SHA256SUMS.txt` | `settings_riptopl.cfg` (legacy `conf_riptopl.cfg`; reads `conf_opl.cfg` read-only as a seed), `config.path`, `favourites.bin` |
| RiptOPL "C++ PS2Build Edition" | `NathanNeurotic/RiptOPL` (4★, created 2026-09) | AFL-3.0 | Rolling tags `rolling-korium`, `rolling-incebtion`, `rolling-ra` | `RIPTOPL.ELF`, `APP_RIPTOPL.psu`, `RIPTOPL-<flavour>.zip`, `BUILD-MANIFEST.json`, `SHA256SUMS.txt` | sha256 | same as RiptOPL |
| **OPL MMCE fork** | `ps2-mmce/Open-PS2-Loader` (98★), branch `mmce_support` | AFL-3.0 | Two pre-releases: `mmce-beta1` (2024-12-23), `OPL-MMCE-beta-2` (2025-01-15). Branch last merged with upstream 2025-11-24, no newer build | `OPNPS2LD-MMCE-b1.ELF`; `OPNPS2LD.ELF` (beta-2) | `null` | `conf_opl.cfg` (+`mmce_*` keys), on MC/USB/HDD only |
| **wOPL** ("Double Unofficial OPL") | `ps2homebrew/wOPL` (66★) | AFL-3.0 | Versioned releases v1.0 (2026-04-16), v1.1 (2026-05-14); pre-release `latest` "v1.2-beta-468" (2026-07-23) | `WOPNPS2LD.ELF`, `WOPNPS2LD.7z`, `-VARIANTS.7z`, `-LANGS.7z`, `-DEBUG.7z`, plus README/LICENSE/CREDITS/DETAILED_CHANGELOG | sha256 | HEAD: `wopl_settings.cfg`, `wopl_network.cfg`, `wopl_global_game.cfg`, `wopl_last_played.cfg`, `wopl_boot.cfg` (**libconfig**); older: `conf_wopl.cfg`. APA `hdd0:__common/wOPL/conf_hdd.cfg`, partition `+wOPL` |
| **NHDDL** | `pcm720/nhddl` (168★; default branch `legacy`) | AFL-3.0 | Releases about every 2–5 months; v1.2.2 2026-04-05 | `nhddl-v1.2.2.zip` (CI zips `nhddl.elf` + extras), bundled `neutrino_v1.8.0-35-g7b6e14b.7z` | sha256 (since v1.2.0) | `nhddl.yaml`; device-side `nhddl/` (`global.yaml`, `<ISO>.yaml`, `cache.bin`, `lastTitle.bin`) |
| **Neutrino** | `rickgaiser/neutrino` (284★) | AFL-3.0 | v1.8.0 2026-03-10; pre-release `latest` 2026-08-31 | `neutrino_v1.8.0.7z` (folder `neutrino/` with `neutrino.elf`, `config/`, `modules/`) | sha256 (since v1.7.0) | `config/*.toml` |

All rows are [V] (gh api `repos/<o>/<r>` and `releases`, 2026-10-22), except where a cell names a file read in a clone.

### 1.2 Upstream OPL

- **Version string** [V `Makefile`]: `v1.2.0-Beta-<REVISION>-<hash>`. `REVISION` = `git rev-list --count HEAD` + 2, followed by the 7-char hash. A tagged build uses the tag (`v1.1.0`).
- **Packaging** [V `.github/workflows/compilation.yml`]:
  - `OPNPS2LD.7z` holds `OPNPS2LD-<ver>.ELF` plus `DETAILED_CHANGELOG`, `CREDITS`, `LICENSE`, `README.md` in a folder `OPNPS2LD/` (corrected by listing the release files, research/17 §7.3; this line first said "flat with no folder").
  - The bare `OPNPS2LD.ELF` asset is the same ELF renamed.
  - The un-suffixed `.7z` names are byte-identical copies of the versioned ones; the digests match.
- **Variants** [V CI, Makefile]:
  - `OPNPS2LD-VARIANTS.7z` holds **unpacked** ELFs named `OPNPS2LD-<ver>-PADEMU=<0|1>-EXTRA_FEATURES=<0|1>.ELF`.
  - `EXTRA_FEATURES=1` turns on `RTL` (right-to-left text) and `IGS` (in-game screenshot).
  - The default release build is `PADEMU=1 EXTRA_FEATURES=0`.
  - v1.1.0 named variants `OPNPS2LD-v1.1.0-<DTL_T10000>-<IGS>-<PADEMU>-<RTL>.ELF`.
  - Debug builds are CI artifacts only, not release assets.
- **Languages** [V `lng_pack.sh`]:
  - `OPNPS2LD-LANGS.7z` wraps a zip with one folder per language, `<Lang>-<ver>/lang_<Lang>.lng`, plus `font_<Lang>.ttf` when one exists.
  - These go into the device's `LNG/`.
- **Backends**: USB, MX4SIO, iLink, ATA-exFAT (BDM), APA HDD, SMB. No MMCE, UDPBD or UDPFS (§1.8; research/02 §2).

### 1.3 RiptOPL

- It is a downstream fork of the MMCE fork, so it has MMCE built in, plus many additions [V `src/` file list]:
  - `httpsupport.c` (HTTP source), `udpfssupport.c`, UDPBD through Neutrino;
  - PC-card CF (`enable_pccf`), PSX DVR (`enable_dvr`);
  - Ember PS1 core, Favourites, RetroAchievements.
- **Package layout** [R docs site `install.html`; V `Example/APP_RIPTOPL-OFFICIALROLLING-VERSION.ZIP/`, `.github/workflows/rolling-release.yml`]:
  ```
  RIPTOPL-<version>.zip
    APPS/APP_RIPTOPL/               ready app: RIPTOPL.ELF, title.cfg, icon.sys, list.icn, copy.icn, del.icn, APPINFO.PBT, bgm.ogg
    APPS/APP_RIPTOPL-PS2DEVPINNED/  alternative flavours (PS2DEVPINNED, OFFICIALPINNED, PS2DEVROLLING, OFFICIALROLLING)
    APP_RIPTOPL.psu                 same app as a memory-card save
    ART/RIPTOPL.ELF_{COV,COV2,ICO,LAB,LGO,BG,SCR,SCR2}.png
    POPS/                           POPSTARTER.ELF, SMB modules, PATCH_5.BIN, BDMA usbd/usbhdfsd pairs (.ata .ilink .mmce .mx4sio .udpfs .usbexfat)
    EMBER/                          ember.elf, LICENSE-BETA.txt, placeholder for the user's BIOS
    neutrino/                       Neutrino, pre-configured for UDPFS
    *.url                           links to PS2-Servers, udpfs-server, OrbitPS2, PS1 AIO converter, CHT manager
  ```
- **Version metadata in the package** [V]: `APPS/APP_RIPTOPL/title.cfg` has `title=`, `boot=RIPTOPL.ELF` and **`Version=<version> <flavour>`**. Example: `Version=RiptOpen PS2 Loader v1.2.0 OFFICIAL ROLLING`. The CI stages it via `.github/scripts/package_release_app.py --version --flavour`.
- **Updating** [R]: the release's loose `RIPTOPL.ELF` is the `-OFFICIALROLLING` flavour; its file name and link never change. Users overwrite their installed ELF; `POPS/`, `EMBER/`, `neutrino/` and settings stay.
- **Where the ELF goes** [R]: "RiptOPL has no path requirements of its own". Common conventions: `mass:/APPS/RIPTOPL/RIPTOPL.ELF` or `mc0:/RIPTOPL/RIPTOPL.ELF`.
- **Settings home** [V `src/opl.c`, `include/config.h`; R `install.html`]:
  - The folder the ELF was launched from (its CWD).
  - If the CWD is unknown: the `config.path` redirect file, then `mc0:/OPL`, `mc0:/`, `mc1:…`, then USB `massN:/` and `massN:/OPL`, then MMCE root, then BDM, then the HDD.
  - If a folder has no RiptOPL settings but has `conf_opl.cfg`, RiptOPL **reads it once and never writes it**.
- **New device keys** [V `include/config.h`]:
  - `enable_usb` (USB must now be enabled explicitly), `enable_pccf`, `enable_dvr`, `enable_udpbd`, `net_start_mode`, `udpfs_access`, `fav_mode`;
  - `mmce_mode`, `mmce_prefix`, `mmce_slot`, `mmceigr_slot`, `mmce_gameid`, `mmce_wait_cycles`, `mmce_use_alarms`, `mmce_pacing_migrated`;
  - art toggles `enable_discart`, `enable_logoart`, `enable_bgart`, `enable_art_tar`.
- **Fresh-start defaults** [R]: every device and start mode is **off**. Nothing is created until the user enables a device and saves.

### 1.4 OPL MMCE fork (ps2-mmce)

- Work by bbsan2k and HowlingWolfHWC. The code lives only on `mmce_support`, 52 commits ahead of `master` [V].
- Files [V]:
  - `src/mmcesupport.c` (new `MMCE_MODE` in `IO_MODES`);
  - `modules/iopcore/cdvdman/device-mmce.c` (in-game streaming via MMCEDRV);
  - `modules/mcemu/device-mmce.c` (VMC on MMCE).
- **Keys** [V `include/config.h`, `src/opl.c`]:

  | Key | Values | Default |
  |---|---|---|
  | `mmce_mode` | 0 off, 1 manual, 2 auto | 0 |
  | `mmce_slot` | 0 = `mmce0`, 1 = `mmce1`, 2 = auto | 2 |
  | `mmceigr_slot` | 0–3 | 3 |
  | `mmce_prefix` | ≤31 chars | empty |
  | `mmce_gameid` | 0/1 | 1 |

  Old beta-era configs may still carry `mmce_wait_cylces` (sic) and `mmce_use_alarms`; preserve them as unknown keys [V commit 7e5995e].
- **Prefix bug** [V]: the root is `sprintf("mmce0:/%s", gMMCEPrefix)` followed by `"%sCD"`, so a non-empty prefix **must end with `/`**. RiptOPL normalises this; the MMCE fork does not.
- The 8BitMods MemCard PRO 2 manual and sd2psXtd's homebrew page both send users to this fork's releases [R].

### 1.5 wOPL (and the uOPL/sOPL lineage)

- wOPL continues KrahJohlito's uOPL and is maintained by Wolf3s and Ripto (README) [R].
- Changelog highlights [R README]:
  - v1.0: UDPBD removed, `art.tar` support, MMCE support;
  - v1.1: favourites on by default;
  - v1.2: Neutrino for MMCE.
- **Config migration to libconfig** [V `src/config_wopl.c`, `src/config_migration.c`, `src/gui.c`; commit `03758fb` 2026-06-20]:
  - Global files are renamed to `wopl_*.cfg` and written in libconfig syntax.
  - Per-game settings use libconfig keys: `compat`, `dma`, `core_loader`, `vmc1`, `vmc2`, and groups such as `gsm.enable` and `cheat.mode`.
  - Metadata goes to `CFG/<ID>.info`.
  - `guiShowCfgMigration` / `cfgBatchMigratePerGame` rewrites every `CFG/*.cfg` on a chosen device. It can write to another prefix and can keep `.bak` originals.
- Neutrino goes in a `neutrino/` folder at the root of USB, MX4SIO, ATA, iLink, MMCE or APA [R].
- **sOPL** (`mystyq/Stable-Open-PS2-Loader`, AFL-3.0, 9★): single `SOPL.ELF` release on 2026-05-09 with a digest. It is the origin of the lazy `art.tar` loader [V tar.c header; gh api].

### 1.6 NHDDL + Neutrino

- NHDDL is a launcher UI. Neutrino is the loader core (a device emulator). NHDDL scans **ISO only**, up to 5 directories deep [V `src/devices/mmce.c`, README].
  - It skips folders starting with `.` or `$`, and also `nhddl`, `APPS`, `ART`, `CFG`, `CHT`, `LNG`, `THM`, `VMC`, `XEBPLUS` and `MemoryCards`.
- **Device modes**: `ata`, `mx4sio`, `usb`, `udpfs`, `ilink`, `mmce`, `hdl` (APA). Select with `mode:` in `nhddl.yaml`, with `-mode=` argv, or by naming the ELF `nhddl-<mode>.elf` [R README].
- **Neutrino search paths** [R README]:
  1. the launch dir;
  2. `massX:/neutrino/neutrino.elf`;
  3. `hdd0:/<OPL partition>/neutrino/neutrino.elf`;
  4. `mmceX:/neutrino/neutrino.elf`;
  5. `mcX:/APPS/neutrino/neutrino.elf`;
  6. `mcX:/NEUTRINO/NEUTRINO.ELF` or `mcX:/NEUTRINO/neutrino.elf`.
- **`nhddl.yaml` search paths** [R README]: the launch dir, `massX:/nhddl/`, `hdd0:/<OPL partition>/nhddl/`, `mmceX:/nhddl/`, `mcX:/APP_NHDDL/`.
- **Art**: `ART/<ID>_COV.png`, 140×200 [R].
- **Neutrino**: no ZSO, CSO or `ul` support on the console; ZSO only through the PC-side `udpfs_server.py` [V]. Its MMCE support arrived in v1.5.0 [R].

### 1.7 Other maintained forks (catalogue candidates, not targets)

| Fork | Licence | Notes |
|---|---|---|
| `ps2max32/Open-PS2-Loader` (21★) | AFL-3.0 | Mirrors upstream asset names (`OPNPS2LD.ELF`, `.7z`), `v1.2.0-Beta-2257-d882fd7`, 2026-02-22 [V] |
| `officialjuicedesigns/OPL-Evolution` (21★) | AFL-3.0 | "Experimental"; `OPL-EVOLUTION-0.27.0-NATIVE-VMC.ELF` + `CHECKSUMS.txt`, alpha 2026-07-26 [V] |
| `L10N37/Open-PS2-Loader-Extended-APA` | AFL-3.0 | APA >2 TiB, `OPNPS2LD-Extended-APA-v1.2.0-Beta-2273.ELF` + `SHA256SUMS.txt`, 2026-09-06 [V] |
| `362053534/Open-PS2-Loader` | — | Chinese-list fork [V gh api] |

### 1.8 Storage backends per loader

| Backend | OPL 1.1.0 | OPL 1.2.0 beta | MMCE fork | RiptOPL | wOPL | NHDDL + Neutrino |
|---|---|---|---|---|---|---|
| USB (BDM `usb`) | FAT32 | FAT32/exFAT | FAT32/exFAT | FAT32/exFAT | FAT32/exFAT | FAT32/exFAT |
| MX4SIO (BDM `sdc`, slot 2) | FAT32 (`enable_mx4sio`) | yes | yes | yes | yes | yes |
| iLink (BDM `sd`) | FAT32 (`enable_ilink`) | yes | yes | yes | yes | yes |
| ATA exFAT (BDM `ata`) | no | yes (`enable_bdm_hdd`) | yes | yes ("BDMAssault") | yes | yes |
| MMCE (`mmce0:`/`mmce1:`) | no | **no** | **yes** | **yes** | **yes** | **yes** |
| APA HDD (HDL) | yes | yes | yes | yes | yes (`+wOPL`) | yes (`hdl`) |
| SMB | yes | yes | yes | yes | yes | no |
| UDPBD | no | no | no | yes (Neutrino core) | removed in v1.0 | via Neutrino |
| UDPFS | no | no | no | yes | no | yes |
| HTTP | no | no | no | yes | no | no |

Sources: config keys in each `include/config.h`, `src/*support.c` file lists, READMEs; v1.1.0 `include/config.h` via gh api (it has no `enable_bdm_hdd`) [V]/[R].

### 1.9 Detecting an installed loader and its version

- **The ELF is packed.**
  - Upstream: `EE_BIN_PACKED = OPNPS2LD.ELF` built by `ps2-packer $< $@`. RiptOPL: `RIPTOPL.ELF` the same way [V Makefiles].
  - ps2-packer's default packer is **LZMA** [R ps2dev/ps2-packer README].
  - The version string (`"Open PS2 Loader %s"`, `"OPL/" OPL_VERSION`) is therefore not visible in the file.
  - VARIANTS ELFs are unpacked (`NOT_PACKED=1`), so their version string is plain text [D].
- **Practical detection order** [D]:
  1. **YAOPLM's own install manifest**, written at install time (§6.5).
  2. **SHA-256 match** against the digests of every known release asset. GitHub returns them per asset; we cache release lists.
  3. **Sidecar metadata.**
     - RiptOPL `APPS/<app>/title.cfg` `Version=` and `boot=`.
     - wOPL and RiptOPL release names: `RIPTOPL-v1.2.0-Beta-4221-Korium.zip`, `Double Unofficial OPL v1.2-beta-468`.
     - Upstream archive member names: `OPNPS2LD-v1.2.0-Beta-2271-b3a2199.ELF`.
  4. **File-name heuristics**: `OPNPS2LD*.ELF` → OPL family, `RIPTOPL*.ELF`, `WOPNPS2LD.ELF`, `SOPL.ELF`, `nhddl*.elf`, `neutrino/neutrino.elf`.
  5. **Settings-file fingerprints** next to the ELF or on the device root. These identify the *profile* even without the ELF:
     - `conf_opl.cfg` → OPL;
     - `settings_riptopl.cfg` / `conf_riptopl.cfg` → RiptOPL;
     - `wopl_settings.cfg` / `conf_wopl.cfg` → wOPL;
     - `nhddl/` → NHDDL;
     - `mmce_*` keys in `conf_opl.cfg` → MMCE fork.
  6. Later and optional: unpack the ps2-packer LZMA payload and grep `v\d+\.\d+\.\d+-\w+-\d+(-[0-9a-f]{7})?`. The packed layout has not been examined [D].
- **Superseded in part by research/16 (2026-10-24):** `mmce_*` keys are YAOPL's too; the absence of the keys every beta from build 1871 writes is the stronger `conf_opl.cfg` sign; `CD/`/`DVD/games.bin` record sizes (92 vs 188 bytes) tell the generation that last listed the games; `nhddl/` alone is not NHDDL. The order YAOPLM implements is ADR-0039.

### 1.10 GitHub `digest` observations

- Every asset uploaded in roughly the last year has `digest: "sha256:<hex>"`. Older uploads have `null`:
  - OPL v1.1.0 (2021);
  - MMCE fork betas (2024/25);
  - NHDDL ≤v1.1.2 and Neutrino ≤v1.6.1;
  - PS2BBL v1.2.0 (2023).
  [V gh api]
- PS2BBL's 2023 `kelf` pre-release *does* have a digest, so the field seems to depend on when the asset was (re)uploaded [V observation; cause U].
- Rolling tags (`latest`, `rolling*`) replace assets in place. The same tag name can point to a different digest an hour later [V: RiptOPL `rolling` published 2026-10-22T13:07Z].

### 1.11 Picking a version: what each repository keeps (2026-10-24)

For the version picker of SPEC LDR-6 (§5.10.2, ADR-0038). All [V] (gh api `releases` and `tags`, 2026-10-24) unless marked.
- **Upstream OPL:**
  - releases: `latest` (pre-release, re-published in place, last 2026-10-13, `v1.2.0-Beta-2271-b3a2199`), `v1.1.0` (2021-09-09), `v1.0.0` (2021-01-06), `v1.0.0-rc1` (pre-release) and `0.5`–`0.9.3` (all uploaded 2016-10-26). No earlier 1.2.0 beta is kept on GitHub;
  - release text: `latest` has a commit list grouped by type with links (about 0.6 KB); `v1.1.0` has a hand-written "New in OPL v1.1.0" section plus every commit (about 20 KB);
  - `v1.1.0` has no bare ELF asset: the ELF is inside `OPNPS2LD-v1.1.0.7z` (= `OPNPS2LD.7z`), and every asset has `digest: null`;
  - the README says every build dispatched to the release section is also uploaded to a MEGA folder "since 05/07/2021" [R README]. MEGA is not a GitHub release and needs its own client, so YAOPLM does not download from it (ADR-0038).
- **YAOPL** (`belomaxorka/yaopl`): one pre-release `latest` (2026-10-23, `v1.2.0-Beta-2370-33a2b19-yaopl`); its text names the commit and points to `FORK.md` and `SHA256SUMS`. `FORK.md`: "Only the build of the current tip of `master` is published." The tag list shows upstream's tags, inherited through git, without releases.
- **RiptOPL** (`NathanNeurotic/Open-PS2-Loader`):
  - releases: `rolling` and `rolling-korium` (rebuilt on every push, last 2026-10-24), `rolling-ra` (pre-release, 2026-10-12), `current-fan-favorite` (2026-09-04);
  - tags without releases: `rolling-alpha`, `rebuild-66`, `rebuild-74`, `rebuild-91`, `pops-bundle`, `checkpoint/2026-08-15-zack-approved-v2582`;
  - release text (3.5–6.5 KB) starts with the build type, the commit link and the version string, then a `<details>` changelog of the last 10 commits.
- **wOPL**: `latest` (pre-release, 2026-07-23), `v1.1-351-…` (2026-05-14), `pre-vv1.0-287-…` (2026-04-16), `beta` (pre-release, 2025-03-01).
- **What this means for LDR-6:** a version picker works for upstream (`latest`, `v1.1.0`), wOPL and the MMCE fork (§1.1). RiptOPL can only offer its flavours. YAOPL needs immutable versioned releases next to `latest` in its CI.

---

## 2. Boot setups: where the loader ELF must live

### 2.1 Path prefixes each launcher understands

| Launcher | mc | USB | MX4SIO | MMCE | APA HDD | exFAT HDD | Notes |
|---|---|---|---|---|---|---|---|
| **FMCB 1.953/1.966** (closed source) | `mc?:` | `mass:` (FAT32; exFAT only with the israpps package's custom IRX in `SYS-CONF/`) | no | no | `hdd0:<part>:pfs:/` | no | [V configs; U for "no MX4SIO/MMCE"] |
| **OSDMenu** v1.3.0 | `mc?:`, `mc0/1:` | `mass?:`, `usb?:` | `mx4sio:` | `mmce?:`, `mmce0/1:` | `hdd?:…:pfs:` | `ata?:` | Also `ilink:`, `xfrom:`, `rom?:`, `cdrom`. `udpbd:`/`udpfs:` exist but are disabled in OSDMenu builds [V `launcher/src/common.c`] |
| **PS2BBL** | `mc?:` | `mass:` (FAT32/exFAT, embedded BDM) | `massX:` (MX4SIO build) | `mmce?:` (MMCE build) | `hdd0:<part>:pfs:/` (HDD build) | — | MX4SIO and MMCE builds are mutually exclusive [V Makefile, `src/main.c`] |
| **wLaunchELF** upstream | `mc0:`, `mc1:` | `mass:`, `mass0..9:` | no | no | `hdd0:` | via BDM | [V `filer.c`] |
| **wLaunchELF_ISR** (israpps) | yes | yes | `mx4sio:` alias of `massN:` | `mmce0:`/`mmce1:` (MMCE builds) | yes | yes | 40 build flavours [V] |

`mc?:` means try slot 1 then slot 2. PS2BBL tries the slot its config came from first [V]. FMCB 1.9xx is closed, so its exact search order is [U].

### 2.2 FreeMcBoot 1.953 / 1.966 (israpps/FreeMcBoot-Installer)

- **Config file** [V `installer_res/1966/INSTALL/SYS-CONF/FREEMCB.CNF` header]:
  - `mc?:/SYS-CONF/FREEMCB.CNF` or `mass:/FREEMCB.CNF`. The priority between them is unknown [U].
  - FreeHDBoot reads `hdd0:__sysconf/FMCB/FREEHDB.CNF`.
- **Card layout made by the installer** [V `installer/system.c`; release package]:
  - `APPS/`, `BOOT/`, `SYS-CONF/` (FMCB_CFG.ELF, FREEMCB.CNF, FREEHDB.CNF, USBD.IRX, USBHDFSD.IRX);
  - the system-update folder `B?EXEC-SYSTEM`, where `?` is the region: I = JP, A = US, E = EU, C = CN;
  - OPL goes to `mc?:/APPS/OPNPS2LD.ELF`, wLaunchELF to `mc?:/BOOT/BOOT.ELF`.
- **Stock OPL menu item, 1.966** [V]:
  ```
  name_OSDSYS_ITEM_3  = Open PS2 Loader
  path1_OSDSYS_ITEM_3 = mass:/UPDT/OPL.ELF
  path2_OSDSYS_ITEM_3 = mc?:/APPS/OPNPS2LD.ELF
  path3_OSDSYS_ITEM_3 = mc?:/OPL/OPNPS2LD.ELF
  ```
  - At most 3 paths per item.
  - Launch keys are `LK_<Key>_E1..E3`, where Key is Auto, Circle, Cross, Square, Triangle, L1–R3, Up/Down/Left/Right, Start or Select.
  - Special targets: `OSDSYS`, `FASTBOOT`, `POWEROFF`.
- **Stock 1.953** [V]: `LK_L1_E1..E3 = mass:/OPL.ELF, mc?:/OPL/OPNPS2LD.ELF, mc?:/APPS/OPNPS2LD.ELF`. It has the item 3 paths above too: `mass:/UPDT/OPL.ELF` is path1 in all five stock configs, 1.953 and 1.963–1.966, and the OPL the installer puts on the card is v1.0.0 (research/17 §4.1).
- **MASS package** (FMCB itself on USB) [V]:
  - `mass:/APPS/OPNPS2LD_LATEST.ELF` and `hdd0:+OPL:pfs:/APPS/OPL/OPNPS2LD_LATEST.ELF`;
  - `LK_L1`: `mass:/APPS/OPL.ELF`.
- **Releases** [V]: no stable release. Pre-release `latest` (2025-01-01) has `FMCB-1953.7z … FMCB-1966.7z`, all digest `null`. There are also `FreeMcBoot-19xx.mc2` 8 MiB card images for MemCard PRO 2. Licence: **none**.

### 2.3 OSDMenu (pcm720/OSDMenu), the modern FMCB 1.8 successor

- v1.3.0 (2026-06-30), asset `osdmenu-v1.3.0.zip`, digest set [V].
- **Install** [R README]: `osdmenu.elf` goes to `mc?:/BOOT/`, or you paste `SYS_OSDMENU.psu`. Config is `mc?:/SYS-CONF/OSDMENU.CNF`, also `xfrom:/osdmenu/OSDMENU.CNF`. The patcher does **not** read config from USB.
- **CNF** [R/V]:
  - Keys are FMCB-1.8-style `name_OSDSYS_ITEM_N`, `path<k>_OSDSYS_ITEM_N` (unlimited paths) and `arg_OSDSYS_ITEM_N`.
  - Up to 200 entries; no `LK_*` keys.
  - The stock example has **no OPL entry**. It shows `mmce?:/apps/wLaunchELF.elf` and `usb:/apps/wLaunchELF.elf`.
- **HOSDMenu (HDD-OSD 1.10U)**: binary `hdd0:__system:pfs:/osdmenu/hosdmenu.elf`, config `hdd0:__sysconf:pfs:/osdmenu/OSDMENU.CNF` [R].
- Licence is mixed [V]:
  - `patcher/` is under the "Free McBoot License": non-commercial, derivatives stay under it;
  - the launcher and the rest are AFL-3.0.

### 2.4 PS2BBL (israpps/PlayStation2-Basic-BootLoader)

- GPL-3.0 [V].
- Releases [V]:
  - stable v1.2.0 (2023-10-05), digest `null`;
  - rolling pre-release `latest` (2026-02-11): `PS2BBL.7z` (digest set). It contains `PS2/`, `PS2_MX4SIO/`, `PS2_MMCE/`, `PS2_HDD/`, `PSX/`, `PSX_MMCE/` and a sample `SYS-CONF/PS2BBL.INI`.
- **Config search, first match wins** [V `src/main.c`, `include/common.h`]:
  1. `./CONFIG.INI` (CWD)
  2. PSX: `mc?:/SYS-CONF/PSXBBL.INI`
  3. MMCE: `mmce1:/PS2BBL/PS2BBL.INI`, then `mmce0:/PS2BBL/PS2BBL.INI`
  4. XFROM: `xfrom:/PS2BBL/CONFIG.INI`
  5. HDD: `hdd0:__sysconf:pfs:/PS2BBL/CONFIG.INI`
  6. MX4SIO: `massX:/PS2BBL/CONFIG.INI`
  7. `mass:/PS2BBL/CONFIG.INI`
  8. `mc1:/SYS-CONF/PS2BBL.INI`
  9. `mc0:/SYS-CONF/PS2BBL.INI`

  **A config on USB overrides the memory-card config.**
- **Shipped `PS2BBL.INI`** [V]:
  - `LK_AUTO_E1..E3 = mass:/APPS/OPNPS2LD.ELF, mc?:/APPS/OPNPS2LD.ELF, mc?:/OPL/OPNPS2LD.ELF`.
  - Keys are uppercase and matched by exact `strcmp`.
  - Commands: `$CDVD`, `$CDVD_NO_PS2LOGO`, `$OSDSYS`, `$CREDITS`, `$RUNKELF:<path>`, `$HDDCHECKER`.
  - Emergency mode: R1+START runs `mass:/RESCUE.ELF`.

### 2.5 wLaunchELF

- **Upstream** (AFL-2.0 per LICENSE; GitHub says NOASSERTION):
  - The real distribution is pre-release `latest`: one asset `BOOT.ELF`, 2026-10-13, digest set.
  - `/releases/latest` is an empty 2019 release.
  - `LAUNCHELF.CNF` is read from the ELF's directory, then `mcN:/SYS-CONF/LAUNCHELF.CNF`.
  - Keys `LK_auto_E1`, `LK_Circle_E1`, … [V].
- **israpps/wLaunchELF_ISR** ("4.43x_isr", no licence): MMCE and MX4SIO builds, e.g. `BOOT-EXFAT-DS34-MMCE.ELF` and `BOOT-EXFAT-DS34-MX4SIO.ELF` [V]. This is the practical way for a user to copy ELFs between `mmce0:`, `mc0:` and USB.

### 2.6 FreeDVDBoot, FreeHDBoot, HOSDMenu

- **FreeDVDBoot** (CTurt): no licence, releases or tags; prebuilt ISOs live in the repo [V].
  - Slim and 3.03–3.11 payloads boot `cdrom0:\VIDEO_TS\VTS_02_0.IFO` as an ELF. The shipped one is uLaunchELF.
  - Phat 1.00–2.13 payloads load an ELF from raw ISO offset `0x5BB000` [V payload sources].
  - The console language must be English [R].
  - YAOPLM has no role here beyond documentation.
- **FreeHDBoot** [V configs]:
  - `hdd0:__sysconf:pfs:/FMCB/{FREEHDB.CNF,BOOT.ELF,FMCB_CFG.ELF}`;
  - OPL at `hdd0:PP.FHDB.APPS:pfs:/OPL/OPNPS2LD.ELF`.
  - OSDMenu's MBR and HOSDMenu replace it for HDD-OSD setups [R].
  - APA work is out of scope until HDD-1.

### 2.7 MMCE cards (SD2PSX / sd2psXtd, MemCard PRO 2, PicoMemcard+)

- **Protocol** [V `ps2-mmce/mmceman` docs and source]:
  - "Multi-purpose Memory Card Emulator", defined by `ps2-mmce/mmceman` (MIT). It is not in upstream ps2sdk.
  - Ping returns the product: 1 SD2PSX, 2 MemCard PRO2, 3 PicoMemcard+, 4 PicoMemcardZero.
  - Modules:
    - **MMCEMAN**: iomanX device `mmce`, units `mmce0:` = slot 1 and `mmce1:` = slot 2;
    - **MMCEDRV**: about 12 KB in-game streaming driver;
    - **MMCEIGR**: on IGR, switches back to the boot card.
  - It is a **file-system passthrough**: the card's MCU mounts FAT32/exFAT, and lseek64 exists. There is no fragment limit.
  - Measured speed is about 1350–1700 KB/s, against 900/750 KB/s for USB [R Neutrino README].
- **sd2psXtd firmware** (GPL-3; stable 1.4.0 2026-07-08; release `.uf2` assets with digests) [V].
  - MMCE FS, Game ID switching and Game2Folder came in 1.0.0 (2025-01-31).
  - **SD layout** [V `src/ps2/ps2_cardman.c`, `game_db.c`, `card_config.c`]:
    ```
    .sd2psx/settings.ini            firmware settings ([General] [PS1] [PS2]: Autoboot, GameID, CardSize, Variant, MaxCardIdx)
    .sd2psx/Game2Folder.ini         [PS1]/[PS2] ID=FolderName overrides
    MemoryCards/PS2/BOOT/BootCard-<ch>.mcd   boot card (FMCB/PS2BBL live inside this image); legacy BootCard.mcd
    MemoryCards/PS2/Card<N>/Card<N>-<ch>.mcd generic cards
    MemoryCards/PS2/<ID>/<ID>-<ch>.mcd       per-game cards; <ID> = "SLUS_200.62" -> "SLUS-20062", parent (disc 1) ID
    MemoryCards/PS1/...                      same scheme for PS1
    ```
  - Images are raw: 512-byte pages, no ECC.
  - The official SD builder (`sd2psXtd.github.io/sd_builder`) generates boot cards from bundled FMCB 1.953–1.966 and PS2BBL images [V].
- **MemCard PRO 2** (8BitMods; closed firmware) [R manual]:
  - microSD or USB drive, FAT32 or exFAT.
  - Firmware files `mcp2.bin` and `os/` sit at the SD root.
  - PS2 cards are `PS2/<GameID>/<GameID>-<ch>.mc2`, PS1 cards `PS1/<GameID>/<GameID>-<ch>.mcd`. The exact naming is [U] (craigshaw/ps2mcs).
  - Templates are in `PS2/Templates/`.
  - The manual recommends OPL from the ps2-mmce fork: MMCE Start Mode Auto, slot Auto, IGR = boot-card slot, prefix empty (SD root).
  - Which PRO 2 firmware first added MMCE is not documented [U].
- **How the card learns the game** [V]:
  1. MMCE devctl `0x8` with the ID string (OPL fork, RiptOPL, NHDDL). OPL then polls status until "not busy", for up to 15 s.
  2. The firmware watches writes to `/B?DATA-SYSTEM/history` on the emulated card (sd2psXtd `ps2_history_tracker.c`). Upstream OPL writes that history on every launch (`src/system.c:817` → `AddHistoryRecordUsingFullPath`). So **per-game cards also work with stock OPL loading from USB, HDD or SMB.**
- **Booting** [R]: the card cannot start an ELF itself. It presents an emulated memory card, and the boot card (Autoboot=ON) holds FMCB or PS2BBL. That launcher then runs an ELF from:
  - inside the boot-card image (`mc0:/APPS/OPNPS2LD.ELF`), or
  - the SD card via `mmce?:` (PS2BBL MMCE build, OSDMenu, wLE_ISR MMCE).
- **Caveat** [V `nhddl src/devices/mmce.c`; R RiptOPL install page]: a Game ID switch changes what `mc0:` contains. Loaders refuse or skip the switch when their own files (e.g. `mc0:/neutrino/`) are on that emulated card. That is why Neutrino should live on `mmceN:/neutrino/` for MMCE setups.

### 2.8 MX4SIO

- SD-card adapter in **memory-card slot 2** (`mx4sio_bd`, `PORT_NR 3`). BDM driver name `sdc`; it appears as a `massN:` unit next to USB [V ps2sdk, upstream `src/bdmsupport.c`].
- Present in OPL **v1.1.0** (`enable_mx4sio`, commit 1743719 from 2021-07-21), but FAT32-only there. exFAT came in 1.2.0 beta rev1880 [V/R].
- Community advice for 1.1.0 is Manual BDM start mode [U].
- It is a block device: the PS2 parses FAT itself, so the OPL BDM budget of 64 fragments applies [R]. Many SD cards are incompatible [R Neutrino README].
- Booting from it needs:
  - PS2BBL MX4SIO build (`massX:/…`; config `massX:/PS2BBL/CONFIG.INI`);
  - OSDMenu (`mx4sio:/…`); or
  - wLE_ISR MX4SIO.
  - FMCB cannot [U].

### 2.9 Where to put the loader ELF, per boot method (recommended presets)

| Boot method | Device YAOPLM writes to | Path on the PS2 | Path on the PC-mounted device | Works with stock config? |
|---|---|---|---|---|
| FMCB 1.966 | USB (FAT32) | `mass:/UPDT/OPL.ELF` | `<usb>/UPDT/OPL.ELF` | **Yes**, path1 of item 3; overrides the MC copy |
| FMCB 1.953 | USB | `mass:/OPL.ELF` (`LK_L1_E1`) | `<usb>/OPL.ELF` | Yes (L1 key) |
| FMCB, any | MC | `mc?:/APPS/OPNPS2LD.ELF` | — (physical card, out of scope) | Yes |
| PS2BBL (stock INI) | USB | `mass:/APPS/OPNPS2LD.ELF` | `<usb>/APPS/OPNPS2LD.ELF` | Yes (`LK_AUTO_E1`) |
| PS2BBL MMCE / OSDMenu | MMCE SD | `mmce?:/APPS/OPNPS2LD.ELF` (any path) | `<sd>/APPS/OPNPS2LD.ELF` | No: the user adds the path to `PS2BBL.INI` / `OSDMENU.CNF` |
| PS2BBL MX4SIO | MX4SIO SD | `massX:/APPS/OPNPS2LD.ELF` | `<sd>/APPS/OPNPS2LD.ELF` | No: the user adds the path |
| Launch from OPL's Apps page (any loader) | any game device | `<root>APPS/<folder>/<ELF>` + `title.cfg` | `<root>/APPS/APP_RIPTOPL/RIPTOPL.ELF` etc. | Yes, needs no launcher config |
| RiptOPL package style | USB/MMCE | `mass:/APPS/APP_RIPTOPL/RIPTOPL.ELF` | `<root>/APPS/APP_RIPTOPL/` | No: add the launcher path. Settings are created beside the ELF |
| NHDDL | any | anywhere; `nhddl-<mode>.elf` forces a mode | e.g. `<root>/APPS/NHDDL/nhddl.elf` + `<root>/neutrino/` | No |

Notes:
- The FMCB stock path `mass:/UPDT/OPL.ELF` is odd but real [V]. It makes USB the primary update channel, so the memory card never needs to be touched.
- A loose `APPS/OPNPS2LD.ELF` without `title.cfg` is ignored by OPL's own Apps scan, which only takes sub-folders with `title.cfg` (research/02 §11) [V].

### 2.10 The owner's setup: FMCB card + MMCE card

| Option | What runs where | What YAOPLM can do now | Limits |
|---|---|---|---|
| **A. Physical FMCB card in slot 1; MMCE card in slot 2 as game storage** | FMCB → `mass:/UPDT/OPL.ELF` (USB) or `mc0:/APPS/OPNPS2LD.ELF` → **RiptOPL or the MMCE fork** reads games from `mmce1:` | Install the loader ELF to USB (`UPDT/OPL.ELF`); prepare the SD card (folders, games, art, `neutrino/`) | Upstream OPL cannot see MMCE. The MMCE fork's `conf_opl.cfg` lives on mc0 (OK, a physical card). Per-game cards switch on slot 2 only |
| **B. MMCE card is the boot card (FMCB inside `BootCard-1.mcd`)** | FMCB from the emulated card → OPL inside the image (`mc0:/APPS/OPNPS2LD.ELF`) or on USB | Prepare the SD card. Install the ELF to USB. Editing the `.mcd` image is a later feature (§6.7) | FMCB cannot launch `mmce0:/…ELF`. A Game ID switch replaces the boot card until IGR (MMCEIGR) |
| **C. Swap FMCB for PS2BBL-MMCE or OSDMenu in the boot card** | Launcher → `mmce?:/APPS/RIPTOPL/RIPTOPL.ELF` on the SD card | Everything on the SD card: ELF, settings next to it (RiptOPL), games, `neutrino/` | Changing the boot card is a user action (sd_builder, KELFBinder). Out of scope for us |

---

## 3. What OPL needs on a fresh device

### 3.1 Folder set and when it is created

- `sbCreateFolders(prefix, createDiscImgFolders)` creates `CFG THM LNG ART VMC CHT APPS`, plus `CD DVD` when the second argument is 1 [V `src/supportbase.c:842`].
- **When OPL calls it** [V]:
  - **BDM** (USB, MX4SIO, iLink, ATA): in `bdmNeedsUpdate()` every time a BDM device is (re)detected and its page refreshes (`src/bdmsupport.c:250`). This needs `usb_mode` ≠ 0 (off) and, for MX4SIO/iLink/ATA, `enable_*`=1.
  - **SMB**: after a successful share connect (`src/ethsupport.c:268`).
  - **APA HDD**: in `hddInitModules()` without `CD/DVD` (`src/hddsupport.c:58`).
  - **MMCE fork**: in `mmceNeedsUpdate()` (`src/mmcesupport.c:143` on `mmce_support`).
- **Defaults** [V `src/opl.c` `setDefaults`]: every start mode is `START_MODE_DISABLED` and `enable_ilink`, `enable_mx4sio`, `enable_bdm_hdd` are 0. So **a fresh OPL creates nothing until the user enables a device**.
  - RiptOPL behaves the same and also requires `enable_usb`.
  - wOPL v1.2 enables USB on BDM "by standard" [R].
- **Consequence**: OPL does not need pre-created folders, but a manager that creates them (LIB-3) lets games appear on the first scan.
  - `POPS/` and `EMBER/` are never created by OPL.
  - RiptOPL wants `POPS/` and `EMBER/` **at the device root even when a prefix is set** [V `mmcesupport.c`; R].

### 3.2 Prefixes

| Key | Applies to | Built path | Source |
|---|---|---|---|
| `usb_prefix` | **all** BDM units: USB, MX4SIO, iLink, ATA | `mass<N>:<prefix>/`, e.g. `usb_prefix=OPL` → `mass0:OPL/CD`. Empty → `mass0:` | [V `bdmUpdateDeviceData`, `bdmFindPartition`] |
| `eth_prefix` | SMB | `smb0:<prefix>\` | [V research/02 §3] |
| `mmce_prefix` (MMCE fork, RiptOPL, wOPL) | MMCE | `mmce<N>:/<prefix>` + `CD`, so it **needs a trailing `/`** in the fork | [V] |
| APA `hdd_partition` | APA | `pfs0:` (`+OPL`) or `pfs0:OPL/` when the partition name has no `+` | [V research/02 §3] |

### 3.3 Where the global config lives

- **Upstream OPL** [V `src/opl.c` `_loadConfig`, `tryAlternateDevice`, `checkLoadConfigBDM`, `_saveConfig`]:
  1. `mc?:OPL/conf_opl.cfg`, the memory card, read first.
  2. If not there: the boot device (CWD `mass*`/`hdd*`), then any BDM unit (`mass<N>:<usb_prefix>/conf_opl.cfg`, then `mass<N>:conf_opl.cfg`), then the APA `pfs0:conf_opl.cfg`.
  3. **When a config is found on BDM, OPL forces `usb_mode=2` (auto); when found on ATA it also sets `enable_bdm_hdd=1`.**
  4. With no config anywhere it saves to the MC if one is inserted, else to `mass0:`, else to the HDD.

  So a `conf_opl.cfg` that YAOPLM puts on a USB stick is **ignored whenever the memory card already has one**. That is the owner's FMCB case.
- **MMCE fork**: the same. **No `mmce:` config location** [V]. The config lives in `mc?:/OPL/` (the boot-card image for MMCE-boot users), or on USB/HDD.
- **RiptOPL**: beside the ELF (CWD) first, then the fallback chain in §1.3. It can live on `mmceN:/` [V].
- **wOPL HEAD**:
  - the boot dir's `wopl_boot.cfg` names `boot.config_dir`;
  - files `wopl_*.cfg`;
  - APA `hdd0:__common/wOPL/conf_hdd.cfg`;
  - partition `+wOPL` (not `+OPL`) [V/R].
- **NHDDL**: `nhddl.yaml`, optional (§1.6).

### 3.4 Keys that enable devices

| Key | Loader | Values | Default |
|---|---|---|---|
| `usb_mode` (= BDM) | all OPL family | 0 off / 1 manual / 2 auto | 0 |
| `enable_usb` | RiptOPL | 0/1 | 0 |
| `enable_mx4sio` | OPL ≥1.1.0, forks | 0/1 | 0 |
| `enable_ilink` | OPL ≥1.1.0, forks | 0/1 | 0 |
| `enable_bdm_hdd` | OPL 1.2 beta, forks | 0/1 | 0 |
| `hdd_mode`, `eth_mode`, `app_mode` | all | 0/1/2 | 0 |
| `mmce_mode`, `mmce_slot`, `mmce_prefix`, `mmceigr_slot`, `mmce_gameid` | MMCE fork, RiptOPL (+ `mmce_wait_cycles`, `mmce_use_alarms`), wOPL | see §1.4 | `mmce_mode=0` |
| `enable_udpbd`, `net_start_mode`, `udpfs_access`, `enable_pccf`, `enable_dvr`, `fav_mode` | RiptOPL | — | off |
| `mode:` in `nhddl.yaml` | NHDDL | `ata`, `mx4sio`, `usb`, `udpfs`, `ilink`, `mmce`, `hdl` | all BDM |

All [V] from each `include/config.h` and `src/opl.c`, except the `nhddl.yaml` row [R].

### 3.5 What this means for a PC tool

- Creating the folder set is safe and useful. Seeding **loader config** on the device mostly is not:
  - upstream and the MMCE fork prefer the memory card;
  - PS2BBL prefers a USB `CONFIG.INI` over the card's, so we would silently override the user;
  - the FMCB `mass:/FREEMCB.CNF` priority is unknown.
- **Exception**: for RiptOPL installed by YAOPLM into a folder (e.g. `APPS/APP_RIPTOPL/`), its `settings_riptopl.cfg` lives in that same folder. Seeding it (`usb_mode=2`, `enable_usb=1`, `mmce_mode=2`, …) is predictable. It should be opt-in [D].
- The user must still **turn the device on in the loader's menu** on first boot. YAOPLM should print the exact steps for the chosen loader and device (§6.4).

---

## 4. RiptOPL artwork: cross-check of the pasted spec

The owner's spec (reportedly from the OPLNova psx-place thread; the thread is Cloudflare-blocked, so its origin is unverified) checked against RiptOPL code at `40c52ce`, its docs site and OrbitPS2 at `cdd590a`.

### 4.1 Claim by claim

| Claim in the pasted spec | Verdict | Evidence |
|---|---|---|
| "Format: 8-bit PNG required" | **True only as "≤8 bits per channel".** Indexed is *not* required | `src/textures.c`: `if (bitDepth > 8) return ERR_BAD_DEPTH`. Colour types RGBA→`CT32`, RGB→`CT24`, palette 4-bit→`T4`, 8-bit→`T8`; grey promoted to RGB. Upstream instead calls `png_set_strip_16` [V]. RiptOPL's own `ART/RIPTOPL.ELF_*.png` are **8-bit RGBA** [V, PNG headers read] |
| PS2 key `<DISC_ID>_<TYPE>.png` | True | Art key = `itemGetStartup` (`src/themes.c` `getGameImageTextureEx`); path `"%s%s/%s_%s"` in `bdmGetImage` [V] |
| VCD key `<VCD_FILENAME>_<TYPE>.png`, ".VCD stripped" | True | `vcdFillGameList` stores the basename as both name and startup; `bdmGetGameStartup` returns `g->name` in PS1 view [V]. A disc-ID fallback exists **for CFG only** (`supportbase.c` ~1217). `docs/VCD.md` and the site's ps1-vcd page also claim it for art, but `bdmGetImage` says "there is no second directory … a miss is simply no cover" [V] |
| EMBER key `<FOLDER_NAME>_<TYPE>.png` | True | `cuesupport.c:989-990` name = startup = folder `d_name` [V]. The site's `install.html` table says "The PS1 disc ID", which is wrong; `ps1-vcd.html` says folder name [R, inconsistent] |
| APP key `<ELF_FILENAME>.ELF_<TYPE>.png` | True | `appSetArtSource` → `appGetBoot` (file name of `boot`), the same as upstream [V]. RiptOPL looks app art up **only on the app's own device** (`oplGetAppImageByMode`); upstream searches all devices [V] |
| Examples `SLUS_203.12_COV.png`, `Crash Bandicoot_COV.png`, `Spyro 2 (Ripto's Rage)_COV.png`, `RIPTOPL.ELF_COV.png` | Consistent | `RIPTOPL.ELF_COV.png` ships in the package [V]; Spyro example on the docs site [R] |
| Dimensions table | **OrbitPS2's normalisation table, not a RiptOPL requirement** | Identical to `EXPECTED_DIMENSIONS` in OrbitPS2 `src/utils/png-artwork.ts` [V]. RiptOPL only enforces ≤1024 px per side and a ≤720×512×4-byte texture (`texSizeValidate`) [V]. Its own app art is 140×200 / 64×64 [V] |
| PS1/VCD/EMBER/**APP** COV 400×400 | Orbit convention | RiptOPL's Coverflow draws apps and PS1 in a **square** box (184×184) and PS2 in 184×256 [R `coverflow.html`]. Square art fits; nothing enforces it |
| APA HDD: `hdd0:/+OPL/ART/` or `hdd0:/__common/OPL/ART/` | True | `docs/VCD.md`; matches upstream `hddsupport.c` (+OPL root, or `OPL/` prefix on `__common`) [V/R] |
| PSBBN Game Collection `hdd0:/PP.<entry>/res/jkt_001.png`; HDD-OSD `list.ico`/`del.ico`/`icon.sys` | **Not RiptOPL art.** These are PSBBN / HDD-OSD (HOSDMenu) assets in APA partitions | No `jkt_001` in RiptOPL. These are PSBBN Definitive / HDD-OSD conventions; [U] as to exact paths |
| exFAT: OPL/NHDDL `ART/<ID>_COV.png`, POPSLoader `ART/<VCD>_COV.png`, apps `ART/<ELF>_COV.png` | True for OPL and NHDDL | NHDDL README: 140×200 `ART/<ID>_COV.png` [R]. The POPSLoader row is outside RiptOPL; POPSLoader also uses `POPS/ART/` [R `docs/VCD.md`] |
| "Only COV and ICO use doubled dimensions" | True relative to OPL Manager sizes | See 4.2 |

### 4.2 What "Only COV and ICO use doubled dimensions" means

| Type | OPL Manager standard (research/02 §7) | Pasted / OrbitPS2 | Factor |
|---|---|---|---|
| PS2 COV | 140×200 | 280×400 | **2×** |
| PS1 / app COV | 200×200 | 400×400 | **2×** |
| ICO | 64×64 | 128×128 | **2×** |
| COV2 | 242×344 / 222×200 | same | 1× |
| LAB | 18×240 / 12×200 | same | 1× |
| LGO | 300×125 | same | 1× |
| SCR, SCR2 | 250×188 | same | 1× |
| BG | 640×480 | same | 1× |

Why [D]:
- RiptOPL's Coverflow and PSBBN-Flow themes draw the selected cover enlarged. Boxes are 184×256 or 184×184 before the centre-scale factor, so a 140×200 cover would be upscaled.
- `ICO` is RiptOPL's "disc art" (`enable_discart`, `themes.c` `isHiddenArtworkCache`), also drawn larger.
- The other types are drawn at or below their standard size.
- The classic built-in theme still assumes 140×200 (`THM_COVER_W/H` in `themes.c`), so doubled art is only downscaled there.

### 4.3 Is 8-bit indexed PNG required?

- **No** [V]: RiptOPL decodes RGB and RGBA. Its own release art is RGBA.
- **What is required**: bit depth ≤8 per channel. A 16-bit PNG fails with `ERR_BAD_DEPTH` on RiptOPL; upstream OPL would load it.
- **Why OrbitPS2 insists on indexed** [V `png-artwork.ts`, `artwork.service.ts`]:
  - Its resizer (`resizeIndexedPng`, nearest-neighbour on palette indices) only handles colour type 3.
  - It skips art-DB candidates that are not indexed.
  - It applies the RiptOPL table to every download.
  - This is a tool limitation, presented to users as a RiptOPL rule.
- **Why indexed is still a good idea** [D]:
  - Palette textures upload as CLUT `T8`.
  - A 280×400 cover costs 112,000 B as T8 versus 448,000 B as RGBA (`CT32`).
  - With covers cached at several sizes in 32 MB EE RAM, that matters for doubled art.
  - Use an 8-bit indexed PNG with tRNS when the image allows it, and fall back to RGB(A) at 8 bits per channel.

### 4.4 Differences from research/05 §3.3

1. **Pixel format.** 05 said "OrbitPS2 enforces 8-bit indexed; whether RiptOPL rejects RGBA is unverified". **Resolved**: RiptOPL accepts RGBA and rejects only >8-bit depth.
2. **VCD identity.** 05 said "a serial-named file is accepted only if the VCD name itself begins with that serial". At `40c52ce` the art path has **no ID fallback**; only CFG has one. The docs disagree with the code.
3. **Apps.** 05 had "per OrbitPS2; not cross-checked". Now verified: `<boot ELF file name>` including `.ELF`, looked up on the app's own device only.
4. **Suffixes.** 05 listed a `DISC` suffix and Orbit's `COV3`. The RiptOPL code knows no `DISC`; disc art is `ICO`.
   - Built-in themes use `COV`, `ICO`, `BG`, `SCR`, `SCR2`, and `LGO` behind a toggle (`misc/*.cfg`, `themes.c`).
   - `COV2` and `LAB` are used only by custom themes.
   - The docs site states that the suffix set is not fixed by the loader [R].
5. **`art.tar`** [V `src/tar.c`]: matches 05 (ustar, port from wOPL by mystyq, `art_cache.bin` sidecar, opt-in `enable_art_tar`). New details:
   - archives are probed on `mass0–7:`, `mmce0/1:`, `pfs0:` and `smb0:`;
   - `CFG/cfg.tar` and `CHT/cht.tar` are defined, but only ART is wired.
6. **Dimensions.** Unchanged values, but they are OrbitPS2's convention, not RiptOPL's (§4.1).

### 4.5 Recommendation for ART-6

- Keep one ART pipeline:
  - always PNG;
  - 8 bits per channel, never 16 (fix this for **all** profiles, since it is free);
  - ≤1024 px.
- Add a **size preset** to the RiptOPL profile:
  - "Standard (OPL Manager sizes)", the default and safe everywhere;
  - "RiptOPL HD (2× COV/ICO)".
- Add a **pixel-format option**: "Indexed 8-bit (smaller, recommended for RiptOPL HD)" or "RGB(A)". Quantise ourselves, e.g. with the `imagequant`/`color_quant` family. Check the licence before adding it; libimagequant is reportedly GPL-3.0-or-later, which would suit us [U].
- Art keys per row kind:
  - PS2 → ID;
  - VCD → VCD stem;
  - Ember → folder;
  - app → `boot` ELF file name.
- For RiptOPL apps, put the art on the **same device** as the app.

---

## 5. Legal and licensing

| Component | Licence | May YAOPLM bundle it? | May YAOPLM download it at runtime and install it? |
|---|---|---|---|
| OPL (upstream, MMCE fork, RiptOPL, wOPL, sOPL) | AFL-3.0 [V] | Legally possible (OSI licence; mere aggregation with GPL-3). The FSF lists AFL as GPL-incompatible for *combining code* [U] | **Yes**. Preferred |
| NHDDL, Neutrino | AFL-3.0 [V] | as above | Yes |
| PS2BBL | GPL-3.0 [V] | yes | Yes (but installing a bootloader is out of scope) |
| wLaunchELF | AFL-2.0 file; GitHub NOASSERTION [V] | avoid | Yes, if ever needed |
| wLaunchELF_ISR, FreeMcBoot-Installer, FreeDVDBoot | **no licence** [V] | **No** (all rights reserved) | Link only |
| FMCB 1.9xx / OSDMenu patcher | "Free McBoot License": non-commercial, derivatives under the same [V] | No | Out of scope |
| POPSTARTER (inside RiptOPL `POPS/`) | Closed freeware, no licence file [V absence] | No | Only as part of the user-requested RiptOPL package, unmodified, from upstream |
| Ember (inside RiptOPL `EMBER/`; `Gageformer/Ember`) | "Ember Public Beta Testing Licence 1.0" [V `EMBER/LICENSE-BETA.txt`] | Bundling allowed if unmodified, complete, with the licence file, credited to Gageformer with a link, and not sold | Yes, unmodified with its licence |
| POPS.ELF, PS1/PS2 BIOS | Sony | **Never** | **Never** (ADR-0007) |
| sd2psXtd firmware | GPL-3 [V] | n/a | Out of scope (firmware flashing) |
| MemCard PRO 2 firmware | proprietary | n/a | Out of scope |

Rules for YAOPLM [D, following ADR-0007]:
1. **Download on demand from the upstream GitHub release.** No mirror, no re-hosting, and no binaries in our installers or Docker image. This keeps us out of the distributor role, avoids the AFL/GPL combination question, and respects unlicensed packages.
2. **Attribution in the UI.**
   - Show the loader's name, authors, licence (SPDX), repo URL and release tag before installing.
   - Keep the `LICENSE`/`CREDITS` files that come with OPL archives in the cache. Offer to copy them next to the installed ELF (cheap, and good practice for AFL §6 notices).
   - Credit Ember to Gageformer with a link whenever we install the RiptOPL full package.
3. **Install unmodified bytes only.**
   - Never patch an ELF.
   - Renaming the file is fine (`OPNPS2LD-v1.2.0-…ELF` → `OPNPS2LD.ELF`).
4. **GitHub API etiquette** (consistent with CLAUDE.md rule 3):
   - send `User-Agent: YAOPLM/<version> (+repo URL)` (GitHub requires a UA) [R GitHub docs];
   - unauthenticated REST limit is 60 requests per hour per IP [R], so use `ETag`/`If-None-Match` (304s are free) [R];
   - cache release lists for hours;
   - asset downloads go through `browser_download_url` to GitHub's CDN [U: not counted against the REST limit];
   - an optional user token raises the limit; store it in the OS keychain, never in config files.
5. **Show warnings.**
   - Forks' disclaimers, e.g. the upstream README note that RiptOPL "heavily uses LLM tools".
   - Pre-release status of rolling builds.
   - "Experimental" labels (OPL-Evolution, RiptOPL RA).

---

## 6. Proposal: YAOPLM requirements

> **Adopted 2026-10-22 in condensed form:** SPEC §5.10 uses LDR-1…5 and BOOT-1, plus CFG-1 (wOPL format) and DEV-3 (emulator SD backups). The IDs suggested below are this report's draft numbering and do not match the SPEC one to one.

### 6.1 New requirements (suggested IDs, `LDR` = loader, `BOOT` = boot guidance)

| ID | Requirement | Stage |
|---|---|---|
| LDR-1 | **Loader catalogue.** A bundled, curated data file (our own data, no licence issue) with one entry per loader and channel. Fields: repo, tag or "latest release", asset pattern, archive member, licence, profile, settings files, backends, notes. It is extended by app updates. Users can add a custom entry (repo + asset regex) | v0.2 |
| LDR-2 | **Download, verify, cache.** Releases API with ETag; download the asset; verify the GitHub `digest` when present and the size always. For assets without a digest, use the catalogue's pinned SHA-256 (e.g. OPL v1.1.0, computed once by maintainers) or TOFU with a warning. Extract `.7z`/`.zip` safely: no path traversal, no symlinks, size caps. Cache under the app data dir keyed by sha256. Never execute anything | v0.2 |
| LDR-3 | **Detect the profile** of a device from settings and folders (§1.9 step 5) and **the installed loader and version** by our manifest, then SHA-256, then `title.cfg` `Version=`, then names. Profile detection is part of LIB-1 | MVP (profile), v0.2 (version) |
| LDR-4 | **Install a loader ELF** to a selected device and path from a **boot-method preset** (§2.9) or a custom path. Plan → confirm → atomic write (temp + rename). Keep the previous ELF as a backup. Record it in the device manifest. Never write outside the selected root | v0.2 |
| LDR-5 | **Update check and update**: compare the installed digest with the channel's current digest and show the release date and notes. Rolling channels are pinned by digest | v0.2 |
| LDR-6 | **Prepare a device for a loader**: folder set (LIB-3) under the right prefix; root-level `POPS/` and `EMBER/` for RiptOPL; `neutrino/` for NHDDL and RiptOPL; optional `LNG/` packs from the LANGS archive. Optional, opt-in seeding of RiptOPL `settings_riptopl.cfg` next to the ELF | v0.2 (folders), v0.3 (companions, seeding) |
| LDR-7 | **MMCE SD card as a storage target**. Detect it by the presence of `.sd2psx/` or `MemoryCards/` (sd2psX) or `mcp2.bin` + `os/` (MCP2). Block writes into those reserved paths. Warn when the profile is upstream OPL ("cannot see MMCE"). When showing an `mmce_prefix` value, always end it with `/`. Recommend exFAT and ISO/ZSO; refuse to create `ul` sets on MMCE [D] | MVP (guard rails), v0.2 (profile rules) |
| LDR-8 | **NHDDL + Neutrino install**: `nhddl.elf` (optionally renamed `nhddl-<mode>.elf`), `neutrino/` at the device root, optional `nhddl.yaml` with `mode:` | v0.3 |
| LDR-9 | **wOPL libconfig awareness**: detect libconfig-syntax `CFG/*.cfg` and `CFG/*.info`. Treat them read-only with a clear warning; never rewrite them as key=value. Show a per-device "mixed CFG formats" check in LIB-4 | MVP (detect + protect), later (editing) |
| BOOT-1 | **Boot guide**: for the chosen launcher and device, show the exact path the PS2 will use and the text to add, e.g. an FMCB Configurator item, a `PS2BBL.INI` line or an `OSDMENU.CNF` item. Show the first-boot steps inside the loader (enable device, start mode, save). Copyable; YAOPLM does not write launcher configs | v0.2 |
| BOOT-2 | **Install into an MMCE boot-card image** (`MemoryCards/PS2/BOOT/BootCard-<n>.mcd`, raw 512-byte pages, no ECC): copy an ELF into `mc0:/APPS/…` inside the image. Needs a PS2 memory-card filesystem writer shared with VMC-1/VMC-2. Image backup first | later |

### 6.2 Catalogue entry (example)

```toml
[[loader]]
id        = "opl-beta"
name      = "Open PS2 Loader 1.2.0 beta (rolling)"
repo      = "ps2homebrew/Open-PS2-Loader"
release   = { tag = "latest", prerelease = true, mutable = true }
asset     = { name = "OPNPS2LD.ELF" }                       # bare ELF
companions= [{ name = "OPNPS2LD.7z", keep = ["LICENSE", "CREDITS", "README.md"] },
             { name = "OPNPS2LD-LANGS.7z", optional = true }]
licence   = "AFL-3.0"
profile   = "opl-1.2"
elf_name  = "OPNPS2LD.ELF"
settings  = ["conf_opl.cfg"]
backends  = ["usb", "mx4sio", "ilink", "ata-exfat", "apa", "smb"]

[[loader]]
id        = "opl-1.1.0"
repo      = "ps2homebrew/Open-PS2-Loader"
release   = { tag = "v1.1.0", mutable = false }
asset     = { name = "OPNPS2LD-v1.1.0.7z", member = "OPNPS2LD-v1.1.0.ELF" }
sha256    = "<pinned by maintainers>"                         # API digest is null
profile   = "opl-1.1"

[[loader]]
id        = "riptopl"
repo      = "NathanNeurotic/Open-PS2-Loader"
release   = { tag = "rolling", mutable = true }
asset     = { name = "RIPTOPL.ELF" }                         # OFFICIALROLLING flavour
package   = { pattern = "^RIPTOPL-v[0-9.]+-Beta-\\d+\\.zip$", app_dir = "APPS/APP_RIPTOPL" }
profile   = "riptopl"
settings  = ["settings_riptopl.cfg", "conf_riptopl.cfg"]
notes     = ["Upstream README: 'heavily uses LLM tools'", "MMCE native"]
```

The `member` path for v1.1.0 is derived from the v1.1.0 Makefile and CI [D]; check it once by listing the archive.

### 6.3 Download, verify, cache: details

- Endpoint: `GET /repos/{o}/{r}/releases/tags/{tag}`, or `/releases/latest` for "latest stable". Keep the `ETag`. On a 304, reuse the cached JSON.
- Pick the asset by exact name or regex. Refuse if several match.
- Download `browser_download_url` with our UA, stream to a temp file, and hash while downloading.
- Accept the file when:
  1. `sha256 == asset.digest` (if the digest is present), and
  2. `size == asset.size`, and
  3. for pinned entries, `sha256 == catalogue.sha256`.
  Otherwise delete it and report.
- Cache layout: `<app-data>/loaders/<id>/<sha256>/{asset, release.json, extracted/…}`. Keep the last N versions per loader so the user can roll back.
- Archive extraction: `sevenz-rust2` / `zip` (already in our stack). Normalise paths; reject absolute paths, `..`, symlinks and members over a size cap.
- Offline mode: install from the cache or from a user-picked local file (the hash is checked against the catalogue when known; otherwise it is labelled "unverified local file").

### 6.4 Install per boot target

| Target device (as mounted on the PC) | Default preset | Extra preparation |
|---|---|---|
| USB FAT32/exFAT | FMCB 1.966: `UPDT/OPL.ELF`; PS2BBL: `APPS/OPNPS2LD.ELF`; or app style `APPS/<APP>/` + `title.cfg` | Folder set under `usb_prefix`; warn that FMCB's stock driver is FAT32-only |
| MX4SIO SD | app style or custom path (launcher: PS2BBL-MX4SIO `massX:` / OSDMenu `mx4sio:`) | Same folder set (`usb_prefix` applies); warn about the 64-fragment budget (IMP-7b) |
| MMCE SD | app style `APPS/APP_RIPTOPL/` or `APPS/OPL/OPNPS2LD.ELF` (launcher: PS2BBL-MMCE / OSDMenu `mmce?:`) | Folder set at root or under `mmce_prefix` (trailing `/`); `neutrino/` at root for RiptOPL/NHDDL; reserved-path guard (LDR-7); no `ul` |
| iLink, ATA exFAT (HDD in a USB enclosure) | app style or custom | Folder set; `enable_ilink` / `enable_bdm_hdd` steps in the boot guide |
| APA HDD | later (HDD-1) | — |

Every install is a plan:
- "write 1.36 MB to `E:\UPDT\OPL.ELF`, replacing a 1.35 MB file (OPL v1.2.0-Beta-2257, sha …), backup to `E:\.yaoplm\backup\…`".
- Then confirm, temp file + rename, and a journal entry.

### 6.5 Device manifest

- `<root>/.yaoplm/manifest.json` records:
  - the YAOPLM version;
  - each installed loader: id, tag, `published_at`, sha256, path;
  - the profile;
  - the date.
- Hidden dot folder: NHDDL skips `.`-folders [V], OPL only scans known folders [V], and sd2psXtd uses the same convention (`.sd2psx/`).
- If the owner objects, fall back to a local DB keyed by volume serial and path (open question 9).

### 6.6 Updating

- Detection (LDR-3) gives the installed sha256. The catalogue channel gives the current one.
- "Update available" = the digests differ and the channel's `published_at` is newer.
- Pinned (stable) channels never auto-switch to a rolling build.
- Update = install over the same path, with a backup. RiptOPL: replace only `RIPTOPL.ELF` unless the user selects "full package" (POPS/EMBER/neutrino), as RiptOPL's own docs advise [R].

### 6.7 Out of scope (v1)

- Writing FMCB, PS2BBL or OSDMenu to **physical** memory cards; KELF/system-update installs (KELFBinder); MBR, FHDB, HOSDMenu installs; FreeDVDBoot discs.
- Writing launcher configs (`FREEMCB.CNF`, `PS2BBL.INI`, `CONFIG.INI`, `OSDMENU.CNF`) to a device. USB copies can override the memory-card config (§2.4), so we only *show* the lines (BOOT-1).
- Flashing MMCE or MCP2 firmware; editing `.sd2psx/settings.ini` or `Game2Folder.ini`; managing per-game card images (a later feature, next to VMC-2).
- Editing `.mcd` boot-card images (BOOT-2, later).
- Downloading POPS.ELF, BIOS or any Sony file. POPSTARTER only arrives inside a user-requested RiptOPL package.
- APA partitions and `+OPL`/`+wOPL` installs (HDD-1).
- UDPBD/UDPFS servers. That could be a nice later idea for `yaoplm-server`, but it is not a loader install.

### 6.8 Suggested SPEC edits (for the owner to approve; not applied)

1. **§4 profile table**:
   - Add an **MMCE** column (1.1.0: no; 1.2.0 beta: no; MMCE fork: yes; RiptOPL: yes; wOPL: yes; NHDDL: yes).
   - Add a **wOPL** row: "CFG in libconfig since 2026-06; read-only for us".
   - Add an **MMCE fork** row, or fold it into a "OPL 1.2.0 beta (MMCE fork)" variant.
2. **RiptOPL row**: replace "8-битный индексированный PNG (обязательность не подтверждена)" with "PNG ≤8 бит на канал (16-бит отвергается); индексированный — рекомендация; удвоенные COV/ICO — опция HD". **Close open question 1.**
3. **§4 storage list**: add "SD-карта MMCE (SD2PSX / MemCard PRO 2) как обычная папка, с защитой служебных путей".
4. **ART-2 / ART-6**: always write 8 bits per channel. The RiptOPL HD preset is optional. Art keys: VCD stem, Ember folder, app ELF name; apps on the same device.
5. **CFG-1**: add "detect non key=value (libconfig) CFG and refuse to rewrite it".
6. **§5.9**: add the LDR-*/BOOT-* block. **§8** already excludes writing to a physical memory card ("запись на физическую карту памяти"); add "launcher configs (FMCB/PS2BBL/OSDMenu)" and "MMCE firmware and card settings".

### 6.9 Open questions for the owner (concrete list)

1. **Which MMCE card** exactly:
   - SD2PSX (sd2psXtd firmware version?), MemCard PRO 2 (firmware version?), PicoMemcard+, or PSXMemCard?
   - Which slot is it in during play?
2. **Where FMCB lives**: on a physical card in slot 1, or as the MMCE boot card (`BootCard-1.mcd`)? FMCB version (1.953 or 1.966)?
3. **Current OPL launch path**: in FMCB Configurator → OSDSYS items, which path does the "Open PS2 Loader" item use? A photo is enough. Is `mc0:/OPL/conf_opl.cfg` present?
4. **Which loader** do you want on MMCE:
   - RiptOPL (maintained daily, MMCE native, LLM-heavy);
   - the ps2-mmce OPL fork (beta-2, Jan 2025);
   - wOPL;
   - NHDDL+Neutrino (ISO only)?
5. **Default channel** for upstream OPL: rolling 1.2.0 beta `latest` (ADR-0004) or stable 1.1.0? Must YAOPLM warn on every rolling update?
6. **SD card format** (FAT32 or exFAT), size, and current top-level folders. A `tree /a` or `dir` of the root is enough.
7. **Console model** (SCPH-…), for MX4SIO/MMCE quirks and FreeDVDBoot notes in the boot guide.
8. Should YAOPLM install **companions** from the RiptOPL package (POPSTARTER, Ember, Neutrino) or only the ELF?
9. Is a hidden **`.yaoplm/` folder** on your devices OK (install manifest and backups)?
10. Should we offer **launcher-config writing** later (e.g. an `OSDMENU.CNF` on the SD card for MMCE setups), or keep it guide-only?
11. Console tests we will need later:
    - (a) FMCB 1.966 stock item launching `mass:/UPDT/OPL.ELF`;
    - (b) RiptOPL on MMCE seeing games under a prefix;
    - (c) a multi-part `ul` game on MMCE (to confirm the single-handle inference);
    - (d) RiptOPL showing an RGBA 280×400 cover versus an indexed one.

---

## Sources

**Repositories and files** (local clones under `$YAOPLM_TESTDATA\refs\`):

- `ps2homebrew/Open-PS2-Loader`:
  - `Makefile`, `.github/workflows/compilation.yml`, `lng_pack.sh`
  - `src/opl.c` (`setDefaults`, `_loadConfig`, `tryAlternateDevice`, `checkLoadConfigBDM`, `trySaveConfigBDM`, `_saveConfig`, `miniInit`)
  - `src/supportbase.c` (`sbCreateFolders`), `src/bdmsupport.c` (`bdmFindPartition`, `bdmNeedsUpdate`, `bdmUpdateDeviceData`), `src/ethsupport.c`, `src/hddsupport.c`
  - `src/textures.c`, `src/gui.c`, `include/compatupd.h`, `include/config.h`
  - `src/system.c:817`, `src/OSDHistory.c`
  - v1.1.0 `include/config.h` and `compilation.yml` via gh api
- `NathanNeurotic/Open-PS2-Loader` (RiptOPL):
  - `include/config.h`, `src/opl.c`, `src/textures.c` (`texSizeValidate`, PNG decode)
  - `src/themes.c` (`getGameImageTextureEx`, `isHiddenArtworkCache`, `THM_COVER_W/H`), `src/texcache.c`, `src/tar.c`
  - `src/bdmsupport.c` (`bdmGetImage`, `bdmGetGameStartup`), `src/vcdsupport.c` (`vcdFillGameList`, `vcdExtractGameId`), `src/cuesupport.c`
  - `src/appsupport.c` (`appSetArtSource`, `appGetImage`), `src/supportbase.c` (VCD CFG fallback), `src/favsupport.c`
  - `misc/conf_theme_OPL.cfg`, `misc/theme_coverflow.cfg`
  - `Example/APP_RIPTOPL-OFFICIALROLLING-VERSION.ZIP/` (title.cfg, ART PNG headers)
  - `.github/workflows/rolling-release.yml`, `.github/scripts/package_release_app.py`
  - `EMBER/LICENSE-BETA.txt`, `docs/VCD.md`, `docs/DOCUMENTATION-GAPS.md`
- RiptOPL docs site (`gh-pages`): `install.html`, `ps1-vcd.html`, `coverflow.html`, `releases.html`, `mmce.html` — https://nathanneurotic.github.io/Open-PS2-Loader/
- `ps2-mmce/Open-PS2-Loader` branch `mmce_support`: `include/config.h`, `src/mmcesupport.c`
- `ps2homebrew/wOPL`: `README.md`, `src/config_wopl.c`, `src/config_migration.c`, `src/gui.c`; commits via gh api (`03758fb`, `ec47a8f`)
- `Luden02/OrbitPS2-Manager`: `src/utils/png-artwork.ts`, `src/utils/artwork-name.ts`, `src/services/artwork.service.ts`
- `pcm720/nhddl`: `README.md`, `.github/workflows/build.yml`, `src/devices/mmce.c`, `src/neutrino.c`; `rickgaiser/neutrino` README and `ee/loader/config/bsd-mmce.toml`
- `ps2-mmce/mmceman`: README, `docs/MMCE CMD Protocol.md`, `docs/MMCE FS Protocol.md`, `mmceman/src/mmce_fs.c`, `mmceigr.c`
- `sd2psXtd/firmware`: README, CHANGELOG, `src/ps2/ps2_cardman.c`, `src/game_db/game_db.c`, `src/ps2/history_tracker/ps2_history_tracker.c`; `sd2psXtd/sd2psXtd.github.io` `sd_builder/`
- `israpps/FreeMcBoot-Installer`: `installer_res/{1953,1966}/INSTALL/SYS-CONF/FREEMCB.CNF`, `FREEHDB.CNF`, `MASS/FREEMCB.CNF`, `installer/system.c`
- `israpps/PlayStation2-Basic-BootLoader`: `Makefile`, `src/main.c`, `include/common.h`, `release/SYS-CONF/PS2BBL.INI`, CI; docs https://israpps.github.io/PlayStation2-Basic-BootLoader/
- `pcm720/OSDMenu`: README, `patcher/README.md`, `launcher/README.md`, `examples/OSDMENU.CNF`, `patcher/src/settings.c`, `launcher/src/common.c`, `handler_mc.c`, licences
- `ps2homebrew/wLaunchELF` (`filer.c`, `config.c`, LICENSE); `israpps/wLaunchELF_ISR` (README, release assets)
- `CTurt/FreeDVDBoot`: README, `PAYLOADS/`
- `ps2dev/ps2sdk` `iop/sio/mx4sio_bd/`; `ps2dev/ps2-packer` README (default LZMA)

**GitHub API** (2026-10-22): `repos/<o>/<r>` (licence, stars, pushed_at) and `releases` (assets, size, digest) for every repo above, plus `ps2max32/Open-PS2-Loader`, `officialjuicedesigns/OPL-Evolution`, `L10N37/Open-PS2-Loader-Extended-APA`, `mystyq/Stable-Open-PS2-Loader`, `NathanNeurotic/RiptOPL`, `NathanNeurotic/wOPL`; `repos/ps2homebrew/Open-PS2-Loader/forks`.

**Vendor docs**: 8BitMods MemCard PRO 2 wiki (https://www.8bitmods.wiki/memcard-pro2 and its first-time-setup, basic and advanced usage pages).

**Unverified / snippets**: ConsoleMods OPL, PS2BBL and FMCB pages (403); psx-place threads (Cloudflare); craigshaw/ps2mcs (MCP2 naming); Time Extension and RetroRGB MemCard PRO reviews; scribd MX4SIO guide; moddingfridays OPL wiki.
