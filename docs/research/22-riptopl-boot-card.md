# 22. RiptOPL inside a memory card emulator's boot card

Date: 2026-10-27. The owner asked whether RiptOPL would be a better OPL to put inside the emulator's boot card (LDR-5, plan `docs/plans/emulator-sd.md` PR 4) than the MMCE fork's `v1.2.0-Beta-2216-bb415d2`, which the factory card runs today. The setup is a PSXMemCard Gen2 on sd2psXtd 1.4.0, FreeMcBoot booting from an 8 MiB boot card, and games on the emulator's exFAT microSD read through MMCE (research/19, research/20).

| Source | Commit |
|---|---|
| RiptOPL "Legacy Edition", `NathanNeurotic/Open-PS2-Loader` `rebuild/main` | `38ea46ad` (2026-10-26) |
| RiptOPL C++ edition, `NathanNeurotic/RiptOPL` | `6edea174` |
| MMCE fork, `ps2-mmce/Open-PS2-Loader` `mmce_support` (2216 = `bb415d2`) | `e554796` |
| Upstream OPL | `b3a21997` |
| sd2psXtd firmware 1.4.0 | `8a20bd9` |
| YAOPL `FORK.md`, `belomaxorka/yaopl` | GitHub, 2026-10-27 |

Earlier notes: research/07 §1.3 (package layout, flavours, settings home), §1.8 (backends), research/15 (modes 7 and 8), research/16 (signs). Tags: **[V]** read in code or release data, **[R]** docs, issues or release text, **[D]** deduced. Two subagents did the first pass; the claims below marked [V] were checked again in code, except the commit counts.

## 1. Verdict

- **Not as the main boot-card OPL yet.** Its MMCE code is better than 2216's (§3), but it changes several times a day. Two regressions in October hit exactly this setup, and the fix for one still waits for a console check (§4).
- **It fits only as a replacement.** No RiptOPL ELF fits next to the others in channel 1's free 465 KiB (§2).
- **If it goes in:** replace 2216's file and keep 2216 as the backup. Take either:
  - the `current-fan-favorite` snapshot (Beta-3390), which predates both regressions but lacks the October VMC fixes;
  - or a rolling standard ("inceBtion") build at or after `11a5150e`, which needs the console checks in §8.
- **Turn "Send GameID on launch" on** (§5).
- **The safest trial** is a USB entry: FreeMcBoot already tries `mass:/…` first (research/20 §1.1), so the boot card stays untouched.

## 2. Size, files it needs

Budget on the factory channel 1: 465 free clusters plus the 1,318 of `APPS/OPL-MMCE-BETA2.ELF` = **1,783 clusters** (1 KiB each, research/20 §2).

| Release tag | Version | `RIPTOPL.ELF` | Clusters | Fits in place of 2216 |
|---|---|---|---|---|
| `rolling` (inceBtion, not a pre-release) | Beta-4308 | 1,564,804 B [V `gh release view`] | 1,529 | yes |
| `rolling-korium` | Beta-4308 | ~1,831,400 B [V] | 1,789 | no, 6 over |
| `rolling-ra` (pre-release) | Beta-4308 | 1,698,276 / 1,964,836 B [V] | 1,659 / 1,919 | standard yes, Korium no |
| `current-fan-favorite` | Beta-3390 (`c53fd8cb`, 2026-09-30) | ~1,482,700 / ~1,749,200 B [V] | 1,448 / 1,709 | yes / yes |

- For comparison [V]:

  | Build | ELF | Clusters |
  |---|---|---|
  | MMCE fork beta-2 | 1,348,708 B | 1,318 |
  | YAOPL | 1,377,492 B | — |
  | wOPL | 1,538,308 B | — |
- **The ELF runs alone from `mc0:/APPS/`** [V package listing, `src/opl.c:1902-1930`]:
  - the IRX modules are inside it;
  - English is built in, and other languages load from `LNG/` on any device, MMCE included;
  - without `bgm.ogg` there is only no music.
