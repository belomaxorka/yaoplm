# 08 - Format edge cases found while implementing M1

Date: 2026-10-22. Scope: milestone M1 (OPL formats). Sources: local clones from [REFERENCES.md](REFERENCES.md), refreshed the same day. Upstream is `ps2homebrew/Open-PS2-Loader@b3a21997` unless noted. This report adds to and corrects [02-opl-formats.md](02-opl-formats.md).

Every rule below is covered by a test in `crates/yaoplm-formats` (unit tests, `tests/fixtures.rs`, `tests/properties.rs`) or `crates/yaoplm-core/tests/scan.rs`.

Tags:
- **[V]** read in source.
- **[T]** confirmed by running OPL's own code: `tests/opl_oracles.rs` compiles OPL's C function or runs `pc/ziso.py`.
- **[C]** confirmed on the community corpus (`tests/corpus.rs`).

## 1. Forks agree on the core formats [V]

Compared with upstream (`git diff --no-index`, CR at EOL ignored):

| File | RiptOPL `40c52ceb` | wOPL `4ae74148` | OPL-Evolution `47119b71` | ps2-mmce `e2090114` |
|---|---|---|---|---|
| `src/ps2cnf.c` | identical | identical | identical | identical |
| `modules/isofs/isofs.c` | only `strncpy` of the informational name | identical | identical | identical |
| `modules/isofs/zso.c` | NULL check after a failed allocation | identical | identical | identical |
| `pc/ziso.py` | explicit `<` byte order | identical | identical | identical |
| `isValidIsoName`, `GetStartupExecName` | same logic (adds a length guard) | same | same | same |
| `USBA_crc32` | reads bytes as `unsigned char` | identical | identical | identical |

The real divergences are elsewhere:
- **wOPL** writes per-game CFG in libconfig syntax since 2026-06-20 (`03758fb`) and adds `CFG/<ID>.info`. Its `config_migration.c` converts old `key=value` files only when libconfig fails to parse them.
- **RiptOPL** adds folder browsing inside `CD/` and `DVD/`, PS1 in `POPS/` and `EMBER/`, its own art rules and compatibility bits 0x40/0x80.
- **RiptOPL `USBA_crc32`** is defined for bytes >= 0x80, whereas upstream's result for such bytes is undefined. Both give the same result for ASCII.

## 2. SYSTEM.CNF and the game ID

New findings, all [V], all hide the game:
- **No `:` in the BOOT2 value** (`BOOT2 = SLUS_200.62;1`). `GetStartupExecName` returns -1. research/02 §4.3 described the skip but not this failure.
- **An empty ID** (`cdrom0:\;1` or `cdrom0:\\\;1`).
- **A UTF-8 BOM before `BOOT2` on line 1.** `ps2cnf.c` has no BOM handling, unlike the CFG reader. A BOM followed by a newline is harmless.
- **A NUL byte where a line starts, before the BOOT2 line.** `CNFGetToken` treats NUL as end of file.

Other behaviour:
- **Whitespace skipping crosses newlines**, both before the key and around `=` (`BOOT2\n=\ncdrom0:\X;1` works).
- **`BOOT2` is matched as a prefix.**
  - `BOOT2X = ...` matches the key and then fails on `=`.
  - When `=` is missing, upstream calls `CNFGetKey(NULL, ...)`. The outcome is undefined; YAOPLM reports it as hidden.
- **The value is the first run of printable characters (0x21-0x7E), at most 64.**
  - A space ends it.
  - A control byte or a byte >= 0x80 also ends it: the copy loop spins without advancing.
  - Example: `cdrom0:\AB\x01CD;1` gives `AB`.
- **Exactly 64 printable characters leave the buffer unterminated.**
  - The ID is still defined when it fits inside those 64 bytes.
  - Otherwise YAOPLM reports `Unpredictable`.
