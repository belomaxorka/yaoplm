# 09 - Import, ul writing, BIN/CUE and game operations: prior art

Date: 2026-10-23. Scope: milestone M2 (import to a device, rename, delete, move). Sources: the local clones from [REFERENCES.md](REFERENCES.md), refreshed the same day. Upstream OPL is `ps2homebrew/Open-PS2-Loader@b3a21997` (AFL-3.0: logic only, no code copied). Every rule below is implemented in `crates/yaoplm-core` (`import.rs`, `ops.rs`, `exec.rs`, `source.rs`) or `crates/yaoplm-formats/src/cue.rs` and covered by tests there.

Short names:
- OPL = upstream; NN = RiptOPL `NathanNeurotic/Open-PS2-Loader@40c52ceb`.
- PCT = `brainstream/OPL-PC-Tools@2291b5a` (GPL-3).
- HD = `ps2homebrew/hdl-dump@32c296c` (GPL-2+).
- Tbx = `lucasonline0/opl-linux-toolbox@3a776d1` (GPL-3.0-only).
- Orbit = `Luden02/OrbitPS2-Manager@cdd590a` (GPL-3).
- Others as named; licences in REFERENCES.md.

## 1. Writing `ul` games

### 1.1 What the managers do
| Topic | Finding | Source |
|---|---|---|
| Chunk size | 1 GiB everywhere | iso2opl `iso2opl.c`, PCT, Tihwin, POUM, PyOPLM |
| Part number | OPL reads `%02x`; iso2opl, PCT, Tihwin, POUM write decimal, PyOPLM and PS2IsoManager uppercase hex, TinyPS2Manager `.0%d` (part 10 becomes `.010`) | OPL `supportbase.c:726`; iso2opl `:218-222`; TinyPS2Manager `split.go:82` |
| CRC padding | PyOPLM writes the CRC unpadded (`common.py:49`, `ul.py:193,211`) | |
| `ul.cfg` update | iso2opl and PCT append; OPL, POUM, PS2IsoManager, PyOPLM, TinyPS2Manager rewrite in place; only Tbx writes temp + fsync + rename and keeps raw records and the trailing fragment (`ul.service.ts:176-182`) | |
| Bytes 0x31-0x3F | OPL's `sbRebuildULCfg` zeroes them (one template record, 0x08 at 0x35); only Informaldehyde and Tbx keep them | `supportbase.c:689-720` |
| Order | Everybody writes the chunks first, then the record | iso2opl `:564→585`, POUM `conversion.rs:51-61` |
| Failure | No cleanup in iso2opl, POUM, Tihwin; PS2IsoManager leaves preallocated full-size garbage chunks on cancel; PCT rolls back (`UlConfigGameInstaller.cpp:68-117`); nobody journals | |
| Duplicates | iso2opl refuses the same name or ID; POUM checks names only after it has overwritten the existing chunks; PS2IsoManager deletes every record with the ID | `iso2opl.c:117-127`, POUM `games.rs`, `UlCfgService.cs:79` |
| Media byte | Not cosmetic: OPL passes it as the disc type and throttles reads by it (`cdvdman.c:300,507`). Tools pick it by size (PCT > 681,984,000 bytes), by hand (iso2opl, Tihwin) or always DVD (TinyPS2Manager) | |

### 1.2 YAOPLM rules
- Names follow OPL's reader: `ul.%08X.%s.%02x`, CRC zero-padded, part number lower-case hex.
- Before any write: printable-ASCII title of 1-32 bytes (hashed exactly as stored), a valid ID, no record with the same (CRC, ID), no existing chunk file with a target name. Over SMB, at most 10 parts.
- Chunks are written as `*.yaoplm-part`, synced and renamed; `ul.cfg` last, atomically. The new record goes before a ragged tail, so every other byte of the file stays.
- A failure before `ul.cfg` is written undoes the chunks (journal, §4).

## 2. BIN/CUE to ISO

