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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_ndjson() {
        assert_eq!(WireFormat::from_data_format(None), WireFormat::Ndjson);
        assert_eq!(WireFormat::default(), WireFormat::Ndjson);
    }
    #[test]
    fn parses_arrow() {
        assert_eq!(
            WireFormat::from_data_format(Some("arrow_ipc")),
            WireFormat::ArrowStream
        );
        assert_eq!(
            WireFormat::from_data_format(Some("arrow_framed")),
            WireFormat::ArrowFramed
        );
    }
    #[test]
    fn unknown_falls_back() {
        assert_eq!(WireFormat::from_data_format(Some("x")), WireFormat::Ndjson);
    }
    #[test]
    fn parse_strict_rejects_unknown() {
        assert!(WireFormat::parse_strict(Some("x")).is_err());
    }
    #[test]
    fn parse_strict_accepts_known() {
        assert_eq!(WireFormat::parse_strict(None).unwrap(), WireFormat::Ndjson);
        assert_eq!(
            WireFormat::parse_strict(Some("ndjson")).unwrap(),
            WireFormat::Ndjson
        );
        assert_eq!(
            WireFormat::parse_strict(Some("arrow_ipc")).unwrap(),
            WireFormat::ArrowStream
        );
        assert_eq!(
            WireFormat::parse_strict(Some("arrow_framed")).unwrap(),
            WireFormat::ArrowFramed
        );
    }
    #[test]
    fn is_arrow_flag() {
        assert!(!WireFormat::Ndjson.is_arrow());
        assert!(WireFormat::ArrowStream.is_arrow());
        assert!(WireFormat::ArrowFramed.is_arrow());
    }
}
