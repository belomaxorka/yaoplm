# 04 — Technology stack research for a cross-platform OPL library manager

_Research date: 2026-10-22. Method: WebSearch and WebFetch only. No binaries were downloaded or run. Versions come from GitHub release pages, lib.rs/docs.rs, the npm registry and pkg.go.dev as of this date. Anything that could not be confirmed from a primary source is marked **(unverified)**._

---

## 0. TL;DR

| Decision | Recommendation |
|---|---|
| **Desktop shell** | **Tauri 2** (stable 2.12.x). Rust core, system webview. |
| **Core language** | **Rust**, as a Cargo workspace of plain library crates with no Tauri or axum dependencies. |
| **CLI** | `clap` binary over the same core crates. |
| **Headless web mode** | `axum` 0.8 binary that serves the same SPA (embedded) plus JSON-RPC over HTTP, with WebSocket for progress events. Ships as a static Docker image. |
| **Frontend** | **React 19 + TypeScript + Vite**, TanStack Virtual/Query, shadcn/ui (Tailwind). Svelte 5 is a fine alternative if the developer prefers it. |
| **Runner-up** | **Wails v3 (Go)**. It has a built-in "server build" mode that fits the Docker requirement very well, but it is still beta (v3.0.0-beta.34). Go has no CHD library and weaker SMB maturity. |
| **Third** | Electron + Node/TS. Easiest for a JS developer, but 120–200 MB installers, and the native libraries for CHD/7z-write/SMB/raw disk are weak or missing. |
| **Signing** | Windows: SignPath Foundation (free for OSS). macOS: Apple Developer Program ($99/yr) for Developer ID + notarization; until then, ad-hoc signing. Linux: none required; the Tauri updater key is mandatory and free. |
| **CI** | GitHub Actions matrix with `tauri-apps/tauri-action@v1`. Separate jobs for the CLI/server binaries and a multi-arch Docker image (GHCR). |
| **Tests** | Generate synthetic ISO/BIN/CUE/DAT fixtures in code (a test-support crate). Use FAT32 disk images in-process, Samba in a CI service container, and fuzzing for parsers. Never use real games. |

---

## 1. What drives the choice (requirements → technical pressure)

| Requirement | Pressure on stack |
|---|---|
| Hash multi-GB files (CRC32/MD5/SHA1) | Needs native code with SIMD and multi-threaded fan-out, and must avoid GC or JS-heap copies. |
| Copy/split 4+ GB files to USB (FAT32/exFAT) | Large sequential I/O, preallocation, progress and cancellation. |
| ISO9660 / BIN/CUE / CHD / ZSO / 7z / RAR / ZIP | Library breadth. CHD and ZSO are niche. |
| PS2 memory cards (VMC), APA/PFS HDD | No mainstream libraries exist; they will be written in-house or wrapped. |
| Raw block device access, maybe formatting | Privilege elevation per OS, plus sandbox limits (Flatpak, macOS TCC, Docker). |
| SMB client | Ideally pure-language, with no libsmbclient dependency. |
| CLI + desktop GUI + Docker web UI from **one core** | The core must be a UI-agnostic library. The web frontend must be transport-agnostic. |
| Main developer: PHP/JS. AI writes much of the code | Prefer well-documented, high-training-data ecosystems and strong compiler feedback. Avoid fast-moving APIs. |
| Windows, Linux (incl. Steam Deck), macOS | Linux webview quality (WebKitGTK) and packaging (AppImage/Flatpak) matter. |

---

## 2. Tauri 2 (Rust core + web frontend)

### 2.1 Version / status
- **tauri crate v2.12.5 (21 Oct 2026)**, tauri-cli 2.12.4, `@tauri-apps/cli` 2.12.4, `@tauri-apps/api` 2.12.4 (npm "latest"). The blog announced "Tauri 2.12" on 26 Sep 2026. Tauri 2.0 stable shipped 2 Oct 2024. [GitHub releases, tauri.app/blog]
- **Tauri v3 is in alpha:** `tauri v3.0.0-alpha.5` (14 Oct) on GitHub releases. The alpha line includes work on a **CEF (Chromium) runtime** option. A May 2026 digest mentions a CEF data-dir fix in a v3 alpha. The exact v3 alpha numbering conflicts between sources, and the CEF scope and timeline are **(unverified)**. Plan on v2; v3 is a later migration.
- Some plugins (updater, dialog) document **Rust ≥ 1.90** as a requirement.

### 2.2 Bundle sizes
- Official claim: a minimal Tauri app "can be less than 600KB" because it uses the system webview. [v2.tauri.app/start]
- Typical hello-world **installers are about 3–9 MB** on Windows/macOS (secondary sources, **unverified** as benchmarks).
- **Linux AppImage: "70+ MB"**, because it bundles WebKitGTK and its dependencies (official AppImage docs). deb/rpm stay small because they depend on the system `libwebkit2gtk-4.1`.
- Idle RAM is often quoted at about 30–50 MB versus 120–300 MB for Electron (secondary, **unverified**).

### 2.3 Linux packaging
| Format | Support | Notes |
|---|---|---|
| **deb, rpm** | Built by `tauri build` | Depends on system webkit2gtk-4.1. |
| **AppImage** | Built by `tauri build` (linuxdeploy) | Must build on the **oldest base you support** (Ubuntu 22.04 / Debian 12 recommended) to avoid glibc errors. ARM AppImages must be built on ARM runners (`ubuntu-22.04-arm`); linuxdeploy can't cross-compile. Destination paths for custom files must start with `/usr/`. |
| **Flatpak / Flathub** | **Manual** `flatpak-builder` manifest that unpacks the `.deb` from `tauri build -b deb`. Needs `flatpak-cargo-generator` and `flatpak-node-generator` offline sources, GNOME runtime (docs mention 46/47), and AppStream metainfo | Best channel for **Steam Deck** (Discover store). The sandbox blocks raw block devices unless you add `--device=all`. |
| Snap, AUR | Documented distribution guides | — |

Linux webview caveats: Tauri's "Linux Graphics Issues" page documents blank or white windows and flicker, mostly from the WebKitGTK DMABUF renderer on NVIDIA. Workarounds include `WEBKIT_DISABLE_DMABUF_RENDERER=1` and `WEBKIT_DISABLE_COMPOSITING_MODE=1`. Third-party projects report newer WebKitGTK versions crashing inside AppImages with `EGL_BAD_PARAMETER`; xmr-btc-swap pinned WebKitGTK 2.44. **Steam Deck specifically: no reports found (unverified). Test early on real hardware.**