### 2.1 Findings
- **User-data offsets agree everywhere:** MODE1/2048 → 0, MODE1/2352 → 16, MODE2/2352 → 24 (Form 1), MODE2/2336 → 8 (HD `iin_cdrwin.c:46-47`, rcheevos `cdreader.c:171-194` in NathanNeurotic/PS2-Servers).
- **Bugs elsewhere:** Orbit reads MODE2/2336 with a 2352 stride; PCT assumes 2352/24 for every BIN and never reads the sheet (`BinCueDeviceSource.cpp:23-26`), so MODE1 dumps fail.
- **Validation:** only ps2-opl-ripper checks every sector (sync, mode byte, subheader copies) and refuses Mode 2 Form 2 (`cd_sector.py:17-38`). Form 2 data cannot become ISO data: OPL stores 2048-byte sectors only.
- **Multi-track:** HD refuses it (`RET_MULTITRACK`), but its check is "line 5 is an INDEX line", so a single-file sheet with `TRACK 02 AUDIO` followed by `INDEX 00` gets through. PyOPLM keeps track 01 silently; Orbit merges everything, audio included. OPL emulates a disc with exactly one track (`ncmd.c:131,148`).
- **CUE syntax:** HD needs `FILE` on line 1 and fails on a BOM or a leading `REM`; bchunk matches keywords case-sensitively; Orbit needs quoted names.
- **Lone BIN:** rcheevos finds the PVD at sector 16 with a 2352 stride and picks offset 24 or 16 from where `CD001` is.

### 2.2 YAOPLM rules
- The CUE parser skips a BOM, takes CR, LF or CRLF, matches keywords case-insensitively, accepts quoted and unquoted `FILE` names and ignores metadata lines.
- `FILE` is resolved next to the sheet, then by base name in the same folder; never anywhere else.
- **Refused:** more than one `FILE` or `TRACK`, a type other than `BINARY`, `AUDIO`/`CDG` modes, a second `REM SESSION`, a missing BIN, a BIN that is not whole sectors, and any sector with a bad sync, a wrong mode or Form 2. Empty Mode 0 sectors read as zeros.
- The track starts at `INDEX 01`. A BIN that a sheet names is never imported on its own, even when the sheet is refused.
- **A lone BIN** is accepted when sector 16 holds a PVD at one of the four layouts; the plan says which layout was detected.

## 3. CD or DVD (IMP-3)

### 3.1 Findings
- **Tools:**
  - HD reads `CD-XA001` at byte 1024 of the PVD → CD, eight zero bytes → DVD (`isofs.c:274-289`).
  - TinyPS2Manager looks for UDF `NSR02`/`NSR03`.
  - Everyone else uses a size threshold of about 700 MiB.
  - PCT removed its CD/DVD selector in `4dc7f70`.
- **What the media byte changes in OPL** (corrects research/02 §4.4):
  - it is returned by `sceCdGetDiskType`;
  - the faked TOC differs: a CD TOC, or a DVD physical-format block (`ncmd.c:104-185`);
  - Mode 1 read throttling is 900 KB/s for CD and 2200 KB/s for DVD (`cdvdman.c:301`, `bd858ed`);
  - layer-1 probing happens only for DVD.
- Whether a DVD game in `CD/` breaks is untested (console test).

#### 3.1.1 Second look (2026-10-27, for `m5/walkthrough-core`; clones at the commits in REFERENCES.md)
Read for walkthrough findings 1 and 6 (a small DVD game lands in `CD/`). What each tool does, with the file and line:

