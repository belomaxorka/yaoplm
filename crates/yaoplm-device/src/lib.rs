//! Volume discovery, filesystem detection, preallocated sequential copy,
//! `ul` splitting and fragmentation checks.
//!
//! Synchronous by design: no tokio, tauri or axum (ADR-0003).
