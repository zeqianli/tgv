use noodles::sam::{self, alignment::RecordBuf, alignment::io::Write as _};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextPopup {
    pub title: String,
    pub rows: Vec<(String, String)>,
}

impl TextPopup {
    const SAM_FIELDS: [&'static str; 11] = [
        "QNAME", "FLAG", "RNAME", "POS", "MAPQ", "CIGAR", "RNEXT", "PNEXT", "TLEN", "SEQ", "QUAL",
    ];

    pub fn read_details(header: &sam::Header, record: &RecordBuf) -> Result<Self, crate::Error> {
        let mut writer = sam::io::Writer::new(Vec::new());
        writer.write_alignment_record(header, record)?;
        let text = String::from_utf8(writer.into_inner())?;
        let mut fields = text.trim_end_matches('\n').split('\t');
        let mut rows: Vec<(String, String)> = Self::SAM_FIELDS
            .iter()
            .zip(fields.by_ref())
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        rows.extend(fields.map(|field| (String::new(), field.to_string())));
        Ok(Self {
            title: "Read details (Esc to close)".to_string(),
            rows,
        })
    }
}
