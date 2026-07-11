//! Shared utilities for WP connector implementations.
//!
//! # Modules
//! - `arrow` — Arrow IPC encode/decode + WireFormat
//! - `batch` — BatchMeta processing helpers (inject fields, resolve tags)

pub mod arrow;
pub mod batch;
pub mod ndjson;
