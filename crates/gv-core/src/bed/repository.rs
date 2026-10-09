//! Readers for plain BED, indexed BED, and bigBed files.

use super::bed::{BedColumns, BedSchema, BedTable, is_data_line};
use crate::{error::TGVError, intervals::Region};
use bigtools::{BigBedRead, utils::reopen::ReopenableFile};
use noodles::{bgzf, csi, csi::BinningIndex, tabix};
use polars::prelude::*;
use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

/// A plain BED file, parsed whole when it opens.
pub struct PlainBed {
    path: String,
    /// Contig names the file uses, in order of first use.
    contigs: Vec<String>,
    /// Every feature in the file, with each row's contig name in `CONTIG`. Rows get their
    /// contig index on read, since the contig header doesn't exist when the file opens.
    whole_file: DataFrame,
}

impl PlainBed {
    /// The file's name for each row's contig.
    const CONTIG: &'static str = "contig";

    /// Parses the whole file and lists the contig names it uses.
    fn open(path: &str) -> Result<Self, TGVError> {
        // Bgzipped files without an index are read whole.
        let lines: Box<dyn BufRead> = if path.to_lowercase().ends_with(".gz") {
            Box::new(BufReader::new(bgzf::io::Reader::new(File::open(path)?)))
        } else {
            Box::new(BufReader::new(File::open(path)?))
        };
        let mut columns = BedColumns::default();
        let mut row_contigs: Vec<String> = Vec::new();
        for line in lines.lines() {
            let line = line?;
            if !is_data_line(&line) {
                continue;
            }
            row_contigs.push(line.split('\t').next().unwrap_or_default().to_owned());
            // The real contig index is filled in on read.
            columns.push_line(&line, 0)?;
        }
        let frame = columns
            .into_frame()?
            .lazy()
            .with_columns([lit(Series::new(Self::CONTIG.into(), row_contigs))])
            .collect()?;
        let contigs = frame
            .column(Self::CONTIG)?
            .unique_stable()?
            .str()?
            .iter()
            .flatten()
            .map(str::to_owned)
            .collect();
        let whole_file = frame
            .lazy()
            .sort(
                [BedSchema::START, BedSchema::END, BedSchema::ROW_ID],
                SortMultipleOptions::default(),
            )
            .collect()?;
        Ok(Self {
            path: path.to_owned(),
            contigs,
            whole_file,
        })
    }

    fn read_contigs(&self) -> Vec<(String, Option<u64>)> {
        self.contigs
            .iter()
            .map(|name| (name.clone(), None))
            .collect()
    }

    /// Returns the whole contig, filtered from the whole file.
    fn read_bed(&self, region: &Region, contig_name: Option<&str>) -> Result<BedTable, TGVError> {
        let contig_index = region.contig_index();
        let Some(name) = contig_name else {
            return BedColumns::default().into_table(contig_index, (1, u64::MAX));
        };
        let data = self
            .whole_file
            .clone()
            .lazy()
            .filter(col(Self::CONTIG).eq(lit(name)))
            .drop(cols([Self::CONTIG]))
            .collect()?;
        BedTable::whole_contig(&data, contig_index)
    }
}

/// A bgzipped BED file with a tabix or CSI index, read by region.
pub struct IndexedBed {
    path: String,
    reader: csi::io::IndexedReader<bgzf::io::Reader<File>, Box<dyn BinningIndex>>,
    /// Contig names in the index, which queries must use.
    contigs: Vec<String>,
}

impl IndexedBed {
    fn open(path: &str, index: Box<dyn BinningIndex>) -> Result<Self, TGVError> {
        let contigs = index
            .header()
            .map(|header| {
                header
                    .reference_sequence_names()
                    .iter()
                    .map(|name| name.to_string())
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            path: path.to_owned(),
            reader: csi::io::IndexedReader::new(File::open(path)?, index),
            contigs,
        })
    }

    fn read_contigs(&self) -> Vec<(String, Option<u64>)> {
        self.contigs
            .iter()
            .map(|name| (name.clone(), None))
            .collect()
    }

