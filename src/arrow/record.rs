//! `DataRecord` → `RecordBatch` conversion with typed column builders.

use arrow::array::{
    ArrayRef, BinaryBuilder, BooleanBuilder, Float64Builder, Int32Builder, Int64Builder,
    StringBuilder, TimestampNanosecondBuilder,
};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;
use orion_error::conversion::ToStructError;
use std::sync::Arc;
use wp_connector_api::{SinkReason, SinkResult};
use wp_model_core::model::DataRecord;
use wp_model_core::model::Value;

// ---------------------------------------------------------------------------
// DataRecord → RecordBatch (migrated from arrow_conv/batch.rs)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// DataRecord → RecordBatch
// ---------------------------------------------------------------------------

/// Convert a single `DataRecord` into an Arrow `RecordBatch`.
///
/// Each field in the schema is looked up by name in the record.
/// Missing fields default to null.
pub fn data_record_to_batch(record: &DataRecord, schema: &Arc<Schema>) -> SinkResult<RecordBatch> {
    let mut columns: Vec<ArrayRef> = Vec::with_capacity(schema.fields().len());

    for field in schema.fields() {
        let records = [Arc::new(record.clone())];
        columns.push(build_column_from_field(field, &records)?);
    }

    RecordBatch::try_new(Arc::clone(schema), columns).map_err(|e| {
        SinkReason::Sink
            .to_err()
            .with_detail(format!("data_record_to_batch failed: {e}"))
    })
}

/// Convert multiple `DataRecord`s into a single Arrow `RecordBatch`.
///
/// Each field in the schema is looked up by name in each record.
/// Missing fields default to null.
pub fn data_records_to_batch(
    records: &[Arc<DataRecord>],
    schema: &Arc<Schema>,
) -> SinkResult<RecordBatch> {
    if records.is_empty() {
        let empty_columns: Vec<ArrayRef> = schema
            .fields()
            .iter()
            .map(|f| empty_column_for_type(f.data_type(), 0))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| {
                SinkReason::Sink
                    .to_err()
                    .with_detail(format!("data_records_to_batch empty failed: {e}"))
            })?;
        return RecordBatch::try_new(Arc::clone(schema), empty_columns).map_err(|e| {
            SinkReason::Sink
                .to_err()
                .with_detail(format!("data_records_to_batch empty failed: {e}"))
        });
    }

    let num_fields = schema.fields().len();
    let mut columns: Vec<ArrayRef> = Vec::with_capacity(num_fields);

    for field in schema.fields() {
        columns.push(build_column_from_field(field, records)?);
    }

    RecordBatch::try_new(Arc::clone(schema), columns).map_err(|e| {
        SinkReason::Sink
            .to_err()
            .with_detail(format!("data_records_to_batch failed: {e}"))
    })
}

// ---------------------------------------------------------------------------
// Column builders
// ---------------------------------------------------------------------------

fn build_column_from_field(field: &Field, records: &[Arc<DataRecord>]) -> SinkResult<ArrayRef> {
    let field_name = field.name();
    match field.data_type() {
        DataType::Boolean => {
            let mut builder = BooleanBuilder::with_capacity(records.len());
            for record in records {
                match record.field(field_name).map(|f| f.get_value()) {
                    Some(Value::Bool(v)) => builder.append_value(*v),
                    Some(Value::Chars(s)) => builder.append_value(s.eq_ignore_ascii_case("true")),
                    _ => builder.append_null(),
                }
            }
            Ok(Arc::new(builder.finish()) as ArrayRef)
        }
        DataType::Int64 => {
            let mut builder = Int64Builder::with_capacity(records.len());
            for record in records {
                match record
                    .field(field_name)
                    .and_then(|f| parse_digit(f.get_value()))
                {
                    Some(v) => builder.append_value(v),
                    None => builder.append_null(),
                }
            }
            Ok(Arc::new(builder.finish()) as ArrayRef)
        }
        DataType::Int32 => {
            let mut builder = Int32Builder::with_capacity(records.len());
            for record in records {
                match record
                    .field(field_name)
                    .and_then(|f| parse_digit(f.get_value()))
                {
                    Some(v) => builder.append_value(v as i32),
                    None => builder.append_null(),
                }
            }
            Ok(Arc::new(builder.finish()) as ArrayRef)
        }
        DataType::Binary => {
            let mut builder = BinaryBuilder::with_capacity(records.len(), records.len() * 64);
            for record in records {
                match record.field(field_name).map(|f| f.get_value()) {
                    Some(v) => {
                        let bytes = to_raw_bytes(v);
                        builder.append_value(&bytes[..]);
                    }
                    None => builder.append_null(),
                }
            }
            Ok(Arc::new(builder.finish()) as ArrayRef)
        }
        DataType::Float64 => {
            let mut builder = Float64Builder::with_capacity(records.len());
            for record in records {
                match record
                    .field(field_name)
                    .and_then(|f| parse_float(f.get_value()))
                {
                    Some(v) => builder.append_value(v),
                    None => builder.append_null(),
                }
            }
            Ok(Arc::new(builder.finish()) as ArrayRef)
        }
        DataType::Timestamp(TimeUnit::Nanosecond, None) => {
            let mut builder = TimestampNanosecondBuilder::with_capacity(records.len());
            for record in records {
                match record
                    .field(field_name)
                    .and_then(|f| parse_timestamp_ns(f.get_value()))
                {
                    Some(v) => builder.append_value(v),
                    None => builder.append_null(),
                }
            }
            Ok(Arc::new(builder.finish()) as ArrayRef)
        }
        // Utf8 and everything else
        _ => {
            let mut builder = StringBuilder::with_capacity(records.len(), records.len() * 32);
            for record in records {
                match record.field(field_name) {
                    Some(f) => builder.append_value(format_utf8_value(f.get_value())),
                    None => builder.append_null(),
                }
            }
            Ok(Arc::new(builder.finish()) as ArrayRef)
        }
    }
}

