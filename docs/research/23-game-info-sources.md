# 23 - Game info (META-3): what OPL shows, where the data is, what may be bundled

Date: 2026-10-27. Scope: META-3 (v0.3, M6) before any design: genre, developer, publisher, release date, player count and a short description on the game page and in the add plan, and the same fields written to `CFG/<ID>.cfg`, where the info page of OPL's theme shows them. The owner's brief of 2026-10-27 asks five things: (a) which keys OPL and its forks read and in what format, (b) what the Tom-Bruise/VTSTech CFG database holds and where its text came from, (c) whether Wikidata can be matched by ID or title and what it covers, (d) how Wikipedia's first paragraph could be used and credited, (e) what other managers do. Sources: the local clones from [REFERENCES.md](REFERENCES.md) (upstream OPL `ps2homebrew/Open-PS2-Loader@b3a21997` and tag `v1.1.0`, RiptOPL `NathanNeurotic/Open-PS2-Loader@38ea46ad`, wOPL `ps2homebrew/wOPL@4ae7414`, NHDDL `pcm720/nhddl@89141d4`, Neutrino `rickgaiser/neutrino@7be8de2`, `Tom-Bruise/PS2-OPL-CFG-Database@7dc9204`, `VTSTech/PS2-OPL-CFG@c669f0c`, the managers in §5), live requests to Wikidata, Wikipedia, GitHub and PSXDataCenter on 2026-10-27 with a research User-Agent, and a 40-game sample (§7). Nothing was copied: OPL and its forks are AFL-3.0 (ADR-0002). Not legal advice.

Legend: **[verified]** read in the source or fetched today; **[secondary]** another tool's reading or a README claim; **[unverified]** not checked.

## 0. TL;DR

- **OPL reads five text keys on its info page** (`Title`, `Genre`, `Release`, `Developer`, `Description`) plus `#Size`, which it computes itself, and image keys of the form `Attribute=anything/value` (`Vmode`, `Aspect`, `Scan`, `Rating`, `Device`). It parses none of the text values: `Release` is shown as typed, a value is cut at 255 bytes, `CfgVersion` appears nowhere in OPL's source, and `Players` is invisible in the built-in theme because no `Players_N` texture exists. Unknown keys survive OPL's own save. The same code and aliases are in v1.1.0, 1.2 beta, RiptOPL and the MMCE fork (§1).
- **Nothing shows Russian text without the Russian font.** The default font has no Cyrillic glyphs; OPL's Russian language pack ships `font_Russian.ttf` (§1.5).
- **The Tom-Bruise database is ScreenScraper text under a GPL label.** Its script runs Skyscraper against screenscraper.fr; every description, genre, date, developer and rating in it (6–7 k of 13,620 files) is a ScreenScraper record, and ScreenScraper's footer licenses its data CC BY-NC-SA 4.0. The GPL-3.0 file cannot relicense that. The `Developer` field often holds the publisher and `Release` is not region-specific (§2.2, §6). Keep it as a round-trip corpus and a cross-check; bundle nothing from its scraped fields.
- **Two sources can be bundled as facts.** libretro-database's `metadat/developer/Sony - PlayStation 2.dat` (CC BY-SA 4.0, the same repo and licence as the Redump DAT in `data/titles.json.gz`, ADR-0024) keys 4,729 games by serial with developer, publisher, day-exact release date, player count and a one-word genre; its descriptions are uncredited PSXDataCenter copy and stay out. Wikidata (CC0) has 3,465 PS2 items with genre, developer, publisher and date for 2.5–3 k of them, Russian labels for 2,241 and a Russian Wikipedia link for 923, but only 48 carry a PlayStation ID: matching is by title and found 9 of the 40 sampled games (§3).
- **Wikipedia's first paragraph** comes from the REST `page/summary` endpoint (plain text, cached, 200 req/s global rule, mandatory User-Agent) in `en` and, where the Wikidata sitelink exists, `ru`. Text is CC BY-SA 4.0 with attribution by URL (Terms of Use §7.7); a 255-byte cut is a modification and must be marked. Show the whole paragraph with the link; write it to the CFG only on the user's explicit choice, with a sibling key naming the source (§4).
- **No open manager has a licensed info source.** OPL Manager edits the fields by hand and fetches only the title from its private DB; OPL PC Tools and OrbitPS2 edit or display the keys; PFS BatchKit and PS2FASTLOADER copy GDX-X's `PS2DB_<lang>.xml` (10,748 games, ScreenScraper and PSXDataCenter data, six languages, no licence) into CFGs, PS2FASTLOADER by rewriting the whole file (§5).

## 1. What OPL and its forks read [verified]

### 1.1 Upstream OPL: `src/themes.c`, `misc/conf_theme_OPL.cfg`

The info page is a list of theme elements `info0…infoN`. An `AttributeText` element names a CFG key in `attribute=`; `drawAttributeText` fetches that key from the game's config set with `configGetStr` and prints `<label>: <value>`, wrapping it inside `width×height` when `wrap=1` (`fntFitString`). The label comes from `initMutableText` (`themes.c:135-165`): if the attribute is `Title`, `Genre`, `Release`, `Developer`, `Size` or `Description` the localised strings `_STR_INFO_TITLE…_STR_INFO_DESCRIPTION` and `_STR_SIZE` are used (`lng_tmpl/_base.yml:585-593`: "Title", "Genre", "Release", "Developer", "Description"); any other attribute is labelled with its own name, so a custom theme can show any key. `#Size` is special-cased to append " MiB" and is set by OPL itself (`supportbase.c:814`, `hddsupport.c:639`, `appsupport.c:436`), never read from the file.

The built-in theme (`misc/conf_theme_OPL.cfg:76-196`) shows, in order: `Title` (y=28), `Genre` (47), `Release` (66), `Developer` (85), `#Size` (104), `Description` (123, box 574×80, `wrap=1`), then the `AttributeImage` row `#Media`, `#Format`, `Vmode`, `Aspect`, `Scan`, `Rating` (default `Rating_0`), `Device`, and two `GameImage` slots `SCR` and `SCR2`. `v1.1.0:src/themes.c:144-154` has the same six aliases and the same draw code (`:201-213`).

