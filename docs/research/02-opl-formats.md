# 02 - Open PS2 Loader (OPL): what it reads from storage

Research date: 2026-10-22. Target: `ps2homebrew/Open-PS2-Loader`, branch `master` (default branch), at the commit behind the rolling `latest` pre-release (`b3a2199`).

How this was researched: WebFetch of `raw.githubusercontent.com` / GitHub API pages plus WebSearch. No binaries were downloaded or run, and no code was executed.

Verification tags used below:

| Tag | Meaning |
|---|---|
| **[V]** | Verified by reading the OPL source (file and function named) |
| **[D]** | Derived: computed or reasoned from verified source definitions (struct offsets, limits). Not compiled or tested |
| **[R]** | From the official README or repo metadata, not from code |
| **[U]** | Unverified: community docs, third-party tools or forum knowledge |

Code blocks are written for this report: offset tables, Python ports and examples. They describe the OPL behaviour named next to them. OPL's own C source is not copied verbatim. Identifiers, constants and format strings are quoted exactly.

---

## 1. Version, release date, license

| Item | Value | Tag |
|---|---|---|
| Latest stable release | **v1.1.0**, published 2021-09-09 (v1.0.0: 2021-01-06) | [V] GitHub releases API |
| Latest build | Rolling pre-release tag `latest`, published **2026-10-13**. Asset `OPNPS2LD-v1.2.0-Beta-2271-b3a2199.7z`, so the version string is **v1.2.0-Beta-2271-b3a2199** | [V] releases API |
| Version scheme | `Makefile`: `VERSION=1`, `SUBVERSION=2`, `PATCHLEVEL=0`, `EXTRAVERSION=Beta`. `REVISION` = `git rev-list --count HEAD` + 2, followed by the 7-character git hash | [V] Makefile |
| License | **AFL-3.0** (Academic Free License 3.0), from the GitHub API, README and file headers | [V] |
| License caveat | `src/cheatman.c` is ported from PS2rd and carries a GPLv3-or-later header. This only matters if code is reused; a PC tool will reimplement anyway | [V] |
| Build variants | Release (GSM, PADEMU, VMC, PS2RD cheats, parental controls), plus DTL_T10000, IGS, PADEMU and RTL variants. Makefile flags: `PADEMU ?= 1`, `IGS ?= 0`, `RTL ?= 0`, `DTL_T10000 ?= 0` | [V]/[R] |

Practical meaning: real users run either 1.1.0 or a 1.2.0 beta build. Most 2022–2026 features (ZSO, exFAT, BDM fragment tolerance, PNG-only art) exist **only in the 1.2.0 betas**. The tool should target 1.2.0 beta behaviour and flag where 1.1.0 differs.

---

## 2. Devices and backends

Modes come from `include/iosupport.h` `enum IO_MODES`: `BDM_MODE`, `BDM_MODE1` to `BDM_MODE4` (5 BDM units: `mass0:` to `mass4:`), `ETH_MODE`, `HDD_MODE`, `APP_MODE` [V].