| Tool | Rule | Source |
|---|---|---|
| OPL (BDM, USB, SMB) | Only the folder: `CD/` → `SCECdPS2CD`, `DVD/` → `SCECdPS2DVD`; the image is opened for `SYSTEM.CNF` alone. `ul.cfg`'s byte is copied as is; an unknown value counts as DVD everywhere. HDD takes the APA header's `discType`. | `src/supportbase.c:286-353,426`, `modules/iopcore/cdvdman/device-smb.c:84-86`, `src/hdd.c:163` |
| OPL, what it reads | Only the layer-1 PVD, and only for a DVD; `mediaLsnCount` comes from the file size because, the comment says, badly mastered discs understate the PVD's volume size. | `supportbase.c:497-569`, `bdmsupport.c:471-498` |
| OPL `pc/iso2opl` | The user passes `CD` or `DVD` (0x12/0x14). | `pc/iso2opl/src/iso2opl.c:163-166,519` |
| hdl-dump CLI | `inject_cd`/`inject_dvd`: the user. `install`: `CD-XA001` at byte 1024 of sector 16 → CD, eight zero bytes → DVD, else unknown and > 750 MiB → DVD ("TODO: the following assumption might be incorrect"). Raw 2352/2336-byte sectors are un-rawed first and never used as a signal. | `isofs.c:274-289`, `hdl_dump.c:1182-1193`, `iin_cdrwin.c:152-165` |
| hdl-dump GUI | The same XA probe, then > 800 MiB → DVD, then the user's combobox. | `gui/gui_main.c:557,599-602,772,799` |
| OPL PC Tools | > 681,984,000 bytes (333,000 sectors) → DVD; BIN/CUE normalised to 2048-byte sectors; no choice in the UI. | `GameInstaller.cpp:56-59`, `BinCueDeviceSource.cpp:52` |
| TinyPS2Manager | Per-hash user override → UDF (`NSR02`, but compared at byte 0 instead of 1, so it never matches a real volume) → PVD volume size ≤ 700 MiB → file size ≤ 700 MiB; a titles database that ships empty. `ul.cfg` always 0x14. | `internal/library/detect.go:88-143`, `isotool/info.go:59-117` |
| OPLNova, PyOPLM, donydaily, Zsubzeroz, PS2IsoManager, dheison0, gpulch | A size threshold only: 1,000,000,000 bytes, 700 MiB, 700 MiB, 700 MiB, 700 MiB, 700,000,000 bytes, 800 MiB. | `Core/Game.cs:49-56`, `opl/games_manager.py:124`, `iso.py:18`, `core/splitter.py:100`, `Iso9660Reader.cs:94`, `manager/game.go:18-24`, `src-tauri/src/transfer.rs:4-7` |
| Tihwin, opl-linux-toolbox, OrbitPS2 | The user: radio buttons, or the import mode (`ps2-cd` takes CUE, `ps2-dvd` takes ISO). | `MainAppUi.java:119-131`, `import.component.ts:30-43`, `library.service.ts:427-432` |
| libopl, logi-26, opl-manager-next | Always DVD (`#FIXME: static media type`); logi-26 even drops HDD CD games from its list. | `libopl/opl.py:162`, `fx/AddGameSmbController.java:121-135`, `PS2Manager.jsx:19` |
| ps2-opl-ripper, ps-disc-detect | A physical drive's profile (MMC 0x08-0x0A, Sony's 0xFF60 PS2 CD / 0xFF61 PS2 DVD); nothing from the image. | `optical/media_detector.py:15-30`, `include/disc_type.h:18-21` |

