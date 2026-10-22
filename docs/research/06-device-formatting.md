# 06 — Preparing storage: formatting USB sticks and SD cards for the PS2

Research date: 2026-10-22. Context: ADR-0013 accepts formatting for v0.2 (SPEC DEV-3, DEV-4) and leaves the per-OS method to this report. It also answers SPEC open question 6: own formatter or OS tools, and FAT32 above 32 GB on Windows.

**Method**
- WebSearch, WebFetch and `gh api`, read only.
- Read-only reading of the local reference clones in `D:\yaoplm-testdata\refs\` (see [REFERENCES.md](REFERENCES.md)), and of source files saved as text.
- No downloaded code was built or run.
- One read-only probe on the dev machine (Windows 11 26200, not elevated): open `\\.\PhysicalDriveN` with access 0 and query it.
- psx-place.com, consolemods.org, ps2-home.com and psdevwiki returned Cloudflare 403. We did not bypass the check; those sources appear only as search snippets.

**Pinned versions**

| Project | Version |
|---|---|
| OPL master | `b3a2199` (2026-10-12) |
| OPL v1.1.0 | `07beff6` |
| ps2sdk | `329e2bb` (2026-10-12) |
| FatFs fork `fjtrujy/FatFs`, branch `iop-r0.16` | the one ps2sdk `download_dependencies.sh` clones |
| Neutrino | `7be8de2` |
| NHDDL | `89141d4` |
| Rufus | 4.15 |
| Raspberry Pi Imager | 2.0.12 |
| balenaEtcher | 2.1.8 |
| udisks | 2.11.2 |
| exfatprogs | 1.4.3 |
| dosfstools | 4.2 |

| Tag | Meaning |
|---|---|
| **[V]** | Verified in source code or an official document (file or URL given) |
| **[D]** | Derived: reasoned from verified code or spec, not tested on hardware |
| **[R]** | Reported by a project README, maintainer, release note or news article |
| **[U]** | Unverified: forum snippet, anecdote, or a claim we could not check |

---

## 0. Summary

| Question | Recommendation |
|---|---|
| Filesystem | **exFAT** for the OPL 1.2.0 beta profile on every BDM target (USB, MX4SIO, MMCE, iLink, HDD in a USB adapter). **FAT32** for the OPL 1.1.0 profile, or when the console starts a loader *from this device* through FMCB 1.966 / legacy `usbhdfsd`, or on user request |
| Partition table | **MBR** with exactly one primary partition in entry 1. Type `0x07` for exFAT, `0x0C` for FAT32, not active. GPT only for disks over 2 TiB (stage 2). Never superfloppy (no table), never extended or logical partitions |
| Logical sector size | **512 bytes only.** Refuse a device that reports 4096 |
| Cluster size | exFAT uses Windows "Default": 4 KiB up to 256 MiB, 32 KiB up to 32 GiB, 128 KiB above. FAT32 uses 32 KiB, or the largest smaller size that still gives at least 65,525 clusters. The user may change it from 4 KiB to 128 KiB (256 KiB with a warning); 512 KiB and above is never offered |
| Alignment | Partition starts at 1 MiB, and the FAT32 data region or exFAT cluster heap is aligned to 1 MiB. SD cards get the SD boundary unit instead (4/16/32/64 MiB). At least 1 MiB is left unallocated at the end |
| Label | Optional, `[A-Z0-9_-]`, at most 11 characters, default `OPL`. OPL never reads it |
| Engine | **Our own pure-Rust formatter** (MBR + FAT32 + exFAT) modelled on FatFs `f_mkfs`, Rufus `FormatLargeFAT32` and exfatprogs `mkfs`. Every layout is checked in CI on image files with `fsck`, `chkdsk` and **FatFs built with the PS2 configuration** |
| Device access | Windows: a UAC-elevated helper on a secured named pipe. Linux: udisks2 `OpenDevice` gives an unprivileged helper a file descriptor (one polkit prompt); `root` is the CLI fallback. macOS: `authopen` file descriptor. Server/Docker: off |
| Safety | Bus allow-list, system disk excluded, content preview, typed size confirmation, fingerprint re-checked through the very handle that writes, MBR written last, read-back verification |
| Stages | v0.1: read-only "storage doctor" (DEV-2). v0.2: formatter on Windows and Linux, MBR ≤2 TiB; macOS too if a spike passes. v0.3: GPT >2 TiB, macOS, MMCE details. Later: server mode, SATA-attached disks, APA |

The biggest surprises:
1. The OPL README advice "Allocation unit size: Default" is written only for the **internal HDD** section, not for USB (§1.6).
2. Current OPL mounts GPT and even partition-less disks, but only since 2023 and April 2025 respectively, and a GPT bug was fixed on 2026-10-05 (§1.4). MBR stays the safe default.
3. No existing PC tool formats USB sticks or SD cards for OPL (§5.1).
4. On macOS, Rust's `std::fs::copy` copies extended attributes, which leaves `._*` files on FAT and exFAT (§1.10).

---

## 1. What the PS2 side needs

### 1.1 The storage stack on the console

OPL 1.2.0 beta, Neutrino, NHDDL, wLaunchELF (since 2022) and PS2BBL share one stack. At the bottom is the BDM block-device layer. Above it come the ps2sdk partition drivers and then `bdmfs_fatfs`, which is FatFs r0.16 built with a PS2 configuration [V].

| Layer | Rule that affects formatting | Source | Tag |
|---|---|---|---|
| Block drivers | `usb` (usbmass_bd), `sdc` (MX4SIO), `sd` (iLink SBP-2), `ata` (internal HDD, `enable_bdm_hdd`). `mx4sio_bd` sets block length with `CMD16, 512` | ps2sdk `iop/...`, OPL `src/bdmsupport.c` | [V] |
| Probe order | Partition drivers MBR, then GPT, then FatFs. A device that neither partition driver claims is handed to FatFs as a whole (superfloppy) | ps2sdk `iop/fs/bdm/src/bdm.c`, `part_driver.c` | [V] |
| MBR driver | The 0xAA55 signature is required.<br>If entry 0 has type `0xEE`, the disk is left to the GPT driver.<br>Sanity check: any used entry with `first_lba == 0` or `first_lba >= sectorCount` makes the table invalid ("most likely a VBR").<br>All **4 primary entries** with a non-zero type are mounted, whatever the type (`// TODO: Filter out unsupported partition types.`).<br>Extended and logical partitions are **not** parsed.<br>`first_lba` and the sector count are u32 | `part_driver_mbr.c` L12–128 | [V] |
| GPT driver | Checks only the `EFI PART` signature: no header or table CRC, no backup header.<br>Entries must be 128 bytes.<br>Skips: zero GUID, Microsoft Reserved, EFI System, the "ignore" attribute, and entries outside the usable LBA range.<br>**Any other type GUID is mounted.**<br>`sectorCount = last_lba - first_lba` is one sector short (harmless).<br>Before ps2sdk `65f988c` (2026-10-05, "continue scanning after unused GPT entries") the scan **stopped at the first empty entry** | `part_driver_gpt.c` | [V] |
| Partition count | `MAX_PARTITIONS 10`. OPL shows `mass0:`…`mass4:` only (`MAX_BDM_DEVICES 5`), one page per mounted volume | ps2sdk `part_driver.h`; OPL `include/bdmsupport.h` | [V] |
| FatFs config | `FF_FS_EXFAT 1`, `FF_LBA64 1`, `FF_MIN_SS = FF_MAX_SS = 512`, `FF_MULTI_PARTITION 0`, `FF_USE_LABEL 0`, `FF_USE_MKFS 0`.<br>Fork-only `FF_FS_MBR 0`: FatFs never parses MBR or GPT itself and expects a VBR at sector 0 of the device it is given | `fjtrujy/FatFs@iop-r0.16 source/include/ffconf.h` L33, 50, 199, 208–218, 246, 312 | [V] |
| FAT32 mount | Boot signature `0xAA55`.<br>The **`"FAT32   "` string at offset 82 is mandatory**: the fallback heuristic can never accept FAT32.<br>`BytsPerSec == 512`; 1 or 2 FATs; `SecPerClus` a power of two ≤128 (**≤64 KiB clusters**); `RootEntCnt == 0`; `FSVer == 0`.<br>FAT type is decided only by cluster count: **≤65,525 clusters is classed FAT16 and rejected**.<br>FSInfo is trusted when its signatures match.<br>Backup boot sector, dirty bit and label are never read | `ff.c` `check_fs` (L3382–3386), `mount_volume` | [V] |
| exFAT mount | `EB 76 90 "EXFAT   "`, MustBeZero area zero, revision exactly 1.00 (L3538).<br>`BytesPerSectorShift == 9`, `NumberOfFats == 1` (TexFAT rejected).<br>Cluster at most **16 MiB**: the size is stored in a WORD, so the spec's 32 MiB wraps to 0.<br>`NumClus ≤ 0x7FFFFFFD`.<br>The Allocation Bitmap entry must be in the root directory's first cluster.<br>**The bitmap's FAT chain must be recorded and contiguous** (L3580). A formatter that leaves the bitmap's FAT entries zero produces a volume the PS2 cannot mount, even though some PC drivers accept it | `ff.c` | [V] |
| Hidden/system attributes | Not honoured. FatFs `dir_read` skips only deleted, dot and label entries; `fs_dread` returns every name | ps2sdk `bdmfs_fatfs/src/fs_driver.c` | [V] |

### 1.2 Per target and per loader