- **The full app does not fit:** `APP_RIPTOPL.psu` is 1,985,024 B.
- **PS1 needs folders on the SD:** `POPS/`, `EMBER/` and `neutrino/` belong at the SD root. Native PS2 games need none of them [R `install.html`].
- **Flavours:** PS2DEV/OFFICIAL × PINNED/ROLLING are toolchains, built from the same commit [V `.github/workflows/rolling-release.yml:11-45`]. The loose `RIPTOPL.ELF` is OFFICIALROLLING; the docs recommend PS2DEVPINNED [R].

## 3. MMCE: what RiptOPL does better than 2216

- **Modules:**
  - mmceman comes from ps2-mmce `db3e93f0` plus four local patches: an fd-slot leak, ENOENT for `open` and `dopen`, and end of directory on `dread` [V `.github/scripts/install_coherent_mmce.sh`];
  - mmcedrv and mmceigr are ps2-mmce v2.1.1 prebuilts, essentially 2216's [V/R].
- **In-game reads** (`modules/iopcore/cdvdman/device-mmce.c`) [V]:
  - 64-bit `get_size`, where 2216 truncates to 32 bits and an error reads as a size;
  - a bounded 10 s device wait instead of `while(1)`;
  - a read that succeeds on a retry is no longer reported as failed.
- **VMC on MMCE:**
  - 2216 reports every page write as done [V `mcemu/device-mmce.c`]. RiptOPL passes failures on (`2a497b1f`, 2026-10-26);
  - IGR and power-off wait for a VMC save (`32ce8b1c`);
  - no false fragmentation warnings on MMCE (`554c2739`).
- **Prefix:** the trailing `/` of `mmce_prefix` is normalised again after re-detection (research/07 §1.4: 2216 needs it typed).
- **Launch:** `last_played` is written before the card switch, so it lands on the boot card; the GameID is sent before any VMC or ISO is opened.
- **IGR:** the same mmceigr mechanism.
- **Upstream fixes:** upstream is not merged (`b3a21997` is no ancestor), but its fixes are ported one by one:
  - MX4SIO ZSO: `35af507b`;
  - media size: `1e321048`;
  - parental lock: `06864626`.

  2216 lacks 52 upstream commits [V].

## 4. Risks

