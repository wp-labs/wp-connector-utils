//! Arrow IPC encoding (sink direction).
//!
//! - [`encode_batch_ipc_stream`] — bare Arrow IPC Stream bytes
//! - [`encode_ipc_frame`] — wp_arrow frame `[tag_len][tag][IPC]`
//! - [`encode_ipc_frame_multi`] — multiple batches in one frame

use arrow::ipc::writer::StreamWriter;
use arrow::record_batch::RecordBatch;
use orion_error::conversion::SourceRawErr;
use wp_connector_api::{SinkReason, SinkResult};

// ---------------------------------------------------------------------------
// Encode (sink direction)
// ---------------------------------------------------------------------------

pub fn encode_batch_ipc_stream(batch: &RecordBatch) -> SinkResult<Vec<u8>> {
    let schema = batch.schema();
    let mut buf = Vec::new();
    {
        let mut writer = StreamWriter::try_new(&mut buf, &schema)
            .source_raw_err(SinkReason::Sink, "arrow create stream writer")?;
        writer
            .write(batch)
            .source_raw_err(SinkReason::Sink, "arrow encode batch")?;
        writer
            .finish()
            .source_raw_err(SinkReason::Sink, "arrow finish stream")?;
    }
    Ok(buf)
}

pub fn encode_ipc_frame(tag: &str, batch: &RecordBatch) -> SinkResult<Vec<u8>> {
    let tag_bytes = tag.as_bytes();
    let schema = batch.schema();
    let mut buf = Vec::with_capacity(4 + tag_bytes.len() + 1024);
    buf.extend_from_slice(&(tag_bytes.len() as u32).to_be_bytes());
    buf.extend_from_slice(tag_bytes);
    {
        let mut writer = StreamWriter::try_new(&mut buf, &schema)
            .source_raw_err(SinkReason::Sink, "arrow create framed writer")?;
        writer
            .write(batch)
            .source_raw_err(SinkReason::Sink, "arrow encode framed")?;
        writer
            .finish()
            .source_raw_err(SinkReason::Sink, "arrow finish framed")?;
    }
    Ok(buf)
}

pub fn encode_ipc_frame_multi(tag: &str, batches: &[RecordBatch]) -> SinkResult<Vec<u8>> {
    if batches.is_empty() {
        let tag_bytes = tag.as_bytes();
        let mut buf = Vec::with_capacity(4 + tag_bytes.len());
        buf.extend_from_slice(&(tag_bytes.len() as u32).to_be_bytes());
        buf.extend_from_slice(tag_bytes);
        return Ok(buf);
    }
    let tag_bytes = tag.as_bytes();
    let schema = batches[0].schema();
    let mut buf = Vec::with_capacity(4 + tag_bytes.len() + 1024);
    buf.extend_from_slice(&(tag_bytes.len() as u32).to_be_bytes());
    buf.extend_from_slice(tag_bytes);
    {
        let mut writer = StreamWriter::try_new(&mut buf, &schema)
            .source_raw_err(SinkReason::Sink, "arrow create framed multi writer")?;
        for batch in batches {
            writer
                .write(batch)
                .source_raw_err(SinkReason::Sink, "arrow encode framed multi")?;
        }
        writer
            .finish()
            .source_raw_err(SinkReason::Sink, "arrow finish framed multi")?;
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::StringArray;
    use arrow::datatypes::{DataType, Field, Schema};
    use std::sync::Arc;

    fn make_schema() -> Arc<Schema> {
        Arc::new(Schema::new(vec![Field::new("x", DataType::Utf8, false)]))
    }

    fn make_batch(schema: &Arc<Schema>, values: Vec<&str>) -> RecordBatch {
        RecordBatch::try_new(schema.clone(), vec![Arc::new(StringArray::from(values))]).unwrap()
    }

    #[test]
    fn ipc_roundtrip() {
        let s = make_schema();
        let b = make_batch(&s, vec!["hi"]);
        let ipc = encode_batch_ipc_stream(&b).unwrap();
        // Arrow IPC Stream starts with 0xFFFFFFFF (continuation marker)
        assert!(ipc.len() > 8);
        assert_eq!(&ipc[0..4], &[0xff, 0xff, 0xff, 0xff]);
    }

    #[test]
    fn framed_roundtrip() {
        let s = make_schema();
        let b = make_batch(&s, vec!["hi"]);
        let frame = encode_ipc_frame("my_tag", &b).unwrap();
        // tag_len = 6, tag = "my_tag"
        assert_eq!(&frame[0..4], &6u32.to_be_bytes());
        assert_eq!(&frame[4..10], b"my_tag");
    }

    #[test]
    fn ipc_frame_multi_roundtrip() {
        let s = make_schema();
        let b1 = make_batch(&s, vec!["a"]);
        let b2 = make_batch(&s, vec!["b", "c"]);
        let frame = encode_ipc_frame_multi("multi", &[b1, b2]).unwrap();
        assert_eq!(&frame[0..4], &5u32.to_be_bytes());
        assert_eq!(&frame[4..9], b"multi");
    }

    #[test]
    fn ipc_frame_multi_empty() {
        let frame = encode_ipc_frame_multi("tag", &[]).unwrap();
        assert_eq!(&frame[0..4], &3u32.to_be_bytes());
        assert_eq!(&frame[4..7], b"tag");
    }
}