| Target | Driver | Filesystems | Partition tables | Target-specific notes | Tag |
|---|---|---|---|---|---|
| **USB mass storage** | `usb` | FAT32, exFAT (1.2.0 beta) | MBR, GPT (README since 2026-10-05) | Measured speed 750–900 KB/s (USB 1.1). Neutrino: "some (mostly USB3.0 sticks) don't work" | [V] code, [R] README/Neutrino |
| **MX4SIO** (SD in the memory-card slot) | `sdc` | Same as USB | Same as USB | Fixed 512-byte sectors. 1,150–1,500 KB/s. Card compatibility varies ("many SD cards are not compatible", Neutrino). No official cluster or formatter advice | [V], [R] |
| **MMCE** (SD2PSX / sd2psXtd, MemCard PRO 2) | Game files are read by the **card firmware**, not FatFs on the PS2 | FAT32, exFAT (SdFat in sd2psXtd firmware; MCP2 manual) | **MBR only, partition entry 1 only.** sd2psXtd firmware (SdFat `c20d9c4`): boot byte must be 0x00/0x80, no GPT, no superfloppy; FAT needs 2 FATs. GPT support reached the SdFat fork on 2026-09-18 but not the firmware yet | Neutrino: no 64-fragment limit on MMCE. OPL support only in forks: ps2-mmce `mmce_support`, wOPL, RiptOPL (research/02 §2). SD2PSX docs: "partition table type is MBR, … exFAT or FAT32". MCP2: "simple MBR partition scheme", "1GB to 2TB" | [V] firmware, [R] docs |
| **iLink** (IEEE1394 SBP-2) | `sd` | Same as USB | Same as USB | Sector-size handling of `IEEE1394_bd` not inspected | [R] README, [U] sector size |
| **Internal HDD as BDM** | `ata` | FAT32, exFAT | "MBR partitioning (up to 2TB) or GPT partitioning (over 2TB)" | 28/48-bit LBA. README: exFAT "Allocation unit size … Default". APA disks are not mounted as BDM. APA-Jail (APA + MBR exFAT) is a separate layout (PSBBN, RiptOPL) | [R] README, [V] code |

| Software on the console | exFAT | GPT | Partition-less disk | Fragments | Tag |
|---|---|---|---|---|---|
| OPL **1.1.0** (2021, last stable) | **No** (`bdmfs_vfat`) | **No** | Yes (old MBR driver fell back to the raw disk) | **Must be one fragment** (`USBMASS_IOCTL_CHECK_CHAIN` refuses otherwise) | [V] tag `07beff6` |
| OPL **1.2.0 beta** (`latest`) | Yes since rev1880 | Yes since ps2sdk #418 (2023-04-06); gap bug fixed 2026-10-05 | Broken from about rev1981 until ps2sdk `af3d64c` (2025-04-22, PR #751); works now | ≤64 fragments per game since rev1893 | [V] code, [R] README/PR |
| Neutrino / NHDDL | Yes (`bsdfs-exfat.toml` loads `bdmfs_fatfs`) | Yes | As ps2sdk | **64 fragments in total**, ISO and VMCs combined (BDM only) | [R] README, [V] config |
| wLaunchELF (since 2022-06), PS2BBL | Yes (BDM FatFs) | As ps2sdk | As ps2sdk | n/a | [V] |
| FMCB 1.966 / legacy `usbhdfsd` | **No** (the FMCB installer offers optional "EXFAT USB" IRX files) | **No** | — | n/a | [R] installer README, [D] |
| Legacy `usbhdfsd` MBR filter | Mounts only types **0x01, 0x04, 0x06, 0x0B, 0x0C, 0x0E** | — | — | — | [V] ps2sdk `vfat/part_driver.c` |

Consequence for YAOPLM: the device format also depends on **how the user starts the loader**. If FMCB 1.966 or an old uLaunchELF has to read an ELF from this stick, the stick must be FAT32 with type 0x0C. The DEV-4 wizard should ask this question together with the loader choice (LDR-3).

### 1.3 FAT32 or exFAT

| | FAT32 | exFAT |
|---|---|---|
| OPL 1.1.0 | Yes | No |
| OPL 1.2.0 beta, Neutrino, NHDDL, MMCE firmware | Yes | Yes |
| Boot from the device through FMCB 1.966 or legacy uLE | Yes | No |
| Images > 4 GiB | Only as `ul` chunks (IMP-5); OPL 1.2 also accepts ZSO < 4 GiB | Plain ISO/ZSO in `DVD/` |
| Neutrino / NHDDL | ISO only, no `ul` (research/01): DVD-9 games over 4 GiB are not playable from FAT32 | Full |
| VMC | Must be one fragment and below LBA 2^32 (both FS) | Same; rickgaiser in #1695: "a limitation with all exFat devices" — OPL checks VMC fragments, Neutrino maps them |
| Windows can format it | GUI ≤32 GB; command line ≤2 TB only on recent Windows 11 (§2.1) | Yes, any size |

**Default: exFAT** for the 1.2.0 beta profile. FAT32 for the 1.1.0 profile and the legacy-boot case. This matches SPEC DEV-2.

### 1.4 Partition table: MBR, GPT, or none

- **MBR** works on every loader and on the MMCE firmware. Its only limit is 2 TiB (u32 LBA with 512-byte sectors) [V].
- **GPT** works in current OPL, Neutrino and NHDDL [V]. It does **not** work in OPL 1.1.0, the MMCE firmware or legacy `usbhdfsd`.
  - The README listed GPT only from 2026-10-05: commit `9357948`, authored 2026-03-11, merged with PR #1677 [V git log]. Before that it read "both use the MBR partition table".
  - Builds before ps2sdk `65f988c` stop at the first empty GPT entry, so the data partition must be **entry 1** [V].
  - Windows' Disk Management adds a Microsoft Reserved partition on GPT. The ps2sdk driver skips it, and it is not empty, so the scan continues [D].
- **No partition table** (a VBR at LBA 0) mounts on current builds only through a fallback.
  - It was broken between about rev1981 and the April 2025 ps2sdk fix: OPL issues #1271, #1295, #1311, #1493 [R].
  - The detection is fragile: a VBR whose boot code passes the MBR sanity check is taken for an MBR [D].
  - The MMCE firmware does not support it [V].
  - exfatprogs ≥1.4 `-P auto/mbr` writes a "recursive" MBR inside the exFAT boot sector [V]. YAOPLM must not produce that layout.
- **Extended and logical partitions** are ignored by the current MBR driver [V].

**Rule:** MBR with one primary partition in entry 1. GPT only for disks over 2 TiB, with the partition in entry 1, type Microsoft Basic Data, and no MSR. Never superfloppy, never logical.

### 1.5 Logical sector size: 512 only

- FatFs is built with `FF_MIN_SS = FF_MAX_SS = 512` [V].
- `bd_cache.c` hard-codes 512, and so do the MBR and GPT drivers' buffers and OPL's cdvdman (`lsn * 4`) [V].
- The USB driver accepts whatever READ CAPACITY reports, without a check [V].
- 4Kn drives, and USB bridges that present a 512e disk as 4096-byte sectors, therefore do not work. OPL FR #1666 is open. The maintainer's answer: "You can use GPT partitioning for 2+TB drives, and exFat for 4+GB iso's" [R].
- RiptOPL refuses such drives on purpose (error code 500). It notes that a drive moved to a different enclosure "must be reformatted" [R]. A filesystem built through a 4K bridge has `BytesPerSector = 4096` and is unreadable on the PS2 even after the disk is moved [D].
- **YAOPLM must:**
  - always write 512-byte-sector filesystems;
  - refuse a device whose logical sector size is not 512, with a message that names the adapter as the likely cause;
  - flag existing volumes with `BytesPerSector ≠ 512` in DEV-2.

### 1.6 Cluster (allocation unit) size

**Primary statements**
- OPL README, **HDD section only**: "When formatting drives for the exFAT filesystem, please make sure the `Allocation unit size` is set to `Default`." Added by PR #1584 (2025-09-18), with no rationale [V git log].
- The README's USB/MX4SIO/iLink section says nothing about cluster size [V].
- The phrase traces back to grimdoomer's fork release v1.2.0.2-beta (2023-04-17), which says "cluster size must be default" and "Setting the cluster size to 512k WILL cause issues" [R]. No code path that explains a 512 KiB failure was found: FatFs accepts exFAT clusters up to 16 MiB [V].
- The old OPL wiki page `usb-mode` (last edited 2020-07-16, before BDM) says "format all space as FAT32 partition with the biggest cluster size" [R, outdated].

**What "Default" means**
- Microsoft KB140365 (2019 archive) [V]:
  - exFAT: 7–256 MB → 4 KiB; 256 MB–32 GB → 32 KiB; 32 GB–256 TB → **128 KiB**.
  - FAT32: 256 MB–8 GB → 4 KiB; 8–16 GB → 8 KiB; 16–32 GB → 16 KiB. Windows does not format FAT32 above 32 GB.
- exfatprogs (`lib/libexfat.c`), Rufus, hadris-fat and exfat-fs use the same exFAT thresholds [V].
- The SD Association table, as quoted in `mkfs.exfat(8)`, differs above 32 GiB [R]:
  - ≤128 GiB → 128 KiB;
  - ≤512 GiB → 256 KiB;
  - ≤2 TiB → **512 KiB**.
  - So SD Card Formatter on a 1 TB card probably produces the size grimdoomer warns about [D]. We did not check what the tool actually writes [U].

**Community evidence (conflicting)**

| Claim | Source | Tag |
|---|---|---|
| exFAT 32–64 KiB loads themes faster than 128 KiB | JoseAaronLopezGarcia (author of the BDM FatFs driver), OPL #662 | [R] contributor |
| Cluster size "should not" matter much for speed: "we only read the part of the cluster that has data on it" | rickgaiser (maintainer), OPL #662 | [R] |
| 128 KiB vs 32 KiB changed VMC hangs, inconsistently | OPL #1197 | [U] |
| Large clusters broke HDD loading; another user saw no failure up to 512 KiB | OPL #1098 | [U] |
| MX4SIO: 16 KiB vs 128 KiB made no difference; card model matters more | psx-place snippets, OPL #752, #831 | [U] |
| PSBBN creates its exFAT partition with `mkfs.exfat -c 32K -L "OPL"`; #1773 user did the same | PSBBN `scripts/PSBBN-Installer.sh`; OPL #1773 | [V] / [U] |
| "FAT32 with 32 KB clusters" for OPL | No PS2 primary source found. It matches the Windows/Rufus/dosfstools default above 32 GB | [U] |