| Backend | How OPL sees it | Filesystem | Notes | Tag |
|---|---|---|---|---|
| **USB mass storage** | BDM driver name `"usb"` (`usbmass_bd`). Prefix `massN:` | FAT32 and **exFAT** via `bdmfs_fatfs` (FatFs r0.16) | MBR and GPT. Fragment-tolerant (see 4.7) | [V] `src/bdmsupport.c` `bdmGetTextId`/`bdmUpdateDeviceData`, Makefile |
| **MX4SIO** (SD card on the memory card port) | BDM driver `"sdc"` (`mx4sio_bd`). Needs `enable_mx4sio=1` | FAT32/exFAT | | [V] |
| **iLink / IEEE1394 (SBP-2)** | BDM driver `"sd"` (`IEEE1394_bd`, `iLinkman`). Needs `enable_ilink=1` | FAT32/exFAT | `"sd"` here means iLink, not an SD card | [V] |
| **Internal HDD as BDM** | BDM driver `"ata"` (`ata_bd`). Needs `enable_bdm_hdd=1` | FAT32/exFAT on MBR (≤2 TB) or GPT (>2 TB) | Same folder layout as USB | [V] code, [R] README |
| **Internal HDD, APA/PFS** | `HDD_MODE`, `src/hddsupport.c`, `src/hdd.c` | Sony APA partitions, PFS for the OPL data partition | Games in **HDL format** (APA partition type `0x1337`). See section 13 | [V] |
| **SMB** | `ETH_MODE`, device `smb0:`, path separator `\` | Server-side | **SMBv1 only.** The only dialect is `"NT LM 0.12"`. Uses OPEN_ANDX, READ_ANDX (≤8192 bytes per request), WRITE_ANDX and 64-bit offsets. No SMB2/3. Port setting `smb_port`; NetBIOS name lookup through NBNS | [V] `modules/iopcore/cdvdman/smb.c`, `src/ethsupport.c` |
| **UDPBD** | **Not supported in official OPL.** `bdmsupport.c` has no `udp` code path. Feature requests #325, #1095 and #1290 are still open. A `gfx/udp_bd.png` asset exists but nothing uses it | n/a | | [V] (absence in code, issue search) |
| **MMCE** (SD2PSX, MemCard PRO 2 and other memory card emulators) | **Not in official OPL** (no MMCE code at `b3a2199`). Supported by: the `ps2-mmce/Open-PS2-Loader` fork, **only on branch `mmce_support`** (last commit 2025-11-24; `master` has none); wOPL (in its default branch); RiptOPL (both repos); Neutrino (`bsd-mmce.toml`) and NHDDL (`src/devices/mmce.c`). Protocol docs: `ps2-mmce/mmceman` `docs/`; card side: `sd2psXtd/firmware` `src/ps2/mmceman/` | FAT32/exFAT on the emulator's SD | Updated 2026-10-22 from local clones (research/REFERENCES.md) | [V] code in clones |
| **NBD server** | OPL can export the internal APA HDD over NBD (lwNBD), so PC tools such as `hdl_dump` can write to it | n/a | Replaces the old HDL server | [R] |

### FatFs build settings used by BDM

From `fjtrujy/FatFs` branch `iop-r0.16`, `source/include/ffconf.h`, which PS2SDK `download_dependencies.sh` clones [V]:

- `FF_FS_EXFAT 1`, `FF_LBA64 1` (GPT and >2 TB), `FF_USE_LFN 2`, `FF_MAX_LFN 255`.
- `FF_LFN_UNICODE 0` with `FF_CODE_PAGE 869`: names reach OPL as single-byte OEM code page 869 strings. **Use ASCII-only file names.**
- `FF_MIN_SS = FF_MAX_SS = 512`: only 512-byte logical-sector devices are supported. 4Kn drives will not work [D].
- The README says exFAT works "since OPL v1.2.0 beta - rev1880", and asks for the default allocation unit size when formatting exFAT [R]. **v1.1.0 has no exFAT.**

---

## 3. Folder layout on a device

`<root>` is the device prefix (`massN:`, `smb0:`, `pfs0:`) plus an optional sub-folder:

- `usb_prefix` (`gBDMPrefix`) for BDM: `massN:<prefix>/`
- `eth_prefix` (`gETHPrefix`) for SMB: `smb0:<prefix>\`

Both default to empty [V] (`src/opl.c` `setDefaults`, `src/bdmsupport.c` `bdmUpdateDeviceData`, `src/ethsupport.c`).

`sbCreateFolders()` in `src/supportbase.c` creates `CFG THM LNG ART VMC CHT APPS`, plus `CD DVD` for devices that hold disc images [V].

| Path | Contents | Read by | Tag |
|---|---|---|---|
| `<root>CD/` | CD-media images, `*.iso` / `*.zso`. Media is set to `SCECdPS2CD` (0x12) | `scanForISO(path, SCECdPS2CD)` | [V] |
| `<root>DVD/` | DVD5/DVD9 images, `*.iso` / `*.zso`. Media is set to `SCECdPS2DVD` (0x14) | `scanForISO(..., SCECdPS2DVD)` | [V] |
| `<root>CD/games.bin`, `<root>DVD/games.bin` | Binary cache of the scan result (see 4.6) | `loadISOGameListCache`, `updateISOGameList` | [V] |
| `<root>ul.cfg` | USBExtreme/USBAdvance game table | `sbReadList` | [V] |
| `<root>ul.<CRC>.<ID>.<nn>` | USBExtreme 1 GiB chunks, stored in the **root**, not in CD/DVD | `sbCreatePath` | [V] |
| `<root>CFG/<ID>.cfg` | Per-game settings and metadata (text) | `sbPopulateConfig`; HDD `hddGetConfig` | [V] |
| `<root>CFG/<ELF>.cfg` | Per-app config for **legacy** `conf_apps.cfg` apps | `oplGetLegacyAppsInfo` | [V] |
| `<root>CHT/<ID>.cht` | PS2RD-style cheat file | `sbLoadCheats` | [V] |
| `<root>VMC/<name>.bin` | Virtual memory card images | `sysCheckVMC`, launch code | [V] |
| `<root>ART/<ID>_<SUFFIX>.png` | Cover art and other images (section 7) | `bdmGetImage` and equivalents | [V] |
| `<root>THM/thm_<Name>/conf_theme.cfg` (+ images, fonts) | Themes. The folder name must contain `thm_`; the display name is the folder name minus its first 4 characters | `thmAddElements` | [V] |
| `<root>LNG/lang_<Name>.lng` (+ optional `font_<Name>.ttf` or `.otf`) | Translations. The first 5 characters of the file name are dropped to form the display name | `lngReadEntry`, `lngLoadFont` | [V] |
| `<root>APPS/<folder>/title.cfg` + ELF | Homebrew apps (section 11) | `oplScanApps` | [V] |
| `<root>conf_opl.cfg`, `conf_last.cfg`, `conf_apps.cfg`, `conf_network.cfg`, `conf_game.cfg` | Global settings, when saved to that device | `configInit` | [V] |
| `POPS/` | **Not created, scanned or read by OPL.** It belongs to POPStarter (section 10) | n/a | [V] (no `POPS`/`VCD` strings in `opl.c` or `appsupport.c`) |

Where the global config lives (`src/opl.c` `checkLoadConfigBDM`/`checkLoadConfigHDD`, `bdmFindPartition`) [V]:

- **BDM:** first match of `mass%d:<usb_prefix>/conf_opl.cfg` or `mass%d:conf_opl.cfg`, trying units 0 to 4.
- **HDD:** `pfs0:conf_opl.cfg` on the OPL partition.
- **Memory card:** `mc?:OPL/` (`gBaseMCDir = "mc?:OPL"`).
- The boot device is tried first (`tryAlternateDevice`).

On the APA HDD [V] `hddsupport.c`:

- `hdd0:__common` is mounted and `pfs0:OPL/conf_hdd.cfg` is read, using the key `hdd_partition`. If the file is missing it is created with `+OPL`.
- The OPL partition (default `hdd0:+OPL`) is mounted as `pfs0:`. Its root holds `CFG/ ART/ VMC/ CHT/ THM/ LNG/ APPS/ conf_*.cfg`.
- If the partition name does not start with `+` (for example `__common`), the prefix becomes `pfs0:OPL/`.

Paths are built into 256-byte buffers (`sbCreatePath_name`) [V].

---

## 4. ISO / ZSO discovery

### 4.1 Scan order
`sbReadList(prefix)` in `src/supportbase.c` [V]:

1. `scanForISO("<prefix>CD", SCECdPS2CD)`
2. `scanForISO("<prefix>DVD", SCECdPS2DVD)`
3. Read `<prefix>ul.cfg` and append one entry per 64-byte record.

On SMB the same function runs against `smb0:[prefix\]` [V] `ethsupport.c`.

### 4.2 File-name rules (`isValidIsoName`) [V]
- The extension must be `.iso` or `.zso`, compared case-insensitively on the last 4 characters. **No `.cso`, `.bin`, `.img` or `.chd`.**
- **"Old" format** `SLUS_200.62.Game Name.iso`: accepted when the total length is ≥17 and the characters at positions **4 = `_`, 8 = `.`, 11 = `.`**. The game ID is the first 11 characters. The title is the text between position 12 and the extension (`NameLen = len - 16`). The extension is the last 4 characters. **SYSTEM.CNF is not read and the entry is not cached.** Any file whose name happens to match this pattern is treated as old-format.
- **"New" format** (anything else with length ≥5): the title is the file name minus its extension (`NameLen = len - 4`). The ID is read from inside the image (4.3).
- Limits: the title must be **≤ `ISO_GAME_NAME_MAX` = 160** characters, or the file is skipped (`NameLen > ISO_GAME_NAME_MAX`). `ISO_GAME_EXTENSION_MAX = 4`, `GAME_STARTUP_MAX = 12` [V] `include/supportbase.h`.
- Directory entries are not type-checked; only names are filtered [V].
- **SMB-specific limit [D]:** the in-game SMB driver stores the image file name in `cdvdman_settings_smb.filename[160]` (`modules/iopcore/common/cdvd_config.h`). For ISO it holds `name + ext`; for old format, `ID.name + ext`. It is filled with `snprintf`, so **file names of 160 bytes or more get truncated and will not open on SMB.** Keep SMB image file names ≤159 bytes. The in-game path is `\<smb_prefix>\CD|DVD\<filename>` [V] `device-smb.c`.
- The display format attribute shows "ZSO" only if the extension is exactly lowercase `".zso"` (`strcmp` in `sbPopulateConfig`), but listing is case-insensitive. **Write lowercase extensions** [V].

### 4.3 Game ID detection (new format) [V]
`scanForISO` → `fileXioMount("iso:", <file>, FIO_MT_RDONLY)` (the `isofs` module reads both ISO and ZSO) → `GetStartupExecName("iso:/SYSTEM.CNF;1", startup, GAME_STARTUP_MAX - 1)` → unmount.

SYSTEM.CNF parsing (`src/ps2cnf.c` `ps2cnfGetBootFile`):

- At most **1023 bytes** of the file are read (`CNF_LEN_MAX 1024`).
- Lines split on `\n`; `\r` is treated as whitespace, so CRLF works.
- The key **`BOOT2`** is matched **case-sensitively**, with optional whitespace around `=`. The value ends at the first whitespace character. `BOOT` (PS1 discs), `VER` and `VMODE` are ignored.

`GetStartupExecName`:

- skips everything up to and including the first `:` (for example `cdrom0:`),
- skips any leading `\`,
- copies up to the first `;` or the end of the value,
- **truncates to 11 characters**.

Example: `BOOT2 = cdrom0:\SLUS_200.62;1` gives `SLUS_200.62`.

Consequences:

- An image with no `BOOT2` line (PS1 disc, DVD video, corrupt image) **silently disappears** from the list.
- An ELF in a sub-folder (`cdrom0:\DATA\MAIN.ELF;1`) gives the useless ID `DATA\MAIN.E` [D].
- **A PC tool should run the same parse to predict the ID** that names CFG, ART, CHT and VMC files.

The ID is used verbatim (e.g. `SLUS_200.62`) as `#Startup` and as the base name for `CFG/`, `ART/` and `CHT/` [V].

### 4.4 CD vs DVD [V]
- Media type comes **only from the folder** (`CD/` → 0x12, `DVD/` → 0x14). OPL does not inspect the image size or content.
- For ul.cfg games, the media byte in the record decides.
- For HDL games, `discType` in the HDL header decides.

