//! Batch-level metadata processing helpers.
//!
//! These utilities know how to apply [`wp_connector_api::BatchMeta`] to different
//! output formats. Shared across `wp-core-connectors` and `wp-connectors`.

use std::sync::Arc;
use wp_connector_api::BatchMeta;
use wp_model_core::model::DataRecord;
use wp_model_core::model::Field as ModelField;

/// If `meta.oml_name` is non-empty and not disabled, clone each record and
/// append a `wp_oml_name` field. Returns the original `data` unchanged when
/// no injection is needed (avoids clone overhead).
///
/// Skips injection when `"wp_oml_name"` is in [`BatchMeta::output_disabled`].
///
/// Used by text-format sinks (JSON/CSV/TCP/syslog) in `sink_records_with_meta`.
pub fn inject_oml_name(meta: &BatchMeta, data: Vec<Arc<DataRecord>>) -> Vec<Arc<DataRecord>> {
    if meta.is_output_disabled("wp_oml_name") {
        return data;
    }
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
/// Priority: `BatchMeta.oml_name` (non-empty, not disabled) → connector `config_tag`.
///
/// Returns `config_tag` when `"tag"` or `"oml_name"` is in
/// [`BatchMeta::output_disabled`].
pub fn resolve_frame_tag<'a>(meta: &'a BatchMeta, config_tag: &'a str) -> &'a str {
    if meta.is_output_disabled("tag") || meta.is_output_disabled("oml_name") {
        return config_tag;
    }
    meta.oml_name()
        .filter(|n| !n.is_empty())
        .unwrap_or(config_tag)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use wp_connector_api::BatchMeta;
    use wp_model_core::model::DataRecord;
    use wp_model_core::model::Field as ModelField;
    use wp_model_core::model::FieldStorage;

    fn make_record(field_storages: Vec<FieldStorage>) -> Arc<DataRecord> {
        Arc::new(DataRecord::from(field_storages))
    }

    // -- inject_oml_name ------------------------------------------------

    #[test]
    fn inject_adds_field_when_oml_name_present() {
        let meta = BatchMeta::with_oml_name("nginx_access");
        let data = vec![make_record(vec![FieldStorage::from(
            ModelField::from_chars("msg", "hello"),
        )])];

        let result = inject_oml_name(&meta, data);
        assert_eq!(result.len(), 1);

        let rec = &result[0];
        let field = rec.field("wp_oml_name").expect("wp_oml_name should exist");
        assert_eq!(field.get_value().to_string(), "nginx_access");
    }

    #[test]
    fn inject_skips_when_oml_name_none() {
        let meta = BatchMeta::default();
        let rec = make_record(vec![FieldStorage::from(ModelField::from_chars(
            "msg", "hello",
        ))]);
        let original_ptr = Arc::as_ptr(&rec);
        let data = vec![rec];

        let result = inject_oml_name(&meta, data);
        assert_eq!(result.len(), 1);
        assert_eq!(Arc::as_ptr(&result[0]), original_ptr);
        assert!(result[0].field("wp_oml_name").is_none());
    }

    #[test]
    fn inject_skips_when_oml_name_empty() {
        let meta = BatchMeta::with_oml_name("");
        let rec = make_record(vec![FieldStorage::from(ModelField::from_chars(
            "msg", "hello",
        ))]);
        let original_ptr = Arc::as_ptr(&rec);
        let data = vec![rec];

        let result = inject_oml_name(&meta, data);
        assert_eq!(result.len(), 1);
        assert_eq!(Arc::as_ptr(&result[0]), original_ptr);
    }

    #[test]
    fn inject_adds_field_to_all_records() {
        let meta = BatchMeta::with_oml_name("stream_a");
        let data = vec![
            make_record(vec![FieldStorage::from(ModelField::from_chars("v", "1"))]),
            make_record(vec![FieldStorage::from(ModelField::from_chars("v", "2"))]),
            make_record(vec![FieldStorage::from(ModelField::from_chars("v", "3"))]),
        ];

        let result = inject_oml_name(&meta, data);
        assert_eq!(result.len(), 3);
        for rec in &result {
            let field = rec.field("wp_oml_name").expect("wp_oml_name should exist");
            assert_eq!(field.get_value().to_string(), "stream_a");
        }
    }

    #[test]
    fn inject_preserves_existing_fields() {
        let meta = BatchMeta::with_oml_name("out");
        let data = vec![make_record(vec![
            FieldStorage::from(ModelField::from_chars("name", "alice")),
            FieldStorage::from(ModelField::from_int("count", 42)),
        ])];

        let result = inject_oml_name(&meta, data);
        let rec = &result[0];
        assert_eq!(rec.field("name").unwrap().get_value().to_string(), "alice");
        assert_eq!(rec.field("count").unwrap().get_value().to_string(), "42");
        assert_eq!(
            rec.field("wp_oml_name").unwrap().get_value().to_string(),
            "out"
        );
    }

    #[test]
    fn inject_skips_when_disabled() {
        let mut meta = BatchMeta::with_oml_name("should_not_appear");
        meta.set_output_disabled(["wp_oml_name"]);
        let data = vec![make_record(vec![FieldStorage::from(
            ModelField::from_chars("msg", "hello"),
        )])];

        let result = inject_oml_name(&meta, data);
        assert_eq!(result.len(), 1);
        assert!(result[0].field("wp_oml_name").is_none());
    }

    // -- resolve_frame_tag ----------------------------------------------

    #[test]
    fn resolve_uses_oml_name_when_present() {
        let meta = BatchMeta::with_oml_name("stream_x");
        assert_eq!(resolve_frame_tag(&meta, "default"), "stream_x");
    }

    #[test]
    fn resolve_falls_back_to_config_when_none() {
        let meta = BatchMeta::default();
        assert_eq!(resolve_frame_tag(&meta, "config_tag"), "config_tag");
    }

    #[test]
    fn resolve_falls_back_to_config_when_empty() {
        let meta = BatchMeta::with_oml_name("");
        assert_eq!(resolve_frame_tag(&meta, "fallback"), "fallback");
    }

    #[test]
    fn resolve_falls_back_when_tag_disabled() {
        let mut meta = BatchMeta::with_oml_name("stream_x");
        meta.set_output_disabled(["tag"]);
        assert_eq!(resolve_frame_tag(&meta, "config_tag"), "config_tag");
    }

    #[test]
    fn resolve_falls_back_when_oml_name_disabled() {
        let mut meta = BatchMeta::with_oml_name("stream_x");
        meta.set_output_disabled(["oml_name"]);
        assert_eq!(resolve_frame_tag(&meta, "config_tag"), "config_tag");
    }
}
