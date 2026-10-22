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

### 3.2 YAOPLM order
1. User override.
2. A raw BIN dump → CD.
3. Two layers → DVD.
4. UDF descriptor in sectors 16-31 → DVD.
5. `CD-XA001` → CD.
6. Volume over 360,000 sectors → DVD, else CD.

Conflicting hints (XA but bigger than a CD) give a warning.

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

## 6. Still open

- **Real dumps:** single-track and multi-track PS2 CD BIN/CUE files are needed to confirm the Redump layout (MODE2/2352 is assumed).
- **Console:**
  - does a small DVD game work from `CD/`, and a CD game from `DVD/`?
  - does Mode 1 behave differently there?
- **Fragmentation:** the 64-fragment total per game is not checked yet (IMP-7b, v0.2).
