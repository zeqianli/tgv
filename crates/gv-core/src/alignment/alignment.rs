use crate::alignment::{
    coverage::CoverageTable,
    tables::{self, AlignmentTables},
    viewport::AlignmentViewport,
};
use crate::error::TGVError;
use crate::intervals::{GenomeInterval, Region};
use crate::message::{AlignmentFilter, AlignmentSort};
use crate::sequence::Sequence;
use noodles::sam::alignment::RecordBuf;
use polars::prelude::*;

/// An alignment stack
#[derive(Debug)]
pub struct Alignment {
    /// Contig of the current alignment
    pub contig_index: usize,

    /// The original decoded records in stable read-ID order.
    pub records: Vec<RecordBuf>,

    /// The queryable alignment representation.
    pub tables: AlignmentTables,

    /// Derived coverage of the visible reads.
    pub coverage: CoverageTable,

    /// Read IDs to track positions.
    pub ys: Vec<usize>,

    /// y -> read indexes at y location
    pub ys_index: Vec<Vec<usize>>,

    /// The left bound of region with complete data.
    /// 1-based, inclusive.
    data_complete_left_bound: u64,

    /// The right bound of region with complete data.
    /// 1-based, inclusive.
    data_complete_right_bound: u64,

    // Whether to display the read
    pub show_read: Vec<bool>,
}

impl Default for Alignment {
    fn default() -> Self {
        Self {
            contig_index: 0,
            records: Vec::new(),
            tables: AlignmentTables::default(),
            coverage: CoverageTable::default(),
            ys: Vec::new(),
            ys_index: Vec::new(),
            data_complete_left_bound: 0,
            data_complete_right_bound: 0,
            show_read: Vec::new(),
        }
    }
}

impl Alignment {
    /// Check if data in [left, right] is all loaded.
    /// 1-based, inclusive.
    pub fn has_complete_data(&self, region: &Region) -> bool {
        (region.contig_index() == self.contig_index)
            && (region.start() >= self.data_complete_left_bound)
            && (region.end() <= self.data_complete_right_bound)
    }

    pub(super) fn ensure_position_has_complete_data(&self, position: u64) -> Result<(), TGVError> {
        if position < self.data_complete_left_bound || position > self.data_complete_right_bound {
            return Err(TGVError::AlignmentSortPositionNotLoaded {
                position,
                loaded_left: self.data_complete_left_bound,
                loaded_right: self.data_complete_right_bound,
            });
        }

        Ok(())
    }

    /// Return the number of alignment tracks.
    pub fn depth(&self) -> usize {
        self.ys_index.len()
    }

    /// Iterate over indexed reads whose display spans overlap an inclusive interval.
    /// The display span includes soft clips, matching the read hit test.
    pub fn overlapping_reads(
        &self,
        contig_index: usize,
        start: u64,
        end: u64,
    ) -> Result<Vec<usize>, TGVError> {
        if self.contig_index != contig_index || start > end {
            return Ok(Vec::new());
        }
        let reads = &self.tables.reads;
        let left = reads.column("stacking_start")?.u64()?;
        let right = reads.column("stacking_end")?.u64()?;
        let mask = left.lt_eq(end) & right.gt_eq(start);
        let hits = reads.column("read_id")?.filter(&mask)?;
        Ok(hits
            .u64()?
            .into_no_null_iter()
            .map(|id| id as usize)
            .collect())
    }

    /// Return the read at x_coordinate, yth track
    pub fn read_overlapping(
        &self,
        left: u64,
        right: u64,
        y: usize,
    ) -> Result<Option<usize>, TGVError> {
        if y >= self.depth() {
            return Ok(None);
        }
        let hits = self.overlapping_reads(self.contig_index, left, right)?;
        Ok(self.ys_index[y]
            .iter()
            .copied()
            .find(|id| hits.contains(id)))
    }

    pub fn record(&self, read_id: usize) -> &RecordBuf {
        &self.records[read_id]
    }

    pub fn read_count(&self) -> usize {
        self.records.len()
    }

