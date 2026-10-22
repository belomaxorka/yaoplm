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
   - `yaoplm-core`, `-formats`, `-hash` and `-device` must not depend on tauri, axum or tokio. CI checks this with `cargo tree -i <crate>`.
   - The core is synchronous.
5. **Secrets.** Never commit tokens, signing keys or `.env`. The updater signing key lives only in GitHub Secrets.
6. **Prior art first (ADR-0015).** Upstream OPL, its forks and neighbouring tools hold years of fixes for edge cases and hardware quirks.
   - Before implementing a format, a device feature or a loader rule, read the matching code in upstream OPL and the forks, plus their commits and issues. The index is `docs/research/REFERENCES.md`; clones live in `D:\yaoplm-testdata\refs\<owner>__<repo>` (`git -C <dir> pull` to refresh).
   - Upstream OPL defines behaviour. Forks and other managers are a source of edge cases to verify, not to copy blindly.
   - Cite the source file and commit in a code comment. Record new findings in `docs/research/`.
   - Licences: never copy AFL code (OPL, ps2sdk) — reimplement from the logic (ADR-0002). GPL-3-compatible code may be ported with attribution.
   - Forums: psx-place blocks bots; read threads through the user's Chrome (Claude in Chrome), never bypass the check.

## Stack (versions verified 2026-10-22; re-check before adding a dependency)
- Rust stable (MSVC on Windows) pinned in `rust-toolchain.toml`.
- Tauri 2.12, axum 0.8, clap, ts-rs 12.
- Do not use tauri-specta (still RC), `serde_yaml` (archived) or `serde_yml` (unsound). Use `serde_norway` or `serde_yaml_ng` for YAML.
- Formats and data: hadris-iso 2.5, chd 0.4 (feature `chd`), zip 9, sevenz-rust2 0.24, `unrar` only behind feature `rar`, crc32fast, sha1/md-5 0.11, lz4_flex 0.14, image 0.25, fast_image_resize 6, quick-xml 0.42, reqwest 0.13 (rustls), rusqlite (bundled), windows-sys 0.61 and rustix 1.1 for volumes (ADR-0019; no sysinfo), smb 0.12 (feature `smb`).
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
- TS: strict mode, no `any`. API types come only from `web/src/api/generated/` (ts-rs output), never hand-written. Every user-visible string goes through i18n with both `en` and `ru` keys.
- Keep comments sparse and factual: why, not what.