    /// Returns the region, or an empty table covering the whole contig when the file has no
    /// features on it.
    fn read_bed(
        &mut self,
        region: &Region,
        contig_name: Option<&str>,
    ) -> Result<BedTable, TGVError> {
        let contig_index = region.contig_index();
        let mut columns = BedColumns::default();
        let Some(name) = contig_name else {
            return columns.into_table(contig_index, (1, u64::MAX));
        };
        for record in self.reader.query(&region.to_noodles_region(name)?)? {
            columns.push_line(record?.as_ref(), contig_index)?;
        }
        columns.into_table(contig_index, (region.start(), region.end()))
    }
}

/// A bigBed file, read by region.
pub struct BigBed {
    path: String,
    // PERF: bigtools reads synchronously, like the other BED and VCF readers.
    reader: BigBedRead<ReopenableFile>,
}

impl BigBed {
    fn open(path: &str) -> Result<Self, TGVError> {
        Ok(Self {
            path: path.to_owned(),
            reader: BigBedRead::open_file(path)?,
        })
    }

    fn read_contigs(&self) -> Vec<(String, Option<u64>)> {
        self.reader
            .chroms()
            .iter()
            .map(|chrom| (chrom.name.clone(), Some(u64::from(chrom.length))))
            .collect()
    }

    /// Returns the region, or an empty table covering the whole contig when the file has no
    /// features on it.
    fn read_bed(
        &mut self,
        region: &Region,
        contig_name: Option<&str>,
    ) -> Result<BedTable, TGVError> {
        let contig_index = region.contig_index();
        let mut columns = BedColumns::default();
        let Some(name) = contig_name else {
            return columns.into_table(contig_index, (1, u64::MAX));
        };
        let end = u32::try_from(region.end()).unwrap_or(u32::MAX);
        // bigBed coordinates are 0-based and half-open.
        for entry in self
            .reader
            .get_interval(name, (region.start() - 1) as u32, end)?
        {
            let entry = entry?;
            // The reader also returns features that only touch the window.
            if u64::from(entry.start) < region.end() && u64::from(entry.end) >= region.start() {
                // Rebuild the BED line, so bigBed features go through the same validation.
                let mut line = format!("{name}\t{}\t{}", entry.start, entry.end);
                if !entry.rest.is_empty() {
                    line.push('\t');
                    line.push_str(&entry.rest);
                }
                columns.push_line(&line, contig_index)?;
            }
        }
        columns.into_table(contig_index, (region.start(), region.end()))
    }
}

/// Reads features from one BED or bigBed file.
pub enum BedRepositoryEnum {
    Bed(PlainBed),
    IndexedBed(IndexedBed),
    BigBed(BigBed),
}

impl BedRepositoryEnum {
    /// Opens a file, choosing the reader from its extension and whether an index exists.
    ///
    /// Bgzipped BED files use a `.tbi` or `.csi` index when one exists next to them, and are
    /// otherwise read whole.
    pub fn new(path: &str) -> Result<Self, TGVError> {
        let lower = path.to_lowercase();
        if lower.ends_with(".bb") || lower.ends_with(".bigbed") {
            return Ok(Self::BigBed(BigBed::open(path)?));
        }
        if lower.ends_with(".gz") {
            let index: Option<Box<dyn BinningIndex>> = if let Some(tbi) = index_path(path, "tbi") {
                Some(Box::new(tabix::fs::read(tbi)?))
            } else if let Some(csi) = index_path(path, "csi") {
                Some(Box::new(csi::fs::read(csi)?))
            } else {
                None
            };
            if let Some(index) = index {
                return Ok(Self::IndexedBed(IndexedBed::open(path, index)?));
            }
        }
        Ok(Self::Bed(PlainBed::open(path)?))
    }

    pub fn path(&self) -> &str {
        match self {
            Self::Bed(file) => &file.path,
            Self::IndexedBed(file) => &file.path,
            Self::BigBed(file) => &file.path,
        }
    }

    /// Whether reads go through an index. Indexed files can be too large to read whole.
    pub fn is_indexed(&self) -> bool {
        !matches!(self, Self::Bed(_))
    }

    /// Lists the contig names that reads can use, with their lengths when known.
    pub fn read_contigs(&self) -> Vec<(String, Option<u64>)> {
        match self {
            Self::Bed(file) => file.read_contigs(),
            Self::IndexedBed(file) => file.read_contigs(),
            Self::BigBed(file) => file.read_contigs(),
        }
    }

