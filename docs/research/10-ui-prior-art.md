# 10 - Desktop UI: prior art

Date: 2026-10-23. Scope: milestone M3 (the desktop and web interface: device picker, library, game card, import, plan screen, jobs panel, recovery, settings). Sources: the local clones from [REFERENCES.md](REFERENCES.md), refreshed the same day, read before each screen was built (ADR-0015). Nothing was copied: Orbit and Toolbox are GPL-3, POUM's licence is unverified, OPLNova is GPL-3 with unclear provenance in places (research/05). Ideas only.

Short names:
- Orbit = `Luden02/OrbitPS2-Manager@cdd590a` (Electron/Angular, GPL-3).
- Tbx = `lucasonline0/opl-linux-toolbox@3a776d1` (fork of Orbit, GPL-3).
- POUM = `gecm0/POUM@bffd600` (Tauri 2 + React, our stack).
- PCT = `brainstream/OPL-PC-Tools@2291b5a` (Qt), PIM = `imadali120/PS2IsoManager@b1a5d16` (WPF).
- Nova = `WeeXnes/OPLNova@7c30757` (Avalonia), audited in research/05.

## 1. What was taken, and from where

| Screen | Idea | Source | Where in YAOPLM |
|---|---|---|---|
| Jobs | One write job at a time, in queue order; a job's result goes into a per-job **live log** with time stamps and a type per line (step, success, error) | Orbit `jobs.service.ts` (`JobLogEntry`, `logJob`, `processNext`) | `JobEvent::Log`, `JobInfo.log` (`crates/yaoplm-api/src/jobs.rs`); `JobLog` in `web/src/features/jobs/JobsPage.tsx` |
| Jobs | **One library rescan after the whole queue drains**, not one per finished job (`pendingFullRefresh`) | Orbit `jobs.service.ts:208-286` | `JobsProvider.rescanIfIdle`: the library query is invalidated when the last `executePlan` job of a run ends |
| Jobs | "Clear finished", remove one job, a collapsed rail with the active count | Orbit `jobs-panel.component.html` | `JobsPage` (clear, hide), the Jobs badge in the sidebar, `JobsDock` |
| Jobs | Percentage plus transferred/total size, throughput and ETA on the same line | Tbx 1.4.1 changelog and `jobs.service.ts:507` | `JobProgressLine` (`formatRate`, `formatEta`) |
| Jobs | Per-transaction outcome summary after a run | Orbit's delete dialog (per-file success/error log) | outcome badges (`done`, `rolledBack`, `notStarted`, `interrupted`) from `ExecutionReport` |
| Devices | A drive picker with free space, and an "Initialize for OPL" action when the folders are missing | POUM `DriveSelector.tsx`, `App.tsx` | `DevicesPage` with the storage doctor's verdict; the "Create folders" hint on the library page (`PlanInit`) |
| Add games | Show total size against free space before anything is written; "Need X more" when it does not fit | POUM `IsoImport.tsx` | `PlanSummary` (writes, deletes, free now, free after) and the core's `noSpace` blocker |
| Add games | A title length counter and per-game editing before import | POUM `IsoImport.tsx`, Orbit `import.component.html` (staged files with ID and name inputs) | per-transaction title and CD/DVD overrides inside the plan (`AddGamesPage.renderOverride`), re-planned on every change |
| Add games | Move vs copy as an explicit preference, never the default | Nova (`preferred_installation_method`), Orbit (`deleteOriginal` only for conversions) | the "Move instead of copying" switch; the plan then carries `sourcesDeleted` and needs `confirmDelete` |
| Rename | Explain that `ART/` is keyed by the game ID and is not renamed | Orbit `rename-dialog.component.html` | `game.renameDescription`, `game.fixIdHint` |
| Library | Grid and list, search, sort, a refresh button, tabs by system | Orbit `library.component.html` | `LibraryPage`; systems become CD/DVD and ISO/ZSO/UL filters (PS1 and APPS arrive in M6) |
| Library | A poster tile with the game ID when there is no art | Orbit `gamecard.component.html` (`poster-no-art`) | `GameCover`: COV proportions, ID, media and format, a tint from the ID |
| Game | A details page with chips (ID, format, size) and the actions next to them | Orbit `details.component.html`, PCT's game list | `GamePage` |
| Everywhere | A close guard while a job runs | Orbit (`beforeunload` guard) | not needed: jobs run in the core, not in the page; closing the desktop window still waits for the core (M4: a close guard in Tauri) |

