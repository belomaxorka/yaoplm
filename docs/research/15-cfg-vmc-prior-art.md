# 15 - Game settings (CFG) and memory cards (VMC): how OPL and others do it

Date: 2026-10-24. Scope: milestone M5, CFG-2 and VMC-1 (ADR-0036). Sources: the local clones from [REFERENCES.md](REFERENCES.md), refreshed the same day: upstream OPL `ps2homebrew/Open-PS2-Loader@b3a21997` (and tag `v1.1.0` = `07beff6`), RiptOPL `NathanNeurotic/Open-PS2-Loader@55261de1`, OPL-Evolution `@47119b7`, OPL-PC-Tools `@2291b5a`, mymcplusplus `@3247c9c`, ps2vmc-tool `@32a0245`, OrbitPS2 `@cdd590a`, the CFG corpora. Nothing was copied: OPL and its forks are AFL-3.0 (ADR-0002), the GPL-3 tools were read for ideas and used as test oracles.

Legend: **[verified]** read in the source and, where said, run; **[secondary]** a fork or another tool; **[guess]** an inference.

## 1. CFG: what OPL's game menu writes

### 1.1 Save rules [verified]

"Save Changes" (`src/menusys.c:1191-1198`) calls `guiGameSaveConfig()` (`src/guigame.c:983-1108`), then writes the game's CFG and `conf_game.cfg`. `configSetInt`/`configSetStr` return 1 even when the value does not change (`src/config.c:280-301`), and any such call makes the save return 1, which then adds `$ConfigSource=1`.

| Key | Written when | Removed when | Notes |
|---|---|---|---|
| `$Compatibility` | value ≠ 0 | value = 0 | Rebuilt from six checkboxes: OPL's own save drops bits 0x40 and up |
| `$DMA` | device has `MODE_FLAG_COMPAT_DMA` and value ≠ 7 | value = 7 | HDD (and, in 1.2, BDM over ATA, which ignores the level). Not touched on USB, MX4SIO, iLink, SMB |
| `$GSMSource` | per game, always `1` | the moment the dialog switches to Global (`guigame.c:344-347`) | |
| `$EnableGSM`, `$GSMVMode`, `$GSMXOffset`, `$GSMYOffset`, `$GSMFIELDFix` | per game and ≠ 0 | per game and = 0 | **Global leaves them in the file**: switching back to per game finds them again. Offsets −100..100 in the menu |
| `$CheatsSource`, `$EnableCheat`, `$CheatMode` | as GSM | as GSM | `$CheatMode` 1 ("select") is 1.2 only |
| `$PADEMUSource`, `$EnablePadEmu`, `$PadEmuSettings` | as GSM | as GSM | |
| `$AltStartup` | non-empty | empty | a–z made upper case, nothing else checked (`guigame.c:83-96`) |
| `$VMC_0`, `$VMC_1` | non-empty | empty | **Does not count for `$ConfigSource`** (`config.c:632-641`) |
| `$DNAS` | non-empty | never | |
| `$ConfigSource` | `1` when anything above was set | — | 2 = "downloaded defaults"; OPL's compatibility-list update skips games with 1 (`src/opl.c:1338`) |

New keys are appended in call order; keys already in the file keep their place; unknown keys stay. "Remove Settings" deletes every known key and truncates an empty file to 0 bytes rather than deleting it.

**Launch rule** [verified]: a source key's *presence* selects per-game values, whatever it holds (`src/gsm.c:42`, `src/cheatman.c:45`, `src/supportbase.c:639`; the same in 1.1.0). `$GSMSource=0` therefore means *per game*. Never write `$xxxSource=0`; omit the key for global. Per-game GSM and cheat values are read only when `$EnableGSM`/`$EnableCheat` is non-zero.

### 1.2 Limits and bits [verified]

