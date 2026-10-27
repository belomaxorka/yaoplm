# CLAUDE.md — YAOPLM agent rules

YAOPLM (Yet Another OPL Manager) is an open-source, cross-platform manager for PlayStation 2 game libraries used with Open PS2 Loader (OPL) and its forks. One Rust core drives three front ends: a Tauri 2 desktop app, a CLI (`yaoplm`) and a web server (`yaoplm-server`, Docker).

Read before working:
- `docs/SPEC.md`: what we build. Requirement IDs such as `IMP-5` go in commits and PRs.
- `docs/ARCHITECTURE.md`: crates, API layer, job model, stack versions.
- `docs/ROADMAP.md`: milestones and the work cycle. Always know which milestone you are in.
- `docs/DECISIONS.md`: ADRs. Do not silently contradict them; add a new ADR instead.
- `docs/TESTING.md`: test levels, fixtures, test-data policy.
- `docs/research/`: source research. `02-opl-formats.md` is the format reference; cite its sections in code comments where a rule comes from OPL source.

## Communication
- Talk to the user in **Russian**. They own the product, test on a real PS2 and review PRs.
- Write code, comments, commit messages, PR titles and descriptions, README and i18n keys in **English**. Docs in `docs/` are in Russian.
- At the end of each milestone, give the user a short checklist of what to test on the console.
- If you need something from the user (disc dumps, a USB stick prepared by OPL Manager, console tests), ask with a concrete list.

## Non-negotiable rules
1. **User data safety.**
   - Every write goes through plan → confirm → execute.
   - Writes are atomic: temp file plus rename.
   - Multi-step operations keep a journal.
   - Never write outside the selected root.
   - Delete user files only after an explicit confirmation.
   - Never write to a real removable drive in automated tests. Use temp dirs and FAT32/exFAT image files.
2. **Byte-exact OPL formats.** Follow OPL source (research/02), not other managers' behaviour. Key invariants:
   - Game ID parse = OPL's: `SYSTEM.CNF` `BOOT2` (case-sensitive), text after `:`, strip `\`, stop at `;`, max 11 chars. No `BOOT2` → OPL hides the game.
   - Old-style names: `_` at index 4, `.` at 8 and 11 (`SLUS_200.62.Title.iso`). Title ≤160 chars; filename ≤159 bytes on SMB.
   - `ul.cfg`: packed 64-byte records:
     - `name[32]` at 0x00;
     - `"ul."` at 0x20;
     - `startup[12]` at 0x23;
     - `parts` at 0x2F;
     - `media` at 0x30 (0x12 CD / 0x14 DVD);
     - 0x08 at 0x35;
     - zeros elsewhere.
   - `ul` chunk files:
     - named `ul.%08X.%s.%02x`: uppercase CRC, lowercase part number;
     - every chunk except the last is exactly 1 GiB;
     - at most 10 parts on SMB;
     - names must be ASCII and ≤32 bytes.
   - `USBA_crc32` is NOT standard CRC-32:
     - table built with poly 0x04C11DB7 and an inverted top-bit test, stored reversed;
     - the initial value is the leftover from table generation;
     - hashes the terminating NUL, up to 33 bytes;
     - no final XOR.
     - Port it exactly and test with known vectors.
   - CFG: `key=value`, CRLF, key ≤31, value ≤255, never write `#` keys, **preserve unknown keys and their order**.
   - `$Compatibility` bits 0x01–0x20 upstream; 0x40/0x80 only in the RiptOPL profile.
   - Art: `ART/<ID>_<SUFFIX>.png`, always PNG, ≤1024 px (ADR-0005).
   - Delete `CD/games.bin` / `DVD/games.bin` after changing those folders.
   - ZSO: 24-byte LE header `ZISO`, block 2048, u32 index with bit 31 = uncompressed, raw LZ4 blocks.
3. **Legal and data sources (ADR-0007).**
   - Never call OPL Manager's private art server.
   - Never bundle cover images.
   - Never download commercial game images, BIOS or POPS files.
   - Bundled data must have a compatible licence and attribution in `data/ATTRIBUTION.md`.
   - Respect rate limits, send a User-Agent `YAOPLM/<version> (+repo URL)`, cache everything.
