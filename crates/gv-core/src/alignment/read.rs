use crate::error::TGVError;
use crate::message::AlignmentFilter;
use noodles::sam::{
    self,
    alignment::{RecordBuf, record::cigar::op::Kind},
};

/// A temporary borrowed view for base queries, filtering, and read details.
#[derive(Clone, Debug)]
pub struct AlignedReadRef<'a> {
    /// The original alignment record.
    pub record: &'a RecordBuf,

    /// Non-clipped start genome coordinate on the alignment view
    /// 1-based, inclusive
    pub start: u64,
    /// Non-clipped end genome coordinate on the alignment view
    /// Note that this includes the soft-clipped reads and differ from the built-in methods. TODO
    /// 1-based, inclusive
    pub end: u64,

    /// Leading softclips. Used for track stacking calculation.
    pub leading_softclips: u64,

    /// Trailing softclips. Used for track stacking calculation.
    pub trailing_softclips: u64,
}

impl<'a> AlignedReadRef<'a> {
    pub fn borrowed(record: &'a RecordBuf) -> Result<Self, TGVError> {
        let start = record.alignment_start().ok_or_else(|| {
            TGVError::AlignmentParseError(
                "Alignment record is missing a start position.".to_string(),
            )
        })?;
        let start = start.get() as u64;
        let alignment_span = record.cigar().alignment_span() as u64;
        let end = start.saturating_add(alignment_span.saturating_sub(1));
        let cigars = record.cigar().as_ref();
        let leading_softclips = cigars
            .iter()
            .find(|op| !matches!(op.kind(), Kind::HardClip | Kind::Pad))
            .map_or(0, |op| match op.kind() {
                Kind::SoftClip => op.len() as u64,
                _ => 0,
            });
        let trailing_softclips = cigars
            .iter()
            .rev()
            .find(|op| !matches!(op.kind(), Kind::HardClip | Kind::Pad))
            .map_or(0, |op| match op.kind() {
                Kind::SoftClip => op.len() as u64,
                _ => 0,
            });
        Ok(Self {
            record,
            start,
            end,
            leading_softclips,
            trailing_softclips,
        })
    }

    /// Read details
    pub fn describe(&self) -> Result<String, TGVError> {
        // FIXME: improve display information
        // Example IGV display:
        // Read name = HISEQ1:29:HA2WPADXX:1:1216:5183:9385
        // Read length = 148bp
        // Flags = 147
        // ----------------------
        // Mapping = Primary @ MAPQ 70
        // Reference span = chr20:78,203-78,350 (-) = 148bp
        // Cigar = 148M
        // Clipping = None
        // ----------------------
        // Mate is mapped = yes
        // Mate start = chr20:77619 (+)
        // Insert size = -731
        // Second in pair
        // Pair orientation = F1R2
        // ----------------------
        // PG = novoalign
        // AM = 70
        // NM = 0
        // SM = 70
        // PQ = 5
        // UQ = 0
        // AS = 0
        // Hidden tags: MDLocation = chr20:78,249
        // Base = C @ QV 30
        let read_name = self
            .record
            .name()
            .map(|name| name.to_string())
            .unwrap_or_else(|| "<missing>".to_string());
        let mapping_quality = self
            .record
            .mapping_quality()
            .map(|quality| quality.get().to_string())
            .unwrap_or_else(|| ".".to_string());
        let flags = u16::from(self.record.flags());
        let cigar = cigar_to_string(self.record.cigar())?;

        Ok(format!(
            "{}  Flags={}  MAPQ={}  Cigar={}",
            //String::from_utf8_lossy(&self.record.sequence()[..]),
            read_name,
            flags,
            mapping_quality,
            cigar
        ))
    }

