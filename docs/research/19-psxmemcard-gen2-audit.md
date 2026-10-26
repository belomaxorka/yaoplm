# 19. The owner's PSXMemCard Gen2: microSD audit, firmware update, a larger card

Date: 2026-10-26.

On that date the owner's memory card emulator (BitFunX PSXMemCard Gen2) was updated to sd2psXtd 1.4.0, and its factory microSD was read **read-only**: nothing on it was changed. The audit covers:
- the layout;
- the card images;
- what the boot cards hold;
- how to move to a larger card;
- what YAOPLM needs to work with such a card.

The rules shared by every sd2psx-family device are not repeated here: the SD layout, MMCE and Game ID switching are in research/07 §2.7, and the partition rules in research/06 §1.4 ("MBR only, partition entry 1 only").

Sources:
- sd2psXtd/firmware `8a20bd9` (tag 1.4.0) and its README;
- sd2psXtd.github.io `download.md`;
- sd2psx.net `update-firmware.html` and `format-microsd.html`;
- ps2-mmce/Open-PS2-Loader `mmce_support` `e554796`;
- mymcplusplus `3247c9c`, run on copies of the images;
- YAOPLM's `elf_version` example and `yaoplm device` / `yaoplm scan`.

The backup is in `$YAOPLM_TESTDATA\backups\psxmemcard-gen2-sd-2026-10-26\`: every file, `files.tsv` with sizes, times and SHA-256 checked equal to the card, and a README. It is a file-level copy only; there was no admin shell for a raw disk image.

## 1. Device and firmware

- **Which firmware [V].** PSXMemCard Gen2 is sd2psx hardware: RP2040, OLED, two buttons, PSRAM. The README says "PSXMemCard Gen2 … Use *sd2psx* firmware variant". `psxmemcard.uf2` is for Gen1, which is PicoMemcard+ hardware without OLED, buttons or PSRAM. `download.md` gives the same mapping.
- **Bootloader [V].** RP2040 boot ROM: `INFO_UF2.TXT` says `UF2 Bootloader v3.0`, `Model: Raspberry Pi RP2`, `Board-ID: RPI-RP2`.
  - Firmware 1.4.0 enters it when either top button is held at power-on (`check_bootloader_reset()`, `src/main.c`). The sd2psx site also names the internal BOOT button as the last resort.
  - On Windows the device then shows as VID `2E8A` PID `0003`: a mass-storage drive plus an "RP2 Boot" (PICOBOOT) interface without a driver. The missing driver is harmless; only `picotool` needs that interface.
- **USB in normal mode [V].**
  - Releases before 1.4.0 show nothing on USB; the cable only powers the card.
  - 1.4.0 enables a USB-CDC command port (`help`, `reset bl`, `set card`, …), but only once the SD has mounted: `stdio_usb_init()` runs after `sd_init()` in `main.c`.
  - Without an SD the firmware stops at "FATAL ERROR / failed to mount the card" (`ext/ESP8266SdFatWrapper/src/sd.cpp`) and never appears on USB.
- **The update done on 2026-10-26.**
  - File: `sd2psx.uf2` from release 1.4.0, 2 361 344 bytes, SHA-256 `0551bb60…7024`, equal to the release digest. It holds 4 612 UF2 blocks, family `0xe48bff56` (RP2040), addresses `0x10000000–0x10120400`.
  - It was copied to `RPI-RP2`; the card rebooted on its own, which the ROM only does after a complete image.
  - The factory firmware version was not recorded. The owner confirmed 1.4.0 in the device's Info menu the same day, with the Kingston card inserted.
  - **The CDC port works [V 2026-10-26].** With the Kingston card in, the device enumerates as VID `2E8A` PID `000A`: a serial port (COM3 here) plus a "Reset" interface. `help` (CR-terminated; the parser takes CR, LF or CR LF, `src/serial_input.c`) lists exactly 1.4.0's commands and ends with a `>` prompt. No other command was sent.
  - **No file access over USB [V 2026-10-26].** Neither sd2psXtd nor the original sd2psx has USB mass storage; both only call `pico_enable_stdio_usb`. No branch reads or writes SD files over the port, and the tracker has no request for it.
  - **The unreleased `develop` branch** (`b3c9a54`, 27 commits after 1.4.0) adds two things:
    - read-only inspection over the port: `version`, plus `ls [path]` and `stat <path>`, both refused with `Busy` while the console uses the card (PR #106);
    - GPT and partition-less SD cards (PR #112).

    The 2026-10-09 "nightly" release was built from `main`, so it has neither yet.
  - The plan is docs/plans/emulator-sd.md. After it was written the owner kept only the card reader: YAOPLM has no USB or serial code, because no firmware gives file access over USB (ADR-0048).

## 2. The factory microSD

| What | Value |
|---|---|
| Card | SDHC, 8 032 092 160 bytes, 512-byte sectors |
| Partition table | MBR (signature 7919718), one partition: type `0x07`, active, from LBA 2048 (1 MiB), 7 518 289 920 bytes. About 489 MiB after it are unallocated, so the factory probably wrote a smaller image |
| File system | exFAT, cluster 32 KiB, volume serial `C0EE-DE33`, no label, 241 MB used |

```
.sd2psx/settings.ini                     121 B, LF
MemoryCards/PS2/BOOT/BootCard-1..6.mcd   8 MiB each, BootCard.ini (channel names)
MemoryCards/PS2/Card1|Card7|Card9/CardN-1.mcd   64 MiB each, identical: blank
MemoryCards/PS1/BOOT/BootCard-1.mcd      128 KiB, BootCard.ini
MemoryCards/PS1/Card1|Card4/CardN-1.mcd  128 KiB each, identical: blank
MemoryCards/COH/BOOT/BootCard-1.mcd      1 MiB (arcade variant), BootCard.ini
```

`settings.ini` (LF line ends) is still in the pre-1.4.0 form: 1.4.0 adds `EnableControllerCombo` and `MaxCardIdx`.

```ini
[General]
Mode=PS2
FlippedScreen=OFF
[PS1]
Autoboot=OFF
GameID=ON
[PS2]
Autoboot=ON
GameID=ON
CardSize=64
Variant=RETAIL
```

- **Card size.** `CardSize=64` makes the firmware create every new card, Card*N* or per-game, at 64 MiB; that is where the three blank 64 MiB cards come from. The README recommends 8 MiB, the size of a real card: larger cards depend on a fast SD card and may get corrupted on a slow one.
- **Channel names.** `MemoryCards/PS2/BOOT/BootCard.ini` names the channels 1 `FMCB 1.966`, 2 `FunTuna`, 3 `FMCB 1.965`, 4 `FMCB 1.963`, 5 `FMCB 1.953`, 6 `PS2BBL`. The images disagree for channels 3–6 (§4). PS1 channel 1 is `Unirom`, COH channel 1 `wLaunchELF 4.43x_isr_coh`.

## 3. Card images

- **All PS2-side images use the standard PS2 memory card file system [V].** This includes the arcade COH card.
  - The superblock reads `Sony PS2 Memory Card Format`, version `1.2.0.0`, page 512, 2 pages per cluster, 16 pages per block, card type 2, flags `0x2b`.
  - There is no ECC: the file size equals clusters × 1 KiB.
  - This is the format of OPL's VMCs, which `yaoplm-formats::vmc` already reads and creates. mymcplusplus `check` passes on every image.
  - **No proprietary format** is involved.
- **Two formatters are visible.**

  | IFC at cluster | `alloc_offset` | Cards |
  |---|---|---|
  | 16 | 49 (8 MiB) or 273 (64 MiB) | boot cards 1, 3–6, and the blank 64 MiB cards the firmware made |
  | 8 | 41 | boot card 2 (FunTuna), as in genvmc and YAOPLM's blank cards |

  Both are valid, so a reader takes the IFC list, `alloc_offset` and the backup blocks from the superblock and never assumes them.
- **PS1 cards** are raw 128 KiB images starting with `MC`.
  - The PS1 boot card holds FreePSXBoot: an exploit whose directory entry has a nonsense size on purpose. A tool must not "repair" it.
  - The two blank cards are identical.
- **COH** holds `boot.bin` and `ACJVLOAD.IRX`: the arcade build of wLaunchELF that `BootCard.ini` names.
- **Corrupt images in 1.4.0 [V `ps2_cardman.c`].** The firmware moves an image it cannot use aside as `<folder>-<ch>.crp.<n>` and creates a new one. A tool listing cards should show those files rather than hide them.
- **Other files the firmware owns [V sd2psXtd `8a20bd9`, sd2psx `2408cef`; checked 2026-10-26].**
  - `civ.bin` at the SD root is the MagicGate key. `keystore_deploy()` (`src/keystore.c`) writes it to flash, deletes the file and keeps a copy as `.sd2psx/civ.bin`; the GUI has a "Deploy CIV.bin" item. A tool must never write, move or delete either file. The factory card has neither.
  - Card folders per variant: `MemoryCards/PS2`, `PS1`, `COH` (arcade), `SC2`, `PROT` (`src/card_config.c`). Per-card ini files sit next to the image as `<folder>/<base>.ini`.
  - The original sd2psx firmware uses the same `MemoryCards/PS2|PS1/…` layout with a single `BOOT/BootCard.mcd`, but keeps its settings in flash: its SD has no `.sd2psx/`. Detection therefore cannot rely on `.sd2psx/` alone (ADR-0048).

## 4. What the PS2 boot cards hold

Versions come from the ELF contents (`elf_version`), not from the file names. "Builder image" means a byte-identical match (SHA-256) with an image of sd2psXtd's SD Builder (`sd2psXtd.github.io/sd_builder/images/`, MIT, clone `7a204d9`).

| Channel | `BootCard.ini` says | The image is | OPL and other programs |
|---|---|---|---|
| 1 | FMCB 1.966 | builder `FMCB-1966.mcd` plus factory additions: the same `osdmain.elf`, `FMCB_CFG.ELF` and `BOOT.ELF`, a longer `FREEMCB.CNF` (3 069 bytes against 2 863), extra programs and saves | `APPS/OPNPS2LD.ELF` v1.2.0-Beta-2049-513e9b3; `APPS/OPL-MMCE-BETA2.ELF` v1.2.0-Beta-2216-bb415d2 (the MMCE fork's `OPL-MMCE-beta-2`, research/07 §1.4); `APPS/OPNPS2LD10th.ELF` (unpacked, version not readable); FC, PS2InfoGB, SNES with their saves; `BOOT/BOOT.ELF` wLaunchELF; `OPL/conf_opl.cfg` |
| 2 | FunTuna | not a builder image (`BOOT/` titled FORTUNA) | `BOOT/`: ESR, SMS, POWEROFF, wLaunchELF, OPL 2049 and 2216 |
| 3 | FMCB 1.965 | builder **`FMCB-1963.mcd`** | OPL 2049 and 2216 in `APPS/` |
| 4 | FMCB 1.963 | builder **`FMCB-1953.mcd`** | the same |
| 5 | FMCB 1.953 | builder **`PS2BBL.mcd`** | **Names swapped:** `APPS/OPL-MMCE-BETA2.ELF` is OPL **v1.0.0**, `APPS/OPNPS2LD.ELF` is the 2216 MMCE build; `SYS-CONF/PS2BBL.INI` |
| 6 | PS2BBL | builder **`FMCB-1965.mcd`** | OPL 2049 and 2216 in `APPS/` |

The COH boot card is the builder's `wLaunchELF_Arcade.mcd`. The PS1 boot card matches none of the builder's 22 FreePSXBoot images.

- **The factory SD is built from the SD Builder's images and then customised.** Its channel names are wrong for channels 3–6, so a tool must read what a card holds and never trust `BootCard.ini`.
- **The factory mislabels files.** This is real-world evidence for ADR-0041: a build is known by the version inside its ELF, never by its file name.
- **An unpacked OPL ELF gives no version** (`OPNPS2LD10th.ELF`, the 10th-anniversary build; its `OPL/` save says "OpenPS2Loader 10TH_ANNIVERSARY"). This is a gap in research/16 §1.6 detection.
- **OPL settings live inside the boot card**, as research/07 §2.7 expects: `OPL/conf_opl.cfg` on BootCard-1, CRLF.
  - It sets `default_device=4`, which is `MMCE_MODE` in the fork (upstream has modes 0–3), and has no `mmce_*` keys.
  - **That config was last saved by a build without MMCE.** The fork writes `mmce_mode`, `mmce_slot` and `mmceigr_slot` on every save (`src/opl.c`, `mmce_support`). Without them the fork starts with MMCE **disabled** (`gMMCEStartMode = START_MODE_DISABLED`), so its game list from the SD stays hidden until the user sets the MMCE start mode to Auto in OPL's settings and saves. That save lands in this boot card's `OPL/conf_opl.cfg`.
- **FreeMcBoot's menu on channel 1** (`SYS-CONF/FREEMCB.CNF`, the factory's customised one):
  - item 3 "Open PS2 Loader": `mass:/UPDT/OPL.ELF` first, then `mc?:/APPS/OPNPS2LD.ELF` (2049, no MMCE). With a USB stick holding YAOPLM's FMCB preset `UPDT/OPL.ELF` plugged in, this item starts the stick's OPL instead;
  - item 4 "Open PS2 Loader (MMCE Beta2)": `mass:/UPDT/OPL-MMCE-BETA2.ELF`, then `mc?:/APPS/OPL-MMCE-BETA2.ELF` (2216). This is the only entry that sees games on the SD;
  - item 6 "Open PS2 Loader(10th)", items 7–9 the GB, FC and SNES emulators, items 1–2 wLaunchELF and ESR;
  - hotkeys: R1 wLaunchELF, R2 ESR, L2 SMS, L1 HDLoader, Start the FMCB configurator, Down power-off; `OSDSYS_Skip_Logo=1`, `OSDSYS_Skip_MC=1`.
- **The boot channel at power-on** [V `ps2_cardman.c` `try_set_boot_card()`]: with PS2 `Autoboot=ON` the firmware mounts `BOOT` on the **last boot channel used**. That index lives in the device's flash (`ps2_boot_channel`), not in any file on the SD. Choosing channel 1 once on the device makes FreeMcBoot 1.966 start at every power-on.
  - `OPL/conf_network.cfg` holds defaults: DHCP, no SMB server.
- **Played games.** `BADATA-SYSTEM/history` holds one launched ID (`SLES_549.45`). The firmware's Game ID switching watches writes to this file.

## 5. Moving to a larger microSD (for example 128 GB)

The rules come from research/06 §1.4, sd2psx.net `format-microsd.html` and the firmware: SdFat built with `SDFAT_FILE_TYPE 3`, i.e. FAT16/32 and exFAT, and `sd.begin()` on partition 1 [V].

1. Before anything else, copy everything from the old card. Take `.sd2psx/` and `MemoryCards/` as they are; the 2026-10-26 backup above is one such copy. Make a fresh copy if the card has been used since.
2. Partition the new card with **MBR**: one primary partition in entry 1, either exFAT (type `0x07`) or FAT32.
   - **Never use GPT with 1.4.0**: its SdFat setup reads only MBR. The unreleased `develop` branch adds GPT and partition-less cards (PR #112); MBR stays the safe choice.
   - The SD Association's SD Memory Card Formatter does this for SDXC: exFAT with the cluster size and alignment the SD specification gives (128 KiB clusters for 64–512 GB).
   - Windows' own format dialog keeps the existing table; a factory SDXC card is MBR, but check in Disk Management.
3. Copy `.sd2psx/` and `MemoryCards/` to the root of the new card unchanged. Games for MMCE loaders go next to them in the USB layout: `CD/`, `DVD/`, `CFG/`, `ART/`, `VMC/` (research/07 §2.7).
4. Put the card in the emulator and power it on. Check:
   - the boot card starts FreeMcBoot on the console;
   - the Info menu shows the new card;
   - `.sd2psx/settings.ini` gains `EnableControllerCombo` and `MaxCardIdx`. 1.4.0 rewrites the file; when a `settings.ini` exists, it wins over the copy in flash (`settings_load_sd()`).
5. Optional, the owner's choice: set `CardSize=8`, so new per-game cards are created at the size games expect. Existing cards keep their size.

**A new `settings.ini` takes effect from the second power-on [V `src/main.c`, `src/settings.c`, `src/ps2/ps2_cardman.c` at `8a20bd9`; seen on the owner's Kingston 2026-10-26].**
- At power-on the firmware reads its settings from flash (`settings_init()`), mounts the SD, then runs `ps2_init()`: that picks the card with the flash values (`ps2_cardman_init()` → `try_set_boot_card()` checks `Autoboot`). Only after it does `settings_load_sd()` read `settings.ini` and write any difference to flash. PS1 mode runs in the same order.
- So the first power-on after `settings.ini` was replaced still uses the old values. On the Kingston, flash still held the defaults from the card's first start (`Autoboot=OFF`, §5.1), so the console got the plain card, not `BOOT`. The owner then switched Autoboot on in the device's menu.
- That menu change is safe. `settings_update_part()` writes flash and then rewrites `settings.ini` whole in the firmware's own form (`settings_serialize()`): `[General]` `Mode`, `FlippedScreen`; `[PS1]` `Autoboot`, `GameID`, `EnableControllerCombo`, `MaxCardIdx`; `[PS2]` `Autoboot`, `GameID`, `CardSize`, `Variant`, `MaxCardIdx`; LF, every line terminated. `BootCard.ini` and the card images are not touched.
- PS1 `Autoboot=OFF` was not a fault: it is the factory card's value, kept on purpose (§5.1).
- Every `.ini` the firmware reads loses an unterminated last line: `ini_reader_sd()` (`ext/inih/ini.c`) gives up at the end of the file in the middle of a line. That is why the README asks for an empty last line.
- For YAOPLM: a plan that changes `settings.ini` says that the change applies from the second power-on.

### 5.1 Done on 2026-10-26: the owner's Kingston 128 GB

- **The card as bought:**
  - SDXC, 125 098 786 816 bytes, MBR with disk signature 0;
  - one partition in entry 1: type `0x07`, not active, from 16 MiB;
  - exFAT, cluster 128 KiB, volume serial `0008-0E00`.

  This is SD-specification formatting and was kept as it is, with no reformat. Without admin rights the exFAT boot sector could not be read raw, so the cluster heap alignment is unchecked.
- **What firmware 1.4.0 made on the blank card** (the owner had inserted it in the emulator first):
  - `.sd2psx/settings.ini` with its defaults after the update: PS2 `Autoboot=OFF`, `CardSize=8`, `EnableControllerCombo=ON`, `MaxCardIdx=0` in both modes;
  - blank `MemoryCards/PS2/Card1/Card1-1.mcd` and `Card1-2.mcd` (8 MiB each);
  - blank `MemoryCards/PS1/Card1/Card1-1.mcd`.

  It made **no `BOOT` folder**, so FreeMcBoot would not have started from that card. The blank cards were kept. The firmware's `settings.ini` was saved to `$YAOPLM_TESTDATA\backups\kingston-128-before-layout-2026-10-26\` before it was replaced.
- **What was written** (the owner's choices: the order the factory names intend, every SD Builder PS2 image, 8 MiB cards). Each file went to a temporary name and was then renamed, and its SHA-256 was checked against the source (`written-layout.tsv` in the same backup folder):

  | Channel | Name in the new `BootCard.ini` | Image |
  |---|---|---|
  | 1 | FMCB 1.966 | factory BootCard-1 (customised FMCB 1.966) |
  | 2 | FunTuna | factory BootCard-2 |
  | 3 | FMCB 1.965 | factory BootCard-6 = builder `FMCB-1965.mcd` |
  | 4 | FMCB 1.963 | factory BootCard-3 = builder `FMCB-1963.mcd` |
  | 5 | FMCB 1.953 | factory BootCard-4 = builder `FMCB-1953.mcd` |
  | 6 | PS2BBL | factory BootCard-5 = builder `PS2BBL.mcd` |
  | 7 | FMCB 1.964 | builder `FMCB-1964.mcd` |
  | 8 | UMCS beta PS2BBL_FMCB | builder `UMCS-beta-PS2BBL_FMCB.mcd` |
  | 9 | UMCS beta SLIMTUNA_PS2BBL_FMCB | builder `UMCS-beta-SLIMTUNA_PS2BBL_FMCB.mcd` |

  - **PS1 and COH:** the factory boot cards, named `Unirom` and `wLaunchELF 4.43x_isr_coh`.
  - **ini files:** every `BootCard.ini` is written in the SD Builder's form — `[ChannelName]` plus `[Settings] MaxChannels=<n>` and `CardSize=8`, LF line ends, an empty last line. `MaxChannels` keeps the firmware from making blank channels past the last one.
  - **`settings.ini`** keeps the factory values in the builder's key order (`Mode=PS2`; PS1 `Autoboot=OFF`, `GameID=ON`; PS2 `Autoboot=ON`, `GameID=ON`, `Variant=RETAIL`), plus `EnableControllerCombo=ON`, `MaxCardIdx=0` and `CardSize=8`.
  - **No image was modified.** The swapped ELF names in channel 6 (PS2BBL) come from the SD Builder image itself and were left alone: PS2BBL's hotkeys may name those files.
  - mymcplusplus `check` passes on the three images that were new to this audit.

**Without the original card**, a new SD can be built three ways:
- **SD Builder** (`sd2psXtd.github.io/sd_builder`). In the browser it writes `.sd2psx/` and `MemoryCards/` with chosen boot cards: FMCB 1.953, 1.963, 1.964, 1.965 and 1.966, PS2BBL, two UMCS betas, the arcade wLaunchELF, and PS1 FreePSXBoot for 22 BIOS versions. Four of the factory boot cards are exactly these images.
- **The firmware alone.** It creates missing folders and blank cards at `CardSize` (`ps2_cardman.c`). A blank boot card boots nothing, so FMCB or PS2BBL still has to come from somewhere.
- **The backup above**, for the owner's own card, including the customised channel 1 and FunTuna.

Fragmentation does not matter on this card. MMCE reads go through the card firmware's file system: OPL-MMCE's `modules/iopcore/cdvdman/device-mmce.c` reads by file handle, with no fragment list (research/07 §2.7: "no fragment limit") [V code]. This has not been checked on the console yet.

## 6. YAOPLM today and what it needs

What `yaoplm device G:\` and `yaoplm scan G:\` print for this card today:
- "exFAT … SD removable";
- mode "plugged into the PS2";
- profile "OPL 1.2.0 beta: the device is exFAT";
- "No problems found";
- every OPL folder missing, 0 games.

So the program treats the card as a blank USB stick. From there it would offer to create OPL folders (harmless) and to install upstream OPL into `UPDT/` on the SD, where nothing starts it and which cannot see MMCE games. The formatter (DEV-3) would wipe `MemoryCards/`.

Mapped to SPEC:
1. **Detection first (safety).**
   - Signs: `.sd2psx/` (sd2psx/sd2psXtd), `MemoryCards/` with `PS2/BOOT/BootCard-*.mcd`, and MemCard PRO 2's `mcp2.bin` and `os/` (research/07 §2.7).
   - Such a card is a "memory card emulator SD": its own device kind next to "plugged into the PS2" and "SMB folder" (ADR-0047).
   - Its service folders are never written outside explicit memory-card features.
   - No loader install at its root.
   - DEV-3 refuses to format it, or offers only the move of §5.
2. **Games on the SD for MMCE loaders.**
   - Same layout as USB, under `mmce_prefix` or the SD root.
   - Profile: the MMCE fork, RiptOPL or wOPL (SPEC §4, LDR-1 entries "later").
   - No fragment rule. Multi-part `ul` is doubtful (research/07 §2.7).
   - Upstream OPL never sees these games.
3. **OPL inside the boot card (LDR-5).**
   - Updating OPL here means writing `APPS/…ELF` (and `OPL/conf_opl.cfg`) inside `BootCard-<ch>.mcd`. That needs a PS2 memory card file-system writer in `yaoplm-formats`. Today `vmc.rs` only creates blank cards and checks them.
   - mymcplusplus (GPL-3) may be ported with attribution; OPL and ps2sdk code (AFL) may not be copied (ADR-0002).
   - Writes go through the usual plan with a backup of the whole `.mcd`.
   - The installed build is known by the ELF version, as §4 shows.
4. **Saves (VMC-2).**
   - List, export and import saves (`.psu`, `.max`) in PS2 `.mcd` images and in PS1 128 KiB images.
   - Per-game cards in `MemoryCards/PS2/<ID>/`.
   - The same code reads OPL's `VMC/*.bin` and these cards.
5. **Card management.**
   - Create cards at a chosen size; 8 MiB recommended.
   - Name channels (`BootCard.ini`, `CardX.ini`), edit `Game2Folder.ini` and `settings.ini`. These are LF files whose last line must be empty (README).
   - Show `.crp.<n>` files.
   - Tell what a boot card holds from its content: the `B?EXEC-SYSTEM` folders and their `icon.sys` titles (FreeMcBoot, PS2BBL), and SD Builder images by hash. Then offer to fix the channel names, which are wrong on this very card (§4).
6. **Version detection.** Read a version from unpacked OPL ELFs (§4) or report "OPL, version unknown" (LDR-4 already has that state).
7. **A new SD from scratch.** The emulator branch of the preparation wizard (DEV-4) would write `.sd2psx/` and `MemoryCards/` with boot cards from the SD Builder, the way the site does. It downloads them on request and never bundles them (ADR-0007). FMCB's and PS2BBL's own licences have to be checked first, beyond the site's MIT. The alternative is restoring a backup YAOPLM took earlier.
8. **Not planned:** a firmware updater inside YAOPLM (owner, 2026-10-26). The README's UF2 drag-and-drop is enough.

## 7. Still to check

- On the device: the Info menu shows the Kingston card.
- On the console:
  - boot card channel 1 starts FreeMcBoot;
  - OPL 2216 (MMCE) lists games put in `CD/` and `DVD/` on the SD;
  - a game started from OPL creates `MemoryCards/PS2/<ID>/` at `CardSize`;
  - a game from a fragmented file plays.
- On the factory firmware: which version it was. Not recoverable now; another Gen2 or BitFunX's page would tell (bitfunx.com did not resolve from this machine on 2026-10-26).