**What the code says** [D]:
- OPL reads data by sector through the fragment list (`device-bdm.c`), so read speed does not depend on cluster size.
- Cluster size matters once per launch: `get_frag_list()` walks the FAT chain cluster by cluster.
  - A 4 GiB image needs 1,024 FAT sectors (512 KiB) at 32 KiB clusters, 2,048 at 16 KiB and 8,192 (4 MiB) at 4 KiB.
  - At USB 1.1 speeds (~750 KB/s) that is under a second against several seconds.
- For an exFAT file written contiguously with the NoFatChain flag, FatFs synthesises the chain without reading the FAT at all (`get_fat()`, `obj->stat == 2`) [V].
- `ul` chunks (1 GiB) are a multiple of every cluster size, so they leave no slack.
- Slack on small files (CFG, ART) costs about half a cluster per file: around 300 MiB for 5,000 files at 128 KiB.

**Recommendation**
- **exFAT**: Windows "Default" — 4 KiB ≤256 MiB, 32 KiB ≤32 GiB, 128 KiB above. This is what the README asks for, what Windows, exfatprogs and the HDD users in #1695 use, and it stays below the reported 512 KiB problem.
- **FAT32**: 32 KiB.
  - It gives fewer FAT entries to walk at launch, matches #662, and is the >32 GB default everywhere.
  - fatgen103 warns that clusters over 32 KiB break some systems, so 64 KiB is never the default.
  - Step down to 16 KiB and below only when needed to keep **≥65,525 clusters plus a margin**. FatFs classes a FAT32 volume with fewer clusters as FAT16 and rejects it (§1.1).
- **Overrides**: 4 KiB–128 KiB freely, 256 KiB with a warning, never 512 KiB or more.
- **DEV-2** should report a volume's cluster size and warn on exFAT clusters of 512 KiB or more.

### 1.7 Alignment and the end of the disk

- **Partition start.** 1 MiB (LBA 2048) is the Windows, sfdisk ("strongly recommended to use the default") and Rufus default [V]. ps2sdk only requires `first_lba != 0` [V].
- **Inside the volume.**
  - FAT32: pad the reserved sectors (at least 32) so that the data region starts on a 1 MiB boundary, as Rufus does [V].
  - exFAT: `FatOffset` and `ClusterHeapOffset` aligned to the boundary. exfatprogs' default boundary is 1 MiB [V].
- **SD cards** (MX4SIO, MMCE): align to the SDA boundary unit instead.
  - The units are 4 MiB ≤32 GiB, 16 MiB ≤128 GiB, 32 MiB ≤512 GiB and 64 MiB ≤2 TiB [R, `mkfs.exfat(8)`].
  - The man page notes the alignment should be "half of the card's native boundary unit" [R].
  - Raspberry Pi Imager puts its FAT32 partition at sector 8192 (4 MiB) [V].
  - The cost is at most 64 MiB.
- **End of disk.** Leave at least 1 MiB unallocated after the partition.
  - OPL #1773: a Linux-formatted exFAT HDD was detected only after about 0.69 MiB at the end was left free. The cause is unknown [U].
  - A USB bridge and the PS2's ATA path can report slightly different capacities [U].
  - The cost is negligible.

### 1.8 Size limits

| Limit | Value | Tag |
|---|---|---|
| MBR partition | Start and length are u32 sectors, so **2 TiB** with 512-byte sectors | [V] ps2sdk |
| FAT32 volume | `TotSec32` u32, so 2 TiB. Cluster count from 65,525 to 0x0FFFFFF5 | [V] fatgen103, [D] |
| FAT32 file | 4 GiB − 1, so `ul` splitting | [V] |
| exFAT volume | FatFs `NumClus ≤ 0x7FFFFFFD`. With MBR it is still capped at 2 TiB; beyond that GPT is needed | [V] |
| VMC position | Below LBA 2^32 | [V] OPL `bdmsupport.c` |
| Visible volumes in OPL | 5 (`mass0:`–`mass4:`) | [V] |
| MemCard PRO 2 card | "1GB to 2TB", Class 4 or better | [R] manual |
| FAT32 formatting in Windows | GUI: 32 GB. `format.com`: 2 TB on Windows 11 24H2/25H2 since **KB5083631** (2026-04-30, builds 26100/26200.8328) | [V] KB, [R] GUI |

### 1.9 Volume label

- **OPL and ps2sdk never read it.** `FF_USE_LABEL 0`, and a code search found no label use [V]. The label is cosmetic: it is what the user sees on the PC.
- **FAT** [V] (FatFs `f_setlabel`; fatgen103):
  - at most 11 bytes, OEM upper case;
  - forbidden: `" * + , . / : ; < = > ? [ \ ] |`, 0x7F and control characters;
  - the first byte must not be 0xE5;
  - an empty label is `NO NAME`;
  - it lives in two places: `BS_VolLab` in the boot sector and a root-directory entry with `ATTR_VOLUME_ID`. Windows shows the directory entry, so write both.
  - Apple `newfs_msdos` upper-cases the label [V].
- **exFAT**: a Volume Label directory entry of at most 11 UTF-16 characters, using the FileName invalid-character set (0x00–0x1F `" * / : < > ? \ |`) [V, exFAT spec §7.3].
- **YAOPLM:** allow `[A-Z0-9_-]` plus space, at most 11 characters, for both filesystems so every OS shows the same thing. Default `OPL` (PSBBN uses the same label [V]).

### 1.10 Hidden and system files

| Item | Created by | Where | Effect on OPL | Effect on NHDDL | YAOPLM action |
|---|---|---|---|---|---|
| `._<name>` (AppleDouble) | macOS, when a file with extended attributes is copied to FAT/exFAT. This includes Finder copies of downloaded ISOs (quarantine attribute) and **Rust `std::fs::copy`**: on macOS it uses `fcopyfile(…, COPYFILE_METADATA \| COPYFILE_DATA)` | Next to each copied file | **`CD/`, `DVD/`:** `isValidIsoName` has no dot-file check, so `._Game.iso` is mounted and probed. It fails and is skipped, but it is never cached, so it is **re-probed on every refresh** (slower scans) [V].<br>**`LNG/`:** `lngReadEntry` accepts any name containing `.lng`, so `._lang_Russian.lng` becomes a **bogus language entry** [V].<br>`APPS/`, `THM/`, ART/CFG/CHT/VMC: harmless (directory-only scans or exact-name opens) [V] | Skipped (`d_name[0] == '.'`) [V] | On macOS use our own copy loop, which also preallocates (IMP-7), and never `std::fs::copy` to the device [V rust-lang `library/std/src/sys/fs/unix.rs`]. LIB-4: detect `._*` in `CD/ DVD/ LNG/ APPS/` and offer cleanup with confirmation |
| `.Spotlight-V100`, `.fseventsd`, `.Trashes`, `.TemporaryItems` | macOS | Root | None: OPL never lists the root [V] | Skipped [V] | Optional: after formatting on macOS, create `.metadata_never_index` and `.fseventsd/no_log` [U for current macOS] |
| `System Volume Information/` (`IndexerVolumeGuid`, `WPSettings.dat`) | Windows | Root | None | Scanned (no `.`/`$` prefix) but holds no ISOs [D] | None |
| `$RECYCLE.BIN` | Windows | Root | None | Skipped (`$` prefix) [V] | None |
| FAT hidden/system attribute | any | — | Ignored: hidden files are listed like any other [V] | — | Do not rely on attributes to hide anything from the PS2 |

### 1.11 Fragmentation: why a fresh format matters

- OPL 1.2: at most 64 fragments per game, and all `ul` parts share one array (`BDM_MAX_FRAGS 64`) [V].
- OPL checks `iFragCount > BDM_MAX_FRAGS` **per part** and keeps the running total in a `u8 iTotalFragCount` (`src/bdmsupport.c` L432–461) [V]. A multi-part game whose parts total more than 64 fragments is not rejected cleanly: the remaining buffer size goes negative [D]. **YAOPLM must enforce at most 64 fragments in total itself** (IMP-7b).
- VMCs must be a single fragment [V].
- OPL 1.1.0 needs every image in a single fragment [V].
- README: "We do **not** recommend using any defrag programs. The best way for defragmenting - copy all files to PC, format USB, and copy all files back." [R]
- A clean format followed by preallocated sequential writes is the practical guarantee. Formatting is therefore also the fix the app can offer when the fragmentation check fails.

---

## 2. How to format from a desktop app, per OS

### 2.1 Windows

| Method | Admin | FAT32 > 32 GB | Cluster | Partition/alignment | Notes | Tag |
|---|---|---|---|---|---|---|
| Explorer dialog / `SHFormatDrive` | Not for removable media (policy "Allowed to format and eject removable media") | No | Dialog | No | Interactive only | [V] policy, [R] limit |
| `format.com /FS:exFAT\|FAT32 /A:<size> /V:<label> /Q /X /Y` | **Yes** ("You must be a member of the Administrators group") | Only on Windows 11 24H2/25H2 with KB5083631 or later (2026-04). Not on Windows 10 or older 11 builds | `/A`: FAT32 512 B–64K; exFAT 512 B–32M | Formats an existing volume only | Docs still state a "65527 and 4177917" cluster range | [V] |
| `Format-Volume` (+ `Clear-Disk -RemoveData`, `Initialize-Disk -PartitionStyle MBR`, `New-Partition -Offset/-Alignment -MbrType`) | Cmdlet pages are silent; Disk Management needs Administrators or Backup Operators | Unknown | `-AllocationUnitSize` | Yes. `-MbrType` default is "Huge" | "The WhatIf switch does not work with this cmdlet" | [V] docs, [U] admin and limit |
| `diskpart` script (`clean`, `convert mbr`, `create partition primary align=1024 id=0c`, `format fs=fat32 unit=32K quick label=`) | **Yes** | No (archived docs) | `unit=` | `align=` (KB). Default MBR type 0x06 | exFAT not in the archived syntax | [V], [U] exFAT |
| **Raw**: own formatter via `\\.\PhysicalDriveN` (Rufus, fat32format, RPi Imager) | **Yes** | Yes, to 2 TiB | Full | Full | The only route with identical output everywhere | [V] |