    /// One-based, inclusive display bounds, including soft clips.
    pub fn stacking_bounds(&self, read_id: usize) -> Option<(u64, u64)> {
        let reads = &self.tables.reads;
        let start = reads
            .column("stacking_start")
            .expect("reads have stacking bounds")
            .u64()
            .expect("bounds are u64")
            .get(read_id);
        let end = reads
            .column("stacking_end")
            .expect("reads have stacking bounds")
            .u64()
            .expect("bounds are u64")
            .get(read_id);
        start.zip(end)
    }

    pub fn query_viewport(
        &self,
        region: &Region,
        read_ids: &[usize],
    ) -> Result<AlignmentViewport, TGVError> {
        if region.contig_index() != self.contig_index || region.start() > region.end() {
            return self.tables.viewport(1, 1, &[]);
        }
        self.tables.viewport(region.start(), region.end(), read_ids)
    }

    pub(crate) fn prepare_reference_mismatches(
        &self,
        reference: &Sequence,
    ) -> Result<DataFrame, TGVError> {
        tables::reference_mismatches(
            self.tables
                .run(noodles::sam::alignment::record::cigar::op::Kind::Match),
            reference,
            self.contig_index,
        )
    }

    pub(crate) fn replace_reference_mismatches(&mut self, table: DataFrame) {
        self.tables.reference_mismatches = table;
    }

    pub fn from_records(
        records: Vec<RecordBuf>,
        contig_index: usize,
        data_complete_bound: (u64, u64),
        reference_sequence: &Sequence,
    ) -> Result<Self, TGVError> {
        let tables =
            AlignmentTables::default().add_records(&records, reference_sequence, contig_index)?;
        Self::from_tables(
            records,
            tables,
            contig_index,
            data_complete_bound,
            reference_sequence,
        )
    }

    pub(crate) fn from_tables(
        records: Vec<RecordBuf>,
        tables: AlignmentTables,
        contig_index: usize,
        data_complete_bound: (u64, u64),
        reference_sequence: &Sequence,
    ) -> Result<Self, TGVError> {
        assert_eq!(
            records.len(),
            tables.reads.height(),
            "record IDs and table rows have matching shapes"
        );
        let show_reads = tables
            .reads
            .column("stacking_start")?
            .is_not_null()
            .iter()
            .map(|value| value.expect("validity masks are non-null"))
            .collect::<Vec<_>>();
        let ys = stack_tracks_for_reads(&tables.reads, &show_reads)?;
        let mut alignment = Self {
            records,
            tables,
            coverage: CoverageTable::default(),
            contig_index,
            data_complete_left_bound: data_complete_bound.0,
            data_complete_right_bound: data_complete_bound.1,
            ys,
            show_read: show_reads,
            ys_index: Vec::new(),
        };
        alignment
            .build_y_index()?
            .build_coverage(reference_sequence)?;
        Ok(alignment)
    }

    /// Build indexes, coverages after key assets are set: reads, show_read, ys
    pub fn build_y_index(&mut self) -> Result<&mut Self, TGVError> {
        let mut ys_index = vec![Vec::new(); *self.ys.iter().max().unwrap_or(&0) + 1];
        self.ys
            .iter()
            .zip(self.show_read.iter())
            .enumerate()
            .for_each(|(i, (y, show_read))| {
                if *show_read {
                    ys_index[*y].push(i)
                }
            });
        self.ys_index = ys_index;

        Ok(self)
    }

    pub fn build_coverage(&mut self, reference_sequence: &Sequence) -> Result<&mut Self, TGVError> {
        let selected = self
            .show_read
            .iter()
            .enumerate()
            .filter_map(|(id, show)| show.then_some(id))
            .collect::<Vec<_>>();
        let viewport = self.tables.viewport(1, u64::MAX, &selected)?;
        self.coverage = CoverageTable::from_runs(&viewport, self.contig_index, reference_sequence)?;

        Ok(self)
    }

