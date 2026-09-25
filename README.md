# wp-connector-utils

[![Crates.io](https://img.shields.io/crates/v/wp-connector-utils.svg)](https://crates.io/crates/wp-connector-utils)
[![Docs.rs](https://docs.rs/wp-connector-utils/badge.svg)](https://docs.rs/wp-connector-utils)
[![CI](https://img.shields.io/github/actions/workflow/status/wp-labs/wp-connector-utils/ci.yml?branch=main)](https://github.com/wp-labs/wp-connector-utils/actions/workflows/ci.yml)
[![Crates.io downloads](https://img.shields.io/crates/d/wp-connector-utils)](https://crates.io/crates/wp-connector-utils)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Rust Edition](https://img.shields.io/badge/edition-2024-orange.svg)](https://doc.rust-lang.org/edition-guide/rust-2024/index.html)

Shared utilities for **WarpParse connector implementations** — Arrow IPC encode/decode,
wire-format handling, NDJSON conversion, and batch metadata processing. Used by
[`wp-core-connectors`](https://crates.io/crates/wp-core-connectors) and
[`wp-connectors`](https://crates.io/crates/wp-connectors).

## Quick Start

```bash
cargo add wp-connector-utils
```

```rust
use wp_connector_utils::ndjson::ndjson_to_record_batch;

let lines = vec![
    r#"{"name":"alice","count":42}"#.to_string(),
    r#"{"name":"bob","count":7}"#.to_string(),
];
let batch = ndjson_to_record_batch(&lines, &schema)?.expect("non-empty input");
```

## Features

- **Arrow IPC 59** — Full Arrow IPC Stream encode/decode (schema + batches + EOS).
- **`wp_arrow` framing** — `[tag_len][tag][IPC]` frame format for per-stream routing.
- **WireFormat** — Centralised `data_format` enum with strict & lenient parsing.
- **Typed RecordBatch** — `DataRecord` → `RecordBatch` (Bool / Int32 / Int64 / Float64 /
  Timestamp / Binary / Utf8).
- **NDJSON** — Line-oriented JSON → columnar Arrow, incl. numeric epoch timestamps
  (s / ms / us / ns) and `true/false`/`1/0` boolean text.
- **Batch metadata** — OML-name injection for text sinks, frame-tag resolution for
  Arrow sinks.
- **Codec** — sink-side compression (`gzip`/`zstd`) + encryption (`AES-256-GCM`/`SM4-GCM`),
  chainable (compress-then-encrypt) with a self-describing frame header and GCM-authenticated
  frames. See [performance benchmarks](BENCHMARKS.md).

## Modules

| Module | Purpose |
| --- | --- |
| [`arrow`](src/arrow/) | IPC encode/decode, WireFormat, schema inference, `DataRecord → RecordBatch` |
| [`codec`](src/codec.rs) | Compression (gzip/zstd) + encryption (AES-256-GCM/SM4-GCM) encode/decode |
| [`ndjson`](src/ndjson.rs) | NDJSON lines → Arrow `RecordBatch` against a known schema |
| [`batch`](src/batch.rs) | Batch metadata helpers (OML name injection, frame-tag resolution) |

### `arrow` — IPC + WireFormat

```rust
use wp_connector_utils::arrow::WireFormat;

// Strict parsing for factory validation.
let fmt = WireFormat::parse_strict(Some("arrow_framed"))?;

// Lenient parsing (unknown values fall back to Ndjson).
let fmt = WireFormat::from_data_format(Some("arrow_ipc"));

// Is this an Arrow binary format?
assert!(WireFormat::ArrowFramed.is_arrow());
```

```rust
use wp_connector_utils::arrow::{encode_batch_ipc_stream, encode_ipc_frame};

// Bare Arrow IPC stream (sink direction).
let ipc_bytes = encode_batch_ipc_stream(&batch)?;

// wp_arrow frame: [4B tag_len][tag][Arrow IPC stream].
let framed_bytes = encode_ipc_frame("nginx_access", &batch)?;
```

```rust
use wp_connector_utils::arrow::{decode_arrow_ipc_batches, decode_arrow_framed_batches};

// Decode a raw Arrow IPC stream (source direction).
let batches: Vec<RecordBatch> = decode_arrow_ipc_batches(&raw_bytes)?;

// Decode wp_arrow frames (skips the tag header).
let batches: Vec<RecordBatch> = decode_arrow_framed_batches(&framed_bytes)?;
```

```rust
use wp_connector_utils::arrow::{
    infer_schema_from_record, data_record_to_batch, data_records_to_batch,
};

// Infer an Arrow schema from typed DataRecord fields.
let schema = Arc::new(infer_schema_from_record(&record));

// Convert a single record or a batch of records.
let batch = data_record_to_batch(&record, &schema)?;
let batch = data_records_to_batch(&records, &schema)?;
```

### `batch` — metadata helpers

```rust
use wp_connector_utils::batch::{inject_oml_name, resolve_frame_tag};
use wp_connector_api::BatchMeta;

// Inject the wp_oml_name field into each DataRecord (JSON / CSV / syslog sinks).
let meta = BatchMeta::with_oml_name("nginx_access");
let records = inject_oml_name(&meta, data);

// Resolve the Arrow frame tag: meta.oml_name takes precedence over the config tag.
let tag = resolve_frame_tag(&meta, "default_tag");
```

### `codec` — compression + encryption

```rust
use wp_connector_utils::codec::{
    build_decoder, build_encoder, Cipher, CompressConfig, CompressionAlgo, EncryptConfig,
};

// Sink side: compress-then-encrypt.
let compress = CompressConfig { algo: CompressionAlgo::Zstd, level: 3 };
let encrypt = EncryptConfig { cipher: Cipher::Aes256Gcm, key: vec![0u8; 32] };
let mut encoder = build_encoder(Some(&compress), Some(&encrypt))?;

let mut wire = Vec::new();
encoder.encode(b"log line\n", &mut wire)?;
encoder.finish(&mut wire)?; // flush the compression tail block

// Source side: decrypt-then-decompress (reverse order).
let mut decoder = build_decoder(Some(&compress), Some(&encrypt))?;
let mut plain = Vec::new();
decoder.decode(&wire, &mut plain)?;
decoder.finish(&mut plain)?;
```

> **Performance** — `zstd` is the preferred compression default (faster *and* smaller than
> `gzip` on log-like text). `AES-256-GCM` is hardware-accelerated (~5 GiB/s), while `SM4-GCM`
> is a pure-software implementation (~75 MiB/s, ~70× slower). See
> [BENCHMARKS.md](BENCHMARKS.md) for full data and conclusions.

### `ndjson` — JSON lines → Arrow

```rust
use wp_connector_utils::ndjson::ndjson_to_record_batch;
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};

let schema = Schema::new(vec![
    Field::new("name", DataType::Utf8, true),
    Field::new("ts", DataType::Timestamp(TimeUnit::Nanosecond, None), true),
]);

let lines = vec![
    r#"{"name":"alice","ts":1643163078468}"#.to_string(), // ms epoch
];
let batch = ndjson_to_record_batch(&lines, &schema)?.expect("non-empty input");
```

## License

Licensed under the [Apache License, Version 2.0](LICENSE).

Unless required by applicable law or agreed to in writing, software distributed
under the License is distributed on an **"AS IS" BASIS, WITHOUT WARRANTIES OR
CONDITIONS OF ANY KIND**, either express or implied. See the License for the
specific language governing permissions and limitations under the License.
