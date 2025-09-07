use crate::{
    contig_header::ContigHeader,
    error::TGVError,
    intervals::{GenomeInterval, Region, SortedIntervalCollection},
};
use noodles::bed::{self};

#[derive(Debug, Clone)]
pub struct BEDInterval {
    contig_index: usize,

    pub index: usize,

    start: usize,
    end: usize,

    record: bed::Record<3>,
}

impl BEDInterval {
    pub fn new(
        record: bed::Record<3>,
        index: usize,
        contig_header: &ContigHeader,
    ) -> Result<Self, TGVError> {
        let start = record.feature_start()?.get(); // Noodles already converted to 1-based, inclusive
        Ok(Self {
            contig_index: contig_header
                .get_index_by_str(&record.reference_sequence_name().to_string())?,
            index,
            start, // BED start is 0-based, inclusive
            end: match record.feature_end() {
                Some(end) => end?.get(),
                None => start, // BED end is 0-based, exclusive
            },
            record,
        })
    }

    pub fn describe(&self) -> String {
        format!(
            "{}:{}-{}",
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

    fn start(&self) -> usize {
        self.start
    }

    fn end(&self) -> usize {
        self.end
    }
}
#[derive(Debug, Clone)]
pub struct BEDIntervals {
    pub intervals: SortedIntervalCollection<BEDInterval>,
}

impl BEDIntervals {
    pub fn from_bed(bed_path: &str, contig_header: &ContigHeader) -> Result<Self, TGVError> {
        let mut reader = bed::io::reader::Builder::<3>.build_from_path(bed_path)?;
        let mut record = bed::Record::default();

        let mut records = Vec::new();

        let mut index = 0;

        while reader.read_record(&mut record)? != 0 {
            records.push(BEDInterval::new(record.clone(), index, contig_header)?);
            index += 1;
        }

        Ok(Self {
            intervals: SortedIntervalCollection::new(records)?,
        })
    }

    pub fn overlapping(&self, region: &Region) -> Result<Vec<&BEDInterval>, TGVError> {
        self.intervals.overlapping(region)
    }
}