- `$AltStartup`: the menu takes 31 characters, but OPL boots `cdrom0:\<value>;1` from `ElfPath[32]` (`src/system.c:810,1002`): **21 characters** fit. The value has no `;1`. It also replaces the game ID in `GameID[16]` for per-game patches.
- `$VMC_N`: `char vmc1[32]`, so 31 characters; the file is `<prefix>VMC/<name>.bin` (`system.c:1048`).
- `$GSMVMode`: 0–28 (`predef_vmode[29]` in `src/gsm.c`; an index above 28 is out of bounds). Names are hard-coded English in `guigame.c:375-405`: NTSC, NTSC Non Interlaced, PAL, PAL Non Interlaced, PAL @60Hz, PAL @60Hz Non Interlaced, PS1 NTSC (HDTV 480p @60Hz), PS1 PAL (HDTV 576p @50Hz), HDTV 480p @60Hz, HDTV 576p @50Hz, HDTV 720p @60Hz, HDTV 1080i @60Hz, HDTV 1080i @60Hz Non Interlaced, VGA 640x480p @60/72/75/85Hz, VGA 640x960i @60Hz, VGA 800x600p @56/60/72/75/85Hz, VGA 1024x768p @60/70/75/85Hz, VGA 1280x1024p @60/75Hz.
- `$PadEmuSettings` (`guigame.c:632-652`): bits 0–7 mode (0 DualShock 3/4 over USB, 1 over Bluetooth); bit 8 + port: emulation on that port; bit 16 + port: vibration; bit 24 multitap, 25 its port − 1, 26 the fake-DS3 workaround. Multitap on forces bits 9–11. Port 1 with vibration over USB is `0x10100` (65792).
- `$Compatibility` bits 0x01–0x20 are modes 1–6 in 1.1.0 and 1.2 alike (`include/iosupport.h:63-72`, `COMPAT_MODE_COUNT 6`). Menu names are "Mode 1".."Mode 6" with hints Accurate Reads, Synchronous Mode, Unhook Syscalls, Skip Videos, Emulate DVD-DL, Disable IGR (`lng_tmpl/_base.yml`).

### 1.3 OPL 1.1.0 [verified]

The same keys, defaults, save and launch rules, GSM list and PADEMU layout. Missing in 1.1.0: `$ParentalLock`, the pad macro, the OSD language keys, `$CheatSel###`. `$CheatMode` exists but is read and never used. No key was renamed. A writer for 1.1.0 therefore needs no special case for the keys YAOPLM edits.

### 1.4 Forks [secondary]

- **RiptOPL** (`NathanNeurotic/Open-PS2-Loader`): `COMPAT_MODE_COUNT 8`; 0x40 = mode 7 "Neutrino only: fix game buffer overrun" (passed as `-gc=7`), 0x80 = mode 8 "Slow Reads" (OPL core, DVD read at CD speed, added 2026-10-20 in `6f7c7cdd`). Extra keys `$CoreLoader`, `$Neutrino*`, `$VMCDisable_N`, `$Ember*`, `$EnableImage`; `$GSMSource=2` means "mixed". Per-title default modes apply only while `$Compatibility` is absent, and its menu writes an explicit `$Compatibility=0` to switch a default off. It **writes LF** (`src/config.c:1531`) and reads wOPL's libconfig files. Its gh-pages docs still say "modes 1–7".
- **0.9.3 legacy:** bits 0x40 "High Module Storage" and 0x80 "Hide DEV9 module" (labels in OPL-PC-Tools `GameConfigWidget.cpp:220-246`, OrbitPS2). Community CFGs still carry them: five files with `$Compatibility=100` (0x64) in Tom-Bruise and VTSTech, e.g. the Jak games.
- **OPL-Evolution:** upstream `config.c`; per-slot VMC mode Physical / Generic / Per-game.

### 1.5 Other managers [secondary]

| Tool | Keeps unknown keys | Line ends | Pitfalls |
|---|---|---|---|
| OPL-PC-Tools `GameConfiguration.cpp` | yes, in place | `QFile::Text`: LF off Windows | "Use global GSM" writes `$GSMSource=0` (per game for OPL); `$GSMSkipVideos` no longer exists; values trimmed; `toLatin1`; remove-then-rename leaves a moment without a file; always writes `$ConfigSource=1` |
| OrbitPS2 `cfg.service.ts` | yes (JS object) | CRLF | writes `#Name`, which OPL ignores and drops; trims; no BOM handling; save always "dirty"; drops bits above 0xFF |
| OPLNova `Config.cs` | **no**: rewrites the file with three keys | LF off Windows | saves on browsing (toggle events) |
| logi-26 ps2-game-manager | yes (LinkedHashMap) | platform | crashes on a non-numeric `$Compatibility` |
| PFS-BatchKit-Manager | no: deletes and rebuilds from a database | — | cuts `Description` to 255 "to fix OPL freezes" |

Good ideas taken: a tri-state "global / per game" for GSM, cheats and PADEMU (logi-26); listing slots that name a missing card (OrbitPS2); "N other keys kept" (opl-linux-toolbox); "Mode N — label" with a hint (OPL-PC-Tools).

### 1.6 Corpora [verified]

Tom-Bruise (13,620 files per language), VTSTech (13,620), GDX-X (70, HDD): all CRLF but two GDX-X files in LF; 41/18/16 files without a final newline; no BOMs, no duplicate keys. `$Compatibility` in 146 / 145 / 62 files (values 32, 1, 3, 2, 45, 8, 7, 6, 5 and the legacy 100), `$ConfigSource=1` in 22 / 4 / 70, `$VMC_0` in 2 (group names such as `SCUS_973.99_0`), no `$GSM*`, `$DMA`, `$PADEMU*`. 239 (en) and 397 (fr) files have a `Description` over 255 bytes, up to 405: OPL keeps 255 bytes, so the editor must not rewrite keys it was not asked to change.