In short: no tool asks a database (TinyPS2Manager's is empty), the thresholds range from 650 to 954 MiB, three managers always say DVD, and upstream OPL warns that the PVD's volume size can be too small. Nothing in the image tells a small DVD game from a CD except the XA mark of a CD.

### 3.2 YAOPLM order
1. User override.
2. A raw BIN dump → CD.
3. Two layers → DVD.
4. UDF descriptor in sectors 16-31 → DVD.
5. `CD-XA001` → CD.
6. The title database (Redump dumps a CD as BIN/CUE and a DVD as one ISO; `data/titles.json.gz` keeps the disc of every serial) — since 2026-10-27, walkthrough finding 1.
7. A game with the same ID already in `CD/`, `DVD/` or `ul.cfg` — walkthrough finding 6.
8. Volume over 360,000 sectors → DVD, else CD.

Conflicting hints (XA but bigger than a CD) give a warning; the plan names the step that decided.

## 4. Rename, delete, move

### 4.1 Findings
- **OPL `sbRename`** renames the image (title part only; the ID and extension stay) or the chunks and the `ul.cfg` record. It ignores `rename()` errors (`supportbase.c:758-775`); a failure midway leaves the parts split between two CRCs.
- **The UI skips unchanged names:** renaming a file to itself "can cause the file to be deleted on certain file systems" (`menusys.c:104-108`, `a21073f`).
- **No fork and no manager moves CFG, ART, CHT or VMC on a rename.** Those are keyed by the game ID, not the title. The ID only changes when the naming style flips or a wrong ID prefix is fixed (PCT does this implicitly, `DirectoryGameStorage.cpp:60-100`).
- **OPL `sbDelete`** removes the image or chunks first and rebuilds `ul.cfg` after, so a crash in between leaves a record that points to missing files. It never deletes CFG, ART, CHT or VMC.
- **VMC cards are shared by design:** the default name is `<group ID or ID>_<slot>` (`guigame.c:212-218`, `vmc_groups.c`), and `generic_0`/`generic_1` exist.
- **Managers that delete related files:**
  - PCT deletes `ART/<ID>*`, CFG and CHT only when no other game has the ID (`GameListWidget.cpp:473-504`);
  - PyOPLM deletes `ART|CFG|CHT|VMC/<ID>*` with no confirmation;
  - OPLNova asks nothing.
- **`games.bin`:** no USB/SMB manager deletes it. NN keeps one per browsed subfolder.
- **Overwrites:** Node's `fs.rename` and Rust's `std::fs::rename` both replace an existing target. Case-only renames on FAT need two steps (Orbit `renameCaseAware`).
- **Backups:** no manager backs up CFG files. NN snapshots a CFG and restores it if the save fails (`config.c:1467-1490`).

### 4.2 YAOPLM rules (ADR-0020)
- **A new title** renames only the image, or the chunks and the record.
- **A new effective ID** (a corrected ID, or a style switch that changes it) also moves `CFG/<ID>.cfg`, wOPL's `.info`, `ART/<ID>_*` and `CHT/<ID>.cht`. When another game still uses the old ID they are copied instead. An existing target stops the plan.
- **VMC files and `$VMC_*` values never change.**
- Every rename checks the target case-insensitively, does case-only changes in two steps, and refuses a rename that changes nothing.
- **Delete:**
  - an image, or the `ul.cfg` record first (atomic), then the chunks;
  - CFG, art and cheats only on request and only when no other game uses the ID;
  - VMC never;
  - the CFG is backed up first.
- **Move CD↔DVD:** a rename between the folders (`games.bin` goes in both), or the media byte of the `ul` record.
- **Write order:** chunk renames first, then `ul.cfg` atomically, all journalled and undone on failure.

## 5. Write safety

### 5.1 Findings
- The safest model found is Tbx:
  - copy to `.part`;
  - check SHA-256;
  - fsync the directory;
  - refuse an existing target (`safe-copy.service.ts:36-87`);
  - snapshot and roll back `ul` changes (`ul.service.ts:197-215`).
- Nobody journals across a crash.
- PCT once wrote its `ul.cfg` temp file to the system folder, which then could not be renamed onto the USB stick. The fix was to put the temp file next to `ul.cfg` (`35e77da`).

### 5.2 YAOPLM design
- Temp files sit next to their target.
- A journal on the device marks transactions with a commit point: before it a crash is undone, after it finished (ADR-0021, `crates/yaoplm-core/src/exec.rs`).

### 5.3 Renames and removals on Windows FAT (2026-10-26)
Source: Microsoft's FAT driver sample, `microsoft/Windows-driver-samples` at `bb68ed5e77`, `filesys/fastfat/`. exFAT's `exfat.sys` is closed but grew from the same code.
- **Flushing a folder does nothing** on FAT. Only the root folder is special: flushing it flushes the whole volume (`flush.c`, `FatCommonFlushBuffers`, case `UserDirectoryOpen`: "Flush a directory does nothing", the root DCB falls through to `UserVolumeOpen`). A directory fsync, as on Linux, has no Windows FAT equivalent other than the volume root.
- **Metadata goes to the stick at once only on hot-pluggable devices with write caching off.** `strucsup.c` sets `VCB_STATE_FLAG_DEFERRED_FLUSH` for hot-pluggable media unless `WriteCacheEnableOverride` is set, which is what the "Better performance" removal policy does. With the flag, `FatUnpinRepinnedBcbs` (`cachesup.c`) writes the dirents of a rename through. Without it, a rename or a delete sits in the cache until a flush or the lazy writer.
- **A rename over an existing file is not atomic on FAT.** The old entry is removed, then the new one is renamed in. A stick pulled in between loses both names but keeps the new file's data under its temporary name.
- **What YAOPLM does** (`yaoplm-device` `sys::windows::sync_dir`):
  - after every rename and removal of a plan, it flushes the folder and then the root of its volume;
  - a folder that cannot be opened for writing or flushed (the root of a system disk for a standard user, some network redirectors) is left as it is.
- **Journal temp file:** `journal::load_all` takes a `<id>.json.yaoplm-tmp` without its `.json` as the journal, and `journal::clean_temp` renames it back instead of deleting it.

## 6. Still open

- **Real dumps:** single-track and multi-track PS2 CD BIN/CUE files are needed to confirm the Redump layout (MODE2/2352 is assumed).
- **Console:**
  - does a small DVD game work from `CD/`, and a CD game from `DVD/`?
  - does Mode 1 behave differently there?
- **Fragmentation:** the 64-fragment total per game is checked after every write and in the library (IMP-7b, ADR-0037; research/06 §1.11).
