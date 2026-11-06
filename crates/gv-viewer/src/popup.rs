//! A text popup over the main view, such as a read's SAM record.
//!
//! While a popup is open, it blocks all other input except scrolling. `Esc` closes it.

use crate::keys::{Key, KeyCode};
use gv_core::prelude::*;
use noodles::sam::{
    self,
    alignment::{
        RecordBuf,
        io::Write as _,
        record::cigar::op::Kind,
        record_buf::data::field::{Value, value::Array},
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextPopup {
    pub title: String,
    /// Rows of a label and a value. Values line up in one column, and the renderer wraps long
    /// values within it.
    pub rows: Vec<(String, String)>,
    /// The first wrapped line shown. Wrapping depends on the front end, so the front end clamps
    /// this to its own line count.
    pub scroll: usize,
}

/// How far a key or the wheel scrolls a popup.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PopupScroll {
    Lines(isize),
    HalfPages(isize),
    Pages(isize),
    Top,
    Bottom,
}

impl PopupScroll {
    /// The scroll a key asks for, with `less`'s keys.
    pub fn from_key(key: Key) -> Option<Self> {
        Some(match (key.code, key.modifiers.control) {
            (KeyCode::Char('j') | KeyCode::Down, false) => Self::Lines(1),
            (KeyCode::Char('k') | KeyCode::Up, false) => Self::Lines(-1),
            (KeyCode::Char('d'), true) => Self::HalfPages(1),
            (KeyCode::Char('u'), true) => Self::HalfPages(-1),
            (KeyCode::Char('f'), true) | (KeyCode::Char(' ') | KeyCode::PageDown, false) => {
                Self::Pages(1)
            }
            (KeyCode::Char('b'), true) | (KeyCode::PageUp, false) => Self::Pages(-1),
            (KeyCode::Char('g') | KeyCode::Home, false) => Self::Top,
            (KeyCode::Char('G') | KeyCode::End, false) => Self::Bottom,
            _ => return None,
        })
    }
}

impl TextPopup {
    /// Scrolls the popup, which shows `visible` of its `lines` wrapped lines at a time. Returns
    /// whether the shown lines changed.
    pub fn scroll_by(&mut self, scroll: PopupScroll, visible: usize, lines: usize) -> bool {
        let max = lines.saturating_sub(visible);
        let current = self.scroll.min(max);
        let page = visible.max(1) as isize;
        self.scroll = match scroll {
            PopupScroll::Lines(lines) => current.saturating_add_signed(lines),
            PopupScroll::HalfPages(pages) => {
                current.saturating_add_signed(pages * (page / 2).max(1))
            }
            PopupScroll::Pages(pages) => current.saturating_add_signed(pages * page),
            PopupScroll::Top => 0,
            PopupScroll::Bottom => max,
        }
        .min(max);
        self.scroll != current
    }

    /// The SAM names of the mandatory fields, in order.
    const SAM_FIELDS: [&'static str; 11] = [
        "QNAME", "FLAG", "RNAME", "POS", "MAPQ", "CIGAR", "RNEXT", "PNEXT", "TLEN", "SEQ", "QUAL",
    ];

    /// Shows a record as its SAM text, one mandatory field per line followed by the optional
    /// fields.
    ///
    /// When `position` is a base the read covers, arrays with one value per read base show only
    /// the value at that base, as IGV does.
    pub fn read_details(
        header: &sam::Header,
        record: &RecordBuf,
        position: Option<u64>,
    ) -> Result<Self, TGVError> {
        let mut writer = sam::io::Writer::new(Vec::new());
        writer.write_alignment_record(header, record)?;
        let text = String::from_utf8(writer.into_inner())?;
        let mut fields = text.trim_end_matches('\n').split('\t');
        let mut rows: Vec<(String, String)> = Self::SAM_FIELDS
            .iter()
            .zip(fields.by_ref())
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();

        let read_length = record.sequence().len();
        let offset = position.and_then(|position| Self::query_offset(record, position));
        // The writer emits optional fields in data order, so each text field pairs with its
        // value. Optional fields are already `TAG:TYPE:VALUE`, so they need no label.
        rows.extend(
            fields
                .zip(record.data().iter())
                .map(|(field, (_, value))| match (value, offset) {
                    (Value::Array(array), Some(offset)) => {
                        let (length, value) = match array {
                            Array::Int8(values) => {
                                (values.len(), values.get(offset).map(i8::to_string))
                            }
                            Array::UInt8(values) => {
                                (values.len(), values.get(offset).map(u8::to_string))
                            }
                            Array::Int16(values) => {
                                (values.len(), values.get(offset).map(i16::to_string))
                            }
                            Array::UInt16(values) => {
                                (values.len(), values.get(offset).map(u16::to_string))
                            }
                            Array::Int32(values) => {
                                (values.len(), values.get(offset).map(i32::to_string))
                            }
                            Array::UInt32(values) => {
                                (values.len(), values.get(offset).map(u32::to_string))
                            }
                            Array::Float(values) => {
                                (values.len(), values.get(offset).map(f32::to_string))
                            }
                        };
                        match value {
                            // `TAG:B:T` precedes the values. A colon, not SAM's comma, marks the
                            // value as one base's rather than a one-value array.
                            Some(value) if length == read_length => {
                                format!("{}:{value}", &field[..6])
                            }
                            _ => field.to_string(),
                        }
                    }
                    _ => field.to_string(),
                })
                .map(|field| (String::new(), field)),
        );

        let locus = match position {
            Some(position) => {
                let contig = &rows[2].1;
                match offset {
                    Some(offset) => format!(
                        " at {contig}:{position}, read base {} of {read_length}",
                        offset + 1
                    ),
                    None => format!(" at {contig}:{position}, no read base"),
                }
            }
            None => String::new(),
        };
        Ok(Self {
            title: format!("Read details{locus} (Esc to close, j/k to scroll)"),
            rows,
            scroll: 0,
        })
    }

    /// The zero-based offset into SEQ of the read base aligned to a 1-based reference
    /// `position`, or `None` if the read has no base there. Soft clips count toward the offset,
    /// as they are part of SEQ.
    fn query_offset(record: &RecordBuf, position: u64) -> Option<usize> {
        let mut reference = record.alignment_start()?.get() as u64;
        let mut query = 0;
        for op in record.cigar().as_ref() {
            let length = op.len();
            match op.kind() {
                Kind::Match | Kind::SequenceMatch | Kind::SequenceMismatch => {
                    if (reference..reference + length as u64).contains(&position) {
                        return Some(query + (position - reference) as usize);
                    }
                    reference += length as u64;
                    query += length;
                }
                Kind::Insertion | Kind::SoftClip => query += length,
                Kind::Deletion | Kind::Skip => {
                    if (reference..reference + length as u64).contains(&position) {
                        return None;
                    }
                    reference += length as u64;
                }
                Kind::HardClip | Kind::Pad => {}
            }
        }
        None
    }
}