An `AttributeImage` reads the key named by its `attribute`; in the built-in theme a value with `/` becomes the internal texture `<Attribute>_<text after the slash>` (`Vmode=vmode/ntsc` → `Vmode_ntsc`), a value without `/` is a texture name itself, and when the texture is unknown nothing is drawn, not even the default (`themes.c:610-626`). Built-in textures exist for `Vmode_{multi,ntsc,pal}`, `Rating_0…5`, `Aspect_*`, `Scan_*`, `Device_*` and the media/format icons (`gfx/`); **there is no `Players_*` texture**, so `Players=players/2` is invisible in the built-in theme. A custom theme (`thmGetGuiValue() != 0`) loads `<theme>/<Attribute>_<value>.png` through the texture cache instead and can therefore draw players.

What OPL does with the values:

| Rule | Where | Consequence for YAOPLM |
|---|---|---|
| A value is at most 255 bytes: `CONFIG_KEY_VALUE_LEN 256`, `strncpy` into `val[256]` (`include/config.h:136`, `src/config.c:479-493`, `:128-131`) | parser | `Description` ≤ 255 bytes, cut on a UTF-8 boundary and ideally at a sentence end; research/15 §1.6 counted 239 (en) and 397 (fr) Tom-Bruise files over the limit |
| A key is at most 31 bytes (`CONFIG_KEY_NAME_LEN 32`) | parser | fine for every key here |
| `Release`, `Genre`, `Developer`, `Description` are printed as stored; OPL never parses a date or a number | `drawAttributeText` | the `MM-DD-YYYY` date is a manager convention (OPL Manager, VTSTech, OPL PC Tools `GameConfiguration.cpp:66` "MM-dd-yyyy"), not OPL's; OPL would show `2001-10-22` just as well |
| `CfgVersion` does not occur in OPL's sources (`grep -r CfgVersion` on `b3a21997`: nothing) | — | an OPL Manager key; preserve it, never require it |
| OPL's own save writes every key it read back (`configWrite` over the whole set; research/15 §1.1) | writer | info keys survive the user's settings menu, and so would a sibling key such as `DescriptionSource` |
| The file is read as bytes and rendered with FreeType through OPL's UTF-8 decoder (`src/fntsys.c`, `include/utf8.h`) | renderer | UTF-8 in values is fine; glyph coverage depends on the font (§1.5) |

### 1.2 RiptOPL (`NathanNeurotic/Open-PS2-Loader@38ea46ad`)

The same six aliases (`src/themes.c:376-387`). Its default theme (`misc/conf_theme_OPL.cfg:203-292`) shows `Title`, `Developer`, `#Size`, `Description`, the image row `#Media`, `#Format`, `Vmode`, `Aspect`, `Scan`, **`Players`** (`default=missing`, 32×16) and `Rating` (`default=no_Rating`); `Genre` and `Release` are not on it. `theme_coverflow.cfg` has the same set; `theme_psbbn_flow.cfg` shows only `Title` and `Description`. RiptOPL's `gfx/` (107 files) and `src/textures.c` have `missing` and `no_Rating` textures but no `Players_*` either, so with a key present the slot stays empty and without the key it shows the "missing" picture. `drawAttributeImage` (`themes.c:2147-2170`) keeps upstream's early return for built-in themes.

### 1.3 wOPL (`ps2homebrew/wOPL@4ae7414`)

libconfig syntax (CFG-1), and the info block is a typed struct `game_info_t` (`include/iosupport.h:164-180`): `title`, `description[256]`, `developer[64]`, `genre[64]`, `publisher[64]`, `serial[16]`, `release[32]`, `aspect[32]`, `parental[32]`, `region[32]`, `players` (int), `user_rating` (int), `version[32]`, `package[64]`, `source[128]`. `parse_game_info` (`src/config_wopl.c:1525-1557`) reads the keys `Title`, `Serial`, `Description`, `Developer`, `Genre`, `Publisher`, `Release`, `Aspect`, `Parental`, `Region`, `Players`, `UserRating`, `Version`, `Package`, `Source`; `Players` accepts an integer or a string and takes the text after the last `/` (`lookup_info_int`), so the legacy `players/2` still works. The migration from an old-style CFG (`src/config_migration.c:560-625`) reads the same names and converts `Players=players/N` to the integer. wOPL therefore also understands `Publisher`, which upstream ignores harmlessly.

### 1.4 Neutrino, NHDDL and the MMCE fork

`ps2-mmce/Open-PS2-Loader@e209011` has upstream's `themes.c` and theme. Neutrino and NHDDL have no info page and read none of these keys (`grep -ri "genre\|developer\|description"` over their sources finds only protocol docs); NHDDL's per-title data is launch options (research/21 §2.4).

### 1.5 Fonts and Russian text

`thirdparty/PoeVeticaNew.ttf`, OPL's built-in font, has 245 glyphs: Latin, Latin-1 Supplement, no Cyrillic, no kana (cmap read with fontTools today). The language repository `ps2homebrew/Open-PS2-Loader-lang` ships `thirdparty/font_Russian.ttf` next to `Russian.yml`; `lng_pack.sh:39-44` copies the font into the language pack, and the pack's README says the `.lng` and the font go into the memory card's OPL folder together. A Russian `Description` therefore renders only when OPL's language is Russian (or a custom theme sets a font with Cyrillic), otherwise as missing glyphs. **[unverified on the console]**: how OPL draws a glyph the font lacks (box, blank, skip).

## 2. The CFG databases and the bundled data

### 2.1 VTSTech/PS2-OPL-CFG (`c669f0c`, 2023-03-01; formerly Veritas83)