4. **Core purity (ADR-0003).**
   - `yaoplm-core`, `-formats`, `-hash`, `-device` and `-i18n` must not depend on tauri, axum or tokio. CI checks this with `cargo tree -i <crate>`.
   - The core is synchronous.
5. **Secrets.** Never commit tokens, signing keys or `.env`. The updater signing key lives only in GitHub Secrets.
6. **Prior art first (ADR-0015).** Upstream OPL, its forks and neighbouring tools hold years of fixes for edge cases and hardware quirks.
   - Before implementing a format, a device feature or a loader rule, read the matching code in upstream OPL and the forks, plus their commits and issues. The index is `docs/research/REFERENCES.md`; clones live in `$YAOPLM_TESTDATA\refs\<owner>__<repo>` (`git -C <dir> pull` to refresh).
   - Upstream OPL defines behaviour. Forks and other managers are a source of edge cases to verify, not to copy blindly.
   - Cite the source file and commit in a code comment. Record new findings in `docs/research/`.
   - Loader-detection signs (research/16) are snapshots of moving repos, YAOPL included. Before relying on a sign or changing detection, re-check it against the current code of every build it names, and update the commit it was checked at.
   - Licences: never copy AFL code (OPL, ps2sdk) — reimplement from the logic (ADR-0002). GPL-3-compatible code may be ported with attribution.
   - Forums: psx-place blocks bots; read threads through the user's Chrome (Claude in Chrome), never bypass the check.

## Stack (versions verified 2026-10-22; re-check before adding a dependency)
- Rust stable (MSVC on Windows) pinned in `rust-toolchain.toml`.
- Tauri 2.12, axum 0.8, clap, ts-rs 12.
- Do not use tauri-specta (still RC), `serde_yaml` (archived) or `serde_yml` (unsound). Use `serde_norway` or `serde_yaml_ng` for YAML.
- Formats and data: hadris-iso 2.5, chd 0.4 (feature `chd`), zip 9, sevenz-rust2 0.24, `unrar` only behind feature `rar`, crc32fast, sha1/md-5 0.11, lz4_flex 0.14, image 0.25, fast_image_resize 6, quick-xml 0.42, encoding_rs 0.8 (Shift-JIS titles of memory card saves; already in the tree through reqwest), reqwest 0.13 (rustls), rusqlite (bundled), windows-sys 0.61 and rustix 1.1 for volumes (ADR-0019; no sysinfo), smb 0.12 (feature `smb`).
- Frontend: React 19, TypeScript 6 (strict), Vite 8, pnpm 12 (via corepack), TanStack Query, TanStack Virtual, shadcn/ui (`radix-nova`, `cn` helper), Tailwind 4, React Router 7, react-i18next (`en`, `ru`), vitest, Playwright.
  - Stay on TypeScript 6 until typescript-eslint supports TS 7 (it requires `<6.1` as of 2026-10-22).
  - React Router 8 exists; moving to it needs an ADR.
- Before adding a crate or npm package, check its maintenance and licence. GPL-3-compatible only.
- Pin exact npm versions. pnpm refuses releases younger than a day (`minimumReleaseAge`): pick an older version, never add `minimumReleaseAgeExclude`.

## Code style
- Rust:
  - `cargo fmt`; `cargo clippy --all-targets --all-features -- -D warnings`;
  - no `unwrap()`/`expect()` in library code outside tests;
  - errors via `thiserror` in libs and `anyhow` in apps;
  - small modules, one format per module in `yaoplm-formats`.
- Each format module starts with a doc comment that links the research/02 section and the OPL source file it mirrors.
- TS: strict mode, no `any`. API types come only from `web/src/api/generated/` (ts-rs output), never hand-written.
- i18n (`docs/I18N.md`): every user-visible string goes through i18n, with keys in every supported language (`en`, `ru`) in the same commit as the feature, following the glossary, plural and typography rules there. Texts the core shows to users are `Phrase`s (`Phrase::title`/`error`/`notice`/`reason` with named params, crate `yaoplm-i18n`), never English-only strings: the core renders English from the `en` catalogue, so a new key goes into every language in the same commit, and the i18n test checks that each phrase sets exactly the parameters its template uses (ADR-0029). Technical details (OS messages, parser positions) go in as text parameters. `pnpm -C web test` checks key parity, plural forms, placeholders, duplicate keys, typography, untranslated copies and that every core notice code and keyed enum value has a translation; reread new strings in both languages before a PR.
- Texts are for end users, not engineers (owner, 2026-10-26; `docs/I18N.md`, "Для кого тексты"): one sentence, what happened and what to do; how it works inside (paths, files, signs, versions, hashes) goes to a tooltip, the advanced mode or the docs; the same action gets the same wording everywhere (device card, library, plan, CLI); look at how OPL Manager and similar tools say it before writing new strings.
- Keep comments sparse and factual: why, not what.

