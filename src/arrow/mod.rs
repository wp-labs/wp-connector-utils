//! Arrow IPC encode / decode helpers shared across connector crates.

pub mod decode;
pub mod encode;
pub mod format;
pub mod record;
pub mod schema;

pub use decode::{decode_arrow_framed_batches, decode_arrow_ipc_batches};
pub use encode::{encode_batch_ipc_stream, encode_ipc_frame, encode_ipc_frame_multi};
pub use format::SUPPORTED_DATA_FORMATS;
pub use format::WireFormat;
pub use record::{data_record_to_batch, data_records_to_batch};
pub use schema::{infer_arrow_schema, infer_schema_from_record};
