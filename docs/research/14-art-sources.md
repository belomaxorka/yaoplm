# 14 - A network source for every picture type

Date: 2026-10-24. Scope: milestone M5, ART-1, ART-2, ART-4, META-4 (ADR-0035). Goal set by the product owner: every OPL picture type (COV, ICO, COV2, LAB, BG, SCR, SCR2, LGO) gets a working network source with fallbacks, chosen silently (the user never picks a source), so that a picture is found for about 98% of games. The right game and the right region come first.

Legend: **[verified]** a request was made or the primary page or file was read; **[secondary]** third-party code, docs or forums; **[unverified]** not confirmed.

Method: about 75 small HTTP requests with UA `YAOPLM-research/0.1 (+https://github.com/belomaxorka/yaoplm)` and a 1 s gap. Largest downloads: four libretro directory indexes (about 2 MB each), the xlenore `covers/default` tree (2.3 MB JSON), the TheGamesDB spec (66 KB), four libretro box arts. No archive was downloaded whole, no credentials were used. Local data: the table of contents of `OPLM_ART_2024_09` (`D:\yaoplm-testdata\oplm-art\OPLM_ART_2024_09.tsv`, from `scripts/oplm-backup-toc.py`), the source file lists (`D:\yaoplm-testdata\art-lists\`, from `scripts/art-source-lists.py`) and the reference clones (REFERENCES.md).

## 1. Result

| Type | Order of sources (OPL profile) | Coverage, serials with a Redump name (n = 11,571) | NTSC-U / PAL / NTSC-J+K |
|---|---|---|---|
| COV | xlenore → OPL Manager → libretro → ScreenScraper | **92.4%** (90.6% before) | 93.6 / 91.6 / 93.0% |
| ICO | OPL Manager → ScreenScraper | **52.7%** (none before) | 89.7 / 73.4 / 24.5% |
| COV2 | OPL Manager → ScreenScraper | **56.6%** (none before) | 87.2 / 75.4 / 32.2% |
| LAB | OPL Manager → ScreenScraper | **52.1%** (none before) | 85.2 / 72.3 / 25.9% |
| BG | OPL Manager (`BG_00`, then `SCR_02`+) → ScreenScraper → libretro snap | **82.5%** (none before) | 93.5 / 89.0 / 74.4% |
| SCR | libretro → OPL Manager (`SCR_00`) → ScreenScraper | **83.4%** (72.5% before) | 93.5 / 88.7 / 76.6% |
| SCR2 | libretro titles → OPL Manager (`SCR_01`) → ScreenScraper | **75.9%** (58.6% before) | 91.7 / 86.4 / 63.0% |
| LGO | libretro → OPL Manager → ScreenScraper | **73.6%** (59.9% before) | 93.3 / 86.5 / 57.7% |

Measured by `crates/yaoplm-api/tests/art_coverage.rs` (a real-data test) with the code that picks the sources, against the file lists of 2026-10-24. Over all 12,978 serials of the bundled database the numbers are 4-8 points lower (COV 87.7%, ICO 48.3%). ScreenScraper is not in these numbers: it answers only to a registered program (§4).

**The 98% target is not reached without ScreenScraper.** US and European games get 72-94% of every type; the gap is the packaging of Japanese and Korean releases (ICO, COV2, LAB at 25-32%), which no keyless source has. ScreenScraper is the only candidate to close it, and only its coverage per type, once the key exists, will tell how far (§7).

The order puts a source whose pictures are smaller than the slot last: for RiptOPL's 280×400 covers and 128×128 icons, OPL Manager's 140×200 and 64×64 pictures come after the others (`sources::order`).

## 2. Region policy

A picture of another region's release is the wrong picture: covers, back covers, spines and discs differ by rating badges, the PAL or NTSC bar, the language. Rules (`yaoplm_net::region`):

- **Packaging (COV, COV2, LAB, ICO)** comes only from a match on the game's ID or serial (xlenore, OPL Manager), from a Redump name tagged with the game's region (libretro, `region::name_fits`; the first parenthesised group that names a region, so `Bonus Demo 7 (6-7) (Europe)` reads as Europe), or from a ScreenScraper media tagged with it. An empty slot is better than a guess. Region families: NTSC-U (America), PAL (Europe, Australia), NTSC-J and NTSC-K (Asia), from the serial prefix; a prefix that names no region (homebrew, `PBPX` promotional discs) lets everything through.
- **Byte-identical OPL Manager pictures across regions.** The central directory gives every picture's CRC-32 and size. A packaging picture whose bytes also occur under a game of another region family is flagged (`Picture::shared_across_markets`): one of them shows the wrong release (Dragon Rage's PAL IDs and Herdy Gerdy's Japanese and Korean IDs carry the US cover with its ESRB badge). Silent runs leave it out ("only another region's release", `notices.artOtherMarket`); the game card offers it last. 2.1% of covers and about 1% of icons, spines and back covers are flagged; the slot stays empty for 6 covers, 29 back covers, 36 spines and 47 icons that no other source has. Within one family sharing is normal (PAL language variants).
- **Shared types (SCR, SCR2, BG, LGO)** may come from the game's releases elsewhere: when the game's own serial has nothing, its other releases with the same Redump title (`titles::Database::same_title`, at most two, America first) are asked at every source but ScreenScraper. RetroArch does the same with its "short name" fallback (research/03 §2.3). This added 4-6 points to each shared type.
- libretro's `Dead or Alive 2 (Japan)` box art is the PAL scan: no rule can tell from the name, so the order of sources does (xlenore and OPL Manager have the Japanese cover and come first).

### Dead or Alive 2 across sources [verified]

EU `SCES-50003` ("Dead or Alive 2 (Europe) (En,Fr,De,Es,It)"; PCSX2 wrongly titles it "Dead or Alive 2 - Hardcore"), JP `SLPS-25002`, US `SLUS-20071` ("DOA2 - Hardcore (USA)"). The JP "DOA2 Hardcore" is a separate disc, `SLPS-25026`.

| Source | EU | JP | US |
|---|---|---|---|
| xlenore COV | PAL ✔ | JP ✔ | US Hardcore ✔ |
| libretro Boxarts | PAL ✔ | **PAL cover under the Japan name ✘** | US Hardcore ✔ |
| OPL Manager COV, ICO, LAB | PAL ✔ | JP ✔ | US Hardcore ✔ |
| OPL Manager COV2 | PAL back (Spanish text) ✔ | not viewed | US Hardcore back ✔ |
| OPL Manager SCR, BG, LGO | shared across the three IDs, as expected | | |
| TheGamesDB | UK entry, no UID | NTSC-J entry carries **UID SLPS-25026** | "DOA2: Hardcore" carries **UIDs SLPS-25002, SCES-50003 ✘** |
| LaunchBox | one entry with EU and JP front, back, spine, disc | same | separate "DOA2: Hardcore" |
| Nostlan | — | — | disc ✔; **full wrap is the PAL one ✘** |

## 3. OPL Manager's backup on archive.org

### 3.1 The item [verified]

- `OPLM_ART_2024_09` is the last "entire OPLM ART database backup" (`oplmanager.com/site/?backups`, collection "Community Data", uploader `danielb7390`). `OPLM_ART_2024_09.zip`: 5,539,563,942 bytes, md5 `e6a2ec7413c7f58d79b1bff831114e91`, Last-Modified 2024-09-02. No licence stated.
- Zip64, **every member stored (method 0)**. Central directory: 102,803 entries, 8,623,924 bytes at offset 5,530,939,920. Members ordered by type, then ID: ICO → COV2 → LAB → COV → SCR_nn → BG_nn → LGO.
- Local headers use data descriptors (flag 0x0808) with zeroed sizes and a 24-byte zip64 extra field: sizes, CRCs and offsets come from the central directory only.
- Names: `PS2/<ID>/<ID>_<T>.png`, T in COV, COV2, ICO, LAB, LGO, SCR_00..SCR_nn, BG_00..BG_nn. All PS2 pictures are 8-bit palette PNGs at OPL's sizes (COV 140×200, ICO 64×64, COV2 242×344, LAB 18×240), written to the device byte for byte (ADR-0033). One game's eight types come to about 355 KB.
- 10,350 PS2 IDs: COV 10,061, COV2 6,826, LGO 6,606, SCR_00 6,576, ICO 6,362, LAB 6,293, SCR_01 6,133, BG_00 5,959. 105 IDs are not in the title database (homebrew, prototypes).

| Region | n | COV | COV2 | ICO | LAB | BG | SCR | SCR2 | LGO |
|---|---|---|---|---|---|---|---|---|---|
| NTSC-U | 2,147 | 90% | 85% | 87% | 83% | 79% | 85% | 83% | 84% |
| PAL | 3,925 | 88% | 76% | 75% | 73% | 68% | 73% | 69% | 70% |
| NTSC-J | 4,757 | 77% | 33% | 24% | 25% | 25% | 29% | 25% | 29% |
| NTSC-K | 537 | 64% | 31% | 28% | 31% | 42% | 47% | 44% | 51% |

(The backup alone, serials with a Redump name.)

### 3.2 Access methods [verified 2026-10-24]

| Method | Result | Time |
|---|---|---|
| Per-member URL `archive.org/download/OPLM_ART_2024_09/OPLM_ART_2024_09.zip/PS2/<ID>/<ID>_<T>.png` (redirects to `view_archive.php`) | 200 `image/png` in 12 of 12 tries | 2.5-8.7 s |
| The same for a missing member | **503 text/html after 9.3 s**, not 404 | |
| Range on the zip through `/download/` (redirects to `dn721905.ca`) | first try 500, second 206 with the right bytes and CRC | 7.9-8.4 s |
| Range straight to `dn721905.ca.archive.org` | 500 twice | |
| Range straight to `ia800701.us.archive.org/11/items/...` (the item's `d2`) | 206 | 18 s |
| Tail (`bytes=-4096`) | 206 | 3.5 s |
| Two ranges in one request | 206 `multipart/byteranges` | 42.6 s |
| GitHub re-host `Luden02/psx-ps2-opl-art-database` | 200, the same bytes | 1.25 s |

### 3.3 What YAOPLM does (`yaoplm_net::oplm`, `remote_zip`)

- **Index once.** One suffix range for the end records (66 KB: the longest comment a zip can have), one range for the zip64 end record if it is not in that tail, one range for the central directory (8.6 MB, a 300 s timeout). The directory bytes and a small `meta.json` (size, ETag, offsets, the URL that answered) are kept in `art-cache/oplm-index/OPLM_ART_2024_09/`; the next process reads them from disk. Clearing the art cache drops them too.
- **One range per picture.** 30 bytes of local header, the name, 256 bytes of slack for the extra field and the data, in one request (a second one if the extra field is longer). The data must match the directory's size and CRC-32; deflated members are inflated (the 2023 backups use deflate). A mismatch, or a file size that is not the one indexed, means the item changed: the index is deleted and read again next time.
- **Never ask for an absent member.** The index answers "not found" offline and exactly, and maps OPL Manager's numbering before any download: `SCR_00` → SCR, `SCR_01` → SCR2, `BG_00` → BG; a background falls back to `BG_01`+ and then to screenshots OPL shows nowhere else (`SCR_02`+). The game card offers further numbered screenshots as variants (up to four).
- **archive.org is slow and fails now and then.** One connection, a second between requests (`LIMITER = "oplm"`), retries with back-off on 5xx. When the download URL fails, the item's metadata (`archive.org/metadata/OPLM_ART_2024_09`) gives its own servers (`d1`, `d2`, `server`), which are tried next; the URL that answered is asked first from then on. After the index could not be read the source rests ten minutes, so a bulk run of 500 games does not wait on archive.org 500 times.
- **Load.** 8.6 MB once, then about 355 KB per game for all types: three orders of magnitude less than the 5.5 GB a user downloads for the picture-set import (ART-5).
- **Live check** (`crates/yaoplm-net/tests/live.rs`, 2026-10-24) **[verified]**: the first picture, with the index, took 36.6 s; then every type of DOA2 in three regions came in 0.4-1.6 s each, the right size and CRC, through range requests on the `/download/` URL. That is 3-5 times faster than the research measurements of §3.2, but archive.org varies: plan for a bulk run of minutes to hours (500 games × 4 types at 1-5 s). The job shows progress and can be cancelled.
- **Mirror.** A folder with the backup's layout (`PS2/<ID>/<ID>_<T>.png` or `.jpg`): an unpacked backup or a copy of the GitHub re-host. Without the CRC index, the cross-region rule does not apply there.

### 3.4 Precedent [verified]

- PSBBN Definitive Project (`scripts/Game-Installer.sh`, `5999256`, GPL-3) fetches `archive.org/download/OPLM_ART_2024_09/OPLM_ART_2024_09.zip/PS2/${game_id}/${game_id}_COV.png`, `_COV2`, `_LAB` member by member.
- HDL Batch Installer: default art source `https://archive.org/download/OPLM_ART_2023_07/OPLM_ART_2023_07.zip/PS2%2F`, configurable (`HDL_Batch_installerMain.cpp:546`).
- PyOPLM (`storage.py` `index_artwork`): parses the HTML "view contents" listing of a backup, then fetches members.
- OPL PC Tools: `view_archive.php` on a solid 7z (`PS2_OPL_ART_kira.7z`), which makes archive.org decode the 7z for every cover: the costly case ADR-0033 rejected, and still rejected.
- Orbit, PS2-HDD-Manager (L10N37), PS2-MANAGER-FOR-ANDROID: the Luden02 GitHub dump.

### 3.5 Legal

- CLAUDE.md rule 3 forbids OPL Manager's **private art server**. The archive.org backups are files OPL Manager's author published for anyone; reading them is not calling the server.
- No licence is stated, so the backup is used like xlenore (ADR-0007): at run time, when the user asks, cached, never bundled, credited on the About screen.
- archive.org's current Terms of Use page renders with JavaScript and was not readable; a 2010 version says access is "for scholarship and research purposes" **[unverified for the current wording]**. No published per-client rate limit; a 2023 Internet Archive post asks heavy users to throttle **[secondary]**. One connection and a second between requests is well below that.
- Risk: the item can be taken down. The art is fixed at 2024-09.

### 3.6 Derivatives on archive.org [verified metadata]

| Item | Content | Use |
|---|---|---|
| `oplm_ps2_ico_cov` | ICO and COV from OPL Manager in a 325 MB 7z | a subset, per-file reads costly: no |
| `oplm-128x128`, `oplm-256x256` | ICOs upscaled with AI (Upscayl/Remacri) | tempting for RiptOPL, but invented detail: not silently |
| `ps2-3d-art` | 3-D renders credited to xlenore | YAOPLM has xlenore's `covers/3d` already |
| `ps2-opl-cover-art-set` | Kira's 1.9 GB 7z | picture-set import only |
| `hdd-osd-icons-pack`, `art-para-opl`, `art_20250317`, ... | HDD-OSD icons, ad-hoc user packs | not OPL types, or import only |

## 4. ScreenScraper

- **API** `https://api.screenscraper.fr/api2/jeuInfos.php`. Without developer credentials: HTTP 403 "Erreur de login : Vérifier vos identifiants développeur !" **[verified]**.
- **Credentials.** `devid`, `devpassword`, `softname`, requested on the ScreenScraper forum by presenting the program; `ssid`, `sspassword` of a user account are optional **[verified webapi2]**. A guest pool exists (FAQ: "guest threads open : 34 / 256") **[verified]**; 401 means "closed to non-members" while the server is busy.
- **Terms.** "can only be integrated into fully free and distributed applications"; other uses need prior authorisation; abuse leads to suspension; quota handling is mandatory **[verified]**. Content CC BY-NC-SA 4.0 **[verified, research/03]**.
- **Lookup.** `serialnum` forces a search by the disc's serial; `systemeid` 58 is the PS2 (ES-DE `ScreenScraper.cpp`, `SONY_PLAYSTATION_2, 58`) **[secondary]**. The documentation asks for at least one hash with `romtaille` "unless an exception applies"; YAOPLM sends `romtype=iso`, `romnom=<serial>.iso` and `serialnum` **[unverified until the key exists]**.
- **Media types** (ES-DE `ScreenScraper.h`): `box-2D` (COV), `box-2D-back` (COV2), `box-2D-side` (LAB, from the API doc), `support-2D` (ICO), `ss` (SCR), `sstitle` (SCR2), `fanart` (BG), `wheel-hd` and `wheel` (LGO). ES-DE reads the XML answer: `Data/jeu/medias/media` elements with `type`, `region` and `format` attributes and the URL as text; `ssuser` has `requeststoday` and `maxrequestsperday` **[secondary]**.
- **Error codes** **[verified webapi2]**: 400 bad request, 401 closed to non-members, 403 developer login error, 404 not found, 423 API closed, 426 program blacklisted, 429 thread limit, 430 daily quota, 431 too many unknown ROMs today.
- **What YAOPLM does** (`yaoplm_net::screenscraper`): developer credentials from `YAOPLM_SCREENSCRAPER_DEVID`, `_DEVPASSWORD`, `_SOFTNAME` at run time or built into a release (`option_env!`, CI secrets, never in the repository); without them the source is off and shown nowhere in the interface (neither the settings nor the credits). The developer key registers the program, not the user, so there is no field for it. A user account from the settings or `YAOPLM_SCREENSCRAPER_USER`, `_PASSWORD`. One `jeuInfos.php` per serial, cached 30 days ("not found" for seven); media from the URLs it gives, cached like any picture. Packaging only from media of the game's region (`own region` first, then the family's codes; `wor` is not a region); shared types from any region, the game's first, then `wor` and `ss`. 401 pauses the source for ten minutes, 430 and 431 for six hours, 403, 423 and 426 for the session; a spent quota in `ssuser` pauses it too. Credentials never reach the log or the cache (`screenscraper::redact`). The XML parsing, region choice, redaction and pauses are tested against canned answers; nothing has been tried against the live API yet.
- Coverage: 4,414 PS2 games and 33,783 ROMs on ScreenScraper **[verified]**; per type and for the DOA2 regions **[unverified]**.