## Commands
Update this section if they change.
- Rust: `cargo fmt --all`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace`
- Core purity (ADR-0003): `bash scripts/check-core-deps.sh`
- Types: `cargo test -p yaoplm-api --features ts-export` (regenerates `web/src/api/generated/`; CI fails if it is stale)
- Web: `pnpm install`, then `pnpm -C web lint`, `pnpm -C web typecheck`, `pnpm -C web test`
- E2E: `pnpm -C web build`, then `pnpm -C web e2e` (starts `yaoplm-server` on 127.0.0.1:7879; once per machine: `pnpm -C web exec playwright install chromium`)
- Desktop dev: `pnpm tauri dev` (from `apps/desktop`; starts Vite on 5173)
- Server: `cargo run -p yaoplm-server -- --root <dir> --bind 127.0.0.1:7878` (serves `web/dist`; debug builds read it from disk)
- CLI: `cargo run -p yaoplm-cli -- rpc '{"type":"Ping"}'`; on Windows PowerShell 5.1 pipe the JSON instead: `'{"type":"Ping"}' | cargo run -p yaoplm-cli -- rpc`
- Previews: `.claude/launch.json` has `yaoplm-server` (7878) and `web-dev` (Vite, 5173, proxies `/api` to 7878). Stop the server preview before `cargo test`: Windows locks the running `.exe`.
- Real-data tests: set `YAOPLM_TESTDATA=D:\yaoplm-testdata`, then `cargo test --workspace -- --ignored` (CFG/CHT corpus: `cargo test -p yaoplm-formats --test corpus -- --ignored --nocapture`)
- OPL cross-checks (C `USBA_crc32`, `pc/ziso.py`; needs a C compiler and Python with `lz4`): `YAOPLM_OPL_SRC=D:\yaoplm-testdata\refs\ps2homebrew__Open-PS2-Loader cargo test -p yaoplm-formats --test opl_oracles -- --ignored`
- Fuzz (nightly, `cargo install cargo-fuzz`): `cd fuzz && cargo +nightly fuzz run -O <iso|system_cnf|cfg|ul_cfg|zso|cht> -- -max_total_time=60`. On Windows add the MSVC ASan runtime folder (`VC\Tools\MSVC\<ver>\bin\Hostx64\x64`) to `PATH`. Seed ISOs: `cargo run -p yaoplm-test-support --example write_fixtures -- fuzz/corpus/iso`
- Library tools: `cargo run -p yaoplm-cli -- scan <root>` and `cargo run -p yaoplm-cli -- info <image>` (`--json`, `--profile opl-1.2|opl-1.1|riptopl`)
- Devices (read-only): `yaoplm devices [--all]`, `yaoplm device <root>`
- Writes print a plan and need `--yes` (or "y" on a terminal): `yaoplm init <root>`, `yaoplm add <root> <files or folders> [--move] [--ul] [--title T] [--media cd|dvd]`, `yaoplm rename <root> <game> <title> [--id ID]`, `yaoplm rm <root> <game> [--with-related]`, `yaoplm mv <root> <game> --to cd|dvd`, `yaoplm recover <root> [--resume]`. `<game>` is an ID, title, file name, path or `ul:<index>`. Never point them at a real drive without the owner's confirmation (rule 1).
- Real FAT32/exFAT drivers (Linux, mounted image files; CI job `real-fs`): `YAOPLM_FAT32_DIR=/mnt/fat32 YAOPLM_EXFAT_DIR=/mnt/exfat cargo test -p yaoplm-core --test real_fs -- --ignored`
- Console test disc: `cargo run -p yaoplm-test-support --example make_boot_iso -- --elf <homebrew.elf> --out <image.iso> [--min-mib 4300]`

## Workflow (details in docs/ROADMAP.md)
- One branch per milestone or task: `m<N>/<slug>`.
- Conventional Commits in English with requirement IDs: `feat(formats): ul.cfg writer (IMP-5)`.
- Before every commit: fmt, clippy, tests. Before every PR: the full CI set locally where possible.
- For UI work, run `yaoplm-server` or the Vite dev server through `.claude/launch.json`. Check it in the browser preview or with Playwright, and attach screenshots (light/dark, en/ru) to the PR.
- PR description: summary, requirement IDs, how it was tested, **PS2 test checklist** for the user.
- After merge: tick ROADMAP checkboxes and add an ADR if a decision was made.

## Environment (developer machine)
- Windows 11, PowerShell plus Git Bash. VS Build Tools 2026 and WebView2 are installed.
- Rust via rustup (`stable-x86_64-pc-windows-msvc`; the repo pins its version in `rust-toolchain.toml`), in `%USERPROFILE%\.cargo\bin`.
- Node 26 no longer bundles corepack: it is installed with `npm install -g corepack`, then `corepack enable pnpm`. Set `COREPACK_ENABLE_DOWNLOAD_PROMPT=0` in non-interactive shells.
- Python 3.14, Go, git, `gh` (account `belomaxorka`).
- Docker is not local: Docker images build in CI.
- Test data lives in `D:\yaoplm-testdata\` (outside the repo). Never commit game images or anything from that folder.
- The user's PS2 hardware (FMCB card, memory card emulator, MX4SIO, USB sticks) and the console test matrix are in `docs/TESTING.md`. When the user connects a device, inspect it read-only first; write only after a plan, the user's confirmation and a re-check of the disk identity. D: is the user's data drive: write only under `D:\yaoplm-testdata`.
- The user has authorised installing whatever the dev environment needs. Prefer `winget`, `rustup`, `corepack` and `cargo install` from official sources.
