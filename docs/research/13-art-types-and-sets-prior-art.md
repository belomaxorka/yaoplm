# 13 - Picture types, picture sets and checks on add: prior art

Date: 2026-10-23. Scope: milestone M5, PR 2a (ART-2, ART-3, ART-5, ART-8, GAME-5 on add, META-1 titles on add). Sources: the local clones from [REFERENCES.md](REFERENCES.md), read at the HEADs below before each piece was built (ADR-0015). Nothing was copied: the format rules come from OPL itself (research/02 §7); the other tools gave edge cases and UI ideas. CFG and VMC prior art (PR 2b) goes into a separate note.

| Short name | Repository | HEAD |
|---|---|---|
| OPL | `ps2homebrew/Open-PS2-Loader` | `b3a2199` |
| RiptOPL | `NathanNeurotic/Open-PS2-Loader` | `57c45b4f` |
| wOPL | wOPL (mystyq) | `4ae7414` |
| Orbit | `Luden02/OrbitPS2-Manager` | `cdd590a` |
| PCT | `brainstream/OPL-PC-Tools` | `2291b5a` |
| PyOPLM | `edisnord/PyOPLM` | `a8e3ceb` |
| HDL | `israpps/HDL-Batch-installer` | `a724a26` |
| Tbx | `lucasonline0/opl-linux-toolbox` | `3a776d1` |
| vapi | `WeeXnes/oplnova_vapi` | `4806a79` |

## 1. Picture types in OPL and in the managers

- Upstream OPL builds the path `<prefix>ART/<ID>_<SUFFIX>` (`src/bdmsupport.c` `bdmGetImage`) and appends `.png` only (`src/textures.c` `texDiscoverLoad`, since `c5a12a0`, 2024-10-22; before that png, jpg, bmp in that order). Over 1024 px on a side is refused (`texSizeValidate`).
- The default theme asks for `ICO` (item icon), `COV` (item cover), `BG` (info page background), `SCR` and `SCR2` (info page). `COV2`, `LAB` and `LGO` have no literal in upstream code or the default theme: they show only in custom themes that set `pattern=` to them (`src/themes.c`, `misc/conf_theme_OPL.cfg`).
- **The default theme draws `ItemIcon` at 128×128** (`main4`, changed in `05caa00`, 2024-10-17). Every manager writes a 64×64 ICO (PCT `GameArt.cpp`, PyOPLM `PS2_ART_SIZES`), so stock OPL upscales it twice. YAOPLM keeps 64×64 for the OPL profiles and 128×128 for RiptOPL, like the rest of the ecosystem; whether 128×128 looks better on a real console is an open check (§6).
- Sizes per type are the same in PCT (`GameArt.cpp` `makeGameArtProperies`) and PyOPLM: COV 140×200, COV2 242×344, LAB 18×240, ICO 64×64, SCR and SCR2 250×188, BG 640×480, LGO 300×125. Orbit (`png-artwork.ts` `EXPECTED_DIMENSIONS`) doubles COV and ICO, the RiptOPL sizes (research/05 §3.4). YAOPLM takes them from `yaoplm_formats::art::recommended_size`.
- Every manager stretches to the exact size (PCT `Qt::IgnoreAspectRatio`, PyOPLM `image.resize`), because OPL stretches the texture into the theme element anyway. **A logo is the exception in YAOPLM**: libretro's `Named_Logos` are wide transparent PNGs of any ratio, and stretching one to 300×125 distorts the lettering, so LGO is fitted inside the box on a transparent background (`transcode::Fit::Contain`).

## 2. Open sources per type

| Type | Source | Path |
|---|---|---|
| COV | xlenore/ps2-covers | `covers/default/<SERIAL>.jpg`, and 3-D renders `covers/3d/<SERIAL>.png` as a second variant |
| COV | libretro-thumbnails | `Named_Boxarts/<Redump name>.png` |
| SCR | libretro-thumbnails | `Named_Snaps/` |
| SCR2 | libretro-thumbnails | `Named_Titles/` (the title screen) |
| LGO | libretro-thumbnails | `Named_Logos/` |
| ICO, COV2, LAB, BG | none open | picture sets the user has (§3), or by hand |

- Orbit's 3-D covers are a `COV3` that no loader reads (Orbit `artwork-presets.ts`, Tbx's RiptOPL preset asks for it). YAOPLM offers xlenore's 3-D render as a **variant of COV**, picked by hand on the game card; the bulk run takes the flat cover.
- The cache key of a libretro picture carries its folder (`Named_Snaps/<name>`), so a snap and a box art of one game do not collide; `Named_Boxarts` keeps the bare key of M4 so existing caches stay valid. Derived files are labelled `<TYPE>-<W>x<H>.png`.
- **Variants.** Orbit's wizard shows remote thumbnails to pick from (`artwork-wizard-dialog`, screenshots capped at two, "OPL only shows two screenshots"). YAOPLM's `FindArt` job asks every source for one type of one game and returns the resized candidates; picking one is an ordinary `SetArt` plan. Orbit walks up to ~32 URLs per slot (`SCR_00`..`SCR_30`); YAOPLM asks each source for its known names only.
- **Bulk types.** Orbit preselects nothing but counts with its default types (`artwork-bulk-dialog`, the counts do not match what Start does). PCT always takes all eight types. YAOPLM's bulk dialog offers only the types some source has, preselects COV, and counts exactly what Start will look for.

## 3. Picture sets (ART-5)

### OPL Manager backups

No backup was available locally; the layout comes from four independent readers and matches all of them:

