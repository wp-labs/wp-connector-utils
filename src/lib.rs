//! Shared utilities for WP connector implementations.
//!
//! # Modules
//! - `arrow` — Arrow IPC encode/decode + WireFormat
//! - `batch` — BatchMeta processing helpers (inject fields, resolve tags)
//! - `codec` — compression (gzip/zstd) + encryption (AES-256-GCM) encode/decode

pub mod arrow;
pub mod batch;
pub mod codec;
pub mod ndjson;