### 2.4 macOS signing / notarization
- **Developer ID Application certificate + notarization require the paid Apple Developer Program: $99 USD/year** (fee waivers exist only for nonprofits, education and government). A free account can't notarize.
- CI env vars: `APPLE_CERTIFICATE` (base64 .p12), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`. For notarization, either the App Store Connect API (`APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_KEY_PATH`) or an Apple ID (`APPLE_ID`, `APPLE_PASSWORD` as an app-specific password, `APPLE_TEAM_ID`).
- **Ad-hoc signing** (`"signingIdentity": "-"`) is free and needed on Apple Silicon. Users still have to approve the app in **System Settings → Privacy & Security**, because macOS Sequoia removed the Control-click Gatekeeper bypass for unnotarized apps.
- Notarization only works from macOS (fine on `macos-latest` runners).

### 2.5 Windows / WebView2
- WebView2 ships with **Windows 10 (April 2018+) and Windows 11**. Installer modes: `downloadBootstrapper` (default, +0 MB), `embedBootstrapper` (+~1.8 MB), `offlineInstaller` (+~127 MB), `fixedVersion` (+~180 MB), `skip`.
- Installers: **NSIS `-setup.exe`** (can be cross-built from Linux/macOS, less tested) and **MSI** (WiX v3, Windows-only, needs the VBSCRIPT feature).

### 2.6 Auto-updater (`tauri-plugin-updater`)
- **Signatures are mandatory** ("This cannot be disabled"). Generate a keypair with `tauri signer generate`. The public key goes in `tauri.conf.json` and the private key in `TAURI_SIGNING_PRIVATE_KEY`. Losing the key strands existing installs.
- **Static JSON** (e.g. `…/releases/latest/download/latest.json` on GitHub Releases, generated by tauri-action) or a **dynamic server** (`{{target}}/{{arch}}/{{current_version}}`; 204 means no update).
- Updatable bundles: **AppImage** (Linux), **.app.tar.gz** (macOS), **NSIS .exe / .msi** (Windows). **deb/rpm are not self-updated** (docs don't mention them). Flatpak updates come through Flathub.
- **No built-in channels.** Implement them by choosing the endpoint at runtime (stable vs beta JSON) or adding a `?channel=` query.
- `createUpdaterArtifacts: true` (the `"v1Compatible"` value goes away in v3).

### 2.7 File dialogs, drag-and-drop
- `tauri-plugin-dialog`: open file(s)/folder(s) (`multiple`, `directory`), save with filters, message/ask/confirm. Supported on Windows, Linux and macOS. Desktop returns real filesystem paths.
- Drag-drop: native OS drops are intercepted while `dragDropEnabled` is true (the default), and `getCurrentWebviewWindow().onDragDropEvent()` delivers **absolute paths** on `enter` and `drop`. `over` carries only the position, by design. Setting `dragDropEnabled: false` restores HTML5 DnD but loses paths; it's required for HTML5 DnD on Windows.

### 2.8 Long-running tasks + progress
- Commands can be `async` (tokio runtime). CPU- or IO-heavy work belongs on `spawn_blocking` / rayon / dedicated threads.
- **Use `tauri::ipc::Channel<T>` for progress.** Per the docs, channels are "designed to be fast and deliver ordered data", and Tauri uses them internally for download progress and child-process output. Global events (`emit`) are "not designed for low latency or high throughput". Pattern: `#[tauri::command] async fn start_hash(job: JobSpec, on_event: Channel<JobEvent>)`, with cancellation via `tokio_util::sync::CancellationToken` held in a job registry in `State`.

### 2.9 Sharing one Rust core with a CLI and an axum server
Recommended shape:

```
crates/oplm-core     (pure library: scan, formats, hash, copy, jobs; sync std::fs + threads; NO tauri/axum/tokio in public API)
crates/oplm-api      (transport-agnostic RPC: `enum Request`/`enum Response`/`enum JobEvent` (serde, tagged) + `async fn dispatch(ctx, Request)`;
                      JobManager with broadcast of JobEvent; DTOs derive ts-rs → generated TS types)
apps/cli             (clap → calls oplm-core directly; indicatif progress)
apps/server          (axum 0.8: POST /api/rpc → dispatch; GET /api/events (WebSocket, axum "ws" feature) or SSE; GET /media/cover/:id;
                      SPA embedded via rust-embed or served by tower-http ServeDir; token auth)
apps/desktop         (Tauri 2: one `rpc` command → dispatch; one `subscribe(channel)` command → JobEvent stream;
                      custom URI scheme for covers → same handler as server route)
```

- **Frontend transport switch:** `isTauri()` from `@tauri-apps/api/core` (added in api 2.0.0-beta.9) picks `TauriTransport` (`invoke('rpc')` + `Channel`) or `HttpTransport` (`fetch('/api/rpc')` + WebSocket). Every UI call goes through `api.call(req)`, so each feature is written once.
- Typed bindings: **ts-rs 12.0.1** (derive TS from Rust types, serde-compatible) is transport-neutral. **tauri-specta 2.0.0-rc.26** is still RC and Tauri-only, so it's not recommended as the sole source of types.
- Native-only features in web mode: replace native dialogs with a **server-side path browser** (restricted to configured roots such as `/games`, `/usb`, SMB mounts). Drag-drop becomes "paste a server path". The tray and system menu are unavailable.
- An alternative some apps use is to always run an embedded axum server on `127.0.0.1:random` inside the desktop app and use HTTP everywhere (e.g. the Codeberg *aesthesis* project). It's simpler (one transport) but adds a localhost attack surface that needs auth tokens. The dual-transport dispatcher above is only about 100 lines more and keeps Tauri's IPC security model.

---

## 3. Wails (Go)

### 3.1 Status
- **Wails v2.17.0** (18 Oct) is the stable line, marked "Latest" on GitHub.
- **Wails v3.0.0-beta.34** (20 Oct) is an automated nightly-style prerelease from master: "The API is stable, but you may still encounter issues before the final 3.0 release." **No date for v3.0 final.**

### 3.2 v3 features relevant to us
- **Server build mode (big plus):** `go build -tags server` runs the same app as a plain HTTP server with no native windows or GUI deps. **CGO is not required**, service bindings work, events go over WebSocket, and each browser tab becomes a pseudo-window (`browser-N`). It includes a `/health` endpoint and graceful SIGTERM, a generated distroless `Dockerfile.server`, and `WAILS_SERVER_HOST`/`PORT`. **No auth built in.** Tray, dialogs and menus are stubbed.
- **Packaging:** `wails3 package` produces **AppImage, DEB, RPM, Arch** (via nfpm, with PGP signing). Windows gets **NSIS** by default and **MSIX** optionally. macOS signing and notarization run through `wails3 task darwin:sign:notarize`, plus `wails3 setup signing`. **No Flatpak docs.**
- **Linux webview:** the default is **GTK4 + WebKitGTK 6.0**. GTK3/WebKit2GTK 4.1 is available via `-tags gtk3` and is to be **removed in v3.1**. Newer distros only; Steam Deck compatibility is **unverified**.
- **Updater:** built-in `app.Updater` with providers (GitHub Releases, keygen.sh, Sparkle AppCast, Wails manifest). It checks a SHA-256 digest with an optional Ed25519 signature, swaps the binary and relaunches. Behaviour is still changing between betas.
- Generated TS bindings from Go services, events, multi-window, and file drop with paths.
- **CI:** no official action for v3. `dAppServer/wails-build-action` targets v2. For v3, call the `wails3` CLI directly in a matrix.

### 3.3 Verdict
Wails v3 fits the "CLI + desktop + Docker web UI" requirement best on paper, because server mode is first-class. It loses on prerelease status, missing CHD support, and the Go SMB situation (see §7). Go is easier than Rust for a PHP developer to read and review.

---

## 4. Electron + Node

- **Electron 45.0.0** (20 Oct 2026) bundles Chromium 156 and Node 24.22. Lines 44 and 43 are still supported.
- **Size:** installers are typically **~120–200 MB** (e.g. a 151 MB electron-builder Windows installer) and idle RAM is ~120–300 MB (secondary sources, **unverified**).
- **Pros:**
  - One language (TS) for core, CLI, server and UI.
  - The core can be a plain Node package used by an Express/Fastify server in Docker with zero adaptation.
  - Consistent Chromium on every OS, so no WebKitGTK problems on Linux.
  - Mature tooling (electron-builder, electron-updater).
  - balenaEtcher proves Electron + native drivelist works for raw disk writing.
- **Native performance:**
  - Hashing is fine: `crypto.createHash('md5'|'sha1')` is OpenSSL (asm, SHA-NI), and `zlib.crc32` exists since Node 22.2.
  - Copy via `fs` streams or `copyFile` is OS-bound and fine.
  - Parallel multi-hash needs worker_threads, and buffer handling through the JS heap adds overhead (**unverified** magnitude).
- **Cons for this project:**
  - No CHD reader.
  - 7z write and RAR need WASM or external binaries.
  - SMB clients are old or weak.
  - Raw disk and drive enumeration need native addons (node-gyp/N-API) built per platform and arch.
  - Large downloads for a utility app.
  - Electron's security model needs care (contextIsolation, preload).
- A hybrid (Electron + napi-rs Rust core) brings Rust's complexity anyway, and then Tauri is strictly smaller.
- Related: **Electrobun** (Bun + system webview, v1.0 Feb 2026, npm 1.18.x) is young. On Linux it recommends bundling CEF. Not recommended yet.

---

## 5. Others (brief)

