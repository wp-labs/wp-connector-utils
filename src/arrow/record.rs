//! `DataRecord` → `RecordBatch` **值层**（A-2 第 3 步：实现已迁往 `wp-arrow`）。
//!
//! 本模块现在只做两件事：**转发**到 `wp_arrow::contract::{encode_record, encode_records}`，
//! 以及把 [`wp_arrow::error::WpArrowError`] 映射回 connector 侧的 `SinkResult`。
//! 公开签名与错误文案形状不变，调用方（`wp-core-connectors` 的 file/tcp sink）零改动。
//!
//! # 为什么迁走
//!
//! 本 crate 是「面向 sink 的 connector 工具」，而 `DataRecord → 列` 的编码口径属于
//! **wparse(sink) ↔ wfusion(接收) 的线协议契约**，语义归属在专门做这件事的 `wp-arrow`
//! （与 [`crate::arrow::wp_type_to_arrow`] 的表同处一层：表决定「这一列是什么 Arrow 类型」，
//! 编码决定「值怎么写进那一列」）。分开两处会让「改了一边忘了另一边」变成静默的线上错配。
//!
//! 规格表（单一事实来源）：`wp-reactor/docs/design/arrow-type-mapping.md`。

use std::sync::Arc;

use arrow::datatypes::Schema;
use arrow::record_batch::RecordBatch;
use orion_error::conversion::ToStructError;
use wp_connector_api::{SinkReason, SinkResult};
use wp_model_core::model::DataRecord;

/// 单条 `DataRecord` → 一行 `RecordBatch`（转发，见模块文档）。
///
/// 每个列按名字在记录里查找；缺字段 → null。
pub fn data_record_to_batch(record: &DataRecord, schema: &Arc<Schema>) -> SinkResult<RecordBatch> {
    wp_arrow::contract::encode_record(record, schema).map_err(|e| {
        SinkReason::Sink
            .to_err()
            .with_detail(format!("data_record_to_batch failed: {e}"))
    })
}