## 2. What was deliberately not repeated

| Mistake | Where | YAOPLM |
|---|---|---|
| Selecting a game rewrites its CFG through change handlers, dropping unknown keys | Nova `MainWindow.SetShowcaseArea` + `Config.SaveConfig` (research/05 §2.4, bug 1) | The UI never writes on selection. Every write is a plan the user confirms; CFG keys are preserved by the core (CFG-1) |
| `Delete` removes the ISO with no confirmation | Nova `MainWindow_OnKeyDown` (bug 2) | Delete needs the plan screen plus a separate "I understand that files will be deleted" checkbox; the core refuses `ExecutePlan` without `confirmDelete` |
| Rename always moves the file into `DVD/` | Nova `Game.ChangeName` (bug 3) | Rename and CD↔DVD are separate plans; the plan lists the exact rename |
| Progress of a bulk run only shown per file; the library rescanned after every single job | Orbit before `pendingFullRefresh`, PIM | one rescan per queue (§1) |
| Writes happen in the renderer and die with the window | Orbit, Tbx (Electron main process), POUM (Tauri command per file) | Jobs run in the core with a journal (ADR-0021); a reconnecting page gets the job list and log back from `ListJobs`/`GetJob` |
| Drive list shows internal and system disks as targets | PIM, Nova (a folder picker only) | The storage doctor's verdict is on every card; system and non-external volumes are hidden by default on the desktop |
| Free space checked with `TotalSize` instead of available space | Nova `IsoConverter.cs` (bug 8) | The core compares `write_bytes` with `free_bytes` when planning and again before the first write |
| Dialog strings hard-coded in English | Nova, POUM, Orbit | every string goes through i18n (`en`, `ru`); warnings are translated by their stable `code` and keep the core's message with the specifics |

## 3. Decisions made while building the UI

- **Settings live in the browser storage** (`yaoplm.settings`: profile, name style, root, view, verify). ARCHITECTURE.md §6 plans `settings.toml` in the app data folder; that lands when settings need to be shared with the CLI or the server. The desktop WebView keeps `localStorage` per app identifier, so the choice survives restarts.
- **Filters live in the address** (`?q=&media=&format=&warnings=&sort=&desc=`), so Back from a game card keeps them.
- **The plan is the only source of names and media.** The Add screen never guesses a title or CD/DVD itself; it asks the core for a plan, shows the core's choices and sends overrides back for a new plan (`ImportOverride`). This keeps the UI honest about what will land on the device.
- **Server mode browses folders through the core** (`ListDir`, `PathPolicy`): a browser has no file paths to give, and the server must never list the host outside its roots (ADR-0009). The desktop uses native dialogs (`tauri-plugin-dialog` 2.8.3, MIT OR Apache-2.0) and the window's drag-and-drop events.
- **Virtualization** uses TanStack Virtual (rows of a computed column count in the grid, rows in the list). `cargo run -p yaoplm-api --example make_library -- <dir> --games 3000` builds a library to try it on.
- **Keyboard**: `/` focuses the search, arrow keys and Home/End move between game cards (roving focus over the virtualized rows), Enter opens, Esc closes dialogs; every control is reachable with Tab (Radix primitives from shadcn/ui).

## 4. Still open

- Cover art and the art dialogs (Orbit's artwork wizard, bulk scopes, "skip existing") are M4.
- A Tauri close guard while a write job runs (Orbit has one for Electron).
- Settings file shared by the three front ends (ARCHITECTURE.md §6).
- Desktop e2e through `tauri-driver` (TESTING.md).
