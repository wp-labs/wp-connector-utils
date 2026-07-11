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
