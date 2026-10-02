use crate::alignment::{
    coverage::{BaseCoverage, DEFAULT_COVERAGE},
    read::AlignedReadRef,
    tables::{self, AlignmentTables},
    viewport::AlignmentViewport,
};
use crate::error::TGVError;
use crate::intervals::{GenomeInterval, Region};
use crate::message::{AlignmentFilter, AlignmentSort};
use crate::sequence::Sequence;
use noodles::sam::alignment::RecordBuf;
use polars::prelude::*;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum BaseSortKey {
    A,
    T,
    C,
    G,
    N,
    OtherBase,
    Deletion,
    Insertion,
    PairGap,
}

impl BaseSortKey {
    fn from_base(base: u8) -> Self {
        match base.to_ascii_uppercase() {
            b'A' => Self::A,
            b'T' => Self::T,
            b'C' => Self::C,
            b'G' => Self::G,
            b'N' => Self::N,
            _ => Self::OtherBase,
        }
    }
}

pub(super) struct SortableStackItem {
    pub show: bool,
    pub stacking_start: u64,
    pub stacking_end: u64,
    pub sort_key: Option<BaseSortKey>,
}

/// An alignment stack
#[derive(Debug)]
pub struct Alignment {
    /// Contig of the current alignment
    pub contig_index: usize,

    /// The original decoded records in stable read-ID order.
    pub records: Vec<RecordBuf>,

    /// The queryable alignment representation.
    pub tables: AlignmentTables,

    /// Read IDs to track positions.
    pub ys: Vec<usize>,

    /// y -> read indexes at y location
    pub ys_index: Vec<Vec<usize>>,

    /// Coverage at each position. Keys are 1-based, inclusive.
    /// Calculated as needed.
    coverage: BTreeMap<u64, BaseCoverage>,

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
            ys: Vec::new(),
            ys_index: Vec::new(),
            coverage: BTreeMap::new(),
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

