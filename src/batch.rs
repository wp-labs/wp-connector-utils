//! Batch-level metadata processing helpers.
//!
//! These utilities know how to apply [`wp_connector_api::BatchMeta`] to different
//! output formats. Shared across `wp-core-connectors` and `wp-connectors`.

use std::sync::Arc;
use std::sync::OnceLock;
use wp_connector_api::BatchMeta;
use wp_model_core::model::DataRecord;
use wp_model_core::model::Field as ModelField;

/// 是否保留上游（接力场景）已经写入的 `wp_oml_name`。
///
/// 默认 `false`：本实例的 OML 名覆盖上游值（同名只保留一个字段）。
/// 设为 `true` 时，记录已带 `wp_oml_name` 就原样放过，不再追加。
///
/// 与 `WP_INS_ID` / `WP_GLOBAL_TAGS` 一样从进程环境读取（启动时加载的 `.env` 同样生效）。
pub const UPSTREAM_OML_NAME_KEEP_ENV: &str = "UPSTREAM_OML_NAME_KEEP";

const OML_NAME_FIELD: &str = "wp_oml_name";

/// If `meta.oml_name` is non-empty and not disabled, clone each record and
/// append a `wp_oml_name` field. Returns the original `data` unchanged when
/// no injection is needed (avoids clone overhead).
///
/// Skips injection when `"wp_oml_name"` is in [`BatchMeta::output_disabled`].
///
/// Relay behaviour is controlled by [`UPSTREAM_OML_NAME_KEEP_ENV`]:
/// - `false` (default) — an existing `wp_oml_name` (came from upstream) is
///   **replaced** by this instance's OML name, never duplicated
/// - `true` — the upstream value is kept and no field is appended
///
/// Used by text-format sinks (JSON/CSV/TCP/syslog) in `sink_records_with_meta`.
pub fn inject_oml_name(meta: &BatchMeta, data: Vec<Arc<DataRecord>>) -> Vec<Arc<DataRecord>> {
    inject_oml_name_with(keep_upstream_oml_name(), meta, data)
}

/// [`inject_oml_name`] with an explicit `keep_upstream` flag (pure, testable).
pub fn inject_oml_name_with(
    keep_upstream: bool,
    meta: &BatchMeta,
    data: Vec<Arc<DataRecord>>,
) -> Vec<Arc<DataRecord>> {
    if meta.is_output_disabled(OML_NAME_FIELD) {
        return data;
    }
    let Some(name) = meta.oml_name().filter(|name| !name.is_empty()) else {
        return data;
    };

    data.into_iter()
        .map(|rec| {
            let has_upstream = rec.field(OML_NAME_FIELD).is_some();
            if has_upstream && keep_upstream {
                // 保留上游值，连 clone 都不需要
                return rec;
            }

            let mut cloned = DataRecord::clone(&rec);
            if has_upstream {
                // 不保留上游：先清掉同名旧值，避免一条记录出现两个 wp_oml_name
                while cloned.remove_field(OML_NAME_FIELD) {}
            }
            cloned.append(ModelField::from_chars(OML_NAME_FIELD, name));
            Arc::new(cloned)
        })
        .collect()
}

fn keep_upstream_oml_name() -> bool {
    static KEEP: OnceLock<bool> = OnceLock::new();
    *KEEP.get_or_init(|| {
        std::env::var(UPSTREAM_OML_NAME_KEEP_ENV)
            .ok()
            .and_then(|raw| parse_flag(&raw))
            .unwrap_or(false)
    })
}

/// 解析布尔开关；无法识别时返回 `None`（调用方按默认值处理）。
pub fn parse_flag(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" | "" => Some(false),
        _ => None,
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

    // -- 接力：上游已带 wp_oml_name ---------------------------------------

    /// 默认（`UPSTREAM_OML_NAME_KEEP=false`）：本实例的 OML 名覆盖上游值，且只有一个字段。
    #[test]
    fn inject_replaces_upstream_when_keep_disabled() {
        let meta = BatchMeta::with_oml_name("stage_b");
        let data = vec![make_record(vec![
            FieldStorage::from(ModelField::from_chars("wp_oml_name", "stage_a")),
            FieldStorage::from(ModelField::from_chars("msg", "hello")),
        ])];

        let result = inject_oml_name_with(false, &meta, data);
        let rec = &result[0];
        assert_eq!(
            rec.field("wp_oml_name").unwrap().get_value().to_string(),
            "stage_b"
        );
        assert_eq!(
            rec.items
                .iter()
                .filter(|f| f.get_name() == "wp_oml_name")
                .count(),
            1,
            "不得出现两个 wp_oml_name"
        );
    }

    /// `UPSTREAM_OML_NAME_KEEP=true`：保留上游值，不追加、不拷贝。
    #[test]
    fn inject_preserves_upstream_when_keep_enabled() {
        let meta = BatchMeta::with_oml_name("stage_b");
        let rec = make_record(vec![FieldStorage::from(ModelField::from_chars(
            "wp_oml_name",
            "stage_a",
        ))]);
        let original_ptr = Arc::as_ptr(&rec);

        let result = inject_oml_name_with(true, &meta, vec![rec]);
        assert_eq!(Arc::as_ptr(&result[0]), original_ptr, "保留时应零拷贝");
        assert_eq!(
            result[0]
                .field("wp_oml_name")
                .unwrap()
                .get_value()
                .to_string(),
            "stage_a"
        );
        assert_eq!(
            result[0]
                .items
                .iter()
                .filter(|f| f.get_name() == "wp_oml_name")
                .count(),
            1
        );
    }

    /// 开启保留时，没有上游值的记录仍然由本实例盖戳。
    #[test]
    fn inject_appends_when_keep_enabled_but_no_upstream() {
        let meta = BatchMeta::with_oml_name("stage_b");
        let data = vec![make_record(vec![FieldStorage::from(
            ModelField::from_chars("msg", "hello"),
        )])];

        let result = inject_oml_name_with(true, &meta, data);
        assert_eq!(
            result[0]
                .field("wp_oml_name")
                .unwrap()
                .get_value()
                .to_string(),
            "stage_b"
        );
    }

    #[test]
    fn parse_flag_accepts_common_spellings() {
        for raw in ["1", "true", "TRUE", " yes ", "on"] {
            assert_eq!(parse_flag(raw), Some(true), "raw={raw:?}");
        }
        for raw in ["0", "false", "FALSE", " no ", "off", ""] {
            assert_eq!(parse_flag(raw), Some(false), "raw={raw:?}");
        }
        assert_eq!(parse_flag("maybe"), None);
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