// ---------------------------------------------------------------------------
// Value formatting
// ---------------------------------------------------------------------------

/// Format a [`Value`] for Utf8 column output.
///
/// Complex types (`Obj`, `Array`) are serialized as JSON; all others use [`Display`].
fn format_utf8_value(v: &Value) -> String {
    match v {
        Value::Obj(_) | Value::Array(_) => {
            serde_json::to_string(v).unwrap_or_else(|_| format!("{v:?}"))
        }
        _ => v.to_string(),
    }
}

/// Convert a [`Value`] to raw bytes for Binary column output.
///
/// `Value::Hex` stores the decoded value as `u128` — extract minimal big-endian bytes.
/// All other types fall back to the UTF-8 string representation.
fn to_raw_bytes(v: &Value) -> Vec<u8> {
    match v {
        Value::Hex(h) => {
            if h.0 == 0 {
                return vec![0];
            }
            let be = h.0.to_be_bytes();
            let start = be.iter().position(|&b| b != 0).unwrap();
            be[start..].to_vec()
        }
        _ => format_utf8_value(v).into_bytes(),
    }
}

// ---------------------------------------------------------------------------
// Parse helpers — extract typed values with Chars fallback
// ---------------------------------------------------------------------------

fn parse_digit(v: &Value) -> Option<i64> {
    match v {
        Value::Digit(d) => Some(*d),
        Value::Float(f) => Some(*f as i64),
        Value::Chars(s) => s.parse().ok(),
        _ => None,
    }
}

fn parse_float(v: &Value) -> Option<f64> {
    match v {
        Value::Float(f) => Some(*f),
        Value::Digit(d) => Some(*d as f64),
        Value::Chars(s) => s.parse().ok(),
        _ => None,
    }
}

fn parse_timestamp_ns(v: &Value) -> Option<i64> {
    match v {
        Value::Time(t) => Some(t.and_utc().timestamp_nanos_opt()?),
        Value::Digit(d) => d.checked_mul(1_000_000),
        Value::Chars(s) => chrono::DateTime::parse_from_rfc3339(s)
            .ok()
            .or_else(|| {
                chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
                    .ok()
                    .map(|dt| dt.and_utc().fixed_offset())
            })
            .and_then(|dt| dt.timestamp_nanos_opt()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Empty column helpers
// ---------------------------------------------------------------------------

fn empty_column_for_type(data_type: &DataType, _capacity: usize) -> Result<ArrayRef, String> {
    let arr: ArrayRef = match data_type {
        DataType::Boolean => Arc::new(arrow::array::BooleanArray::from(Vec::<bool>::new())),
        DataType::Int32 => Arc::new(arrow::array::Int32Array::from(Vec::<i32>::new())),
        DataType::Int64 => Arc::new(arrow::array::Int64Array::from(Vec::<i64>::new())),
        DataType::Float64 => Arc::new(arrow::array::Float64Array::from(Vec::<f64>::new())),
        DataType::Timestamp(TimeUnit::Nanosecond, None) => Arc::new(
            arrow::array::TimestampNanosecondArray::from(Vec::<i64>::new()),
        ),
        DataType::Binary => Arc::new(arrow::array::BinaryArray::from(Vec::<Option<&[u8]>>::new())),
        _ => Arc::new(arrow::array::StringArray::from(Vec::<Option<&str>>::new())),
    };
    Ok(arr)
}
