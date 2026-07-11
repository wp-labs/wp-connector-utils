# wp-connector-utils

Shared utilities for WP connector implementations — Arrow IPC encode/decode, WireFormat, NDJSON conversion, and batch metadata processing.

## Modules

### `arrow` — Arrow IPC Utilities

Arrow IPC encode/decode and data conversion helpers, shared across `wp-core-connectors` and `wp-connectors`.

```
arrow/
├── format.rs    # WireFormat enum + parse_strict / from_data_format / is_arrow
├── encode.rs    # encode_batch_ipc_stream / encode_ipc_frame / encode_ipc_frame_multi
├── decode.rs    # decode_arrow_ipc_batches / decode_arrow_framed_batches
├── schema.rs    # infer_arrow_schema / infer_schema_from_record
└── record.rs    # data_record_to_batch / data_records_to_batch + typed column builders
```

#### WireFormat

```rust
use wp_connector_utils::arrow::WireFormat;

// Strict parsing for factory validation
let fmt = WireFormat::parse_strict(Some("arrow_framed"))?;

// Lenient parsing (unknown → Ndjson)
let fmt = WireFormat::from_data_format(Some("arrow_ipc"));

// Is this an Arrow binary format?
assert!(WireFormat::ArrowFramed.is_arrow());
```

#### Encoding (Sink Direction)

```rust
use wp_connector_utils::arrow::{encode_batch_ipc_stream, encode_ipc_frame};

// Bare Arrow IPC Stream
let ipc_bytes = encode_batch_ipc_stream(&batch)?;

// wp_arrow frame: [4B tag_len][tag][Arrow IPC Stream]
let framed_bytes = encode_ipc_frame("nginx_access", &batch)?;
```

#### Decoding (Source Direction)

```rust
use wp_connector_utils::arrow::{decode_arrow_ipc_batches, decode_arrow_framed_batches};

// Decode raw Arrow IPC Stream bytes
let batches: Vec<RecordBatch> = decode_arrow_ipc_batches(&raw_bytes)?;

// Decode wp_arrow frame (skips tag header)
let batches: Vec<RecordBatch> = decode_arrow_framed_batches(&framed_bytes)?;
```

#### DataRecord → RecordBatch

```rust
use wp_connector_utils::arrow::{
    infer_schema_from_record, data_record_to_batch, data_records_to_batch,
};

// Infer schema from typed DataRecord fields
let schema = Arc::new(infer_schema_from_record(&record));

// Convert single record
let batch = data_record_to_batch(&record, &schema)?;

// Convert batch of records
let batch = data_records_to_batch(&records, &schema)?;
```

### `batch` — Batch Metadata Processing

```rust
use wp_connector_utils::batch::{inject_oml_name, resolve_frame_tag};
use wp_connector_api::BatchMeta;

// Inject wp_oml_name field into each DataRecord (text sinks: JSON, CSV, syslog)
let meta = BatchMeta::with_oml_name("nginx_access");
let records = inject_oml_name(&meta, data);

// Resolve Arrow frame tag: meta.oml_name > connector config tag
let tag = resolve_frame_tag(&meta, "default_tag");
```

### `ndjson` — NDJSON Conversion

```rust
use wp_connector_utils::ndjson::ndjson_to_record_batch;

let lines = vec![
    r#"{"name":"alice","count":42}"#.to_string(),
    r#"{"name":"bob","count":7}"#.to_string(),
];
let batch = ndjson_to_record_batch(&lines, &schema)?
    .expect("non-empty input");
```

## Features

- **Arrow IPC v59** — Full Arrow IPC Stream encode/decode (schema + batches + EOS)
- **wp_arrow frame** — Custom frame format `[tag_len][tag][IPC]` for stream routing
- **WireFormat** — Centralised `data_format` enum with strict & lenient parsing
- **Typed RecordBatch** — `DataRecord` → `RecordBatch` with Bool/Int32/Int64/Float64/Timestamp/Binary/Utf8
- **NDJSON** — Line-oriented JSON to Arrow columnar conversion
- **Batch metadata** — OML name injection for text sinks, frame tag resolution for Arrow sinks

## License

Apache-2.0
