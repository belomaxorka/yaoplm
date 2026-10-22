# 11 - Cover art and metadata: prior art

Date: 2026-10-23. Scope: milestone M4 (the bundled title database, cover download by ID, transcoding, cache, manual replacement, bulk runs). Sources: the local clones from [REFERENCES.md](REFERENCES.md), refreshed the same day, read before each piece was built (ADR-0015). Nothing was copied: Orbit and Toolbox are GPL-3 but written in TypeScript for Electron, PyOPLM is GPL-3 Python, OPL PC Tools is GPL-3 C++/Qt. Ideas only; the format rules come from OPL itself (research/02 §7, research/03 §1).

Short names:
- Orbit = `Luden02/OrbitPS2-Manager@cdd590a` (`src/services/artwork.service.ts`, `src/services/artwork-filenames.ts`, `src/utils/png-artwork.ts`, `angular/.../artwork-bulk-dialog`, `artwork-wizard-dialog`).
- Tbx = `lucasonline0/opl-linux-toolbox@3a776d1` (`src/services/artwork.service.ts`, `src/services/art-url.service.ts`).
- PCT = `brainstream/OPL-PC-Tools@2291b5a` (`src/OplPcTools/GameArtManager.cpp`, `GameArtNetworkSource.cpp`).
- PyOPLM = `edisnord/PyOPLM@a8e3ceb` (`pyoplm/storage.py`, `pyoplm/opl/pyoplm_manager.py`).
- Nova = `WeeXnes/OPLNova@7c30757` with its art server `WeeXnes/oplnova_vapi@4806a79`.
- OPL = `ps2homebrew/Open-PS2-Loader@b3a21997` (`src/textures.c`, `src/bdmsupport.c`).

## 1. What was taken, and from where

| Topic | Idea | Source | Where in YAOPLM |
|---|---|---|---|
| Bulk run | Scope "only missing" (default) or "overwrite"; games that already have every requested type are neither queued nor logged, only counted | Orbit `artwork-bulk.targets.ts` (`artTargetsForScope`, `onlyMissing`), `artwork-bulk-dialog.component.ts` (`overwrite` signal, `completeCount`) | `ArtScope::Missing | All`, the `coversPresent` notice; `ArtFetchDialog` shows the counts before Start |
| Bulk run | One live log line per game and asset type; a run that is cancelled lets the file in flight finish and drops the queue | Orbit `artwork-bulk-dialog` (`logEntries`, `cancelling`), `jobs.service.ts` | `JobLogEntry::Art` per game; the job checks the cancel token between games |
| Bulk run | An elapsed counter because a single game can spend many seconds in silence | Orbit `runStartedAt` / `elapsedLabel` | The progress line shows `done / total` per game, not bytes, and the game being asked for |
| Single game | "Skip existing" is on by default in the wizard; replacing is an explicit choice | Orbit `artwork-wizard-dialog` (`skipExisting`) | The game card asks for `all` only when the game already has a cover; a manual file or paste always replaces |
| Health | A count of games without art, and a repair that touches only those | Tbx README ("artwork health", "Repair") and `artwork.service.ts` (`checkArtFilesExist`) | The "N without cover" filter in the library; the bulk run's default scope |
| Download | A per-request timeout so one hung socket does not stall a bulk run; retries with a short back-off; a `Content-Type` check and an image-signature check before anything is written; temp file + fsync + rename | Orbit `DOWNLOAD_TIMEOUT_MS`; Tbx `request()` (retries), `downloadOne()` (content type, `validateImageBytes`, `.part` file) | `yaoplm-net::http` (60 s timeout, 3 retries with back-off and `Retry-After`), `transcode::looks_like_image`, `cache::write_atomic` |
| Download | Every type claimed once: a remote file written for one slot is never reused for another | Orbit `claimed` set in `downloadArtByGameId` | Not needed yet (COV only); kept in mind for ICO/SCR/BG in v0.2 |
| Cache | A directory-listing cache with a TTL, and a cached answer served when the API fails | Tbx `discover()` (`CACHE_TTL_MS`, `readCache` fallback) | `cache.sqlite` with `fetched_at`, "not found" rows kept for seven days, cached originals served without a request |
| Local serving | Cover files reach the renderer through a private URL scheme with a token per file, never a raw path | Tbx `art-url.service.ts` (`opl-art://asset/<sha256>`) | Desktop: Tauri's asset protocol with the open root's `ART` folder allowed at runtime; server: `/media/art?path=` inside the roots only, with an ETag |
| Mirror | A source can be a local folder or URL with the same layout as the upstream pack; the tool never re-hosts anything | PyOPLM `storage.py` (`storage_location`, `get_filename_options`), `pyoplm.ini` | `Location::Mirror(dir)` per source, also the e2e fixture and the test mock |
| Overwrite | `--overwrite` as a flag, otherwise existing `ART/<ID>*` files are left alone | PyOPLM `get_artwork_for_game(region_code, overwrite)` | `yaoplm art fetch --all`, `art set --keep-existing` |
| Resize | Resize to the loader's exact size without cropping: OPL stretches the texture into the theme's element anyway | PCT `GameArtManager::setArt` (`Qt::IgnoreAspectRatio`), PyOPLM `PS2_ART_SIZES`, OPL `src/textures.c` | `transcode::to_png` (Lanczos3 through `fast_image_resize`) |
| Lookup | Several extensions per type in lookup order; art keyed by the game ID | PCT `makeFilePaths` (`.png .jpeg .jpg .bmp`), OPL `bdmGetImage()` | `existing_art()` in `yaoplm-core::art` treats `png/jpg/jpeg/bmp` as the same slot when replacing or deleting |
| Backups | Deleting art is a confirmed operation; a copy survives | PCT `deleteArt` (no backup) and YAOPLM's own CFG rule (CFG-5) | A replaced cover is moved to `.yaoplm/backup/<id>/ART/`, a deleted one copied there first (ADR-0025) |
| About | Credits per source with the licence next to it | PCSX2's GameIndex header, libretro's `LICENSE` | `data/attribution.json` → `data/ATTRIBUTION.md` and the About screen |

