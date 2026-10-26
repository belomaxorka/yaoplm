# 12 - Dump checks, conversions and packed sources: prior art

Date: 2026-10-23. Scope: milestone M5, first PR (GAME-5 and META-2 dump checks against Redump, GAME-4 conversions between ISO, ZSO and `ul`, IMP-1b import from CHD, zip, 7z and multi-track BIN/CUE). Sources: the local clones from [REFERENCES.md](REFERENCES.md), refreshed the same day, read before each piece was built (ADR-0015). Nothing was copied; format rules come from OPL itself (research/02).

Short names:
- OPL = `ps2homebrew/Open-PS2-Loader@b3a21997` (`src/supportbase.c`, `include/supportbase.h`, `modules/iopcore/cdvdman/device-smb.c`).
- Orbit = `Luden02/OrbitPS2-Manager@cdd590a` (`src/services/zso.service.ts`, `src/workers/zso.worker.ts`, `src/utils/zip-extract.ts`).
- PDD = `Al-Azif/ps-disc-detect@0869fd1` (`tools/build_redump_db.py`, `include/redump.h`, `src/redump.c`).
- HBI = `israpps/HDL-Batch-installer@a724a26` (`HDL-Batch-installer-SRC/MD5Man.cpp`, `MD5Report.cpp`).
- chd-rs = crate `chd` 0.4.0 (BSD-3-Clause), a Rust port of MAME's `libchdr`.

## 1. Redump DAT

| Finding | Source | In YAOPLM |
|---|---|---|
| `https://redump.info/datfile/ps2/serial,version` serves a zip with one Logiqx `.dat` that carries `<serial>`; the serial-only URLs are deprecated | PDD `build_redump_db.py` (`DAT_URLS`), checked by hand 2026-10-23 | `yaoplm-net::redump::URL` |
| Redump answers 403 to a request without a `User-Agent` | PDD `build_redump_db.py` comment | Every request sends `YAOPLM/<version> (+repo URL)` (rule 3) |
| No `ETag`, no `Last-Modified` on the DAT (headers saved in `$YAOPLM_TESTDATA\dat\redump-headers.txt`) | checked 2026-10-23 | Our own rule: at most one request a week, counted for every answer including errors (`db/redump-ps2.asked`); import of a downloaded `.dat`/`.zip` works offline |
| Serials are comma lists with suffixes: `SLES-52046, SLES-52046/P/GER`, `SCES-53358#`, `SLES-53994-ANZ`, `SLES-53906/1.00`. The PS2 DAT of 2026-10-23 (11 845 games, 11 375 serials) has no ranges; three serials have no ID shape (`5074607`, `5074567`, `GLD-DR01`) | the DAT; PDD splits on `[,;&/]` and expands ranges like `SLUS-20012-20014` (needed for its PSX/PS3 DATs) | `yaoplm-hash::redump::serial_keys`: split on commas, key = letters plus five digits; ranges not expanded (none in PS2) |
| A verdict needs the image hashed as Redump hashed it. CDs are hashed per raw BIN track (2352-byte sectors), so the ISO of a CD never matches | PDD `redump_verify()` compares track checksums | ISOs of CDs get "unknown: CD image" with the entries of the same ID and a same-sector-count hint, not "bad dump" |
| HBI computes an MD5 and offers "Search on redump.org" for it; no local DAT, no verdict | HBI `MD5Report.cpp` | Verdicts against a local DAT, a link to the matching disc page (`redump.info/disc/<id>`) |

## 2. ZSO

| Finding | Source | In YAOPLM |
|---|---|---|
| Index shift chosen from the worst case (every block stored), so offsets of incompressible images fit in 31 bits | Orbit `chooseAlign(worstCaseEnd)` | `zso::align_for`, `zso::max_output` (also the space a conversion reserves) |
| Header and index written after the data; the ISO is unlinked only after the output is closed | Orbit `zso.service.ts` (end of `compressIso`) | Same order, plus a journal: written to `.yaoplm-part`, synced, renamed, the ISO removed after the commit point; a crash or cancel removes the part |
| Compression runs on a worker pool in chunks of 4096 blocks, written in order | Orbit `PARALLEL_CHUNK_BLOCKS`, `WorkerPool` | Not taken yet: our compressor is single-threaded. Worth doing when users report slow bulk compression |

## 3. `ul.cfg` and chunks

| Finding | Source | In YAOPLM |
|---|---|---|
| Chunk names depend only on `USBA_crc32(name)`, the ID and the part: two records with the same name and ID share their files | OPL `sbCreatePath_name()` | Converting a `ul` game removes every record of it, with a notice |
| Deleting a game removes its chunks and rewrites `ul.cfg` without that one record (`sbRebuildULCfg(.., excludeID)`): a duplicate record stays, listing chunks that are gone | OPL `sbDelete()` | Deliberate deviation (above): no record is left pointing at deleted chunks |
| The in-game driver reads part *n* at *n* × 0x80000 sectors (1 GiB): only the last part may be shorter | OPL `device-smb.c` (bounds of 0x80000 sectors) | Joining chunks refuses other sizes (`ulPartSize`) |

## 4. Packed sources

| Finding | Source | In YAOPLM |
|---|---|---|
| Zip archives of discs are unpacked to a temporary folder first; `__MACOSX` folders are skipped | Orbit `zip-extract.ts` (`extractDiscZip`, `findFirstByExt`) | Members are streamed from the archive while copying, no temporary copy; `._*` and other hidden files are skipped by the import walk |
| 7z entries in a solid block are decoded in order; a member late in a block costs decoding everything before it | sevenz-rust2 `BlockDecoder` | Small members (CUE sheets) are read once and kept; images are read front to back once |
| Entry names may carry drive letters and `..` (7-Zip `-spf`, hand-made zips) | 7-Zip's own extraction drops them | `archive::safe_name` drops drives, `.` and `..`, so no member leads out of the archive |
| CHD: DVDs in 2048-byte units; CDs in 2448-byte frames with `CHT2` track metadata, tracks padded to 4 frames, a pregap stored in front when `PGTYPE` starts with `V`; a child CHD opens without its parent and fails only when a hunk refers to it | chd-rs `chdfile.rs`, `metadata`, `cdrom.rs`; MAME `chdcd.cpp` | `yaoplm-formats::chd`; a CHD that needs a parent is refused at open; a CHD inside an archive is refused (its map sits at the end and hunks point back) |
| Redump keeps every track of a disc in its own `(Track n).bin`; some tools write one BIN with every track and `INDEX 00` pregaps | the DAT (`rom` names), cue sheets in the corpus | Track 1 is the data track; audio tracks are dropped with a notice (OPL plays one-track discs); in one BIN the data ends where the next track's `INDEX 00` (else `INDEX 01`) starts |

## 5. Not taken

- PDD's SQLite database built from several DATs: one PS2 DAT is parsed in memory in under half a second even in a debug build (0.47 s on 2026-10-23) and needs no build step.
- Unpacking archives to a temporary folder (Orbit): doubles the disk space needed and the time on a USB 2 stick.
- RAR: the only decoder (`unrar`) has a licence that does not fit GPL-3; it stays behind the `rar` feature, not built by default (ARCHITECTURE.md §2).