    /// Return the base at coordinate.
    /// None: Not covered, deletion, softclip.
    /// Insertion: the inserted sequences are not returned.
    ///
    /// coordinate: 1-based
    pub fn base_at(&self, coordinate: u64) -> Option<u8> {
        if coordinate < self.start || coordinate > self.end {
            return None;
        }

        let coordinate = coordinate as usize;

        let mut reference_pivot = self.start as usize;
        let mut query_pivot: usize = 1; // 1-based. # bases on the sequence. Note that need to substract leading softclips to get aligned base coordinate.

        for op in self.record.cigar().as_ref() {
            if reference_pivot > coordinate {
                break;
            }

            let kind = op.kind();
            let len = op.len();

            let next_reference_pivot = if kind.consumes_reference() {
                reference_pivot + len
            } else {
                reference_pivot
            };

            let next_query_pivot = if kind.consumes_read() {
                query_pivot + len
            } else {
                query_pivot
            };

            if next_reference_pivot <= coordinate {
                reference_pivot = next_reference_pivot;
                query_pivot = next_query_pivot;
                continue;
            }

            match kind {
                Kind::SoftClip | Kind::Insertion | Kind::HardClip | Kind::Pad => {
                    // This should never reach
                    reference_pivot = next_reference_pivot;
                    query_pivot = next_query_pivot;
                }

                Kind::Deletion | Kind::Skip => {
                    return None;
                }

                Kind::SequenceMismatch | Kind::SequenceMatch | Kind::Match => {
                    return self
                        .record
                        .sequence()
                        .get(query_pivot + coordinate - reference_pivot - 1);
                }
            }
        }
        None
    }

    pub fn is_softclip_at(&self, coordinate: u64) -> bool {
        if coordinate < self.start && coordinate + self.leading_softclips >= self.start {
            return true;
        }
        if coordinate > self.end && coordinate <= self.end + self.trailing_softclips {
            return true;
        }
        false
    }

    pub fn is_deletion_at(&self, coordinate: u64) -> bool {
        if coordinate < self.start || coordinate > self.end {
            return false;
        }

        let coordinate = coordinate as usize;
        let mut reference_pivot: usize = self.start as usize;
        let mut query_pivot: usize = 1; // 1-based. # bases on the sequence. Note that need to substract leading softclips to get aligned base coordinate.

        for op in self.record.cigar().as_ref().iter() {
            if reference_pivot > coordinate {
                break;
            }
            let kind = op.kind();

            let next_reference_pivot = if kind.consumes_reference() {
                reference_pivot + op.len()
            } else {
                reference_pivot
            };

            let next_query_pivot = if kind.consumes_read() {
                query_pivot + op.len()
            } else {
                query_pivot
            };

            if next_reference_pivot <= coordinate {
                reference_pivot = next_reference_pivot;
                query_pivot = next_query_pivot;
                continue;
            }

            match kind {
                Kind::SoftClip | Kind::Insertion | Kind::HardClip | Kind::Pad => {
                    // This should never reach
                    reference_pivot = next_reference_pivot;
                    query_pivot = next_query_pivot;
                }

                Kind::Deletion | Kind::Skip => {
                    return true;
                }

                Kind::SequenceMismatch | Kind::SequenceMatch | Kind::Match => {
                    return false;
                }
            }
        }
        false
    }

    /// Return whether an insertion is anchored at the coordinate.
    pub fn has_insertion_at(&self, coordinate: u64) -> bool {
        let mut reference_pivot = self.start;

        for op in self.record.cigar().as_ref() {
            let kind = op.kind();

            if kind == Kind::Insertion && reference_pivot == coordinate {
                return true;
            }

            if kind.consumes_reference() {
                reference_pivot = reference_pivot.saturating_add(op.len() as u64);
            }
        }

        false
    }

    pub fn passes_filter(&self, filter: &AlignmentFilter) -> bool {
        match filter {
            AlignmentFilter::Default => true,
            AlignmentFilter::Base(position, base) => {
                if let Some(base_u8) = self.base_at(*position) {
                    *base as u8 == base_u8
                } else {
                    false
                }
            }

            AlignmentFilter::BaseSoftclip(position) => self.is_softclip_at(*position),

            // They should be not be passed here.
            // They should be translated upstream.
            AlignmentFilter::BaseAtCurrentPosition(_)
            | AlignmentFilter::BaseAtCurrentPositionSoftClip => true,

            _ => true, // TODO
        }
    }
}