## 2. VMC

### 2.1 How OPL formats a card [verified]

The VMC menu always calls the IOP module `genvmc` with `VMC_blocksize = 16` and `VMC_card_slot = -1` (`src/system.c:1088-1096`; 1.1.0 the same), i.e. `vmc_mcformat()` (`modules/vmc/genvmc/genvmc.c:289-524`):

1. `open(O_RDWR|O_CREAT|O_TRUNC)`: an existing file is overwritten without a check.
2. Fill the whole file with 0xFF, 16 KiB at a time.
3. The indirect FAT at cluster `blocksize / 2` = 8: the numbers of the FAT clusters, then zeros.
4. The FAT from cluster 9: entry 0 (the root directory) `0xFFFFFFFF`, then `0x7FFFFFFF` (free) up to `alloc_end`, zeros after it in the last FAT cluster.
5. The root directory at `alloc_offset`: `.` (mode 0x8427, length 2) and `..` (0xA426, hidden, length 2), times from the console clock (JST; 2000-03-04 05:06:07 if it cannot be read), name at 0x40.
6. The superblock last: the 384-byte static `MCDevInfo` (fields it never sets are zero), the rest of the cluster 0xFF.

No ECC or spare bytes: the file is exactly N MiB. Backup blocks stay 0xFF.

| MiB | Clusters | FAT clusters | `alloc_offset` | `alloc_end` | Backup blocks | `max_allocatable` |
|---|---|---|---|---|---|---|
| 8 | 8192 | 9–40 | 41 | 8135 | 1023 / 1022 | 8001 |
| 16 | 16384 | 9–72 | 73 | 16295 | 2047 / 2046 | 16001 |
| 32 | 32768 | 9–136 | 137 | 32615 | 4095 / 4094 | 32001 |
| 64 | 65536 | 9–264 listed, **9–263 written** | 265 | 65255 | 8191 / 8190 | 65001 |

`max_allocatable` is genvmc's fixed-point multiply by `0x10624DD3`, i.e. `clusters / 1000 * 1000 + 1`. The 64 MiB card's last FAT cluster (264) is never written because the allocatable clusters end earlier: it keeps 0xFF. Superblock fields past the Sony header: `0x154` cluster size 1024, `0x158` FAT entries per cluster 256, `0x15C` clusters per block 8, `0x160` `cardform` 1, `0x170` `max_allocatable`, `0x17C` −1; `0x2E` is 0 (Sony and mymc write 0xFF00); unused `ifc_list` entries are 0xFFFFFFFF. research/02 §9 named `0x15C` after `mcemu.h` (`size_in_megs`); genvmc writes the clusters per block there, which is also 8 for an 8 MiB card.

**Cross-checks of YAOPLM's formatter** (`yaoplm_formats::vmc::write_blank`):
- OPL's PC copy `pc/genvmc` (`genvmc <MB> <file>`), built from the clone in an ignored test: **byte for byte the same** for 8, 16, 32 and 64 MiB except the two root entries' times (`crates/yaoplm-formats/tests/opl_oracles.rs`). The PC copy stamps UTC through a broken BCD conversion, so its times never match anything.
- mymcplusplus: `check` passes, `df` reports `(alloc_end − 1) × 1024` free (8,329,216 bytes for 8 MiB), `ls` shows only `.` and `..`; a save directory added by mymc keeps `check` passing (`tests/vmc_oracles.rs`).
- ps2vmc-tool `samples/oplcard.bin`, `card16mb.bin`, `card32mb.bin` (made by genvmc, with saves; read locally, never committed): their superblock clusters equal ours byte for byte.

### 2.2 Other formatters [secondary]

| | genvmc | mymc++ `format` | OPL-PC-Tools | ps2vmc-tool `--mc-create` | OrbitPS2, OPLNova |
|---|---|---|---|---|---|
| `ifc` / `alloc_offset` | 8 / 41 | 8 / 41 | 8 / 41 | **16 / 49** (mcman quirk) | — |
| `alloc_end` | 8135, relative | 8135 | **8176, absolute** | 8127 | — |
| Fill | 0xFF | 0x00 | 0xFF | 0xFF | **zeros**: OPL refuses the file |
| `..` | 0xA426, length 2 | 0xA426, length 0 | **0x8027** | 0xA426, length 0 | — |
| Times | JST | JST | JST | UTC | — |