### 4.5 DVD-9 (dual layer) [V] `bdmLaunchGame`, `sbGetISO9660MaxLBA`, `sbProbeISO9660`
- `layer1_start` = the PVD "volume space size" (u32 LE at sector 16, byte offset 80).
- If a second PVD (`\x01CD001`) is found at that LBA, `layer1_start -= 16` is passed to cdvdman. Otherwise the image is single-layer (`layer1_start = 0`).
- In practice, a DVD-9 image must be **layer 0 followed by layer 1, as a single image**, which is what normal dumps look like.
- `mediaLsnCount` = file size / 2048. For ZSO it is the uncompressed block count.
- Compatibility mode 5 ("Emulate DVD-DL") exists for split-layer cases.

### 4.6 Game list cache `games.bin` [V] / layout [D]
- One file per folder: `<root>CD/games.bin` and `<root>DVD/games.bin`, built with `"%s/games.bin"`.
- It is a raw array of in-memory `base_game_info_t` records, read and written with `fread`/`fwrite`. There is no header, magic or version.
- Lookup is by `name + extension` (the file name). On a hit, the cached ID and media are used **without mounting the image**.
- The file is rewritten when a game is missing from the cache or the count changes, and deleted when no games remain. Old-format names are never cached.
- **Stale-cache hazard [D]:** if a file is replaced by another image **with the same file name**, OPL keeps the old cached ID. A PC tool should **delete or regenerate `games.bin`** in any folder it modifies.

`base_game_info_t` layout, assuming EE GCC alignment (u32 aligned to 4, little-endian) [D]:

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0 | 161 | `name[ISO_GAME_NAME_MAX+1]` | Title, NUL-terminated |
| 161 | 13 | `startup[GAME_STARTUP_MAX+1]` | e.g. `SLUS_200.62` |
| 174 | 5 | `extension[5]` | `.iso` / `.zso` |
| 179 | 1 | `parts` | 1 for ISO |
| 180 | 1 | `media` | 0x12 or 0x14 |
| 181 | 1 | `format` | `GAME_FORMAT_USBLD=0`, `OLD_ISO=1`, `ISO=2` |
| 182 | 2 | padding | |
| 184 | 4 | `sizeMB` | Written as 0 by the scanner |
| **188** | | record size | **unverified, compiler-dependent** |

The tool could pre-generate this file (Python `struct` `"<161s13s5sBBB2xI"`) to skip OPL's mount-and-parse step, but deleting it is the safe default.

The HDD has its own optional cache: `pfs0:games.bin` (`hdd_game_list_cache=1`), a raw array of `hdl_game_info_t` [V].