fn cigar_to_string(cigar: &sam::alignment::record_buf::Cigar) -> Result<String, TGVError> {
    let mut buf = Vec::new();
    sam::io::writer::record::write_cigar(&mut buf, cigar)?;
    Ok(String::from_utf8(buf)?)
}

pub fn matches_base(base1: u8, base2: u8) -> bool {
    if base1 == base2 {
        return true;
    }

    match (base1, base2) {
        (b'A', b'a')
        | (b'a', b'A')
        | (b'C', b'c')
        | (b'c', b'C')
        | (b'G', b'g')
        | (b'g', b'G')
        | (b'T', b't')
        | (b't', b'T') => true,
        _ => false,
    }
}

#[derive(Clone, Debug)]
pub struct ReadPair {
    /// Read 1 index in the alignment
    pub read_1_index: usize,

    /// If some: Read 2 index in the alignment
    /// if none: Read not shown as paired
    pub read_2_index: Option<usize>,
}

#[cfg(test)]
mod tests {

    use super::*;
    use noodles::sam::{
        self,
        alignment::{
            record::{
                MappingQuality,
                cigar::{Op, op::Kind},
            },
            record_buf::Cigar,
        },
        record::data::field::value::base_modifications::group::modification,
    };

    use crate::{
        alignment::{Alignment, CoverageTable, tables},
        sequence::Sequence,
    };
    use noodles::sam::alignment::{
        record::{Flags, data::field::Tag},
        record_buf::data::{Data, field::Value},
    };

    fn extract_base_modifications(
        mm: String,
        ml: Option<Vec<u8>>,
        flags: &Flags,
        sequence: &sam::alignment::record_buf::Sequence,
        cigars: &[Op],
        start: u64,
    ) -> Result<
        Vec<(
            u64,
            noodles::sam::record::data::field::value::base_modifications::group::Modification,
            Option<u8>,
        )>,
        TGVError,
    > {
        Ok(tables::extract_base_modifications(
            &mm,
            ml.as_deref(),
            *flags,
            sequence,
            cigars,
            start,
            0,
        )?
        .into_iter()
        .map(|(_, _, pos, modification, probability)| (pos, modification, probability))
        .collect())
    }

    fn get_reference_position_from_seq_position(
        pos: u64,
        start: u64,
        cigars: &[Op],
    ) -> Option<u64> {
        tables::locate_query_base(pos, start, cigars).map(|(_, _, pos)| pos)
    }

    use rstest::rstest;

    fn read_from_parts(
        start: u64,
        cigar_ops: impl IntoIterator<Item = (Kind, usize)>,
        sequence: &[u8],
    ) -> RecordBuf {
        let cigar: Cigar = cigar_ops
            .into_iter()
            .map(|(kind, len)| Op::new(kind, len))
            .collect();

        let record = sam::alignment::RecordBuf::builder()
            .set_alignment_start(noodles::core::Position::try_from(start as usize).unwrap())
            .set_cigar(cigar)
            .set_sequence(sam::alignment::record_buf::Sequence::from(sequence))
            .build();

        record
    }

    #[test]
    fn describe_shows_sam_style_flags_and_cigar_without_start() -> Result<(), TGVError> {
        let cigar: Cigar = [Op::new(Kind::Match, 4), Op::new(Kind::SoftClip, 2)]
            .into_iter()
            .collect();

        let record = sam::alignment::RecordBuf::builder()
            .set_name("r0")
            .set_flags(Flags::from(80))
            .set_alignment_start(noodles::core::Position::try_from(3).unwrap())
            .set_mapping_quality(MappingQuality::new(60).unwrap())
            .set_cigar(cigar)
            .build();

        let read = AlignedReadRef::borrowed(&record)?;

        assert_eq!(read.describe()?, "r0  Flags=80  MAPQ=60  Cigar=4M2S");

        Ok(())
    }