- **Only `\` is skipped after the colon:** `cdrom0:/SLUS_200.62;1` gives `/SLUS_200.6`.

## 3. ISO9660 as OPL's isofs reads it [V]

- **Only sector 16 is examined:** type 1 plus `CD001`. There is no descriptor-set scan and no Joliet.
- **The root is searched with `strcmp` against the bytes at offset 33 of each record.**
  - The file must be exactly `SYSTEM.CNF;1`: upper case, with the version suffix.
  - It must be followed by a zero byte (the ISO padding byte).
  - Images mastered without `;1` versions are hidden.
- **A zero record length ends the current sector.** Scanning continues with the next sector of the directory.
- **A record with a name length of 0 hangs OPL:** the scan position never advances.
- **The record length is read as a packed `short`.** A non-zero extended-attribute length byte therefore changes the step.
- **The container is chosen by the `ZISO` magic, not the extension** (`ProbeZISO`). A ZSO renamed to `.iso` still mounts. Its format label follows the extension.

## 4. `ul.cfg` and `USBA_crc32`

- **The leftover value the hash starts with is 0.** For `t = 255` the top bit stays set for all 8 rounds, so the value is only shifted out. [V]
- **Ported and checked against the real function.** [T]
  - The C function is compiled from `src/system.c`; MSVC locally, gcc in CI.
  - Checked on 5,000+ random 7-bit names (0–40 bytes), plus the edge names.
  - Vectors:

    | Name | CRC |
    |---|---|
    | `""` | `00000000` |
    | `"A"` | `935FAD90` |
    | `"Grand Theft Auto III"` | `10D00014` |
    | 32 × `Z` | `C5B91150` |
    | 33+ × `Z` | `92A46C81` (only 33 bytes are hashed) |
    | `"AG"` (leading zero) | `017A9F71` |

  - The only vectors in other repos come from ulmake's test suite (via TinyPS2Manager `split_test.go` and POUM `crc.rs`). All match:

    | Name | CRC |
    |---|---|
    | `f` | `8433E5CC` |
    | `F` | `A490D3EA` |
    | `f` + 31 × `o` | `84BA9D95` |
    | `F` + 31 × `O` | `8CAF0142` |

- **A ragged `ul.cfg`** (size not a multiple of 64):
  - `sbReadList` still loops while `size > 0`.
  - The partial read leaves part of the previous record in the buffer, which becomes one garbage entry.
  - With no ISO games present, it writes one entry past the allocation. [V]
- **Unknown media bytes count as DVD everywhere** (`media == SCECdPS2CD ? "CD" : "DVD"`). [V]
- **Other managers diverge.**
  - Part numbers: decimal in iso2opl, OPL-PC-Tools and POUM; uppercase hex in PyOPLM and PS2IsoManager; `"0%d"` in TinyPS2Manager.
  - Unpadded CRC: TinyPS2Manager, and PyOPLM when writing.
  - zlib CRC-32 instead of `USBA_crc32`: OrbitPS2 (`src/utils/crc32.ts`).
  - YAOPLM follows the reader: `%08X`, `%02x`.

## 5. CFG [V], [C]

- **Correction to research/02 §6.1 ("there is no comment syntax").**
  - `readFileBuffer()` in `src/util.c` skips every line whose first byte is `#`. This is why `#Name=` lines in a file are never loaded.
  - An indented `#key=value` is a real key, but it is never written back.
- **A UTF-8 BOM at the start of the file is skipped.**
- **CR handling:** one CR before LF, or at the end of the file, is removed. A lone CR is part of the line, so an old-Mac file is one line.
- **Prefixes:**
  - A prefix (`name:` without `=`) keeps any leading whitespace.
  - A colon in column 0 does not start a prefix.
- **Duplicate keys** keep the position of the first occurrence and the value of the last.
- **Key and value limits:** a key keeps 31 bytes and a value 255. `atoi` is used for integers.
- **Corpus check:** 40,860 community CFG files (Tom-Bruise CFG_en and CFG_fr, VTSTech) [C]:
  - all round-trip byte for byte;
  - none looks like libconfig;
  - setting `$Compatibility` leaves every other key and its order unchanged.

## 6. CHT [V], [C]

- **A comment line with leading blanks becomes a cheat named by those blanks.**
  - Example: ` //Gameplay 16:9`.
  - `term_str` cuts at `//`. `trim_str` then refuses to trim a string that has no printable characters, so the line stays `" "`, which is non-empty and not a code.
  - The codes that follow belong to that blank-named cheat. In "select cheats" mode the user sees an unnamed entry; auto mode is unaffected.
  - Real files: `SLES_505.41.cht` and `SLES_539.08.cht` in PS2-Widescreen/OPL-Widescreen-Cheats.
- **Lines longer than 255 bytes** are cut at 255. The 256th byte is skipped and the rest is parsed as the next line. Seen in `SLPM_622.58.cht`.
- **`cheat_index < MAX_CODES` allows a 251st cheat** in a 250-slot array (off by one).
- **Code limits:** the engine gets at most 254 address/value pairs in total. A code with address 0 ends that cheat's list.
- **Corpus check:** 3,629 widescreen CHT files parse; 3,623 have a master code; 3 trigger the warnings above.

## 7. ZSO

