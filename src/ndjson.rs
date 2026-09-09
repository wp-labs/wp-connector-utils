//! NDJSON → Arrow RecordBatch conversion.
//!
//! Used by `BatchSource` adapters to convert raw text lines into
//! columnar Arrow batches matching a given schema.

use arrow::array::{
    ArrayRef, BooleanArray, Float64Array, Int64Array, StringArray, TimestampNanosecondArray,
};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;
use chrono::{DateTime, NaiveDateTime};
use std::collections::HashMap;
use std::sync::Arc;

/// Parse a batch of NDJSON lines into a single Arrow [`RecordBatch`].
///
/// Returns `None` if the input is empty. Returns an error if any line
/// is not valid JSON or a field value cannot be converted to the
/// expected Arrow type.
pub fn ndjson_to_record_batch(
    lines: &[String],
    schema: &Schema,
) -> Result<Option<RecordBatch>, String> {
    if lines.is_empty() {
        return Ok(None);
    }

    let mut columns: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
    for field in schema.fields() {
        columns.insert(field.name().clone(), Vec::with_capacity(lines.len()));
    }

    for line in lines {
        let obj: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(line).map_err(|e| format!("invalid JSON: {e}"))?;
        for field in schema.fields() {
            let val = obj
                .get(field.name())
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            columns.get_mut(field.name()).unwrap().push(val);
        }
    }

    let arrays: Result<Vec<ArrayRef>, String> = schema
        .fields()
        .iter()
        .map(|field| build_array(field, columns.get(field.name()).unwrap()))
        .collect();

    let batch = RecordBatch::try_new(Arc::new(schema.clone()), arrays?)
        .map_err(|e| format!("arrow error: {e}"))?;
    Ok(Some(batch))
}

/// JSON value → epoch nanoseconds for a `time`-typed column.
///
/// Numeric epoch timestamps are recognized by digit width (seconds,
/// milliseconds, microseconds, nanoseconds); strings may be RFC3339,
/// `%Y-%m-%d %H:%M:%S`, or numeric epoch values using the same unit
/// inference. Any other value yields `None` (the cell stays null).
fn timestamp_value_nanos(value: &serde_json::Value) -> Option<i64> {
    match value {
        serde_json::Value::Number(number) => number
            .as_i64()
            .and_then(epoch_int_nanos)
            .or_else(|| number.as_f64().and_then(epoch_float_nanos)),
        serde_json::Value::String(text) => {
            if let Some(nanos) = DateTime::parse_from_rfc3339(text)
                .ok()
                .or_else(|| {
                    NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S")
                        .ok()
                        .map(|dt| dt.and_utc().fixed_offset())
                })
                .and_then(|dt| dt.timestamp_nanos_opt())
            {
                return Some(nanos);
            }
            text.parse::<i64>()
                .ok()
                .and_then(epoch_int_nanos)
                .or_else(|| text.parse::<f64>().ok().and_then(epoch_float_nanos))
        }
        _ => None,
    }
}

/// Epoch-unit multiplier chosen by absolute digit width, so
/// `1643163078` (s), `1643163078468` (ms), `1643163078468000` (us) and
/// `1643163078468000000` (ns) all normalize to the same instant.
fn epoch_unit_multiplier(abs: i64) -> i64 {
    match abs {
        0..=9_999_999_999 => 1_000_000_000,
        10_000_000_000..=9_999_999_999_999 => 1_000_000,
        10_000_000_000_000..=9_999_999_999_999_999 => 1_000,
        _ => 1,
    }
}

fn epoch_int_nanos(raw: i64) -> Option<i64> {
    let abs = raw.checked_abs().unwrap_or(i64::MAX);
    let nanos = i128::from(raw) * i128::from(epoch_unit_multiplier(abs));
    i64::try_from(nanos).ok()
}

fn epoch_float_nanos(raw: f64) -> Option<i64> {
    if !raw.is_finite() {
        return None;
    }
    let nanos = raw * epoch_unit_multiplier(raw.abs() as i64) as f64;
    if !nanos.is_finite() || nanos < i64::MIN as f64 || nanos > i64::MAX as f64 {
        return None;
    }
    Some(nanos.round() as i64)
}