    #[test]
    fn base_at_returns_reference_aligned_bases_only() {
        let record = read_from_parts(
            10,
            [
                (Kind::SoftClip, 1),
                (Kind::Match, 2),
                (Kind::Insertion, 1),
                (Kind::SequenceMatch, 1),
                (Kind::SequenceMismatch, 1),
                (Kind::Deletion, 1),
                (Kind::Match, 1),
                (Kind::SoftClip, 1),
            ],
            b"SATIGCR",
        );
        let read = AlignedReadRef::borrowed(&record).unwrap();

        assert_eq!(read.base_at(9), None);
        assert_eq!(read.base_at(10), Some(b'A'));
        assert_eq!(read.base_at(11), Some(b'T'));
        assert_eq!(read.base_at(12), Some(b'G'));
        assert_eq!(read.base_at(13), Some(b'C'));
        assert_eq!(read.base_at(14), None);
        assert_eq!(read.base_at(15), Some(b'R'));
        assert_eq!(read.base_at(16), None);
    }

    #[test]
    fn is_deletion_at_detects_deletions_and_reference_skips() {
        let record = read_from_parts(
            10,
            [
                (Kind::Match, 2),
                (Kind::Deletion, 2),
                (Kind::Match, 1),
                (Kind::Skip, 1),
                (Kind::Match, 1),
            ],
            b"AAAA",
        );
        let read = AlignedReadRef::borrowed(&record).unwrap();

        assert!(!read.is_deletion_at(9));
        assert!(!read.is_deletion_at(10));
        assert!(!read.is_deletion_at(11));
        assert!(read.is_deletion_at(12));
        assert!(read.is_deletion_at(13));
        assert!(!read.is_deletion_at(14));
        assert!(read.is_deletion_at(15));
        assert!(!read.is_deletion_at(16));
        assert!(!read.is_deletion_at(17));
    }

    #[test]
    fn has_insertion_at_detects_insertion_anchors() {
        let record = read_from_parts(
            10,
            [
                (Kind::Match, 2),
                (Kind::Insertion, 2),
                (Kind::Match, 1),
                (Kind::Insertion, 1),
            ],
            b"AAIIT",
        );
        let read = AlignedReadRef::borrowed(&record).unwrap();

        assert!(!read.has_insertion_at(11));
        assert!(read.has_insertion_at(12));
        assert!(read.has_insertion_at(13));
        assert!(!read.has_insertion_at(14));
    }

    #[test]
    fn is_softclip_at_detects_leading_and_trailing_softclips() {
        let record = read_from_parts(
            10,
            [(Kind::SoftClip, 2), (Kind::Match, 3), (Kind::SoftClip, 1)],
            b"SSAATZ",
        );
        let read = AlignedReadRef::borrowed(&record).unwrap();

        assert!(!read.is_softclip_at(7));
        assert!(read.is_softclip_at(8));
        assert!(read.is_softclip_at(9));
        assert!(!read.is_softclip_at(10));
        assert!(!read.is_softclip_at(12));
        assert!(read.is_softclip_at(13));
        assert!(!read.is_softclip_at(14));
    }

    #[test]
    fn extract_base_modifications_preserves_missing_probabilities_for_each_position() {
        let cigars = vec![Op::new(Kind::Match, 3)];
        let sequence = sam::alignment::record_buf::Sequence::from(b"CCC");

        let modifications = extract_base_modifications(
            "C+m,0,0,0;".to_string(),
            None,
            &Flags::default(),
            &sequence,
            &cigars,
            10,
        )
        .unwrap();

        assert_eq!(
            modifications,
            vec![
                (10, modification::FIVE_METHYLCYTOSINE, None),
                (11, modification::FIVE_METHYLCYTOSINE, None),
                (12, modification::FIVE_METHYLCYTOSINE, None),
            ]
        );
    }

