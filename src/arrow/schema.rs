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

/// Map `wp_model_core::model::DataType` to Arrow `DataType`.
///
/// **这是 wp-model ↔ Arrow 列类型契约的唯一实现**（线协议口径的单一事实来源）：
/// 接收侧期望（`wf-runtime`）与规格表都以它为准，规格表见
/// `wp-reactor/docs/design/arrow-type-mapping.md`。
///
/// match 是穷尽的（无 `_` 兜底）：`wp-model-core` 新增变体会直接**编译失败** —— 这是刻意的，
/// 逼对新类型表态，而不是静默兜到 `Utf8`。
pub fn wp_type_to_arrow(dt: &wp_model_core::model::DataType) -> DataType {
    use wp_model_core::model::DataType as WpDt;
    match dt {
        WpDt::Bool => DataType::Boolean,
        WpDt::Int => DataType::Int64,
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
        // DIV-1（已修复）：`hex` 走 Utf8（十六进制字符串）。期望侧（`wf-runtime`）与
        // `wp-arrow` 都是 Utf8，且 `Value::Hex` 的 `Display` 就是 `{:#X}`
        // （`wp-model-core` primitive.rs），与本 crate 的 Utf8 值层
        // （`format_utf8_value`）同形 —— 所以只需这一行对齐，值层不用改。
        // 规格表：wp-reactor `docs/design/arrow-type-mapping.md` DIV-1
        WpDt::Hex => DataType::Utf8,
        WpDt::Base64 => DataType::Binary,
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
            FieldStorage::from(ModelField::from_int("count", 1)),
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
        let dt = wp_type_to_arrow(&wp_model_core::model::DataType::Int);
        assert_eq!(dt, DataType::Int64);
    }

    #[test]
    fn hex_maps_to_utf8() {
        // DIV-1：`hex` 与 wp-arrow / wf-runtime 期望侧一致走 Utf8（十六进制字符串）
        let dt = wp_type_to_arrow(&wp_model_core::model::DataType::Hex);
        assert_eq!(dt, DataType::Utf8);
    }

    #[test]
    fn bigint_maps_to_utf8() {
        // 任意精度整数（BigUint）以十进制字符串输出
        let dt = wp_type_to_arrow(&wp_model_core::model::DataType::BigInt);
        assert_eq!(dt, DataType::Utf8);
    }

    /// 规格表钉桩：`wp_type_to_arrow` 的**全 37 个** DataType 变体映射。
    ///
    /// 规格表：`wp-reactor/docs/design/arrow-type-mapping.md` §3（B 列）。
    /// 该表是 sink 侧与接收侧（`wf-runtime`）之间 Arrow 列类型契约的单一事实来源；
    /// 本测试把它钉死，使任何口径漂移都以测试失败暴露，而不是线上静默出错。
    ///
    /// `wp_type_to_arrow` 的 match 是穷尽的（无 `_` 兜底），因此 wp-model-core
    /// 新增变体会直接编译失败；`cases.len()` 断言则保证本表与文档同步更新。
    #[test]
    fn arrow_contract_full_mapping_is_pinned() {
        use wp_model_core::model::{ArraySubtype, DataType as WpDt};

        let ts = DataType::Timestamp(TimeUnit::Nanosecond, None);
        let cases: Vec<(WpDt, DataType)> = vec![
            (WpDt::Bool, DataType::Boolean),
            (WpDt::Chars, DataType::Utf8),
            (WpDt::Symbol, DataType::Utf8),
            (WpDt::PeekSymbol, DataType::Utf8),
            (WpDt::Int, DataType::Int64),
            // DIV-2：wp-arrow 侧为 Decimal256(39,0)（数值语义），见规格表 §4
            (WpDt::BigInt, DataType::Utf8),
            (WpDt::Float, DataType::Float64),
            // `Ignore` 本身映 Utf8，但 `infer_schema_from_record` 会把它剔除
            (WpDt::Ignore, DataType::Utf8),
            (WpDt::Time, ts.clone()),
            (WpDt::TimeISO, ts.clone()),
            (WpDt::TimeRFC3339, ts.clone()),
            (WpDt::TimeRFC2822, ts.clone()),
            (WpDt::TimeTIMESTAMP, ts.clone()),
            (WpDt::TimeCLF, ts.clone()),
            (WpDt::IP, DataType::Utf8),
            (WpDt::IpNet, DataType::Utf8),
            (WpDt::Domain, DataType::Utf8),
            (WpDt::Email, DataType::Utf8),
            (WpDt::Port, DataType::Int32),
            (WpDt::SN, DataType::Utf8),
            // DIV-1（已修复）：Hex 与 wp-arrow / 期望侧一致走 Utf8
            (WpDt::Hex, DataType::Utf8),
            (WpDt::Base64, DataType::Binary),
            (WpDt::KV, DataType::Utf8),
            (WpDt::KvArr, DataType::Utf8),
            (WpDt::Json, DataType::Utf8),
            (WpDt::ExactJson, DataType::Utf8),
            (WpDt::HttpRequest, DataType::Utf8),
            (WpDt::HttpStatus, DataType::Utf8),
            (WpDt::HttpAgent, DataType::Utf8),
            (WpDt::HttpMethod, DataType::Utf8),
            (WpDt::Url, DataType::Utf8),
            (WpDt::Auto, DataType::Utf8),
            (WpDt::ProtoText, DataType::Utf8),
            // DIV-3：结构化字段只给 Utf8 且不带 `wfl_field_type` 元数据；
            // 期望侧给 Utf8 + kind 元数据，wp-arrow 给 List。
            (WpDt::Obj, DataType::Utf8),
            (WpDt::Array(ArraySubtype::new("int")), DataType::Utf8),
            (WpDt::IdCard, DataType::Utf8),
            (WpDt::MobilePhone, DataType::Utf8),
        ];

        assert_eq!(
            cases.len(),
            37,
            "wp-model-core DataType 变体数变化 → 同步更新规格表 §3"
        );
        for (dt, expected) in cases {
            assert_eq!(wp_type_to_arrow(&dt), expected, "DataType::{dt:?}");
        }
    }
}
