//! Single-pass CRC32/SHA-1/MD5 hashing, Logiqx/Redump DAT parsing and
//! dump matching.
//!
//! Synchronous by design: no tokio, tauri or axum (ADR-0003).