**Admin facts**
- Opening `\\.\PhysicalDriveN` with access 0 works without admin. `IOCTL_STORAGE_QUERY_PROPERTY` (BusType, RemovableMedia, vendor/product/serial) and `IOCTL_DISK_GET_DRIVE_GEOMETRY_EX` succeed [V, local probe: USB stick BusType 7, NVMe 17].
- `IOCTL_DISK_GET_DRIVE_LAYOUT_EX` and `IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS` are `FILE_ANY_ACCESS` [V winioctl.h].
- `GENERIC_READ` fails with ERROR_ACCESS_DENIED even on a USB stick [V, local probe]. **Every write path needs UAC.**
- `Get-Disk` (MSFT_Disk: `IsSystem`, `IsBoot`, `BusType`, `PartitionStyle`) works unelevated [V, local probe].

**Raw write sequence** (Rufus `src/drive.c`, `src/format.c`, `src/format_fat32.c`) [V]
1. Lock and dismount every volume on the disk: `FSCTL_LOCK_VOLUME`, then `FSCTL_DISMOUNT_VOLUME`.
   - Rufus retries for about 15 s and names the processes holding the drive.
   - The lock fails while files are open, or on the system volume or a page-file volume [V MS docs].
2. Open the physical drive read/write and call `FSCTL_ALLOW_EXTENDED_DASD_IO`.
3. Write the partition table with the IOCTLs:
   - `IOCTL_DISK_CREATE_DISK` (without it, `SET_DRIVE_LAYOUT_EX` fails);
   - `IOCTL_DISK_SET_DRIVE_LAYOUT_EX`;
   - `IOCTL_DISK_UPDATE_PROPERTIES`.
   - Rufus: Windows keeps a **cached** partition table if the MBR is written raw, and also zeroes old partition starts because UPDATE_PROPERTIES alone does not clear the cached view.
4. Write the FAT32 structures, then dismount, unlock and close so Windows mounts the new filesystem.
   - Rufus `FormatLargeFAT32` writes through the new partition's volume handle after locking it.
   - Rufus formats exFAT, NTFS and FAT32 ≤32 GB through the undocumented `fmifs.dll FormatEx`.
   - Vista and later block raw writes into sectors that belong to a mounted volume [R OSR; one line in the CreateFileW docs].
- I/O must be whole, aligned sectors; volume handles are non-cached [V CreateFileW docs].
- Rufus first partition: "Go with the MS 1 MB wastage" [V].

**Elevation and IPC**
- `ShellExecuteExW` with verb `runas` and `SEE_MASK_NOCLOSEPROCESS` returns the process handle. `SHELLEXECUTEINFOW` has **no std-handle fields**, so stdio pipes cannot be passed to an elevated child [V].
- Rufus and Raspberry Pi Imager 2.x avoid the issue by elevating the whole app (`requireAdministrator` manifest) [V].
- Etcher keeps the GUI unelevated and starts its helper with `powershell Start-Process … -Verb runAs`. They talk over an **unauthenticated** WebSocket on 127.0.0.1:3435 [V]. Do not copy that.

### 2.2 Linux

| Method | Privilege | Cluster control | Notes | Tag |
|---|---|---|---|---|
| **udisks2** `Block.Format("dos")` → `PartitionTable.CreatePartition(offset, size, "0x07"\|"0x0c")` → `Block.Format("exfat"\|"vfat", {label})` | polkit `modify-device`: **no password** for an active local session on a removable or USB drive (`HintSystem` false) | **Only through `mkfs-args`** (udisks ≥2.10), which **always** escalates to `modify-device-system` (`auth_admin_keep`, password) | libblockdev runs `mkfs.vfat` without `-F` (FAT32 chosen automatically at ≥512 MiB) and `mkfs.exfat -n` (`-P none` on exfatprogs ≥1.4). exfatprogs' defaults equal Windows "Default"; dosfstools' FAT32 defaults are 4–16 KiB below 32 GiB | [V] `udiskslinuxblock.c` L2986, policy, libblockdev `vfat.c`/`exfat.c` |
| **udisks2** `Block.OpenDevice("rw", {flags})` → file descriptor; then our own formatter | `open-device`: `auth_admin_keep` (one password prompt per session) | Full | Used by GNOME Impression and System76 Popsicle (both Rust, both on Flathub without `--device=all`) | [V] |
| `mkfs.fat` / `mkfs.exfat` + `sfdisk` / `parted` directly | root | Full (`-s`/`-c`, `-b` boundary) | sfdisk default grain is 1 MiB. `mkfs.fat` refuses a partitioned whole device without `-I` | [V] man pages |
| `pkexec` + own polkit policy | root via `auth_admin` | Full | AppImage needs a per-path policy (RPi Imager generates one). Etcher uses `pkexec`; Ventoy's GUI restarts itself through `pkexec` | [V] |

**Enumeration and safety**
- Disks: `lsblk --json --bytes -o NAME,PATH,TRAN,RM,HOTPLUG,MODEL,SERIAL,SIZE,LOG-SEC,PHY-SEC,RO,TYPE,PKNAME,MOUNTPOINTS,FSTYPE,LABEL,PTTYPE`, or the udisks2 `Drive` properties `ConnectionBus`, `Removable`, `Serial`, `Size` together with `Block.HintSystem`.
- `/sys/block/X/removable` is usually 0 for USB HDDs [U].
- System-disk heuristics:
  - drivelist: `isSystem = !removable && !virtual`.
  - RPi Imager: any disk mounted at `/`, `/usr`, `/var`, `/home`, `/boot`, `/boot/firmware` or `/snap/*`; NVMe counts as system by default.
  - PS2 HDD Manager adds `/boot/efi`, swap, holders (LVM, LUKS, md) and refuses everything if the check itself fails [V].
- RPi Imager opens block devices with **`O_EXCL`** so udisks/udev cannot automount during the write [V].
- Re-reading the partition table (`BLKRRPART`) needs `CAP_SYS_ADMIN` [V]. With a udisks file descriptor, rely on udev's watch-on-close re-read, or call `Block.Rescan` [U].

**Security notes**
- CVE-2025-8067: udisksd out-of-bounds read, fixed in 2.10.2 [V].
- CVE-2025-6019: libblockdev via udisks, `allow_active` → root. Qualys' mitigation is to set `modify-device` to `auth_admin` [V].
- So hardened systems may ask for a password even for USB. The app must handle a polkit prompt at every step.

**Licences.** dosfstools is GPL-3.0 and exfatprogs is GPL-2.0. Running them as separate programs is aggregation under the GPL FAQ [V]; bundling them means shipping their source.

### 2.3 macOS

| Method | Privilege | Cluster control | Notes | Tag |
|---|---|---|---|---|
| `diskutil eraseDisk ExFAT\|"MS-DOS FAT32" <NAME> MBRFormat /dev/diskN` | "Ownership of the affected disk is required". Whether that works without sudo for the console user on external disks: [U] | **None** | Partition offset and newfs defaults not documented | [V] man, [U] rest |
| `newfs_msdos -F 32 -b <bytes> -v <label>` / `newfs_exfat -b <bytes> -v <label>` on `/dev/rdiskNs1` | root [U] | Full | `newfs_msdos` FAT32 defaults are as Microsoft's, and **SDHC cards get 64 sectors per cluster**. It defers SDXC to `newfs_exfat` | [V] Apple msdosfs source |
| **`authopen`** file descriptor + own formatter | Admin credentials through Authorization Services (`sys.openfile.readwrite.<dev>`); fd arrives over `SCM_RIGHTS` | Full | Raspberry Pi Imager (`src/mac/macfile.cpp`). Unmount first (`diskutil unmountDisk`) | [V] |
| `SMAppService` privileged daemon (macOS 13+) | User approval in System Settings | Full | `SMJobBless` deprecated in 13.0; `AuthorizationExecuteWithPrivileges` deprecated in 10.7 | [V] Apple docs |

- **Enumeration.** `diskutil list -plist external physical` and `diskutil info -plist` give `Internal`, `Removable`, `RemovableMedia`, `Ejectable`, `BusProtocol`, `MediaName`, `Size`, `DeviceBlockSize`, `Writable`, `OSInternalMedia` [V via ec2-macos-utils types].
- **System disk heuristic.** drivelist: `isSystem = internal && !removable` [V].
- **Etcher on macOS.** `sudo -E --askpass` with an osascript dialog [V].
- **Unknown.** Whether DiskArbitration re-probes the new MBR after an `authopen` write without a replug [U].

### 2.4 How other apps elevate

| App | Windows | Linux | macOS | IPC | Tag |
|---|---|---|---|---|---|
| Rufus (GPL-3) | Whole app `requireAdministrator` | — | — | in-process | [V] |
| balenaEtcher (Apache-2.0) | GUI unprivileged; helper via `Start-Process -Verb runAs` | `pkexec --disable-internal-agent` / kdesudo | `sudo -E --askpass` | WebSocket on localhost, no auth | [V] |
| Raspberry Pi Imager 2.x (Apache-2.0) | Whole app `requireAdministrator` | Whole app relaunched via `pkexec` with a policy file | `authopen` fd | in-process | [V] |
| Ventoy (GPL-3) | Admin GUI [U]; USB-only filter, VDS/diskpart/WMSA disk back ends [V] | `Ventoy2Disk.sh` as root; GUI `restart_by_pkexec` | — | — | [V] |
| GNOME Disks | — | udisks2 polkit | — | D-Bus | [V] |
| Impression, Popsicle (Rust) | — | udisks2 `OpenDevice` fd; Flatpak with `--system-talk-name=org.freedesktop.UDisks2` only | — | D-Bus | [V] |
| caligula (Rust, GPL-3) | — | Re-execs a "herder" daemon through the first of sudo/doas/run0/su | osascript, then sudo | stdio | [V] |
| PS2 HDD Manager (GPL-3+) | `requireAdministrator` | Separate writer process via `pkexec` | — | — | [V] |
| PSBBN Definitive | `Start-Process -Verb RunAs`, then `wsl --mount --bare` | `sudo` per command | — | — | [V] |