## Commands
Update this section if they change.
- Rust: `cargo fmt --all`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace`
- Core purity (ADR-0003): `bash scripts/check-core-deps.sh`
- Types: `cargo test -p yaoplm-api --features ts-export` (regenerates `web/src/api/generated/`; CI fails if it is stale)
- Web: `pnpm install`, then `pnpm -C web lint`, `pnpm -C web typecheck`, `pnpm -C web test`
- Release UI pass (docs/plans/release-ui-consistency.md): with the `yaoplm-server-fixture` preview running, `pnpm -C web exec playwright test --config tests/release/playwright.config.ts` walks every screen in both modes, languages and themes at 1024 px (screenshots, axe and layout reports in `web/test-results/release/`). `node web/tests/release/desktop.mjs <text|shot|goto|…>` drives the dev desktop over CDP 9222.
- E2E: `pnpm -C web build`, then `pnpm -C web e2e` (`web/tests/e2e/server.mjs` builds a fixture library with `make_library` in a temp folder and starts `yaoplm-server` on 127.0.0.1:7879 with it; once per machine: `pnpm -C web exec playwright install chromium`). Screenshots land in `web/test-results/screenshots/`.
- Fixture library: `cargo run -p yaoplm-api --example make_library -- <dir> [--games N]` (`<dir>/library`, `<dir>/sources`, `<dir>/mirror` with a cover per art source and loader releases in `loaders/` and, one beta newer, `loaders-next/`, OPL 1.1.0's ELF in `own/`, an empty `<dir>/stick`, and `<dir>/extras` with a ZSO and an ISO with a JPG cover)
- Title database (META-1, ADR-0024): `python scripts/build-data.py` rebuilds `data/titles.json.gz` and `data/ATTRIBUTION.md` from PCSX2 GameIndex and the libretro PS2 DAT (needs PyYAML; `--pcsx2 FILE --libretro FILE` for offline builds). Commit the result.
- Game info table (META-3, ADR-0050): `python scripts/build-game-info.py` rebuilds `data/game-info.json.gz` from the libretro developer DAT and a Wikidata extract it fetches by SPARQL (saved to `target/game-info/wikidata-ps2.json`; `--libretro FILE --wikidata FILE` for offline builds, `--wikidata none` for facts only), then `python scripts/build-data.py --attribution-only` refreshes `data/ATTRIBUTION.md`. Commit both. The genre vocabulary lives in the script (`GENRES`, `LIBRETRO_GENRES`, `WIKIDATA_GENRES`) and in `genre.*` of both catalogues; a new libretro phrase fails the build. Script tests: `python -m unittest discover -s scripts/tests`.
- Desktop dev: `pnpm tauri dev` (from `apps/desktop`; starts Vite on 5173)
- Server: `cargo run -p yaoplm-server -- --root <dir> --bind 127.0.0.1:7878` (serves `web/dist`; debug builds read it from disk). `--data-dir <dir>` or `YAOPLM_DATA_DIR` places the art cache; the default is the platform's app data folder (ADR-0025).
- CLI: `cargo run -p yaoplm-cli -- rpc '{"type":"Ping"}'`; on Windows PowerShell 5.1 pipe the JSON instead: `'{"type":"Ping"}' | cargo run -p yaoplm-cli -- rpc`
- Previews: `.claude/launch.json` has `yaoplm-server` (7878, no roots), `yaoplm-server-fixture` (7878, fresh fixture library), `yaoplm-server-fixture-3000` (the same with 3000+ games) and `web-dev` (Vite, 5173, proxies `/api` to 7878). Stop the server preview before `cargo test` or a cargo build of the server: Windows locks the running `.exe`.
- Running processes (read-only): `powershell -NoProfile -File scripts/dev-processes.ps1` lists running YAOPLM programs (dev builds and the installed app) with their parents, the app's WebView2 profiles and the dev ports 5173, 7878–7880 and 9222.
- Test data folder and volumes (read-only): `. scripts/testdata.ps1` sets `YAOPLM_TESTDATA` and `YAOPLM_OPL_SRC` and lists the volumes with their serials; `-Bash` prints exports for Git Bash (see Environment).
- Real-data tests: set `YAOPLM_TESTDATA`, then `cargo test --workspace -- --ignored` (CFG/CHT corpus: `cargo test -p yaoplm-formats --test corpus -- --ignored --nocapture`; OPL Manager backup names: `python scripts/oplm-backup-toc.py OPLM_ART_2024_09 OPLM_ART_2023_07` reads only the zips' directories from archive.org into `$YAOPLM_TESTDATA\oplm-art`, then `cargo test -p yaoplm-formats --test oplm_backups -- --ignored --nocapture`)
- Loader ELF corpus (LIB-1, LDR-4, research/16 §1.6, §13): `python scripts/loader-corpus.py fetch` (official release assets of OPL, its forks, NHDDL and Neutrino through `gh`, unpacked into `$YAOPLM_TESTDATA\downloads\loaders`), then `python scripts/loader-corpus.py expect` (expected versions from names and git history), then with `YAOPLM_TESTDATA` set: `cargo test -p yaoplm-formats --test loader_elfs -- --ignored --nocapture` (every ELF's version, damaged copies) and `cargo test -p yaoplm-core --test loader_corpus -- --ignored --nocapture` (detection in clean, renamed, mixed and cut installs; about 8 minutes). A loader ELF's version and size: `cargo run -p yaoplm-formats --example elf_version -- <file.elf>...`.
- OPL cross-checks (C `USBA_crc32`, `pc/ziso.py`, `pc/genvmc`; needs a C compiler and Python with `lz4`): with `YAOPLM_OPL_SRC` set to `$YAOPLM_TESTDATA\refs\ps2homebrew__Open-PS2-Loader` (`scripts/testdata.ps1` sets it), `cargo test -p yaoplm-formats --test opl_oracles -- --ignored`
- VMC read by other tools (mymcplusplus, genvmc samples of ps2vmc-tool): with `YAOPLM_TESTDATA` set, `cargo test -p yaoplm-formats --test vmc_oracles -- --ignored`. The memory card reader against mymcplusplus on the factory SD's boot cards, genvmc samples and the fixture's cards (ADR-0048): `cargo test -p yaoplm-formats --test memcard_oracles -- --ignored --nocapture`; what the factory boot cards hold: `cargo test -p yaoplm-core --test emulator -- --ignored --nocapture`. The fixture's boot cards for other tools: `cargo run -p yaoplm-test-support --example emulator_cards -- <dir>`. A file's fragment count, read only: `YAOPLM_FRAGMENT_FILES="E:\file;..." cargo test -p yaoplm-device --test fragments -- --ignored --nocapture`
- Fuzz (nightly, `cargo install cargo-fuzz`): `cd fuzz && cargo +nightly fuzz run -O -a <iso|system_cnf|cfg|ul_cfg|zso|cht|tar|ps2_elf|cue|memcard|ps1mc|sd2psx_ini> -- -max_total_time=60`, plus `-dict=dict/<target>.dict` where one exists. Keep `-a`: without it overflows wrap silently. On Windows add `--no-include-main-msvc` (`fuzz/build.rs`) and the MSVC ASan runtime folder (`VC\Tools\MSVC\<ver>\bin\Hostx64\x64`) to `PATH`; in PowerShell quote `'-dict=dict/cue.dict'`. Seed ISOs: `cargo run -p yaoplm-test-support --example write_fixtures -- fuzz/corpus/iso`
- Library tools: `cargo run -p yaoplm-cli -- scan <root>` and `cargo run -p yaoplm-cli -- info <image>` (`--json`, `--profile opl-1.2|opl-1.1|riptopl`)
- Devices (read-only): `yaoplm devices [--all]`, `yaoplm device <root>` (also the loader profile and why, LIB-1, the mode and the version question). Every command's `--profile` defaults to `auto`: the device's files decide (ADR-0039); `opl-1.2`, `opl-1.1`, `riptopl` force one. `device` and `scan` take `--mode auto|ps2|smb|emulator` (ADR-0047, ADR-0048): `auto` goes by the device's place (removable media is plugged into the PS2; folders on the computer, network folders and server roots are read over SMB, with SMB's rules and no fragment rule), except that a memory card emulator's files (`.sd2psx/`, `MemoryCards/<PS2|PS1|COH|SC2|PROT>`, MemCard PRO 2's `mcp2.bin`) in the folder or above it make it the emulator's SD: `device` lists its boot channels, and in any mode YAOPLM puts no loader there and never writes its service files.
- Dump checks (GAME-5, META-2, read-only): `yaoplm verify <root> [--game G]... [--md5] [--fresh] [--json]` (exit code 1 on a bad dump or an unreadable image), `yaoplm redump [--update | --import FILE] [--json]` (`--update` asks Redump at most once a week; tests and CI always use `--import`).
- Writes print a plan and need `--yes` (or "y" on a terminal): `yaoplm init <root> [--folder NAME]`, `yaoplm add <root> <files, folders, zip or 7z> [--move] [--ul] [--zso] [--title T] [--media cd|dvd] [--no-info]` (by default the bundled game info goes into the CFG of every added game the table knows, META-3), `yaoplm convert <root> <game>... --to iso|zso|ul [--name-style id-title|title]`, `yaoplm rename <root> <game> <title> [--id ID]`, `yaoplm rm <root> <game> [--with-related]`, `yaoplm mv <root> <game> --to cd|dvd`, `yaoplm recover <root> [--resume]`. `<game>` is an ID, title, file name, path or `ul:<index>`. Never point them at a real drive without the owner's confirmation (rule 1).
- Game settings and memory cards (CFG-2, VMC-1, ADR-0036): `yaoplm cfg <root> <game>` shows them; `--mode N`, `--no-mode N`, `--vmc SLOT=NAME`, `--alt-startup X`, `--set KEY=VALUE`, `--unset KEY` plan a change. Game info (META-3, ADR-0050): `yaoplm cfg <root> <game> --info` shows the bundled facts next to the CFG's keys; `--write-info [--fields title,genre,release,developer,publisher,players]` plans writing them (CFG-2 rules, backup, `Players` + `PlayersText`, `Release` as `MM-DD-YYYY`); renaming a game updates an existing `Title` in its CFG. `yaoplm vmc ls <root>`, `yaoplm vmc create <root> <name> [--size 8|16|32|64] [--game G --slot 0|1] [--replace]`.
- Covers (ART-1..4, ADR-0035): `yaoplm art fetch <root> [--all] [--game ID]... [--kind T]... [--mirror xlenore=DIR] [--mirror oplm=DIR] [--mirror libretro=DIR] [--skip-source S]`, `yaoplm art set <root> <game> <image> [--keep-existing]`, `yaoplm art rm <root> <game>`, `yaoplm art cache [--clear]`, `yaoplm about`. Sources: xlenore, oplm (OPL Manager's archive.org backup by range requests), libretro, screenscraper (only with `YAOPLM_SCREENSCRAPER_DEVID`/`_DEVPASSWORD`, optional `_USER`/`_PASSWORD`; never commit them). `fetch` goes to the network only without a mirror; tests and CI always use mirrors, and `YAOPLM_OFFLINE=1` (set in CI and the e2e server) refuses every non-local host.
- Picture sources on the live network, by hand, never in CI: `YAOPLM_LIVE=1 cargo test -p yaoplm-net --test live -- --ignored --nocapture`.
- Picture source coverage (real data): `python scripts/art-source-lists.py` (into `$YAOPLM_TESTDATA\art-lists`), then with `YAOPLM_TESTDATA` set, `cargo test -p yaoplm-api --test art_coverage -- --ignored --nocapture`.
- Real FAT32/exFAT drivers (Linux, mounted image files; CI job `real-fs`): `YAOPLM_FAT32_DIR=/mnt/fat32 YAOPLM_EXFAT_DIR=/mnt/exfat cargo test -p yaoplm-core --test real_fs -- --ignored`. Fragment counts on them (IMP-7b; also macOS, CI job `real-fs-macos`): `cargo test -p yaoplm-device --test fragments -- --ignored fragments_on_real_drivers --nocapture` with the same variables; `YAOPLM_FRAGMENTS_KNOWN=1|0` says whether the OS must tell (Linux tells root only; macOS 15 does not tell on FAT32/exFAT).
- Loader catalogue (LDR-1, ADR-0042): `yaoplm loader ls [--lang en|ru] [--json]`. Data: `data/loaders/index.toml` plus one `<id>.toml` per build. No hash history of new builds: a build is known by the version inside its ELF (ADR-0041); `data/loader-hashes/opl-stable.txt` only holds frozen releases.
- Loader downloads (LDR-2, ADR-0043; the app data `loaders/` store, no device is touched): `yaoplm loader release <build> [--tag T] [--mirror DIR]` (what a download would be), `yaoplm loader download <build> [--tag T] [--mirror DIR] [--yes]` (held to the hash it printed), `yaoplm loader import <file>` (own ELF, zip or 7z; no network), `yaoplm loader files`, `yaoplm loader update-catalog [--mirror DIR]`. A mirror has GitHub's layout (`repos/<o>/<r>/releases/tags/<tag>.json`, `<o>/<r>/releases/download/<tag>/<asset>`, `data/loaders/`); `yaoplm_test_support::loaders::ReleaseMirror::official` writes one with test ELFs. Tests and CI always use mirrors.
- Loader install (LDR-3, BOOT-1, ADR-0044): `yaoplm loader install <root> <build> [--preset fmcb|ps2bbl|app]... [--path P] [--mx4sio] [--language Russian] [--starter-settings] [--replace-foreign] [--file F | --tag T --mirror DIR]` downloads (after a confirmation), plans, and prints the launcher lines. Updates and rollbacks (LDR-4, ADR-0045): `yaoplm loader update <root> [--path P] [--mirror DIR]` (reads the release, then a plan replacing only the ELF and its language pack), `yaoplm loader rollback <root> [--path P]` (swaps the ELF with its newest backup); `yaoplm device` prints the installed build and whether a newer one is known from the cache. Choice and replacement (LDR-6, LDR-7, ADR-0046): `yaoplm loader releases <build> [--mirror DIR]` (the releases the catalogue allows), `yaoplm loader replace <root> <build> [--preset P]... [--path P] [--keep-zso] [--keep-art] [--retire-previous] [--file F | --tag T --mirror DIR]` (says what changes for the games, then one plan: conversions, pictures, the loader last). Never at a real drive without the owner's confirmation (rule 1).
- Console test disc: `cargo run -p yaoplm-test-support --example make_boot_iso -- --elf <homebrew.elf> --out <image.iso> [--min-mib 4300]`

## Workflow (details in docs/ROADMAP.md)
- One branch per milestone or task: `m<N>/<slug>`.
- Conventional Commits in English with requirement IDs: `feat(formats): ul.cfg writer (IMP-5)`.
- Before every commit: fmt, clippy, tests. Before every PR: the full CI set locally where possible.
- Every bug fix is proven (owner's rule, 2026-10-26): first a test that reproduces the bug and fails, then the fix, then the same test green, and the affected suites rerun more than once. Check the fix against upstream OPL and the other official repos in `refs/` before and after (rule 6), take their handling of the case where they have one, and say in the PR how it was verified.
- For UI work, run `yaoplm-server` or the Vite dev server through `.claude/launch.json`. Check it in the browser preview or with Playwright, and attach screenshots (light/dark, en/ru) to the PR.
- PR description: summary, requirement IDs, how it was tested, **PS2 test checklist** for the user. The checklist also goes into `docs/testing/` as a file (in Russian) with a row in `docs/testing/README.md`, the list of everything still waiting for the console; results are recorded there when the owner runs it.
- After merge: tick ROADMAP checkboxes and add an ADR if a decision was made.
- Before a release tag (`v0.N.0`), and after PRs that change the device card, the library header, the storage doctor or the "what next" hints: run the UI consistency pass in `docs/plans/release-ui-consistency.md` (one story per device, no actions that cannot work, one main button, visible reasons, axe on every state).

## Environment (developer machine)
- Windows 11, PowerShell plus Git Bash. VS Build Tools 2026 and WebView2 are installed.
- Hardware (since 2026-10-25): 64 GB RAM, 12 cores / 16 threads, 2 TB system SSD. `cargo test` and E2E can run side by side. Cargo locks one target dir per run, so a parallel run gets its own: `CARGO_TARGET_DIR=target\e2e` for `pnpm -C web e2e` while `cargo test` runs. Run `pnpm -C web test` on its own: next to a cargo build, vitest's 5 s timeouts trip (9 of 112 on 2026-10-26). Timings on this machine: first build of the workspace with tests about 4 min, `cargo test --workspace` about 2.5 min, E2E about 7 min, `loader_corpus` about 8 min.
- Rust via rustup (`stable-x86_64-pc-windows-msvc`; the repo pins its version in `rust-toolchain.toml`), in `%USERPROFILE%\.cargo\bin`.
- Node 26 no longer bundles corepack: it is installed with `npm install -g corepack`, then `corepack enable pnpm`. Set `COREPACK_ENABLE_DOWNLOAD_PROMPT=0` in non-interactive shells.
- Python 3.15 (with `PyYAML` and `lz4` for the scripts and OPL cross-checks), Go, git, `gh` (account `belomaxorka`). Rust nightly and `cargo-fuzz` for fuzzing.
- Docker is not local: Docker images build in CI.
- Test data lives in a `yaoplm-testdata` folder at the root of one of the owner's drives, outside the repo. Drive letters change with what is plugged in, so the docs write `$YAOPLM_TESTDATA` for that folder. In a command meant to be pasted, write `$env:YAOPLM_TESTDATA\...` for PowerShell and `"$YAOPLM_TESTDATA"/...` for Git Bash: unquoted `$YAOPLM_TESTDATA\out` breaks in both.
  - `. scripts/testdata.ps1` (PowerShell) finds it by name, sets `YAOPLM_TESTDATA` and `YAOPLM_OPL_SRC` in that shell and lists the volumes with their serials. In Git Bash: `eval "$(powershell -NoProfile -File scripts/testdata.ps1 -Bash)"`. Tool shells forget variables between calls: run it in the same call as the command that needs them.
  - The rest of that drive holds the owner's private files: never read or write outside the test data folder. Never commit game images or anything from that folder.
  - The clones in `refs\` belong to the SID of an earlier Windows install, so git needs `safe.directory <letter>:/yaoplm-testdata/refs/*` for the drive's current letter in the owner's global config. If git refuses a clone after the letter moved, ask the owner.
- The user's PS2 hardware (FMCB card, memory card emulator, MX4SIO, USB sticks) and the console test matrix are in `docs/TESTING.md`. When the user connects a device, inspect it read-only first; write only after a plan, the user's confirmation and a re-check of the disk identity.
- Know a drive by its volume serial, never by its letter. The owner's test stick (64 GB, FAT32, serial `3C76-1797`) may be written and wiped in live tests (owner, 2026-10-26): check its serial right before, then leave it in a working state and say what is on it.
- Device sessions (`docs/TESTING.md`, "Сессии с устройствами владельца"): the owner will connect sticks and the emulator's factory-prepared microSD. Record every layout read-only first (in `docs/research/`, no binaries), then install and swap OPL builds only through plans, and test on the console.
- **Shared machine: processes and locks.** Several sessions and the owner use this machine, often the same checkout and `target/`.
  - Before building, testing or starting the app or a server, run `scripts/dev-processes.ps1`. A running `yaoplm-*.exe` from `target\` locks it: cargo fails with "Access is denied" (os error 5). A running desktop app holds the WebView2 profile `%LOCALAPPDATA%\io.github.belomaxorka.yaoplm\EBWebView`: a second instance with other WebView2 arguments (a CDP port) fails with 0x8007139F. Dev ports may be taken.
  - Never stop a process you did not start. The installed app (`%LOCALAPPDATA%\YAOPLM\`) and anything started from the owner's terminal belong to the owner; another session's servers belong to it. Work around them: another port, a dev desktop with its own profile (`WEBVIEW2_USER_DATA_FOLDER=<scratchpad folder>`, which also keeps the owner's settings untouched), or ask the owner.
  - Start servers through `preview_start` (the app tracks and stops them). Stop what you started (`preview_stop`, the dev desktop and its Vite) as soon as you no longer need it, and always before you end your turn on a task: never leave a server, the desktop app or a CDP port running for the next session.
- The user has authorised installing whatever the dev environment needs. Prefer `winget`, `rustup`, `corepack` and `cargo install` from official sources.
