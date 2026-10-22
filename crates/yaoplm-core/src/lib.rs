//! Domain model (game, library, device, loader profile), library scanner,
//! job primitives and the SQLite cache.
//!
//! Synchronous by design: no tokio, tauri or axum (ADR-0003).