    /// Basewise coverage at position.
    /// 1-based, inclusive.
    pub fn coverage_at(&self, pos: u64) -> &BaseCoverage {
        match self.coverage.get(&pos) {
            Some(coverage) => coverage,
            None => &DEFAULT_COVERAGE,
        }
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
            contig_index,
            coverage: BTreeMap::new(),
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

    /// Build an alignment from the existing owned read fixtures.
    #[cfg(test)]
    pub fn from_aligned_reads(
        reads: Vec<crate::alignment::read::AlignedRead>,
        contig_index: usize,
        data_complete_bound: (u64, u64),
        reference_sequence: &Sequence,
    ) -> Result<Self, TGVError> {
        let records = reads
            .into_iter()
            .map(|read| read.record.into_owned())
            .collect::<Vec<_>>();
        let mut tables = AlignmentTables::default();
        for batch in records.chunks(1) {
            tables = tables.add_records(batch, reference_sequence, contig_index)?;
        }
        Self::from_tables(
            records,
            tables,
            contig_index,
            data_complete_bound,
            reference_sequence,
        )
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
        use noodles::sam::alignment::record::cigar::op::Kind;
        let selected = self
            .show_read
            .iter()
            .enumerate()
            .filter_map(|(id, show)| show.then_some(id))
            .collect::<Vec<_>>();
        let viewport = self.tables.viewport(1, u64::MAX, &selected)?;
        let mut coverage = BTreeMap::new();
        for (kind, runs) in &viewport.runs {
            if !matches!(
                kind,
                Kind::Match | Kind::SequenceMatch | Kind::SequenceMismatch | Kind::SoftClip
            ) {
                continue;
            }
            let starts = runs.column("display_start")?.u64()?;
            let ends = runs.column("display_end")?.u64()?;
            let offsets = runs.column("run_offset")?.u32()?;
            let sequences = runs.column("seq")?.str()?;
            for row in 0..runs.height() {
                let start = starts.get(row).expect("queried runs have display bounds");
                let end = ends.get(row).expect("queried runs have display bounds");
                let offset = offsets.get(row).expect("queried runs have offsets") as usize;
                let Some(sequence) = sequences.get(row) else {
                    continue;
                };
                let sequence = sequence.as_bytes();
                for position in start..=end {
                    let base = sequence[offset + (position - start) as usize];
                    let coordinate = position;
                    let reference_base = if reference_sequence.contig_index == self.contig_index {
                        reference_sequence.base_at(coordinate).unwrap_or(b'N')
                    } else {
                        b'N'
                    };
                    let entry = coverage
                        .entry(coordinate)
                        .or_insert_with(|| BaseCoverage::new(reference_base));
                    if *kind == Kind::SoftClip {
                        entry.update_softclip(base)
                    } else {
                        entry.update(base)
                    }
                }
            }
        }
        self.coverage = coverage;

        Ok(self)
    }

    pub fn filter(
        &mut self,
        filter: AlignmentFilter,
        reference_sequence: &Sequence,
    ) -> Result<(), TGVError> {
        let candidate_ids = match &filter {
            AlignmentFilter::Base(position, _) => self.base_candidates(*position)?,
            AlignmentFilter::BaseSoftclip(position) => {
                self.overlapping_reads(self.contig_index, *position, *position)?
            }
            _ => (0..self.records.len()).collect(),
        };
        self.show_read.fill(false);
        for i in candidate_ids {
            if self.stacking_bounds(i).is_some() {
                let read = AlignedReadRef::borrowed(&self.records[i])?;
                self.show_read[i] = read.passes_filter(&filter);
            }
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

        let candidates = self.base_candidates(position)?;
        let candidate_set = candidates
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        let items = self
            .show_read
            .iter()
            .enumerate()
            .map(|(i, show_read)| {
                let bounds = self.stacking_bounds(i);
                Ok(SortableStackItem {
                    show: *show_read,
                    stacking_start: bounds.map_or(0, |b| b.0),
                    stacking_end: bounds.map_or(0, |b| b.1),
                    sort_key: if candidate_set.contains(&i) {
                        read_base_sort_key_at(
                            &AlignedReadRef::borrowed(&self.records[i])?,
                            position,
                        )
                    } else {
                        None
                    },
                })
            })
            .collect::<Result<Vec<_>, TGVError>>()?;

        self.ys = stack_tracks_by_sort_key(&items, 3);
        self.build_y_index()?;

        Ok(())
    }

    fn base_candidates(&self, position: u64) -> Result<Vec<usize>, TGVError> {
        use noodles::sam::alignment::record::cigar::op::Kind;
        if position == 0 {
            return Ok(Vec::new());
        }
        let pos = position;
        let mut ids = std::collections::HashSet::new();
        for kind in [
            Kind::Match,
            Kind::SequenceMatch,
            Kind::SequenceMismatch,
            Kind::Deletion,
            Kind::Skip,
            Kind::Insertion,
        ] {
            let runs = self.tables.run(kind);
            let starts = runs.column("ref_start")?.u64()?;
            let lengths = runs.column("op_len")?.cast(&DataType::UInt64)?;
            let ends = starts + lengths.u64()?;
            let mask = starts.lt_eq(pos) & ends.gt(pos);
            let frame = runs.column("read_id")?.filter(&mask)?;
            for id in frame.u64()?.into_no_null_iter() {
                ids.insert(id as usize);
            }
        }
        Ok(ids.into_iter().collect())
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

pub(super) fn read_base_sort_key_at(
    read: &AlignedReadRef<'_>,
    position: u64,
) -> Option<BaseSortKey> {
    if let Some(base) = read.base_at(position) {
        return Some(BaseSortKey::from_base(base));
    }

    if read.is_deletion_at(position) {
        return Some(BaseSortKey::Deletion);
    }

    if read.has_insertion_at(position) {
        return Some(BaseSortKey::Insertion);
    }

    None
}

pub(super) fn stack_tracks_by_sort_key(items: &[SortableStackItem], min_gap: u64) -> Vec<usize> {
    let mut ys = vec![0; items.len()];
    let mut sorted_item_indexes = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            if !item.show {
                return None;
            }

            Some((item.sort_key?, index))
        })
        .collect::<Vec<_>>();

    sorted_item_indexes.sort_by_key(|(sort_key, index)| (*sort_key, *index));

    let mut is_sorted_item = vec![false; items.len()];
    let mut track_left_bounds = Vec::with_capacity(sorted_item_indexes.len());
    let mut track_right_bounds = Vec::with_capacity(sorted_item_indexes.len());
    for (y, (_sort_key, index)) in sorted_item_indexes.iter().enumerate() {
        ys[*index] = y;
        is_sorted_item[*index] = true;
        track_left_bounds.push(items[*index].stacking_start);
        track_right_bounds.push(items[*index].stacking_end);
    }

    for (index, item) in items.iter().enumerate() {
        if !item.show || is_sorted_item[index] {
            continue;
        }

        ys[index] = find_track(
            item.stacking_start,
            item.stacking_end,
            &mut track_left_bounds,
            &mut track_right_bounds,
            min_gap,
        );
    }

    ys
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
    use crate::alignment::read::AlignedRead;
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
    ) -> AlignedRead {
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

        AlignedRead::try_from(record).unwrap()
    }

    fn alignment_with_reads(reads: Vec<AlignedRead>, data_complete_bound: (u64, u64)) -> Alignment {
        Alignment::from_aligned_reads(
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
