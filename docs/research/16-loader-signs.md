# 16 - Loader signs on a device: which build a library is for

Date: 2026-10-24. Scope: milestone M5, loader-profile auto-detection (SPEC LIB-1, §5.1.1, ADR-0023) and loader recognition (LDR-4). Goal: every sign a PC can read on a USB stick, SD card or folder that tells which PS2 loader build the library is prepared for or was last used with, how strong each sign is, and what it proves.

Sources (local clones from [REFERENCES.md](REFERENCES.md), pulled the same day; nothing built, run or downloaded, no release binary opened):

| Build | Clone | HEAD |
|---|---|---|
| OPL upstream | `ps2homebrew__Open-PS2-Loader` | `b3a21997` (2026-10-12, `v1.2.0-Beta-2271`); tags `v1.0.0` `a0f71770`, `v1.1.0` `07beff6a` |
| YAOPL (owner's fork) | `C:\Users\Admin\Projects\Open-PS2-Loader` (read only, not fetched) | `33a2b19a` (2026-10-23, `v1.2.0-Beta-2370-33a2b19-yaopl`) |
| OPL MMCE fork | `ps2-mmce__Open-PS2-Loader`, branch `origin/mmce_support` | `e554796` (2025-11-24) |
| RiptOPL (C, "Legacy Edition") | `NathanNeurotic__Open-PS2-Loader`, `rebuild/main` | `55261de1` (2026-10-23) |
| RiptOPL (C++, "PS2Build Edition") | `NathanNeurotic__RiptOPL`, `rebuild/main` | `6edea174` (2026-10-20) |
| wOPL | `ps2homebrew__wOPL`, `wOPL-base` | `4ae74148` (2026-09-13) |
| OPL-Evolution | `officialjuicedesigns__OPL-Evolution`, `evo-development` | `47119b71` (2026-07-25) |
| NHDDL | `pcm720__nhddl`, `legacy` | `89141d47` (v1.2.2) |
| Neutrino | `ps2max32__neutrino` | `7be8de27` (2026-08-31) |

Release facts come from `gh api repos/<owner>/<repo>/releases` on 2026-10-24.

Tags: **[V]** verified in code, CI config or a GitHub API response (file or endpoint named); **[R]** read in a README, docs site or release text; **[D]** deduced from verified facts, not tested on a console; **[U]** unknown or unverified.

Licences: OPL, its forks, NHDDL and Neutrino are AFL-3.0. Nothing here is copied code; the tables restate behaviour (ADR-0002).

---

## 0. Key findings

1. **OPL writes a version sign on the game device itself: `CD/games.bin` and `DVD/games.bin`.** Every OPL-family build caches its game list there. The record is 92 bytes in 1.0.0, 1.1.0 and betas up to 2016 and 188 bytes from Beta-2017 (2023-07-15) and in every 1.2 fork. The file is rewritten whenever another generation lists the folder, so its record size says which generation last used the device, wherever the settings live (§1.5).
2. **Upstream 1.2 is a ladder, not one version.** By first `master` build:
   - Beta-1871 ZSO and `bdm_cache`;
   - Beta-1880 exFAT;
   - Beta-1895 64 shared fragments (before that, every file in one piece, as in 1.1.0);
   - Beta-2017 160-character titles (1.1.0 **hides** longer ones);
   - Beta-2169 PNG only (JPG and BMP were read until then);
   - Beta-2194 `enable_bdm_hdd`;
   - Beta-2207 VMC groups `XEBP_100.NN`;
   - Beta-2260 / 2265 `$ParentalLock`, `$CheatSel` (§1.2–§1.4).
3. **The absence of keys is the useful `conf_opl.cfg` sign.** OPL saves every key it knows on each save and keeps keys it does not know. A 1.2 key therefore proves only that some 1.2 build saved the file once. A file without the keys every beta from 1871 writes was never saved by one, which points to the 1.1.0 rules. The file is usually on the memory card, not on the device (§1.2, §1.5).
4. **`mmce_*` does not mean "MMCE fork".** YAOPL writes the same five keys since 2026-10-23 and always includes `mmce_gameid`; a release build of the MMCE fork never writes `mmce_gameid`. wOPL and RiptOPL keep their MMCE keys in their own files. YAOPL is under active development, so this split must be re-checked before it is used (§2, §3, §13).
5. **RiptOPL and wOPL keep their settings beside the ELF and never write `conf_opl.cfg`.** RiptOPL reads one once as a seed. Each also writes files nobody else does:
   - RiptOPL: `settings_riptopl.cfg`, `config.path`, `favourites.bin` with an `OFAV` header, `art_cache.bin` version 4, LF-only CFG files since 2026-09-07, the folder `POPS/`;
   - wOPL: `wopl_*.cfg` in libconfig syntax, `wopl_boot.cfg`, `config_errors.log`, the folder `IMG/`, libconfig per-game CFG (§4, §5).
6. **NHDDL is recognised by `nhddl/cache.bin` (magic `NIDC`) or `nhddl/lastTitle.bin`,** not by the folder `nhddl/` itself: the PSBBN installer creates that folder too. OPL-Evolution leaves nothing on the device apart from its ELF (§6, §7).
7. **Versions collide across forks.** Upstream rewrote its `master` history after v1.1.0, so the tags are not ancestors of today's `master`. YAOPL, the MMCE fork and RiptOPL each number their own commits. A build is identified by its hash, or by number + hash + suffix, never by the number alone (§1.1–§4).
8. **Hashes:**
   - 1.1.0 and 1.0.0 have no GitHub `digest`. Each is 17 ELFs inside 7z files: one packed and 16 unpacked variants (§1.6).
   - The unpacked variants, and every `NOT_PACKED` build, carry their version string in plain text.
   - Rolling RiptOPL hashes stay valid for a few hours only.
9. The proposed ranking, conflict rules and a device test matrix for the owner's tests-only stick are in §9–§11. §12 lists what differs from SPEC §5.1.1 and research/07. §13 has the owner's rule that every sign must be re-checked against current code.

---

## 1. Upstream OPL: a version ladder

### 1.1 Two histories and the beta number

- **Upstream `master` was rewritten after v1.1.0** [V]. The tags `v1.0.0` (`a0f71770`) and `v1.1.0` (`07beff6a`) are not ancestors of today's `master`, `git merge-base v1.1.0 HEAD` is empty, and GitHub agrees (`gh api …/compare/v1.1.0...master`: "No common ancestor between v1.1.0 and master"). The same commits exist in the new history under other hashes: "OPL v1.1.0-Beta" `1f2f266a` (= tree of `v1.0.0`) and "OPL v1.1.0" `5db29c2b` (= tree of `v1.1.0`). The only difference between the trees is `modules/usbhdfsd/` (6,172 lines), which the rewrite removed (`git diff --stat v1.1.0 5db29c2b`). When the rewrite happened is [U].
  - Consequence: the 7-character hash in the version string of an old beta (`v1.2.0-Beta-<N>-<hash>`) may name a commit that no longer exists upstream. Map a build by its number, not by its hash.
  - Consequence for research: `git log v1.1.0..HEAD` walks the whole history since 2009. Use `5db29c2b..HEAD`.
- **Beta number** [V `Makefile:54-65`]: `REVISION = git rev-list --count HEAD + 2`, version `v1.2.0-Beta-<REVISION>-<hash>[-<LOCALVERSION>]`. The rewrite kept the count: `5db29c2b` has 1,789 ancestors, like the old tag. A tagged build uses the tag instead (`v1.1.0`).
- A feature commit made on a pull-request branch gets its number only when merged. The tables below give the **first `master` build that contains the commit** (first-parent search: the earliest first-parent commit of `master` that has the commit as an ancestor, by `git merge-base --is-ancestor` and bisection; its `rev-list --count` plus 2). Builds between `v1.1.0` and that number lack the feature [V].

### 1.2 Global settings: keys by version

`conf_opl.cfg` keys come from `#define CONFIG_OPL_*` in `include/config.h`; `_saveConfig()` in `src/opl.c` sets **every** known key on each save, unconditionally [V `src/opl.c:1087-1145` at `b3a21997`; checked at each introducing commit that the key is saved there too]. `configWrite()` writes every key in memory, including keys the build does not know, and skips only `#` keys [V `src/config.c:540-563` at `5db29c2b` and `b3a21997`].

| Key (file) | 1.0.0 | 1.1.0 | First `master` build | Commit, date |
|---|---|---|---|---|
| `check_usb_frag` (`conf_opl.cfg`) | yes (default 1) | **no** | — | `c71e9163` "Remove option to check if usb games are fragmented and force the check" (2021-01-21) |
| `enable_mx4sio` (`conf_opl.cfg`) | no | yes | — | `897a8376` (2021-07-21), after the BDM rework `17c4a624` (2021-05-09) |
| `enable_ilink` (`conf_opl.cfg`) | no | yes | — | `d664c1d7` (2021-09-02) |
| `nbd_default_export` (`conf_network.cfg`) | no | yes | — | `5b82898a` (2021-09-05) |
| `bdm_cache`, `hdd_cache`, `smb_cache` | no | no | **Beta-1871** | `c34a072b` "Added ZSO code, sector cache…" (2022-05-26) |
| `enable_bgm`, `bgm_volume`, `default_bgm_path` | no | no | **Beta-1954** | `b571e599` via merge `be74f364` (2022-12-08) |
| `x_sensitivity`, `y_sensitivity` | no | no | **Beta-2129** | `e6ceb078` via merge `8c50a079` (2024-09-30) |
| `enable_bdm_hdd` | no | no | **Beta-2194** | `626e5e46` via merge `a626d98a` (2024-12-27) |

All other `conf_opl.cfg` and `conf_network.cfg` keys exist in 1.0.0, 1.1.0 and Beta-2271 alike [V: every `#define CONFIG_* "<key>"` in `include/` and every literal key passed to `config{Get,Set,Remove}*` in `src/`, extracted at `1f2f266a`, `5db29c2b` and `b3a21997` and compared; the method is in §13]. No key was renamed.

Consequences [D from the two [V] rules above]:
- A key proves that **some build at least that new saved the file once**. It does not prove that this build is the one in use: a 1.2 file opened and saved by 1.1.0 keeps its 1.2 keys.
- The **absence** of keys that a build always writes is the stronger sign. A `conf_opl.cfg` that has `usb_mode`, `enable_ilink` and `enable_mx4sio` but none of `bdm_cache`, `enable_bgm`, `x_sensitivity`, `enable_bdm_hdd` was never saved by any build from Beta-1871 on: its last writer is 1.1.0 or a beta 1792–1870.
- Builds 1792–1870 follow every 1.1.0 rule YAOPLM checks (no ZSO, no exFAT, one piece per file, JPG/BMP art, 64-character titles; §1.4). For the profile, "1.1.0 or an old beta" is simply the OPL 1.1.0 profile.
- `check_usb_frag` without `enable_ilink` → last saved by 1.0.0 (or an earlier beta). YAOPLM has no 1.0.0 profile; 1.0.0 follows the 1.1.0 rules and lacks MX4SIO and iLink [V `1f2f266a:src/usbsupport.c`, no `bdmsupport.c`].
- The newest key gives a lower bound for the newest writer: `enable_bdm_hdd` → ≥ Beta-2194, `x_sensitivity` → ≥ Beta-2129, `enable_bgm` → ≥ Beta-1954, `bdm_cache` → ≥ Beta-1871.

### 1.3 Per-game CFG: keys by version

Per-game files live **on the game device**, `<root or usb_prefix>/CFG/<ID>.cfg` (research/15 §1). Unlike `conf_opl.cfg` they are written only for the keys the user changed (research/15 §1.1), so a missing key proves nothing.

| Key | First `master` build | Commit, date |
|---|---|---|
| `$PadMacroSource`, `$PadMacroSettings` | Beta-1836 | `ab78f9d6` via `9fc5b821` (2022-03-14) |
| `$CustomLanguageSource`, `$CustomLanguageValue` | Beta-1920 | `230f3288` via `3e26d1f9` (2022-08-14) |
| `$CustomLanguageEnable` | Beta-1920, **removed in Beta-2046** | same; `9e12706e` (2023-08-14) |
| `$OSDSettingsEnable`, `$OSDAspectRatio`, `$OSDVideoMode` | Beta-2046 | `9e12706e` (2023-08-14) |
| `$ParentalLock` | Beta-2260 | `154fb8af` via `5eed6d77` (2026-10-05) |
| `$CheatSel<N>` | Beta-2265 | `7cef2479` (2026-10-05) |

All [V] (`include/config.h` history, `CONFIG_ITEM_*`). `$Compatibility` has bits 0x01–0x20 in every version (research/15 §1.2); bit 0x40 belongs to RiptOPL and wOPL, 0x80 to RiptOPL only (§4, §5). A per-game key is a sign about whoever edited that game, so it can come from another console or loader that shared the device.

### 1.4 Device and format rules by version

| Rule | 1.0.0 | 1.1.0 and Beta ≤ the number | From | Commit |
|---|---|---|---|---|
| USB driver | `usbhdfsd` | BDM `bdmfs_vfat` (FAT only) | exFAT via `bdmfs_fatfs`: **Beta-1880** | `4e1a3415` via `a361ba9e` (2022-06-08) |
| ZSO images | no | no | **Beta-1871** | `c34a072b` (2022-05-26) |
| Game file fragments (BDM) | one piece per file | one piece per file (`USBMASS_IOCTL_CHECK_CHAIN`) | 64 entries shared by the parts: **Beta-1895** | `8f04d009` via `4a1e95e3` (2022-06-15) |
| VMC on BDM | one piece | one piece | one piece (unchanged) | ADR-0037 |
| ISO title length | 64 | 64: longer names are **skipped** (`if (NameLen > ISO_GAME_NAME_MAX) continue;`) | 160: **Beta-2017** | `0d6f8844` (2023-07-15) |
| Art formats | PNG, JPG, BMP | PNG, JPG, BMP (`texDiscoverLoad` tries `png`, `jpg`, `bmp`) | PNG only: **Beta-2169** | `c5a12a0a` "Remove .bmp and .jpg images support." via `5a3ef8a8` (2024-10-31) |
| BDM HDD (ATA exFAT) | no | no | **Beta-2194** | `626e5e46` (2024-12-27) |
| VMC name suggested by the menu | `<ID>_<slot>` | `<ID>_<slot>` | `<group>_<slot>`, groups `XEBP_100.NN`: **Beta-2207** | `48da113d` (2025-06-04), `src/vmc_groups.c` |
| `$Compatibility` modes | 1–6 | 1–6 | 1–6 | research/15 §1.2 |

All [V] (the files named, read at the commits named and at their parents). What this corrects or adds:
- **"OPL 1.2 is PNG only" holds only from Beta-2169.** Betas 1792–2168 still load JPG and BMP. A JPG cover therefore points to "1.1.0 or a beta older than 2169", or simply to an old manager (§9).
- **The one-piece rule is not only 1.1.0's.** Betas before 1895 refuse a fragmented game the same way. With §1.2 this means a device whose last writer is ≤ Beta-1870 must be checked with the 1.1.0 rules anyway.
- New for LIB-4: **OPL 1.1.0 hides ISO files whose title part is longer than 64 characters** (`5db29c2b:src/supportbase.c:279`; old-style names count from position 12, new-style names without `.iso`). It is a warning for the 1.1.0 profile, and a library full of such names was prepared for ≥ Beta-2017.

### 1.5 What OPL writes on a device

| File | Written by | When | Notes |
|---|---|---|---|
| `CD/games.bin`, `DVD/games.bin` | every OPL-family build | each scan of that folder whose game list changed; removed when the last game is gone | Flat array of `base_game_info_t`, see below [V `src/supportbase.c:135-262`] |
| `CFG/<ID>.cfg` | every build | the user saves per-game settings | §1.3; research/15 |
| `VMC/<name>.bin` | every build | the user creates a card in the VMC menu | name `<ID>_<slot>` or, ≥ Beta-2207, `XEBP_100.NN_<slot>` |
| `conf_opl.cfg`, `conf_last.cfg`, `conf_apps.cfg`, `conf_network.cfg`, `conf_game.cfg` | every build | the user saves settings, and the config home is this device | the same five names in 1.0.0, 1.1.0 and Beta-2271 [V `src/config.c:23-27`] |
| `ul.cfg`, renamed images | every build | rename or delete in the menu (`enable_delete_rename=1`) | — |
| `CFG THM LNG ART VMC CHT APPS`, plus `CD DVD` | every build | each BDM rescan with the device enabled | `sbCreateFolders`, research/07 §3.1 |

**Where `conf_opl.cfg` is looked for** [V `src/opl.c` `checkLoadConfigBDM`, `tryAlternateDevice`; the same in 1.1.0 `5db29c2b:src/opl.c:715-805`]: the memory card `mc?:OPL/` first; then the boot device; then each BDM unit at `mass<N>:conf_opl.cfg` (the root: `gBDMPrefix` is still empty at this point); then the HDD. Only with no config anywhere and no memory card does OPL create one on `mass0:`. When the user saves, OPL writes to the folder the config came from; if that fails (a full card, the card pulled out), it tries the boot device, the memory card, BDM and the HDD in that order (`_saveConfig`, `trySaveAlternateDevice`, `src/opl.c:1054-1176`). On BDM it writes `mass<N>:<usb_prefix>/conf_opl.cfg` (`bdmFindPartition(…, 1)` uses the prefix by then) [V]. So:
- on a device used with a memory card that holds settings (the owner's FMCB setup), there is **no** `conf_opl.cfg`;
- with a non-empty `usb_prefix`, a copy saved to USB lands in `<prefix>/conf_opl.cfg`, which the next boot does not find at the root [D]. YAOPLM should look at the root and one level down.

**`games.bin` is the best upstream sign, because OPL itself writes it on the game device** [V]:
- Record layout `{ char name[ISO_GAME_NAME_MAX+1]; char startup[13]; char extension[5]; u8 parts, media, format; u32 sizeMB; }`, EE alignment 4.
  - `ISO_GAME_NAME_MAX` = 64 in 1.0.0, 1.1.0 and up to Beta-2016 → **92-byte records** (name 0–64, startup 65–77, extension 78–82, parts 83, media 84, format 85, size 88–91).
  - 160 from Beta-2017 (`0d6f8844`) → **188-byte records** (name 0–160, startup 161–173, extension 174–178, parts 179, media 180, format 181, size 184–187).
  - Every 1.2 fork read for this report keeps 160 (MMCE fork `include/supportbase.h`, RiptOPL C, wOPL `include/iosupport.h`, OPL-Evolution, YAOPL).
- The record is a `memcpy` of a zeroed and filled struct, so a valid record has a NUL-terminated name, an ID in `startup`, and `.iso`/`.zso` (any case) at the extension offset. A file size divisible by both 92 and 188 (multiples of 4,324) is decided by where the extension sits.
- A build that finds records of the other size reads garbage, sees a changed list and rewrites the file in its own size [D from `loadISOGameListCache`/`updateISOGameList`]. So the record size tells **which generation of OPL last listed that folder**: 92 → 1.1.0 (or ≤ Beta-2016, or 1.0.0), 188 → ≥ Beta-2017 or a fork. An `extension` of `.zso` proves ≥ Beta-1871 too.
- YAOPLM deletes `games.bin` after changing `CD`/`DVD` (CLAUDE.md rule 2), so after our own writes the sign is gone until the console scans again.

### 1.6 ELF, packaging and hashes

| Release | ELF in the release | Packed? | Version string readable? | SHA-256 from GitHub |
|---|---|---|---|---|
| `v1.0.0` (2021-01-06) | inside `OPNPS2LD-v1.0.0.7z` (= `OPNPS2LD.7z`, 1,174,960 B) as `OPNPS2LD-v1.0.0.ELF`; `OPNPS2LD-VARIANTS-v1.0.0.7z` (2,359,387 B) has 16 variants `OPNPS2LD-v1.0.0-<DTL_T10000>-<IGS>-<PADEMU>-<RTL>.ELF` | main: yes (`ps2-packer`); variants: no (`NOT_PACKED=1`) | main: no; variants: yes [D] | `digest: null` on every asset |
| `v1.1.0` (2021-09-09) | the same layout: `OPNPS2LD-v1.1.0.7z` (= `OPNPS2LD.7z`, 1,238,940 B), `OPNPS2LD-VARIANTS-v1.1.0.7z` (1,473,070 B), `OPNPS2LD-LANGUAGES-AND-FONTS-v1.1.0.7z` | the same | the same | `digest: null`; no bare ELF asset |
| `latest` (re-published, now `v1.2.0-Beta-2271-b3a2199`) | bare `OPNPS2LD.ELF` (1,361,812 B) = the member of `OPNPS2LD.7z`; `OPNPS2LD-VARIANTS*.7z` with `OPNPS2LD-<ver>-PADEMU=<n>-EXTRA_FEATURES=<n>.ELF` (n = 0 or 1) | main yes, variants no | main no, variants yes [D] | `sha256:` on every asset |

Sources: [V] `gh api …/releases`; `Makefile` (`EE_BIN_PACKED`, `ps2-packer $< $@`, `$(EE_VPKD).ELF: … cp -f`); `.github/workflows/compilation.yml` at `1f2f266a`, `5db29c2b` (variant matrix, `NOT_PACKED=1`, `mv opl.elf OPNPS2LD-<ver>-<t10k>-<igs>-<pademu>-<rtl>.ELF`, empty matrix values leave empty fields). `src/gui.c:207` formats `"Open PS2 Loader %s"` with `OPL_VERSION`, so an unpacked ELF carries the version in plain text [D]; that the packed one does not is [R] research/07 §1.9.
- **An exact SHA-256 table for 1.1.0 needs 17 ELFs** (the packed main ELF and 16 variants), and 17 more for 1.0.0 if wanted. GitHub has no digest for any of them, so the table has to be built by downloading the two 7z files once and hashing their members. Users rename the ELF (`OPNPS2LD.ELF`, `OPL.ELF`), which does not change the hash.
- **Old 1.2 betas are not on GitHub** (research/07 §1.11). Their hashes exist only in our own history (LDR-2) or in the MEGA folder the README mentions. A beta with an unknown hash can still be dated by the device signs of §1.2–§1.5.
- **Unpacked ELFs give the exact version** [D]: variants, debug builds and self-built `NOT_PACKED=1` ELFs contain `v1.1.0` or `v1.2.0-Beta-<N>-<hash>[-<suffix>]` in plain text. Searching the file for that pattern is cheap and needs no unpacker. A packed ELF needs an LZMA unpacker for ps2-packer's format, which nobody here has examined [U].
- `LNG/` from the language archive: `lng_pack.sh` packs each language as `<Lang>-<OPL_VERSION>/lang_<Lang>.lng` (+ `font_<Lang>.ttf`). OPL reads only files directly in `LNG/` (`src/lang.c:104`, directories skipped), but a user who copied the folders leaves the version string in the folder name [V script; the copying is D]. Weak hint only.

---

## 2. YAOPL (`belomaxorka/yaopl`)

- **Same files as upstream** [V `src/config.c:22-27`, `src/opl.c` at `33a2b19a`]: `conf_opl.cfg` and the other four, looked for and saved exactly like upstream (§1.5). Per-game CFG, `games.bin` (188-byte records), VMC groups and art (PNG only) are upstream's.
- **The only device-side difference is MMCE** (FORK.md, commit `1046d006` "mmce: the MMCE device: game list, settings and launch", 2026-10-23, YAOPL Beta-2367) [V]:
  - keys `mmce_mode`, `mmce_slot`, `mmce_prefix`, `mmceigr_slot`, `mmce_gameid` in `conf_opl.cfg`, all five saved unconditionally (`src/opl.c:1130-1142`);
  - `MMCE_MODE` = 8 in `enum IO_MODES`, so `default_device=8` means MMCE (YAOPL `src/opl.c:1200`; upstream has no mode 8 and clamps anything above `APP_MODE` = 7, `b3a21997:src/opl.c:1181`);
  - games on the MMCE card under `mmce<N>:/<mmce_prefix>` with the usual folders and `games.bin`.
  - Before Beta-2367 YAOPL leaves no trace upstream does not.
  - **Volatile:** YAOPL is in active development (owner, 2026-10-24), and its MMCE code is a day old. Do not build detection on these details without re-checking them (§13).
- **Version string** `v1.2.0-Beta-<N>-<hash>-yaopl` (`LOCALVERSION ?= yaopl`, `Makefile:56`), plus `-nosio2man` for the `MMCE_NO_SDK_SIO2MAN=1` A/B build (`Makefile:57-59`) [V]. `<N>` counts YAOPL's own commits (2,368 at HEAD vs upstream's 2,269), so **YAOPL and upstream reuse the same numbers for different code**; only the suffix or the hash tells them apart.
- **Release** [V gh api]: one pre-release `latest` (`v1.2.0-Beta-2370-33a2b19-yaopl`, 2026-10-23) with `OPNPS2LD.ELF`, the three versioned 7z files and `SHA256SUMS`; every asset has a `digest`. The CI writes `SHA256SUMS` (`.github/workflows/compilation.yml:230`). The append-only `HASHES.txt` that SPEC §5.10.1 expects from the fork's CI does **not exist yet** [V: no such file or CI step at `33a2b19a`].
- The ELF is packed like upstream's; the VARIANTS ELFs are unpacked and carry `-yaopl` in plain text [D, same Makefile rules].

## 3. OPL MMCE fork (`ps2-mmce/Open-PS2-Loader`, branch `mmce_support`)

- **Base** [V `git merge-base`]: the released `OPL-MMCE-beta-2` (`bb415d2`) sits on upstream `a626d98` (Beta-2194); the branch head `e554796` (2025-11-24, no release) last merged upstream `b491c1c` (2025-10-27, Beta-2218). Same rewritten history as upstream.
- **Files**: the same five `conf_*.cfg` names and the same search order as upstream; **no config location on MMCE** [V `src/opl.c` @`e554796`]. On the SD card it creates `CFG THM LNG ART VMC CHT APPS CD DVD` under `mmce<N>:/<mmce_prefix>` with upstream's `sbCreateFolders` and reads the usual layout; nothing on the card is unique to it [V `src/mmcesupport.c`].
- **Keys** [V `include/config.h`, `src/opl.c`; the subagent's `keys.py` run over the three revisions]:

  | Key | beta-1 (≈ `5962294`, D) | beta-2 `bb415d2` | head `e554796` |
  |---|---|---|---|
  | `mmce_mode`, `mmce_slot` | yes | yes | yes |
  | `mmce_prefix`, `mmceigr_slot` | — | yes | yes |
  | `mmce_wait_cylces` (sic) | yes | yes | — (removed in `7e5995e`) |
  | `mmce_use_alarms` | — | yes | — |
  | `mmce_gameid` | debug builds only | debug only | debug only (`#ifdef __DEBUG` around the get and the set, `src/opl.c:952-954`, `1116-1118`) |

  So a release build of the MMCE fork **never writes `mmce_gameid`**, while YAOPL always does. `default_device=8` is MMCE here as in YAOPL.
- **Art** PNG only (base ≥ Beta-2169) [V `src/textures.c`]. VMC on MMCE: `<prefix>VMC/<name>.bin`, pages through `mmcedrv`, no fragment limit [V `modules/mcemu/device-mmce.c`]. Multi-part `ul` on MMCE opens only part 0 [D].
- **Version string** `v1.2.0-Beta-<N>-<hash>` without a suffix [V Makefile]: a build of `e554796` would say `Beta-2246`, a number that upstream's own Beta-2246 also uses for other code [D].
- **Releases** [V gh api]: `mmce-beta1` (`OPNPS2LD-MMCE-b1.ELF`, 2024-12-23) and `OPL-MMCE-beta-2` (`OPNPS2LD.ELF`, 2025-01-15); `digest: null` on both. Exact hashes need a one-time download.

## 4. RiptOPL (`NathanNeurotic/Open-PS2-Loader` and `NathanNeurotic/RiptOPL`)

Two repos, one device layout: the C "Legacy Edition" (`rebuild/main` @`55261de1`) and the C++ "PS2Build Edition" (@`6edea174`). Paths below are the C repo's unless marked; the facts were read by a subagent and the key ones spot-checked [V].

**Lineage and numbers.** `rebuild/main` restarted from upstream in August 2026: its parent of `41df9cdf` ("rebuild-01: identity & doctrine (branding, settings_riptopl.cfg, USB opt-in…)", 2026-08-07, RiptOPL Beta-2247) still has upstream's `conf_opl.cfg` names [V `git show 41df9cdf^:src/config.c`]. Since then about 2,000 commits: HEAD is Beta-4257. The older lineage (release `current-fan-favorite`, Beta-3390, 2026-09-04) is not in the clone, and which settings name it wrote is [U] (research/07 calls `conf_riptopl.cfg` the legacy name). RiptOPL's numbers count its own commits and say nothing about upstream.

| File | Where | R/W | Source |
|---|---|---|---|
| `settings_riptopl.cfg` | **the ELF's folder** (argv[0], else CWD); save fallback: boot device → `mc?:OPL/` → `mmce0:/`, `mmce1:/` root → `mass<N>:` root → HDD | RW | `include/config.h:28`; `src/opl.c:5792`, `3838-3895` [V]; since `41df9cdf` |
| `conf_riptopl.cfg` | same folder | R; the next save writes the new name | `include/config.h:29`; `src/config.c:1371-1384` [V] |
| `conf_opl.cfg` | same folder, HDD home | **read once as a seed, never written** (`default_device` 5–7 remapped) | `include/config.h:30-32`; `src/config.c:1394-1420` [V] |
| `conf_last.cfg`, `conf_apps.cfg`, `conf_network.cfg`, `conf_game.cfg` | settings folder | RW, **upstream's names**; `conf_network.cfg` gains `http_ip`, `http_port`, `http_base_path`, `conf_last.cfg` gains `lib_view_retained`, `lib_view_mixed` | `src/config.c:26-32`; `src/opl.c:4163-4166`; `src/libview.c:341-344` [V] |
| `wopl_network.cfg`, `wopl_global_game.cfg`, `wopl_theme.cfg` | home, theme | read when the `conf_*` file is missing, saved back in libconfig | `src/config.c:1205-1240`, `1349-1361` [V] |
| `config.path` | `<ELF folder>/config.path`; mirror `mc<N>:/OPL/config.path` on network boots | RW, one line; overrides every other search | `src/opl.c:1969-2110`, `2690-2710` [V] |
| `favourites.bin` (+ `.new`) | beside the settings file | RW; header `OFAV` (`0x4641464F`) + u16 version 3. wOPL's file has no header | `include/favsupport.h:9`; `src/favsupport.c:66-85` [V] |
| `CD/games.bin`, `DVD/games.bin`, and `CD/<sub>/games.bin` with `folder_nav=1` | game device | RW, upstream's 188-byte record | `src/supportbase.c:164-261`, `545-558` [V] |
| `ART/art_cache.bin`, `CHT/cht_cache.bin` | beside `art.tar`, `cht.tar` | W; header `ARC\0`, **version 4** (wOPL: 3) | `src/tar.c:37-60`; `include/tar.h:14-28` [V] |
| `POPS/IPCONFIG.DAT`, `POPS/UDPFSCONFIG.DAT`, `SMBCONFIG.DAT` | device | W | `src/vcdsupport.c:2137-2160`, `2368-2391` [V] |
| `EMBER/settings.txt`, `EMBER/games/<name>/settings.txt` | device | W (own keys only) | `src/cuesupport.c:697` [V] |
| `neutrino/config/riptopl-cheats.toml` ("# Written by RiptOPL…", C only) | Neutrino folder | W | `src/system.c:1660-1767` [V] |
| `VMC/<name>.bak`, `.new` (VMC Rebuild, C only) | device | W | `src/vmcrebuild.c:8-58` [V] |
| RA builds: `RA/`, `RA/<serial>.wl`, `RA/<key>.md5`, `RA/hashes.txt`, `POPS/MODULE_9.IRX` | device | W | `src/rahash.c:803-830`, `src/rapopslaunch.c:43-85` [V] |
| memory card: `mc?:OPL/icon.sys`, `list.icn`, `copy.icn`, `del.icn` (upstream: `opl.icn`); `mc?:/POPSTARTER/bdma_config.txt` | MC | W | `src/util.c:176-238`; `src/vcdsupport.c:1134-1152` [V] |

- **Folders** created on enabled devices: upstream's set **plus `POPS`** (and `RA` in RA builds) (`src/supportbase.c:1373-1390`) [V]. The install kit adds `POPS/POPSTARTER VERSIONS/…`, `POPS/usbd.irx.<ata|ilink|mmce|mx4sio|udpfs|usbexfat>`, `EMBER/LICENSE-BETA.txt`, `neutrino/config/bsd-udpfsbd.toml` (not in upstream Neutrino), root shortcuts `PS2-Servers.url`, `OrbitPS2-Manager.url`, … and `ART/RIPTOPL.ELF_*.png` (`.github/scripts/package_release_app.py:296-330`) [V].
- **Line endings: LF** for every file RiptOPL writes since `4dee9fdf` (2026-09-07, RiptOPL Beta-2819; `src/config.c:1531` writes `"%s=%s\n"`); upstream and OrbitPS2 write CRLF [V]. A per-game CFG saved by RiptOPL therefore differs byte-wise from upstream's.
- **Global keys**: none of upstream's is dropped or renamed. Unique to RiptOPL (not in upstream, the MMCE fork, wOPL or YAOPL) [V `include/config.h:115-245`]: `enable_dvr`, `enable_pccf`, `enable_udpbd`, `udpfs_access`, `net_boot_protocol`, `network_protocol`, `net_start_mode`, `net_protocol_pick`, `smb_dialect`, `custom_settings_path`, `folder_nav`, `apply_gameid`, `mmce_wait_cycles` (spelled right), `mmce_pacing_migrated`, `art_delay`, `enable_bgart`, `enable_logoart`, `enable_save_icons`, `enable_art_tar`, `enable_analog_nav`, `enable_rumble`, `font_size`, `default_game_view`, `apps_display`, `game_type_labels`, `cfg_titles_in_lists`, `title_color`, `default_core`, `neutrino_*`, `ember_*`, `popstarter_*`, `bdma_*`, `vcd_*`, `auto_create_vmc`; RA builds `ra_*`. Shared with old wOPL: `enable_usb`, `fav_mode`, `mmce_use_alarms`, `enable_discart`, `coverflow_*`, `plasma_blend_color`. `default_device` numbering differs: 0–7 BDM, 8 ETH, 9 HDD, 10 APP, 11 MMCE, 12 FAV, 13 UDPFS, 14 HTTP (`include/iosupport.h:10-27`).
- **Per-game keys only RiptOPL writes**: `$NeutrinoArgs`, `$NeutrinoVideo`, `$NeutrinoGsmComp`, `$NeutrinoBsdfs`, `$EmberTiming`, `$EmberDither`, `$EmberShading`, `$EmberController`, `$VMCDisable_0`/`_1`; `$GSMVMode=29` (1080p; upstream stops at 28); `$Compatibility` **0x80 = Slow Reads** (`include/iosupport.h:103`). `$CoreLoader`, `$EnableImage` and bit 0x40 are shared with wOPL. PS1 CFG and art are keyed by the VCD stem or Ember folder (`src/supportbase.c:1183-1224`) [V].
- Formats: PNG only, `.iso`/`.zso`, exFAT, 64 fragments shared (`modules/iopcore/common/cdvd_config.h:91-92`), VMC one piece [V; R docs].
- **C++ edition**: the same file names, LF, `OFAV` v3, `ARC` v4, folders and `config.path` (`src/core/config.cpp:30-34`, `settingsio.cpp:238`). It lacks `enable_dvr`, `enable_pccf`, `enable_save_icons`, `font_size`, `neutrino_show_args`, `udpfs_access` and the RA extras, which the C edition always writes, so a saved file without them points to the C++ edition [D]. Its `title.cfg` says `PS2BUILD` (below).
- **Releases** [V gh api]: C `rolling` and `rolling-korium` (`RIPTOPL.ELF`, `APP_RIPTOPL.psu`, `RIPTOPL-v1.2.0-Beta-4257[-Korium].zip`, `-src`, `-LANGS`, `-VARIANTS`, `-DEBUG`), `rolling-ra` (`RIPTOPL-RA-{inceBtion,Korium}.ELF`, `SHA256SUMS.txt`, `RA-RELEASE.json`), `current-fan-favorite` (Beta-3390 zips). C++ `rolling-korium`, `rolling-ra`, `rolling-incebtion` with `BUILD-MANIFEST.json` and `SHA256SUMS.txt`. Every asset has a `digest`; the C `rolling` set ships no `SHA256SUMS` on purpose. Rolling assets are replaced several times a day, so a hash identifies one build for a few hours: our own history (LDR-2) is the only way to recognise older ones.
- **Version string** `v1.2.0-Beta-<N>[-Korium]-<hash>[-dirty]-<FLAVOUR>` (C `Makefile:1-4,117-137`); C++ adds `LOCALVERSION=PS2BUILD-<Edition>-GCC`. **Neither edition writes its version on the device**; the C ELF is packed with ps2-packer (`Makefile:594-596`), the C++ packer is [U].
- **`title.cfg`** in `APPS/APP_RIPTOPL/` (and the flavour folders `APP_RIPTOPL-PS2DEVROLLING`, `-PS2DEVPINNED`, `-OFFICIALPINNED`, `-OFFICIALROLLING`, `APP_RIPTOPL-RA/`): keys `title`, `boot`, `Title`, `CfgVersion=8`, `$ConfigSource=1`, `Version`, `Package`, `Release`, `Developer`, `Genre`, `Description`, …; C: `title=[APP] Open PS2 Loader: RiptOPL`, `Version=<ver> <FLAVOUR>`, `Release=Rolling|Tagged`; C++: `title=RiptOPL <Edition>`, flavour `PS2BUILD`, `Description=C++ ps2build migration; <Edition>` [V `Example/…/title.cfg`, `package_release_app.py:113-246`, C++ `tools/package_edition.py:41-77`].
- **False positives**: OrbitPS2 Manager copies RiptOPL's PS1 layout (VCD-stem and Ember-folder art, `EMBER/games/`, `POPS/*.VCD`) but writes no `settings_riptopl.cfg`, `OFAV` or `ARC` file [V `library.service.ts:161-166`]. POPStarter tools create `POPS/` and `IPCONFIG.DAT`; `neutrino/` is generic.

## 5. wOPL (`ps2homebrew/wOPL`)

wOPL continues KrahJohlito's uOPL. Its settings moved three times, so the file names date the build [V, files and commits named; read by a subagent, spot-checked]:

| Range | Global settings | Where | Syntax |
|---|---|---|---|
| uOPL (≤ 2025-02, release `beta`, `0daf81f`) | `conf_uopl.cfg` + `conf_last.cfg`, `conf_apps.cfg`, `conf_network.cfg`, `conf_game.cfg` | `mc?:OPL/`, then devices; APA `+OPL` | key=value (`src/config.c`, `Makefile:119` @`0daf81f`) |
| `d299492`/`a9e1591` (2026-02) → v1.0 (`0cc657e`), v1.1 (`8742426`) | `conf_wopl.cfg` + the four upstream-named `conf_*.cfg` | `mc?:wOPL/`, boot device, `mass<N>:[usb_prefix/]`, HDD; APA `__common/wOPL/conf_hdd.cfg`, `+wOPL` | key=value, CRLF (`src/config.c:36-41` @`8742426`) |
| `08b93cf` (2026-05-16) → the migration | the same | folder `wOPL_<VERSION>_<SUBVERSION>`, now `wOPL_1_2`: `mc?:wOPL_1_2/`, `+wOPL_1_2`, `__common/wOPL_1_2/conf_hdd.cfg` | key=value (`Makefile:237`, `src/hddsupport.c:157` @`08b93cf`) |
| `03758fb` (2026-06-20, PR #286) → `09da627` | `wopl_settings.cfg`, `wopl_network.cfg`, `wopl_global_game.cfg`, `wopl_last_played.cfg` | MC `wOPL_1_2/` first, then boot device, BDM, HDD | **libconfig** (`src/config_wopl.c` @`03758fb`) |
| `09da627` (2026-06-22) → HEAD, release `latest` "v1.2-beta-468" | the above + **`wopl_boot.cfg`** (`boot : { config_dir = "…"; };`, rewritten on every save) | **the ELF's folder**; `wopl_boot.cfg` may redirect; the memory card only as a fallback | libconfig (`src/config_wopl.c:47-58`, `453-646`; `src/pathsupport.c:166` @`4ae7414`) |

- **wOPL never reads or writes `conf_opl.cfg`** [V: no such string in `src/` at `4ae7414`]. Before the migration its `conf_last/apps/network/game.cfg` have upstream's names; at a shared root they collide with upstream's files. The HEAD migration renames its own `conf_wopl.cfg`, `conf_network.cfg`, `conf_game.cfg` to `.bak` in its config folder (`config_wopl.c:986-993`, `1102-1108`, `1316-1321`), which takes upstream's files away if both use the same folder [D].
- **Other files only wOPL writes** [V `@4ae7414`]: `config_errors.log` (libconfig parse errors, `config_wopl.c:106`); `CFG/<ID>.info` and libconfig `CFG/<ID>.cfg` (only through the menu's batch converter, `config_migration.c:655-820`, which by default deletes the originals); `APPS/*/title.cfg.bak` with a libconfig `title.cfg` (only builds `03758fb`..`d94a80d`, 2026-06-20..07-22); `THM/thm_*/wopl_theme.cfg`; the folder **`IMG/`**, created with the others since `ddc0f2c` (2026-05-14, `src/supportbase.c:1393`; no other build creates it); `wopllog.txt` on `mass0:` in debug builds.
- **Shared with RiptOPL** (the uOPL family): `favourites.bin`, `ART/art.tar` with `ART/art_cache.bin` (magic `ARC\0`; also `CHT/cht.tar`, `THM/thm.tar`), `$CoreLoader`, `$EnableImage`, `$Compatibility` bit 0x40 = mode 7 (`include/iosupport.h:85`; 0x80 unused) [V].
- **Telling libconfig from key=value** [V writers; output shape D from libconfig's defaults]: wOPL writes `key = value;` and `name : {`…`};` with LF; upstream and old wOPL write `%s=%s\r\n` with no spaces and no `;` (upstream `src/config.c:552`, wOPL `config_migration.c:1285`). A key followed by ` = ` or ` : ` and a line ending in `;` (or a next line `{`) is libconfig; RiptOPL's C++ edition tests the same way (`src/core/config.cpp:81-97` @`6edea174`). Do not rely on line endings.
- **Global keys**: libconfig groups `display`, `ui`, `audio`, `startup`, `devices`, `paths`, `mmce`, `debug`, `coverflow`, `frames_delay` (`config_wopl.c:823-912`). Old key=value keys unique to wOPL: `bdm_debug`, `enable_mmce` (v1.1+), `use_bg_main` (v1.0–1.1), `enable_archivedart` (v1.0), `coverflow_*` (2026-06-03..06-20), `mmce_wait_cylces` (sic, like the MMCE fork's betas). It has the five MMCE keys of §2/§3 too, and in its numbering `default_device=8` is Favourites (MMCE is 9) [V, keys.py over `0cc657e`, `8742426`, `4ae7414`].
- Formats: PNG only (plus grey PNGs), ZSO, exFAT, 64 shared fragments, VMC one piece — as upstream [V `src/textures.c:607`, `cdvd_config.h:65`, `bdmsupport.c:546-573`].
- **Releases** [V gh api]: v1.0, v1.1 and `latest` each ship `WOPNPS2LD.ELF` (1,531,316 / 1,534,148 / 1,538,308 B), `WOPNPS2LD.7z`, `-VARIANTS.7z`, `-LANGS.7z`, `-DEBUG.7z`; every asset has a `digest`, there is no `SHA256SUMS`; `latest` is replaced on every build. The uOPL `beta` release has `digest: null`. Version `v<V>.<S>[-<EXTRA>]-<rev-list+2>` without a hash (`Makefile:73-83`), e.g. `v1.2-beta-468`; the ELF is packed (`Makefile:401-409`).
- When wOPL runs from a memory card, its settings live there, and a stick holds only games, `IMG/` and the shared folders [D].

## 6. OPL-Evolution (`officialjuicedesigns/OPL-Evolution`)

- Upstream `3e3f34e9` (2026-06-07, Beta-2245) plus two commits (`2a4d113` "Publish OPL Evolution v0.27.0 alpha", `47119b7`) [V `git log`].
- **Upstream files and keys, unchanged**: `src/config.c` untouched, settings saved to `conf_opl.cfg` through `saveConfig(CONFIG_OPL)` (`src/evo_settings.c:190-200`); per-game VMCs through upstream's `$VMC_0`/`$VMC_1`, names `generic_<n>` or `<group or ID>_<slot>`, sizes 8–64 MB (`src/evo_engine.c:251-267`, `src/evo_ui.c:968-992`). PS1 support only reads `POPS/*.VCD`, `POPS/POPSTARTER.ELF` and art [V].
- **Nothing on the device is unique to it except the ELF.** Release v0.27.0-alpha: `OPL-EVOLUTION-0.27.0-NATIVE-VMC.ELF` (1,778,660 B, `digest` present), `CHECKSUMS.txt` in BSD form `SHA256 (name) = HEX`, `README-DEPLOYMENT.txt` (FAT32 USB, start from `mass:/`) [V gh api; R readme]. `OPL_EVO_VERSION "0.27.0-alpha"` (`include/evo_ui.h:10`), built by `make evolution` into a packed `OPL-EVOLUTION.ELF` (`Makefile:132,259,397`) [V].
- Detection: hash or ELF name; otherwise it looks like upstream Beta-2245 (its base) and gets the 1.2.0 beta profile, which is right [D].

## 7. NHDDL and Neutrino

NHDDL (`pcm720/nhddl`) is a launcher; Neutrino (`ps2max32/neutrino`, formerly `rickgaiser/neutrino`) is the loader core it starts. Both are ISO-only on the console.

| Path on the device | R/W | Versions | Source |
|---|---|---|---|
| `nhddl/` (lower case), made on the first write | W | v0.3.0 and later | `src/options.c`, `src/devices/iso.c` @`89141d4`; `BASE_CONFIG_PATH "/nhddl"` @`f235bc2` [V] |
| `config/` with `lastTitle.txt`, `global.yaml` (the earlier name) | R/W | v0.1.1–v0.2.0 | `src/options.c` @`b2e06ba`, @`2410fc3` [V] |
| `nhddl/cache.bin`: magic `NIDC`, `u8` version (2 since v1.0.0), count, then per title `char id[12]`, a path length and the path without the mount point | W | v0.3.0 (version 1), v1.0.0+ (version 2) | `src/devices/iso.c:252-297` @`89141d4` [V]; padding [D] |
| `nhddl/lastTitle.bin`: packed RTC time, then the relative path; rewritten on every launch | W | v1.0.0+ (`lastTitle.txt` before) | `src/options.c:23` [V] |
| `nhddl/global.yaml`, `nhddl/<ISO name>.yaml` (`key: value`, LF, `$key` = disabled; keys `gc`, `gsm`, `logo`, `dbc`, any other is passed to Neutrino as `-key=value`) | R / R+W | all | `src/options.c`, `src/ui/args.c` [V] |
| `nhddl/nhddl.yaml` | R | v0.3.0+ | `main.c` [V]; v1.2.2 reads only the launch folder and `/nhddl/nhddl.yaml` (the README at that commit still lists older paths) |
| `ART/<ID>_COV.png`, PNG only | R | v0.2.0+ (v0.1.1 used `_COV.jpg`) | `src/ui/gui.c` [V] |
| `neutrino/` with `neutrino.elf`, `version.txt` (v1.6.0+), `config/*.toml`, `modules/*.irx`, `ee_core.elf` | R | — | Neutrino `Makefile` @`7be8de2` [V]; flat layout without `config/system.toml` since v1.8.0 [V `ee/loader/src/main.c`] |

- NHDDL reads **no** OPL file: no `conf_opl.cfg`, `CFG/<ID>.cfg`, `CHT`, `ul.cfg` or ZSO; the scan takes only `.iso`/`.ISO` and skips `nhddl`, `neutrino`, `APPS`, `ART`, `CFG`, `CHT`, `LNG`, `THM`, `VMC`, `XEBPLUS`, `MemoryCards`, `bbnl` and names starting with `.`, `$`, `?` (`iso.c` @`89141d4`) [V]. Neutrino writes nothing: VMC and HDD images are opened without `O_CREAT` (`main.c:910`) [V]. Fragments: Neutrino shares 64 entries between the image and both VMCs (ADR-0037).
- Mode from the ELF name: `nhddl-<mode>.elf` with `ata|m4s|udpfs|usb|ilink|mmce|hdl` [V `parseFilename`]. The ELF is packed (`nhddl_unc.elf` → `nhddl.elf` via ps2-packer) [V CMakeLists]. Version = `git describe --tags`.
- Releases [V gh api]: `nhddl-<tag>.zip` (v1.2.2 holds `nhddl.elf`, `README.md`, `examples/nhddl.yaml`, plus a separate `neutrino_v1.8.0-35-g7b6e14b.7z`); digests from v1.2.0 on, `null` for v1.1.2 and `mmce-hdl-4`. Neutrino `neutrino_<tag>.7z`, digests from v1.7.0 on.
- Unpublished NHDDL 2.0 (branch `rewrite`, `bbbb254`) moves to `nhddl/nhddl.cnf`, `global.cnf`, `<ISO>.cnf` and keeps `cache.bin`, `lastTitle.bin` [R README].
- **Strong sign**: `nhddl/cache.bin` starting with `NIDC` or `nhddl/lastTitle.bin`; nothing else writes them. **False positives**: `nhddl/<iso>.yaml` is also written by the PSBBN PC installer (`CosmicScale/PSBBN-Definitive-Project` `scripts/Game-Installer.sh` @`5999256`) [V]; `neutrino/` alone says only "Neutrino installed": RiptOPL's package ships it and RiptOPL, wOPL, XEB+, OSD-XMB and PSBBN use it.

## 8. All builds at a glance

| Build | Global settings, where | State files only it writes on the game device | Unique global keys | `games.bin` record | Art | ZSO / exFAT | BDM fragments | ELF | Hash from GitHub |
|---|---|---|---|---|---|---|---|---|---|
| OPL 1.0.0 | `conf_opl.cfg` + 4, memory card first | — | `check_usb_frag` | 92 B | PNG, JPG, BMP | no / no | one piece (check could be off) | `OPNPS2LD-v1.0.0.ELF` in 7z, 16 variants | none |
| OPL 1.1.0 | same | — | `enable_ilink`, `enable_mx4sio`; none of the 1.2 keys | 92 B | PNG, JPG, BMP | no / no | one piece | `OPNPS2LD-v1.1.0.ELF` in 7z, 16 variants | none |
| OPL 1.2 betas | same | — | ladder §1.2 | 92 B ≤ 2016, 188 B ≥ 2017 | JPG/BMP up to 2168 | ≥ 1871 / ≥ 1880 | 64 shared ≥ 1895 | `OPNPS2LD.ELF` | `latest` only |
| YAOPL | same | — | `mmce_*` **with** `mmce_gameid` (≥ 2367, volatile) | 188 B | PNG | yes / yes | 64 shared | `OPNPS2LD.ELF`, `-yaopl` in the version | `digest` + `SHA256SUMS`, current build only |
| MMCE fork | same; no config on MMCE | — | `mmce_*` **without** `mmce_gameid`; betas `mmce_wait_cylces`, `mmce_use_alarms` | 188 B | PNG | yes / yes | 64 shared; none on MMCE | `OPNPS2LD-MMCE-b1.ELF`, `OPNPS2LD.ELF` | none |
| RiptOPL (C) | `settings_riptopl.cfg` **beside the ELF** (reads `conf_opl.cfg` once) | `config.path`, `favourites.bin` (`OFAV`), `art_cache.bin` (`ARC` v4), `EMBER/settings.txt`, `RA/`; folder `POPS/` auto-created; LF line endings | many (§4) | 188 B (also in sub-folders) | PNG | yes / yes | 64 shared | `APPS/APP_RIPTOPL/RIPTOPL.ELF` + `title.cfg` `Version=` | `digest`, valid for hours |
| RiptOPL (C++) | same | same | lacks some C keys | 188 B | PNG | yes / yes | 64 shared | `RIPTOPL.ELF`, `title.cfg` with `PS2BUILD` | `digest`, `SHA256SUMS.txt`, `BUILD-MANIFEST.json` |
| wOPL | HEAD: `wopl_*.cfg` (libconfig) + `wopl_boot.cfg` **beside the ELF**; v1.0–1.1: `conf_wopl.cfg` + 4 on `mc?:wOPL/` | `IMG/`, `config_errors.log`, `CFG/*.info`, libconfig `CFG/*.cfg`, `art_cache.bin` (`ARC` v3) | `bdm_debug`, `enable_mmce`, … (§5) | 188 B | PNG (+ grey) | yes / yes | 64 shared | `WOPNPS2LD.ELF` | `digest` |
| OPL-Evolution | `conf_opl.cfg` (upstream's) | — | — | 188 B | PNG | yes / yes | 64 shared | `OPL-EVOLUTION-0.27.0-NATIVE-VMC.ELF` | `digest` + `CHECKSUMS.txt` |
| NHDDL + Neutrino | `nhddl/nhddl.yaml` | `nhddl/cache.bin` (`NIDC`), `nhddl/lastTitle.bin`, `nhddl/<ISO>.yaml` | YAML keys | — | `ART/<ID>_COV.png` only | ISO only / yes | 64 shared with both VMCs | `nhddl.elf`, `nhddl-<mode>.elf`; `neutrino/neutrino.elf` | `digest` from NHDDL v1.2.0, Neutrino v1.7.0 |

---

## 9. Signs ranked

"Proves" is what the sign shows on its own. "Profile" is what YAOPLM should conclude today (profiles: OPL 1.2.0 beta, OPL 1.1.0, RiptOPL; everything else maps to 1.2.0 beta until it gets its own).

| # | Sign (read from a PC) | Proves | Profile | Strength | False positives and limits |
|---|---|---|---|---|---|
| 1 | `.yaoplm/manifest.json` whose recorded SHA-256 still matches the ELF at the recorded path | the build YAOPLM installed | that build's | exact | the user may also start another copy (memory card); if the hash no longer matches, ignore the manifest and fall to #2 |
| 2 | SHA-256 of an ELF on the device equals a known release asset (`digest`, `SHA256SUMS`, our history, the 1.1.0 table) | exact build | that build's | exact | none for the match itself; an unknown hash proves nothing. Old betas and rolling RiptOPL builds are often unknown (§1.6) |
| 3 | Version string in an unpacked ELF (`v1.1.0`, `v1.2.0-Beta-<N>-<hash>[-yaopl]`, RiptOPL/wOPL strings) | exact version; the suffix gives the family | that version's | exact | packed ELFs (all default release ELFs) have none; numbers collide between upstream, YAOPL and the MMCE fork, so keep the hash and suffix |
| 4 | `APPS/<app>/title.cfg` with `boot=RIPTOPL.ELF` and `Version=…` | RiptOPL and its flavour | RiptOPL | strong | a user-edited `title.cfg`; check that the named ELF exists |
| 5 | State files only one family writes: `settings_riptopl.cfg`, `conf_riptopl.cfg`, `config.path`, `favourites.bin` with `OFAV`, `ART/art_cache.bin` with `ARC\0` v4, `EMBER/settings.txt`, `neutrino/config/riptopl-cheats.toml` (RiptOPL, §4); `wopl_*.cfg`, `wopl_boot.cfg`, `conf_wopl.cfg` or its `.bak`, `config_errors.log`, the folder `IMG/` (wOPL, §5); `conf_uopl.cfg` (uOPL); `nhddl/cache.bin` (`NIDC`), `nhddl/lastTitle.bin` | the device was used with that family | that family's | strong | they survive a switch to another loader; prefer the newest (mtime) |
| 6 | Format facts: the volume is exFAT; any `.zso` in `CD/`/`DVD/` | not 1.1.0, not a beta before 1880 / 1871 | not 1.1.0 → 1.2.0 beta unless a stronger sign names a fork | strong (negative) | a 1.1.0 user's stick that simply does not work; DEV-2 reports that |
| 7 | `CD/games.bin`, `DVD/games.bin` record size (§1.5) | the generation of the OPL-family build that last listed the folder: 92 B → 1.1.0 or ≤ Beta-2016; 188 B → ≥ Beta-2017 or any 1.2 fork | 92 → 1.1.0; 188 → 1.2.0 beta | strong for "last used with" | absent after YAOPLM writes (we delete it) and on fresh devices; does not name the fork |
| 8 | `conf_opl.cfg` key set (root or `<usb_prefix>/`) | lower bound for the newest writer (§1.2); absence of the always-written 1.2 keys → never saved by ≥ Beta-1871; `mmce_*` with `mmce_gameid` → YAOPL ≥ 2367; without it → MMCE fork release; `mmce_wait_cylces`/`mmce_use_alarms` → MMCE-fork beta (or wOPL, §5) | per §1.2 | medium | usually absent (config on the memory card, §1.5); keys survive switching loaders both ways; the YAOPL/MMCE-fork split rests on one key in a fork under active development (§2, §13) |
| 9 | ELF file names: `RIPTOPL*.ELF` → RiptOPL; `WOPNPS2LD.ELF` → wOPL; `nhddl*.elf` → NHDDL; `OPNPS2LD*.ELF`, `OPL.ELF` → OPL family | family | family's | medium | users rename ELFs; `OPNPS2LD.ELF` is shared by upstream, YAOPL, the MMCE fork and other forks |
| 10 | Per-game keys in `CFG/*.cfg` (§1.3) and VMC names `XEBP_100.NN_<slot>` (§1.4) | some build ≥ that beta edited that game | ≥ the beta → not 1.1.0 | medium-weak | another console or loader may share the device; OPL Manager and others do not write these keys |
| 11 | Fork-only per-game keys and syntax: `$Neutrino*`, `$Ember*`, `$VMCDisable_N`, `$GSMVMode=29`, `$Compatibility` bit 0x80, LF-only CFG files → RiptOPL; `$CoreLoader`, `$EnableImage`, bit 0x40 → RiptOPL or wOPL; libconfig syntax in `CFG/*.cfg`, `CFG/*.info` → wOPL | that family edited the game | that family's | medium-weak | copied CFG sets |
| 12 | `conf_opl.cfg` `default_device=8` | an OPL build with MMCE as mode 8 (YAOPL, MMCE fork) | 1.2.0 beta | weak | wOPL and RiptOPL number their modes differently (§4, §5) |
| 13 | Folders: `POPS/`, `EMBER/`, `neutrino/`, `nhddl/` without caches, `ART/art.tar` | a family was intended | hint | weak | PC managers create them (OrbitPS2, the PSBBN installer makes `POPS` and `nhddl`); RiptOPL's package ships `neutrino/` |
| 14 | Art: JPG/BMP in `ART/` | written by an old manager or for 1.1.0 / a beta < 2169 | hint | weak | PyOPLM, libopl, logi-26, PyOPL-Utility and PS2IsoManager copy JPG covers as they are (research/13 §3); OPL Manager's backups are mostly JPG |
| 15 | Art: 8-bit palette PNG at doubled sizes (COV 280×400, ICO 128×128) | prepared with OrbitPS2 Manager's RiptOPL pipeline | hint for RiptOPL | weak | upstream reads them too (any size ≤ 1024 px) |
| 16 | ISO titles longer than 64 characters | prepared for ≥ Beta-2017 | hint | weak | Redump-named dumps copied by hand |
| 17 | `LNG/<Lang>-<version>/` folders | the user unpacked that version's language pack | hint | weak | OPL ignores the folders |
| 18 | no `conf_opl.cfg` | nothing | — | none | config on the memory card is the normal case |

## 10. Conflicts and the order YAOPLM should use

Proposal [D], to check against ADR-0023 in the PR that implements it.

1. **Collect, do not stop at the first hit.** Every sign of §9 yields `(family?, version range?, rule constraints, strength, file)`. The profile is chosen afterwards, so the card can show the winning sign and the losers.
2. **Hard constraints first.** exFAT and ZSO rule out every build before Beta-1880 / 1871. If the strongest sign is such a build (a 1.1.0 hash on an exFAT stick), keep the profile the user's build implies, and show the conflict as a device problem ("OPL 1.1.0 cannot read exFAT"), not as a detection doubt: DEV-2 already says the device does not suit that loader.
3. **Then by strength.** Manifest (if its hash still matches the ELF) > ELF hash > version string in an unpacked ELF > RiptOPL `title.cfg` > loader-written state files unique to a family > `games.bin` generation > `conf_opl.cfg` key set > ELF names > per-game keys and VMC names > folders and art. Within one level, a newer file (FAT modification time) beats an older one.
4. **ELF versus traces of use.** An ELF says what is installed; `games.bin`, `conf_opl.cfg`, `settings_riptopl.cfg`, `nhddl/cache.bin` say what last used the device. If a trace is newer than the ELF and points to another generation (for example a 92-byte `games.bin` written after a 1.2 ELF was copied), the console probably starts another copy, e.g. from the memory card. Keep the ELF's profile (stronger), and warn: "The stick was last listed by OPL 1.1.0 or an old beta. If you start OPL from the memory card, choose the profile by hand."
5. **Several loaders on one device** (a 1.2 `UPDT/OPL.ELF` and `APPS/APP_RIPTOPL/`): list all on the device card. Pick the one the manifest marks as installed by YAOPLM; otherwise the one whose own state files are newest; otherwise ask once and remember the choice per volume serial (ADR-0023 override). Do not merge profiles.
6. **Family versus profile.** Several families share a profile today (YAOPL, upstream beta, the MMCE fork, wOPL, OPL-Evolution → OPL 1.2.0 beta). A conflict between two of them changes the device card's loader line, not the rules, so it needs no warning in the library.
7. **Weak hints never decide.** JPG covers, `POPS/`, `neutrino/`, long names and folder names only add a line to the reason ("JPG covers found: they are not shown by OPL 1.2 betas from 2169 on").

## 11. Device test matrix

For the detection code (LIB-1, LDR-4) and, later, the installer (LDR-3) and the formatter (DEV-3). Part A runs in CI on temp folders and FAT32/exFAT image files (CLAUDE.md rule 1: never a real drive in automated tests). Part B is by hand on the owner's **tests-only stick** (2026-10-24: disk "USB3.0 FLASH DRIVE", serial `037115C350B0`, 57.77 GB, MBR, one FAT32 partition at sector 8064, 64 KiB clusters, volume serial `3C761797`, empty). The owner allows formatting it; every write still goes through a plan, a confirmation and a re-check of the disk and volume serials (the volume serial changes after a format).

### 11.1 Part A: fixtures (no console needed)

| # | Fixture | Expected profile, confidence, reason |
|---|---|---|
| A1 | empty FAT32 image | default, "by default" |
| A2 | `.yaoplm/manifest.json` naming each catalogue build | that build's profile, "exact (manifest)" |
| A3 | `conf_opl.cfg` from 1.1.0's `_saveConfig` key set (has `enable_ilink`, no `bdm_cache`/`enable_bgm`/`x_sensitivity`/`enable_bdm_hdd`) | OPL 1.1.0, "indirect": "settings last saved by OPL 1.1.0 or a beta before 1871" |
| A4 | the same file plus `bdm_cache` only | 1.2.0 beta, "indirect" (≥ Beta-1871) |
| A5 | `conf_opl.cfg` with `check_usb_frag` and no `enable_ilink` | OPL 1.1.0 rules, reason "OPL 1.0.0" |
| A6 | `conf_opl.cfg` with the five `mmce_*` keys incl. `mmce_gameid` | 1.2.0 beta; family YAOPL (or a debug MMCE-fork build) |
| A7 | the same without `mmce_gameid` | 1.2.0 beta; family MMCE fork |
| A8 | `CD/games.bin` with 92-byte records matching the ISOs in `CD/` | OPL 1.1.0, "indirect": "last listed by OPL 1.1.0 or a beta before 2017" |
| A9 | 188-byte records; one with `.zso` | 1.2.0 beta, "indirect" |
| A10 | a `games.bin` whose size is a multiple of 4,324 | decided by the extension offset, never by size alone |
| A11 | exFAT image, no other sign | 1.2.0 beta, "indirect" (not 1.1.0) |
| A12 | `DVD/*.zso` on FAT32 | 1.2.0 beta, "indirect" |
| A13 | only JPG covers in `ART/` | default; hint "JPG covers: OPL 1.1.0 or a beta before 2169 reads them" |
| A14 | `VMC/XEBP_100.01_0.bin` or `$VMC_0=XEBP_100.01_0` | not 1.1.0 → 1.2.0 beta, "indirect" |
| A15 | ISO with a 70-character title | no profile change; for the 1.1.0 profile a LIB-4 warning "OPL 1.1.0 hides names longer than 64 characters" |
| A16 | an unpacked ELF containing `v1.1.0` / `v1.2.0-Beta-2271-b3a2199` / `…-yaopl` | that build, "exact (version string)" |
| A17 | RiptOPL, wOPL, NHDDL settings files and caches (§4–§7) | that family |
| A18 | conflicts from §10 (e.g. a 1.2 hash and a 92-byte `games.bin`) | the stronger sign, plus a warning naming the weaker one |

### 11.2 Part B: the tests-only stick and the console

Each step: plan → confirm → write → `yaoplm device F:\` (or the device card) → compare with the expected line → console check where marked.

| # | Prepare (through YAOPLM where it can, by hand otherwise) | YAOPLM should show | On the console |
|---|---|---|---|
| B1 | format FAT32 (DEV-3) or leave as is; nothing else | default profile; no loader | — |
| B2 | install OPL 1.1.0 to `UPDT/OPL.ELF` | "OPL 1.1.0, exact (manifest)"; folders `CD DVD CFG CHT VMC ART THM LNG APPS` | FMCB menu starts it; enable USB, games appear |
| B3 | delete `.yaoplm/manifest.json` by hand | "OPL 1.1.0, exact (hash)" once the 1.1.0 table exists, else "OPL, version unknown" | — |
| B4 | add a game, scan on the console with 1.1.0 | `CD/games.bin` with 92-byte records → still 1.1.0 | the game is listed |
| B5 | replace the ELF with 1.2 `latest`, scan on the console | ELF: 1.2.0 beta (hash); `games.bin` now 188-byte; no conflict warning | the game is listed; `games.bin` rewritten |
| B6 | replace with YAOPL `latest` | "YAOPL v1.2.0-Beta-…-yaopl, exact (hash)"; profile 1.2.0 beta | MMCE page present in settings |
| B7 | install RiptOPL (`APPS/APP_RIPTOPL/`) | "RiptOPL, by `title.cfg`"; `POPS/`, `EMBER/` at the root; after a console save `settings_riptopl.cfg` beside the ELF → "exact" stays | RiptOPL starts from the Apps page or the launcher path |
| B8 | install wOPL `WOPNPS2LD.ELF` | "wOPL, exact (hash)"; after a console save `wopl_*.cfg` | — |
| B9 | NHDDL + `neutrino/` | "NHDDL"; after a launch `nhddl/cache.bin`, `lastTitle.bin` | a game starts |
| B10 | format exFAT, no ELF, one game | "1.2.0 beta, indirect (exFAT)"; installing 1.1.0 is refused or warned (exFAT unreadable) | 1.1.0 does not see the stick |
| B11 | FAT32 with a ZSO and with a fragmented ISO | ZSO → "not 1.1.0"; fragmented ISO → 1.1.0 warning `gameFragmentedOpl110` under the 1.1.0 override | 1.1.0 refuses the fragmented game |
| B12 | JPG covers left in `ART/` with 1.2 installed | profile stays 1.2.0 beta (hash); warning "JPG covers are not shown by this build" (LIB-4) | covers missing on ≥ Beta-2169 |
| B13 | the owner's MMCE SD card, read only | MMCE layout; profile from `conf_opl.cfg` only if a copy is on the card (usually it is not: §1.5) | — |

The 1.1.0 hashes for B3 (and the MMCE-fork betas, if they are added to the catalogue) need the one-time download noted in §1.6 and §3, with the owner's go-ahead.

## 12. What changes against SPEC §5.1.1 and research/07 §1.9

Suggested edits, for the owner to approve; none is applied here.

| Today | Correction | Section |
|---|---|---|
| §5.1.1 step 2: "`mmce_*` → MMCE fork" | `mmce_*` in `conf_opl.cfg` means "an OPL build with MMCE". YAOPL writes the same five keys since 2026-10-23; a release of the MMCE fork never writes `mmce_gameid`, YAOPL always does. The profile is 1.2.0 beta either way, so only the loader line (LDR-4) depends on it | §2, §3 |
| §5.1.1 step 2: "keys absent from 1.1.0 → 1.2.0 beta" | A 1.2 key proves only that some ≥ beta saved the file once (OPL keeps unknown keys, so they survive a downgrade). The **absence** of the keys every beta ≥ 1871 always writes is the useful sign: it points to 1.1.0 rules. SPEC has no positive 1.1.0 sign except the hash today | §1.2 |
| §5.1.1 step 3: "JPG covers hint at 1.1.0" | JPG and BMP are read by 1.1.0 **and by every beta before 2169** (2024-10-31); many managers copy JPG covers as they are. Keep it as a hint with that wording | §1.4, §9 #14 |
| §5.1.1 step 3: "exFAT or ZSO exclude 1.1.0" | Correct, and they also exclude betas before 1880 (exFAT) and 1871 (ZSO) | §1.4 |
| — (missing) | **`CD/games.bin` and `DVD/games.bin`**: OPL writes them on the game device itself; 92-byte records = 1.1.0 or ≤ Beta-2016, 188-byte records = ≥ Beta-2017 or a 1.2 fork. A strong "last listed by" sign that does not depend on where the config lives | §1.5 |
| — (missing) | Per-game keys and VMC group names (`XEBP_100.NN_<slot>`) date a 1.2 build; unique state files of RiptOPL, wOPL and NHDDL | §1.3, §1.4, §4–§7 |
| SPEC §4: "кэш `games.bin`" is listed as a 1.2.0 beta feature | `games.bin` exists in 1.0.0 and 1.1.0 too; only its record size changed | §1.5 |
| LDR-4: "ELF сжат ps2-packer, строку версии из него не прочитать" | True for the default release ELFs. VARIANTS, debug and self-built `NOT_PACKED=1` ELFs carry the version in plain text, including 1.1.0's 16 variants | §1.6 |
| ADR-0023 decision "exact SHA-256 table of OPL 1.1.0 release ELFs" | The table is 17 ELFs (the packed main ELF and 16 variants), all inside 7z files with `digest: null`; building it needs one download. 1.0.0 has the same shape | §1.6 |
| research/07 §1.9 "Upstream archive member names: `OPNPS2LD-v1.2.0-Beta-2271-b3a2199.ELF`" as version source | Beta numbers collide: YAOPL counts its own commits and the MMCE fork's are upstream's numbers for other code. A version is number + hash + suffix. Hashes of builds made before upstream's history rewrite may not resolve in today's repo | §1.1, §2, §3 |
| SPEC §5.10.1: YAOPL's CI appends `HASHES.txt` | Not there yet: the release has `SHA256SUMS` for the current build only | §2 |
| §5.1.1 "Отсутствие `conf_opl.cfg` ничего не значит" | Correct. Also: OPL looks for it at the root of each BDM unit at boot, but saves it under `usb_prefix` when one is set, so read both places | §1.5 |
| Profile "OPL 1.2.0 beta" assumes the newest rules | Betas 1871–1894 read ZSO but still need each game in one piece. A device dated to such a beta (§1.2) is safest with the 1.1.0 fragment rule. Rare; worth a note in the reason, not its own profile. Older betas being more permissive (JPG up to 2168) does no harm | §1.4 |

| Today (SPEC §5.1.1 step 2, research/07) | Correction | Section |
|---|---|---|
| "`settings_riptopl.cfg` and `conf_riptopl.cfg` → RiptOPL" | Correct. Add `config.path`, `favourites.bin` starting with `OFAV`, `art_cache.bin` with `ARC` v4 and LF-only CFG files. A `conf_opl.cfg` beside a RiptOPL ELF is RiptOPL's read-only seed, not a sign of upstream | §4 |
| "`wopl_*.cfg` and `conf_wopl.cfg` → wOPL" | Correct; add `conf_uopl.cfg` (uOPL), `wopl_boot.cfg`, `config_errors.log`, the folder `IMG/`, libconfig `CFG/*.cfg` and `CFG/*.info`. wOPL never touches `conf_opl.cfg`. HEAD keeps its settings beside the ELF, v1.0–1.1 on `mc?:wOPL/` | §5 |
| "`nhddl/` → NHDDL" | `nhddl/` with `<ISO>.yaml` is also made by the PSBBN installer. Strong only with `nhddl/cache.bin` (`NIDC`) or `nhddl/lastTitle.bin`. NHDDL ≤ v0.2.0 used a generic `config/` folder | §7 |
| research/07 §1.4: `mmce_gameid` default 1 in the MMCE fork | Read and saved only in `__DEBUG` builds | §3 |
| research/07 §1.9 #5: settings files "identify the profile even without the ELF" | They identify the family. `games.bin` (any OPL-family build) and the format facts identify the generation. RiptOPL and wOPL HEAD keep their settings beside the ELF, so a stick without the ELF often has none | §4, §5, §9 |
| research/07 §1.10: rolling `digest`s | RiptOPL's rolling assets change several times a day: a `digest` is good for one build; only our own append-only history recognises the rest | §4 |

## 13. Keeping the signs current

Every sign above is true **at the commit it was read at**, nothing more. The builds keep changing, and so the product owner's rule (2026-10-24) is: before relying on a sign, or changing detection, re-check it against the current code of every build it names (CLAUDE.md rule 6).

How fast each build moves, as of 2026-10-24 [V commit dates, release lists]:

| Build | Pace | What is most likely to change |
|---|---|---|
| YAOPL | active development by the owner; MMCE arrived 2026-10-23 | the `mmce_*` key set and whether `mmce_gameid` is always written (§2), the version suffix, release assets (`SHA256SUMS`, the planned `HASHES.txt`) |
| RiptOPL (both repos) | several builds a day on rolling tags; large LLM-assisted changes | settings and cache file names, keys, the C++ edition's layout (§4) |
| wOPL | config format changed in 2026-06 (libconfig) | file names and syntax (§5) |
| Upstream 1.2 beta | new keys every few months (§1.2, §1.3) | new always-written keys, art and format rules |
| NHDDL | 2.0 rewrite pending (`.cnf` instead of `.yaml`) | the files in `nhddl/` (§7) |
| MMCE fork | no change since 2025-11-24 | — |
| OPL 1.0.0, 1.1.0 | frozen | nothing; the hash table is final once built |

Routine when detection code or the loader catalogue changes, and before each YAOPLM release:
1. Pull the clones (REFERENCES.md loop); read YAOPL from GitHub (`gh api`) or a clone of our own, never by fetching into the owner's working copy.
2. Re-extract the key sets at each build's current head and diff them with §1.2, §1.3 and §2–§7. The method used here: `#define CONFIG_<X> "<key>"` in `include/*.h` plus string literals passed to `config{Get,Set,Remove}*` in `src/*.c`, per revision with `git show <rev>:<path>`; for the C++ RiptOPL and wOPL's libconfig files, grep the accessors named in §4 and §5. For each new key, find the first `master` build that contains it (first-parent search, §1.1).
3. Re-check what the record size of `games.bin` depends on (`base_game_info_t`, `ISO_GAME_NAME_MAX`), the settings file names (`configFilenames[]` and the forks' equivalents), the state files and their magic (`NIDC`), and the ELF and asset names (`gh api …/releases`).
4. Update the "checked at" commit in this report and in the code comment next to the rule. Each rule in code should name the build and commit it was checked at, and have a fixture per build in the tests (§11.1), so a change in a build turns into a deliberate fixture update, not a silent miss.
5. Detection must tolerate the unknown: a key, file or record size it does not know lowers confidence, never raises an error, and never picks a profile by itself.