13,620 `CFG/<ID>.cfg`, `CfgVersion=7` in 13,616, `Title` in 13,613, `Vmode`/`VmodeText` in 13,270. Only a few hundred carry info fields: `Release` 325, `Developer` 362, `Genre` 361, `Description` 298, `Players` 281, `Rating` 1, `$Compatibility` 145 (research/21 §3). The README says "Still needs: Release Dates, Genre, Description, Players". The files were generated from `test/PS2-GAMEID-TITLE-MASTER.csv` by batch scripts (`test/generate-ntsc-u.bat` echoes `CfgVersion=7` and `Title=` per ID); the CSV header is an OPL Manager export layout (`GameID;Name;Region;Release Date;Genre;Developer;Publisher;Players;Aspect;Scan;ESRB;PEGI;CERO;…;Box Text;Video Mode;Compat Mode;…`), mostly empty. Where a description exists its commas are replaced by hyphens (`SLUS_200.62`: "with his girlfriend-Catalina-and a male accomplice"), the trace of a CSV round trip; the origin of those 298 texts is not stated **[unverified]**, but the GTA III text is the same ScreenScraper sentence Tom-Bruise later scraped (§2.2). Licence GPL-3.0 (LICENSE file).

### 2.2 Tom-Bruise/PS2-OPL-CFG-Database (`7dc9204`, 2021-11-22)

A fork of VTSTech's set, "enriched with game information from ScreenScraper.fr, using a rather hacky script" (README). `scripts/scrape_info.sh` [verified]: for every CFG it takes `Title`, creates an empty `<title>.iso`, runs `Skyscraper -p ps2 -s screenscraper --lang=en …`, reads the one-game `gamelist.xml` and maps `desc` → `Description` (cut to 255 characters, then trimmed to the last full stop), `players` → `Players=players/N` and `PlayersText=N`, `genre` → `Genre`, `releasedate` → `Release` in `%m-%d-%Y`, `developer` → `Developer`, `rating` (0–1) → `Rating=rating/<round(5×rating)>` and `RatingText`. `CFG_fr/` was made with `--lang=fr` and falls back to the English file. `CfgVersion` was bumped to 8 (`update_to_version.sh`). Later commits add a few hand-written descriptions (`d05d7e0`, 2020-10-10: Xenosaga, Ratchet & Clank).

Counts today (`CFG_en` / `CFG_fr`, by `grep -l`):

| Key | en | fr | Format |
|---|---|---|---|
| `Title` | 13,613 | 13,613 | text |
| `Release` | 6,432 | 7,193 | `MM-DD-YYYY` (all 6,420 well-formed values; 12 files lack a final newline and run into `CfgVersion`) |
| `Developer` | 6,823 | 7,621 | text |
| `Genre` | 6,262 | 7,015 | ScreenScraper genre names: "Sports" 797, "Action" 683, "Race, Driving" 663, "Role playing games" 437, … "Various" 179 |
| `Description` | 6,301 | 7,621 | 14–417 characters, median 195; 144 contain non-ASCII |
| `Players` + `PlayersText` | 5,541 / 5,392 | 6,267 | `players/1` 2,400, `players/2` 1,982, `players/4` 559, `players/8` 422, up to `players/32` |
| `Rating` + `RatingText` | 5,289 | 5,902 | `rating/4` 3,891, `rating/3` 744 …: ScreenScraper's user score, not an age rating |
| `Publisher` | 82 | 82 | text (hand-added) |
| `Aspect`, `Scan` | 247 / 242 | | `aspect/w`, `scan/480p` |
| `$Compatibility` | 146 | 145 | research/21 |

`stats/missing_games.txt` lists 5,896 titles ScreenScraper could not match (Japanese titles dominate).

**Licence and provenance.** The repository is GPL-3.0 "following Veritas83's original repository". The scraped values are ScreenScraper records; screenscraper.fr's footer says the site's data is licensed CC BY-NC-SA 4.0 [verified today, footer badge linking creativecommons.org/licenses/by-nc-sa/4.0/] and its API is for free software with developer credentials (research/03 §2.4). A downstream GPL label does not change the upstream licence of the text; the NonCommercial and ShareAlike terms travel with it. Facts (a date, a developer name, a player count) are not copyrightable as such, but 6–7 k records taken in one pass are a substantial extraction of ScreenScraper's database, which in the EU carries the sui generis database right. Honest reading: the repository's own contributions (scripts, hand-written descriptions, the file layout) are GPL-3.0; the scraped fields are ScreenScraper's under BY-NC-SA, and `data/ATTRIBUTION.md` could not state a compatible licence for them (ADR-0007). What the repository is good for: the round-trip and editor corpus it already is (`tests/corpus.rs`, research/15 §1.6), a cross-check of values, and a model of the key conventions OPL Manager users expect (`PlayersText`, `RatingText`, `MM-DD-YYYY`).

**Quality** (sample, §6–§7): `Developer` holds the publisher in 5 of 8 games where libretro and GDX-X name a studio (GTA III "Take 2 Interactive", Dead or Alive 2 "Tecmo", Sonic Riders "Sega", World Tour Soccer "Sony", Graffiti Kingdom "505 Games"); `Release` is one date per title regardless of the serial's region (Tak, `SLUS_212.18`: `02-17-2006`, the PAL date, against PSXDataCenter's 19/20 September 2005 for the USA disc; Dead or Alive 2 PAL: `03-30-2000` against 2000-10-26 and 15 December 2000); `Genre` is "Various" for GTA III.

### 2.3 GDX-X/OPL-Games-Infos-Database-Project (`PS2DB_<EN|DE|ES|FR|IT|PT>.xml`, pushed 2025-01-24)

