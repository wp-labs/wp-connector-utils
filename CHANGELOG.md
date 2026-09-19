# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2026-09-19

### ⚠️ BREAKING CHANGES

- 依赖 `arrow` 59 → 60。arrow 类型在本 crate 的公开 API 里（`encode_batch_ipc_stream(&RecordBatch)`、`decode_arrow_ipc_batches(...) -> Result<Vec<RecordBatch>, String>`、`infer_arrow_schema(...) -> Schema`、`data_record_to_batch(...) -> RecordBatch` 等），而 Cargo 把「公开 API 中出现的依赖大版本」视为公开 API 的一部分 —— 消费方必须**同一次**一起升到 arrow 60；依赖图里同时存在 59 与 60 时会出现两个不同的 `RecordBatch` / `Schema` 类型，编译期直接失败。
- 依赖 `wp-model-core` 0.9 → 0.10：上游把整数类型正名（`Value::Digit` → `Value::Int`、`DataType::Digit` → `DataType::Int`、`from_digit` → `from_int`、serde 名 `"digit"` → `"int"`）。本 crate 的 `arrow::schema` / `arrow::record` / `batch` 共 7 处随之改名；`data_record_to_batch` 等签名里的 `DataRecord` 就是 `wp-model-core` 的类型，故对消费方同样是破坏性的。
- 依赖 `wp-connector-api` 0.12 → 0.13（该版本对齐 `wp-model-core` 0.10 与 `wp-source-types` 0.3，两者需同升）。

### Dependencies

- `arrow`：`59` → `60`
- `wp-model-core`：`0.9` → `0.10`
- `wp-connector-api`：`0.12` → `0.13`

## [0.2.1] - 2026-09-09

### Fixed

- **NDJSON 数字时间戳转 null（warp-fusion#95）**（另补同类：Boolean 文本 `1/0`/大小写/空白与文件输入一致）：`ndjson_to_record_batch` 的 `Timestamp(Nanosecond)` 列此前只识别 RFC3339 字符串；现在支持 JSON 数字时间戳（秒/毫秒/微秒/纳秒按位宽归一化，含浮点）与数字字符串、`%Y-%m-%d %H:%M:%S` 字符串——与运行时文件输入的 NDJSON 时间语义一致。

## [0.2.0] - 2026-08-04

### ⚠️ BREAKING CHANGES

- 依赖 `wp-model-core` 0.8 → 0.9（上游新增 `Value::BigUint` / `DataType::BigInt` 变体），版本升至 0.2.0

### Added

- `arrow::schema` 映射 `DataType::BigInt` → `DataType::Utf8`：任意精度整数（`Value::BigUint`）字段以十进制字符串输出，与 `format_utf8_value` 的 `to_string()` 路径一致

### Tests

- 新增 `bigint_maps_to_utf8` 断言

## [0.1.2] - 2026-07-12

### Added

- `batch::inject_oml_name` / `batch::resolve_frame_tag` 支持 `BatchMeta::output_disabled`：禁用时跳过字段注入 / 回退到 connector 配置 tag
- arrow 模块拆分：`arrow/{format,encode,decode,schema,record}.rs`，每文件 < 300 行
- 补回 arrow 模块 29 个测试用例（WireFormat / encode / decode / schema / record）
- `Cargo.toml` 补全 crates.io 发布元数据

## [0.1.0] - 2026-07-12

### Added

- 初始发布：Arrow IPC 编解码、WireFormat、NDJSON 转换、BatchMeta 处理
- `arrow` 模块：`encode_batch_ipc_stream` / `encode_ipc_frame` / `encode_ipc_frame_multi` / `decode_arrow_ipc_batches` / `decode_arrow_framed_batches` / `WireFormat` / `infer_arrow_schema` / `infer_schema_from_record` / `data_record_to_batch` / `data_records_to_batch`
- `batch` 模块：`inject_oml_name` / `resolve_frame_tag`
- `ndjson` 模块：`ndjson_to_record_batch`
- 测试覆盖：45 个（arrow 29 + batch 8 + ndjson 8）

[Unreleased]: https://github.com/wp-labs/wp-connector-utils/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/wp-labs/wp-connector-utils/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/wp-labs/wp-connector-utils/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/wp-labs/wp-connector-utils/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/wp-labs/wp-connector-utils/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/wp-labs/wp-connector-utils/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/wp-labs/wp-connector-utils/releases/tag/v0.1.0
