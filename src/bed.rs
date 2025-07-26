use crate::{
    contig::Contig,
    error::TGVError,
    intervals::{GenomeInterval, SortedIntervalCollection},
};
use noodles::bed::{self, io::reader};
use std::{fs, io};

pub struct BEDInterval {
    contig: Contig,

    start: usize,
    end: usize,

    record: bed::Record<3>,
}

impl BEDInterval {
    pub fn new(record: bed::Record<3>) -> Result<Self, TGVError> {
        let start = record.feature_start()?.get() + 1;
        Ok(Self {
            contig: Contig::new(&record.reference_sequence_name().to_string()),
            start: start, // BED start is 0-based, inclusive
            end: match record.feature_end() {
                Some(end) => end?.get(),
                None => start, // BED end is 0-based, exclusive
            },
            record: record,
        })
    }
}

impl GenomeInterval for BEDInterval {
    fn contig(&self) -> &Contig {
        &self.contig
    }

    fn start(&self) -> usize {
        self.start
    }

    fn end(&self) -> usize {
        self.end
    }
}

pub struct BedIntervals {
    intervals: SortedIntervalCollection<BEDInterval>,
}

impl BedIntervals {
    pub fn from_bed(bed_path: &str) -> Result<Self, TGVError> {
        let mut reader = bed::io::reader::Builder::<3>::default().build_from_path(bed_path)?;
        let mut record = bed::Record::default();

        let mut records = Vec::new();

        loop {
            match reader.read_record(&mut record) {
                Ok(i) => records.push(BEDInterval::new(record.clone())?),
                Err(e) => {
                    break;
                }
            }
        }

        Ok(Self {
            intervals: SortedIntervalCollection::new(records)?,
        })
    }
}