## 5. Sources not taken

| Source | Why not |
|---|---|
| LaunchBox Games DB (`Metadata.zip` 108 MB daily, images on `images.launchbox-app.com`) | No API, no stated terms; title-keyed (no serial), so packaging would be guessed. Only with LaunchBox's written permission, and then for shared types first. DOA2 entry 3940 has EU and JP front, back, spine and disc **[verified]**. |
| TheGamesDB | API key with a monthly allowance; its serial UIDs are wrong for DOA2 (§2), so they cannot key packaging. |
| IGDB | Twitch client secret cannot ship; no serials. |
| MobyGames | API is a paid feature now **[secondary]**; title-keyed. |
| GameTDB | No PS2 database (`art.gametdb.com/ps2/...` 404) **[verified]**. Two reference tools (`gpulch/ps2-manager` `metadata.rs`, `JO0812/TinyPS2Manager` `art.go`) carry GameTDB PS2 and `images.vgdb.io` URLs that do not work. |
| SteamGridDB | Title-keyed, no PS2 focus. |
| IGN | PSBBN's `art_downloader.py` scrapes IGN pages with a spoofed User-Agent: rejected. |
| Nostlan `nostlan-ps2` | 1,832 US IDs only, AVIF, the DOA2 wrap is the PAL one, Kirkland's scans were licensed to Nostlan; adds 17 US icons beyond OPL Manager. |
| PSBBN art DB (`CosmicScale/psbbn-art-database`) | Square 256×256 key art from IGN, not an OPL type. |
| Luden02 GitHub re-host of the OPL Manager backup | The same bytes, faster (1.25 s), but a re-host without a licence: the product owner decides (§7). |