/// 布尔文本（去空白/大小写后）→ bool：与运行时文件输入解析一致，
/// 仅接受 true/false 与 1/0（`"1"`/`"TRUE"`/`" true "` 均有效）。
fn parse_bool_text(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

fn build_array(field: &Field, values: &[serde_json::Value]) -> Result<ArrayRef, String> {
    match field.data_type() {
        DataType::Utf8 | DataType::LargeUtf8 => {
            let arr: StringArray = values
                .iter()
                .map(|v| match v {
                    serde_json::Value::String(s) => Some(s.clone()),
                    serde_json::Value::Null => None,
                    other => Some(other.to_string()),
                })
                .collect::<Vec<Option<String>>>()
                .into_iter()
                .collect();
            Ok(Arc::new(arr))
        }
        DataType::Int64 => {
            let arr: Int64Array = values
                .iter()
                .map(|v| match v {
                    serde_json::Value::Number(n) => n.as_i64(),
                    serde_json::Value::String(s) => s.parse::<i64>().ok(),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .into();
            Ok(Arc::new(arr))
        }
        DataType::Float64 => {
            let arr: Float64Array = values
                .iter()
                .map(|v| match v {
                    serde_json::Value::Number(n) => n.as_f64(),
                    serde_json::Value::String(s) => s.parse::<f64>().ok(),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .into();
            Ok(Arc::new(arr))
        }
        DataType::Boolean => {
            let arr: BooleanArray = values
                .iter()
                .map(|v| match v {
                    serde_json::Value::Bool(b) => Some(*b),
                    serde_json::Value::String(s) => parse_bool_text(s),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .into();
            Ok(Arc::new(arr))
        }
        DataType::Timestamp(TimeUnit::Nanosecond, None) => {
            let arr: TimestampNanosecondArray = values
                .iter()
                .map(timestamp_value_nanos)
                .collect::<Vec<_>>()
                .into();
            Ok(Arc::new(arr))
        }
        other => Err(format!("unsupported Arrow type for NDJSON: {other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::Array;

    fn test_schema() -> Schema {
        Schema::new(vec![
            Field::new("sip", DataType::Utf8, true),
            Field::new("dport", DataType::Int64, true),
            Field::new("score", DataType::Float64, true),
            Field::new("active", DataType::Boolean, true),
            Field::new(
                "event_time",
                DataType::Timestamp(TimeUnit::Nanosecond, None),
                true,
            ),
        ])
    }

    #[test]
    fn parse_simple_ndjson() {
        let schema = Schema::new(vec![
            Field::new("sip", DataType::Utf8, true),
            Field::new("dport", DataType::Int64, true),
        ]);
        let lines = vec![
            r#"{"sip":"10.0.0.1","dport":"443"}"#.to_string(),
            r#"{"sip":"10.0.0.2","dport":"80"}"#.to_string(),
        ];
        let batch = ndjson_to_record_batch(&lines, &schema).unwrap().unwrap();
        assert_eq!(batch.num_rows(), 2);
        assert_eq!(batch.num_columns(), 2);
    }

    #[test]
    fn empty_lines_returns_none() {
        let schema = test_schema();
        let result = ndjson_to_record_batch(&[], &schema).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn null_fields_become_null() {
        let schema = test_schema();
        let lines = vec![
            r#"{"sip":null,"dport":null,"score":null,"active":null,"event_time":null}"#.to_string(),
        ];
        let batch = ndjson_to_record_batch(&lines, &schema).unwrap().unwrap();
        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 5);
    }

    #[test]
    fn float_and_bool_types() {
        let schema = Schema::new(vec![
            Field::new("score", DataType::Float64, true),
            Field::new("active", DataType::Boolean, true),
        ]);
        let lines = vec![
            r#"{"score":70.5,"active":true}"#.to_string(),
            r#"{"score":0.0,"active":false}"#.to_string(),
            r#"{"score":"99.9","active":"true"}"#.to_string(),
        ];
        let batch = ndjson_to_record_batch(&lines, &schema).unwrap().unwrap();
        assert_eq!(batch.num_rows(), 3);
        assert_eq!(batch.num_columns(), 2);
    }

    #[test]
    fn timestamp_from_rfc3339() {
        let schema = Schema::new(vec![Field::new(
            "ts",
            DataType::Timestamp(TimeUnit::Nanosecond, None),
            true,
        )]);
        let lines = vec![
            r#"{"ts":"2026-01-01T00:00:00Z"}"#.to_string(),
            r#"{"ts":"2026-01-01T00:00:01Z"}"#.to_string(),
        ];
        let batch = ndjson_to_record_batch(&lines, &schema).unwrap().unwrap();
        assert_eq!(batch.num_rows(), 2);
    }

    #[test]
    fn timestamp_from_numeric_epoch_units() {
        // issue #95: numeric JSON timestamps (s / ms / us / ns and numeric
        // strings) must map to epoch nanoseconds — previously only RFC3339
        // strings were recognized and numbers became null.
        let schema = Schema::new(vec![Field::new(
            "ts",
            DataType::Timestamp(TimeUnit::Nanosecond, None),
            true,
        )]);
        let lines = vec![
            r#"{"ts":1600000000}"#.to_string(),
            r#"{"ts":1600000000001}"#.to_string(),
            r#"{"ts":1600000000001002}"#.to_string(),
            r#"{"ts":1600000000001002003}"#.to_string(),
            r#"{"ts":"1600000000001"}"#.to_string(),
            r#"{"ts":1600000000001.0}"#.to_string(),
        ];
        let batch = ndjson_to_record_batch(&lines, &schema).unwrap().unwrap();
        let arr = batch
            .column(0)
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .expect("timestamp column");
        assert_eq!(
            &arr.values()[..5],
            &[
                1600000000000000000, // s
                1600000000001000000, // ms
                1600000000001002000, // us
                1600000000001002003, // ns
                1600000000001000000, // numeric string (ms)
            ]
        );
        // 浮点路径在 ns 量级受 f64 精度限制，允许毫秒舍入误差。
        assert!(
            (arr.value(5) - 1600000000001000000).abs() < 1000,
            "float ms drifted: {}",
            arr.value(5)
        );
    }

    #[test]
    fn boolean_text_forms_match_file_input() {
        // 与运行时文件输入一致：true/false/1/0 + 大小写与空白均有效。
        let schema = Schema::new(vec![Field::new("active", DataType::Boolean, true)]);
        let lines = vec![
            r#"{"active":true}"#.to_string(),
            r#"{"active":"true"}"#.to_string(),
            r#"{"active":"TRUE"}"#.to_string(),
            r#"{"active":" 1 "}"#.to_string(),
            r#"{"active":"0"}"#.to_string(),
            r#"{"active":"false"}"#.to_string(),
            r#"{"active":null}"#.to_string(),
        ];
        let batch = ndjson_to_record_batch(&lines, &schema).unwrap().unwrap();
        let arr = batch
            .column(0)
            .as_any()
            .downcast_ref::<BooleanArray>()
            .expect("boolean column");
        assert_eq!(
            (0..batch.num_rows())
                .map(|r| arr.is_valid(r).then(|| arr.value(r)))
                .collect::<Vec<_>>(),
            vec![
                Some(true),
                Some(true),
                Some(true),
                Some(true),
                Some(false),
                Some(false),
                None,
            ]
        );
    }

    #[test]
    fn timestamp_issue_sample_millis() {
        // Issue #95 复现样本：1643163078468 ms → 1643163078468000000 ns。
        let schema = Schema::new(vec![Field::new(
            "time_field",
            DataType::Timestamp(TimeUnit::Nanosecond, None),
            true,
        )]);
        let lines = vec![r#"{"time_field":1643163078468}"#.to_string()];
        let batch = ndjson_to_record_batch(&lines, &schema).unwrap().unwrap();
        let arr = batch
            .column(0)
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .expect("timestamp column");
        assert_eq!(arr.value(0), 1_643_163_078_468_000_000);
    }

    #[test]
    fn missing_field_defaults_to_null() {
        let schema = Schema::new(vec![
            Field::new("sip", DataType::Utf8, true),
            Field::new("dport", DataType::Int64, true),
        ]);
        let lines = vec![r#"{"sip":"10.0.0.1"}"#.to_string()];
        let batch = ndjson_to_record_batch(&lines, &schema).unwrap().unwrap();
        assert_eq!(batch.num_rows(), 1);
    }

    #[test]
    fn invalid_json_returns_error() {
        let schema = test_schema();
        let lines = vec!["not json".to_string()];
        let result = ndjson_to_record_batch(&lines, &schema);
        assert!(result.is_err());
    }

    #[test]
    fn extra_fields_are_ignored() {
        let schema = Schema::new(vec![Field::new("sip", DataType::Utf8, true)]);
        let lines = vec![r#"{"sip":"10.0.0.1","extra_field":"ignored","another":42}"#.to_string()];
        let batch = ndjson_to_record_batch(&lines, &schema).unwrap().unwrap();
        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 1);
    }
}