Not in `refs/`; read through the GitHub API and the raw EN file today. One `<game serial="SLUS-20062">` per release with `Title`, `Serial`, `Region`, `Release` ("22 October 2001"), `Developer`, `Publisher`, `Genre`, `Description`, `Players` (`players/N`), `Parental` (`esrb/17`, `pegi/16`, `cero/…`), `Device`, `Scan`, `Aspect`, `Vmode`, `Rating`, `Notes`. EN file: 10,748 games (NTSC-J 4,598, PAL 3,668, NTSC-U/C 2,127, NTSC-K 210, NTSC-A 106, NTSC-C 24); `Title` 10,740, `Developer` 10,342, `Publisher` 10,429, `Genre` 10,343, `Description` 8,889 (all ≤ 254 bytes, median 189, cut to 255 and trimmed to the last full stop exactly as Tom-Bruise did), `Release` 8,853, `Players` 10,748 of which 1,553 are the empty `players/`, `Parental` 9,416, `Rating` 6,459. The header comment names the sources: Redump (titles only), PlayStation DataCenter, ScreenScraper. No licence; the README says "please consider crediting me". Used by PFS BatchKit (`!PFS-BatchKit-Manager.bat:8660-8750`: `sed` over `PS1DB.xml`/`PS2DB.xml` into CFG fields) and PS2FASTLOADER (`metadata.rs:22`, `OPL_PS2DB_URL`). The description text has the same ScreenScraper origin and limits as §2.2; the facts largely coincide with PSXDataCenter (§2.4). Six languages, no Russian. Coverage in the sample: 37 of 40 serials, 34 with a description, 28 with a date.

### 2.4 libretro-database `metadat/developer/Sony - PlayStation 2.dat` (3.7 MB; last change 2026-10-19 `a0a1c17`)

The repository whose Redump DAT YAOPLM already bundles (ADR-0024, LICENSE: CC BY-SA 4.0). This DAT is the only other PS2 file under `metadat/` besides `redump/` and `hacks/` (tree listing today); the `publisher/`, `genre/`, `releaseyear/`, `users/` folders have no PS2 file. Header `homepage "https://github.com/robloach/libretro-database-psxdatacenter#readme"`, version 2020.3.19: it is the output of robloach's Puppeteer scraper of psxdatacenter.com (`index.js`: the `ulist2`/`jlist2`/`plist2` indexes, then each game page; `#table16 td` → `description`), first committed 2020-03-19 ("Move PSXDataCenter dats"), with 40 commits since, the last three in October 2026 ("Fixed metadata"). 4,729 `game (` records keyed by `serial`, names in PSXDataCenter form ("Tak: The Great Juju Challenge (USA)"; 2,724 Japan, 1,203 Europe, 672 USA), fields: `description` 4,727 (median 451 characters, up to 1,152), `developer` 4,729, `publisher` 4,729, `releaseyear`/`releasemonth`/`releaseday` 4,727, `users` 4,729 (1 = 3,136, 2 = 1,391, 4 = 172, 8 = 27, 6 = 2, 3 = 1), `genre` 4,729, one phrase ("Action" 2,071, "Adventure" 1,030, "Racing" 256, "Role-playing (RPG)" 213, "Simulation" 186, "Sports" 184, "Fighting" 160, "Shooter" 155, … "Mahjong" 30).

Provenance of the text: PSXDataCenter's game page for `SLUS-20062` shows the same synopsis ("Rockstar Games once again invites players to 'explore their inner criminal' …") with no source credit and no licence or terms anywhere on the site [verified today]; the same sentence appears as product copy in retail listings of the game, so it is publisher or review text of unknown copyright. The facts on that page (developer DMA Design Ltd., publisher Rockstar Games, release 23 October 2001, 1 player, genre Action / Driving / Shooter, languages) are the per-serial, per-region values a manager wants. Verdict: the same footing as the Redump names already bundled, namely a CC BY-SA 4.0 compilation whose individual facts are not copyrightable, for `developer`, `publisher`, the date, `users` and `genre`; the descriptions are not covered by that reasoning and stay out of the build. Sample coverage: 11 of 40 (NTSC-U 4/15, PAL 4/12, NTSC-J 3/10): the scrape stopped at the games PSXDataCenter had info pages for in 2020.

### 2.5 PCSX2 GameIndex and Redump (already bundled)

Serial → title, region, related serials, Redump names and media (ADR-0024); no genre, developer or date. The bundled `data/titles.json.gz` keys everything by `SLUS-20062`, the same key the libretro developer DAT and GDX-X use, so a bundled info table joins by serial with no name matching.

### 2.6 OPL-CL

Its CFG fragments hold `CfgVersion=1` and modes only (research/21 §1.2); no info fields, and ADR-0049 excludes it anyway.

## 3. Wikidata (CC0) [verified today, SPARQL and the Action API]

Counts for items with `P400` (platform) = `Q10680` (PlayStation 2):

| What | Items | Note |
|---|---|---|
| PS2 items | 3,465 | one item per title, not per regional release; `data/titles.json.gz` has 12,978 serials |
| genre `P136` | 2,969 | labels such as "action-adventure game", "racing video game" |
| developer `P178` | 2,538 | |
| publisher `P123` | 2,538 | |
| publication date `P577` | 2,889 | often several, with place-of-publication qualifiers; precision can be year only (`+2001-00-00`) |
| number of players `P1872` | 14 | effectively absent |
| game mode `P404` | 2,228 | single-player / multiplayer only |
| PlayStation ID `P2606` | 48 items, 63 values | format `S(L|C|I)(E|U|P|K)(M|S|A)-\d{5}`; one value has a space ("SLPM 62599") |
| SerialStation ID `P7919` | 248 | |
| PlayStation DataCenter URL `P7707` | 320 | |
| vglist ID `P8351` | 2,901 | |
| Russian label | 2,241 | |
| English Wikipedia article | 2,515 | sitelinks |
| Russian Wikipedia article | 923 | |