    /// Reads the features overlapping a region. Plain files return the whole contig.
    /// `contig_name` is the file's name for the region's contig, or `None` when the file
    /// doesn't have it, which reads an empty table.
    pub fn read_bed(
        &mut self,
        region: &Region,
        contig_name: Option<&str>,
    ) -> Result<BedTable, TGVError> {
        match self {
            Self::Bed(file) => file.read_bed(region, contig_name),
            Self::IndexedBed(file) => file.read_bed(region, contig_name),
            Self::BigBed(file) => file.read_bed(region, contig_name),
        }
    }
}

/// Returns the path of an index next to the data file, if one exists.
fn index_path(path: &str, extension: &str) -> Option<String> {
    let index_path = format!("{path}.{extension}");
    Path::new(&index_path).exists().then_some(index_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bed::BedSchema, intervals::Focus, intervals::IntervalTable};
    use rstest::rstest;
    use std::io::Write as _;
    use tempfile::TempDir;

    const SIMPLE_BED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../tgv/tests/data/simple.bed");
    const BIGBED: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tgv/tests/data/cache/GCF_000005845.2/GCF_000005845.2_ASM584v2.ncbiGene.bb"
    );

    fn region(contig_index: usize, start: u64, end: u64) -> Region {
        Region {
            focus: Focus {
                contig_index,
                position: (start + end) / 2,
            },
            half_width: (end - start) / 2,
        }
    }

    /// Plain and indexed BED files return the same features for a region and cover it.
    #[rstest]
    #[case::plain(false)]
    #[case::indexed(true)]
    fn reads_bed_by_region(#[case] indexed: bool) {
        let dir = TempDir::new().unwrap();
        let path = if indexed {
            let path = dir
                .path()
                .join("simple.bed.gz")
                .to_str()
                .unwrap()
                .to_owned();
            let mut writer = bgzf::io::Writer::new(File::create(&path).unwrap());
            writer
                .write_all(&std::fs::read(SIMPLE_BED).unwrap())
                .unwrap();
            writer.finish().unwrap();
            tabix::fs::write(
                format!("{path}.tbi"),
                &noodles::bed::fs::index(&path).unwrap(),
            )
            .unwrap();
            path
        } else {
            SIMPLE_BED.to_owned()
        };
        let mut repository = BedRepositoryEnum::new(&path).unwrap();
        assert_eq!(repository.is_indexed(), indexed);
        let viewed = region(1, 88_000, 88_300);
        let table = repository.read_bed(&viewed, Some("chr20")).unwrap();
        assert!(table.has_complete_data(&viewed));
        let rows = table.query(1, viewed.start(), viewed.end()).unwrap();
        let column = |name: &str| -> Vec<u64> {
            rows.column(name)
                .unwrap()
                .u64()
                .unwrap()
                .into_no_null_iter()
                .collect()
        };
        assert_eq!(column(BedSchema::START), [88_001, 88_013, 88_101]);
        assert_eq!(column(BedSchema::END), [88_010, 88_013, 88_200]);
    }

    /// bigBed features convert to 1-based coordinates and keep their names and strands.
    #[test]
    fn reads_bigbed_by_region() {
        let mut repository = BedRepositoryEnum::new(BIGBED).unwrap();
        assert!(repository.is_indexed());
        let (name, _) = repository.read_contigs().remove(0);

        let viewed = region(0, 1, 20_000);
        let table = repository.read_bed(&viewed, Some(&name)).unwrap();
        assert!(table.has_complete_data(&viewed));
        let rows = table.query(0, viewed.start(), viewed.end()).unwrap();
        assert!(rows.height() > 0);
        let names = rows.column(BedSchema::NAME).unwrap().str().unwrap();
        let strands = rows.column(BedSchema::STRAND).unwrap().str().unwrap();
        assert!(names.iter().all(|name| name.is_some()));
        assert!(
            strands
                .iter()
                .all(|strand| matches!(strand, Some("+" | "-")))
        );
        let starts = rows.column(BedSchema::START).unwrap().u64().unwrap();
        assert!(starts.into_no_null_iter().all(|start| start >= 1));
    }
}
