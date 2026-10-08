//! A text popup over the main view, such as a read's SAM record.
//!
//! While a popup is open, it blocks all other input. `Esc` closes it.

use gv_core::prelude::*;
use noodles::sam::{self, alignment::RecordBuf, alignment::io::Write as _};

#[derive(Debug, Clone)]
pub struct TextPopup {
    pub title: String,
    /// Rows of a label and a value. Values line up in one column, and the renderer wraps long
    /// values within it.
    pub rows: Vec<(String, String)>,
}

impl TextPopup {
    /// The SAM names of the mandatory fields, in order.
    const SAM_FIELDS: [&'static str; 11] = [
        "QNAME", "FLAG", "RNAME", "POS", "MAPQ", "CIGAR", "RNEXT", "PNEXT", "TLEN", "SEQ", "QUAL",
    ];

    /// Shows a record as its SAM text, one mandatory field per line followed by the optional
    /// fields.
    pub fn read_details(header: &sam::Header, record: &RecordBuf) -> Result<Self, TGVError> {
        let mut writer = sam::io::Writer::new(Vec::new());
        writer.write_alignment_record(header, record)?;
        let text = String::from_utf8(writer.into_inner())?;
        let mut fields = text.trim_end_matches('\n').split('\t');
        let mut rows: Vec<(String, String)> = Self::SAM_FIELDS
            .iter()
            .zip(fields.by_ref())
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        // Optional fields are already `TAG:TYPE:VALUE`, so they need no label.
        rows.extend(fields.map(|field| (String::new(), field.to_string())));
        Ok(Self {
            title: "Read details (Esc to close)".to_string(),
            rows,
        })
    }
}
