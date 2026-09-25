# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.2] - 2026-09-25

### Added

- 新增 `codec` 性能基准（`benches/codec.rs`，criterion）：压缩（gzip/zstd × 级别）、加密（AES-256-GCM/SM4-GCM）、组合（先压缩后加密）与解码吞吐，及启动时的压缩比打印；dev-dependency 增加 `criterion`。

### Documentation

- 新增 `BENCHMARKS.md`：codec 性能数据与结论（SM4-GCM 慢 AES-GCM 约 70×；zstd 优于 gzip；压缩对日志文本收益大、对高熵数据无效；帧开销 36 B/块）。
- README 补 `codec` 模块说明与性能提示，链接到 `BENCHMARKS.md`。

## [0.4.1] - 2026-09-25

### Fixed

- **`Encoder` / `Decoder` 补齐 `Sync` 上界**：codec 会被持有在 `Send + Sync` 的 sink 结构体里（如 `wp-core-connectors` 的 `NetWriter`），而 0.4.0 的 trait 只要求 `Send`，导致 `Box<dyn Encoder>` 无法被跨线程共享、集成方编译失败。现在 `Encoder` / `Decoder`（及内部 `AeadCipher`）统一要求 `Send + Sync`；所有内建实现（`Vec<u8>` / 函数指针 / 计数器）均满足，行为不变。

## [0.4.0] - 2026-09-23

### Added

- **新增 `codec` 模块（sink 压缩 + 加密，字节级、同步、无 tokio）**：提供 `Encoder` / `Decoder` trait 与
  `build_encoder` / `build_decoder`。压缩支持 `gzip` / `zstd`（按 64 KiB 块流式分块）；加密支持 `AES-256-GCM` /
  `SM4-GCM`（国密，每块独立随机 nonce，输出 `nonce(12) || ciphertext+tag`）；链式组合 **先压缩后加密**。
  sink 侧用 `build_encoder`，source 侧用 `build_decoder`，两端共享同一线格式。
- **自描述帧头 + 帧认证**：每个块为 `magic("WP") | version | kind | length | payload`，`kind` 标识 gzip/zstd/aes-256-gcm/sm4-gcm，
  字节流自解释（拿到即可判断是否密文、用哪个算法）；帧的 `kind` 作为 **AAD** 纳入 GCM 认证（换 `kind` 解不开），
  decoder 校验 kind 不匹配即拒绝。
- **可观测统计**：`Encoder::stats()` 返回 `CodecStats { nonces_issued, sealed_bytes }`，`nonces_issued > 0` 即证明
  数据确实经过加密层。
- **金标准测试向量**：钉死 NIST AES-256-GCM 零向量与 Bouncy Castle SM4-GCM 向量，逐字节断言 `seal` 输出与官方标准一致；
  另含密文性质断言（明文不可见、密文不可再压缩）。
- 新增依赖 `flate2`（gzip）、`zstd`（zstd）、`ring`（AES-256-GCM）、`sm4-gcm`（SM4-GCM，国密）。

## [0.3.5] - 2026-09-19

### Changed

- **值层改为转发 `wp-arrow`（A-2 第 3 步 / 2c）**：`arrow::record::{data_record_to_batch, data_records_to_batch}`
  的实现迁到 `wp_arrow::contract::value`，本 crate 只做转发并把 `WpArrowError` 映射回
  `SinkResult`（公开签名不变、错误文案形状不变 → `wp-core-connectors` 的 file/tcp sink 零改动）。
  至此线协议契约的两层（列类型表 + 值编码）都在 `wp-arrow`，本 crate 不再持有任何口径表或编码实现。
- 依赖 `wp-arrow` 要求升到 **`0.4.2`**（`contract::value` 是 0.4.2 新增；按最低可用版本写）。
- 等价性凭据：同一份「金标准」夹具与期望在**迁移前**（本 crate 的实现）与**迁移后**
  （`wp-arrow` 的 `contract::value`）两处各有一份且同时通过 ——
  见 `arrow::record::tests::wire_value_encoding_is_pinned_by_golden_values`
  （实现侧那份在 `wp-arrow`）；本侧那份留作消费侧拼线。

## [0.3.4] - 2026-09-19

### Fixed

- **修正 0.3.3 的依赖要求写法（该版本应被 yank）**：`wp-arrow = "0.4"` 的 caret 语义允许解析到 **0.4.0**，而 `contract` 模块是 **0.4.1** 才有的 —— 于是消费方（如 wp-reactor）`Cargo.lock` 停在 0.4.0 时会直接 `unresolved import wp_arrow::contract`、编不过。现改为 `wp-arrow = "0.4.1"`（`>= 0.4.1, < 0.5.0`）。
  教训：当 B 依赖 C 的**新 API** 时，要求必须写**最低可用版本**，不能写只含 major.minor 的宽松形式。

## [0.3.3] - 2026-09-19

### Changed

- **`arrow::wp_type_to_arrow` 改为再导出（A-2 第 2 步）**：实现迁往 `wp-arrow` 的 `contract::wp_type_to_arrow`，本 crate 只 `pub use` 它。公开路径与签名不变（接收侧 `wf-runtime` 零改动），且这个路径现在**就是**契约实现本身（不是副本）—— 两者在结构上不可能再漂移。
  口径钉桩（全 37 变体）随实现移往 `wp-arrow` 的 `wire_contract_full_mapping_is_pinned`；本 crate 保留消费侧冒烟：「再导出即同一函数」+ DIV-1（`Hex`）/ DIV-2（`BigInt`）/ DIV-3（结构化）与时间/二进制等易错行。
- 新增依赖 `wp-arrow`（它用与本 crate 一致的 `arrow 60` / `wp-model-core 0.10`，依赖图内 `arrow` 仍只有一个版本）。
  > ⚠️ 本条版本的依赖写成 `"0.4"` 是错的，由 **0.3.4** 修正；请用 0.3.4+。

## [0.3.2] - 2026-09-19

### Added

- **公开 `arrow::wp_type_to_arrow`**（`DataType` → Arrow 列类型的映射）。它是 wp-model ↔ Arrow 线协议口径的**唯一实现**，接收侧（`wf-runtime`）现在直接复用它而不是维护自己的第二张表（A-1 收敛）。

## [0.3.1] - 2026-09-19

### Fixed

- **`Hex` 的 Arrow 列类型由 `Binary` 改为 `Utf8`（十六进制字符串）**：`hex` 字段此前被编成原始大端字节（如 `[0x1A, 0x2B]`），而接收侧（`wf-runtime`）与 `wp-arrow` 都是 `Utf8`，导致校验期直接抛 schema mismatch、整列无法送达。`Value::Hex` 的 `Display` 本就是 `{:#X}`（与 `wp-arrow` 同形），因此值层无需改动，`Base64` 仍为 `Binary`。

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

[Unreleased]: https://github.com/wp-labs/wp-connector-utils/compare/v0.3.2...HEAD
[0.3.2]: https://github.com/wp-labs/wp-connector-utils/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/wp-labs/wp-connector-utils/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/wp-labs/wp-connector-utils/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/wp-labs/wp-connector-utils/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/wp-labs/wp-connector-utils/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/wp-labs/wp-connector-utils/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/wp-labs/wp-connector-utils/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/wp-labs/wp-connector-utils/releases/tag/v0.1.0
