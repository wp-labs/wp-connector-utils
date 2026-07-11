//! Arrow IPC decoding (source direction).
//!
//! - [`decode_arrow_ipc_batches`] — Arrow IPC Stream → `Vec<RecordBatch>`
//! - [`decode_arrow_framed_batches`] — wp_arrow frame → `Vec<RecordBatch>`

use arrow::ipc::reader::StreamReader;
use arrow::record_batch::RecordBatch;

// ---------------------------------------------------------------------------
// Decode (source direction)
// ---------------------------------------------------------------------------

pub fn decode_arrow_ipc_batches(payload: &[u8]) -> Result<Vec<RecordBatch>, String> {
    let cursor = std::io::Cursor::new(payload.to_vec());
    let reader = StreamReader::try_new(cursor, None).map_err(|e| format!("arrow ipc: {e}"))?;
    let mut batches = Vec::new();
    for batch in reader {
        let batch = batch.map_err(|e| format!("arrow ipc batch: {e}"))?;
        batches.push(batch);
    }
    Ok(batches)
}

pub fn decode_arrow_framed_batches(payload: &[u8]) -> Result<Vec<RecordBatch>, String> {
    if payload.len() < 4 {
        return Ok(Vec::new());
    }
    let tag_len = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize;
    let ipc_start = 4 + tag_len;
    if ipc_start > payload.len() {
        return Ok(Vec::new());
    }
    decode_arrow_ipc_batches(&payload[ipc_start..])
}