    pub fn filter(
        &mut self,
        filter: AlignmentFilter,
        reference_sequence: &Sequence,
    ) -> Result<(), TGVError> {
        let selected = match filter {
            AlignmentFilter::Base(position, base) => {
                let events = self.base_events(position)?;
                let mask = events.column("base")?.u8()?.equal(base as u8);
                events
                    .column("read_id")?
                    .filter(&mask)?
                    .u64()?
                    .into_no_null_iter()
                    .map(|id| id as usize)
                    .collect::<Vec<_>>()
            }
            AlignmentFilter::BaseSoftclip(position) => {
                let ids = (0..self.read_count()).collect::<Vec<_>>();
                let viewport = self.tables.viewport(position, position, &ids)?;
                let (_, clips) = viewport
                    .runs
                    .iter()
                    .find(|(kind, _)| {
                        *kind == noodles::sam::alignment::record::cigar::op::Kind::SoftClip
                    })
                    .expect("viewport includes the soft-clip table");
                clips
                    .column("read_id")?
                    .u64()?
                    .into_no_null_iter()
                    .map(|id| id as usize)
                    .collect()
            }
            _ => (0..self.read_count()).collect(),
        };
        self.show_read.fill(false);
        for id in selected {
            self.show_read[id] = self.stacking_bounds(id).is_some();
        }

        self.ys = stack_tracks_for_reads(&self.tables.reads, &self.show_read)?;
        self.build_y_index()?.build_coverage(reference_sequence)?;

        Ok(())
    }

    pub fn sort(&mut self, option: AlignmentSort) -> Result<(), TGVError> {
        match option {
            AlignmentSort::BaseAt(position) => self.sort_by_base_at(position),
            option => Err(TGVError::ValueError(format!(
                "Alignment sorting is not implemented yet for option {option}"
            ))),
        }
    }

    fn sort_by_base_at(&mut self, position: u64) -> Result<(), TGVError> {
        self.ensure_position_has_complete_data(position)?;

        let events = self.base_events(position)?;
        let mut items = self.tables.reads.clone();
        items.with_column(events.column("sort_key")?.clone())?;
        self.ys = stack_tracks_by_sort_key(&items, "read_id", &self.show_read, 3)?;
        self.build_y_index()?;
        Ok(())
    }

    /// Query aligned bases and event priorities at a one-based position.
    pub(super) fn base_events(&self, position: u64) -> Result<DataFrame, TGVError> {
        use noodles::sam::alignment::record::cigar::op::Kind;
        let mut bases = vec![None; self.read_count()];
        // Priorities are A, T, C, G, N, other bases, deletions, insertions, then pair gaps.
        let mut keys: Vec<Option<u8>> = vec![None; self.read_count()];
        if position > 0 {
            // Later event kinds take precedence over insertions at the same cursor.
            for kind in [
                Kind::Insertion,
                Kind::Deletion,
                Kind::Skip,
                Kind::Match,
                Kind::SequenceMatch,
                Kind::SequenceMismatch,
            ] {
                let runs = self.tables.run(kind);
                let starts = runs.column("ref_start")?.u64()?;
                let mask = if kind == Kind::Insertion {
                    starts.equal(position)
                } else {
                    let lengths = runs.column("op_len")?.cast(&DataType::UInt64)?;
                    starts.lt_eq(position) & (starts + lengths.u64()?).gt(position)
                };
                let hits = runs.filter(&mask)?;
                let ids = hits.column("read_id")?.u64()?;
                let starts = hits.column("ref_start")?.u64()?;
                let sequences = if matches!(
                    kind,
                    Kind::Match | Kind::SequenceMatch | Kind::SequenceMismatch
                ) {
                    Some(hits.column("seq")?.str()?)
                } else {
                    None
                };
                for row in 0..hits.height() {
                    let id = ids.get(row).expect("run IDs are non-null") as usize;
                    let key = match kind {
                        Kind::Insertion => Some(7),
                        Kind::Deletion | Kind::Skip => Some(6),
                        _ => {
                            let offset = (position
                                - starts.get(row).expect("selected runs have positions"))
                                as usize;
                            let base = sequences
                                .expect("aligned runs have a sequence column")
                                .get(row)
                                .and_then(|seq| seq.as_bytes().get(offset))
                                .copied();
                            bases[id] = base;
                            base.map(|base| match base.to_ascii_uppercase() {
                                b'A' => 0,
                                b'T' => 1,
                                b'C' => 2,
                                b'G' => 3,
                                b'N' => 4,
                                _ => 5,
                            })
                        }
                    };
                    if let Some(key) = key {
                        keys[id] = Some(key);
                    }
                }
            }
        }
        Ok(DataFrame::new(
            self.read_count(),
            vec![
                self.tables.reads.column("read_id")?.clone(),
                Column::new("base".into(), bases),
                Column::new("sort_key".into(), keys),
            ],
        )?)
    }
}