### 2.5 Sandboxes and server mode

- **Flatpak.** The default sandbox has "No access to any device nodes"; `--device=all` gives all of them [V]. Flathub: "Static permissions must be kept to an absolute minimum" [V]. The precedent is `--system-talk-name=org.freedesktop.UDisks2` plus `OpenDevice` (Impression) [V]. **The udisks route is the only one that fits our planned Flatpak (M7).**
- **Snap.** The `udisks2`, `block-devices` and `removable-media` interfaces are all deny-auto-connect; `removable-media` gives no raw devices [V snapd source].
- **Docker** (`yaoplm-server`):
  - No devices by default; `--device=/dev/sdX` is fixed at container creation and is unsafe for hot-plugged devices [V Docker docs].
  - Partition re-read needs `CAP_SYS_ADMIN`, which is not in the default set [V].
  - The browser user cannot physically see which stick is plugged into the server.
  - **Formatting in server mode should stay off** until a later stage, and then only with an explicit `--allow-format <by-id path>` allow-list.
- **Headless Linux `yaoplm-server`.** A process outside an active session gets `auth_admin` from polkit and has no agent to answer it [V policy]. Formatting there needs root anyway.

---

## 3. Rust options

### 3.1 Formatter crates

| Crate | Version (date) | Licence | FAT32 format | exFAT format | Assessment | Tag |
|---|---|---|---|---|---|---|
| `fatfs` (rafalh) | 0.3.6 (2023-01-17). Master says 0.4.0, unreleased since 2023 (issue #81) | MIT | `format_volume` on `Read+Write+Seek` | No | **Not usable for devices.**<br>0.3.6 hard-codes `hidden_sectors: 0` and 8 reserved sectors, has no alignment option, caps clusters at 32 KiB, and writes no backup FSInfo.<br>The FSInfo free-count bug is fixed only on master (PR #108).<br>Fine for reading test images | [V] |
| `fscommon` | 0.1.1 (2020) | MIT | helpers | — | `BufStream` is not sector-aligned; `StreamSlice` gives a partition view | [V] |
| `hadris-fat` | 2.5.0 (2026-10-03); 3.0.0-rc.2 (2026-10-15) | MIT | Yes; `with_alignment`, `with_partition_offset`, label, cluster | 2.5: behind `unstable-exfat` ("Experimental"). 3.0-rc: "exFAT is stable", formats with 1 MiB alignment and 128 KiB default | Same author as hadris-iso (already in our stack). One maintainer, RC only; `hadris-fs` has no stable release. **Candidate for verification reads, and a fallback engine once 3.0 is final** | [V] |
| `exfat-fs` | 0.1.3 (2025-06) | MIT | — | Yes (`boundary_align` default 1 MiB) | 3★, no activity since 2025-06 | [V] |
| `ferrosys`, `fstool`, `rimfs-exfat` | 2026, new | MIT/Apache | yes | yes | Too young | [V] |
| `mbrman` | 0.6.1 (2025-07) | MIT OR Apache-2.0 | MBR with `align` (default 2048) | — | Many small writes; a raw Windows handle needs a sector buffer | [V] |
| `gptman` | 3.1.1 (2026-04) | MIT OR Apache-2.0 | GPT with protective MBR, `DEFAULT_ALIGN 2048` | — | **Use for GPT in stage 2** | [V] |
| `hadris-part` | 2.5.0 / 3.0.0-rc.2 | MIT | MBR/GPT, 1 MiB default alignment | — | Option | [V] |

### 3.2 Device access, enumeration, elevation

| Need | Option | Note | Tag |
|---|---|---|---|
| Windows IOCTLs, SetupDi, ShellExecuteExW, named pipes | `windows` 0.62 / `windows-sys` 0.61 (MIT OR Apache-2.0) | Own code. `STORAGE_BUS_TYPE`: Usb = 7, Sd = 0x0C, Mmc = 0x0D | [V] |
| Linux D-Bus to udisks2 | `zbus` (MIT; has blocking proxies), or the `udisks2` crate 0.4.0 (LGPL-2.0-or-later, zbus 6, pulls in `gettext-rs`) | Keep it in the helper or app layer, not in `yaoplm-device`, to respect ADR-0003. Whether `udisks2` offers blocking proxies: [U] | [V] |
| Physical disk list | `sysinfo` lists **mounted volumes only** (Windows `is_removable` = `GetDriveTypeW`). `rs-drivelist` 0.9.4 (port of balena drivelist with `isSystem`/`isUSB`/`isCard`; shells out to lsblk/diskutil; Windows code on the old `winapi` crate; 3★) | Write our own per-OS enumerator. Use drivelist and RPi Imager as references | [V] |
| Elevation | `runas` 1.2.0 (2024; on macOS it uses the deprecated `AuthorizationExecuteWithPrivileges`). `elevated-command` 1.1.3. `privilege` 0.3.0 | About 50 lines of our own code are simpler and auditable | [V] |
| Tauri | No Tauri 2 plugin for disks or elevation was found | — | [V] absence |

### 3.3 Writing our own formatter

**Effort** [D, from the specs and the references below]
- MBR writer: about 60 lines.
- FAT32: about 400 lines. It writes:
  - the boot sector: `EB 58 90`, `MSWIN4.1`, `HiddSec = partition LBA`, `"FAT32   "`;
  - FSInfo at sector 1;
  - backup boot and FSInfo at sectors 6 and 7;
  - the reserved area padded so the data region lands on the alignment boundary;
  - two FATs with entries 0–2 set;
  - root cluster 2 with the label entry.
- exFAT: about 800 lines plus a 5,836-byte compressed up-case table (checksum `E619D30D`). It writes:
  - a 12-sector boot region twice, with its checksum;
  - the FAT, with chains for the bitmap, up-case table and root directory (**required by FatFs**);
  - the allocation bitmap;
  - the up-case table;
  - a root directory with label, bitmap and up-case entries.

| Reference | What it is | Licence | Use | Tag |
|---|---|---|---|---|
| FatFs `f_mkfs` / `f_fdisk` (ChaN, R0.16) | FAT12/16/32 and exFAT with `align`, `au_size`, `n_fat`; MBR/GPT | 1-clause BSD-style; the appnote says it "can also be changed to GNU GPL" | **Port it**, with the notice kept and an `ATTRIBUTION.md` entry. Its exFAT output is FatFs-mountable by construction | [V] |
| Rufus `FormatLargeFAT32` | FAT32, ≤2 TiB, data region aligned to 1 MiB | GPL-3.0-or-later (derived from Ridgecrop) | Reference for the FAT32 layout and the Windows I/O path | [V] |
| Raspberry Pi Imager `disk_formatter.cpp` | MBR + FAT32 at 4 MiB, type 0x0C | Apache-2.0 | Reference | [V] |
| exfatprogs `mkfs/mkfs.c`, `lib/libexfat.c` | exFAT, boundary alignment, SDA table | GPL-2.0-or-later | Reference and cross-check in CI | [V] |
| dosfstools `mkfs.fat.c` | FAT, `align_structures` | GPL-3.0 | Cross-check in CI | [V] |
| Ridgecrop fat32format | FAT32 | "GPL" (version not stated) | Background only | [V] |

**Legal**
- **fatgen103** is published under a limited licence whose patent covenant covers EFI uses only [V]. Implement from the public references above rather than from its text.
- The VFAT long-file-name patents are reported expired [R]. A formatter needs no LFN anyway.
- **exFAT spec:** the 2019 revision removed the confidentiality notice and the licence agreement [V]. Microsoft backed adding exFAT to OIN's Linux System Definition [V]. OIN protection is scoped to Linux [U].
- **Open question for an ADR:** is an exFAT *formatter* in a GPL-3 app on Windows or macOS covered?
  - Mitigation path: the engine sits behind a trait, so if the review says no, the exFAT path on Windows and macOS can call the OS formatter (`format.com /FS:exFAT /A:…`, `newfs_exfat`). Both OS vendors license exFAT.
  - Many MIT and GPL userspace exFAT formatters already ship (exfatprogs, hadris, FatFs).

---

## 4. Safety design

### 4.1 What existing tools do against picking the wrong disk

| Technique | Rufus | Etcher | RPi Imager | Ventoy | GNOME Disks / udisks | PS2 HDD Manager |
|---|---|---|---|---|---|---|
| Bus allow-list by default | USB, UAS, card readers | — | — | USB only ("Show All Devices" to override) | `HintSystem` makes it admin-only | — |
| Hide the system/boot disk | Yes: Windows dir, app dir, non-USB fixed disks | Yes: "Show N hidden" | Yes: "Exclude system drives" on by default | via the bus filter | password | Yes: refuses; also refuses if the check fails |
| Size heuristic | `IsHDD` score (>800 GB, >1.8 TB, model/VID tables); cards >500 GB hidden | Warning over **128 GB** ("unusually large drive") | — | — | — | refuses <8 GiB |
| Exclude source / self | App's own disk | Source image drive | — | — | — | — |
| Read-only / sector check | — | "Locked" | read-only shown | — | — | refuses `≠512` sectors and write-protected disks |
| Typed confirmation | — | — | Type the drive name (system drives only) | Type `YES` on reinstall | — | Type **`ERASE /dev/sdX`** |
| Double confirmation / delay | Extra box for multi-partition disks | Modal for large or system disks | **2 s countdown** on "ERASE all data" | Two Yes/No boxes, default No | One dialog | Yes/Cancel (default Cancel), then the typed text |
| Busy-process detection | Yes (names the processes) | — | `O_EXCL` | — | `tear-down` option | Lists busy PIDs, unmounts children |
| Re-check before the write | — | — | — | — | — | **Writer re-checks identity and `--expected-size`**; refuses swap and holders |

PS2 HDD Manager has the strongest set of guards. Its writer refuses ([V] `src/ui/Ps2HddSetupDialog.cpp`, `src/core/PhysicalDisk.cpp`, `src/formatter_main.cpp`):
- disks with an MBR active flag, an EFI System Partition or the GPT platform-required attribute;
- virtual, file-backed and Storage Spaces disks;
- everything, when the system-disk check itself is unavailable.

After writing, it verifies that 0 partitions remain.

### 4.2 Proposed pipeline for YAOPLM (DEV-3)

1. **Enumerate** physical disks, unelevated (§2). For each, collect:
   - bus type and removable flag;
   - vendor, model, serial;
   - size in bytes;
   - logical and physical sector size;
   - read-only flag;
   - partition style and entries;
   - volumes with FS type, label, used and free space, and mount points.
2. **Filter.** Only USB, SD or MMC bus disks are shown (ADR-0013). Never show:
   - the system or boot disk, or any disk with a page file, swap or hibernation file;
   - disks with LVM, LUKS or md holders;
   - the disk holding the YAOPLM binary, its data or cache directory, or the source images of a pending job;
   - read-only media (SD lock switch) and devices with a logical sector size other than 512. These are listed but disabled, with the reason.
   - SATA/NVMe disks are not offered in v0.2. HDD users connect the drive through a USB adapter.
3. **Show what will be lost.**
   - Model, serial, exact size ("64,023,232,512 bytes, 59.6 GiB").
   - Every partition with FS, label and used space.
   - The top-level folder names of mounted volumes.
   - **"This disk contains a PS2 library: 152 games"** when `CD/`, `DVD/` or `ul.cfg` is found.
   - An extra warning for "looks like a hard drive" (fixed media, >512 GB) and for multi-partition disks.
4. **Plan (dry run).** Print the exact layout, for example: "MBR; partition 1 at LBA 8192 (4 MiB), 59.5 GiB, type 0x07; exFAT, 128 KiB clusters, cluster heap aligned to 16 MiB, label OPL; then create folders (LIB-3) and install OPL (LDR-3)". Nothing is written. `yaoplm device format` prints only the plan unless `--confirm` is given.
5. **Confirm.**
   - The user types the size shown in the dialog (for example `59.6`). ADR-0013 suggests "size or label"; size is always present and is the best way to tell a stick from a backup drive.
   - Plus a checkbox when existing data was found.
   - The final button stays disabled for 3 s.
   - CLI: `--confirm <size>` must match. `--yes` alone is **not** accepted for formatting.
6. **Start the helper** (§4.3) and send it the plan plus the **fingerprint**: bus, vendor, model, serial, size, logical sector size, partition-table ID (MBR signature or GPT GUID), and a hash of the partition entries.
7. **Re-check through the write handle.**
   - The helper first opens the device exclusively (Windows: lock every volume; Linux: `O_EXCL`; macOS: unmount and `authopen`).
   - Then it reads the identity and the partition table **through that same handle** and compares them with the fingerprint. Any difference aborts before the first write.
   - This closes the time-of-check/time-of-use gap of disk numbers being reassigned after a replug.
8. **Write order** (point of no return = step b):
   - a. sanity-check the computed layout against the FatFs mount rules in memory;
   - b. zero the first and last 1 MiB and the first 64 KiB of every old partition, so no OS mounts a half-old filesystem;
   - c. write the filesystem structures inside the new partition range;
   - d. flush;
   - e. **write the MBR (or GPT) last**;
   - f. flush and ask the OS to re-read the table.
   - An interruption before (e) leaves a disk with no partition table, which every OS shows as "uninitialized" rather than as a corrupt volume an OS might try to "repair".
9. **Verify.**
   - Read back every written sector and compare hashes.
   - Parse the result with our reader, which applies the FatFs rules in §1.1.
   - After the OS mounts it, check the FS type and capacity, then write, read and delete a probe file.
   - A full surface test is out of scope. A capacity check for **fake-capacity sticks** (H2testw/F3-style) is a later option.
10. **Failure handling.**
    - Before step (b): unlock, remount, "nothing was changed".
    - After step (b): "the disk is now blank; retry". Formatting is idempotent, so retrying is safe.
    - Never retry automatically and never "repair".
    - Log every step to the app data directory, not the device.
    - Report a hot-unplug, seen as an I/O error, separately.
11. **Progress.** Formatting writes metadata only: up to about 512 MiB of FATs for a 2 TiB FAT32 volume, about 70 MiB for exFAT. The helper sends `JobEvent`-style progress.

### 4.3 Helper process and IPC

ADR-0013 calls for `yaoplm helper --stdio` "with elevated rights". The findings refine that:

- **Windows.**
  - The helper is the CLI binary, shipped as a Tauri sidecar, so the elevated process never loads the webview.
  - It is started with `ShellExecuteExW("runas", "yaoplm.exe", "helper --pipe <name> --nonce <hex>")`.
  - Because stdio cannot cross `runas`, the parent creates `\\.\pipe\yaoplm-<128-bit random>` with:
    - `FILE_FLAG_FIRST_PIPE_INSTANCE`;
    - `PIPE_REJECT_REMOTE_CLIENTS`;
    - a DACL that allows only the current user SID.
  - After the client connects, the parent checks `GetNamedPipeClientProcessId` against the PID from `hProcess`, and the helper sends the nonce first.
  - The protocol is JSON lines, as with `--stdio`.
  - The helper accepts **one** plan for **one** disk and exits.
- **Linux.** The helper stays **unprivileged**. It asks udisks2 for `OpenDevice("rw")` on exactly one device, which triggers one polkit prompt, and gets a file descriptor. This is least privilege: the helper never runs as root. Fallback for systems without udisks, and for the CLI: run as root (`sudo yaoplm device format …`).
- **macOS.** The helper is unprivileged and gets the file descriptor from `authopen` (an admin password prompt), as Raspberry Pi Imager does.

**Suggested ADR amendment** (not made here): "the helper gets write access to the single target device through the OS broker (UAC / udisks2 / authopen); it runs elevated only on Windows".

### 4.4 Testing (TESTING.md, CLAUDE.md rule 1)

- **Image files.** Every layout (sizes 64 MiB … 2 TiB as sparse files, both filesystems, every cluster option, SD alignment) is formatted onto an image file. Then:
  - Linux CI: `fsck.fat -n` and `fsck.exfat -n`, plus a loop-mount read/write smoke test;
  - Windows CI: attach as VHD with `Mount-DiskImage` and run `chkdsk`;
  - macOS CI: `hdiutil attach -nomount`, then `fsck_msdos -n` and `fsck_exfat -n`;
  - a **FatFs harness**: compile `fjtrujy/FatFs@iop-r0.16` with its PS2 `ffconf.h` in a dev-only test crate (`cc`) and mount every image exactly as the PS2 would.
- **Golden tests.** Byte comparisons of the boot sectors against values computed by hand from the specs.
- **Helper tests.** The helper runs against image files and loop devices only. A test-only `--target-image` mode keeps all guard logic active.
- **Real devices.** Formatting real devices is manual and console-tested by the owner (ADR-0013).

---

## 5. Existing PS2 tools and community advice

### 5.1 Do PS2 tools format drives?

**No existing PC tool formats a USB stick or SD card as FAT32 or exFAT for OPL.** Every "format" feature found writes APA/PFS to an internal HDD. The one exception is PSBBN's exFAT partition, which is on the HDD too.

| Tool | Formats? | What and how | Disk-selection guards | Tag |
|---|---|---|---|---|
| OPL Manager V24 (closed) | No | Docs and changelog never mention FS formatting. HDD mode wraps hdl_dump ("run OPLM as administrator") | — | [V] absence in docs |
| OrbitPS2 Manager | No | `diskutil` only for listing optical drives | — | [V] |
| OPL PC Tools 4.0 (`brainstream/OPL-PC-Tools`) | No | Formats VMC images only | — | [V] |
| PyOPLM, Tihwin, PS2IsoManager | No | — | — | [V] |
| USBUtil 2.2 (closed) | Not known | — | — | [U] |
| **PSBBN Definitive** (GPL-3) | Yes, HDD | `sfdisk` with `,${APA_MiB}MiB,17 / ,32MiB,17 / ,,07`, then **`mkfs.exfat -c 32K -L "OPL"`** on partition 3. `wipefs -a` and zeroing of the first and last 100 MiB | `lsblk` list; the device must be in it; refuses <31 GB; type `yes`; unmounts all. **No system-disk filter** | [V] |
| HDL Batch Installer (GPL-3) | Yes, APA | `iomanX_format` + PFS partitions | Lists `\\.\PhysicalDrive0..24` with **no system or bus filter**; four confirmations | [V] |
| PS2 HDD Manager (GPL-3+) | Yes, APA (+ Extended APA >2 TiB) | patched pfsshell | Strongest guards (§4.1) | [V] |
| PFS BatchKit Manager (no licence) | Yes, APA | pfsshell `initialize yes` | Hides PHYSICALDRIVE0 from the list, but typing a number accepts any disk | [V] |
| pfsshell | Yes, APA | `initialize <anything>`: only the argument count is checked | none | [V] |

### 5.2 Community recommendations

| Recommendation | Where it comes from | Status |
|---|---|---|
| exFAT with "Default" allocation unit | OPL README, **HDD section** (PR #1584, 2025); grimdoomer v1.2.0.2-beta; RiptOPL README and `docs/EXFAT.md` | [R]. Consistent with the code. Not stated for USB |
| Avoid 512 KiB exFAT clusters | grimdoomer release | [R], no mechanism found |
| FAT32 with 32 KiB clusters | Widely repeated; no PS2 primary source | [U]. Reasonable (§1.6) |
| "Biggest cluster" on FAT32 | OPL wiki `usb-mode` (2020, pre-BDM) | [R], outdated, contradicts the above |
| "Use MBR, not GPT" | OPL README until 2026-10-05; ConsoleMods snippet; RiptOPL README ("MBR is the conventional setup for USB and SD media"); SD2PSX and MCP2 docs | [R]. True for 1.1.0, MMCE and legacy loaders; current OPL also reads GPT |
| "Only the first partition is seen" | Old `usbhdfsd` | [V] outdated: current BDM mounts up to 4 MBR primaries |
| Use SD Card Formatter | SD2PSX docs; MemCard PRO 2 manual ("Quick Format"); SDA ("OS tools … may not be optimized") | [R]. Risk of 256–512 KiB clusters on large SDXC cards [D] |
| Copy games one at a time; do not use defrag tools | OPL README | [R] |
| 4K-sector enclosures break OPL | RetroRGB 2023; RiptOPL README; OPL #1666 | [R] / [V] code |
| Windows FAT32 >32 GB: use Rufus, GUIFormat or fat32format | Common in PS2 guides | [U]. Now also `format.com` on current Windows 11 |

---

## 6. Recommendation for YAOPLM

### 6.1 Defaults per target

| Target (DEV-4 "how will you use this device") | FS | Table | Partition start / FS alignment | Cluster |
|---|---|---|---|---|
| USB stick or USB HDD/SSD, OPL 1.2.0 beta / Neutrino / NHDDL | exFAT | MBR (≤2 TiB) | 1 MiB / 1 MiB | ≤256 MiB: 4 KiB; ≤32 GiB: 32 KiB; >32 GiB: 128 KiB |
| Same, but the loader is started from this stick by FMCB 1.966 or legacy uLE, or the OPL 1.1.0 profile | FAT32 (type 0x0C) | MBR | 1 MiB / 1 MiB | 32 KiB (smaller only to keep ≥65,525 clusters) |
| MX4SIO SD card | exFAT (FAT32 as an option) | MBR | SDA boundary: 4 MiB ≤32 GiB, 16 MiB ≤128 GiB, 32 MiB ≤512 GiB, 64 MiB above | as for USB |
| MMCE SD card (SD2PSX, MCP2) | exFAT or FAT32 | **MBR, entry 1, mandatory** | SDA boundary | as for USB. FAT32 always with 2 FATs |
| iLink drive | as USB | MBR | 1 MiB | as for USB |
| Internal PS2 HDD in a USB adapter (BDM `ata`) | exFAT | MBR ≤2 TiB; GPT >2 TiB in stage 2 (entry 1, Basic Data, no MSR) | 1 MiB / 1 MiB | 128 KiB ("Default") |
| Disk >2 TiB in v0.2 | exFAT | MBR, partition capped below 2 TiB, the rest left unused, with an explanation | 1 MiB | 128 KiB |

Common rules:
- 512-byte logical sectors, else refuse.
- One partition; MBR boot flag 0x00; random disk signature.
- At least 1 MiB unallocated at the end.
- Label `OPL` (editable, `[A-Z0-9 _-]{0,11}`).
- FAT32: 32 reserved sectors or more (padded for alignment), 2 FATs, FSInfo at 1, backup at 6/7, `HiddSec` = partition start.
- exFAT: 1 FAT, `PartitionOffset` = partition start, NoFatChain-friendly contiguous system files, recorded FAT chains for the bitmap, up-case table and root.
- After the format, the wizard continues with LIB-3 folders, then LDR-3. On macOS it also writes `.metadata_never_index` and `.fseventsd/no_log` (optional, [U]).

### 6.2 Implementation per OS

| OS | Access | Partition table | Filesystem | Remount |
|---|---|---|---|---|
| Windows 10/11 | Elevated helper (UAC) on a secured named pipe | `IOCTL_DISK_CREATE_DISK` + `SET_DRIVE_LAYOUT_EX` + `UPDATE_PROPERTIES` (Rufus order) | Own engine, written through the locked new volume handle | Unlock and close; Windows mounts it |
| Linux (incl. Steam Deck, Flatpak) | Unprivileged helper + udisks2 `OpenDevice("rw")` (one polkit prompt); `root` fallback | Own engine (raw bytes); MBR last | Own engine | Close the fd (udev re-read); `Block.Rescan` if needed; mount through udisks |
| macOS | Unprivileged helper + `authopen` fd, after `diskutil unmountDisk` | Own engine | Own engine | `diskutil mountDisk` / DiskArbitration; **spike needed** [U] |
| Docker / headless server | Off | — | — | — |

A plain-udisks path (§2.2, no password, mkfs defaults) is worth keeping as an **optional Linux fallback**. It fits when exFAT "Default" is wanted and `mkfs.exfat` is installed, since that produces exactly the recommended exFAT layout.

### 6.3 Staged plan

| Stage | Contents |
|---|---|
| **v0.1 (M2–M3), DEV-1/DEV-2 "storage doctor", read-only** | Per-OS physical-disk enumerator (unelevated).<br>Readers for MBR, GPT, FAT32 and exFAT boot sectors.<br>A compatibility verdict per target profile: FS, table, entry index, sector size, cluster size, alignment, label, `._*` litter, legacy-boot warning, >2 TiB.<br>Short per-OS instructions as a fallback. No writes |
| **v0.2 (M5), DEV-3/DEV-4** | Pure-Rust engine (MBR + FAT32 + exFAT; FatFs `f_mkfs`-derived) with the CI matrix of §4.4 and the FatFs harness.<br>`yaoplm image format` for image files.<br>The full guard pipeline (§4.2).<br>Windows helper + Linux udisks/root.<br>MBR only (≤2 TiB).<br>Profiles: USB, MX4SIO, HDD-in-adapter.<br>macOS if the `authopen` spike passes, otherwise v0.3 |
| **v0.3 (M6)** | GPT for >2 TiB (`gptman`).<br>macOS (if not done).<br>MMCE profile with firmware rules.<br>Fake-capacity check (optional) |
| **later** | Server/Docker formatting with an explicit device allow-list.<br>SATA/NVMe-attached disks behind an "advanced" switch.<br>APA / APA-Jail (HDD-1) |

### 6.4 Decisions for an ADR (owner's call)

1. Own engine as the single implementation, or own FAT32 plus OS-native exFAT on Windows and macOS (exFAT patent question, §3.3).
2. The ADR-0013 wording: "elevated helper" becomes "broker-scoped device access, elevated only on Windows" (§4.3).
3. The typed-confirmation token: the size.
4. Server mode: no formatting until "later".

### 6.5 Console test checklist for the v0.2 formatter (for the owner)

1. A 16 GB stick formatted exFAT and a 64 GB stick formatted exFAT, with a CD game and a DVD game over 4 GiB. Check on OPL 1.2.0 latest, and in Neutrino/NHDDL if installed.
2. A 32 GB stick formatted FAT32 with a `ul` game. Check on OPL 1.2.0 and on OPL 1.1.0, and start OPL from that stick through FMCB or uLE.
3. An MX4SIO SDHC (≤32 GB) and SDXC (≥64 GB) card formatted exFAT.
4. An MMCE card (SD2PSX or MCP2) formatted exFAT. Check that the emulator still boots and keeps its settings.
5. An HDD (≥1 TB) in a USB adapter formatted exFAT, then installed in the PS2 (`enable_bdm_hdd`). Also check that a VMC works.
6. A stick formatted on Windows, then used on Linux and macOS PCs to add games. Check that the console still lists them, with no `._` problems.

---

## 7. Could not verify

- **Windows:**
  - whether `Format-Volume`, diskpart or VDS received the 2 TB FAT32 limit;
  - whether KB5083631 rolled out gradually;
  - a formal admin requirement for the Storage cmdlets;
  - the live KB140365 page (404; the 2019 archive was used);
  - diskpart `fs=exfat`.
- **macOS:**
  - `diskutil eraseDisk` on external disks without sudo, and what "ownership" means;
  - the MBR partition offset diskutil uses;
  - root for `/dev/rdisk`;
  - whether TCC applies to raw devices;
  - DiskArbitration re-probe after an `authopen` write;
  - current behaviour of `.metadata_never_index` and `.fseventsd/no_log`.
- **Linux:**
  - which udisks/libblockdev version first formatted exFAT;
  - the udev re-read after closing a udisks fd;
  - the flags `OpenDevice` accepts (`O_EXCL`);
  - whether distros changed `modify-device` after CVE-2025-6019.
- **PS2:**
  - why 512 KiB exFAT clusters fail;
  - the cause of OPL #1773;
  - whether the current `latest` OPL and Neutrino builds include ps2sdk `65f988c` (GPT gap) and `af3d64c` (partition-less disks);
  - iLink sector-size handling;
  - MMCE firmware behaviour for MemCard PRO 2 (closed);
  - whether 4Kn USB devices fail cleanly;
  - that stock FMCB 1.966 is FAT-only (inferred from its installer's optional exFAT IRX files);
  - the build dates of rev1880, rev1893 and rev1981.
- **Cluster sizes:** what SD Card Formatter actually writes on large SDXC cards. The SDA Part 2 table is known only from the `mkfs.exfat(8)` copy.
- **Legal:**
  - whether OIN or Microsoft exFAT commitments cover a GPL-3 userspace formatter on Windows or macOS;
  - the GPL version of Ridgecrop fat32format;
  - the exact US expiry dates of the LFN patents.
- **Not examined:** the effect of the GPT `sectorCount` off-by-one; the OPL per-part fragment check overflow (code reading only); whether Windows non-cached I/O alignment is satisfied by `hadris-storage` or `fatfs` on PhysicalDrive handles.

---

## Sources

**OPL, ps2sdk, FatFs, loaders**
- OPL README @`b3a2199` (USB section L102–119, HDD section L141–146): https://github.com/ps2homebrew/Open-PS2-Loader/blob/b3a2199/README.md ; history: commits `026b9f4` (#1584) and `9357948` (merged in #1677)
- OPL v1.1.0 README: https://github.com/ps2homebrew/Open-PS2-Loader/blob/v1.1.0/README.md ; wiki `usb-mode`: https://github.com/ps2homebrew/Open-PS2-Loader/wiki/usb-mode
- OPL source @`b3a2199`: `src/bdmsupport.c` (L364–381 VMC, L430–461 fragments), `src/supportbase.c` (`isValidIsoName`, `scanForISO`), `src/lang.c` (`lngReadEntry`), `src/opl.c` (`scanApps`), `src/themes.c`, `include/bdmsupport.h`, `modules/iopcore/common/cdvd_config.h`
- OPL issues and PRs: #662, #1098, #1197, #1271, #1295, #1311, #1493, #1584, #1597, #1666, #1677, #1695, #1773 (https://github.com/ps2homebrew/Open-PS2-Loader/issues/<n>)
- grimdoomer fork releases (v1.2.0.2-beta): https://github.com/grimdoomer/Open-PS2-Loader/releases
- ps2sdk @`329e2bb`: `iop/fs/bdm/src/{bdm.c,part_driver.c,part_driver_mbr.c,part_driver_gpt.c}`, `iop/fs/bdmfs_fatfs/src/fs_driver.c`, `mx4sio_bd`, `usbmass_bd/src/scsi.c`, `download_dependencies.sh`; commits `65f988c`, `af3d64c` (PR #751), PR #418: https://github.com/ps2dev/ps2sdk
- FatFs fork: https://github.com/fjtrujy/FatFs/tree/iop-r0.16 (`source/include/ffconf.h`, `source/ff.c`); FatFs docs: http://elm-chan.org/fsw/ff/doc/mkfs.html , http://elm-chan.org/fsw/ff/doc/appnote.html
- Neutrino: https://github.com/ps2max32/neutrino (README, `ee/loader/config/*.toml`); NHDDL: https://github.com/pcm720/nhddl (README, `src/devices/iso.c`)
- RiptOPL: https://github.com/NathanNeurotic/Open-PS2-Loader (README L309, L373–377, L594–601; `docs/EXFAT.md`)
- SD2PSX: https://github.com/sd2psx/sd2psx.github.io/blob/HEAD/docs/format-microsd.md ; sd2psXtd firmware: https://github.com/sd2psXtd/firmware ; MemCard PRO 2 manual (mirror): https://www.manualslib.com/manual/3396530/8bitmods-Memcard-Pro2.html
- FMCB installer: https://github.com/israpps/FreeMcBoot-Installer ; PS2BBL: https://github.com/israpps/PlayStation2-Basic-BootLoader ; wLaunchELF: https://github.com/ps2homebrew/wLaunchELF

**Specs and defaults**
- fatgen103: https://download.microsoft.com/download/1/6/1/161ba512-40e2-4cc9-843a-923143f3456c/fatgen103.doc
- exFAT specification: https://learn.microsoft.com/en-us/windows/win32/fileio/exfat-specification ; Microsoft OSS blog 2019-08-28: https://opensource.microsoft.com/blog/2019/08/28/exfat-linux-kernel/
- KB140365 (archived): https://web.archive.org/web/20190209091244/https://support.microsoft.com/en-us/help/140365/default-cluster-size-for-ntfs-fat-and-exfat
- exfatprogs: https://github.com/exfatprogs/exfatprogs (`lib/libexfat.c`, `mkfs/mkfs.c`); man page: https://man.archlinux.org/man/mkfs.exfat.8.en.txt
- dosfstools: https://github.com/dosfstools/dosfstools (`src/mkfs.fat.c`, `manpages/mkfs.fat.8.in`)
- SD Association: https://www.sdcard.org/downloads/formatter/ , https://www.sdcard.org/developers/sd-standard-overview/capacity-sd-sdhc-sdxc-sduc/

**Windows**
- `format`: https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/format ; `diskpart`: https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/diskpart , `create-partition-primary`, `clean`, `convert-mbr`
- Insider build 27686: https://blogs.windows.com/windows-insider/2024/08/15/announcing-windows-11-insider-preview-build-27686-canary-channel/ ; Release Preview 26100/26200.8313: https://blogs.windows.com/windows-insider/2026/04/17/releasing-windows-11-builds-26100-8313-and-26200-8313-to-the-release-preview-channel/ ; KB5083631: https://support.microsoft.com/help/5083631
- Storage cmdlets: https://learn.microsoft.com/en-us/powershell/module/storage/format-volume , `initialize-disk`, `clear-disk`, `new-partition`; MSFT_Disk: https://learn.microsoft.com/en-us/windows-hardware/drivers/storage/msft-disk
- CreateFileW: https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew ; FSCTL_LOCK_VOLUME / FSCTL_DISMOUNT_VOLUME (winioctl docs); STORAGE_BUS_TYPE: https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ne-winioctl-storage_bus_type ; SHELLEXECUTEINFOW: https://learn.microsoft.com/en-us/windows/win32/api/shellapi/ns-shellapi-shellexecuteinfow
- Removable-media policy: https://learn.microsoft.com/en-us/previous-versions/windows/it-pro/windows-10/security/threat-protection/security-policy-settings/devices-allowed-to-format-and-eject-removable-media
- Rufus: https://github.com/pbatard/rufus (`src/format.c`, `src/format_fat32.c`, `src/drive.c`, `src/dev.c`, `src/rufus.c`, `src/smart.c`, `res/loc/rufus.loc`, `src/rufus.manifest`); Ridgecrop: http://ridgecrop.co.uk/fat32format.htm , http://ridgecrop.co.uk/guiformat.htm

**Linux**
- udisks: https://github.com/storaged-project/udisks (`data/org.freedesktop.UDisks2.xml`, `data/org.freedesktop.UDisks2.policy.in`, `src/udiskslinuxblock.c`); libblockdev: https://github.com/storaged-project/libblockdev (`src/plugins/fs/vfat.c`, `exfat.c`)
- CVE-2025-8067: https://www.openwall.com/lists/oss-security/2025/08/28/1 ; CVE-2025-6019: https://blog.qualys.com/vulnerabilities-threat-research/2025/06/17/qualys-tru-uncovers-chained-lpe-suse-15-pam-to-full-root-via-libblockdev-udisks
- man7: `sfdisk(8)`, `parted(8)`, `wipefs(8)`, `blockdev(8)`, `partprobe(8)`, `lsblk(8)`: https://man7.org/linux/man-pages/
- GNOME Disks: https://gitlab.gnome.org/GNOME/gnome-disk-utility (`src/disks/gdu-format-disk-dialog.c`)
- GPL FAQ (aggregation): https://www.gnu.org/licenses/gpl-faq.html#MereAggregation
- Flatpak permissions: https://docs.flatpak.org/en/latest/sandbox-permissions.html ; Flathub requirements: https://docs.flathub.org/docs/for-app-authors/requirements ; Impression manifest: https://github.com/flathub/io.gitlab.adhami3310.Impression ; snapd interfaces: https://github.com/canonical/snapd/tree/master/interfaces/builtin ; Docker run: https://docs.docker.com/engine/containers/run/

**macOS**
- `diskutil(8)`: https://keith.github.io/xcode-man-pages/diskutil.8.html ; `newfs_exfat(8)`: https://keith.github.io/xcode-man-pages/newfs_exfat.8.html ; Apple msdosfs (`newfs_msdos`): https://github.com/apple-oss-distributions/msdosfs
- `diskutil info -plist` keys: https://github.com/aws/ec2-macos-utils (`internal/diskutil/types/disk.go`)
- SMJobBless / SMAppService / AuthorizationExecuteWithPrivileges: https://developer.apple.com/documentation/servicemanagement
- Rust std `fs::copy` on macOS: https://github.com/rust-lang/rust/blob/master/library/std/src/sys/fs/unix.rs

**Apps and crates**
- balenaEtcher: https://github.com/balena-io/etcher (`lib/shared/drive-constraints.ts`, `lib/shared/sudo/*.ts`, `lib/util/api.ts`); drivelist: https://github.com/balena-io-modules/drivelist
- Raspberry Pi Imager: https://github.com/raspberrypi/rpi-imager (`src/disk_formatter.cpp`, `src/mac/macfile.cpp`, `src/linux/platformquirks_linux.cpp`, `src/linux/file_operations_linux.cpp`, `src/drivelist/`, `src/windows/rpi-imager.manifest.in`)
- Ventoy: https://github.com/ventoy/Ventoy (`Ventoy2Disk/Ventoy2Disk/*.c`, `INSTALL/tool/VentoyWorker.sh`, `LinuxGUI/Ventoy2Disk/ventoy_gui.c`)
- Impression: https://gitlab.gnome.org/World/Impression ; Popsicle: https://github.com/pop-os/popsicle ; caligula: https://github.com/ifd3f/caligula
- PS2 tools: PSBBN https://github.com/CosmicScale/PSBBN-Definitive-Project (`scripts/PSBBN-Installer.sh`); HDL Batch Installer https://github.com/israpps/HDL-Batch-installer ; PS2 HDD Manager https://github.com/L10N37/PS2-HDD-Manager ; PFS BatchKit https://github.com/GDX-X/PFS-BatchKit-Manager ; pfsshell https://github.com/ps2homebrew/pfsshell ; OrbitPS2 https://github.com/Luden02/OrbitPS2-Manager ; OPL PC Tools https://github.com/brainstream/OPL-PC-Tools ; OPL Manager docs https://oplmanager.com/site/?docs
- Crates: https://crates.io/crates/fatfs , `fscommon`, `hadris-fat`, `hadris-part`, `exfat-fs`, `mbrman`, `gptman`, `gpt`, `sysinfo`, `rs-drivelist`, `udisks2`, `zbus`, `windows`, `runas`, `elevated-command` (each at https://crates.io/crates/<name>)