OPL-PC-Tools offers 8–512 MiB; OPL's menu only 8–64, and Neutrino emulates a fixed 4 MiB geometry. ps2vmc-tool tells ECC from no ECC by the file size (`size % 8 MiB`), mymc by trying ECC on page 0.

### 2.3 Names and groups [verified]

- The menu suggests `<group or ID>_<slot>` (slot 0 or 1) when an empty slot is defined (`guigame.c:216-218`); 1.1.0 always uses `<ID>_<slot>`. "Use Generic" gives lower-case `generic_0` / `generic_1`.
- Groups (`src/vmc_groups.c`): 97 groups `XEBP_100.01`–`XEBP_100.75` and `XEBP_000.01`–`XEBP_000.23` over 1034 title IDs, first match wins; some IDs are malformed and can never match. The file has **no licence header and no statement of where the data comes from**, so the repository's AFL-3.0 applies; RiptOPL, wOPL and OPL-Evolution carry copies. [guess] The `XEBP` IDs look like MemCard PRO group IDs.
- PFS-BatchKit writes `GENERIC_0` in capitals: on FAT that is the same file, on SMB it may not be.

### 2.4 Checks and menu behaviour [verified]

- `sysCheckVMC` (`src/system.c:1044-1099`): missing file −1; with a superblock (at launch) the magic (27 bytes), type 2 and size = `pages_per_cluster × clusters × page_size` must hold, and the size must be whole MiB. From the menu only the size is checked.
- BDM at launch (`src/bdmsupport.c:345-412`): one contiguous cluster chain (`USBMASS_IOCTL_CHECK_CHAIN`) and, in 1.2, the whole file below LBA 2^32; otherwise "VMC … is fragmented. Continue with Memory Card in slot N?". SMB has no contiguity check; HDD allows 11 PFS segments.
- A slot naming a missing file: at launch a Yes/No prompt to use the real card instead. **Leaving the VMC dialog creates an 8 MiB card for every named slot whose file is missing, and deletes and recreates as 8 MiB any file whose size is not 8/16/32/64 MiB** (`guigame.c:332-333`, unlink at `system.c:1083-1084`), even with `enable_delete_rename` off. Changing a card's size in the dialog recreates it without a confirmation; OPL-Evolution's "Create" does the same.
- OPL creates `VMC/` (with the other folders) every time it polls a device (`supportbase.c:842-851`).
- RiptOPL reserves the card's space before writing (FatFs `f_expand`), refuses implicit resizes and removes a partial file on failure [secondary].

### 2.5 Contiguity on the PC [verified]

Windows reports a file's runs through `FSCTL_GET_RETRIEVAL_POINTERS` on FAT32, exFAT and NTFS without administrator rights; extents that continue each other on disk form one run (`fsutil file queryextents` shows the same). YAOPLM reserves the space first (`SetFileInformationByHandle(FileAllocationInfo)`, ADR-0019) and counts the runs once the card is written. Linux (`FIEMAP`) and macOS come with the fragmentation check of the whole library (IMP-7b).

## 3. Editors in other tools [secondary]

- OPL-PC-Tools: one scrolling page per game: Game ID, Custom ELF, metadata, eight mode checkboxes "Mode N — label", two VMC combos with "Create VMC", a GSM group; an "OPL version" switch changes the mode labels and the GSM list.
- OrbitPS2: a modal with modes, two VMC selects (missing cards listed too) and "N other OPL settings will be preserved".
- OPL itself: Game Settings → Compatibility (modes, DMA, Game ID, Custom ELF), Configure GSM, Cheat Settings, Configure PADEMU, Configure VMC (slot 1/2: name, size 8/16/32/64, Create/Modify/Delete, Use Generic), then Save Changes / Test.

## 4. What YAOPLM does with this

ADR-0036. In short: the editor writes only the settings the user changed, by OPL's save rules (remove at default, source key present or absent, `$ConfigSource=1` unless only VMC slots changed); keys, lines, line endings and bits it does not know stay byte for byte, existing out-of-range values do not block other edits; new compatibility bits must be known to the profile; `$AltStartup` ≤ 21 characters; `$DMA` is shown but never written (no HDD yet). Cards are genvmc's layout, written as one preallocated file, checked for runs after writing, and never overwritten: replacing one moves the old file to `.yaoplm/backup/`. Default names are `<ID>_<slot>` (OPL 1.1.0's rule): the group table is not carried.

Open:
- RiptOPL's explicit `$Compatibility=0` and its extra keys belong to the RiptOPL profile work (ART-6, M6).
- Groups could come from a source with a licence (MemCard PRO 2 lists?) later; ask before using one.
- A save browser (VMC-2) can reuse mymc++'s directory logic (GPL-3) or OPL-PC-Tools' `MemoryCard/` (GPL-3) with attribution.
