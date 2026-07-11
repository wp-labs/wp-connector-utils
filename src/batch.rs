//! Batch-level metadata processing helpers.
//!
//! These utilities know how to apply [`wp_connector_api::BatchMeta`] to different
//! output formats. Shared across `wp-core-connectors` and `wp-connectors`.

use std::sync::Arc;
use wp_connector_api::BatchMeta;
use wp_model_core::model::DataRecord;
use wp_model_core::model::Field as ModelField;

/// If `meta.oml_name` is non-empty, clone each record and append a `wp_oml_name`
/// field. Returns `Ok(original)` when no injection is needed (avoids clone overhead).
///
/// Used by text-format sinks (JSON/CSV/TCP/syslog) in `sink_records_with_meta`.
pub fn inject_oml_name(meta: &BatchMeta, data: Vec<Arc<DataRecord>>) -> Vec<Arc<DataRecord>> {
    match meta.oml_name() {
        Some(name) if !name.is_empty() => data
            .into_iter()
            .map(|rec| {
                let mut cloned = DataRecord::clone(&rec);
                cloned.append(ModelField::from_chars("wp_oml_name", name));
                Arc::new(cloned)
            })
            .collect(),
        _ => data,
    }
}

/// Resolve the effective tag for an Arrow frame from batch metadata.
///
/// Priority: `BatchMeta.oml_name` (non-empty) → connector `config_tag`.
pub fn resolve_frame_tag<'a>(meta: &'a BatchMeta, config_tag: &'a str) -> &'a str {
    meta.oml_name()
        .filter(|n| !n.is_empty())
        .unwrap_or(config_tag)
}
