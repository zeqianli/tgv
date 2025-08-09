use crate::{
    contig::Contig,
    error::TGVError,
    intervals::{GenomeInterval, SortedIntervalCollection},
};
use noodles::bed::{self};

#[derive(Debug, Clone)]
pub struct BEDInterval {
    contig: usize,

    pub index: usize,

    start: usize,
    end: usize,

    record: bed::Record<3>,
}

impl BEDInterval {
    pub fn new(record: bed::Record<3>, index: usize) -> Result<Self, TGVError> {
        let start = record.feature_start()?.get(); // Noodles already converted to 1-based, inclusive
        Ok(Self {
            contig: Contig::new(&record.reference_sequence_name().to_string()),
            index,
            start, // BED start is 0-based, inclusive
            end: match record.feature_end() {
                Some(end) => end?.get(),
                None => start, // BED end is 0-based, exclusive
            },
            record,
        })
    }
}

impl GenomeInterval for BEDInterval {
    fn contig(&self) -> usize {
        self.contig
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
    pub fn from_bed(bed_path: &str) -> Result<Self, TGVError> {
        let mut reader = bed::io::reader::Builder::<3>.build_from_path(bed_path)?;
        let mut record = bed::Record::default();

        let mut records = Vec::new();

        let mut index = 0;

        while reader.read_record(&mut record)? != 0 {
            records.push(BEDInterval::new(record.clone(), index)?);
            index += 1;
        }

        Ok(Self {
            intervals: SortedIntervalCollection::new(records)?,
        })
    }
}