### 4.7 Fragmentation (BDM) [V] `bdmLaunchGame`, `cdvd_config.h`
- At launch OPL asks the driver for each part's fragment list (`USBMASS_IOCTL_GET_FRAGLIST`).
- All parts of a game share **one array of `BDM_MAX_FRAGS` = 64 fragments** (`settings->frags[]`), so the **total fragments across all ul parts must be ≤ 64**. Otherwise OPL shows "Error, the game is fragmented."
- The README attributes 64-fragment tolerance to rev1893 (PR #673, June 2022) [R]. Older builds needed fully contiguous files.
- **Recommendation:** write each image (or chunk) with a single preallocated, sequential write to a freshly formatted or defragmented volume, then **verify fragment count ≤64 per game** (Windows `FSCTL_GET_RETRIEVAL_POINTERS`, Linux `FIEMAP`).

---

## 5. `ul.cfg` (USBExtreme / USBAdvance) and chunk files

### 5.1 Record layout: 64 bytes, packed, no header [V] `include/supportbase.h` `USBExtreme_game_entry_t`, `sbReadList`, `sbRebuildULCfg`

| Offset | Size | Field | Value |
|---|---|---|---|
| 0x00 | 32 | `name` | Game title. **Not** necessarily NUL-terminated: a full 32-byte name is used as-is. Shorter names are NUL-padded (OPL zero-fills the record) |
| 0x20 | 3 | `magic` | ASCII `ul.` |
| 0x23 | 12 | `startup` | Game ID, e.g. `SLUS_200.62` (11 chars) + NUL. Also not required to be terminated |
| 0x2F | 1 | `parts` | Number of chunk files |
| 0x30 | 1 | `media` | **0x12** = `SCECdPS2CD`, **0x14** = `SCECdPS2DVD` (PS2SDK `libcdvd-common.h`) |
| 0x31 | 4 | `unknown` | 0 |
| 0x35 | 1 | `Byte08` | **0x08** ("to be compatible with original ul.cfg") |
| 0x36 | 10 | `unknown2` | 0 |

- `ul.cfg` is a plain concatenation of records. OPL computes the count as `size / 64` and fills entries even if a record is corrupt [V].
- When OPL renames or deletes a ul game it rewrites the whole file (`sbRebuildULCfg`) and renames the chunks [V].
- `sbIsSameSize` detects ul.cfg changes by file size [V].
- iso2opl writes the same layout: `char name[32]; char image[15]` (= `"ul." + ID`)`; u8 parts; u8 media; u8 pad[15]` with `pad[4] = 0x08` (which is offset 0x35) [V] `pc/iso2opl/src/iso2opl.h`, `iso2opl.c`.

Python (writer side) [D]:
```python
import struct
def ul_record(name: str, game_id: str, parts: int, is_dvd: bool) -> bytes:
    n = name.encode("ascii")[:32]          # ASCII only, see 5.3
    i = game_id.encode("ascii")[:12]
    return struct.pack("<32s3s12sBB4sB10s", n, b"ul.", i, parts,
                       0x14 if is_dvd else 0x12, b"\0"*4, 0x08, b"\0"*10)  # 64 bytes
```

### 5.2 Chunk file naming [V]
- OPL builds the name with `"%sul.%08X.%s.%02x"` (`sbCreatePath_name`, `src/supportbase.c`). The in-game SMB driver uses the same `.%02x` suffix (`device-smb.c`).
  - `%08X` = `USBA_crc32(name)`, 8 **uppercase** hex digits.
  - `%s` = game ID.
  - `%02x` = part index, 2 **lowercase hex** digits, starting at `00`.
- Example: `ul.1A2B3C4D.SLUS_200.62.00`, `...01`, and so on.
- `pc/iso2opl` writes the part index with `%02d` (decimal). That is identical for parts 0–9, which covers every real disc (DVD-9 ≤ 8.5 GB = 9 parts). Use lowercase hex to match the reader exactly.
- Chunks live in the device root (or `usb_prefix` / `eth_prefix` folder), beside ul.cfg.

### 5.3 The name hash `USBA_crc32` [V] `src/system.c`
This is **not** standard CRC-32 (zlib/IEEE).

1. A 256-entry table is regenerated on every call. For each `t` in 0..255:
   - start with `crc = t << 24` (signed 32-bit),
   - repeat 8 times: if the top bit **is set**, `crc <<= 1`; otherwise `crc = (crc << 1) ^ 0x04C11DB7`. This is the inverse of the usual condition.
   - store the result at `tab[255 - t]`.
2. `crc` is **not reset** before hashing. It starts with the value left by the last table iteration, `t = 255` (= `tab[0]`).
3. Bytes are processed **including the terminating NUL**, `crc = tab[b ^ ((crc >> 24) & 0xFF)] ^ ((crc << 8) & 0xFFFFFF00)`. The loop stops after the NUL or after 33 bytes (condition `count <= 32`).
4. No final XOR. The result is formatted as `%08X`.
5. The parameter is `const char *`, which is signed on the EE. A byte ≥0x80 becomes a negative index into the table (undefined behaviour). **Restrict ul names to 7-bit ASCII.**
6. The hash input is the name **as stored in ul.cfg**, at most 32 characters. OPL hashes the NUL-terminated copy `g->name[33]`.

Python port, written for this report and faithful to the steps above [D]. **Validate it against a known USBUtil/OPL-created set before release; no test vector was computed here**:
```python
def usba_crc32(name: bytes) -> int:
    tab = [0] * 256
    crc = 0
    for t in range(256):
        crc = (t << 24) & 0xFFFFFFFF
        for _ in range(8):
            if crc & 0x80000000:                      # (int)crc < 0
                crc = (crc << 1) & 0xFFFFFFFF
            else:
                crc = ((crc << 1) ^ 0x04C11DB7) & 0xFFFFFFFF
        tab[255 - t] = crc
    # crc intentionally NOT reset (holds the t=255 value)
    s = name + b"\x00"
    count = 0
    while True:
        b = s[count]; count += 1                     # must be < 0x80
        crc = (tab[b ^ ((crc >> 24) & 0xFF)] ^ ((crc << 8) & 0xFFFFFF00)) & 0xFFFFFFFF
        if s[count - 1] == 0 or count > 32:
            break
    return crc

def ul_chunk_name(name: str, game_id: str, part: int) -> str:
    return "ul.%08X.%s.%02x" % (usba_crc32(name.encode("ascii")), game_id, part)
```

### 5.4 Chunk size and part limits
- **Every chunk except the last must be exactly 1 GiB (1,073,741,824 bytes = 0x80000 sectors).**
  - The SMB in-game driver maps an LSN to a part by stepping `0x80000` sectors per part (`device-smb.c` `DeviceReadSectors`) [V].
  - `ethLaunchGame` computes the layer-1 part as `layer1_start / 0x80000` [V].
  - iso2opl splits at 1073741824 [V].
- Max parts:
  - The `parts` field is a u8.
  - **SMB: `ISO_MAX_PARTS` = 10** (`cdvdman_settings_smb.FIDs[10]`) [V].
  - **BDM: no explicit limit**, but at least one fragment per part counts against the 64-fragment budget [V].
  - The CD/DVD media byte decides CD vs DVD.
- ul format is only needed for **FAT32 and images > 4 GiB**. With exFAT or SMB, plain ISOs in `DVD/` are simpler [R]. ul titles are limited to **32 bytes** (iso2opl rejects longer names) [V].

---

## 6. Configuration files

### 6.1 Text format [V] `src/config.c`
- One `key=value` per line. Leading spaces and tabs are skipped. The key is the text before the first `=` and the value is the rest of the line. **Trailing whitespace is not trimmed.**
- Section syntax: a line `name:` with no `=` sets a prefix. Following **indented** lines become `name_key`, and any non-indented line clears the prefix. Themes use this (`main0:` then `\ttype=Background`).
- There is no comment syntax. A `#` line without `=` is logged as malformed.
- Written lines use `"%s=%s\r\n"` (**CRLF**). Keys starting with `#` are **never written**: they are runtime-only values injected by OPL.
- Limits: `CONFIG_KEY_NAME_LEN 32` (key ≤31 chars), `CONFIG_KEY_VALUE_LEN 256` (value ≤255 chars). Integers are written with `%d` and read with `atoi`.
- Reads use a 4096-byte buffer.

### 6.2 Global files [V] `configFilenames[]`
`conf_opl.cfg`, `conf_last.cfg`, `conf_apps.cfg`, `conf_network.cfg`, `conf_game.cfg`. These sit in the config directory (section 3). Legacy `IPCONFIG.DAT` is also read. On the APA HDD, `hdd0:__common` → `OPL/conf_hdd.cfg` holds `hdd_partition`.

- **conf_opl.cfg** keys (`include/config.h`):
  - Appearance: `theme`, `language_text`, `scrolling`, `bg_color`, `text_color`, `ui_text_color`, `sel_text_color`, `enable_notifications`, `enable_coverart`, `wide_screen`, `vmode`, `xoff`, `yoff`, `overscan`.
  - Boot and behaviour: `disable_debug`, `ps2logo`, `hdd_game_list_cache`, `exit_path`, `autosort`, `autorefresh`, `default_device`, `enable_delete_rename`, `hdd_spindown`, `remember_last`, `autostart_last`, `swap_select_btn`, `parental_lock_password`.
  - Devices: `usb_prefix`, `eth_prefix`, `usb_mode`, `hdd_mode`, `eth_mode`, `app_mode`, `bdm_cache`, `hdd_cache`, `smb_cache`, `enable_ilink`, `enable_mx4sio`, `enable_bdm_hdd`.
  - Sound: `enable_sfx`, `enable_boot_snd`, `enable_bgm`, `sfx_volume`, `boot_snd_volume`, `bgm_volume`, `default_bgm_path`.
  - Input: `x_sensitivity`, `y_sensitivity`.
  - The `*_mode` values are `START_MODE`: 0 = off, 1 = manual, 2 = auto [V] `iosupport.h`.
- **conf_network.cfg:** `eth_linkmode`, `ps2_ip_use_dhcp`, `ps2_ip_addr`, `ps2_netmask`, `ps2_gateway`, `ps2_dns`, `smb_share_nb_addr`, `smb_ip`, `smb_share_use_nbns`, `smb_share`, `smb_user`, `smb_pass`, `smb_port`, `nbd_default_export`.
- **conf_game.cfg:** global defaults for the per-game options whose `...Source` key is "global": GSM, cheats, PADEMU, pad macro and OSD language [V] `guigame.c`.
- **conf_apps.cfg:** legacy app list (section 11).

### 6.3 Per-game `CFG/<ID>.cfg`
Path `"%sCFG%s%s.cfg"` (prefix, separator, ID) [V] `sbPopulateConfig`.

**Runtime-only `#` keys** (set by OPL, never saved) [V]: `#Name`, `#LongName`, `#Size` (MB), `#Format` (`ISO`/`ZSO`/`UL`; HDD: `HDL`; apps: `ELF`), `#Media` (`CD`/`DVD`; apps: `APP`), `#Startup` (ID or ELF path).

**Settings keys** [V] `include/config.h`, `src/guigame.c`, `src/supportbase.c`, `src/hddsupport.c`, `src/config.c`. OPL removes a key when it is set back to its default.

| Key | Values |
|---|---|
| `$Compatibility` | Integer bitmask (table below) |
| `$DMA` | HDD only, 0–7. 0–2 = MDMA 0–2 (`dma_type 0x20`), 3–7 = UDMA 0–4 (`dma_type 0x40`, mode − 3). Default 7, which is not stored |
| `$DNAS` | Disc ID as hex. The first 10 hex digits become 5 bytes; non-hex separators are ignored (`configGetDiscIDBinary`) |
| `$AltStartup` | Alternate boot ELF on the disc, used instead of the SYSTEM.CNF ID (`cdrom0:\<value>;1`). ≤31 chars, uppercased by the UI. For apps it is reused as argv1 |
| `$VMC_0`, `$VMC_1` | VMC base name for slot 1 / slot 2 (file `VMC/<name>.bin`). ≤31 chars |
| `$ConfigSource` | Read-only marker: 1 = user-customised, 2 = downloaded defaults (`CONFIG_SOURCE_USER`/`DLOAD`) |
| `$ParentalLock` | `1` = locked |
| `$GSMSource` | 0 = global (`conf_game.cfg`), 1 = per-game |
| `$EnableGSM` | 0/1 |
| `$GSMVMode` | 0–28. 0 NTSC, 1 NTSC non-interlaced, 2 PAL, 3 PAL NI, 4 PAL@60, 5 PAL@60 NI, 6 PS1 NTSC (480p), 7 PS1 PAL (576p), 8 480p, 9 576p, 10 720p, 11 1080i, 12 1080i NI, 13–17 VGA 640x480 @60/72/75/85 and 640x960i, 18–22 VGA 800x600 @56/60/72/75/85, 23–26 VGA 1024x768 @60/70/75/85, 27–28 VGA 1280x1024 @60/75 |
| `$GSMXOffset`, `$GSMYOffset` | Integers |
| `$GSMFIELDFix` | 0/1 |
| `$CheatsSource` | 0 = global, 1 = per-game |
| `$EnableCheat` | 0/1 |
| `$CheatMode` | 0 = auto (all codes), 1 = select at launch |
| `$CheatSel000`, `$CheatSel001`, ... | Names of the cheats selected in "select" mode (`"%s%03d"`) |
| `$PADEMUSource` | 0 = global, 1 = per-game |
| `$EnablePadEmu` | 0/1 |
| `$PadEmuSettings` | Bitfield. Bits 0–7 mode (0 = DS3/4 USB, 1 = BT); bits 8–11 per-port enable; bits 16–19 per-port vibration; bit 24 multitap; bit 25 multitap port; bit 26 workaround. Approximate |
| `$PadMacroSource`, `$PadMacroSettings` | Pad macro (stick slowdown, invert, turbo) bitfield |
| `$OSDSettingsEnable` | 0/1 |
| `$CustomLanguageSource` | 0 = global, 1 = per-game |
| `$CustomLanguageValue` | 0 JA, 1 EN, 2 FR, 3 ES, 4 DE, 5 IT, 6 NL, 7 PT, 8 system |
| `$OSDAspectRatio` | 0 = 4:3, 1 = full, 2 = 16:9, 3 = system |
| `$OSDVideoMode` | 0 = RGB, 1 = YPbPr, 2 = system |

**`$Compatibility` bits** [V] `include/iosupport.h`, `sbPrepare`, `lng_tmpl/_base.yml`:

| Mode | Bit | Meaning | Effect |
|---|---|---|---|
| 1 | 0x01 | Accurate Reads | `IOPCORE_COMPAT_ACCU_READS` |
| 2 | 0x02 | Synchronous Mode (alternative data read) | `IOPCORE_COMPAT_ALT_READ` |
| 3 | 0x04 | Unhook Syscalls | Handled in the EE core (`_CompatMask`) |
| 4 | 0x08 | Skip Videos (PSS reported as 0 bytes, Bink skipped) | `IOPCORE_COMPAT_0_SKIP_VIDEOS` |
| 5 | 0x10 | Emulate DVD-DL | `IOPCORE_COMPAT_EMU_DVDDL` |
| 6 | 0x20 | Disable IGR | Also sets `IOPCORE_ENABLE_POFF` |
| 7, 8 | 0x40, 0x80 | **Unused** in current code (`COMPAT_MODE_COUNT 6`) | Old 0.9.x used mode 7 for HDD stutter fixes; ignore it |

Example: `$Compatibility=3` = modes 1 and 2.

**Metadata keys read by themes, not by OPL logic** [V] `misc/conf_theme_OPL.cfg`, `src/themes.c`:

- **AttributeText** elements in the default info page: `Title`, `Genre`, `Release`, `Developer`, `#Size`, `Description` (wrapped, 574×80 box). Custom themes can display any key.
- **AttributeImage** elements: `#Media`, `#Format`, `Vmode`, `Aspect`, `Scan`, `Rating` (default `Rating_0`), `Device`.
  - With the built-in theme, a value of the form `anything/X` is looked up as the internal texture `<Attribute>_X`. Example: `Vmode=vmode/ntsc` → `Vmode_ntsc`.
  - A value without `/` is used as the texture name itself.
- Built-in textures (`gfx/`) define the useful values:

| Attribute | Values |
|---|---|
| `Aspect` | `s`, `w`, `w1`, `w2` |
| `Scan` | `240p`, `240p1`, `480i`, `480p`, `480p1`–`480p5`, `576i`, `576p`, `720p`, `1080i`, `1080i2`, `1080p` |
| `Vmode` | `multi`, `ntsc`, `pal` |
| `Rating` | `0`–`5` |
| `Device` | `1`–`6`, `all` |
| Media/format images | `CD`, `DVD`, `ISO`, `ZSO`, `UL`, `HDL`, `ELF`, `APPS` |

**Community metadata conventions** [U], as seen in the Veritas83 PS2-OPL-CFG database and OPL Manager files:
- Common keys: `CfgVersion=7`, `Title=`, `Release=MM-DD-YYYY`, `Developer=`, `Genre=`, `Players=players/N`, `Vmode=vmode/ntsc`, `VmodeText=NTSC`, `Description=` (≤255, the OPL value limit), `Rating=rating/N`, `Aspect=aspect/w`, `Scan=scan/480p`.
- Keep `$`-keys untouched when merging metadata.

---

## 7. ART

**Lookup** [V] `src/bdmsupport.c` `bdmGetImage`, `src/textures.c` `texDiscoverLoad`, `src/themes.c`:
- Path pattern `"%s%s/%s_%s"` + `.png` → `<root>ART/<ID>_<SUFFIX>.png`, for example `ART/SLUS_200.62_COV.png`. The `<ID>` is the game ID / `#Startup`, not the file name.
- Apps use `ART/<boot ELF file name>_<SUFFIX>.png`, e.g. `ART/OPNPS2LD.ELF_COV.png` or `ART/XX.Crash Bandicoot.ELF_COV.png` (`appGetItemStartup` returns the `boot` value) [V].
- For apps, the launching device is searched first, then other devices by priority (fastest first) [V] `oplGetAppImage`.

**Formats:**
- **Current 1.2.0 beta: PNG only.** BMP and JPG were removed on 2024-10-22 (commit `c5a12a0`) [V].
- v1.1.0 tried `png`, then `jpg`, then `bmp` [V] `v1.1.0/src/textures.c`. **Ship PNG.**
- Supported PNG types [V]:
  - palette 4-bit or 8-bit (tRNS alpha supported),
  - RGB 8-bit,
  - RGBA 8-bit,
  - 16-bit channels are stripped to 8-bit,
  - grayscale handling is unclear, so convert it to RGB.
- Rejected: width or height **> 1024**, or a texture larger than 720×512×4 bytes [V] `texSizeValidate`.
- Small images keep loading fast and preserve EE RAM [D].

**Suffixes** [V]:
- Used by the built-in theme: **`COV`** (cover, cache of 10), **`ICO`** (icon, cache of 20), **`BG`** (background pattern on the info page), **`SCR`** and **`SCR2`** (two screenshots on the info page, drawn 173×148).
- Anything else (`COV2`, `LAB`, `LGO`, ...) is only shown when a **custom theme** asks for it with `pattern=<SUFFIX>`, which can be any string.
- The default theme draws the cover into a "case" overlay roughly 168×245 for games and 168×171 for apps.

**Recommended pixel sizes** [U] (OPL Manager documentation; the art database enforces them):

| Suffix | What | PS2 size | PS1 size |
|---|---|---|---|
| `COV` | Front cover | 140×200 | 200×200 |
| `COV2` | Back cover | 242×344 | 222×200 |
| `LAB` | Spine | 18×240 | 12×200 |
| `ICO` | Icon | 64×64 | 64×64 |
| `LGO` | Logo | 300×125 | 300×125 |
| `SCR`, `SCR2` | Screenshots | 250×188 | 250×188 |
| `BG` | Background | 640×480 | 640×480 |

Some forks (RiptOPL) use double-size `COV`/`ICO` (280×400, 128×128).

---

## 8. Cheats: `CHT/<ID>.cht` [V] `src/supportbase.c` `sbLoadCheats`, `src/cheatman.c`, `include/cheatman.h`, `src/cheatconfig.c`

- Path `"%sCHT/%s.cht"`, built in a **64-byte buffer**, so keep `usb_prefix` short. Cheats are loaded only when `$EnableCheat` is on (per game or global).
- Plain text, lines split on LF and trimmed (CR is fine). `//` or `#` starts a comment anywhere on a line. Lines are capped at 255 characters.
- A **code line** is any line with exactly **16 hex digits** after removing whitespace, read as `addr val`, e.g. `2032D3B4 00000063`. `00000000 00000000` is ignored.
- **Any other non-empty line is a cheat name** for the codes that follow, truncated to 128 characters. Codes before the first name are dropped.
  - A quoted title line such as `"Game /ID SLUS_200.62"` is not special. It becomes a cheat name and is overwritten if another name follows before any code.
- **Master codes** are recognised **by name only** (after trimming, collapsing whitespace and lowercasing): `mastercode`, `master code`, or `enable code (must be on)`. They are always enabled, even in select mode.
- **Codes are raw (decrypted) PS2RD codes.** There is no decryption and no CodeBreaker, Action Replay or GameShark handling. Encrypted codes must be converted first (for example with Omniconvert) [U].
- Limits:
  - `MAX_CODES 250` cheat entries.
  - `MAX_HOOKS 5`.
  - The flattened list passed to the engine is `MAX_CHEATLIST = 2*5 + 2*250 = 510` u32, about 254 addr/value pairs plus a 0,0 terminator for **all enabled codes combined** [D].
- In select mode the chosen cheats are saved as `$CheatSel000=<cheat name>` and so on in `CFG/<ID>.cfg`.

Example (written for this report; the hex values are **placeholders, not real codes**):
```
"Grand Theft Auto III /ID SLUS_200.62"
Mastercode
9029A2A8 0C0A6A1E
Infinite Health
2032D3B4 00000063
```

**PCSX2 `.pnach` relation** [U] (format from pcsx2.net "Writing patches"; code-type mapping from PS2RD conventions):
- pnach lines are `patch=<place>,<cpu>,<addr>,<type>,<data>`.
- The `extended` type takes the same raw PS2RD-style code, so `AAAAAAAA VVVVVVVV` ↔ `patch=1,EE,AAAAAAAA,extended,VVVVVVVV`.
- PCSX2 does **not** support type `9` hooks (master codes) or `C` codes. Drop master codes when going .cht → .pnach.
- When going .pnach → .cht, generate a master code only if you have one; OPL generally needs the game-specific hook for codes to run.
- `word`, `short` and `byte` patches map to raw type 2, 1 and 0 with the address in the low 28 bits.
- pnach files are named `<SERIAL>_<ELF-CRC>.pnach`, e.g. `SLUS-20062_xxxxxxxx.pnach`. Converting **to** pnach needs the ELF CRC (PCSX2's hash of the boot ELF), so the PC tool must read the ELF from the ISO. Converting **from** pnach to OPL needs only the serial: `SLUS-20062` → `SLUS_200.62`.
- `[Group\Name]` and `gametitle=`/`description=` metadata map to cheat names.

---

## 9. VMC (virtual memory cards)

**Usage** [V] `src/system.c` `sysCheckVMC`, `src/bdmsupport.c`, `src/hddsupport.c`, `src/ethsupport.c`, `src/guigame.c`, `src/vmc_groups.c`:
- File `<root>VMC/<name>.bin` (`"%sVMC%s%s.bin"`), selected per game with `$VMC_0` and `$VMC_1`. Names are ≤31 chars.
- The UI suggests `<GroupID or ID>_<slot>` with slot **0/1** (e.g. `SLUS_200.62_0`), or `generic_0` / `generic_1`.
- **VMC groups** (`vmc_groups.c`) map related title IDs to a shared group ID such as `XEBP_100.01`, so a game series shares one card.
- Created only from the VMC menu (not on launch), through the IOP module `genvmc` (`GENVMC_DEVCTL_CREATE_VMC 0xC0DE0001`) with `VMC_blocksize = 16`.
- Sizes: **8, 16, 32, 64 MB**.

**OPL validation** at check and launch [V]:
- `magic` starts with `Sony PS2 Memory Card Format` (27 characters compared).
- `mc_type == 2`.
- File size == `pages_per_cluster × clusters_per_card × page_size`, and the size is a whole number of MB.
- **BDM:** the file must be **one contiguous cluster chain** (`USBMASS_IOCTL_CHECK_CHAIN`) and lie entirely below LBA 2^32. Otherwise: "VMC ... file is fragmented. Continue with Memory Card in slot N?"
- **HDD (PFS):** at most 5 APA parts and 10 PFS block segments (`hdd_vmc_infos_t`).
- **SMB:** accessed through the file.

**Format.** Standard PS2 memory-card filesystem, **raw user data without the 16-byte spare/ECC area**: 512-byte pages, 1024-byte clusters, file size exactly N MiB [V] `modules/vmc/genvmc/genvmc.c`, `include/mcemu.h`.

Superblock (cluster 0, 512 bytes) [V] field list, [D] offsets:

| Off | Size | Field | genvmc value |
|---|---|---|---|
| 0x000 | 28 | magic | `Sony PS2 Memory Card Format ` (28 chars, trailing space) |
| 0x01C | 12 | version | `1.2.0.0` |
| 0x028 | u16 | page_size | 512 |
| 0x02A | u16 | pages_per_cluster | 2 |
| 0x02C | u16 | pages_per_block | 16 |
| 0x02E | u16 | (0xFF00) | |
| 0x030 | u32 | clusters_per_card | size_MB × 1024 (8 MB → 8192) |
| 0x034 | u32 | alloc_offset (first allocatable cluster) | after the IFC and FAT clusters |
| 0x038 | u32 | alloc_end | clusters up to the two backup blocks, minus alloc_offset |
| 0x03C | u32 | rootdir_cluster | 0 (relative to alloc_offset) |
| 0x040 | u32 | backup_block1 | last block |
| 0x044 | u32 | backup_block2 | second-to-last block |
| 0x050 | u32[32] | ifc_list (indirect FAT clusters) | starting at cluster `blocksize/2` = 8; unused entries −1 |
| 0x0D0 | u32[32] | bad_block_list | all −1 |
| 0x150 | u8 | card_type | 2 |
| 0x151 | u8 | card_flags | 0x2B |
| 0x154 | u32 | cluster_size | 1024 |
| 0x158 | u32 | (0x100) | |
| 0x15C | u32 | size_in_megs | |
| 0x170 | u32 | max_used / allocatable | about 97% |
| 0x180–0x1FF | | reserved | 0xFF fill |

Other layout details [V]:
- The whole file is first filled with 0xFF.
- FAT entries are u32, 256 per cluster. Free = `0x7FFFFFFF`, end of chain = `0xFFFFFFFF`.
- The root directory cluster holds the `.` and `..` entries (512-byte `McFsEntry`; mode includes exists, subdir, rwx; `..` is hidden).
- Timestamps come from the CDVD RTC (fallback 2000-03-04).

A PC tool can generate identical cards with any correct ps2mc-format writer (e.g. a mymc-style implementation) [U]. It must write no ECC, an exact size, and the file must be contiguous on BDM.

---

## 10. POPStarter (PS1) with OPL

**OPL has no native PS1 support** [V]: no `POPS`/`VCD` handling in `src/opl.c` or `src/appsupport.c`. PS1 games are started as **apps** that launch a POPStarter ELF.

The layout below is the POPStarter convention [U]: community POPStarter documentation (nathanneurotic.github.io/POPSTARTERINFO, mirrors of the old ShaolinAssassin Bitbucket wiki) and forum consensus. How to obtain the proprietary POPS files is not covered here.

**USB / BDM:**
- `mass:/POPS/` holds the POPS runtime files (POPStarter expects `POPS_IOX.PAK` here, plus IGR textures) and the games as **`<Name>.VCD`**.
- Optional extra folders `POPS0` to `POPS9`.
- **Per-game support folder** `mass:/POPS/<VCD basename>/` holds `SLOT0.VMC` and `SLOT1.VMC` (PS1 card saves), `CHEATS.TXT`, `DISCS.TXT` (multi-disc list, one VCD file name per line, the same in each disc's folder), `VMCDIR.TXT` (later discs point to disc 1's saves) and other per-game fixes.
- Launcher: a copy of `POPSTARTER.ELF` renamed **`XX.<VCD basename>.ELF`**. The `XX.` prefix tells POPStarter it is in USB mode.
- **Naming convention for ID detection:** `SLUS_002.40.Soul Blade.VCD`. Tools and art databases key PS1 art on this ID or on the ELF name. Names are case-sensitive and must match exactly between `.VCD`, `XX.*.ELF` and the support folder.

**SMB:** launcher prefix `SB.`; support modules in `mc?:/POPSTARTER/`.

**HDD (APA):** VCDs in the `__.POPS` partition (also `__.POPS0`–`__.POPS9`), support files in `hdd0:__common/POPS/`. The legacy per-game `PP.` partition method also exists.

**OPL integration** [V for OPL side]:
- Put the launcher in an APPS folder, e.g. `APPS/PS1_Soul Blade/` containing `XX.Soul Blade.ELF` (or `XX.SLUS_002.40.Soul Blade.ELF`) and `title.cfg` with `title=Soul Blade` and `boot=XX.Soul Blade.ELF`. Alternatively, a legacy `conf_apps.cfg` entry.
- The tool must keep the **ELF name identical to the VCD basename** [U].
- PS1 art goes in `ART/<boot ELF name>_COV.png`, e.g. `ART/XX.Soul Blade.ELF_COV.png` [V lookup logic], with PS1 sizes from section 7 [U].
- The POPStarter docs say POPStarter has no exFAT support of its own on USB; the documented workaround is a BDM driver replacement [U].
- Expect VCDs to need contiguity as well [U].

---

## 11. APPS [V] `src/opl.c` `oplScanApps`, `src/appsupport.c`, `include/appsupport.h`

- Each enabled device is scanned as `<root>APPS/`. **Every sub-folder** containing `title.cfg` is one app.
- Memory cards `mc0:` and `mc1:` are scanned at the root, and only folders whose names contain `_` count.
- `title.cfg` uses the normal config syntax (section 6.1). Keys are **lowercase and case-sensitive**:
  - `title=<display name>` (≤128) — required
  - `boot=<ELF file name inside that folder>` (≤64, name only, no path) — required
  - `argv1=<one argument>` (≤128) — optional
- An entry without `title` or `boot` is skipped.
- The launch path is `<folder path>/<boot>` (`APP_PATH_MAX 128` applies to the folder path).
- Apps use `title.cfg` itself as their config. `#Format=ELF`, `#Media=APP`. argv1 is passed as `argv[0]` through `$AltStartup`.
- **Legacy `conf_apps.cfg`:** lines `Title=device:path/to/FILE.ELF`. The file is looked for at `mc?:OPL/conf_apps.cfg`, then `<root>conf_apps.cfg` on each enabled device.
  - `mass:` paths are rewritten to `mass?:` and probed on all BDM units.
  - Per-app settings live in `CFG/<ELF name>.cfg`.
  - Paths are case-sensitive and the ELF can be anywhere.
- Art: `ART/<boot>_<SUFFIX>.png` (section 7).

Example:
```
APPS/
  WLE_ISR/
    title.cfg      -> title=wLaunchELF
                      boot=WLE.ELF
    WLE.ELF
```

---

## 12. ZSO compression (and CSO)

**History** [V] git log and PR search:
- Added 2022-05-26, commit `c34a072` "Added ZSO code, sector cache, and ZISO compressor" (JoseAaronLopezGarcia; original PR #641 was merged out-of-band). Reader cleanups: #707 and #708 (2022-07).
- OOB fixes plus a new `pc/ziso.py`: #1209 (2024-04).
- MX4SIO unaligned-read fix: #1783 (2026-10-05).
- **ZSO exists only in v1.2.0 betas. v1.1.0 does not read ZSO** (README: "supported as of v1.2.0") [R].

**Detection** [V]:
- Listing needs the `.zso` extension (section 4.2).
- At launch, `ProbeZISO` reads the 24-byte header plus the first index entry and checks `magic == ZSO_MAGIC` (`0x4F53495A`, ASCII `ZISO` in little-endian).
- Works on BDM, SMB and HDL partitions (`hddsupport.c` ZSO branch), and with DVD-9 layer detection.

**Header** (24 bytes, little-endian) [V] `modules/isofs/zso.h`, `pc/ziso.py`:

| Off | Type | Field | Value |
|---|---|---|---|
| 0 | u32 | magic | `ZISO` |
| 4 | u32 | header_size | 0x18 |
| 8 | u64 | total_bytes | Uncompressed ISO size |
| 16 | u32 | block_size | **2048** (the OPL reader hard-codes 2048-byte blocks) |
| 20 | u8 | ver | 1 |
| 21 | u8 | align | Index shift |
| 22 | u8[2] | reserved | 0 |

**Index and data** [V] `modules/isofs/zso.c`, `pc/ziso.py`:
- At offset 0x18 there are `(total_bytes / 2048) + 1` u32 entries.
- Each entry: bit 31 = **plain** (block stored uncompressed); bits 0–30 = file offset `>> align`.
- The size of block *n* = `(idx[n+1] & 0x7FFFFFFF) − (idx[n] & 0x7FFFFFFF)`, shifted left by `align`.
- Compressed blocks are **raw LZ4 block format** (no LZ4 frame, no size prefix), decompressed with `LZ4_decompress_fast(..., 2048)`. Each compressed block must decode to exactly 2048 bytes.
- Plain blocks are copied (at most 2048 bytes; padding ignored).
- Number of sectors = total_bytes / 2048.

**Writer reference** `pc/ziso.py` [V]:
- `align = total_bytes // 2**31` (so 0 for images < 2 GiB, 1 for 2–4 GiB, ...).
- Pad before each block with `X` bytes up to a multiple of `1 << align`.
- Store a block plain if the compressed size is ≥95% of 2048.
- LZ4 high-compression when the level is > 1.
- The last index entry is the end offset.

**CSO** (`CISO`, deflate): **not supported**. Only the ZISO magic and LZ4 exist in the code [V]. A tool may convert CSO → ISO → ZSO, but CSO files placed in CD/DVD are ignored (wrong extension).

---

## 13. HDD (APA) game format, briefly [V] `src/hdd.c`, `include/hddsupport.h`

- OPL lists every APA partition with `stat.mode == HDL_FS_MAGIC (0x1337)`. **Partition names are not parsed** (the `PP.`/`__.` conventions are hdl_dump's). Sub-partitions are grouped by name.
- The HDL header is read from partition start + `(0x100000 + 0x1000)/512` = **sector 0x808** (byte 0x101000): 2 sectors, 1024 bytes.

`hdl_apa_header` [V] fields, [D] offsets:

| Offset | Field |
|---|---|
| 0x000 | `checksum` (HDL writes 0xDEADFEED) |
| 0x004 | `magic` |
| 0x008 | `gamename[160]` |
| 0x0A8 | `hdl_compat_flags` |
| 0x0A9 | `ops2l_compat_flags` |
| 0x0AA | `dma_type` |
| 0x0AB | `dma_mode` |
| 0x0AC | `startup[60]` |
| 0x0E8 | `layer1_start` |
| 0x0EC | `discType` |
| 0x0F0 | `num_partitions` |
| 0x0F4 | `part_specs[65]` (`{part_offset MB, data_start sectors, part_size KB}`) |

- OPL keeps 64 characters of the name (`HDL_GAME_NAME_MAX`) and 12 of the ID.
- Renames rewrite the name and `ops2l_compat_flags`, `dma_type` and `dma_mode`.
- **Current OPL takes compatibility from `CFG/<ID>.cfg` (`$Compatibility`), not from the header.**
- Writing HDL partitions from a PC needs a full APA/HDL writer (hdl_dump-equivalent) or OPL's NBD server [U].

---

## 14. Implementation checklist for the PC tool (derived)

1. Create the folders `CD DVD CFG CHT VMC ART THM LNG APPS` in the root, or under `usb_prefix`.
2. Images:
   - Use `.iso`/`.zso` with lowercase extensions and ASCII names.
   - Title ≤160 characters, file name ≤159 bytes for SMB.
   - Choose CD/DVD by the real media type. PS2 CD-ROM games go in `CD/`; the usual heuristic is image size < 700 MB, but some DVD games are small, so allow an override.
   - Verify `BOOT2` exists, extract the ID with OPL's exact parse, and keep the image to ≤64 fragments on BDM.
   - Delete `games.bin` in folders you changed.
3. FAT32 with images > 4 GiB:
   - Write ul format: exact 1 GiB chunks named `ul.%08X.%s.%02x` using `USBA_crc32`.
   - Append a 64-byte record to `ul.cfg` (media 0x12/0x14, byte 0x35 = 0x08).
   - Titles ≤32 ASCII; ≤10 parts for SMB.
4. Art: PNG only, `ART/<ID>_<SUFFIX>.png`, ≤1024 px. Recommended sizes in section 7.
5. CFG:
   - CRLF, `key=value`, keys ≤31, values ≤255.
   - Never write `#` keys. Preserve unknown keys.
   - Metadata uses `Title`/`Genre`/`Release`/`Developer`/`Description` and `Vmode=vmode/ntsc`-style image values.
6. Cheats: raw 8+8 hex lines. Master codes are named "Mastercode". Convert pnach `extended` lines 1:1.
7. VMC:
   - Exact 8/16/32/64 MiB.
   - PS2 MC format without ECC: page 512, 2 pages/cluster, 16 pages/block, card_type 2, flags 0x2B.
   - Must be contiguous.
   - Reference it with `$VMC_0`/`$VMC_1`.
8. Apps: `APPS/<folder>/title.cfg` with `title=` and `boot=`. PS1 via a POPStarter `XX.<VCD>.ELF` app. Art keyed by the ELF file name.

---

## Sources

OPL repository (`master` unless noted). Raw files at `https://raw.githubusercontent.com/ps2homebrew/Open-PS2-Loader/master/<path>`:
- `include/supportbase.h` — `base_game_info_t`, `USBExtreme_game_entry_t`, name limits
- `src/supportbase.c` — `isValidIsoName`, `GetStartupExecName`, `scanForISO`, `loadISOGameListCache`, `updateISOGameList`, `queryISOGameListCache`, `sbReadList`, `sbCreatePath_name`, `sbCreateFolders`, `sbRebuildULCfg`, `sbRename`, `sbPopulateConfig`, `sbPrepare`, `ProbeZISO`, `sbGetISO9660MaxLBA`, `sbProbeISO9660`, `sbGetMediaLsnCount`, `sbLoadCheats`
- `src/system.c` — `USBA_crc32`, `sysCheckVMC`, `sysLaunchLoaderElf`
- `include/iosupport.h` — IO modes, `COMPAT_MODE_*`
- `include/config.h`, `src/config.c` — keys, file names, parser, writer, `configGetVMC`, `configGetDiscIDBinary`
- `src/guigame.c`, `include/guigame.h` — per-game settings UI, DMA, GSM modes, PADEMU, VMC naming
- `src/ps2cnf.c` — SYSTEM.CNF parser
- `src/bdmsupport.c`, `include/bdmsupport.h` — BDM drivers, prefixes, art path, fragments, VMC checks, `bdmFindPartition`
- `src/ethsupport.c` — SMB prefix, filename, VMC path
- `src/hddsupport.c`, `src/hdd.c`, `include/hdd.h`, `include/hddsupport.h` — APA/HDL, `conf_hdd.cfg`, HDD cache
- `src/opl.c`, `include/opl.h` — config location, `oplScanApps`, `oplGetAppImage`, legacy apps
- `src/appsupport.c`, `include/appsupport.h` — `title.cfg`, app art value
- `src/themes.c`, `misc/conf_theme_OPL.cfg` — theme elements, attributes, art patterns
- `src/textures.c` (and `v1.1.0/src/textures.c`) — image formats and limits
- `src/cheatman.c`, `include/cheatman.h`, `src/cheatconfig.c` — cheat parser, limits, master code, selections
- `include/mcemu.h`, `modules/vmc/genvmc/genvmc.c`, `modules/vmc/genvmc/genvmc.h` — VMC superblock and generator
- `src/vmc_groups.c` — VMC groups
- `src/lang.c` — LNG naming
- `modules/isofs/zso.h`, `modules/isofs/zso.c`, `src/zso.c`, `pc/ziso.py` — ZSO
- `modules/iopcore/common/cdvd_config.h` — `BDM_MAX_FRAGS`, `ISO_MAX_PARTS`, cdvdman settings
- `modules/iopcore/cdvdman/smb.c`, `modules/iopcore/cdvdman/device-smb.c` — SMB dialect, ul part addressing
- `pc/iso2opl/src/iso2opl.c`, `pc/iso2opl/src/iso2opl.h` — reference ul writer
- `lng_tmpl/_base.yml` — UI strings (mode names)
- `Makefile`, `README.md`, `CHANGELOG`

GitHub API / web:
- https://api.github.com/repos/ps2homebrew/Open-PS2-Loader (license, default branch)
- https://api.github.com/repos/ps2homebrew/Open-PS2-Loader/releases and `/releases/tags/latest`
- https://api.github.com/repos/ps2homebrew/Open-PS2-Loader/tags
- https://api.github.com/repos/ps2homebrew/Open-PS2-Loader/commits?path=src/textures.c (commit c5a12a0)
- https://api.github.com/search/commits?q=repo:ps2homebrew/Open-PS2-Loader+zso and `+exfat`
- https://api.github.com/search/issues?q=repo:ps2homebrew/Open-PS2-Loader+udpbd, `+zso+is:pr`, `+mmce+is:pr`
- https://github.com/ps2homebrew/Open-PS2-Loader/wiki

PS2SDK / FatFs:
- https://raw.githubusercontent.com/ps2dev/ps2sdk/master/iop/fs/bdmfs_fatfs/Makefile
- https://raw.githubusercontent.com/ps2dev/ps2sdk/master/download_dependencies.sh
- https://raw.githubusercontent.com/fjtrujy/FatFs/iop-r0.16/source/include/ffconf.h
- https://raw.githubusercontent.com/ps2dev/ps2sdk/master/common/include/libcdvd-common.h

Community / unverified:
- https://oplmanager.com/site/?docs= (art sizes)
- https://github.com/Veritas83/PS2-OPL-CFG (sample `CFG/SLUS_200.62.cfg`, `CFG/SCUS_974.72.cfg`)
- https://github.com/Tom-Bruise/PS2-OPL-CFG-Database
- https://github.com/Luden02/OrbitPS2-Manager/issues/48 (RiptOPL art sizes, via search snippet)
- https://nathanneurotic.github.io/POPSTARTERINFO/ , `usb-storage.html`, `multi-disc-vmc.html`
- https://pcsx2.net/docs/advanced/writing-patches (pnach syntax)
- https://retrorgb.com/?p=48205 and the ConsoleMods OPL wiki search snippet (MMCE only in unofficial builds; https://github.com/ps2-mmce/Open-PS2-Loader)
