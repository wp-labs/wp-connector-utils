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

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::StringArray;
    use arrow::datatypes::{DataType, Field, Schema};
    use arrow::ipc::writer::StreamWriter;
    use std::sync::Arc;

    fn make_ipc_bytes(schema: &Arc<Schema>, values: &[&str]) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut w = StreamWriter::try_new(&mut buf, schema.as_ref()).unwrap();
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(StringArray::from(values.to_vec()))],
        )
        .unwrap();
        w.write(&batch).unwrap();
        w.finish().unwrap();
        buf
    }

    #[test]
    fn ipc_roundtrip() {
        let schema = Arc::new(Schema::new(vec![Field::new("x", DataType::Utf8, false)]));
        let ipc = make_ipc_bytes(&schema, &["hello"]);
        let batches = decode_arrow_ipc_batches(&ipc).unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].num_rows(), 1);
    }

    #[test]
    fn ipc_invalid_returns_err() {
        assert!(decode_arrow_ipc_batches(b"not arrow").is_err());
    }

    #[test]
    fn framed_roundtrip() {
        let schema = Arc::new(Schema::new(vec![Field::new("x", DataType::Utf8, false)]));
        let ipc = make_ipc_bytes(&schema, &["hi"]);
        let tag = b"my_tag";
        let mut frame = Vec::new();
        frame.extend_from_slice(&(tag.len() as u32).to_be_bytes());
        frame.extend_from_slice(tag);
        frame.extend_from_slice(&ipc);
        let batches = decode_arrow_framed_batches(&frame).unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].num_rows(), 1);
    }

    #[test]
    fn framed_too_short_skipped() {
        assert!(decode_arrow_framed_batches(&[0, 0]).unwrap().is_empty());
    }

    #[test]
    fn framed_tag_len_exceeds_skipped() {
        assert!(
            decode_arrow_framed_batches(&[0xff, 0xff, 0xff, 0xff, 0x00])
                .unwrap()
                .is_empty()
        );
    }
}