    #[test]
    fn extract_base_modifications_consumes_probability_for_each_position_and_modification() {
        let cigars = vec![Op::new(Kind::Match, 2)];
        let sequence = sam::alignment::record_buf::Sequence::from(b"CC");

        let modifications = extract_base_modifications(
            "C+mh,0,0;".to_string(),
            Some(vec![10, 200, 180, 20]),
            &Flags::default(),
            &sequence,
            &cigars,
            10,
        )
        .unwrap();

        assert_eq!(
            modifications,
            vec![
                (10, modification::FIVE_METHYLCYTOSINE, Some(10)),
                (10, modification::FIVE_HYDROXYMETHYLCYTOSINE, Some(200)),
                (11, modification::FIVE_METHYLCYTOSINE, Some(180)),
                (11, modification::FIVE_HYDROXYMETHYLCYTOSINE, Some(20)),
            ]
        );
    }

    #[test]
    fn get_reference_position_from_seq_position_handles_cigar_boundaries() {
        let cigars = vec![
            Op::new(Kind::SoftClip, 2),
            Op::new(Kind::Match, 2),
            Op::new(Kind::Insertion, 1),
            Op::new(Kind::Match, 2),
            Op::new(Kind::SoftClip, 1),
        ];

        assert_eq!(
            get_reference_position_from_seq_position(0, 10, &cigars),
            Some(8)
        );
        assert_eq!(
            get_reference_position_from_seq_position(2, 10, &cigars),
            Some(10)
        );
        assert_eq!(
            get_reference_position_from_seq_position(4, 10, &cigars),
            None
        );
        assert_eq!(
            get_reference_position_from_seq_position(5, 10, &cigars),
            Some(12)
        );
        assert_eq!(
            get_reference_position_from_seq_position(7, 10, &cigars),
            Some(14)
        );
    }