fn stack_tracks_for_reads(reads: &DataFrame, show_reads: &[bool]) -> Result<Vec<usize>, TGVError> {
    let starts = reads.column("stacking_start")?.u64()?;
    let ends = reads.column("stacking_end")?.u64()?;
    let mut track_left_bounds = Vec::new();
    let mut track_right_bounds = Vec::new();
    Ok(show_reads
        .iter()
        .enumerate()
        .map(|(id, show)| {
            if *show {
                let start = starts.get(id).expect("visible reads have display bounds");
                let end = ends.get(id).expect("visible reads have display bounds");
                find_track(
                    start,
                    end,
                    &mut track_left_bounds,
                    &mut track_right_bounds,
                    3,
                )
            } else {
                0
            }
        })
        .collect())
}

pub(super) fn stack_tracks_by_sort_key(
    items: &DataFrame,
    id_column: &str,
    show: &[bool],
    min_gap: u64,
) -> Result<Vec<usize>, TGVError> {
    let visible = items.filter(&BooleanChunked::from_slice("show".into(), show))?;
    let sorted = visible
        .filter(&visible.column("sort_key")?.is_not_null())?
        .sort(["sort_key", id_column], SortMultipleOptions::default())?;
    let mut ys = vec![0; items.height()];
    let mut track_left_bounds = Vec::with_capacity(sorted.height());
    let mut track_right_bounds = Vec::with_capacity(sorted.height());
    let ids = sorted.column(id_column)?.u64()?;
    let starts = sorted.column("stacking_start")?.u64()?;
    let ends = sorted.column("stacking_end")?.u64()?;
    for row in 0..sorted.height() {
        ys[ids.get(row).expect("item IDs are non-null") as usize] = row;
        track_left_bounds.push(starts.get(row).expect("visible items have bounds"));
        track_right_bounds.push(ends.get(row).expect("visible items have bounds"));
    }
    let remaining = visible.filter(&visible.column("sort_key")?.is_null())?;
    let ids = remaining.column(id_column)?.u64()?;
    let starts = remaining.column("stacking_start")?.u64()?;
    let ends = remaining.column("stacking_end")?.u64()?;
    for row in 0..remaining.height() {
        let id = ids.get(row).expect("item IDs are non-null") as usize;
        ys[id] = find_track(
            starts.get(row).expect("visible items have bounds"),
            ends.get(row).expect("visible items have bounds"),
            &mut track_left_bounds,
            &mut track_right_bounds,
            min_gap,
        );
    }
    Ok(ys)
}

