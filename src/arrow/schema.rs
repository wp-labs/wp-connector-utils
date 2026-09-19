//! Arrow schema inference from `DataRecord` fields.
//!
//! # 类型映射的归属（A-2 第 2 步）
//!
//! `wp_type_to_arrow` 的**实现已不在本 crate** —— 它迁到了
//! [`wp_arrow::contract::wp_type_to_arrow`]，本模块只做**再导出**（`pub use`），
//! 所以 `wp_connector_utils::arrow::wp_type_to_arrow` 这个公开路径不变、
//! 且它**就是**契约实现本身（不是副本，结构上不可能漂移）。
//!
//! 为什么迁走：本 crate 是「面向 sink 的 connector 工具」，而 wp-model ↔ Arrow 的列类型契约
//! 是 wparse(sink) ↔ wfusion(接收) 之间的**线协议**口径，语义归属应在专门做这件事的 `wp-arrow`。
//! 放在这里造成的实际后果是 `wp-labs/warp-fusion#102`：按自我声明去找权威实现的人找到
//! `wp-arrow` 的 9 变体类型化前端，拿到另一套口径，于是报出「不一致」。
//!
//! 规格表（单一事实来源）：`wp-reactor/docs/design/arrow-type-mapping.md`。

use arrow::datatypes::{DataType, Field, Schema};
use wp_model_core::model::DataRecord;

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

/// `wp_model_core::model::DataType` → Arrow 列类型（**线协议契约口径**）。
///
/// 实现见 [`wp_arrow::contract::wp_type_to_arrow`]（A-2 第 2 步迁出）；本处仅为路径兼容的再导出。
/// match 是穷尽的（无 `_` 兜底）：`wp-model-core` 新增变体会直接**编译失败** ——
/// 这是刻意的，逼对新类型表态，而不是静默兜到 `Utf8`。
pub use wp_arrow::contract::wp_type_to_arrow;

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
    use arrow::datatypes::TimeUnit;
    use wp_model_core::model::{ArraySubtype, DataType as WpDt, Field as ModelField, FieldStorage};

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

    // -----------------------------------------------------------------------
    // 转发路径的冒烟测试
    //
    // **全 37 变体的口径钉桩不在这里** —— 它在唯一实现处：
    // `wp-arrow` 的 `contract::tests::wire_contract_full_mapping_is_pinned`。
    // 这里只断言两件事：① 再导出确实可用；② 历史上**分叉过**的几行（DIV-1 `Hex`、
    // DIV-2 `BigInt`、DIV-3 `Array`）以及时间/二进制这类易错行仍是契约口径。
    // 若 wp-arrow 在 patch 版里改了口径，这几行会在**消费侧**先报警。
    // -----------------------------------------------------------------------

    #[test]
    fn reexported_mapping_is_the_contract_implementation() {
        // 同一函数（`pub use`），不是副本：两侧对同一输入必然一致。
        for dt in [
            WpDt::Bool,
            WpDt::Int,
            WpDt::Float,
            WpDt::Port,
            WpDt::Hex,
            WpDt::BigInt,
            WpDt::Base64,
            WpDt::Obj,
            WpDt::Array(ArraySubtype::new("int")),
        ] {
            assert_eq!(
                wp_type_to_arrow(&dt),
                wp_arrow::contract::wp_type_to_arrow(&dt),
                "DataType::{dt:?}"
            );
        }
    }

    #[test]
    fn hex_maps_to_utf8() {
        // DIV-1（P0）：`hex` 必须 Utf8（十六进制字符串）。退回 `Binary` 会断链：
        // `hex` 不带结构化元数据 → 接收侧走严格相等 → `arrow source schema mismatch`。
        assert_eq!(wp_type_to_arrow(&WpDt::Hex), DataType::Utf8);
    }

    #[test]
    fn bigint_maps_to_utf8() {
        // DIV-2：任意精度整数以十进制字符串传输（`wp-arrow` 的类型化前端才是 Decimal256）。
        assert_eq!(wp_type_to_arrow(&WpDt::BigInt), DataType::Utf8);
    }

    #[test]
    fn time_binary_and_structured_rows_keep_the_contract_shape() {
        let ts = DataType::Timestamp(TimeUnit::Nanosecond, None);
        // 六个时间变体同口径
        for dt in [
            WpDt::Time,
            WpDt::TimeISO,
            WpDt::TimeRFC3339,
            WpDt::TimeRFC2822,
            WpDt::TimeTIMESTAMP,
            WpDt::TimeCLF,
        ] {
            assert_eq!(wp_type_to_arrow(&dt), ts, "DataType::{dt:?}");
        }
        assert_eq!(wp_type_to_arrow(&WpDt::Base64), DataType::Binary);
        // DIV-3：结构化字段走 Utf8（JSON 文本）且**不带** `wfl_field_type` 元数据
        assert_eq!(
            wp_type_to_arrow(&WpDt::Array(ArraySubtype::new("int"))),
            DataType::Utf8
        );
        assert_eq!(wp_type_to_arrow(&WpDt::Obj), DataType::Utf8);
    }
}