## 6. Measuring coverage

- `scripts/art-source-lists.py <dir>` writes the file lists of xlenore (one GitHub API request for the tree) and libretro (four directory indexes); `scripts/oplm-backup-toc.py` the table of contents of the backup. They go to `D:\yaoplm-testdata\art-lists\` and `oplm-art\`, never into the repository.
- `YAOPLM_TESTDATA=D:\yaoplm-testdata cargo test -p yaoplm-api --test art_coverage -- --ignored --nocapture` prints the table of §1 with the source that answered first per type. It uses `sources::order`, `sources::candidates` (libretro names and their region filter), `oplm::Index::pictures` with the cross-region flag and `titles::Database::same_title`, as `ArtService::fetch_cover` does; only the files' existence is looked up in the lists instead of asked for.

## 7. Open

- **ScreenScraper key** (the owner registers YAOPLM on the ScreenScraper forum): then measure its coverage per type, check the `serialnum` lookup without hashes, and decide whether to send Redump hashes from the DAT when one is loaded.
- **Luden02 GitHub re-host** as a faster mirror of the backup (owner's decision; every file would be checked against archive.org's CRC).
- **Generated LAB and ICO** from region templates, as PSBBN does (`assets/Icon-templates/PS2_LAB_PAL.png`): would fill every spine and disc, but with made-up pictures; off by default if ever added.
- **LaunchBox**: ask for permission.
- archive.org's current Terms of Use wording on automated access.
- Whether OPL Manager's COV2 for Japanese DOA2 is the Japanese back cover (not viewed).
- Byte identity catches only copies: a wrong-region picture that is not byte-identical to another ID's is not caught.
- **Serial-keyed sources can be wrong too** (walkthrough of 2026-10-24) **[verified]**: xlenore's `covers/default/SLES-50630.jpg` (Dragon Rage, PAL) is the US cover with the ESRB "T" badge, the same release OPL Manager's backup files under that ID. The policy trusts an ID or serial match, so it takes it; only looking at the picture would tell. A possible hint: when OPL Manager's copy of a cover is flagged as another region's, the other sources' cover for that ID is suspect as well.