/// 多条 `DataRecord` → 一个 `RecordBatch`（转发，见模块文档）。
///
/// 每个列按名字在**每条**记录里查找；缺字段 → null。`records` 为空时按 schema 产零行。
pub fn data_records_to_batch(
    records: &[Arc<DataRecord>],
    schema: &Arc<Schema>,
) -> SinkResult<RecordBatch> {
    wp_arrow::contract::encode_records(records, schema).map_err(|e| {
        SinkReason::Sink
            .to_err()
            .with_detail(format!("data_records_to_batch failed: {e}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::Array;
    use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
    use std::sync::Arc;
    use wp_model_core::model::{DataRecord, Field as ModelField, FieldStorage};

    fn make_schema(fields: &[&str]) -> Arc<Schema> {
        Arc::new(Schema::new(
            fields
                .iter()
                .map(|f| Field::new(*f, DataType::Utf8, true))
                .collect::<Vec<_>>(),
        ))
    }

    // -- data_record_to_batch -------------------------------------------

    #[test]
    fn record_to_batch_roundtrip() {
        let rec = DataRecord::from(vec![
            FieldStorage::from(ModelField::from_chars("name", "alice")),
            FieldStorage::from(ModelField::from_chars("count", "42")),
        ]);
        let schema = make_schema(&["name", "count"]);
        let batch = data_record_to_batch(&rec, &schema).unwrap();
        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 2);
    }

    #[test]
    fn missing_field_defaults_to_null() {
        let rec = DataRecord::from(vec![FieldStorage::from(ModelField::from_chars("x", "v"))]);
        let s = Arc::new(Schema::new(vec![
            Field::new("x", DataType::Utf8, true),
            Field::new("y", DataType::Int64, true),
        ]));
        let b = data_record_to_batch(&rec, &s).unwrap();
        assert_eq!(b.num_columns(), 2);
        let y = b
            .column(1)
            .as_any()
            .downcast_ref::<arrow::array::Int64Array>()
            .unwrap();
        assert!(y.is_null(0));
    }

    // -- data_records_to_batch ------------------------------------------

    #[test]
    fn records_to_batch_multiple_rows() {
        let records: Vec<Arc<DataRecord>> = (0..3)
            .map(|i| {
                Arc::new(DataRecord::from(vec![FieldStorage::from(
                    ModelField::from_chars("v", format!("{i}")),
                )]))
            })
            .collect();
        let s = make_schema(&["v"]);
        let b = data_records_to_batch(&records, &s).unwrap();
        assert_eq!(b.num_rows(), 3);
    }

    #[test]
    fn records_to_batch_empty() {
        let s = make_schema(&["x"]);
        let b = data_records_to_batch(&[], &s).unwrap();
        assert_eq!(b.num_rows(), 0);
    }

    // -- 值层口径（消费侧拼线）-------------------------------------------

    /// DIV-1 修复后的**值层对拍**：`hex` 字段（现为 Utf8 列）写出的字符串，必须与
    /// `wp-arrow`（`convert.rs` `format!("{:#X}", h.0)`）和 `Value::Hex` 的 `Display`
    /// 逐字符一致。三者同形正是「schema 改一行即可对齐、值层不用改」的前提。
    #[test]
    fn hex_column_uses_the_same_string_form_as_wp_arrow() {
        use wp_model_core::model::types::value::HexT;

        let rec = DataRecord::from(vec![FieldStorage::from(ModelField::from_hex(
            "h",
            HexT(0x1A2B),
        ))]);
        let schema = make_schema(&["h"]);
        let batch = data_record_to_batch(&rec, &schema).unwrap();
        let col = batch
            .column(0)
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .expect("hex 列现在应是 Utf8/StringArray");
        assert_eq!(col.value(0), "0x1A2B");
        assert_eq!(col.value(0), format!("{:#X}", 0x1A2Bu128));
    }

    /// 反向钉桩：显式声明为 Binary 的列仍然拿原始字节（`to_raw_bytes` 的 Hex 分支）。
    #[test]
    fn explicit_binary_column_still_uses_raw_bytes() {
        use wp_model_core::model::types::value::HexT;

        let rec = DataRecord::from(vec![FieldStorage::from(ModelField::from_hex(
            "h",
            HexT(0x1A2B),
        ))]);
        let schema = Arc::new(Schema::new(vec![Field::new("h", DataType::Binary, true)]));
        let batch = data_record_to_batch(&rec, &schema).unwrap();
        let col = batch
            .column(0)
            .as_any()
            .downcast_ref::<arrow::array::BinaryArray>()
            .unwrap();
        assert_eq!(col.value(0), &[0x1A, 0x2B]);
    }

    /// 线协议**值层**的金标准：覆盖全部列类型 + 缺字段 + 类型回退（Chars/Int/Float/Time 互转）。
    ///
    /// A-2 2c 把值层实现从本 crate 迁到了 `wp-arrow` 的 `contract::value`；**同一份夹具与
    /// 同一组期望在两处各有一份**（本处 + `wp-arrow` 的
    /// `contract::value::tests::wire_value_encoding_is_pinned_by_golden_values`），
    /// 两份同时通过即证明搬迁是**等价**改动（不只是「看起来一样」）。
    ///
    /// 迁移后本测的职责变为**消费侧**拼线：若 `wp-arrow` 在 patch 版里改了值编码，
    /// 这里会先报。
    #[test]
    fn wire_value_encoding_is_pinned_by_golden_values() {
        use arrow::array::{
            BinaryArray, BooleanArray, Float64Array, Int32Array, Int64Array, StringArray,
            TimestampNanosecondArray,
        };
        use wp_model_core::model::DataField;
        use wp_model_core::model::types::value::{HexT, ObjectValue};

        let epoch =
            chrono::NaiveDateTime::parse_from_str("2024-01-01 00:00:00", "%Y-%m-%d %H:%M:%S")
                .unwrap();
        let ts_val = epoch + chrono::Duration::seconds(5);

        // row0：全字段齐、尽量走「正路」
        let mut obj = ObjectValue::new();
        obj.insert("k", DataField::from_chars("k", "v"));
        let row0 = DataRecord::from(vec![
            FieldStorage::from(DataField::from_bool("b", true)),
            FieldStorage::from(DataField::from_int("i64", 42)),
            FieldStorage::from(DataField::from_int("i32", 70_000)),
            FieldStorage::from(DataField::from_float("f", 1.5)),
            FieldStorage::from(DataField::from_time("ts", ts_val)),
            FieldStorage::from(DataField::from_chars("bin", "hi")),
            FieldStorage::from(DataField::from_obj("s", obj)),
        ]);

        // row1：类型回退（Chars 解析 / Float→Int / Int(ms)→时间戳 / Hex→Binary / Array→JSON）
        let arr = DataField::from_arr(
            "s",
            vec![
                DataField::from_chars("c", "x"),
                DataField::from_int("i", 22),
            ],
        );
        let row1 = DataRecord::from(vec![
            FieldStorage::from(DataField::from_chars("b", "TRUE")),
            FieldStorage::from(DataField::from_chars("i64", "42")),
            FieldStorage::from(DataField::from_float("i32", 3.9)),
            FieldStorage::from(DataField::from_chars("f", "2.71")),
            FieldStorage::from(DataField::from_int("ts", 1_700_000_000_000)),
            FieldStorage::from(DataField::from_hex("bin", HexT(0x1A2B))),
            FieldStorage::from(arr),
        ]);

        // row2：除 `s` 外全缺 → 其余列 null；`s` 走 Hex 的 Utf8 形态
        let row2 = DataRecord::from(vec![FieldStorage::from(DataField::from_hex(
            "s",
            HexT(0x1A2B),
        ))]);

        let rows = vec![Arc::new(row0), Arc::new(row1), Arc::new(row2)];
        let schema = Arc::new(Schema::new(vec![
            Field::new("b", DataType::Boolean, true),
            Field::new("i64", DataType::Int64, true),
            Field::new("i32", DataType::Int32, true),
            Field::new("f", DataType::Float64, true),
            Field::new("ts", DataType::Timestamp(TimeUnit::Nanosecond, None), true),
            Field::new("bin", DataType::Binary, true),
            Field::new("s", DataType::Utf8, true),
        ]));

        let batch = data_records_to_batch(&rows, &schema).unwrap();
        assert_eq!(batch.num_rows(), 3);

        let b = batch
            .column(0)
            .as_any()
            .downcast_ref::<BooleanArray>()
            .unwrap();
        assert_eq!((b.value(0), b.value(1)), (true, true));
        assert!(b.is_null(2));

        let i64c = batch
            .column(1)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert_eq!((i64c.value(0), i64c.value(1)), (42, 42));
        assert!(i64c.is_null(2));

        let i32c = batch
            .column(2)
            .as_any()
            .downcast_ref::<Int32Array>()
            .unwrap();
        assert_eq!((i32c.value(0), i32c.value(1)), (70_000, 3));
        assert!(i32c.is_null(2));

        let fc = batch
            .column(3)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert_eq!(fc.value(0), 1.5);
        assert_eq!(fc.value(1), 2.71);
        assert!(fc.is_null(2));

        let tsc = batch
            .column(4)
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();
        assert_eq!(
            tsc.value(0),
            ts_val.and_utc().timestamp_nanos_opt().unwrap()
        );
        assert_eq!(tsc.value(1), 1_700_000_000_000 * 1_000_000);
        assert!(tsc.is_null(2));

        let binc = batch
            .column(5)
            .as_any()
            .downcast_ref::<BinaryArray>()
            .unwrap();
        assert_eq!(binc.value(0), b"hi");
        assert_eq!(binc.value(1), &[0x1A, 0x2B]);
        assert!(binc.is_null(2));

        let sc = batch
            .column(6)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert!(!sc.is_null(2), "row2 的 s 是有的（Hex），不应 null");
        // 结构化字段走 JSON（`serde_json::to_string` 同一套规则）——不硬编 JSON 形状，
        // 但把「必须等于源 Value 的 serde_json 渲染」钉住。
        for (row, field) in [(0usize, "s"), (1usize, "s")] {
            let src = rows[row].field(field).unwrap().get_value();
            let rendered = sc.value(row);
            match serde_json::to_string(src) {
                Ok(json) => assert_eq!(
                    rendered, json,
                    "row{row} 结构化字段应等于源 Value 的 JSON 渲染"
                ),
                Err(e) => panic!(
                    "row{row}: serde_json 渲染失败（{e}）→ 列里实际是 {rendered:?}，src={src:?}"
                ),
            }
            assert!(
                serde_json::from_str::<serde_json::Value>(rendered).is_ok(),
                "结构化字段在 Utf8 列里必须是合法 JSON"
            );
        }
        // row2：Hex 走 Utf8 时是 `{:#X}` 形态（与 `Value::Hex` 的 Display 同形）
        assert_eq!(sc.value(2), "0x1A2B");
        assert_eq!(sc.value(2), format!("{:#X}", 0x1A2Bu128));
    }
}
