# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/wp-labs/wp-connector-utils/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/wp-labs/wp-connector-utils/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/wp-labs/wp-connector-utils/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/wp-labs/wp-connector-utils/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/wp-labs/wp-connector-utils/releases/tag/v0.1.0