**Matching.** By serial it is hopeless (48 items). By title: `wbsearchentities` on the GameIndex `name-en`, then keep the first candidate whose `P400` includes `Q10680` (`Special:EntityData/<Q>.json`, the access method Wikidata recommends for known items). On the 40-game sample (§7) this found 9 games: genre 9, developer 7, publisher 7, date 9, players 0, game mode 6, Russian label 7, English article 8, Russian article 2. The misses are demos, licensed and sports titles, most PAL and Japanese releases and anything whose regional name differs ("World Tour Soccer 2002" was found only as "This Is Football 2002"). Two matches need a second look (Treasure Planet matched an item with no article; ChoroQ matched "Choro Q HG 4", which is the right game under its Japanese name), so a build-time match must record its method and a human must be able to override it. Two developer labels came back as bare QIDs in the sample (a label fetch that returned nothing, not investigated) **[unverified]**.

**Etiquette** (Wikidata:Data_access, API:Etiquette, the User-Agent policy): data is CC0 and Wikidata asks to be credited; send a descriptive User-Agent with contact (a missing or generic one gets HTTP 403 and may be blocked), requests in series, `maxlag` for non-interactive jobs, honour `Retry-After` on 429, cache what does not change; no numeric per-client limit is published for the API, the REST API's global rule is 200 req/s (§4). This fits ADR-0007: on the user's action, with our `YAOPLM/<version> (+repo URL)` agent, cached.

**Languages.** Labels of genre, developer and publisher items exist in Russian for the common ones **[unverified for the long tail]**; a Russian label of the game itself exists for 65 % of PS2 items, a Russian article for 27 %.

## 4. Wikipedia (CC BY-SA 4.0) [verified today]