## 2. What was deliberately not repeated

| Mistake | Where | YAOPLM |
|---|---|---|
| Directory listings through the unauthenticated GitHub API: 60 requests per hour, a 403 treated as "rate limited, try later", no cache | Orbit `listAvailableArt` (`api.github.com/repos/.../contents/...`); Tbx caches the listing but still calls the API | Only raw URLs that are known in advance: `covers/default/<SERIAL>.jpg` and `Named_Boxarts/<Redump name>.png`; the name comes from the bundled DAT, never from a listing |
| One art database only, a static dump of OPL Manager's server, with no licence | Orbit (`Luden02/psx-ps2-opl-art-database`), Nova (`oplnova_vapi` serves an archive.org backup) | xlenore then libretro-thumbnails, both fetched on the user's request and never bundled (ADR-0007); the OPL Manager backups stay a v0.2 user import (ART-5) |
| Requiring the source to already be an 8-bit indexed PNG and resizing with nearest-neighbour; anything else is "incompatible" and skipped | Orbit `normalizeArtworkPng` / `resizeIndexedPng` | Any PNG/JPEG/BMP/GIF/WebP is decoded, resized with Lanczos3 and encoded as 8-bit RGB or RGBA; a palette is a RiptOPL recommendation, not a requirement (research/07, ART-6 in v0.3) |
| Upscaling to non-standard sizes (353×500) | Nova | Sizes come from `yaoplm_formats::art::recommended_size` per profile: 140×200 for OPL, 280×400 for RiptOPL |
| A downloaded file written straight into `ART/`, with no plan, no confirmation and no backup | Orbit, Tbx, PCT, PyOPLM | Every write is a plan the user confirms; the bytes live in the cache on the host and the journal stays small (ADR-0025) |
| Up to ~32 candidate URLs walked per game for indexed screenshot variants | Orbit `artSlotFileNames` (`SCR_00`..`SCR_30`) | At most one xlenore URL and three libretro names per game, a second apart |
| An archive.org `view_archive.php` URL as the art source, which extracts from a 7z on the Internet Archive's servers for every cover | PCT `GameArtDownloader::makeUrl` | Not used; archive.org packs are a user import (ART-5) |

## 3. Decisions made while building

- **Fetch is a job, the write is a plan.** A bulk download of 500 covers takes minutes at one request per second, so it cannot be a synchronous "plan" request. The job scans the device, asks the sources, logs one line per game and ends with a plan that lists exactly what will be written, with real sizes; the user confirms that plan like any other write. The second phase takes seconds (ADR-0025).
- **Titles are bundled, covers are not.** `data/titles.json.gz` (12 978 serials, 377 KB) embeds PCSX2's titles and the Redump names, which also solves the serial → thumbnail-name problem offline (ADR-0024). Cover bytes are cached per user under the app data folder.
- **Mirrors double as test fixtures.** A source pointed at a folder with the repository's layout never touches the network; `make_library` writes such mirrors, so the e2e tests and CI stay offline (TESTING.md).
- **Region-aware names.** libretro-thumbnails files are Redump names; one serial often has several (revisions, PAL language sets). The DAT names are ordered by PCSX2's region (`NTSC-U` → `USA`, `PAL-*` → the European family) and the first three are tried.
- **No palette quantisation yet.** RiptOPL's "8-bit PNG required" turned out to mean "not 16 bits per channel" (research/07); 8-bit RGB/RGBA satisfies every loader. Indexed output stays with ART-6.

## 4. Still open

- ICO, SCR, SCR2, BG, LGO, LAB and COV2 (ART-2 in v0.2): libretro has snaps, titles and logos; xlenore has 3-D covers; nothing open has discs or spines.
- OPL Manager backup zips as a user import (ART-5), with the per-file naming checked against a real archive.
- A courtesy note to the xlenore and libretro-thumbnails maintainers about the integration (research/03 §7.4).
