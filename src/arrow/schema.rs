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
        // 任意精度整数（BigUint）：以十进制字符串输出（与 format_utf8_value 的 to_string 一致）
        WpDt::BigInt => DataType::Utf8,
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

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::datatypes::DataType;
    use wp_model_core::model::{DataRecord, Field as ModelField, FieldStorage};

    #[test]
    fn schema_inferred_from_record() {
        let rec = DataRecord::from(vec![
            FieldStorage::from(ModelField::from_chars("name", "a")),
            FieldStorage::from(ModelField::from_digit("count", 1)),
        ]);
        let schema = infer_schema_from_record(&rec);
        assert_eq!(schema.fields().len(), 2);
        assert_eq!(schema.field(0).name(), "name");
        assert_eq!(schema.field(1).name(), "count");
    }

    #[test]
    fn ignore_field_excluded() {
        let rec = DataRecord::from(vec![
            FieldStorage::from(ModelField::from_chars("name", "a")),
            FieldStorage::from(ModelField::from_ignore("junk")),
        ]);
        let schema = infer_schema_from_record(&rec);
        assert_eq!(schema.fields().len(), 1);
        assert_eq!(schema.field(0).name(), "name");
    }

    #[test]
    fn bool_maps_to_boolean() {
        let dt = wp_type_to_arrow(&wp_model_core::model::DataType::Bool);
        assert_eq!(dt, DataType::Boolean);
    }

    #[test]
    fn digit_maps_to_int64() {
        let dt = wp_type_to_arrow(&wp_model_core::model::DataType::Digit);
        assert_eq!(dt, DataType::Int64);
    }

    #[test]
    fn hex_maps_to_binary() {
        let dt = wp_type_to_arrow(&wp_model_core::model::DataType::Hex);
        assert_eq!(dt, DataType::Binary);
    }

    #[test]
    fn bigint_maps_to_utf8() {
        // 任意精度整数（BigUint）以十进制字符串输出
        let dt = wp_type_to_arrow(&wp_model_core::model::DataType::BigInt);
        assert_eq!(dt, DataType::Utf8);
    }
}
