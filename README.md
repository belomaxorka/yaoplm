# YAOPLM — Yet Another OPL Manager

Open-source, cross-platform manager for PlayStation 2 game libraries used with [Open PS2 Loader](https://github.com/ps2homebrew/Open-PS2-Loader) (OPL) and its forks.

> **Status: pre-alpha.** Nothing to download yet. See [docs/ROADMAP.md](docs/ROADMAP.md).

## Planned features

- Windows, Linux (including Steam Deck) and macOS, native, with no .NET or Wine.
- One core, three front ends: a desktop app, a CLI (`yaoplm`) and a web UI server (`yaoplm-server`, Docker) for your NAS or home server.
- Import ISO, ZSO, BIN/CUE, CHD and zip/7z images. Game IDs are read exactly as OPL reads them. Files get OPL-friendly names.
- Correct `ul.cfg` splitting for games over 4 GB on FAT32. exFAT is supported for OPL 1.2.
- Cover art from open sources, converted to the PNG sizes OPL expects. No reliance on private servers.
- Dump verification against Redump DATs. Per-game CFG editor that preserves unknown keys. Formatted VMC creation.
- Loader profiles: OPL 1.2.0 beta, OPL 1.1.0, RiptOPL; more later.
- PS1 (POPStarter), APPS and cheats. Internal PS2 HDD support later.
- Every change is shown as a plan before anything is written. No telemetry.
- English and Russian UI.

## Documentation

- [Specification](docs/SPEC.md) (RU)
- [Architecture](docs/ARCHITECTURE.md) (RU)
- [Roadmap](docs/ROADMAP.md) (RU)
- [Decisions](docs/DECISIONS.md) (RU)
- [Testing](docs/TESTING.md) (RU)
- [Research](docs/research/README.md) (EN): competitors, OPL on-disk formats, data sources, stack

## Development

Requirements: Rust via [rustup](https://rustup.rs) (the version is pinned in `rust-toolchain.toml`), Node.js 24+ and pnpm through corepack (`npm install -g corepack && corepack enable pnpm`). On Windows, Visual Studio Build Tools with the C++ workload and WebView2. On Linux, the [Tauri system libraries](https://v2.tauri.app/start/prerequisites/).

```sh
pnpm install
cargo test --workspace            # Rust tests
pnpm -C web test                  # UI unit tests
pnpm -C web build                 # build the SPA into web/dist
cargo run -p yaoplm-server        # web UI and API on http://127.0.0.1:7878
pnpm -C web e2e                   # Playwright against yaoplm-server
cd apps/desktop && pnpm tauri dev # desktop app
cargo run -p yaoplm-cli -- rpc '{"type":"Ping"}'
```

`yaoplm-server` listens on loopback by default. Listening on another address requires `--token`; open the UI once as `http://<host>:7878/?token=<token>`. Allowed library folders are given with `--root <dir>` (repeatable). See [ADR-0009](docs/DECISIONS.md) and ADR-0012.

## Legal

YAOPLM does not include or download games, BIOS files or other proprietary PlayStation software. Use dumps of discs you own. Not affiliated with Sony, the OPL project or OPL Manager.

## License

[GPL-3.0-or-later](LICENSE)
