use crate::{
    contig_header::ContigHeader,
    error::TGVError,
    intervals::{GenomeInterval, SortedIntervalCollection},
};
use noodles::bed::{self};

#[derive(Debug, Clone)]
pub struct BEDInterval {
    contig_index: usize,

    pub index: usize,

    start: u64,
    end: u64,

    record: bed::Record<3>,
}

impl BEDInterval {
    pub fn new(
        record: bed::Record<3>,
        index: usize,
        contig_header: &ContigHeader,
    ) -> Result<Self, TGVError> {
        let start = record.feature_start()?.get() as u64; // Noodles already converted to 1-based, inclusive
        Ok(Self {
            contig_index: contig_header
                .try_get_index_by_str(&record.reference_sequence_name().to_string())?,
            index,
            start, // BED start is 0-based, inclusive
            end: match record.feature_end() {
                Some(end) => end?.get() as u64,
                None => start, // BED end is 0-based, exclusive
            },
            record,
        })
    }

    pub fn describe(&self) -> String {
        format!(
            "BED interval: {}:{}-{}",
            self.record.reference_sequence_name(),
            self.start,
            self.end
        )
    }
}

impl GenomeInterval for BEDInterval {
    fn contig_index(&self) -> usize {
        self.contig_index
    }

    fn start(&self) -> u64 {
        self.start
    }

    fn end(&self) -> u64 {
        self.end
    }
}
#[derive(Debug, Clone)]
pub struct BEDRepository {
    pub bed_path: String,
}

impl BEDRepository {
    pub fn read_bed(
        &self,
        contig_header: &ContigHeader,
    ) -> Result<SortedIntervalCollection<BEDInterval>, TGVError> {
        let mut reader = bed::io::reader::Builder::<3>.build_from_path(self.bed_path.as_str())?;
        let mut record = bed::Record::default();

        let mut records = Vec::new();

        let mut index = 0;

        while reader.read_record(&mut record)? != 0 {
            records.push(BEDInterval::new(record.clone(), index, contig_header)?);
            index += 1;
        }

        SortedIntervalCollection::new(records)
    }
}