- **Two October regressions hit exactly this setup:**
  - Every native MMCE game hung after the PS2 logo from 10-16 to 10-19 [V `ad43082f`, fixed `11a5150e`]. The cause was the SDK SIO2MAN loaded for MMCE games (#962 → #1007).
  - IGR was dead in most games from 10-05 to 10-19 (#984 "P0 Regression"). It took three rewrites, the last `38481e9c`.
- **Merged without a console check:** issue #1033 (open) says recent fixes "were merged on host-test evidence only". The MMCE black-screen fix is one of them [R].
- **Churn:**
  - 131 successful `rebuild/main` publishes between 2026-09-25 and 10-27, up to 8 a day [V `gh run list`];
  - no stable channel: the docs call every build a development build that "may be unstable" [R `releases.html`];
  - the Fan Favorite tag moves too (the previous one was Beta-2753).
- **AI-written code:** 860 of 1,610 non-merge commits carry AI co-author trailers. There are 163 host tests that compile code cut out of the sources, and no emulator or hardware CI [V, counts by a subagent].
- **The C++ edition** says "NOT READY FOR USE" [V]. Legacy is the maintained product.
- **GameID:**
  - The wait differs from the firmware's [V `mmcesupport.c:94-95,202-213`]. RiptOPL reads the busy bit at once, then polls 15 × 200 ms, then waits up to 5 s for a directory open. 2216 waits 1 s first, then polls 15 × 1 s.
  - In sd2psXtd 1.4.0 the busy bit is set only once the switch has started, after closing the old card and a 500 ms pause (`ps2_mmceman.c:123-158`). So RiptOPL's first read can see "idle" before the switch begins [D]. The directory-open wait covers most of this.
  - RiptOPL's `mmceReset` (devctl 0x9, sent on app launches) assumes the firmware returns to card 1, channel 1 and clears the GameID. Firmware 1.4.0 only closes its open SD files [V `ps2_mmceman_commands.c:228-243`].
  - Issue #68 (2026-10-24): on firmware 1.4.0 the GameID works with the OPL core but not with Neutrino [R].

## 5. What it writes to the boot card

- **Settings beside its ELF:** started from `mc0:/APPS/`, the boot folder comes from argv[0]. The first save writes these into `mc0:/APPS/` inside the card image [V `src/opl.c:2749-2800`]:
  - `settings_riptopl.cfg`;
  - `conf_last`, `apps`, `network` and `game` configs.
- **`OPL/conf_opl.cfg` is a read-only seed:** read once, never written (`src/opl.c:2562, 2793`) [V]. Going back to 2216 keeps 2216's settings intact.
- **Other writes:**
  - `favourites.bin`, only when favourites change;
  - a save-icon set (about 35 KiB), only if `APPS/` lacks a full `icon.sys`. The factory card has one;
  - the OSD history entry on every launch, as every OPL does [V].
- **Writes are not atomic** (`O_TRUNC`); RiptOPL keeps a RAM copy to roll back a failed save [V].
- **No log or art cache** goes to the card [V].
- **GameID is off by default** [V `src/opl.c:5044` `gMMCEEnableGameID = 0`; 2216: on]. The emulator still switches to the game's card through its history tracker: on a new entry in the boot card's `B?DATA-SYSTEM/history` it calls `ps2_mmceman_set_gameid` [V sd2psXtd `ps2_history_tracker.c:244-258`]. That path is slower and needs the folder on the card. The factory cards have `BADATA-SYSTEM`.

## 6. Library compatibility

- **New game CFG keys:** `$CoreLoader`, `$Neutrino*`, `$Ember*`, `$VMCDisable`, `$EnableImage` [V `include/config.h`].
- **`$Compatibility` 0x40 and 0x80:** Neutrino `-gc=7` and Slow Reads [V `include/iosupport.h:102-103`]. Upstream, 2216 and YAOPL rebuild the value from six bits when they save a game's settings, so 0x40/0x80 disappear [V upstream `src/guigame.c:989-993`]. YAOPLM already treats these bits as RiptOPL-only (CLAUDE.md, research/15).

## 7. Other builds for this setup

| Build | For | Against |
|---|---|---|
| MMCE fork 2216 (today) | Frozen, widely used; the emulator vendors point to it [R] | 2025-01; misses 52 upstream commits; `mmce_prefix` needs `/`; VMC writes always "succeed" |
| RiptOPL | Hardened MMCE read and VMC paths, newer mmceman, PS1 over MMCE | Several builds a day, October MMCE and IGR regressions, little hardware verification, CFG bits other builds drop |
| YAOPL | Upstream plus fixes, 0 behind; smaller ELF | MMCE since 2026-10-23, without VMC on the card or split games. MMCE games get the SDK SIO2MAN, the setup that black-screened RiptOPL (`11a5150e`); `MMCE_NO_SDK_SIO2MAN=1` builds RiptOPL's way for a console A/B [R `FORK.md`] |
| wOPL | Fits (≈1,503 clusters), MMCE since v1.0 | Last pre-release 2026-07, quiet since 09-24 |

## 8. For YAOPLM

- **PR 4:** RiptOPL can be offered for a boot card only as a replacement for an existing ELF. The size check (research/20 §3 rule 6) decides per release: Korium rolling never fits channel 1.
- **Settings:** a boot card install could carry `settings_riptopl.cfg` with `mmce_gameid=1` next to the ELF in the same plan. That needs a new ADR, since today launcher settings are never written.
- **The catalogue entry (M6, LDR-1/LDR-6)** must say that releases move several times a day. Prefer a release YAOPLM pins (the Fan Favorite snapshot) for boot cards.
- **Console checks before RiptOPL is recommended for this setup** (`docs/testing/`):
  1. RiptOPL at or after `11a5150e` from the boot card: an MMCE game starts past the PS2 logo.
  2. IGR returns to the boot card.
  3. With "Send GameID" on and off, the game saves to its own card, not to the boot card.
  4. A VMC on the SD saves and loads.
  5. The same game list and game run with 2216, for comparison.
- **For YAOPL** (`belomaxorka/yaopl`, the owner's fork): an MMCE game starts with the default SDK SIO2MAN and with `MMCE_NO_SDK_SIO2MAN=1`. RiptOPL hung on the first.
