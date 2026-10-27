# 03 — Data sources and API integrations for an open-source PS2 / OPL library manager

- **Research date:** 2026-10-22
- **Method:** WebSearch and WebFetch only. No binaries were downloaded or run. Two text/data files were cached automatically by WebFetch during the research and were not opened or executed: PCSX2 `GameIndex.yaml` (a 2.6 MB text file, searched with grep only) and a Redump `serial,version` DAT zip (1.3 MB, not extracted).
- **Legend:** **[verified]** = I read it on the primary page or in the primary file. **[secondary]** = it comes from third-party docs, code or forum posts. **[unverified]** = I could not confirm it (page blocked or unreachable, or the claim is from memory).
- **Not legal advice.** Box art and cheat compilations sit in a grey zone. The aim below is to stay "clean by construction": fetch on the user's behalf, cache locally, credit sources, and never re-host anything whose license is unknown.

---

## 0. TL;DR

1. **Identify locally first.** The game ID comes from `SYSTEM.CNF` (`BOOT2 = cdrom0:\SLUS_200.62;1`) inside the ISO/ZSO/UL/HDL image. This needs no network and is exactly what OPL itself does.
2. **Best permissively licensed metadata:**
   - **PCSX2 `GameIndex.yaml`** (GPL-3.0 repo, about 12.8k serials, names and regions).
   - **libretro-database Redump-derived PS2 DAT** (CC BY-SA 4.0 repo LICENSE, with serials and hashes).
   - Both can be bundled with attribution. Use GPL-3.0 for the app to keep this simple.
