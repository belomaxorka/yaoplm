# 18. Folders OPL reads over SMB

What OPL and its forks do with a folder they read over the network, for the "folder for SMB" mode (NET-1, DEV-2, LIB-1; ADR-0045, `docs/plans/smb-folder-mode.md`). Format details are in research/02 (SMB row in §2, §3, `ul` parts); this note collects what the mode needs. Checked 2026-10-25 against:
- upstream OPL `ps2homebrew/Open-PS2-Loader` at `b3a21997` (2026-10-12);
- YAOPL `belomaxorka/yaopl` at `33a2b19a` (2026-10-23);
- RiptOPL `NathanNeurotic/RiptOPL`: C edition `main` at `04aa1369` (2026-09-17), C++ rebuild `rebuild/main` at `6edea174` (2026-10-20);
- wOPL `ps2homebrew/wOPL` at `4ae7414` (2026-09-13);
- the MMCE fork `ps2-mmce/Open-PS2-Loader`, branch `mmce_support`, at `e554796` (2025-11-24; `master` at `e2090114` has no MMCE code).

Re-checked the same day before the implementation, on fresh shallow clones (`D:` was not connected): every commit above was still the head, and `smb.c:267`, `ethPrefix[40]`, `sbCreateFolders` and `ISO_MAX_PARTS 10` (`modules/iopcore/common/cdvd_config.h:24`) are where §1–§4 say in all six trees.

Tags: [V] read in source at the commit above, [R] from a README.

## 1. Protocol

- Upstream, YAOPL, wOPL and the MMCE fork speak **SMBv1 only**: the in-game reader offers one dialect, `"NT LM 0.12"` (`modules/iopcore/cdvdman/smb.c:267`), and none of them has an SMB2 module in `modules/network/` [V].
- **RiptOPL C edition** has an SMB2/SMB3 driver `modules/network/smb2man` over a vendored libsmb2 and the global key `smb_dialect` (`include/config.h:174`). The Network page offers SMBv1 (default) or SMB2; the choice switches both browsing and the in-game reader, so the server must speak SMB2 for the whole session [V tree, R `README.md` "SMB version"]. Its default SMB port is `1111`, not 445 [R].
- **RiptOPL C++ rebuild** speaks SMBv1 only for now: the dialect row stays greyed until "checklist item 4" (`include/opl.h:310-312`, `src/sources/ethsupport.cpp:365-366`) [V].
- So "SMBv1 only" is true for every build YAOPLM has a profile for except RiptOPL, where it is the default and SMB2 is an option.
- RiptOPL C also reads a PC over the network without SMB: UDPFS (files under `CD/`/`DVD/` served by `udpfsd` or PS2-Servers) and UDPBD (a served FAT/exFAT disk image) launch through Neutrino, which reads neither ZSO nor `ul`; HTTP lists ISOs from `games.csv` and refuses compressed images (`README.md` "PS2 game source" table, `modules/network/udpfs_*`, `smap_udpbd`) [R, V tree]. The SMB mode does not cover them: they are for the network library milestone (NET-2), with their own rules.

## 2. Where OPL looks on the share

- Prefix: `smb0:` plus the optional `eth_prefix` folder plus `\`, built in `char ethPrefix[40]` (`src/ethsupport.c:23`, `66-69`, `432`) [V]. With the NUL, `eth_prefix` fits in **33 bytes**; `sprintf` does not check, so a longer one overflows the buffer. The YAOPLM library root is the share root, or the share's `eth_prefix` folder.
- On connect OPL loads themes from `THM` and languages from `LNG` on the share and **creates `CFG THM LNG ART VMC CHT APPS CD DVD` there** (`src/ethsupport.c:260-268`, `sbCreateFolders` in `src/supportbase.c`) [V]. On a read-only share `mkdir` fails silently. LIB-3 creates the same set.
- The game list comes from the same scanner as BDM, `sbReadList` on the prefix (`src/ethsupport.c:494`). It **writes `CD/games.bin` and `DVD/games.bin` on the share** when the list changed (`src/supportbase.c:190-234`), if the share is writable [V]. OPL lists again when the mtime of `CD` or `DVD` or the size of `ul.cfg` changes (`src/ethsupport.c:455-487`) [V].
- Per-game files live on the share too: CFG, CHT (`sbLoadCheats`, `src/ethsupport.c:641`), VMC (`sysCheckVMC`, `604`, `798`; saving needs write access) and art `ART/<ID>_<suffix>` (`743`) [V].

## 3. What does not live on the share

- **OPL settings.** `conf_opl.cfg` and the other `conf_*.cfg` are looked for and saved on the memory card, BDM (`mass0:`) or the HDD only (`checkLoadConfigBDM`, `checkLoadConfigHDD`, `src/opl.c:778-860`) [V]. A folder used only over SMB never has OPL's own settings; any `conf_opl.cfg` there was copied by hand.
- **The loader.** OPL starts from the memory card (FMCB, PS2BBL) or a USB stick; an ELF in the shared folder is a copy, not what the console runs.
- Therefore the only sign a console leaves in an SMB folder is the `games.bin` record size (research/16 §1.5), and only after it listed the folder on a writable share. YAOPLM deletes `games.bin` after changing `CD`/`DVD` (CLAUDE.md rule 2), so after our writes it is gone until the console lists the folder again.

## 4. Images

- ISO and ZSO work over SMB (ZSO cache setting `smb_cache`, `src/ethsupport.c:728-729`) [V]; OPL 1.1.0 reads no ZSO at all (`LoaderProfile::reads_zso`).
- The reader uses 64-bit offsets (research/02 §2), so a game over 4 GiB is one ISO unless the folder's own file system cannot hold it (FAT32 on the PC side). `ul` on SMB: at most 10 parts (`cdvdman_settings_smb.FIDs[10]`, research/02 `ul`).
- Image file names: at most 159 bytes (`cdvdman_settings_smb.filename[160]`, research/02 §3), already enforced by IMP-4.
- Fragments: the SMB reader opens files by handle on the server; there is no cluster list, so the BDM fragment rule (IMP-7b, ADR-0037) does not apply.

## 5. Consequences for YAOPLM

- The device's place, not its contents, says whether OPL reads it plugged into the PS2 or over SMB: a folder on an internal disk can reach a PS2 only over the network.
- In SMB mode the doctor's FS, cluster, partition, sector and exFAT rules do not apply; the SMB rules above do, with the SMB2 note for RiptOPL.
- The profile question matters most here: no ELF and no settings to read, `games.bin` at best.
- A whole system disk shared as the library works for OPL, but a separate folder is cleaner for sharing and for OPL's own `mkdir`s.
- Later (network library milestone, NET-2): `eth_prefix` ≤33 bytes and RiptOPL's port 1111 matter for the sharing helper and for writing `conf_network.cfg`; POPStarter over SMB uses the `SB.` prefix (research/02, PS1-1).
