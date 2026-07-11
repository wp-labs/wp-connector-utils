//! Wire format selection for Arrow IPC payloads.
//!
//! - [`WireFormat`] — enum + `parse_strict` / `from_data_format` / `is_arrow`


// ---------------------------------------------------------------------------
// WireFormat
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WireFormat {
    #[default]
    Ndjson,
    ArrowStream,
    ArrowFramed,
}

pub const SUPPORTED_DATA_FORMATS: &[&str] = &["ndjson", "arrow_ipc", "arrow_framed"];

impl WireFormat {
    pub fn from_data_format(value: Option<&str>) -> Self {
        match value.unwrap_or("ndjson") {
            "arrow_framed" => WireFormat::ArrowFramed,
            "arrow_ipc" => WireFormat::ArrowStream,
            _ => WireFormat::Ndjson,
        }
    }

    pub fn parse_strict(value: Option<&str>) -> Result<Self, String> {
        match value {
            None | Some("ndjson") => Ok(WireFormat::Ndjson),
            Some("arrow_ipc") => Ok(WireFormat::ArrowStream),
            Some("arrow_framed") => Ok(WireFormat::ArrowFramed),
            Some(raw) => Err(format!(
                "data_format must be one of: {} (got '{raw}')",
                SUPPORTED_DATA_FORMATS.join(", ")
            )),
        }
    }

    pub fn is_arrow(self) -> bool {
        matches!(self, WireFormat::ArrowStream | WireFormat::ArrowFramed)
    }
}