3. **Dump verification:** Redump has **moved to `redump.info`** (announced 2026-06-20). The `serial,version` DAT at `https://redump.info/datfile/ps2/serial,version` was downloadable without login on 2026-10-22 (11,844 entries per the filename). Redump states no license or ToS, so fetch at runtime at a low rate and don't bundle.
4. **Covers keyed by serial:**
   - **xlenore/ps2-covers** (`covers/default/<SERIAL>.jpg`, about 10.2k covers; this is what PCSX2's cover downloader uses). No license file.
   - **libretro-thumbnails** (keyed by Redump/No-Intro name, about 8.5k boxarts plus snaps, titles and logos). No license file; the README credits the art to publishers.
   - Fetch both on demand and never bundle images.
5. **OPL Manager's art server is private.** It has no public API, no ToS, and needs OPLM V23+. **Do not call it.** Its author does publish **full monthly ART DB backups on archive.org** (2018-11 to 2024-09, about 5.5–6.3 GB zip, no license stated). The legit way to use them is "user downloads the zip, the app imports it locally".
6. **ScreenScraper** is the richest API: box front/back/spine/3D, disc, logo, fanart, and it can look up by **serial** (`serialnum`) and by hash.
   - It needs **developer credentials** (requested on their forum) plus each user's own account.
   - It is free-software-only and has per-user quotas.
   - Site content is CC BY-NC-SA 4.0.
   - Use it as an optional "power" source, not the MVP default.
7. **TheGamesDB / IGDB / MobyGames:**
   - All are title-keyed with no serials.
   - All need keys, and IGDB's secret can't be shipped in a desktop app. MobyGames' API is now (reportedly) paid.
   - All are optional "bring your own key" (BYO-key) extras only.
   - **GameTDB** has no PS2 [unverified]. **SteamGridDB** is not relevant.
8. **OPL compatibility data:**
   - Best machine-readable source is `ps2homebrew/Open-PS2-Loader-Compatibility-list` (per-serial Markdown files).
   - Also `GDX-X/PS2-OPL-CFG-Compatibility-Database` (about 70 ready-made `$Compatibility=` CFGs).
   - Neither has a license file, so fetch at runtime and label the data "community reports".
9. **Cheats/widescreen:** the CRC "mapping problem" is solvable.
   - The PCSX2 "CRC" is just an XOR fold of the boot ELF's 32-bit words. The app can compute it from the ISO and match `pcsx2_patches/<SERIAL>_<CRC>.pnach` exactly.
   - Simple `patch=` lines convert to RAW `0/1/2aaaaaaa` codes for OPL's PS2RD engine.
   - `pcsx2_patches` has **no license file**, so this is a "later, after asking upstream" feature.

---

## 1. What the app must produce (OPL conventions, verified in OPL source)

| Item | Convention | Evidence |
|---|---|---|
| Game ID | `SLUS_200.62` (4 letters, `_`, 3 digits, `.`, 2 digits). OPL reads it from the "old" filename format `SLUS_200.62.Title.iso` (checks `_` at index 4, `.` at 8 and 11) or, for new-format names, from `SYSTEM.CNF` via `GetStartupExecName("iso:/SYSTEM.CNF;1")` | `src/supportbase.c` **[verified]** |
| Serial normalization | Redump, PCSX2 and covers repos use `SLUS-20062`. OPL uses `SLUS_200.62`. Normalize both ways. Internal (ELF) serial can differ from the box serial (Redump forum thread title, content unreachable) | **[secondary]** |
| ART path | `<device>ART/<GAMEID>_<TYPE>.png`. Built from `"%s%s/%s_%s"` (prefix, folder `ART`, value = startup ID, suffix) + `texDiscoverLoad` appends `.png` | `src/bdmsupport.c`, `src/themes.c`, `src/textures.c` **[verified]** |
| ART format | Current OPL master loads **PNG only** (no JPG/BMP). Max 1024×1024 and ≤ 720·512·4 bytes texture. 8-bit palettized (T8) and 4-bit PNG are supported natively | `src/textures.c` **[verified]** |
| ART types/sizes (OPL Manager spec, PS2) | COV 140×200 · COV2 (back) 242×344 · LAB (spine) 18×240 · ICO (disc) 64×64 (OPLM V24 raised it to 128×128) · SCR/SCR2 250×188 · BG 640×480 · LGO 300×125. OPLM V23 asks for 8-bit PNG "for better performance". COV3 (3-D cover) is a theme-community convention | oplmanager.com docs + changelog **[verified]**; COV3 **[secondary]** |
| Per-game CFG | `CFG/<GAMEID>.cfg` (`"%sCFG%s%s.cfg"` with `game->startup`). Keys seen in the wild: `Title=`, `$Compatibility=`, `$ConfigSource=`, `Modes=` (informational) | `src/supportbase.c` **[verified]**; keys from GDX-X repo **[verified]** |
| Compatibility modes (bitmask for `$Compatibility`) | 1 = Accurate Reads (0x01), 2 = Synchronous / alt read (0x02), 3 = Unhook Syscalls (0x04), 4 = Skip Videos (0x08), 5 = Emulate DVD-DL (0x10), 6 = Disable IGR (0x20). Bits 0x40/0x80 unused. Older OPL had other modes 7/8, so mode numbers differ across versions | `include/iosupport.h`, `lng_tmpl/_base.yml` **[verified]** |
| Cheats | `CHT/<name>.cht` (`"%sCHT/%s.cht"`, buffer 64 chars). Which name the caller passes (likely the game ID) is **[unverified]**. Format is PS2RD text: any non-code line is a cheat name. Code lines are `XXXXXXXX YYYYYYYY`. `//` and `#` start comments. Max line length 255. In "select" mode master codes are always on | `src/supportbase.c`, `src/cheatman.c` (GPL-3 PS2rd code) **[verified]** |
| OPL compat network update | OPL ≥ 0.9.3 has `compatUpdate()`, which fetches records per (startup, device) from the OPL-CL service (`OPL_COMPAT_HTTP_HOST/URI` macros). The OPL-CL site `sx.sytes.net/oplcl` did not respond during research; **it answered on 2026-10-27** (records of 2015, research/21 §1.2) | `src/opl.c` **[verified]**; site **[verified 2026-10-27]** |
| OPL license | Academic Free License 3.0. Only matters if we reuse OPL code (e.g. `pc/ziso.py`), not for writing compatible files | README **[verified]** |

---

## 2. Cover-art sources

### 2.1 OPL Manager (OPLM) art server

- **What it is:** OPLM (closed-source Windows .NET app by "danielb"; latest V24, 2024-09-15) has its own ART database server.
  - The home page says: "Versions older than V23 are unsupported and wont be able to download/upload ART!". So the server is version-gated to the OPLM client.
  - The changelog mentions a "new server communication protocol" (v15).
  - Separately, v19 "removed the obsolete option to download ART from the OPL server", meaning the old *OPL project* art server. **[verified]**
- **Public API / ToS:** none documented anywhere. The docs say only that non-conforming art "will be deleted from the server". **[verified: absence]**
  - **Verdict: do not integrate or reverse-engineer the OPLM server.** This is exactly the "abusing someone's private server" case.
- **Official backups:** `oplmanager.com/site/?backups` lists 44 monthly **"entire OPLM ART database backups"** (PS1 + PS2 ICO/COV/…), from 2018-11 to **2024-09**. Each is an archive.org item `OPLM_ART_YYYY_MM`. **[verified]**
  - Example `OPLM_ART_2023_11`: uploader `danielb7390`, collection "Community Data", about 5.9 GB zip plus torrent. `OPLM_ART_2024_09` is about 5.5 GB.
  - No license field, and no reuse statement either way. **[verified]**
  - Community derivatives also exist on archive.org: `oplm-128x128`, `oplm-256x256`, `oplm_ps2_ico_cov` (about 325 MB), `8bitpng-oplm-art-2023-11.7z`, `ps2-3d-art`. Uploaders and contents **[unverified]**.
  - Existing OSS precedent: **pyoplm / libopl** (Nold360, GPL-3.0) use these backups. You point the tool at a locally unzipped copy or your own static file server; it doesn't contact the OPLM server. **[verified]**
  - **Verdict:** the most OPL-native art set (all 7 types at OPL sizes), but with unknown licensing. Offer "Import OPLM ART backup (.zip)", where the user downloads it from archive.org themselves. Do not re-host it. Reading straight from archive.org's "view contents" zip index (as pyoplm does) is possible, but those are 5–6 GB items on a non-profit's bandwidth, so prefer local import.
- **Other community packs:** the psx-place "Game Compatibility Lists" and art threads (e.g. a "1601 HiRes covers" pack) were blocked (403) for me. Status and licensing **[unverified]**.

### 2.2 xlenore/ps2-covers (GitHub)

- 2D covers at `covers/default/<SERIAL>.jpg` and 3D covers at `covers/3d/<SERIAL>.png`, keyed by **serial** (`SLUS-20062` style, the form PCSX2 uses). **[verified README]**
- README stats show about **10,215 covers out of about 12,838 serial entries** (it's per-prefix, e.g. SLES 2920/3073).
- **This is the URL template PCSX2's built-in Cover Downloader (v1.7.3329+) tells users to use:**
  - `https://raw.githubusercontent.com/xlenore/ps2-covers/main/covers/default/${serial}.jpg`
  - `https://raw.githubusercontent.com/xlenore/ps2-covers/main/covers/3d/${serial}.png`

  **[verified]**
- Credits pcsx2.net, psxdatacenter.com, gvcover.top, imkira3 and waifu2x (upscaled). **No LICENSE file** (raw `LICENSE` returned 404). Resolution not stated. **[verified]**
- **Verdict: MVP cover source** (serial-keyed, no auth, already used this way by PCSX2). Fetch on demand, cache, resize to 140×200 and PNG-encode. Throttle and don't bundle. Consider opening an issue to tell the maintainer about the integration.

### 2.3 libretro-thumbnails (`libretro-thumbnails/Sony_-_PlayStation_2`)

- **Folders:** `Named_Boxarts`, `Named_Boxarts_1024`, `Named_Snaps`, `Named_Titles`, `Named_Logos`. The GitHub listing shows about **8,500 entries in `Named_Boxarts`**. **[verified]**
- **Naming:** the playlist label, which is the **Redump/No-Intro-style name** (e.g. `007 - Agent Under Fire (Europe) (En,Fr,De,Es,Nl,Sv).png`). Characters `& * / : \` < > ? \ | "` are replaced with `_`. PNG only, guideline width ≤ 512 px (the actual files are often 400–600 KB). RetroArch ≥ 1.17 also falls back to the "short name" (text before the first `(`). **[verified]**
- **Fetching:**
  - Server: `https://thumbnails.libretro.com/Sony%20-%20PlayStation%202/Named_Boxarts/<url-encoded name>.png`. Plain Apache index, refreshed from GitHub periodically (README: about every 2 days, contributions show up in 1–2 weeks), behind Cloudflare. **[verified]**
  - Raw GitHub: `https://raw.githubusercontent.com/libretro-thumbnails/Sony_-_PlayStation_2/master/Named_Boxarts/<name>.png`. Pattern **[inferred, not individually tested]**.
- **License:** no LICENSE file in the umbrella or PS2 repo that I could find. The README's credits say game art belongs to its developers and publishers. **[verified absence]**
- **Mapping serial → name:** use the Redump `serial,version` DAT or the libretro-database PS2 DAT (§3.1/3.2). Expect several names per serial (revisions, multi-language) and pick the region-matching one.
- **Media mapping:** Boxarts → COV, Snaps → SCR, Titles → SCR2, Logos → LGO. No back cover, spine or disc.
- **Verdict: MVP secondary source** (adds screenshots and logos). On-demand only.

### 2.4 ScreenScraper.fr (API v2)

- **Auth:** `devid` + `devpassword` + `softname` (developer/app credentials, **requested by presenting the software on the ScreenScraper forum**) plus `ssid` / `sspassword` (each end user's own free account). **[verified]**
  - RomM documents that official builds embed dev creds but source builds don't. Without them you get HTTP 403. **[secondary]**
  - Open-source apps therefore either ship lightly obfuscated dev creds (ES-DE, Skyscraper style) or ask packagers/users for them.
- **Terms:**
  - "The API may only be integrated into fully free, distributed applications". Other uses need prior authorization.
  - Violations can lead to suspension or legal action.
  - v2 is "beta" and may change without notice. **[verified]**
  - The site footer licenses content **CC BY-NC-SA 4.0** **[verified]**. So: non-commercial, attribution, share-alike. Don't bundle scraped data into a GPL repo; keep it in the user's cache.
- **Limits:**
  - Per-user quotas are returned in `ssuser`: `maxthreads`, `maxrequestspermin`, `maxrequestsperday`, `maxrequestskoperday`, `requeststoday`, `maxdownloadspeed`.
  - Errors: 429 (threads), 430 (daily quota), 431 (too many unknown ROMs), 401 (API closed to non-members under load), 423 (API closed), 426 (software blacklisted).
  - You get more threads and quota by contributing or donating (Patreon/Tipeee). **[verified]**
  - Exact free-tier numbers **[unverified]** (FAQ not readable).
- **Lookup:** `jeuInfos.php` takes CRC/MD5/SHA1, `romnom`, `romtaille`, `systemeid`, `romtype` (`rom` | `iso` | `dossier`) and **`serialnum`** ("force search by the ISO's serial"). Responses include `romserial`. `jeuRecherche.php` does name search (top 30). **[verified]**
  - PS2 `systemeid` = **58** **[secondary: ES-DE source]**.
- **Media:**
  - box-2D (front), box-2D-side (spine), box-2D-back, box-3D, box-texture, support-2D (disc), support-texture, ss, sstitle, wheel / wheel-hd (logo), fanart, marquee, video, manuel (PDF), flyer, bezels. Each has a region suffix (`(wor)`, `(eu)`, `(us)`, `(jp)`…). **[verified]**
  - This maps to all 7 OPL types: COV = box-2D, COV2 = box-2D-back, LAB = box-2D-side, ICO = support-2D, SCR = ss, SCR2 = sstitle, BG = fanart, LGO = wheel.
- **Verdict: phase-2 optional source.** It is the richest legitimate API and can key by serial. Requires: a dev-credential request, per-user login, strict quota handling (read `ssuser` and back off on 429/430), a single thread by default, and an "ScreenScraper.fr" attribution in the UI.

### 2.5 TheGamesDB (TGDB)

- **API:** v1 at `api.thegamesdb.net` (the Swagger UI didn't render for me). The legacy API is shut down. **[secondary]**
- **Keys:** request one on the site after logging in (`api.thegamesdb.net/key.php`: "You must be logged in…"). There is an "increase usage" page and a Patreon. **[verified]**
  - Wrapper libraries mention a "lifetime private key" vs a "public key". **[secondary]**
- **Limits:** monthly allowance with `remaining_monthly_allowance` / `extra_allowance` / `allowance_refresh_timer` in responses **[secondary]**. A RetroPie thread quotes about 2,000 requests/month per IP (about 2 per game) **[secondary/unverified]**. IP blocks have been reported after exceeding it.
- **Data:** PS2 is platform id **11** **[verified]**. Images: boxart front/back, fanart, banner, screenshot, clearlogo, titlescreen **[unverified, from memory]**. No spine or disc. **No serial lookup** (title search only) **[unverified]**.
- **License:** website code GPL-3.0 (GitHub). Data/image license **[unverified]**.
- **Verdict:** optional BYO-key fallback for back covers and logos. Low priority because of the tight allowance.

### 2.6 IGDB (Twitch)

- **Auth:** a Twitch developer app ("Confidential" client) with Client ID + Client Secret, and an OAuth2 client-credentials app token. The account needs 2FA. **[secondary: RomM docs, Twitch forum]**
- **Limits:** 4 requests/s and a concurrency cap (8 open requests per aggregators; one forum post says 4) **[secondary]**.
- **Terms:** free for non-commercial use under the Twitch Developer Services Agreement. Commercial use needs a partnership and visible attribution **[secondary]**. Official docs (`api-docs.igdb.com`) returned 403 to me **[unverified]**.
- **Desktop/open-source problem:** the client secret must stay server-side. Options: (a) **each user brings their own Twitch credentials** (the RomM pattern), or (b) we run a caching proxy. That breaks the "no private server" spirit and costs money, so option (a) only.
- **Data:** cover, screenshots, artworks, game localizations/release dates by region **[unverified]**. **No serials** and no back/spine/disc **[unverified]**.
  - **Hasheous** (`hasheous.org`, AGPL-3.0, gaseous-project) is a public hash-lookup service that proxies IGDB. Its ToS and PS2 coverage are **[unverified]**.
- **Verdict:** optional BYO-key source for descriptions, genres and release dates. Not an MVP item.

### 2.7 MobyGames

- The official API page was 403 to me. A search snippet of it says: free key for not-for-profit use from your profile, 360 requests/hour, ≤ 1 request/s, commercial users must contact them. **[secondary]**
- RomM docs (current) say "API access is a paid feature" and recommend ScreenScraper instead. MobyPro (about $5/month) gives "2× hourly rate limit" on the non-commercial API. **[secondary, conflicting]**
- Data: cover groups (front/back/media/spine by region) and screenshots. Title-keyed **[unverified]**.
- **Verdict: skip.** At most a BYO-key extra later.

### 2.8 GameTDB

- Covers Wii, GameCube, Wii U, DS/3DS, PS3 and Switch, keyed by 6-character game ID. **I found no PS2 database** (site FAQ returned 403) **[unverified]**.
- **Verdict:** not applicable.

### 2.9 LaunchBox Games Database

- **No official API.** A full `Metadata.zip` (Metadata.xml, about 190 MB uncompressed per a moderator) is published about daily at `gamesdb.launchbox-app.com/Metadata.zip`. **[secondary: LaunchBox forum]**
- LaunchBox admin Jason Carr confirms the zip includes image filenames that can be turned into image URLs, and that "other apps already use" it. He gave no permission statement or terms. **[verified forum thread]**
- RomM downloads the whole file locally and matches by exact filename **[verified RomM docs]**.
- Data: rich PS2 coverage (front/back/spine/3D box, disc, clear logo, screenshots, fanart) **[unverified]**. No serial key.
- **ToS/license: not found [unverified].**
- **Verdict:** technically attractive (one bulk download, no per-game API calls) but terms are unclear. Ask LaunchBox before integrating. Not MVP.

### 2.10 SteamGridDB

- Free API key per account. Covers grids, heroes, logos and icons for any game, including non-Steam ones, but searched by title or Steam/other store IDs. No serials and no PS2 box-art focus.
- A LaunchBox tracker issue says SteamGridDB asked them not to integrate the then-current API **[secondary, undated]**. ToS page didn't render **[unverified]**.
- **Verdict:** not relevant except perhaps for BG or LGO via a manual "search art" dialog with the user's own key.

### 2.11 Others

- **The Cover Project** (full-wrap print scans): it reportedly added human verification. Nostlan shipped pre-built image databases instead **[secondary]**. Don't scrape it.
- **psxdatacenter.com:** PS2 serial/title/language lists and detail pages with covers. No terms visible (frameset) **[verified absence]**. It's a hobby site, so don't scrape it. Its data already flows into PCSX2 GameIndex and xlenore covers.

---

## 3. Game identification and metadata

### 3.1 Redump (now `redump.info`)

- **Migration:** Redump moved from redump.org to **redump.info** (announcement 2026-06-20). The reason given was the old site's infrastructure (no HTTPS) and an unreachable admin. redump.org did not respond during research. **[verified]**
- **Downloads page:** `/datfile/PS2` (updated 2026-10-22 08:36), `/cues/PS2`, `/bios/PS2`. **[verified]**
  - A July 2026 news item adds a "Database" section with "a backup of the entire disc database, updated daily". Format and license **[unverified]**.
- **Serials:** the plain DAT has **no serials** (per ps-disc-detect, GPL-3) **[secondary]**. Appending `serial,version` to the path adds `<serial>` and `<version>` elements (wiki, flagged "out of date") **[verified wiki]**.
  - `https://redump.info/datfile/ps2/serial,version` returned a zip named `Sony - PlayStation 2 - Datfile (11844) (2026-10-22 08-36-39).dat` with **no login prompt** **[verified]**.
  - On old redump.org that variant reportedly needed a logged-in dumper account (Dolphin issue #12183) **[secondary]**. That may change again on the new site.
- **Format:** Logiqx XML (`<datafile><header>…</header><game name><category/><description/><serial/><version/><rom name size crc md5 sha1/></game>`) **[secondary; element names not inspected]**.
  - PS2 DVD games are a single `.iso`. CD games are `.bin/.cue` (one or more tracks), so verifying an OPL CD-ISO against the Redump `.bin` works only for single-track Mode-2 dumps after conversion. Plan for that.
  - Multiple serials per entry exist ("SLUS-20062, SLUS-20062GH" style) **[unverified]**.
- **Terms:** none found on the new site (no footer terms) **[verified absence]**. Automated download by tools is common (ROMSearch, Datoso, ps-disc-detect), and Datoso advises about 1 s between requests and ETag/date checks.
- **Verdict:** **MVP for dump verification**, fetched at runtime:
  - at most once a day, and only when the user asks or the cache is older than N days;
  - with `If-Modified-Since`;
  - with a polite User-Agent;
  - credited as "Redump.info".
  - Don't bundle (no license), or use 3.2 as the bundled fallback.

### 3.2 libretro-database: `metadat/redump/Sony - PlayStation 2.dat`

- clrmamepro format, about 4 MB, `version "2026.10.07"`, generated by robloach/libretro-dats from Redump. **[verified]**
- Each entry has `name`, `region`, `serial` (most entries, not all; e.g. protos and Action Replay discs lack one) and `rom ( name size crc md5 sha1 serial )`. Duplicate entries exist for alternate serials (`SLPM-65002` / `SLPM-65002-0`). **[verified]**
- Repo LICENSE is **CC BY-SA 4.0** **[verified]**. The data originates from Redump, which states no license.
- **Verdict: bundleable fallback** with attribution ("Redump via libretro-database, CC BY-SA 4.0"). Share-alike applies to our redistributed copy of that file. It also gives the serial → Redump name map needed for libretro-thumbnails.

### 3.3 PCSX2 `GameIndex.yaml`

- **Location:** `bin/resources/GameIndex.yaml`, raw at `https://raw.githubusercontent.com/PCSX2/pcsx2/master/bin/resources/GameIndex.yaml`. **2.6 MB** YAML. **[verified]**
- **Size:** about **12,830 top-level serial keys** of form `ABCD-12345` (grep count) **[verified]**.
- **Fields:**
  - `name`, `name-sort`, `name-en` (romanized/English for JP titles), `region` (`NTSC-U`, `NTSC-J`, `PAL-E`, `NTSC-Unk`…)
  - `compat` (PCSX2 **emulator** rating: 0 Unknown, 1 Nothing, 2 Intro, 3 Menu, 4 In-game, 5 Playable, 6 Perfect; this is *not* OPL compatibility)
  - `roundModes`, `clampModes`, `gameFixes`, `speedHacks`, `gsHWFixes` (emulator-only)
  - `memcardFilters` (lists related serials, a useful "same game family" hint)
  - `patches` **keyed by CRC** inside the serial entry: 215 CRC blocks, e.g. `SLUS-20064 → FD9CD8FC`
  - `dynaPatches`

  **[verified]**
- **Header credits:** sonyindex.com (archived), serialstation.com, psxdatacenter.com, redump.org and the PCSX2 team. **[verified]**
- **License:** the PCSX2 repo is GPL-3.0 (`COPYING.GPLv3`). The YAML has no separate license header **[verified]**. Treat it as GPL-3.0 data.
- **Verdict: MVP serial → title/region database.** It can be bundled if the app is GPL-3.0-compatible (recommended), with credit. Otherwise download at first run and refresh from the raw URL with ETag.

### 3.4 Other serial → title sources

| Source | Notes | Verdict |
|---|---|---|
| psxdatacenter.com PS2 lists (`/psx2/ulist2.html` etc.) | Serial, title and language flags per region, plus INFO pages with covers. No terms or license shown | Don't scrape; already reflected in GameIndex |
| serialstation.com | Cited by PCSX2. Returned 403 to me. API/terms **[unverified]** | Investigate later |
| Tom-Bruise/PS2-OPL-CFG-Database (fork of VTSTech/Veritas83 PS2-OPL-CFG) | **13,620 CFG files** with titles plus ScreenScraper-scraped description, genre, players, release and developer (6.3k with descriptions). **GPL-3.0** | Useful reference, but the GPL label on CC BY-NC-SA ScreenScraper text is a licensing conflict. Don't bundle the descriptions |
| OPLM title lookup | OPLM 16.0 added title lookup "from the database" (server) | Private, don't use |

---

## 4. OPL compatibility lists

| List | Where | Format / machine-readable? | Coverage / freshness | License | Verdict |
|---|---|---|---|---|---|
| **ps2homebrew/Open-PS2-Loader-Compatibility-list** | GitHub + Pages (`ps2homebrew.github.io/Open-PS2-Loader-Compatibility-list/`) | One Markdown file per game at `List/<Letter>/<GAMEID>.md` (e.g. `List/G/SCUS_973.99.md`). A table of device (USB/HDD/SMB/MX4SIO…), image format (ISO/ZSO), OPL build, status, notes, console model and reporter. Jekyll-rendered. **Semi-structured, parseable** | 245 commits, 19 open issues, 12 open PRs (active). Entry count **[unverified]** | **No LICENSE found** | **MVP source for "known issues / reported working"**, fetched at runtime (git tree API or raw files), labelled community data. Ask the maintainers for a license or JSON export |
| **GDX-X/PS2-OPL-CFG-Compatibility-Database** | GitHub | Ready-made OPL CFGs (`HDD/SLUS_219.55.cfg` …) with `Title=`, `$Compatibility=<bitmask>`, `Modes=2+1+3`, `$ConfigSource=`. **Fully machine-readable** | About 68–70 games (Games_List.txt). Updated Oct 2024. Used by PFS BatchKit Manager | **No LICENSE found** | **MVP "suggest modes"** for these games (show and let the user apply; never auto-write) |
| Algol's Google Sheet ("OPL-BDM… Compatibility List") | `docs.google.com/spreadsheets/d/1MLEw7TIKvn9MCwd5Ptbqh2lkUj3lX6YQ2wPgLEx8Koo` | Tabs per device (HDD Int., ETH/SMB, USB iso & ul, DS3/BT, DS3/USB). Columns: TITLE, ID, Media, sizes, Device, **Compatibility modes** (e.g. `1`, `3+6`), DMA mode, VMC/IGR/GSM/PS2RD, Wide Screen, comments, revision ticks, date. Machine-readable via the Sheets CSV export **[untested]** | Last visible entry 30/12/2024. Mostly French comments | None stated | Phase 2. Ask the owner before ingesting |
| ZSO list, MX4SIO list (Google Sheets) | Linked from the psx-place thread (403 to me) | Sheets | **[unverified]** | None | Phase 2 |
| OPL-CL (`sx.sytes.net/oplcl`) | Linked from the OPL README; OPL's built-in "network update" targets it | Was per-(serial, device) HTTP records | Didn't respond during research; **alive on 2026-10-27**: HTTP 200, CFG fragments per (ID, device), records of 2015, launchers up to OPL 1.0.0 (research/21 §1.2) | None | Private server: at most the per-game GET OPL itself makes, on the owner's decision (CFG-3 plan, question 1); never scrape the list |
| El_isra's list | Original GitHub Pages "dead"; mirror at `ps2homebrew.org/Open-PS2-Loader-Compatibility-list/` (didn't respond) | — | **[unverified]** | — | Probably superseded by the ps2homebrew repo |
| "ps2wiki" | Not found | — | — | — | — |
| ConsoleMods wiki / PS2TV list | ConsoleMods returned 403. It says OPL targets "~286 games" on the PS2TV internal list **[secondary]** | Wiki prose | — | Wiki license **[unverified]** | Reference only |

**Suggestion engine design (MVP):**

1. If the game has a GDX-X CFG, propose that `$Compatibility` value.
2. Otherwise, mine the ps2homebrew Markdown and Sheets for reported modes on the user's device type.
3. Show the evidence (who reported it, which OPL build, which device) and let the user decide.
4. Store the decision in the user's `CFG/<GAMEID>.cfg` only on confirmation.

---

## 5. Cheats and widescreen patches

### 5.1 Target format (OPL)

- OPL's cheat engine is PS2RD (GPL-3). The `.cht` text format is: cheat-name lines plus `AAAAAAAA VVVVVVVV` lines of **decrypted RAW** codes. Comments are `//` and `#`. **[verified source]**
- OPL can't decrypt CodeBreaker v7 or ARMAX codes, so anything encrypted must be converted to RAW first.
  - A GPL-3 converter library `ps2-codes` (npm) exists **[secondary]**.
  - Converting encrypted/ARMAX codes is out of scope for the MVP.

### 5.2 PCSX2 patch sources

- **PCSX2/pcsx2_patches** ("Patches for Widescreen, No Interlace etc"; PCSX2 bundles it as `patches.zip`):
  - About **4,700 `.pnach` files** named **`<SERIAL>_<CRC>.pnach`** (e.g. `SCES-50000_5BBC2F40.pnach`). A few are CRC-only.
  - Each file has `gametitle=`, `[Section]` groups, `author=`, `description=`, `gsaspectratio=` / `gsinterlacemode=` directives and `patch=` lines.
  - **No LICENSE file.** The README says they "only supply the patches", the patches belong to their original forum authors, and PCSX2 doesn't create or fix them.
  - Gentoo packages it (license field not readable).

  **[verified]**
- **PCSX2 GameIndex.yaml `patches:`:** 215 CRC-keyed blocks (mostly fixes, not widescreen). GPL-3.0 repo. **[verified]**
- **KrystianLesniak/pcsx2_patches_list:** MIT-licensed index or list (Markdown + Pages) of which games have WS/NI patches. **[verified]**
- `xs1l3n7x/pcsx2_widescreen_patches`: 404, gone or renamed. `PCSX2_Cheats`-style repos: none with a clear license found **[unverified]**.
- **pnach syntax** (pcsx2.net docs) **[verified]**:
  - `patch=<place>,<cpu>,<addr>,<type>,<data>`
  - place: 0 = once at boot, 1 = every vsync, 2 = both, 3 = boot + on enable
  - cpu: `EE` | `IOP`
  - type: `byte|short|word|double|extended|beshort|beword|bedouble|bytes`
  - plus `dpatch=` (pattern-matched; needs the recompiler)
  - `extended` means "RAW codes" (subset; `9…` and `C…` unsupported)

### 5.3 Solving the serial ↔ CRC mapping problem

- PCSX2's "CRC" is **not** a CRC32. `ElfObject::GetCRC()` **XORs all 32-bit little-endian words of the boot ELF** (trailing bytes ignored). **[verified: `pcsx2/Elfheader.cpp`]**
- So the app can compute it itself:
  1. Open the ISO (or ZSO/UL/HDL): ISO9660 → `SYSTEM.CNF` → `BOOT2` path.
  2. Read that ELF.
  3. XOR-fold it, giving e.g. `5BBC2F40`.
  4. Look up `<SERIAL>_<CRC>.pnach`.
- This also distinguishes revisions and regions that share a serial. No external mapping database is needed.

### 5.4 Conversion pnach → OPL RAW (feasibility)

- `patch=*,EE,aaaaaaaa,word,v` → `2AAAAAAA VVVVVVVV` (32-bit write; take the low 25–28 address bits, set the top nibble to 2).
  - `short` → `1AAAAAAA 0000VVVV`
  - `byte` → `0AAAAAAA 000000VV`
  - `double` → two word writes
  - `extended` → copy as-is (it is RAW already)
  - **[standard RAW code types; matches PCSX2's "extended = RAW" note]**
- **Not convertible:**
  - `IOP` patches, `dpatch`, `bytes`, `be*` types
  - Emulator directives (`gsaspectratio`, `gsinterlacemode`, which are hints to PCSX2's renderer)
  - Anything relying on PCSX2 renderer hacks
- **Semantic risk:** PS2RD applies codes repeatedly from its hook. PCSX2 `place=0` patches are one-shot at entry point. Games that load overlays or modules over the patched addresses can crash. Community OPL widescreen packs (e.g. `madmodder123/OpenPS2Loader_Widescreen_Cheats`, license **[unverified]**) often rewrite them as conditional `D/E` codes.
- Some games need a hook or master code (`9…`) for PS2RD **[unverified per-game]**.
- **Verdict:** feasible as an **experimental "convert widescreen patch" feature**:
  - EE-only, simple types only;
  - show the original author;
  - default off;
  - with a "tested on hardware?" flag.
  - Before shipping any converted pack, ask the PCSX2 team and patch authors about redistribution. Without a license, fetch at runtime and convert locally per user.

### 5.5 CodeBreaker / Action Replay code databases

| Source | Keying | Format | Terms | Verdict |
|---|---|---|---|---|
| Codetwink (official CodeBreaker site, CodeMasterX) | Title + game ID per page | CB codes. A PCSX2 guide says they need `CBcrypt` conversion | **[unverified]**. Registration required | Don't scrape |
| root670/CheatDevicePS2 `CheatDatabase.txt` | **Quoted title only, no serial/region** | Decrypted RAW, with "Enable Code (Must Be On)" master-code blocks. About 6.4 MB | Repo GPL-3.0 (archived 2023). Database provenance not stated | Possible import source, but title-keyed (needs fuzzy match to GameIndex). Licensing of the compilation is unclear |
| GameHacking.org | Per game | Many formats, with a converter | **[unverified]** | Ask first |
| Datel Codejunkies (AR MAX) | — | Encrypted ARMAX | Proprietary | No |
| libretro-database `cht/` | — | No PS2 folder exists | CC BY-SA | n/a |

**Cheats verdict:**

- **MVP:** a `.cht` editor and import (the user brings files), plus validation of PS2RD syntax.
- **Later:** a widescreen converter from pcsx2_patches via local CRC.
- **Never:** bundle third-party cheat compilations whose license is unknown. Cheat codes themselves are arguably facts, but compilations can carry EU database rights.

---

## 6. Comparison table

| Source | Data | Key by | Auth | Limits | License / ToS | Verdict |
|---|---|---|---|---|---|---|
| **OPLM art server** | All 7 OPL ART types, titles | Game ID | OPLM client ≥ V23, private protocol | n/a | No public API or ToS | **Do not use** |
| **OPLM ART backups (archive.org)** | All 7 ART types, PS1+PS2, OPL sizes | Game ID (folder/file names, structure **[unverified]**) | None (archive.org) | 5.5–6.3 GB zip | No license stated; IA ToS | Later: **user-initiated local import** only |
| **xlenore/ps2-covers** | 2D front (jpg), 3D (png) | **Serial** (`SLUS-20062`) | None | GitHub raw rate limits **[unverified]** | No LICENSE; credits psxdatacenter and others | **MVP** cover source, on demand |
| **libretro-thumbnails PS2** | Boxart (512 and 1024), snaps, titles, logos | Redump/No-Intro name | None | Cloudflare / GitHub | No LICENSE; art © publishers | **MVP** secondary, on demand |
| **ScreenScraper v2** | Front/back/spine/3D box, disc, ss, title, logo, fanart, video, manual, text metadata | Hash, name, **serial** (`serialnum`), system 58 | Dev ID/pass (forum request) + user account | Per-user threads/min/day quotas (429/430/431) | Free-software-only API; site CC BY-NC-SA 4.0 | **Phase 2** optional (user login) |
| **TheGamesDB** | Front/back box, fanart, banner, screenshot, clearlogo **[unverified]**, metadata | Title (PS2 = platform 11) | API key (site account) | Monthly allowance (about 2k per IP reported) | Data license **[unverified]** | Phase 3, BYO key |
| **IGDB** | Cover, screenshots, artworks, rich metadata | Title / IGDB id | Twitch OAuth (client secret) | 4 req/s **[secondary]** | Non-commercial free; commercial needs a partnership **[secondary]** | Phase 3, BYO credentials only |
| **MobyGames** | Cover groups incl. spine/media, screenshots | Title / Moby id | API key | 360/h, 1/s **[secondary]** | Free non-profit vs paid (**conflicting**) | Skip / BYO |
| **GameTDB** | — | — | — | — | — | **No PS2 [unverified]** |
| **LaunchBox GDB** | Rich PS2 media and metadata (bulk XML) | Title / LB id | None (bulk zip) | One daily zip | **ToS not found** | Ask first; maybe phase 3 |
| **SteamGridDB** | Grids, heroes, logos, icons | Title / store ids | Free per-user key | **[unverified]** | ToS **[unverified]** | Not relevant |
| **Redump (redump.info)** | Hashes, size, serial, version, region, name | Hash / **serial** (with `serial,version`) | None observed (2026-10-22) | Be polite (1/day) | No license or ToS found | **MVP verification**, runtime fetch |
| **libretro-database PS2 DAT** | Redump-derived name, region, serial, size, crc/md5/sha1 | Serial / hash | None | GitHub | **CC BY-SA 4.0** (repo) | **MVP bundled fallback** |
| **PCSX2 GameIndex.yaml** | 12.8k serials: name, name-en, region, PCSX2 compat, CRC-keyed patches | **Serial** | None | GitHub raw | **GPL-3.0** (repo) | **MVP bundled title DB** |
| **ps2homebrew OPL compat list** | Per-game device/format/build/status reports | **Game ID** (`SCUS_973.99.md`) | None | GitHub | No LICENSE | **MVP** runtime fetch, labelled community data |
| **GDX-X OPL CFG compat DB** | `$Compatibility` bitmask CFGs (about 70 games) | Game ID | None | GitHub | No LICENSE | **MVP** suggestions |
| **Google Sheets OPL lists** | Modes, device, DMA, VMC/GSM/WS per game | Game ID | None | Sheets export | None stated | Phase 2 (ask owners) |
| **OPL-CL** | Compat records | Game ID + device | None | — | Private, alive 2026-10-27 (research/21) | Owner's decision (CFG-3) |
| **pcsx2_patches** | About 4.7k WS/NI/fix pnach files | **Serial + ELF XOR-CRC** | None | GitHub | **No LICENSE**; authors credited per file | Later: experimental convert (runtime) |
| **CheatDevicePS2 DB** | Large RAW cheat compilation | Title only | None | — | Repo GPL-3; compilation provenance unclear | Later / maybe |
| **Codetwink / GameHacking.org** | CB/AR codes | Title/ID | Account | — | **[unverified]** | Don't scrape |

---

## 7. Recommendations

### 7.1 Priority order

**MVP (v0.x): fully functional offline-first, no keys needed**

1. Local identification: parse ISO/ZSO/UL/HDL → `SYSTEM.CNF` → game ID, plus ELF XOR-CRC. Normalize `SLUS_200.62` ↔ `SLUS-20062`.
2. **Bundled** `GameIndex.yaml`-derived SQLite (serial → name, name-en, region, related serials). GPL-3.0, attributed. Updated in app releases, with an optional in-app refresh from the raw URL.
3. **Bundled** libretro-database PS2 DAT (CC BY-SA 4.0) for serial ↔ Redump-name ↔ hash. This gives offline verification and libretro-thumbnail names.
4. **Runtime** Redump `serial,version` DAT from redump.info. The user presses "Update Redump data", or auto-refresh at most weekly with conditional GET. Hashes come from a streaming CRC32/MD5/SHA-1 pass during import (ZSO must be decompressed and UL parts concatenated in order).
5. Covers: xlenore/ps2-covers by serial, then libretro-thumbnails by Redump name. Fetched only for games in the user's library, then converted to OPL specs (PNG, ideally 8-bit palettized, COV 140×200 etc.).
6. OPL compat: GDX-X CFGs plus the ps2homebrew Markdown reports, shown as suggestions with evidence. A writer for `CFG/<ID>.cfg` with the `$Compatibility` bitmask.
7. Cheats: `.cht` editor, validator and import only.

**Phase 2**

8. ScreenScraper (opt-in, user enters an SS account; app ships dev creds obtained via the forum) for back cover, spine, disc, logo, BG and SCR. Single thread, quota-aware.
9. "Import OPLM ART backup zip" (user-downloaded) → copy matching files for their library only.
10. Google-Sheets compat import, after asking the sheet owners.

**Phase 3**

11. BYO-key TheGamesDB / IGDB (descriptions, genres, release dates). LaunchBox bulk XML if they grant permission.
12. Experimental pnach → `.cht` widescreen conversion via local CRC match, after contacting PCSX2 and patch authors. Default off, per-game opt-in.

### 7.2 Caching and offline mirrors

- **Cache layout:** `cache/<source>/<key>` plus a SQLite manifest row per object: `source`, `url`, `etag`, `last_modified`, `fetched_at`, `sha256`, `license_tag`, `attribution`.
- Images: keep the original plus the derived OPL-spec PNGs. The derived file is what gets copied to `ART/`.
- **HTTP etiquette per source:**
  - token-bucket limiter (e.g. ≤ 1 req/s to GitHub raw and libretro; ScreenScraper per `ssuser` limits; Redump ≤ 1 req/day);
  - `If-None-Match` / `If-Modified-Since`;
  - exponential back-off on 429/430/5xx;
  - honest `User-Agent: <AppName>/<ver> (+repo URL)`;
  - negative-result caching (don't re-ask for missing covers for N days; ScreenScraper also penalizes "KO" requests).
- **Offline mirrors (user-side, not ours):** allow pointing a source at a local folder or zip. Examples: a `git clone` of `xlenore/ps2-covers` or `libretro-thumbnails/Sony_-_PlayStation_2`, an OPLM backup zip, a downloaded Redump DAT.
  - This gives fully offline operation without us re-hosting anything.
  - It is the same pattern pyoplm uses (`pyoplm.ini` storage URL).
- **No project-run proxy or mirror** for third-party images or ScreenScraper/IGDB data. That would make us a redistributor (licenses unknown or NC-SA) and a server operator.

### 7.3 Bundling rules (by license)

| Data | Bundle in app/repo? | Conditions |
|---|---|---|
| PCSX2 GameIndex.yaml (or derived DB) | **Yes** | App under GPL-3.0 (or keep it as a separately downloaded GPL data file). Include the license and credit "PCSX2 team + sonyindex/serialstation/psxdatacenter/redump" |
| libretro-database PS2 DAT | **Yes** | CC BY-SA 4.0: attribution, and our modified copy stays CC BY-SA |
| Redump DAT | **No** (runtime) | No stated license; credit Redump in the UI |
| Cover images (any source) | **No** | Copyrighted box art; per-user on-demand cache only |
| ScreenScraper data | **No** | CC BY-NC-SA plus API terms; per-user cache only |
| OPLM ART backups | **No** | No license; user import only |
| Compat lists (ps2homebrew, GDX-X, Sheets) | **No** (runtime) until maintainers add a license | Then consider bundling a snapshot |
| pcsx2_patches / cheat DBs | **No** | No license; runtime fetch and local conversion; credit authors |

**Recommended app license: GPL-3.0-or-later.** It matches PCSX2 data, PS2RD/CheatDevice, pyoplm/libopl and the ps-disc-detect ecosystem. If any OPL code (AFL-3.0) is reused, check AFL ↔ GPL compatibility first. Writing OPL-compatible files needs no OPL code.

### 7.4 Things to ask or confirm before shipping

1. ScreenScraper: request a dev ID for the project (forum). Confirm the free-tier quotas and whether open-source distribution of dev creds is acceptable to them.
2. Redump staff: OK to auto-fetch `serial,version` DATs from an app (and at what frequency)? Is there a license for DATs and the new daily DB backup?
3. xlenore/ps2-covers and libretro-thumbnails maintainers: OK to use as the default source? (Courtesy; both are already used this way by PCSX2/RetroArch.)
4. ps2homebrew compat-list and GDX-X maintainers: add a license (CC BY-SA / CC0) and maybe a JSON export.
5. PCSX2 team: redistribution or conversion of `pcsx2_patches` content.
6. LaunchBox: terms for third-party use of `Metadata.zip` and images.

---

## 8. Unverified / open items (summary)

- ScreenScraper free-tier numeric quotas. PS2 systemeid 58 (from ES-DE code only).
- TGDB image type list, allowance numbers, data license.
- IGDB official rate limits and terms (docs 403). Whether IGDB has any serial field.
- MobyGames free vs paid API (conflicting sources).
- GameTDB PS2 absence (site 403).
- LaunchBox ToS, exact Metadata.zip URL and image URL scheme.
- SteamGridDB ToS.
- Redump: exact XML element names, multi-serial representation, the new "Database" backup format, whether `serial,version` will stay login-free. redump.org unreachable.
- OPL `CHT/<name>.cht`: which name OPL passes (game ID vs title). Whether specific games need PS2RD master/hook codes for converted patches.
- psx-place, ConsoleMods, OPL-CL and ps2homebrew.org mirror pages were blocked or unreachable.
- Contents and structure of the OPLM backup zip (per-file naming is assumed to be `<GAMEID>_<TYPE>.png`).

---

## 9. Sources

**OPL / OPL Manager**
- https://github.com/ps2homebrew/Open-PS2-Loader (README, AFL-3.0)
- https://raw.githubusercontent.com/ps2homebrew/Open-PS2-Loader/master/README.md
- https://raw.githubusercontent.com/ps2homebrew/Open-PS2-Loader/master/include/iosupport.h (COMPAT_MODE bits)
- https://raw.githubusercontent.com/ps2homebrew/Open-PS2-Loader/master/lng_tmpl/_base.yml (mode labels)
- https://raw.githubusercontent.com/ps2homebrew/Open-PS2-Loader/master/src/supportbase.c (CFG/CHT paths, ISO name parsing)
- https://raw.githubusercontent.com/ps2homebrew/Open-PS2-Loader/master/src/bdmsupport.c (ART path format)
- https://raw.githubusercontent.com/ps2homebrew/Open-PS2-Loader/master/src/themes.c (ART cache, COV/ICO/BG suffixes)
- https://raw.githubusercontent.com/ps2homebrew/Open-PS2-Loader/master/src/textures.c (PNG-only, size limits)
- https://raw.githubusercontent.com/ps2homebrew/Open-PS2-Loader/master/src/cheatman.c (PS2RD cheat parser)
- https://raw.githubusercontent.com/ps2homebrew/Open-PS2-Loader/master/src/opl.c (compatUpdate)
- https://oplmanager.com/ ; https://oplmanager.com/site/?docs ; https://oplmanager.com/site/?changelog ; https://oplmanager.com/site/?backups
- https://archive.org/details/OPLM_ART_2023_11 ; https://archive.org/advancedsearch.php?q=OPLM_ART&fl[]=identifier&fl[]=date&fl[]=item_size&rows=100&output=json
- https://pypi.org/project/pyoplm/ ; https://github.com/Nold360/libopl
- https://forum.cdrinfo.pl/f106/ps2-opl-manager-96416-print/ (OPLM changelog mirror)

**Covers**
- https://github.com/xlenore/ps2-covers ; https://raw.githubusercontent.com/xlenore/ps2-covers/main/README.md
- https://github.com/libretro-thumbnails/libretro-thumbnails ; https://github.com/libretro-thumbnails/Sony_-_PlayStation_2
- https://thumbnails.libretro.com/Sony%20-%20PlayStation%202/ ; https://thumbnails.libretro.com/Sony%20-%20PlayStation%202/Named_Boxarts/
- https://docs.libretro.com/guides/roms-playlists-thumbnails/
- https://www.screenscraper.fr/webapi2.php ; https://www.screenscraper.fr/ (footer license)
- https://repo.retrodeck.net/Xargon/ES-DE/src/commit/debf072a0eb686d95b91011905318d61fc44b590/es-app/src/scrapers/ScreenScraper.cpp (PS2 = 58)
- https://docs.romm.app/latest/getting-started/metadata-providers/ (ScreenScraper dev creds, MobyGames paid, LaunchBox, Hasheous)
- https://thegamesdb.net/ ; https://api.thegamesdb.net/key.php ; https://thegamesdb.net/platform.php?id=11 ; https://retropie.org.uk/forum/post/177520 ; https://www.rubydoc.info/gems/thegamesdb
- https://discuss.dev.twitch.com/t/commercial-use-of-igdb-api/23567 ; https://macmyths.com/apps/igdb/ ; https://apis.io/rules/igdb/igdb-rules/
- https://mobygames.com/info/api (search snippet only; 403) ; https://Mobygames.com/mobypro/
- https://forums.launchbox-app.com/topic/54163-is-there-a-public-way-to-get-images-from-the-launchbox-games-database/ ; https://forums.launchbox-app.com/topic/52709-launchbox-db-api/
- https://github.com/360900/tinkergame/wiki/SteamGridDB ; https://bitbucket.org/jasondavidcarr/launchbox/issues/7454/scrape-from-steamgriddb
- https://gametdb.com/Wii/Downloads (GameTDB; FAQ 403)
- https://quinton-ashley.itch.io/nostlan/devlog/1012998/nostlan-v31-new-image-databases (The Cover Project)
- https://trendshift.io/repositories/159874 (Hasheous, AGPL-3.0)

**Identification / metadata**
- https://redump.info/ ; https://redump.info/downloads/ ; https://redump.info/datfile/ps2/serial,version
- https://wiki.redump.info/index.php?title=Redump_Search_Parameters
- https://feedbagel.com/post/the-redump-preservation-project-responsible-for-documenting-and-gathering-metada ; https://lemmy.world/post/48476650
- https://bugs.dolphin-emu.org/issues/12183 ; https://github.com/Al-Azif/ps-disc-detect
- http://forum.redump.org/topic/15385/internal-serial-field-for-ps2-games/ (title only; unreachable)
- https://www.mintlify.com/laromicas/datoso/guides/downloading-dats ; https://romsearch.readthedocs.io/en/main/configs/dats.html
- https://github.com/libretro/libretro-database ; https://raw.githubusercontent.com/libretro/libretro-database/master/LICENSE ; https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/redump/Sony%20-%20PlayStation%202.dat
- https://raw.githubusercontent.com/PCSX2/pcsx2/master/bin/resources/GameIndex.yaml ; https://github.com/PCSX2/pcsx2/blob/master/pcsx2/Docs/GameIndex.md ; https://github.com/PCSX2/pcsx2/blob/master/COPYING.GPLv3
- https://psxdatacenter.com/psx2/ulist2.html ; https://psxdatacenter.com/
- https://github.com/Tom-Bruise/PS2-OPL-CFG-Database (via proxy listing and direct fetch)

**Compatibility**
- https://github.com/ps2homebrew/Open-PS2-Loader-Compatibility-list ; https://raw.githubusercontent.com/ps2homebrew/Open-PS2-Loader-Compatibility-list/main/List/G/SCUS_973.99.md
- https://github.com/GDX-X/PS2-OPL-CFG-Compatibility-Database ; https://raw.githubusercontent.com/GDX-X/PS2-OPL-CFG-Compatibility-Database/main/README.md ; https://github.com/GDX-X/PFS-BatchKit-Manager/issues/46
- https://docs.google.com/spreadsheets/d/1MLEw7TIKvn9MCwd5Ptbqh2lkUj3lX6YQ2wPgLEx8Koo/edit
- https://psx-place.com/threads/open-ps2-loader-game-compatibility-lists.19037/ (403; via search snippets)
- http://sx.sytes.net/oplcl/games.aspx (unreachable)
- https://ConsoleMods.org/wiki/PS2:Open_PS2_Loader_(OPL) (403; via search snippets)

**Cheats / patches**
- https://github.com/PCSX2/pcsx2_patches ; https://github.com/PCSX2/pcsx2_patches/tree/main/patches ; https://raw.githubusercontent.com/PCSX2/pcsx2_patches/main/patches/SCES-50000_5BBC2F40.pnach
- https://pcsx2.net/docs/advanced/writing-patches
- https://raw.githubusercontent.com/PCSX2/pcsx2/master/pcsx2/Elfheader.cpp (ELF XOR "CRC")
- https://github.com/KrystianLesniak/pcsx2_patches_list (MIT)
- https://github.com/root670/CheatDevicePS2 ; https://raw.githubusercontent.com/root670/CheatDevicePS2/master/CheatDatabase.txt
- https://www.github.com/madmodder123 (OpenPS2Loader_Widescreen_Cheats listing)
- https://en.wikipedia.org/wiki/Code_Breaker ; https://forums.pcsx2.net/thread-4740.html (Codetwink → pnach)
- https://npmjs.com/package/ps2-codes (GPL-3 converter; page 403, via search)
- https://forums.pcsx2.net/Thread-Important-Patching-Notes-1-7-4546-Pnach-2-0
