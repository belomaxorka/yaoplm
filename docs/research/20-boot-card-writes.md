# 20. Writing into a memory card emulator's boot card: where OPL goes and how not to break it

Date: 2026-10-26. Scope: groundwork for LDR-5 (OPL inside the boot card) and VMC-2 (saves), plan `docs/plans/emulator-sd.md` PR 4–5, ADR-0048. The owner asked for this before any write code: read the sources of FreeMcBoot, PS2BBL, OPL, ps2sdk's memory card driver and the emulator's firmware, and check everything against the factory SD.

Everything here was read, not written: the factory SD's images were copied to a scratch folder and read there (YAOPLM's reader, `cargo run -p yaoplm-formats --example memcard`, and mymcplusplus).

Sources, all under `$YAOPLM_TESTDATA\refs`:

| Clone | Commit | What |
|---|---|---|
| `ps2dev__ps2sdk` | `329e2bb4` | `iop/memorycard/mcman`: the card driver (a reimplementation of Sony's MCMAN) |
| `israpps__FreeMcBoot-Installer` | `ac53a47` | FreeMcBoot 1.9x installer |
| `TnA-Plastic__FreeMcBoot` | `2e4eef9` | FreeMcBoot 1.8b: installer and launcher |
| `israpps__PlayStation2-Basic-BootLoader` | `d2e81fa` | PS2BBL |
| `ps2homebrew__Open-PS2-Loader` | `b3a21997` | upstream OPL; the MMCE fork at `origin/mmce_support` `e554796` |
| `sd2psXtd__firmware` | (research/19) | the emulator's firmware and its `ext/mcfat` |
| `Adubbz__mymcplusplus` | `3247c9c` | PC tool, the reader's oracle |

Tags: **[V]** verified on the factory images or read in the code by me (file named); **[R]** a subagent's reading of the code, not rechecked line by line; **[D]** deduced.

## 1. Where the console looks for OPL

- **The emulated card is `mc0:`.** The console sees the boot card image as an ordinary memory card in slot 1. FreeMcBoot starts from it and reads its own folders there: `mc0:/SYS-CONF/FREEMCB.CNF`, `mc0:/APPS/…` [V research/19 §4].
- **FreeMcBoot cannot read the SD itself.** It has no `mmce?:` device (research/07 §2.10). So OPL for FreeMcBoot lives inside the image, never at the SD root.
- **The factory channel 1 menu** (`SYS-CONF/FREEMCB.CNF`, read from a copy) [V]:

  | Item | Name | Paths, tried in order |
  |---|---|---|
  | 3 | Open PS2 Loader | `mass:/UPDT/OPL.ELF`, `mc?:/APPS/OPNPS2LD.ELF`, `mc?:/OPL/OPNPS2LD.ELF` |
  | 4 | Open PS2 Loader (MMCE Beta2) | `mass:/UPDT/OPL-MMCE-BETA2.ELF`, `mc?:/APPS/OPL-MMCE-BETA2.ELF`, `mc?:/OPL/OPL-MMCE-BETA2.ELF` |
  | 6 | Open PS2 Loader(10th) | `mc?:/APPS/OPNPS2LD10th.ELF` |
  | 7–9 | GB, FC, SNES emulators | `mc?:/APPS/PS2InfoGB.elf`, `FC.ELF`, `SNES.elf` |
  | 98 | Free MCBoot Configurator | `mc?:/SYS-CONF/FMCB_CFG.ELF` |

  Hotkeys: R1 wLaunchELF (`mass:/BOOT/BOOT.ELF`, then `mc?:/BOOT/BOOT.ELF`, `mc?:/B?DATA-SYSTEM/BOOT.ELF`), R2 ESR, L2 SMS, L1 HDLoader, Start the configurator.
- **PS2BBL from SD Builder** (`PS2BBL.mcd`, factory channel 5, `SYS-CONF/PS2BBL.INI`) [V]:
  - `LK_AUTO_E1..3`: `mass:/APPS/OPNPS2LD.ELF`, `mc?:/APPS/OPNPS2LD.ELF`, `mc?:/OPL/OPNPS2LD.ELF`;
  - `LK_L1`: `mmce?:/APPS/OPNPS2LD.ELF`, then `mc?:/APPS/OPNPS2LD.ELF`, `mc?:/OPL/OPNPS2LD.ELF`;
  - `LK_L2`: `mmce?:/APPS/OPL-MMCE.ELF`, then `mc?:/APPS/OPL-MMCE-BETA2.ELF`, `mc?:/OPL/OPL-MMCE-BETA2.ELF`;
  - `LK_R1`: `mmce?:/APPS/ULE.ELF`, …

  On this image the auto-start and L1 paths lead to the 2216 MMCE build, and L2 to OPL v1.0.0 under the name `OPL-MMCE-BETA2.ELF` (research/19 §4).

### 1.1 What wins over the card

The card's own config and ELF are not always the ones that run:
- **FreeMcBoot's config** [V `TnA-Plastic__FreeMcBoot/launcher2/launcher2.c` `loadConfig`]: `mass:/FREEMCB.CNF` first, then `mc<boot slot>:/SYS-CONF/FREEMCB.CNF`, then the other slot. A stick with `FREEMCB.CNF` replaces the whole card menu. Menu items and hotkeys try `path1`, `path2`, `path3` and take the first file that opens [R launcher2.c 1922-1956, 2336-2357]; `mc?:` is `mc0:` unless the file is missing there [R]. The 1.9x launcher has no source in the clones; its shipped CNFs use the same paths [R].
- **A stick also wins per item:** items 3 and 4 try `mass:/UPDT/…` first. The same on all four FreeMcBoot cards of the factory SD (channels 1, 3, 4, 6); channels 4 and 6 also have L1 → `mass:/OPL.ELF` [V, `FREEMCB.CNF` extracted with mymcplusplus]. FreeMcBoot's USB driver on the card (`SYS-CONF/USBHDFSD.IRX`, an uncompressed IRX) names only a "FAT driver" and has no exFAT [V]: the stick must be FAT32.
- **The stick is not the SD:** `mass:` is USB only. The emulator's SD root is `mmce?:`, which FreeMcBoot does not have, so `UPDT/OPL.ELF` at the SD root is never read by it.
- **PS2BBL's config** [V `PlayStation2-Basic-BootLoader/include/common.h` `CONFIG_PATHS`, `src/main.c` the loop from `SOURCE_CWD` down to `SOURCE_MC0`]: the list is searched backwards, so the order is `CONFIG.INI` in the working folder, `mc?:/SYS-CONF/PSXBBL.INI` (PSX builds), `mmce1:` then `mmce0:/PS2BBL/PS2BBL.INI` (MMCE builds), `xfrom:`, `hdd0:`, `massX:`, `mass:/PS2BBL/CONFIG.INI`, `mc1:/SYS-CONF/PS2BBL.INI`, and last `mc0:/SYS-CONF/PS2BBL.INI`. **The boot card's own ini comes last.**
- **PS2BBL reads the SD root first for L1 and L2:** `mmce?:/APPS/OPNPS2LD.ELF` and `mmce?:/APPS/OPL-MMCE.ELF` at the SD root run instead of the card's copies. YAOPLM never writes there (ADR-0048), but it can see them.
- **What it means for LDR-5** [D]: the list "which item starts which ELF" comes from the card's config, and the device card says when something can override it: an `APPS/OPNPS2LD.ELF` or `APPS/OPL-MMCE.ELF` at the SD root (PS2BBL), a `PS2BBL/PS2BBL.INI` at the SD root (MMCE builds of PS2BBL). A stick in the console is outside YAOPLM's view; the console checklist names it.

### 1.2 What the launchers check before starting an ELF

Nothing but that the file opens [V for PS2BBL `src/util.c` `exist()`, called from `src/main.c`; R for FreeMcBoot launcher2.c 309-318]. No hash, size, mode bits or icon.sys. If a menu entry's file is missing, the next path is tried; if the file is there but broken, the launcher has already handed over and the console hangs on a coloured screen [R PS2BBL `elf.c` `RunLoaderElf` → ps2sdk elf-loader]. **So YAOPLM writes only an ELF it recognised as OPL (`ps2_elf::program`), never a truncated or foreign file.**

### 1.3 Where OPL keeps its settings when it starts from the boot card

- `mc?:OPL/conf_opl.cfg` [V upstream `src/util.c` `checkMC`]: OPL picks `mc0` when `mc0:OPL/` exists, else `mc1` when `mc1:OPL/` exists, else the first formatted card, and keeps that choice for the session. With the emulator in slot 1 that is the boot card, when it has an `OPL/` folder (channel 1 has one).
- OPL rewrites the file in place on its own (`O_TRUNC`, no temporary file) [R `src/config.c` `configWrite`]. The MMCE fork handles the card config the same way; its own keys are `mmce_mode`, `mmce_slot`, `mmce_prefix`, `mmceigr_slot` [R fork `include/config.h`, `src/opl.c`]. Upstream keeps unknown keys, so it keeps `mmce_*` too, but clamps `default_device` above 7 to 7, which drops the fork's MMCE value 8 on the next save [R upstream `src/opl.c` 1181].
- The MMCE fork sends the game ID to the emulator only when a game starts, after saving `conf_last.cfg` [R fork `src/mmcesupport.c`]. While OPL's menu runs, `mc0:` stays the boot card.

## 2. What the factory boot cards look like inside

Read with YAOPLM's reader on copies [V]:
- **No cross-linked clusters** on any of the eight images (six PS2 boot cards, the arcade card, a blank card): every cluster belongs to one entry. Each region's `osdmain.elf`, `osd130.elf` and so on is a full copy. No `SYS-CONF/FMCBUINST.dat` or `SYS-CONF/uninstall.dat` (§4.2).
- **Backup blocks.** Backup block 2 (the second-to-last erase block) is all 0xFF on every image. Backup block 1 (the last) holds data on every card the console has written. A blank card has both erased. §4.1 says what this means.
- **Mode bits:**
  - FreeMcBoot, PS2BBL and SD Builder files: `0x8497` (exists, 0x0400, 0x0080, file, rwx);
  - their system folders `B?EXEC-SYSTEM`: `0x842F` (with copy protection 0x0008);
  - ordinary folders: `0x8427`;
  - files OPL wrote itself (`OPL/conf_opl.cfg`) and `FREEMCB.CNF`: `0x8417`;
  - `attr` is 0 everywhere.
- **Allocation is packed from the start:** the highest used cluster is the used count minus one on all six boot cards (channel 1: 7,536 used, highest 7,535).
- **Free space, as the console counts it.** The console allocates only below cluster 8,001 of an 8 MiB card (§4.3), not up to `alloc_end` (8,127). Free there:

  | Channel | Card | Used | Free for the console |
  |---|---|---|---|
  | 1 | FreeMcBoot | 7,536 | 465 KiB |
  | 2 | FunTuna | 4,012 | 3,989 KiB |
  | 3 | SD Builder FMCB 1.963 | 4,082 | 3,919 KiB |
  | 4 | FMCB 1.953 | 4,060 | 3,941 KiB |
  | 5 | PS2BBL | 4,477 | 3,524 KiB |
  | 6 | FMCB 1.965 | 4,075 | 3,926 KiB |

  A cluster is 1 KiB. On channel 1 an OPL ELF is 1.25–1.30 MiB (`APPS/OPNPS2LD.ELF` 1,277 clusters, `APPS/OPL-MMCE-BETA2.ELF` 1,318). A new ELF fits in place of `OPL-MMCE-BETA2.ELF` when it is at most 1,318 + 465 clusters, about 1.74 MiB; it would not fit as an extra file.
- **Geometry differs between cards:** indirect FAT at cluster 16 with the first allocatable cluster 49 (Sony's formatter, SD Builder), or at 8 with 41 (genvmc). The arcade card is 1 MiB: 1,024 clusters, backup blocks 127 and 126, `alloc_end` 995 below the console's limit.

## 3. Rules for LDR-5 and VMC-2 writes

From §1, §2 and §4 and the owner's questions of 2026-10-26 (plan PR 4, PR 5). They apply to every write into a `.mcd`.

**Before the plan (refuse with the reason):**
1. The image has the size it had and a power-of-two size from 512 KiB to 2 GiB without ECC (§4.4); the superblock reads.
2. Backup block 2 is erased: all 0xFF, at least the first word of its pages 0 and 1 (§4.1). Otherwise the console did not finish a write: putting the card in the console once finishes it.
3. The whole card is consistent: no cluster shared by two entries, no loops, every chain as long as its entry needs, no cluster at or above the console's limit (§4.3). mymcplusplus's `check` is the oracle.
4. No FreeMcBoot multi-install: no `SYS-CONF/FMCBUINST.dat`, no `SYS-CONF/uninstall.dat`, no entry with mode `0x8415` (§4.2). Such a card stays read-only.
5. The target is one file that exists (an ELF a menu item starts, `OPL/conf_opl.cfg`, a save's files) and the new bytes are what they claim (an ELF recognised as OPL, §1.2).
6. It fits: free clusters below the limit plus the target's own clusters; else refuse with the missing KiB.
7. LDR-5 changes only `APPS/*.ELF` and `OPL/*.ELF` a menu item names; VMC-2 never deletes or replaces `B?EXEC-SYSTEM`, `SYS-CONF`, `BOOT` or `BADATA-SYSTEM`. The KELFs in `B?EXEC-SYSTEM` are signed for the card during install [R installer `system.c` `SecrDownloadFile`]: their bytes never change.

**Writing (on a copy of the image, the SD in a card reader):**
8. Reuse the target's clusters in chain order, then take the lowest free clusters below the limit, as the console does (§4.3); free what is left over by marking it `0x7FFFFFFF` in the FAT. Nothing else in the FAT changes.
9. Keep the directory entry's mode, `attr`, name and creation time; set the length, the first cluster (`0xFFFFFFFF` for an empty file, §4.3) and the modification time. No other directory entry changes.
10. Do not touch the superblock or the backup blocks (§4.1).
11. Read the new image back before it replaces the old one: every other entry has the same name, mode, length and bytes; the target has the new bytes; check 3 passes. Any difference stops the write.
12. Replace the file without a moment when it is missing (§4.4): the new image is written next to it in `MemoryCards/PS2/BOOT/` under a name that is not `.mcd` and then renamed over the old one. Never a temporary folder in `MemoryCards/PS2/` (§4.4). Before it, the copy of the `.mcd` (ADR-0048) and the journal.

## 4. The card driver, the firmware and OPL (sources)

### 4.1 Backup blocks [V ps2sdk `mcman/src/main.c` `mcman_flushcacheentry`, `mcman_checkBackupBlocks`; the same in sd2psXtd `ext/mcfat/src/mcio.c` `Card_CheckBackupBlocks`]

How the driver rewrites an erase block that holds FAT or directory clusters:
1. Erase backup block 1.
2. Write page 0 of backup block 2 with the first word `block | 0x80000000`.
3. Write the block's **new** contents into backup block 1.
4. Write page 1 of backup block 2 with the same word: the copy is complete.
5. Erase the target block and write it.
6. Erase backup block 2.

At every mount the driver reads the first word of pages 0 and 1 of backup block 2:
- both erased: nothing to do;
- page 0 written, page 1 erased (the copy was never completed): erase backup block 2;
- page 1 written: copy backup block 1 over the block named in page 0, then erase backup block 2.

So after a write on the PC, backup block 2 must stay erased: a mark left there would make the next mount copy backup block 1 over a block YAOPLM changed. Backup block 1's old contents are harmless while block 2 is erased. All factory images are in that state (§2). mymcplusplus also leaves block 2 erased after each write [R `ps2mc.py` 687-691].

### 4.2 FreeMcBoot's cross-linked files [V installer `system.c` `FileAlias`, `irx/source/mctools/src/mctools.c` `CreateCrossLinkedFiles`]

- A normal install, a cross-model and a cross-region install copy every file separately: no cross-links [R `system.c` 1484-1510, 1861-1868].
- **A multi-install** (FMCB 1.9x installer) adds directory entries in the other regions' `B?EXEC-SYSTEM` folders that point at the same clusters as `BREXEC-SYSTEM`'s files, with mode `0x8415`, and records them in `SYS-CONF/FMCBUINST.dat`. The source warns that deleting one of them loses data. FMCB 1.8b does the same with 21 small files it re-points and records in `SYS-CONF/uninstall.dat`, leaving their own clusters allocated and unreferenced [R `TnA-Plastic__FreeMcBoot/main.c`, `modules/mcsp/mcsp.c`].
- The console's driver never notices shared clusters: deleting or rewriting one owner frees them for others [R mcman `McSetDirEntryState`]. So YAOPLM refuses to write into such a card (§3 rule 4), and never "repairs" or reclaims anything on it.
- The israpps installer only offers multi-install when built with `ALLOW_MI` [V `menu.c`]. Cards made by older installers may have it.

### 4.3 Allocation and directory entries [V mcman `main.c` `mcman_setdevinfos`, `mcman_findfree2`; R `ps2mc_fio.c`]

- **The console's limit:** `max_allocatable_clusters` counts `1000 · ⌊clusters_per_card / 1000⌋ + 1` good clusters from `alloc_offset`: 8,001 on an 8 MiB card, 1,001 on a 1 MiB card, never more than the card has. The clusters between it and `alloc_end` are the spare pool for a worn backup block (`mcman_replaceBackupBlock`). `mcman_findfree2` scans linearly from a hint up to the limit and takes the first free FAT entry (bit 31 clear). YAOPLM allocates the same way and below `min(limit, alloc_end)`.
- **A new file** [R `mcman_open2`, `mcman_close2`]: mode `0x8417`; after the first write `0x8497`; empty files keep cluster `0xFFFFFFFF`. A new folder: `0x8427`, length 2, `.` holds the parent's first cluster and the folder's index in the parent, `..` copies the parent's `.`. Times are the console's clock (`sceMcStDateTime`).
- **Delete** clears bit `0x8000` of the entry and marks the chain free with bit 31 cleared; the entry keeps its place [R `McSetDirEntryState`].
- **Mount checks** [R]: the superblock magic, version not 1.0, root entries `.` and `..`. A chain shorter than the entry's length makes reads fail.

### 4.4 The emulator's firmware [V `sd2psXtd__firmware/src/ps2/ps2_cardman.c`; R the rest, research/19]

- The only check on an image is its size: a power of two from 512 KiB to 2 GiB (`ps2_check_cardsize`). Any other size is renamed to `.crp` and replaced with a new empty card.
- A missing `BootCard-N.mcd` at mount is created empty (or the firmware falls back to `BootCard.mcd`) [R]. A replace must therefore never leave the name missing: rename over the old file, never delete first.
- The image stays open read-write while it is mounted, and writes from the console reach the SD about 100 ms later [R]. YAOPLM writes only with the SD in a card reader, never while it is in the emulator.
- Each folder in `MemoryCards/PS2/` is a card; a temporary folder there would show up as one [R]. The `BOOT` folder name is compared case-sensitively [R].
- The firmware parses the boot card's file system itself for the game history (`mcfat`), which runs the same backup block check at mount (§4.1) [R].