- **Endpoint.** `GET https://<lang>.wikipedia.org/api/rest_v1/page/summary/<title>` returns JSON with `title`, `type`, `wikibase_item`, `timestamp`, `content_urls` and `extract`, "an extract of the first paragraph of the page in plain text" (the REST spec's own words), plus `extract_html`. `Grand_Theft_Auto_III`: `en` 591 characters, `ru` 872 characters (about 1.6 KB in UTF-8); `Katamari_Damacy` `en` 688. Response headers: `cache-control: s-maxage=1209600, max-age=300`, an `ETag`. The Action API alternative (`prop=extracts&exintro&explaintext&exsentences=3`) gives the first N sentences, useful for a 255-byte target. The article title must come from the Wikidata sitelink, never from a title search: "Treasure Planet" or "Finding Nemo" by name is the film.
- **Rules.** The spec's global rules: at most 200 requests/s per client to the REST API, a unique `User-Agent` or `Api-User-Agent` with contact; the Foundation's User-Agent policy makes the header mandatory (403 without it). The spec text still says CC BY-SA 3.0; the Terms of Use (§7.1, updated 2023) license text under CC BY-SA 4.0 and GFDL, and the Terms govern.
- **Attribution** (Terms of Use §7.7): credit the authors "through hyperlink (where possible) or URL to the page" (or a stable copy, or the author list). **Share-alike** (§7.8): a modified or shortened text must be licensed CC BY-SA 4.0, marked as changed, and each distributed copy carries a licence notice naming the licence and linking to its text.
- **In the app**: the whole paragraph with "From Wikipedia (CC BY-SA 4.0)" and the article link satisfies §7.7 as is. **In the CFG**: a 255-byte cut is a modification; the value itself cannot carry the URL. OPL keeps unknown keys (§1.1), so the plan can write a sibling key with the article URL (for example `DescriptionSource=https://en.wikipedia.org/wiki/Grand_Theft_Auto_III`; a plain key, not a `$` setting; themes could even display it) and the plan says the text was shortened. Whether a file on the user's own stick is "distribution" at all is moot: doing it this way costs one line. The Russian paragraph needs the Russian OPL font to show (§1.5), so the CFG default should be English, Russian by explicit choice.
- **Coverage**: 2,515 PS2 items have an English article, 923 a Russian one (§3); in the sample 8 and 2 of the 9 matched games.

## 5. What other managers do [verified in the clones unless marked]

| Manager | Fields | Source | Written to CFG |
|---|---|---|---|
| **OPL Manager V24** (closed; changelog read today) | CFG editor with genres list (12.0, 16.0, 18.0 "Sports Genre"), ratings, "Text equivalents of multiple CFG properties" (21.0: `PlayersText`, `RatingText`, `VmodeText`), a button to fetch the **title** from its DB (16.0, 18.0, 19.0, 21.0) | hand-typed; title from the private OPLM DB; no entry in the changelog downloads descriptions, dates or genres | the keys above, `CfgVersion` |
| **OPL PC Tools 4.0** `GameConfiguration.cpp:58-66, 106-115` | `Title`, `Description`, `Genre`, `Developer`, `Release` as `QDate` from `MM-dd-yyyy` | hand-typed | the same keys |
| **OrbitPS2 1.3.1** `details.component.ts:58-73, 136-142` | shows `Developer`, `Genre`, `Release`, `Description`, `PlayersText` or `Players`, rating from the CFG, `title.cfg` for apps | none from the network (title-only API, research/05 §2.3) | does not write them |
| **PS2FASTLOADER** (MIT, `c47be7b`) `metadata.rs:22`, `opl_cfg.rs:160-200` | `Title`, `Description` (`chars().take(255)`), `Release`, `Genre`, `Developer`, `Publisher`, `Players` + `PlayersText` | GDX-X `PS2DB_EN.xml` by serial, libretro Redump DAT for names, xlenore covers | rewrites the whole CFG: `#Name`, `#GameID`, `CfgVersion`, `$ConfigSource=1`, then the fields; foreign keys are lost |
| **PFS BatchKit** (`eb77f51`) `!PFS-BatchKit-Manager.bat:1425, 8660-8750` | `Title`, `Description`, `Developer`, `Genre`, … | GDX-X XML (`PS1DB.xml`, `PS2DB.xml`) | regenerates the CFG from the database; cuts `Description` to 255 (research/15) |
| **PSBBN Definitive** (`6c7c122`) `Game-Installer.sh:505, 1305` | title, publisher from its own `TitlesDB_PS1.csv`/`AppDB.csv`; `Developer=`, `Genre=Homebrew` for apps | own CSVs | `title.cfg` / `info.sys` for HDD-OSD |
| **logi-26/ps2-game-manager** `GameConfigFileManager.java:30-31` | key list `CfgVersion, Title, Genre, Developer, Release, Players, Rating, Description, Notes, $VMC_0, $VMC_1, Cheat, $EnableCheat, Device` | editor | those keys |
| **PyOPLM 0.73** | "RAWG.io metadata" is a TODO (research/01 §2.3) | — | — |
| **OPLNova** | title only (research/05 §2.3 "Metadata sources") | private API | nothing |
| **OPL CFG Database** = Tom-Bruise (§2.2) | the files themselves | ScreenScraper | the user copies the files over with OPL Manager |

Two consequences: the key names and conventions (`Players=players/N` plus `PlayersText=N`, `Release=MM-DD-YYYY`, `Description` ≤ 255 with a sentence-end trim, `Publisher` as an extra key) are shared by every tool that writes info, so YAOPLM should write the same; and every tool that fetches info uses ScreenScraper-derived text through GDX-X, none has a licensed description source.

## 6. Where the sources disagree

From the 27 sampled games with a developer in at least two sources (§7):

1. **Developer vs publisher.** Tom-Bruise's `Developer` is the publisher in 5 of 8 comparable games; libretro (PSXDataCenter) and GDX-X agree with each other on the studio in all 11 games both cover (spelling aside: "DMA Design" / "DMA Design Ltd", "Remedy Entertainment" / "Remedy Entertaiment"). Wikidata's `P178` names the studio where present (Sonic Team, Taito, Asobo Studio).
2. **Dates.** Three sources, three dates for Dead or Alive 2 (`SCES-50003`): Tom-Bruise `03-30-2000` (the Japanese release), libretro 2000-10-26, GDX-X 15 December 2000. Tak (`SLUS-21218`): Tom-Bruise `02-17-2006` (PAL), libretro 2005-09-20, GDX-X 19 September 2005, PSXDataCenter's page 23 October 2001 for GTA III against libretro's 2001-10-23 and GDX-X's 22 October 2001. Only serial-keyed sources (libretro, GDX-X) can give the date of the disc in hand; Wikidata's `P577` carries the region as a qualifier and often only a year.
3. **Genre vocabularies.** Tom-Bruise "Race, Driving" (ScreenScraper, English names of French categories), libretro one word "Action", GDX-X "Action, Racing", Wikidata "racing video game". A bundled table needs one vocabulary with `en` and `ru` labels; Wikidata genre items have both, libretro's 30-odd phrases are easy to map by hand.
4. **Players.** Sonic Riders: libretro 2, GDX-X and Tom-Bruise 4; Dead or Alive 2: Tom-Bruise 2, libretro and GDX-X 4; World Tour Soccer: Tom-Bruise 8, libretro 2, GDX-X 4. Wikidata has no count.
5. **Coverage, not just values.** In the sample: GDX-X 37/40, Tom-Bruise with any info field 24/40, libretro 11/40, Wikidata 9/40, Russian Wikipedia 2/40. The two licensed sources together (libretro facts, Wikidata) cover 16 of 40 with at least a developer or a genre; descriptions under a compatible licence exist for 8 (English) and 2 (Russian).

So every value the app shows or writes needs its source next to it, dates should be the serial's own where a source has them, and the app shows what it has rather than averaging.

## 7. The 40-game sample

Three fixture games (`SLUS-20062`, `SCES-50003`, `SLUS-21218`) plus 15 NTSC-U, 12 PAL and 10 NTSC-J serials drawn from `data/titles.json.gz` with seed 23 (the script and raw results are in the session's scratchpad, not committed). Wikidata by title search with the PS2 filter; libretro and GDX-X by serial; Tom-Bruise by file name.

| ID | Region | Title | Wikidata | ru | libretro | GDX-X | Tom-Bruise |
|---|---|---|---|---|---|---|---|
| SLUS-20062 | NTSC-U | Grand Theft Auto III | Q149106: genre, dev, pub, date | ru article | dev, pub, date, genre, players, text | genre, dev, pub, date, text, players | genre, dev, date, text, players |
| SCES-50003 | PAL-M5 | Dead or Alive 2 - Hardcore | — | — | dev, pub, date, genre, players, text | genre, dev, pub, date, text, players | genre, dev, date, text, players |
| SLUS-21218 | NTSC-U | Tak - The Great Juju Challenge | — | — | dev, pub, date, genre, players, text | genre, dev, pub, date, text, players | genre, dev, date, text, players |
| SLUS-21025 | NTSC-U | Madden NFL 2005 [Special Collectors Edition] | — | — | — | genre, dev, pub, date, text, players | genre, dev, date, players |
| SLUS-20051 | NTSC-U | NFL Blitz 2002 | — | — | — | genre, dev, pub, date, text, players | genre, dev, date, text |
| SCUS-97172 | NTSC-U | World Tour Soccer 2002 | Q60739839 (as "This Is Football 2002"): genre, date | label | dev, pub, date, genre, players, text | genre, dev, pub, date, text, players | genre, dev, date, text, players |
| SLUS-21102 | NTSC-U | Hard Rock Casino | — | — | — | genre, dev, pub, date, text, players | genre, dev, date, text, players |
| SLUS-21644 | NTSC-U | Nickelodeon SpongeBob's Atlantis SquarePantis | — | — | — | genre, dev, pub, text | title only |
| SLUS-21421 | NTSC-U | Spy Hunter - Nowhere to Run | — | — | — | genre, dev, pub, text, players | genre, dev, date, text, players |
| SLUS-21331 | NTSC-U | Sonic Riders | Q683373: genre, dev, pub, date | ru article | dev, pub, date, genre, players, text | genre, dev, pub, date, text, players | genre, dev, date, text, players |
| SLUS-20299 | NTSC-U | Street Hoops - King of the Court | — | — | — | genre, dev, pub, text, players | title only |
| SLUS-20586 | NTSC-U | IHRA Drag Racing 2 | — | — | — | dev, pub, date, text | dev, date, text |
| SLUS-20930 | NTSC-U | ChoroQ | Q5105196 (Choro Q HG 4): genre, dev, pub, date | label | — | genre, dev, pub, date, text, players | title only |
| SLUS-21735 | NTSC-U | Mana Khemia - Alchemists of Al-Revis | — | — | — | genre, dev, pub, date, text, players | genre, dev, date, text, players |
| SCUS-97146 | NTSC-U | Disney's Treasure Planet | Q3211722: genre, date (no article) | label | — | genre, dev, pub, date, text | genre, dev, date, text |
| SLUS-20720 | NTSC-U | Romance of the Three Kingdoms VIII | Q7362424: genre, dev, pub, date | label | — | genre, dev, pub, text, players | genre, dev, date, text, players |
| SLUS-21780 | NTSC-U | Ferrari Challenge - Trofeo Pirelli | — | — | — | genre, dev, pub, date, text | title only |
| SCUS-97216 | NTSC-U | PlayStation Underground Summer Sampler | — | — | — | — | title only |
| SCES-50294 | PAL-M5 | Gran Turismo 3 - A-Spec | — | — | dev, pub, date, genre, players, text | genre, dev, pub, date, text, players | title only |
| SCED-54053 | PAL-M5 | Official PlayStation 2 Magazine Demo 74 | — | — | — | — | title only |
| SLES-53873 | PAL-E | Sol Divide | Q11315414: genre, dev, pub, date | label | — | genre, dev, pub, text | genre, dev, date, text |
| SLES-52336 | PAL-E-G | Max Payne 2 - The Fall of Max Payne | — | — | dev, pub, date, genre, players, text | genre, dev, pub, date, text, players | dev, date, text, players |
| SLES-51678 | PAL-M6 | Super Farm | Q7642336: genre, dev, pub, date | — | — | genre, dev, pub, text, players | genre, dev, date, players |
| SCED-51140 | PAL-M5 | PS2 Bonus Demo 03 | — | — | — | — | title only |
| SLES-52458 | PAL-E | Disgaea - Hour of Darkness | — | — | — | genre, dev, pub, date, text, players | genre, dev, date, text, players |
| SLES-53311 | PAL-M5 | Graffiti Kingdom | Q16259584: genre, dev, pub, date | label | dev, pub, date, genre, players, text | genre, dev, pub, date, text, players | genre, dev, date, text, players |
| SLES-54097 | PAL-G | Dynasty Warriors 5 - Empires | — | — | — | genre, dev, pub, date, text, players | genre, dev, date, text, players |
| SLES-51755 | PAL-E | Disney/Pixar Finding Nemo | — | — | — | genre, dev, pub, date, text, players | title only |
| SLES-51118 | PAL-E | Wizardry - Tale of the Forsaken Land | — | — | — | genre, dev, pub, text, players | title only |
| SLES-54372 | PAL-E-G | RTL Biathlon 2007 | — | — | — | genre, dev, pub, text | genre, dev, date, text |
| SLPM-55076 | NTSC-J | Medal of Honor - Vanguard [EA:SY! 1980] | — | — | — | genre, dev, pub, date, text, players | text |
| SLPM-65156 | NTSC-J | Beatmania IIDX 6th Style - New Songs Collection | — | — | — | genre, dev, pub, date, text | title only |
| SCPS-15094 | NTSC-J | EyeToy - Play 2 [Game Only] | — | — | — | genre, dev, pub, date, text, players | genre, dev, date, text, players |
| SLPM-66757 | NTSC-J | Que - Ancient Leaf no Yousei [Standard Edition] | — | — | dev, pub, date, genre, players, text | genre, dev, pub, date, text, players | title only |
| SLPM-62594 | NTSC-J | Battle Hot Gimmick Cosplay Mahjong [Psikyo the Best] | — | — | — | empty record | title only |
| SLPM-65270 | NTSC-J | Virtua Fighter 4 - Evolution | — | — | — | genre, dev, pub, date, text, players | genre, dev, date, text, players |
| SLPM-66698 | NTSC-J | Shijou Saikyou no Deshi Kenichi - Gekitou! Ragnarok Hachikengou | — | — | dev, pub, date, genre, players, text | genre, dev, pub, date, players | title only |
| SLPS-25981 | NTSC-J | Daisenryaku - Daitoua Kouboushi - Tora Tora Tora Ware Kishuu Seikou Seri | — | — | — | genre, dev, pub, date, players | no file |
| SLPM-66754 | NTSC-J | Getsumento Heiki Mina - Futatsu no Project M | — | — | dev, pub, date, genre, players, text | genre, dev, pub, date, text, players | genre, dev, date, text, players |
| SLPS-73226 | NTSC-J | Tenchu Kurenai [PlayStation2 the Best] | — | — | — | genre, dev, pub, date, text, players | genre, dev, date, text, players |

Totals: Wikidata 9 (genre 9, developer 7, publisher 7, date 9, players 0, English article 8, Russian article 2); libretro 11 (all fields); GDX-X 37 (description 34, date 28, players 29); Tom-Bruise 24 with any info field (genre 22, developer 24, date 24, description 23, players 19), 14 with the title only, 1 missing.

## 8. Options and the decisions needed (for the ADR, not decided)

**Fields and where they would come from.**

| Field | OPL key | Bundled table (offline) | On request (network, cached) | Written to CFG | Shown only |
|---|---|---|---|---|---|
| Title | `Title` | GameIndex / Redump (already) | — | already (rename, CFG-2) | |
| Genre | `Genre` | libretro facts (4.7 k); Wikidata snapshot (≈3 k, matched by title) | Wikidata live | yes, one vocabulary in English | `ru` label in the app |
| Developer | `Developer` | libretro; Wikidata | Wikidata | yes | |
| Publisher | `Publisher` | libretro; Wikidata | Wikidata | yes (wOPL reads it, OPL ignores it, PS2FASTLOADER and GDX-X write it) | |
| Release | `Release` | libretro (day, per serial); Wikidata (per title, region qualifier, sometimes year only) | Wikidata | yes, `MM-DD-YYYY` (the convention every manager uses; OPL does not parse it) | the app formats it per locale |
| Players | `Players=players/N`, `PlayersText=N` | libretro `users` | — | yes (OPL Manager convention; RiptOPL's theme has the slot) | Wikidata game mode (single/multi) only shown |
| Description | `Description` ≤ 255 bytes | — | Wikipedia `en` (and `ru` where linked) through the Wikidata sitelink | only on explicit choice, cut at a sentence end, with `DescriptionSource=<URL>` and the plan saying it was shortened | the whole paragraph with the link and "CC BY-SA 4.0" |
| Rating, Vmode, Aspect, Scan, Parental | image keys | — | — | not in META-3 (CFG-2 territory; `Rating` in Tom-Bruise is a user score) | |

**Build.** `scripts/build-game-info.py` in the style of `build-data.py` (ADR-0024, ADR-0049): inputs by path or URL (the libretro developer DAT; a Wikidata extract fetched by the script through SPARQL and cached as JSON for offline builds), output `data/game-info.json.gz` keyed by serial, each value with a `source` tag and, for Wikidata rows, the match method (title match against GameIndex `name-en`, or `P2606`/`P7919`/`P7707` where present); `data/ATTRIBUTION.md` gains the libretro developer DAT (CC BY-SA 4.0, credits PSXDataCenter and robloach's scraper), Wikidata (CC0, credited on request) and Wikipedia (CC BY-SA 4.0, never bundled). Commit the result.

**Questions for the owner.**

1. **libretro facts in the build**: developer, publisher, date, players, genre for 4,729 serials under CC BY-SA 4.0, like the Redump names today; descriptions excluded. (Recommended: yes.)
2. **Wikidata**: a build-time snapshot matched by title with the method recorded (≈3 k titles, `en`/`ru` labels, genre, developer, publisher, first date, article titles), with a live lookup on request as the fallback; or live only. (Recommended: snapshot plus live.)
3. **Wikipedia descriptions**: on request through the sitelink, `en` and `ru`, shown whole with the link; written to the CFG only on an explicit choice, shortened at a sentence end, with `DescriptionSource`. (Recommended: yes; the default CFG language English because of the font.)
4. **GDX-X `PS2DB_<lang>.xml`**: not used (no licence, ScreenScraper text, its facts duplicate PSXDataCenter), or fetched on request like xlenore with the author's credit, or ask GDX-X for a licence first. (Recommended: not now; open an issue asking for a licence.)
5. **Tom-Bruise/VTSTech**: cross-check and corpus only, nothing bundled or written. (Recommended: yes.)
6. **Which keys the plan writes**: `Title`, `Genre`, `Release`, `Developer`, `Publisher`, `Players` + `PlayersText`, and `Description` + `DescriptionSource` on choice; `CfgVersion` left as found; by CFG-2 rules (foreign keys and order kept, 255 bytes, "was → will be" for every overwritten value).
7. **Languages**: the app shows `en`/`ru` per UI language where the source has both (Wikidata labels, Wikipedia); the CFG gets English unless the user picks Russian, with a hint that OPL needs its Russian font.
8. **Genre vocabulary**: libretro's phrases mapped by hand to a short list with `en`/`ru` labels (Wikidata's genre items where they coincide), one list on the card, in the plan, in the CLI.
9. **Add plan**: info from the bundled table shown per game and written in the same plan when the user ticks it; network sources only from the game page, never during an add.

## 9. Links

- OPL: `src/themes.c`, `misc/conf_theme_OPL.cfg`, `src/config.c`, `include/config.h`, `lng_tmpl/_base.yml`, `thirdparty/PoeVeticaNew.ttf` at `b3a21997`; `v1.1.0:src/themes.c`; https://github.com/ps2homebrew/Open-PS2-Loader-lang (`thirdparty/font_Russian.ttf`)
- RiptOPL `src/themes.c`, `src/textures.c`, `misc/conf_theme_OPL.cfg`, `misc/theme_coverflow.cfg`, `misc/theme_psbbn_flow.cfg` at `38ea46ad`; wOPL `src/config_wopl.c`, `src/config_migration.c`, `include/iosupport.h` at `4ae7414`
- https://github.com/Tom-Bruise/PS2-OPL-CFG-Database (`scripts/scrape_info.sh`, `stats/`); https://github.com/VTSTech/PS2-OPL-CFG; https://www.screenscraper.fr/ (footer: CC BY-NC-SA 4.0)
- https://github.com/GDX-X/OPL-Games-Infos-Database-Project (`PS2DB_EN.xml`, README); https://github.com/ShaunCarrPHL/PS2FASTLOADER (`desktop/src-tauri/src/metadata.rs`, `opl_cfg.rs`); https://github.com/GDX-X/PFS-BatchKit-Manager
- https://github.com/libretro/libretro-database (`metadat/developer/Sony - PlayStation 2.dat`, LICENSE); https://github.com/robloach/libretro-database-psxdatacenter (`index.js`); https://psxdatacenter.com/psx2/games2/SLUS-20062.html
- Wikidata: https://www.wikidata.org/wiki/Wikidata:Data_access ; https://www.wikidata.org/wiki/Property:P2606 ; https://query.wikidata.org/ ; https://www.mediawiki.org/wiki/API:Etiquette ; https://foundation.wikimedia.org/wiki/Policy:Wikimedia_Foundation_User-Agent_Policy
- Wikipedia: https://en.wikipedia.org/api/rest_v1/ (spec: global rules, `page/summary`) ; https://foundation.wikimedia.org/wiki/Policy:Terms_of_Use (§7.1, §7.7, §7.8)
- OPL Manager changelog: https://oplmanager.com/site/?changelog