- **`read_raw_data()` divides by `2048 >> align`**, so `align > 11` divides by zero. [V]
- **`header_size` and `block_size` are ignored by OPL.** The index is always at byte 24 and blocks are 2048 bytes. `ziso.py` rejects `header_size != 24`. [V]
- **Compressed blocks are decoded with `LZ4_decompress_fast(..., 2048)`.**
  - Alignment padding after the data is ignored.
  - Each stored block must fit in 2048 bytes, because a run of blocks is read into the tail of the caller's buffer. [V]
- **`ziso.py` quirks** [V]:
  - The `-a` option is ignored: compression always uses `align = total // 2^31`.
  - For an incompressible image just under 2 GiB, header + index + data can pass 2^31. The script then aborts with "Align error" on a compressed block, or silently corrupts the offset of a plain one.
  - The final index entry is not padded, so with `align > 0` the last block's size can be rounded down.
  - YAOPLM picks the smallest shift that is safe in the worst case, and pads the end.
- **Interop with `ziso.py` works both ways** [T]:
  - YAOPLM reads `ziso.py -c 1` and `-c 9` output.
  - `ziso.py -c 0` restores YAOPLM output with `align` 0 and 2.
- **Compression ratio:** `lz4_flex` has no HC mode. YAOPLM's ZSO files are therefore somewhat larger than `ziso.py -c 9` output; any LZ4 block decoder reads both.

## 8. `games.bin` [V]

- **Media:** a cached entry's media is overwritten by the folder it is in.
- **Stale IDs:** the cached ID is kept, so a replaced image with the same file name keeps the old ID until the cache is rebuilt. YAOPLM reports this as `staleCache`.
- **Upstream bug in `updateISOGameList()`.** It compares `cache->games[i]` inside the `j` loop, so change detection is unreliable. This is one more reason to delete the file in changed folders (IMP-8).

## 9. VMC [V]

- **What `sysCheckVMC()` checks:**
  - magic (27 characters);
  - `mc_type == 2`;
  - file size equal to `pages_per_cluster × clusters_per_card × page_size`;
  - a whole number of MiB.
- **Zero-filled cards fail.** OrbitPS2 writes such a "card" and claims OPL formats it on first boot (`vmc.service.ts`); OPL rejects it.
- **Name buffers are 32 bytes** (`src/guigame.c`), so names are at most 31 characters.

## 10. Which forks fix these bugs (checked 2026-10-22)

✓ = fixed, ✗ = still present. Upstream, wOPL `4ae74148`, OPL-Evolution `47119b71` and ps2-mmce `mmce_support` `e554796` carry all eight.

| Bug | Where (upstream) | RiptOPL C `40c52ceb` | RiptOPL C++ `6edea174` |
|---|---|---|---|
| Indented comment line becomes a blank-named cheat (§6) | `src/cheatman.c` `parse_buf`/`trim_str` | ✗ | ✗ |
| Directory record with a name length of 0 hangs the scan (§3) | `modules/isofs/isofs.c` `cdvdman_locatefile` | ✗ | ✗ |
| ZSO `align > 11` divides by zero (§7) | `read_raw_data` in `modules/isofs/isofs.c`, `src/zso.c`, `modules/iopcore/cdvdman/cdvdman.c` | ✗ | ✗ |
| 251st cheat written past the array (§6) | `src/cheatman.c` (`cheat_index < MAX_CODES`) | ✓ | ✓ |
| `BOOT2` without `=` dereferences NULL (§2) | `src/ps2cnf.c` | ✗ | ✓ |
| Ragged `ul.cfg` writes past the list (§4) | `src/supportbase.c` `sbReadList` (`while (size > 0)`) | ✗ | ✓ (`while (size >= recordSize)`) |
| `updateISOGameList` compares `games[i]` in the `j` loop (§8) | `src/supportbase.c` | ✓ | ✓ |
| Signed `char` in `USBA_crc32` (§4) | `src/system.c` | ✓ | ✓ |

RiptOPL also guards short names in `isValidIsoName` and the 16-byte name copy in `cdvdman_findfile`. Its fixes are AFL-3.0 like OPL itself, so an OPL fork may port them with credit.

## 11. Test images

- **`yaoplm-test-support` writes its own ISO9660 images** (ADR-0016). Images are generated on the fly through `Read + Seek`, so 4+ GiB fixtures use no disk space.
- **Ten variants** cover the cases above: retail, LF-only, no BOOT2, lower-case key, ELF in a subfolder, long ID, PS1, no `;1`, lower-case name, DVD-9.
- **Console test disc:** `make_boot_iso` packs an open homebrew ELF as a bootable disc with ID `YAOP_000.01`. It used wLaunchELF `BOOT.ELF` from the `latest` release (SHA-256 `8199fbd0…f410`, matching the GitHub release digest); the file is kept outside the repo in `$YAOPLM_TESTDATA\homebrew\`.