pub(super) fn find_track(
    start: u64,
    end: u64,
    track_left_bounds: &mut Vec<u64>,
    track_right_bounds: &mut Vec<u64>,
    min_gap: u64,
) -> usize {
    for (y, left_bound) in track_left_bounds.iter_mut().enumerate() {
        if end + min_gap < *left_bound {
            *left_bound = start;

            return y;
        }
    }

    for (y, right_bound) in track_right_bounds.iter_mut().enumerate() {
        if start > *right_bound + min_gap {
            *right_bound = end;
            return y;
        }
    }

    track_left_bounds.push(start);
    track_right_bounds.push(end);
    track_left_bounds.len() - 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use noodles::sam::{
        self,
        alignment::{
            record::{
                Flags,
                cigar::{Op, op::Kind},
            },
            record_buf::Cigar,
        },
    };

    fn read(
        name: &str,
        start: u64,
        cigar_ops: impl IntoIterator<Item = (Kind, usize)>,
        sequence: &[u8],
    ) -> RecordBuf {
        let cigar: Cigar = cigar_ops
            .into_iter()
            .map(|(kind, len)| Op::new(kind, len))
            .collect();

        let record = sam::alignment::RecordBuf::builder()
            .set_name(name)
            .set_flags(Flags::default())
            .set_alignment_start(noodles::core::Position::try_from(start as usize).unwrap())
            .set_cigar(cigar)
            .set_sequence(sam::alignment::record_buf::Sequence::from(sequence))
            .build();

        record
    }

    fn alignment_with_reads(reads: Vec<RecordBuf>, data_complete_bound: (u64, u64)) -> Alignment {
        Alignment::from_records(
            reads,
            0,
            data_complete_bound,
            &Sequence {
                start: 1,
                sequence: vec![b'A'; 100],
                contig_index: 0,
            },
        )
        .unwrap()
    }

    #[test]
    fn sort_by_base_orders_visible_reads_by_base_event_kind() {
        let mut alignment = alignment_with_reads(
            vec![
                read("g", 12, [(Kind::Match, 1)], b"G"),
                read("t", 12, [(Kind::Match, 1)], b"T"),
                read("ins", 10, [(Kind::Match, 2), (Kind::Insertion, 1)], b"AAI"),
                read("a", 12, [(Kind::Match, 1)], b"A"),
                read("n", 12, [(Kind::Match, 1)], b"N"),
                read("del", 12, [(Kind::Deletion, 1)], b""),
                read("c", 12, [(Kind::Match, 1)], b"C"),
                read("hidden-a", 12, [(Kind::Match, 1)], b"A"),
            ],
            (1, 100),
        );
        alignment.show_read[7] = false;

        alignment.sort(AlignmentSort::BaseAt(12)).unwrap();

        assert_eq!(alignment.ys, vec![3, 1, 6, 0, 4, 5, 2, 0]);
        assert_eq!(
            alignment.ys_index,
            vec![
                vec![3],
                vec![1],
                vec![6],
                vec![0],
                vec![4],
                vec![5],
                vec![2]
            ]
        );
    }

    #[test]
    fn sort_by_base_packs_remaining_reads_into_sorted_rows_when_possible() {
        let mut alignment = alignment_with_reads(
            vec![
                read("sorted", 50, [(Kind::Match, 1)], b"A"),
                read("left", 10, [(Kind::Match, 11)], b"AAAAAAAAAAA"),
                read("right", 80, [(Kind::Match, 11)], b"AAAAAAAAAAA"),
                read("too-close-left", 48, [(Kind::Match, 1)], b"A"),
            ],
            (1, 100),
        );

        alignment.sort(AlignmentSort::BaseAt(50)).unwrap();

        assert_eq!(alignment.ys, vec![0, 0, 0, 1]);
        assert_eq!(alignment.ys_index, vec![vec![0, 1, 2], vec![3]]);
    }

    #[test]
    fn sort_by_base_returns_dedicated_error_when_position_is_not_loaded() {
        let mut alignment =
            alignment_with_reads(vec![read("a", 12, [(Kind::Match, 1)], b"A")], (10, 20));

        let error = alignment.sort(AlignmentSort::BaseAt(21)).unwrap_err();

        assert!(matches!(
            error,
            TGVError::AlignmentSortPositionNotLoaded {
                position: 21,
                loaded_left: 10,
                loaded_right: 20,
            }
        ));
    }

    #[test]
    fn find_track_returns_zero_based_new_and_reused_tracks() {
        let mut track_left_bounds = Vec::new();
        let mut track_right_bounds = Vec::new();

        assert_eq!(
            find_track(10, 20, &mut track_left_bounds, &mut track_right_bounds, 3),
            0
        );
        assert_eq!(
            find_track(21, 25, &mut track_left_bounds, &mut track_right_bounds, 3),
            1
        );
        assert_eq!(
            find_track(1, 5, &mut track_left_bounds, &mut track_right_bounds, 3),
            0
        );
    }
}