    #[test]
    fn alignment_tables_store_base_modification_annotations() {
        let mut data = Data::default();
        data.insert(Tag::new(b'M', b'm'), Value::from("C+m,0,0,0;"));
        data.insert(Tag::new(b'M', b'l'), Value::from(vec![255u8, 80, 20]));
        let record = sam::alignment::RecordBuf::builder()
            .set_alignment_start(noodles::core::Position::try_from(10).unwrap())
            .set_cigar([Op::new(Kind::Match, 3)].into_iter().collect())
            .set_sequence(sam::alignment::record_buf::Sequence::from(b"CCC"))
            .set_data(data)
            .build();
        let alignment =
            Alignment::from_records(vec![record], 0, (1, 100), &Sequence::default()).unwrap();
        assert_eq!(
            alignment.record(0).data().get(&Tag::new(b'M', b'm')),
            Some(&Value::from("C+m,0,0,0;")),
        );
        assert_eq!(
            alignment.record(0).data().get(&Tag::new(b'M', b'l')),
            Some(&Value::from(vec![255u8, 80, 20])),
        );
        let table = &alignment.tables.base_modifications;
        assert_eq!(
            table
                .column("display_pos")
                .unwrap()
                .u64()
                .unwrap()
                .into_no_null_iter()
                .collect::<Vec<_>>(),
            vec![10, 11, 12]
        );
        assert_eq!(
            table
                .column("probability")
                .unwrap()
                .u8()
                .unwrap()
                .into_no_null_iter()
                .collect::<Vec<_>>(),
            vec![255, 80, 20]
        );
        assert_eq!(
            table
                .column("code")
                .unwrap()
                .u8()
                .unwrap()
                .into_no_null_iter()
                .collect::<Vec<_>>(),
            vec![b'm'; 3]
        );
        assert_eq!(
            table
                .column("op_index")
                .unwrap()
                .u32()
                .unwrap()
                .into_no_null_iter()
                .collect::<Vec<_>>(),
            vec![0; 3]
        );
        assert_eq!(
            table
                .column("run_offset")
                .unwrap()
                .u32()
                .unwrap()
                .into_no_null_iter()
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }

    #[rstest]
    #[case(10, vec![(Kind::Match, 3)],  b"ATT", false,Sequence::default(), vec![(Kind::Match, 10, 12, vec![], None)])]
    // Test reverse strand
    #[case(10, vec![(Kind::Match, 3)],  b"ATT", true, Sequence::default(), vec![(Kind::Match, 10, 12, vec![], None)])]
    // Test deletion
    #[case(10, vec![(Kind::Match, 3),(Kind::Deletion, 2), (Kind::Match, 3)], b"AAATTT", true, Sequence::default(), vec![(Kind::Match, 10, 12, vec![], None), (Kind::Deletion, 13, 14, vec![], None), (Kind::Match, 15, 17, vec![], None)])]
    // Test RefSkip
    #[case(10, vec![(Kind::Match, 3),(Kind::Skip, 2)], b"AAA", false, Sequence::default(), vec![(Kind::Match, 10, 12, vec![], None), (Kind::Deletion, 13, 14, vec![], None)])]
    // Test insertion
    #[case(10, vec![(Kind::Match, 3), (Kind::Insertion, 2), (Kind::Match, 3)], b"AAATTCCC", false, Sequence::default(), vec![(Kind::Match, 10, 12, vec![], None), (Kind::Match, 13, 15, vec![], None)])]
    // Test soft clips
    #[case(10, vec![(Kind::SoftClip, 2), (Kind::Match, 3), (Kind::SoftClip, 1)], b"GGATTC", true, Sequence::default(), vec![
        (Kind::SoftClip, 8, 8, vec![], Some(b'G')),
        (Kind::SoftClip, 9, 9, vec![], Some(b'G')),
        (Kind::Match, 10, 12, vec![], None),
        (Kind::SoftClip, 13, 13, vec![], Some(b'C'))
    ])]
    // Test Equal cigar (matches current implementation with query pivot)
    #[case(10, vec![(Kind::SequenceMatch, 3)], b"ATT", false, Sequence::default(), vec![(Kind::Match, 10, 12, vec![], None)])]
    // Test Diff cigar (explicit mismatch)
    #[case(10, vec![(Kind::SequenceMismatch, 3)], b"ATT", false, Sequence::default(), vec![(Kind::Match, 10, 12, vec![(10, b'A'),(11, b'T'),(12, b'T')], None)])]
    // Test complex cigar: soft clip + match + insertion + match + deletion + match
    #[case(10, vec![(Kind::SoftClip, 1), (Kind::Match, 2), (Kind::Insertion, 1), (Kind::Match, 2), (Kind::Deletion, 3), (Kind::Match, 2)],
           b"GATCGAAA", false, Sequence::default(), vec![
        (Kind::SoftClip, 9, 9, vec![], Some(b'G')),
        (Kind::Match, 10, 11, vec![], None),
        (Kind::Match, 12, 13, vec![], None),
        (Kind::Deletion, 14, 16, vec![], None),
        (Kind::Match, 17, 18, vec![], None)
    ])]
    // Test soft clips
    #[case(10, vec![(Kind::SoftClip, 2), (Kind::Match, 3), (Kind::SoftClip, 1)], b"GGATTC", true, Sequence{start: 10, sequence: b"AATG".to_vec(), contig_index: 0}, vec![
        (Kind::SoftClip, 8, 8, vec![], Some(b'G')),
        (Kind::SoftClip, 9, 9, vec![], Some(b'G')),
        (Kind::Match, 10, 12, vec![(11, b'T')], None),
        (Kind::SoftClip, 13, 13, vec![], Some(b'C'))
    ])]
    fn run_tables_preserve_displayable_cigar_operations(
        #[case] reference_start: u64,
        #[case] cigars: Vec<(Kind, usize)>,
        #[case] seq: &[u8],
        #[case] is_reverse: bool,
        #[case] reference_sequence: Sequence,
        #[case] expected: Vec<(Kind, u64, u64, Vec<(u64, u8)>, Option<u8>)>,
    ) {
        let flags = if is_reverse {
            Flags::REVERSE_COMPLEMENTED
        } else {
            Flags::default()
        };
        let record = sam::alignment::RecordBuf::builder()
            .set_alignment_start(
                noodles::core::Position::try_from(reference_start as usize).unwrap(),
            )
            .set_flags(flags)
            .set_cigar(
                cigars
                    .into_iter()
                    .map(|(kind, len)| Op::new(kind, len))
                    .collect(),
            )
            .set_sequence(sam::alignment::record_buf::Sequence::from(seq))
            .build();
        let alignment =
            Alignment::from_records(vec![record], 0, (1, 100), &reference_sequence).unwrap();
        assert_eq!(
            alignment
                .tables
                .reads
                .column("pos")
                .unwrap()
                .u32()
                .unwrap()
                .get(0),
            Some(reference_start as u32)
        );
        let first_kind = alignment.record(0).cigar().as_ref()[0].kind();
        assert_eq!(
            alignment
                .tables
                .run(first_kind)
                .column("ref_start")
                .unwrap()
                .u64()
                .unwrap()
                .get(0),
            Some(reference_start)
        );
        assert_eq!(alignment.coverage.data.schema(), &CoverageTable::schema());
        assert_eq!(alignment.coverage.at(reference_start).unwrap().total, 1);
        assert_eq!(alignment.coverage.at(100).unwrap().total, 0);
        let viewport = alignment.tables.viewport(1, 100, &[0]).unwrap();
        let mut actual = Vec::new();
        for (kind, frame) in &viewport.runs {
            if matches!(kind, Kind::Insertion | Kind::HardClip | Kind::Pad) {
                continue;
            }
            for row in 0..frame.height() {
                let start = frame
                    .column("display_start")
                    .unwrap()
                    .u64()
                    .unwrap()
                    .get(row)
                    .unwrap();
                let end = frame
                    .column("display_end")
                    .unwrap()
                    .u64()
                    .unwrap()
                    .get(row)
                    .unwrap();
                let index = frame
                    .column("op_index")
                    .unwrap()
                    .u32()
                    .unwrap()
                    .get(row)
                    .unwrap();
                let mut mismatches = Vec::new();
                for annotation in 0..viewport.reference_mismatches.height() {
                    let table = &viewport.reference_mismatches;
                    if table
                        .column("op_index")
                        .unwrap()
                        .u32()
                        .unwrap()
                        .get(annotation)
                        == Some(index)
                    {
                        mismatches.push((
                            table
                                .column("ref_pos")
                                .unwrap()
                                .u64()
                                .unwrap()
                                .get(annotation)
                                .unwrap(),
                            table
                                .column("base")
                                .unwrap()
                                .u8()
                                .unwrap()
                                .get(annotation)
                                .unwrap(),
                        ));
                    }
                }
                let sequence = if kind.consumes_read() {
                    frame
                        .column("seq")
                        .unwrap()
                        .str()
                        .unwrap()
                        .get(row)
                        .unwrap()
                        .as_bytes()
                } else {
                    &[]
                };
                if *kind == Kind::SequenceMismatch {
                    mismatches
                        .extend((start..=end).map(|pos| (pos, sequence[(pos - start) as usize])));
                }
                if *kind == Kind::SoftClip {
                    for pos in start..=end {
                        actual.push((
                            Kind::SoftClip,
                            pos,
                            pos,
                            vec![],
                            sequence.get((pos - start) as usize).copied(),
                        ));
                    }
                } else {
                    actual.push((
                        if matches!(kind, Kind::Deletion | Kind::Skip) {
                            Kind::Deletion
                        } else {
                            Kind::Match
                        },
                        start,
                        end,
                        mismatches,
                        None,
                    ));
                }
            }
        }
        actual.sort_by_key(|value| value.1);
        assert_eq!(actual, expected);
        assert_eq!(
            alignment
                .tables
                .reads
                .column("reverse")
                .unwrap()
                .bool()
                .unwrap()
                .get(0),
            Some(is_reverse)
        );
    }
}