| Stack | Status (Oct 2026) | Fit |
|---|---|---|
| **Avalonia (.NET/C#)** | Avalonia 12 released Apr 2026 (12.1.x by Oct per Wikipedia, **unverified**); MIT; licensing/"Accelerate" brand overhaul | Very good native desktop on all three OSes, and .NET has strong I/O. **But** it's C#/XAML (new language and UI paradigm for the developer), and the web UI would be a separate app (Avalonia Browser/WASM is client-side, not a server UI). Library gaps for CHD/SMB are similar. |
| **Flutter desktop (Dart)** | Stable on all three desktops. Multi-window still not public in stable (3.44.x, internal API behind flag, **unverified**) | Good UI toolkit, but the Dart ecosystem lacks ISO/CHD/SMB/7z. You'd end up with a Rust core via flutter_rust_bridge anyway. The web UI would be Flutter web plus a separate backend. |
| **Qt 6 (C++/QML or PySide)** | Qt 6.10 (Oct 2025), 6.11 released; LGPLv3/GPLv2 | Most mature native toolkit, but C++ is a poor fit for a PHP/JS developer and AI-heavy workflow. LGPL compliance and large deployments. The web UI needs separate code (Qt WebAssembly is client-side). |

---

## 6. Stack comparison table

| Criterion | **Tauri 2 (Rust)** | **Wails v3 (Go)** | Wails v2 (Go) | **Electron (Node/TS)** | Avalonia | Flutter | Qt |
|---|---|---|---|---|---|---|---|
| Status | **Stable 2.12.5**; v3 alpha | **Beta 34** (nightly prereleases) | Stable 2.17.0 (maintenance) | Stable 45.0.0 | Stable 12.x | Stable | Stable 6.11 |
| Installer size (typ.) | ~3–10 MB (AppImage 70+ MB) | ~10–20 MB (**unverified**) | similar | ~120–200 MB | ~30–80 MB self-contained (**unverified**) | ~20–40 MB (**unverified**) | ~30–80 MB (**unverified**) |
| Rendering | WebView2 / WKWebView / WebKitGTK 4.1 | WebView2 / WKWebView / **WebKitGTK 6 (GTK4)** | WebKit2GTK 4.0/4.1 | Bundled Chromium | Skia native | Skia/Impeller native | Native |
| Linux pkgs | deb, rpm, AppImage built-in; Flatpak manual | AppImage, deb, rpm, Arch built-in; no Flatpak docs | deb/AppImage via scripts | AppImage, deb, rpm, snap, flatpak (electron-builder) | manual | snap/deb manual | manual |
| Updater | Official plugin, signed (mandatory) | Built-in `app.Updater` (new) | none built-in | electron-updater (mature) | 3rd-party | 3rd-party | 3rd-party |
| Same code as **CLI** | Yes, Cargo workspace | Yes, Go packages | Yes | Yes, Node package | Yes, .NET lib | Partly | Yes |
| Same code as **Docker web UI** | Yes: axum adapter + same SPA (~2 small adapters) | **Built-in server mode** (`-tags server`) | No | Yes: Node server + same SPA | Separate UI needed | Separate | Separate |
| Hash/copy perf | Native, SIMD, threads | Native, stdlib asm | Native | OK (OpenSSL), JS-heap overhead | Native (.NET) | Needs FFI | Native |
| Library coverage for our needs | **Best** (CHD, ISO r/w, 7z r/w, SMB pure, LZ4, image) | Good (no CHD; ISO, FAT32, 7z read, RAR pure-Go) | same as Go | Weak (CHD, SMB, raw disk) | Medium | Weak | Medium (C/C++ libs) |
| Learning curve (PHP/JS dev) | **Steep** (Rust), mitigated by AI + compiler | Moderate (Go is simple) | Moderate | **Lowest** | Steep (C#/XAML) | Moderate (Dart) | Very steep |
| CI tooling | **tauri-action@v1** (matrix, release, latest.json) | DIY with `wails3` CLI | dAppServer action | electron-builder / Forge | DIY | DIY | DIY |
| E2E testing | WebDriver on Win/Linux; macOS via WebdriverIO plugin | Playwright against server mode | — | Playwright Electron | Headless | integration_test | Squish/QtTest |
| **Verdict** | **Recommended** | **Runner-up** (re-evaluate at v3.0 final) | No (no server mode) | Third | No | No | No |

---

## 7. Library availability (Rust vs Go)

Legend: **M** = mature/widely used, **A** = active but young/niche, **S** = stale/unmaintained, **✗** = nothing usable found.

| Need | Rust crate (version, maturity) | Go package (maturity) | Recommendation / notes |
|---|---|---|---|
| **ISO9660 read** (find SYSTEM.CNF) | **`hadris-iso` 2.5.0** stable (3.0.0-rc.2 Oct 2026), MIT. Read/write ISO9660 L1–3, Joliet, Rock Ridge, El Torito, no_std read, async optional — **A/M**. `cdfs` 0.2.3 (2023) — **S**. `iso9660` 0.1.1 (2023, "incomplete") — **S** | `kdomanski/iso9660` v0.4.0 (Aug 2023; read+write, no Joliet, RR experimental) — **A/S**. `diskfs/go-diskfs` v1.9.4 (Jul 2026; iso9660 read/write-finalize) — **M** | Rust: `hadris-iso`. Only the PVD, root directory and one file are needed, so a ~200-line in-house reader is a valid fallback and doubles as a fuzz target. |
| **ISO9660 write** (test fixtures, BIN→ISO not needed) | `hadris-iso` writer | kdomanski / go-diskfs | Use for synthetic test ISOs (§10). |
| **BIN/CUE parse + BIN→ISO** | `rcue` (pure, small), `cue_lib` 0.1 (pure, no_std, no multi-FILE), `cue` (libcue bindings, **GPL-2**) — **A/S** | `lmvgo/cue` (Unlicense), `Lotlab/cue-go` (GPL-3) — **A/S** | **Write our own** (~200 lines): PS2 CD dumps are MODE2/2352 (data offset 24) or MODE1/2352 (offset 16). Multi-track Redump CD sets need per-track hashing. |
| **CHD read** | **`chd` 0.4.0 (16 Oct 2026)**, pure safe Rust, libchdr-compatible, V5 codecs LZMA/Deflate/FLAC/Huffman/Zstd/CD-*, optional `encode` — **A/M** (~9.5k dl/mo). `libchdman-rs` (MAME C++ wrapper, read+write) | **✗** (no Go CHD library found; cgo to libchdr would be needed) | Rust wins clearly. |
| **ZIP** | **`zip` 9.0.5** (Oct 2026), Deflate/Deflate64/Bzip2/Zstd/LZMA/XZ/PPMd read, AES — **M** | stdlib `archive/zip` — **M** | — |
| **7z** | **`sevenz-rust2` 0.24.1** (Oct 2026), pure Rust, read **and write**, LZMA/LZMA2/BCJ/BCJ2(dec)/PPMd/zstd/brotli/AES — **M** (~414k dl/mo). Original `sevenz-rust` unmaintained | `bodgit/sevenzip` (read-only, io/fs, BCJ2, AES) — **M**. `mholt/archives` wrapper | — |
| **RAR** | `unrar` 0.5.8 (Feb 2025), wraps RARLAB C++ unrar — **M**, but **UnRAR licence is not OSI/GPL-compatible (unverified wording)** | `nwaples/rardecode` (pure Go, via mholt/archives; read-only) — **M** | License risk in Rust: make RAR an optional cargo feature, or shell out to a user-installed `unrar`/`7z`. Go's pure decoder is a real advantage here. |
| **CRC32** | **`crc32fast` 1.5.3** (20 Oct 2026). Runtime-detected PCLMULQDQ, VPCLMULQDQ AVX2/AVX-512 (Rust ≥1.89), aarch64 CRC. Old benchmark: 7.3 GB/s (pclmul) — **M** | stdlib `hash/crc32` (PCLMUL/arm64 CRC asm) — **M** | — |
| **MD5** | `md-5` 0.11.0 (Mar 2026), pure Rust — **M**. MD5 is inherently serial (~0.6–0.9 GB/s per core, **unverified**) | stdlib `crypto/md5` (amd64 asm) — **M** | MD5 is the bottleneck on NVMe. Make it optional; Redump matching needs size+CRC32 to find a candidate and SHA1 to confirm. |
| **SHA1** | `sha1` 0.11.0 (Mar 2026), uses `cpufeatures` (SHA-NI/ARMv8 SHA detection likely, **unverified** in docs) — **M**. Alternative: `ring`/`aws-lc-rs` SHA1 (asm) | stdlib `crypto/sha1`. Source has `sha1block_amd64_shani` (SHA-NI). FreeBSD's port of Go's SHA-NI code: 10 GiB in 5.6 s ≈ 1.8 GiB/s — **M** | Benchmark both in CI. |
| **Multi-hash pipeline** | Own code: one reader thread (1–8 MiB aligned buffers, `Arc<[u8]>`) fans out to 3 hasher threads via `crossbeam-channel`; rayon for many files | goroutines + `io.MultiWriter` | Read each file **once**. Cache results in SQLite keyed by (path, size, mtime). |
| **LZ4 (ZSO)** | **`lz4_flex` 0.14.0** (Jul 2026), block + frame, safe by default — **M** (13.7M dl/mo) | `pierrec/lz4/v4` v4.1.26 (Mar 2026), block + frame — **M** | ZSO = CSOv1 layout with "ZISO" magic + LZ4 **raw blocks** (2048-byte blocks). Reference is OPL's `pc/ziso.py`. maxcso's ZSO is reported unreliable. Implement in-house and test against ziso.py output in CI. |
| **Image decode/encode** | **`image` 0.25.10** (Mar 2026; PNG/JPEG/WebP/BMP/…) — **M** | stdlib `image/png`, `image/jpeg`, `golang.org/x/image` (webp decode) — **M** | — |
| **Image resize** | **`fast_image_resize` 6.1.0** (Jul 2026), SSE4.1/AVX2/NEON/wasm SIMD, `image` integration — **M** | `golang.org/x/image/draw` (CatmullRom, pure Go, slower) — **M** | — |
| **XML (Redump/Logiqx DAT)** | **`quick-xml` 0.42.0** (Aug 2026), serde via `serialize`, ~50× xml-rs — **M** | stdlib `encoding/xml` — **M** | Stream-parse DATs. |
| **YAML** | `serde_yaml` **archived (Mar 2024)**. **Avoid `serde_yml`** (unsound, RUSTSEC-2025-0068). Use `serde_norway` / `serde_yaml_ng` (drop-in forks) or `serde-saphyr` (new) — **A** | `go.yaml.in/yaml/v3` (frozen, security fixes); v4 in RC (v4.0.0-rc.6) — **M**. `goccy/go-yaml` — **M** | Prefer TOML/JSON for our own config; YAML only for consuming external lists. |
| **HTTP client + retries** | **`reqwest` 0.13.5** (Sep 2026, rustls default). Built-in `reqwest::retry` (scoped `for_host`, budgets, no backoff by default). Or `reqwest-middleware` + `reqwest-retry` (exponential backoff) — **M** | stdlib `net/http` + `hashicorp/go-retryablehttp` — **M** | — |
| **SMB client** | **`smb` 0.12.1** (Sep 2026), pure Rust, SMB 2.x/3.x, signing, encryption (AES-CCM/GCM), compression, NTLM + Kerberos (via `sspi`), QUIC, async tokio — **A** (4k dl/mo). `pavao` 0.3.1 (Sep 2026), libsmbclient C binding, no documented Windows support, calls serialized process-wide — **A** | `hirochachacha/go-smb2` (pkg.go.dev latest tag **v1.1.0, Mar 2022**; GitHub README now lists SMB 3.1.1, QUIC, Kerberos — **unverified** whether released). Fork `cloudsoda/go-smb2`. **Advisory GO-2026-5051** (OOB read/panic in ReadDir) affects both — **A/S** | Rust `smb` is the best pure option but young, so keep it optional. In Docker, the simplest route is to **mount the share on the host** (CIFS) and treat it as a folder. Windows can use UNC paths natively. |
| **List removable drives + FS type** | **`sysinfo` 0.39.6**: `Disk::is_removable()`, `file_system()`, `kind()`, `mount_point()`, `total/available_space()` — **M**. `rs-drivelist` 0.9.3 (port of Node drivelist; physical-disk view) — **A**. `disket` 0.1 (2025, mount/unmount/hotplug) — **A (young)** | `shirou/gopsutil` v4 `disk.Partitions` (fstype, mountpoint; **no removable flag**) + per-OS code (sysfs `removable`, IOKit, Win32 `GetDriveType`) — **M + DIY** | Use sysinfo for mounted volumes. Use an rs-drivelist-style per-OS layer for physical disks (raw/HDD features). |
| **Raw block device access** | `std::fs::File` on `\\.\PhysicalDriveN` / `/dev/sdX` / `/dev/rdiskN` plus per-OS ioctls (`windows-sys`, `nix`). `gptman`/`mbrman` for partition tables — **DIY** | `go-diskfs` (block devices + MBR/GPT; per-OS files present) — **M** | See §7.2. |
| **FAT32 create/format** | `fatfs` 0.3.6 (**Jan 2023**, FAT12/16/32, LFN, `format_volume`) — **S** but widely used (130k dl/mo). **`hadris-fat` 2.x** (same author as hadris-iso; FAT12/16/32 read/write/format; exFAT behind `unstable-exfat`) — **A** | `go-diskfs` fat32 create/read/write — **M** | Use for **test images**. Use on real drives only behind heavy safeguards (§7.1). |
| **PS2 memory card (VMC)** | `ps2-memcard` 0.2.0 (brand new: superblock, indirect FAT, dir tree, ECC), `ps2mc` 0.1.0 — **A (very young)** | **✗** | Write in-house using the Ross Ridge PS2 MCFS spec. Use ps2-memcard as a reference only. |
| **APA/PFS (PS2 HDD)** | **✗** (no crate found) | **✗** | Reference C code: `ps2homebrew/pfsshell` (GPL-2; APA/PFS/iomanX libs are **AFL-2.0**; includes `pfsfuse`) and `ps2homebrew/hdl-dump` (HDLoader installs, ZSO support; license in COPYING, **likely GPL-2 — unverified**). Phase 1: bundle them as **sidecars** built from source in CI. Phase 2: native Rust port (license review needed). |
| **Typed TS bindings** | `ts-rs` 12.0.1 — **M**. `specta`/`tauri-specta` 2.0.0-rc.26 — **A** | Wails generates bindings — **M** | — |
| **Web server** | **`axum` 0.8.9** (Apr 2026; `ws` feature, SSE) + `tower-http` + `rust-embed` — **M** | stdlib `net/http` / Wails server mode — **M** | — |
| **Privilege elevation** | `runas` 1.2.0 (Feb 2024) — **S**. DIY: Windows `ShellExecuteW "runas"`, Linux `pkexec`, macOS `osascript … with administrator privileges` / SMAppService helper | DIY | Run raw-disk ops in an **elevated helper process** (our CLI binary in `--helper` mode) that talks JSON lines over stdio. |

### 7.1 Formatting drives from the app: feasible, but not advisable for v1
- **Windows:** the built-in Format GUI caps FAT32 at 32 GB. `format /FS:FAT32` often fails on big volumes ("The volume is too big for FAT32"). Formatting large FAT32 needs a custom formatter (Rufus, fat32format-style), which means raw-volume access as admin plus lock/dismount ioctls.
- **macOS:** `diskutil eraseDisk FAT32|ExFAT NAME MBR diskN` works (needs admin for some cases).
- **Linux:** `mkfs.vfat` / `mkfs.exfat` as root, or the udisks2 D-Bus `Format` method with polkit authorization (**unverified** details; udisks2 has had recent CVEs such as CVE-2025-8067).
- **OPL context:** OPL ≥1.2.0 supports **exFAT** on USB (BDM), which removes the 4 GB limit and the need to split ISOs (the old "ul" USBExtreme split format is only needed on FAT32). Older guidance says OPL files on USB must be **contiguous (defragmented)**; whether exFAT under BDM tolerates fragmentation is **unverified**.
- **Recommendation:**
  - v1: **don't format.** Detect the FS type (sysinfo), explain the options, link the OS tools, and recommend exFAT when the user's OPL ≥ 1.2.
  - Preallocate destination files (`set_len`) and write sequentially to keep them contiguous.
  - Add a **fragmentation check**: Windows `FSCTL_GET_RETRIEVAL_POINTERS`, Linux `FIEMAP`, macOS `F_LOG2PHYS`.
  - If formatting is added later, wrap OS tools (diskutil / udisks2 / an elevated custom FAT32 formatter via `hadris-fat`/`fatfs`) with strict guards: removable-only, show model+size, type-the-label-to-confirm, never touch the system disk.

### 7.2 Raw block device access (PS2 HDD)
- **Windows:**
  - Open `\\.\PhysicalDriveN` as admin (UAC).
  - I/O must be sector-aligned (with `FILE_FLAG_NO_BUFFERING`).
  - A PS2 APA disk has no Windows-recognized volumes, which helps. If it does, `FSCTL_LOCK_VOLUME`/`DISMOUNT` first.
  - Enumerate with `IOCTL_STORAGE_GET_DEVICE_NUMBER` / SetupAPI.
- **Linux:**
  - `/dev/sdX` needs root or the `disk` group.
  - Use `pkexec` with our helper, or udisks2 `OpenDevice` (polkit; **unverified**).
  - **Flatpak blocks this** unless `--device=all`.
  - **Docker:** `--device=/dev/sdX` plus appropriate caps. On Proxmox LXC, the device must be passed through to the container first.
- **macOS:**
  - `/dev/rdiskN` needs root. Unmount first with `diskutil unmountDisk`.
  - TCC "Removable Volumes" prompts also apply to ordinary USB file access.
- **Design:** keep HDD features in a separate crate and helper process. Default to **disk-image files** (`.img`) so HDD logic is testable without hardware.

---

## 8. Frontend framework for the webview

| | **React 19** (npm latest 19.3.1) | **Svelte 5** (5.57.2) | **Vue 3** (3.5.44; 3.6 "Vapor" in RC) |
|---|---|---|---|
| AI code quality | **Best**: largest training corpus; AI rarely mixes up versions | Risk: AI often emits **Svelte 4** syntax instead of runes (`$state`, `$props`) | Good; Options vs Composition API mixing |
| Familiarity for a PHP dev | JSX is less template-like | Very approachable, closest to "HTML + JS" | Templates feel like Blade/Twig |
| Performance in a WebKitGTK webview | Fine with virtualization; slightly bigger runtime | Smallest runtime, fastest updates | Good; Vapor will help later |
| Virtualized grid/list | **TanStack Virtual 3.14.22** (React, Vue, Svelte, Solid, Lit, Angular adapters; 2026 perf work: 5× faster cold mount at 100k items), react-virtuoso | TanStack svelte-virtual | TanStack vue-virtual |
| Table | **TanStack Table** (npm latest **9.3.1**; most tutorials and AI knowledge are v8, so check the API) or AG Grid Community (built-in virtualization, heavier) | TanStack Table svelte adapter | TanStack / PrimeVue DataTable |
| Component libraries | **shadcn/ui** (Radix/Base UI + Tailwind, copy-in), Mantine, MUI | shadcn-svelte, Bits UI, Skeleton | PrimeVue, Naive UI, Vuetify, shadcn-vue |
| Data layer | TanStack Query (wrap `api.call`) | TanStack Query svelte | TanStack Query vue / Pinia |

**Pick: React + TS + Vite + TanStack Virtual/Query + shadcn/ui + Tailwind + Zustand (UI state).**
- **Why:** it's what an AI assistant writes most reliably, and it has the largest component ecosystem for a data-heavy grid of covers, tables, filters and job panels.
- **Svelte 5** is the better choice only if the developer will hand-write most UI and wants the smallest bundle.
- Either way:
  - Generate API types (ts-rs) into `web/src/api/generated`.
  - Always virtualize: grid view of covers (fixed-size cells), plus a table view.
  - Serve thumbnails at 2–3 fixed sizes from the core (pre-resized with fast_image_resize) through `api.coverUrl(id, size)`. In Tauri that's a custom URI scheme; on the server it's `/media/cover/...`.
  - Test layouts in **Safari/WebKit** (macOS + Linux) as well as Chromium.

---

## 9. CI/CD, release artifacts, signing, update channels

### 9.1 Workflows (GitHub Actions)
1. **`ci.yml`** (every PR):
   - `cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace` (ubuntu/windows/macos).
   - `pnpm lint && pnpm test` (Vitest).
   - Fuzz smoke (Linux, a few minutes).
   - Optional Playwright e2e against `apps/server`.
2. **`release-desktop.yml`** (on tag `v*`), using `tauri-apps/tauri-action@v1`:
   ```yaml
   matrix:
     include:
       - { platform: windows-latest,  args: '' }                                   # x64 NSIS + MSI
       - { platform: macos-latest,    args: '--target universal-apple-darwin' }    # or two jobs aarch64/x86_64
       - { platform: ubuntu-22.04,    args: '' }                                   # glibc baseline → deb, rpm, AppImage
       - { platform: ubuntu-22.04-arm,args: '' }                                   # arm64 AppImage/deb (free on public repos)
       # optional: windows-11-arm (free for public repos)
   ```
   - Linux deps: `libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf xdg-utils`.
   - Inputs: `tagName: v__VERSION__`, `releaseDraft: true`, `uploadUpdaterJson: true` (default), `updaterJsonPreferNsis: true`.
   - Env: `TAURI_SIGNING_PRIVATE_KEY(_PASSWORD)` and the Apple vars from §2.4.
   - Intel macOS runner `macos-15-intel` is the last x86_64 image (**until Aug 2027**); `macos-13` was retired Dec 2025.
3. **`release-cli-server.yml`**:
   - Build `oplm` (CLI) and `oplm-server` for x86_64/aarch64 Linux (musl, static), Windows and macOS, via a plain matrix or `cargo-dist` (0.31/0.32 in 2026, **unverified** latest).
   - Build and push a **multi-arch Docker image** (`linux/amd64,linux/arm64`) to GHCR with `docker/build-push-action`, using native arm runners instead of QEMU where possible.
   - Base image: `gcr.io/distroless/static` or `scratch` + CA certs. Expose 8080, volume `/data`, bind mounts for game folders.
4. **`flatpak.yml`** (later): build with the manifest and submit to Flathub. Flathub builds and signs itself.

### 9.2 Code signing options and costs (open source)
| Platform | Option | Cost | Notes |
|---|---|---|---|
| Windows | **SignPath Foundation** | **Free for OSS** | Requirements:<ul><li>OSI license with no commercial dual-licensing</li><li>no proprietary components</li><li>actively maintained and **already released**</li><li>fully automated build from the repo</li><li>a code-signing policy on the website</li></ul>The cert is issued to **SignPath Foundation** (it appears as the publisher). Used by Flameshot, GitExtensions and Stellarium. Integrates with GitHub CI. EV/OV type not stated (**unverified**). |
| Windows | Azure **Artifact Signing** (formerly Trusted Signing, renamed Jan 2026) | $9.99/mo Basic (5k signatures) | **Individuals: US/Canada only**; organizations: US/CA/EU/UK. |
| Windows | Commercial OV/EV certificate | ~$200–500/yr (**unverified**) | Since **1 Mar 2026** max validity is **460 days** (CA/B ballot CSC-31). Keys must be on an HSM/token. |
| Windows | Unsigned | $0 | SmartScreen "unknown publisher" warnings. Acceptable for early alphas. |
| macOS | **Apple Developer Program** | **$99/yr** | Required for Developer ID + notarization. Without it, ship ad-hoc signed builds plus instructions (Privacy & Security → "Open Anyway"). Apple program availability by country is **unverified**; check for the developer's region. |
| Linux | none required | $0 | Optionally sign deb/rpm repos with GPG; AppImage embedded signature optional. |
| Updater | Tauri updater keypair | $0 | Mandatory. Back up the private key offline (and as a GitHub secret). |

### 9.3 Update channels
- **Stable:** endpoint `https://github.com/<org>/<repo>/releases/latest/download/latest.json` (tauri-action uploads it).
- **Beta/nightly:** publish prereleases and upload a `latest-beta.json` to a fixed-tag release (e.g. `updater-beta`) or GitHub Pages. In-app setting "Update channel: stable/beta" sets the endpoint at runtime via the updater builder.
- **Linux deb/rpm users:** show an "update available" notice with a link, since those packages don't self-update. Flatpak users update via Flathub.
- **Docker:** tags `:latest`, `:beta`, `:x.y.z`. Watchtower/Proxmox users pull.

---

## 10. Testing strategy (no real games in the repo)

**1. A `test-support` crate builds fixtures at test time**, deterministically and from tiny sizes:
- **Synthetic ISO:** use the `hadris-iso` writer, or a hand-rolled minimal ISO9660 builder of about 150 lines (system area of 16 zero sectors, PVD at LBA 16, terminator, path tables, root dir). Files:
  - `SYSTEM.CNF` = `BOOT2 = cdrom0:\SLUS_999.99;1\r\nVER = 1.00\r\nVMODE = NTSC\r\n`
  - a dummy `SLUS_999.99` ELF stub
  - an optional padding file to emulate DVD-5/DVD-9 layer sizes via sparse/virtual readers

  Variants: PS1-style `BOOT =`, lowercase/space quirks, missing `;1`, Joliet-only names, a deep-path SYSTEM.CNF, and corrupted PVDs for negative tests.
- **Synthetic BIN/CUE:** wrap ISO sectors into 2352-byte raw sectors:
  - sync `00 FF×10 00`, MSF header, mode byte
  - Mode2/Form1 subheader, data, and EDC/ECC (zero-filled or computed)

  Emit a `.cue` with `FILE "x.bin" BINARY / TRACK 01 MODE2/2352 / INDEX 01 00:00:00`. Add multi-track variants with an audio track to test Redump per-track hashing.
- **Synthetic ZSO:** round-trip encode/decode with our implementation. In CI, cross-check against OPL's `ziso.py` (Python + `lz4` installed in CI only), comparing decompressed bytes.
- **Synthetic DAT:** generate a Logiqx XML `<datafile><game name=…><rom name size crc md5 sha1/>` from the fixture's computed hashes, plus mismatching entries.
- **CHD:** create tiny CHDs in CI with `chdman` from a distro package (CI-only, not committed), or use `chd`'s `encode` feature, then verify that reading returns the original ISO bytes.
- **Archives:** build zip/7z fixtures in-test with `zip` / `sevenz-rust2` writers. RAR fixtures are tiny, self-made files committed as test data (they contain no game content).

**2. Large-file logic without large files.**
- Abstract the source as a `Read + Seek` trait with a **virtual deterministic generator** (e.g. a PRNG keyed by offset). That makes it possible to test 4 GB+ split boundaries, progress math and cancellation using no disk space. GitHub runners have about 14 GB of free disk.
- Real sparse files (`set_len`) are sparse on ext4/APFS but **not automatically on NTFS**. Keep those tests Linux/macOS-only.

**3. Filesystem targets in-process.**
- Create a **FAT32 image file** (via `fatfs`/`hadris-fat`) and run "copy to USB" against it. Assert 8.3/LFN names, the `ul.cfg` binary layout, `DVD/`/`CD/`/`ART/`/`CFG/`/`VMC/` folders, and split chunk sizes.
- Use exFAT on loop devices only in an optional Linux job (`mkfs.exfat` + `sudo mount -o loop`).

**4. SMB.** In the Linux CI job, run a Samba container as a GitHub Actions `services:` entry (any maintained Samba image; specific image **unverified**). Run integration tests against it, behind a feature flag.

**5. HDD (APA/PFS).**
- Operate on a sparse `.img` file.
- Use `pfsshell`/`hdl-dump`, built from source in CI, as an **oracle**: we write, they read (and vice versa).
- Never touch real `/dev/*` in CI.

**6. Other test layers.**
- **Snapshots:** `insta` for generated config files and DTO JSON. **Property tests:** `proptest` for split/join, CUE parsing and name sanitizing.
- **Fuzzing:** `cargo-fuzz` targets for ISO PVD/dir-record parsing, SYSTEM.CNF, CUE, ZSO header, DAT XML, VMC and APA headers.
- **Benchmarks:** `criterion` for the hash pipeline (MB/s per algorithm), tracked over time.
- **Frontend:** Vitest + Testing Library with a **mock transport** (the same `api.call` interface, so no Tauri is needed). For Tauri-specific code, `@tauri-apps/api/mocks` (`mockIPC`; **unverified** name in 2.12).
- **E2E:**
  - Playwright against `oplm-server` in Docker (covers 95% of the UI).
  - Tauri WebDriver on Windows/Linux only. macOS only via the WebdriverIO service plugin.
- **Optional local-only real-corpus tests:** gated by an env var (`OPLM_REAL_GAMES_DIR`). Never run in CI, never committed.

---

## 11. Recommended stack and reasoning

**Tauri 2 + Rust core workspace + axum server + clap CLI + React/TS frontend.**

1. **Library coverage decides it.** Only Rust has maintained, pure-language libraries for CHD (`chd`), ISO read+write (`hadris-iso`), 7z read+write (`sevenz-rust2`), SMB2/3 (`smb`), fast LZ4 (`lz4_flex`), SIMD resize (`fast_image_resize`) and removable-drive flags (`sysinfo`). Go misses CHD and has a stale SMB library. Node misses most of them.
2. **Performance with no tuning:** native SIMD hashing, zero-copy buffers, threads without GC. The workload (multi-GB hashing and copying) is exactly where a JS runtime adds overhead.
3. **One core, three front doors:** a Cargo workspace makes "CLI + desktop + Docker server" natural. The web UI is literally the same SPA, with a ~50-line transport switch.
4. **Small, signed, auto-updating desktop builds** with official CI tooling (tauri-action, updater JSON).
5. **Docker image:** a static musl `oplm-server` (likely ~15–30 MB image, **unverified**) runs nicely on Proxmox in an LXC with nesting or in a VM, behind a reverse proxy (e.g. Caddy) with token auth.
6. **AI-assisted Rust is workable.** The compiler catches most mistakes AI makes, crates are well-documented, and keeping the core **synchronous** (std::fs + threads, tokio only at the edges) avoids the hardest async/lifetime problems.

**Runner-up: Wails v3 (Go).**
- Choose it if Rust's learning curve proves too costly, or once v3.0 goes final. Its built-in server mode, simpler language, pure-Go RAR and `go-diskfs` (FAT32/ISO/GPT) are real advantages.
- Costs: CHD via cgo/libchdr (or a port), SMB maturity, GTK4/WebKitGTK-6 requirement on Linux, no official CI action, and still-moving beta APIs.

**Not recommended:** Electron (size, weak native libraries), Avalonia/Flutter/Qt (new language, and the web UI must be built twice).

---

## 12. Proposed monorepo structure

```
oplm/                                  # name placeholder
├─ Cargo.toml                          # [workspace] members = crates/*, apps/cli, apps/server, apps/desktop/src-tauri
├─ rust-toolchain.toml                 # pin stable (≥1.90 for Tauri plugins)
├─ package.json  pnpm-workspace.yaml   # web/ + apps/desktop JS tooling
├─ crates/
│  ├─ oplm-core/        # domain model (Game, Library, Device), scanner (walkdir/jwalk), SQLite cache (rusqlite), job runner, ProgressSink trait
│  ├─ oplm-formats/     # iso9660 (hadris-iso wrapper/fallback), system_cnf, cue/bin→iso, zso (lz4_flex), chd (chd), archives (zip, sevenz-rust2, [rar feature]),
│  │                    #   opl configs: ul.cfg, conf_apps.cfg, per-game CFG, ART naming, VMC (ps2 mcfs)
│  ├─ oplm-hash/        # single-read multi-hash pipeline (crc32fast, md-5, sha1), Logiqx/Redump DAT parser (quick-xml), matcher
│  ├─ oplm-device/      # volume listing (sysinfo), FS-type detection, copy/split/preallocate, fragmentation check, raw-disk abstraction (cfg per OS)
│  ├─ oplm-net/         # art/DB downloads (reqwest + retry), image resize (image + fast_image_resize), SMB (smb crate, feature "smb")
│  ├─ oplm-hdd/         # APA/PFS (phase 2; feature-gated), disk-image backend first
│  ├─ oplm-api/         # Request/Response/JobEvent enums (serde + ts-rs), dispatch(), JobManager (cancel tokens, broadcast)
│  └─ oplm-test-support/# fixture builders: tiny ISO, BIN/CUE, ZSO, DAT, FAT32 image, virtual big-file reader
├─ apps/
│  ├─ cli/              # `oplm` (clap, indicatif); also `oplm helper --stdio` = elevated raw-disk helper
│  ├─ server/           # `oplm-server` (axum: /api/rpc, /api/events ws, /media/*, embedded SPA via rust-embed, token auth, /health)
│  └─ desktop/
│     ├─ src-tauri/     # Tauri 2: rpc + subscribe commands, cover URI scheme, dialog/updater/single-instance plugins; frontendDist → ../../web/dist
│     └─ tauri.conf.json, capabilities/
├─ web/                 # Vite + React + TS SPA shared by desktop and server
│  ├─ src/api/          # transport.ts (TauriTransport | HttpTransport | MockTransport), generated/ (ts-rs output)
│  ├─ src/features/     # library grid, game detail, devices, jobs, settings
│  └─ tests/            # vitest; e2e/ (Playwright vs server)
├─ packaging/
│  ├─ docker/Dockerfile.server         # multi-stage: build web → build musl server → distroless/static
│  ├─ flatpak/<app-id>.yml + metainfo.xml
│  └─ sidecars/                        # scripts to build pfsshell / hdl-dump from source (phase 1 HDD)
├─ fuzz/                               # cargo-fuzz targets
├─ .github/workflows/ ci.yml  release-desktop.yml  release-cli-server.yml  flatpak.yml
└─ docs/  (architecture, code-signing policy page required by SignPath, OPL format notes)
```

Rules that keep this clean:
- `oplm-core`/`-formats`/`-hash`/`-device` **must not depend on tauri, axum or tokio**. Check with `cargo tree -i tauri -p oplm-core` in CI.
- `apps/*` are thin adapters, and every feature is an `oplm-api` `Request` variant. That gives automatic CLI parity tests (`oplm rpc '{"type":"ScanLibrary",…}'`).
- Feature flags: `smb`, `rar`, `chd`, `hdd` keep the default build lean and avoid licence problems.

---

## 13. Risks and mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| Rust learning curve for a PHP/JS lead; AI-written Rust that compiles but is hard to review | Slower velocity, review fatigue | Sync core, small crates, `clippy -D warnings`, heavy tests/fuzzing, ADRs. Keep the UI in TS where the lead is strongest. |
| **WebKitGTK quirks** (NVIDIA DMABUF, EGL crashes in AppImage, scroll performance); WKWebView differences | Broken or slow UI on Linux / Steam Deck | Virtualize everything, avoid heavy CSS effects, test the Deck early, document env-var workarounds. Revisit the Tauri v3 CEF runtime if needed. |
| macOS cost/region; Gatekeeper friction | Users can't open the app easily | Ad-hoc signing + docs first; Developer ID ($99/yr) once there are users. |
| SignPath requires an existing release and policy | Unsigned early builds trigger SmartScreen | Release unsigned alphas, then apply to SignPath. |
| **Licensing:** UnRAR licence, GPL code (pfsshell, hdl-dump, libcue), AFL-2.0 ps2sdk libs | Can't relicense; distribution conflicts | Choose the project licence early. Keep RAR optional/external, run GPL tools as separate sidecar processes (aggregation), and get legal review before porting APA/PFS code. |
| Raw disk / formatting data loss; elevation complexity; Flatpak/Docker sandboxing | Catastrophic user data loss | No formatting in v1. Elevated helper process, removable-only filters, typed confirmation, image-file default, dry-run mode. |
| Young dependencies (`smb` 0.12, `hadris-*`, `ps2-memcard`, tauri-specta RC, TanStack Table v9, Wails v3 beta) | API churn | Pin versions, wrap behind our own traits, prefer an in-house implementation for tiny formats (ISO reader, CUE, ZSO, VMC). |
| OPL USB fragmentation / FAT32 4 GB split rules | Games fail to boot on PS2 | Preallocate + sequential writes, a fragmentation checker, exFAT guidance for OPL ≥1.2, correct `ul` split format on FAT32. |
| Web mode exposure on LAN | Unauthorized file access | Token auth, bind to localhost by default, path allow-list roots, reverse proxy with TLS. |
| MD5 throughput on NVMe | Slow "verify" | Make MD5 opt-in, cache hashes, use size+CRC32 prefilter + SHA1 confirm. |

---

## 14. Items explicitly unverified
- Tauri v3 alpha numbering (alpha.5 on GitHub vs alpha.7 mentioned in a May 2026 digest) and the CEF runtime's scope and timeline.
- Wails, Avalonia, Flutter and Qt installer sizes. All "typical" size and RAM figures (secondary sources).
- Steam Deck behaviour of Tauri AppImage/Flatpak and Wails GTK4 builds.
- `sha1` 0.11 SHA-NI use (only the `cpufeatures` dependency is visible). MD5 throughput figures.
- go-smb2 "Requires Go 1.27" README text versus the last tagged v1.1.0 (2022). kdomanski/iso9660 newer-than-v0.4.0 tags.
- UnRAR licence wording; hdl-dump licence (COPYING not read).
- udisks2 `OpenDevice`/`Format` polkit details.
- SignPath certificate type (OV/EV); commercial cert prices; Apple program regional availability.
- cargo-dist latest version; `@tauri-apps/api/mocks` naming in 2.12; a specific Samba CI image.
- Whether OPL exFAT/BDM requires contiguous files.

---

## 15. Sources

**Tauri**
- https://github.com/tauri-apps/tauri/releases
- https://github.com/tauri-apps/tauri/releases/tag/tauri-v3.0.0-alpha.5
- https://tauri.app/blog/
- https://v2.tauri.app/start/
- https://v2.tauri.app/distribute/
- https://v2.tauri.app/distribute/appimage/
- https://v2.tauri.app/distribute/flatpak/
- https://v2.tauri.app/distribute/sign/macos/
- https://v2.tauri.app/distribute/windows-installer/
- https://v2.tauri.app/plugin/updater/
- https://v2.tauri.app/plugin/dialog/
- https://v2.tauri.app/develop/calling-frontend/
- https://v2.tauri.app/develop/debug/linux-graphics/
- https://v2.tauri.app/develop/tests/webdriver/
- https://v2.tauri.app/release/@tauri-apps/api/v2.0.0-beta.9/
- https://github.com/tauri-apps/tauri-action
- https://react-dropzone.js.org/guide/tauri/
- https://redirect.github.com/tauri-apps/tauri/issues/9823
- https://buttondown.com/thewang/archive/rust-daily-digest-2026-05-24/
- https://hub.harborframework.com/tasks/abundant/tauri-apps__tauri-14661
- https://git.hackliberty.org/Git-Mirrors/xmr-btc-swap/commit/a056ae3a47be1f922ec77daa9b060a485725d7bd
- https://codeberg.org/enbyss/aesthesis/pulls/8
- https://registry.npmjs.org/@tauri-apps/api/latest

**Wails**
- https://github.com/wailsapp/wails/releases
- https://docs-preview.wails.io/guides/server-build/
- https://v3.wails.io/guides/server-build
- https://docs-preview.wails.io/guides/build/linux/
- https://v3.wails.io/guides/build/windows
- https://v3.wails.io/guides/build/signing
- https://v3.wails.io/tutorials/04-self-update-a-wails-app/
- https://docs-preview.wails.io/guides/updater/
- https://wails.io/docs/guides/crossplatform-build
- https://docs-preview.wails.io/guides/build/cross-platform/

**Electron and alternatives**
- https://releases.electronjs.org/
- https://tech-insider.org/tauri-vs-electron-2026/
- https://betterstack.com/community/guides/scaling-nodejs/tauri-vs-electron-vs-deno-vs-electrobun/
- https://www.digitalapplied.com/blog/desktop-apps-web-stack-tauri-electron-deno-wails-2026
- https://devtoolpicks.com/blog/tauri-vs-electron-indie-hackers-2026
- https://raxxo.shop/blogs/lab/how-to-build-and-distribute-an-electron-desktop-app-in-2026
- https://www.stork.ai/blog/gos-secret-weapon-for-desktop-apps
- https://blackboard.sh/electrobun/docs/guides/compatability
- https://aicoolies.com/tools/electrobun
- https://en.wikipedia.org/wiki/Avalonia_(software_framework)
- https://wikidocs.net/blog/@lukewire/11052/
- https://startdebugging.net/2026/08/how-to-enable-multi-window-support-in-a-flutter-desktop-app/
- https://ubuntu.com/blog/multiple-window-flutter-desktop
- https://wiki.qt.io/Qt_6.10_Release
- https://doc.qt.io/qt-6/ja/license-changes.html
- https://redirect.github.com/nodejs/node/commit/0f7aed8a59 (zlib.crc32)

**Rust crates**
- https://lib.rs/crates/hadris-iso
- https://lib.rs/crates/cdfs
- https://lib.rs/crates/iso9660
- https://lib.rs/crates/chd
- https://docs.rs/crate/libchdman-rs/0.289.0/source/README.md
- https://lib.rs/crates/sevenz-rust2
- https://lib.rs/crates/unrar
- https://lib.rs/crates/zip
- https://lib.rs/crates/crc32fast
- https://lib.rs/crates/md-5
- https://lib.rs/crates/sha1
- https://docs.rs/sha1/latest/sha1/
- https://docs.rs/md-5/latest/md5/
- https://lib.rs/crates/lz4_flex
- https://lib.rs/crates/image
- https://lib.rs/crates/fast_image_resize
- https://lib.rs/crates/quick-xml
- https://rustsec.org/advisories/RUSTSEC-2025-0068
- https://users.rust-lang.org/t/serde-yaml-deprecation-alternatives/108868
- https://lib.rs/crates/reqwest
- https://seanmonstar.com/blog/reqwest-retries/
- https://www.rustmax.net/api/reqwest/retry/
- https://docs.rs/crate/reqwest-retry/^0.2.2
- https://lib.rs/crates/smb
- https://lib.rs/crates/pavao
- https://docs.rs/sysinfo/latest/sysinfo/struct.Disk.html
- https://docs.rs/rs-drivelist/
- https://rust-digger.code-maven.com/crates/disket
- https://lib.rs/crates/fatfs
- https://docs.rs/crate/hadris-fat/2.2.0
- https://github.com/hxyulin/hadris/wiki
- https://lib.rs/crates/runas
- https://lib.rs/crates/ts-rs
- https://lib.rs/crates/tauri-specta
- https://lib.rs/crates/axum
- https://docs.rs/crate/rcue/latest
- https://docs.rs/crate/cue_lib/0.1.0
- https://lib.rs/crates/cue
- https://docs.rs/crate/ps2mc/0.1.0
- https://www.wingetgui.com/apps/axodotdev-dist (cargo-dist)
- https://www.archlinux.org/packages/extra/x86_64/cargo-dist

**Go packages**
- https://github.com/kdomanski/iso9660
- https://pkg.go.dev/github.com/kdomanski/iso9660
- https://github.com/diskfs/go-diskfs
- https://pkg.go.dev/github.com/diskfs/go-diskfs
- https://github.com/hirochachacha/go-smb2
- https://pkg.go.dev/github.com/hirochachacha/go-smb2
- https://github.com/bodgit/sevenzip
- https://github.com/mholt/archives
- https://pkg.go.dev/github.com/pierrec/lz4/v4
- https://go.googlesource.com/go/+/refs/heads/master/src/crypto/sha1/_asm
- https://reviews.freebsd.org/D45444
- https://github.com/yaml/go-yaml/
- https://zerokspot.com/weblog/2026/02/07/maintained-golang-yaml-library/
- https://www.mintlify.com/shirou/gopsutil/api/disk/partitions
- https://beta.pkg.go.dev/github.com/lmvgo/cue

**PS2 / OPL specifics**
- https://github.com/ps2homebrew/Open-PS2-Loader/wiki/usb-mode
- https://ConsoleMods.org/wiki/PS2:Open_PS2_Loader_(OPL)
- https://psx-place.com/threads/testers-needed-open-ps2-loader-with-zso-support-compressed-iso.37223/post-332134
- https://man.archlinux.org/man/maxcso.1.en
- https://github.com/ps2homebrew/hdl-dump
- https://github.com/ps2homebrew/pfsshell
- https://wiki.nixos.org/wiki/PlayStation_2
- https://csclub.uwaterloo.ca/~rridge/mymc/ps2mcfs.html
- https://github.com/Adubbz/mymcplusplus
- https://github.com/bucanero/ps2vmc-tool/blob/main/README.md
- https://www.mintlify.com/laromicas/datoso/concepts/dats-and-roms
- https://dumping.guide/discs/sony/ps2
- https://pypi.org/project/pyoplm/
- https://pypi.org/project/libopl
- https://www.diskgenius.com/how-to/how-to-fix-the-volume-is-too-big-for-fat32-error.php
- https://driverscloud.com/en/news/2376/microsoft-finally-lifts-the-arbitrary-32gb-limit-on-fat32-partitions-in-windows-well-almost!
- https://ospire.mind.be/yocto/kirkstone/component/udisks2 (udisks2 CVEs)
- https://man.archlinux.org/man/udisks.8.en.txt

**Frontend**
- https://registry.npmjs.org/react/latest
- https://registry.npmjs.org/svelte/latest
- https://registry.npmjs.org/vue/latest
- https://registry.npmjs.org/@tanstack/react-virtual/latest
- https://registry.npmjs.org/@tanstack/react-table/latest
- https://tanstack.com/virtual/latest/docs/blog
- https://tanstack.com/virtual/latest/docs/framework/svelte
- https://www.itechguides.com/vue-3-6-vapor-mode-what-developers-need-to-know/

**CI / signing**
- https://signpath.org/
- https://signpath.org/terms
- https://signpath.io/product/open-source
- https://azure.microsoft.com/en-us/products/artifact-signing
- https://devclass.com/2026/01/14/code-signing-windows-apps-may-be-easier-and-more-secure-with-new-azure-artifact-service/
- https://forum.juce.com/t/azure-code-signing-for-plugin-developers-guide/60391?page=6
- https://knowledge.digicert.com/alerts/code-signing-certificates-459-day-validity
- https://www.encryptionconsulting.com/new-code-signing-certificate-validity-change/
- https://developer.apple.com/programs/whats-included/
- https://developer.apple.com/support/compare-memberships/
- https://www.macworld.com/article/2457844/what-to-do-when-you-cant-open-an-app-you-just-installed-in-macos-sequoia.html
- https://appleinsider.com/inside/macos-sequoia/tips/whats-changed-in-runtime-protection-for-macos-sequoia
- https://docs.github.com/en/actions/reference/runners/github-hosted-runners
- https://github.blog/changelog/2025-08-07-arm64-hosted-runners-for-public-repositories-are-now-generally-available/
- https://blogs.windows.com/windowsdeveloper/2025/04/14/github-actions-now-supports-windows-on-arm-runners-for-all-public-repos/
- https://discuss.python.org/t/dropping-intel-mac-to-tier-2/102100/1
- https://github.com/libgit2/pygit2/pull/1441
