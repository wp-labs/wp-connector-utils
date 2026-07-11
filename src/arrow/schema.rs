//! Arrow schema inference from `DataRecord` fields.

use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use wp_model_core::model::DataRecord;

// ---------------------------------------------------------------------------
// Schema inference (migrated from arrow_conv/schema.rs)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Schema inference
// ---------------------------------------------------------------------------

/// Infer an Arrow schema from field names.
///
/// All fields default to `Utf8` (conservative).
pub fn infer_arrow_schema(fields: &[String]) -> Schema {
    Schema::new(
        fields
            .iter()
            .map(|f| Field::new(f.as_str(), DataType::Utf8, true))
            .collect::<Vec<_>>(),
    )
}

/// Map wp_model_core DataType to Arrow DataType.
fn wp_type_to_arrow(dt: &wp_model_core::model::DataType) -> DataType {
    use wp_model_core::model::DataType as WpDt;
    match dt {
        WpDt::Bool => DataType::Boolean,
        WpDt::Digit => DataType::Int64,
        WpDt::Float => DataType::Float64,
        WpDt::Port => DataType::Int32,
        WpDt::Time
        | WpDt::TimeISO
        | WpDt::TimeRFC3339
        | WpDt::TimeRFC2822
        | WpDt::TimeTIMESTAMP
        | WpDt::TimeCLF => DataType::Timestamp(TimeUnit::Nanosecond, None),
        WpDt::Hex | WpDt::Base64 => DataType::Binary,
        WpDt::Chars
        | WpDt::Symbol
        | WpDt::PeekSymbol
        | WpDt::IP
        | WpDt::IpNet
        | WpDt::Domain
        | WpDt::Email
        | WpDt::Url
        | WpDt::SN
        | WpDt::IdCard
        | WpDt::MobilePhone
        | WpDt::KV
        | WpDt::KvArr
        | WpDt::Json
        | WpDt::ExactJson
        | WpDt::HttpRequest
        | WpDt::HttpStatus
        | WpDt::HttpAgent
        | WpDt::HttpMethod
        | WpDt::Auto
        | WpDt::ProtoText
        | WpDt::Obj
        | WpDt::Ignore => DataType::Utf8,
        WpDt::Array(_) => DataType::Utf8,
    }
}

/// Infer an Arrow schema from a DataRecord using actual field types (get_meta()).
///
/// Fields with `DataType::Ignore` are excluded from the schema.
pub fn infer_schema_from_record(record: &DataRecord) -> Schema {
    Schema::new(
        record
            .items
            .iter()
            .filter(|f| !matches!(f.get_meta(), wp_model_core::model::DataType::Ignore))
            .map(|f| {
                let arrow_type = wp_type_to_arrow(f.get_meta());
                Field::new(f.get_name(), arrow_type, true)
            })
            .collect::<Vec<_>>(),
    )
}