- Items `OPLM_ART_YYYY_MM` on archive.org (2018-11 to 2024-09), one zip of about 6 GB each (research/03 §2.1).
- Zip root `PS1/` and `PS2/`, then `<ID>/` in OPL form (`SLUS_200.62`), then `<ID>_<TYPE>.<ext>` (PyOPLM `storage.py` `get_relative_source_path`; HDL's URL `PS2%2F<ID>%2F<ID>_COV.jpg`; vapi `setup_db.sh`).
- **Screenshots and backgrounds are numbered**: `<ID>_SCR_00`, `_SCR_01`, …, `<ID>_BG_00`, …; `SCR_00` is SCR, `SCR_01` is SCR2, `BG_00` is BG, the rest have no place in OPL (PyOPLM `index_artwork`; HDL `ArtMan.cpp`; Orbit `artwork-filenames.ts` `INDEXED_REMOTE_TYPES`).
- Extensions are mixed: in 2023_07, COV, COV2, LAB, BG and SCR are `.jpg`, ICO and LGO `.png` (HDL `ArtMan.cpp`). A shell script (astaroth666) expects `_SCR.png` instead; YAOPLM accepts both forms, any of png, jpg, jpeg, bmp, gif, webp, in any case.
- Title lists `PS1_LIST.CSV` and `PS2_LIST.CSV` sit at the root; YAOPLM does not need them (titles come from the bundled database, ADR-0024).

Tools that took these files into `ART/` as they are wrote names OPL 1.2 never reads: `.jpg` (PyOPLM, libopl, logi-26, PyOPL-Utility, PS2IsoManager) and `_SCR_01`, `_BG_00` (Tbx). PyOPLM issue #7 is the other trap: a prefix glob `<ID>_COV*` also matches `COV2`. YAOPLM parses each name exactly (`yaoplm_formats::art::pack_picture`) and transcodes every picture to PNG at the profile's size.

### `art.tar`

- RiptOPL (`include/tar.h`, `src/tar.c`, ported from wOPL): plain ustar, 512-byte blocks, members up to 4 MiB, names up to 47 characters (wOPL: 40); zero blocks are skipped rather than ending the scan, so concatenated archives work; only regular files are indexed; the name is ustar `prefix` + `/` + `name` with a leading `./` and `ART/` dropped; matching is case-insensitive (`tarFind`). The archive sits at `ART/art.tar` from the device root; RiptOPL writes `art_cache.bin` next to it.
- wOPL stops at the first zero block and clamps an oversized member before padding, which desyncs its index.
- RiptOPL now also reads `CHT/cht.tar` first (`src/supportbase.c`), which corrects research/07 ("only ART is wired").
- YAOPLM reads a tar as a picture set (it does not write one yet: ART-6, v0.3). The reader is lenient like RiptOPL's: zero blocks are skipped, GNU long names and pax `path` records are honoured, base-256 sizes are read, a bad header checksum is an error with its offset, and a truncated member is reported. It is fuzzed (`fuzz/fuzz_targets/tar.rs`).

### Other packs

- PCT reads covers from a community 7z on archive.org through `view_archive.php`, which makes archive.org decompress the 7z for every cover (`GameArtNetworkSource.cpp`). YAOPLM never does that; the same 7z is a set the user downloads and imports.
- Orbit and Tbx read a GitHub dump of OPL Manager's database (all 8-bit palette PNGs) through the GitHub API. Not used (research/11 §2).

### Decisions

- **Only the games on the device** (the owner's decision, 2026-10-23). A backup holds tens of thousands of games; importing all of them would fill a stick with pictures of games it does not have. The rest are counted in the plan ("left out, for games not on this device: N").
- **One pass over the set.** `archive::for_each_file` walks a zip or a 7z front to back; a solid 7z block is decoded once, not once per picture. A folder is walked to depth 8, skipping dotfiles and opening only names that parse as pictures.
- **When a set has several files for one slot**, png wins over bmp, bmp over jpg, jpg over the rest: lossless first, and what OPL 1.2 loads first.
- **Pictures the device has stay** unless the user asks to replace them; a replaced picture moves to `.yaoplm/backup/` like any other (ADR-0025).
- OPL Manager's private art server is never asked (rule 3, ADR-0007). The files come from the user.

## 4. Covers while games are added (ART-8)

No manager does this in one confirmed plan. Orbit downloads art after the copy finishes as a separate step; PCT and PyOPLM have separate commands. YAOPLM searches while the import plan is shown, then re-plans the import with the covers: one confirmation, and image and cover land in one job (SPEC §5.4.1).

## 5. Dump checks while games are added (GAME-5)

- No manager hashes while it copies. Orbit and OPL Manager verify only by a separate pass over the stick, which is the slowest part of the device.
- Redump hashes CDs by their raw BIN tracks (2352-byte sectors); a 2048-byte ISO of a CD never matches (research/12 §1, ADR-0026). So the copy hashes both: the ISO as OPL reads it, and, for a CD dump from BIN/CUE or CHD, the raw data track sector by sector as it is decoded. Audio tracks are shared across discs in the DAT, so only a data track is matched by its raw hash.
- A ZSO copied as it is is decoded on the fly to hash its ISO, so a ZSO game gets the same verdict as the ISO it came from.

## 6. Still open

- **ICO at 128×128 on stock OPL**: check on the console whether a 128×128 icon looks sharper than the upscaled 64×64 one, and whether OPL 1.1 handles it (it has the old 64×64 theme).
- **Mode 2 Form 2 sectors in a real CD's data track** (postgap): the raw-track hash takes every sector as it is, but the ISO conversion refuses Form 2 user data. A real Redump-format CD dump is needed to confirm the "verified" path end to end.
- A real OPL Manager backup to confirm the numbered names and the extensions per type.
