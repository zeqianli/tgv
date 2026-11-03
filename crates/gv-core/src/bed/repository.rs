//! Readers for plain BED, indexed BED, and bigBed files, on the local filesystem or in S3.

use super::{
    bed::{BedColumns, BedSchema, BedTable, is_data_line},
    edit::BedHistory,
};
use crate::{
    alignment::is_url,
    error::TGVError,
    intervals::Region,
    object_storage::{S3Object, index_contigs, read_tabix_or_csi_index},
    settings::{FilePath, file_name},
};
use async_compat::Compat;
use bigtools::{BigBedRead, utils::reopen::ReopenableFile};
use futures::TryStreamExt;
use noodles::{bgzf, csi, csi::BinningIndex, tabix};
use opendal::{FuturesAsyncReader, Operator};
use polars::prelude::*;
use std::{
    collections::HashSet,
    fs::File,
    io::{BufRead, BufReader, Cursor},
    path::Path,
};

/// A plain BED file, parsed whole when it opens. These are the BED tracks that can be edited.
pub struct PlainBed {
    /// The file path, or the name of a new track that has no file yet.
    pub path: String,
    /// Contig names the file uses, in order of first use.
    pub(super) contigs: Vec<String>,
    /// Every feature in the file, with each row's contig name in `CONTIG`. Rows get their
    /// contig index on read, since the contig header doesn't exist when the file opens.
    pub(super) whole_file: DataFrame,
    /// Comment, `track`, and `browser` lines, in file order. Writing puts them at the top.
    pub(super) header_lines: Vec<String>,
    /// The most standard columns (three to six) any row has. Every written row has this many.
    pub(super) column_count: usize,
    /// Whether the rows were grouped by contig and sorted by start when the file loaded. Writing
    /// a sorted file keeps it sorted; other files keep their row order.
    pub(super) sorted_on_load: bool,
    pub history: BedHistory,
}

impl PlainBed {
    /// The file's name for each row's contig.
    pub(super) const CONTIG: &'static str = "contig";

    /// Parses the whole file and lists the contig names it uses.
    fn open(path: &str) -> Result<Self, TGVError> {
        let failed = |operation| {
            move |source| TGVError::FileOperationError {
                operation,
                path: path.into(),
                source,
            }
        };
        let file = File::open(path).map_err(failed("open the BED"))?;
        // Bgzipped files without an index are read whole.
        let lines: Box<dyn BufRead> = if path.to_lowercase().ends_with(".gz") {
            Box::new(BufReader::new(bgzf::io::Reader::new(file)))
        } else {
            Box::new(BufReader::new(file))
        };
        Self::parse(path, lines).map_err(|error| match error {
            TGVError::FileIOError(source) => failed("read the BED")(source),
            error => error,
        })
    }

    /// Downloads the whole object and parses it. Bgzipped objects are decompressed.
    ///
    /// The track can be edited, and written to a local path.
    async fn open_remote(object: &S3Object, operator: &Operator) -> Result<Self, TGVError> {
        const READ: &str = "read the BED";
        let bytes = Cursor::new(object.read_all(operator, READ).await?);
        let lines: Box<dyn BufRead> = if object.url.to_lowercase().ends_with(".gz") {
            Box::new(BufReader::new(bgzf::io::Reader::new(bytes)))
        } else {
            Box::new(bytes)
        };
        Self::parse(&object.url, lines).map_err(|error| match error {
            TGVError::FileIOError(source) => object.read_error(READ)(source),
            error => error,
        })
    }

    fn parse(path: &str, lines: impl BufRead) -> Result<Self, TGVError> {
        let mut columns = BedColumns::default();
        let mut row_contigs: Vec<String> = Vec::new();
        let mut header_lines = Vec::new();
        for line in lines.lines() {
            let line = line?;
            if !is_data_line(&line) {
                if !line.trim_end().is_empty() {
                    header_lines.push(line);
                }
                continue;
            }
            row_contigs.push(line.split('\t').next().unwrap_or_default().to_owned());
            // The real contig index is filled in on read.
            columns.push_line(&line, 0)?;
        }
        let column_count = columns.standard_columns.max(3);
        let frame = columns
            .into_frame()?
            .lazy()
            .with_columns([lit(Series::new(Self::CONTIG.into(), row_contigs))])
            .collect()?;
        let sorted_on_load = is_sorted(&frame)?;
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
            header_lines,
            column_count,
            sorted_on_load,
            history: BedHistory::new(true),
        })
    }

    /// Creates a track with no features and no file, named `name`.
    fn empty(name: String) -> Result<Self, TGVError> {
        let mut whole_file = BedColumns::default().into_frame()?;
        whole_file.with_column(Column::new(Self::CONTIG.into(), Vec::<String>::new()))?;
        Ok(Self {
            path: name,
            contigs: Vec::new(),
            whole_file,
            header_lines: Vec::new(),
            column_count: 3,
            // New tracks are always written sorted.
            sorted_on_load: true,
            history: BedHistory::new(false),
        })
    }

    fn read_contigs(&self) -> Vec<(String, Option<u64>)> {
        self.contigs
            .iter()
            .map(|name| (name.clone(), None))
            .collect()
    }

    /// Copies the features, header lines, and column layout into a track with no file, named
    /// `name`. The copy has a history of its own, and no saved revision until it is written.
    pub fn duplicate(&self, name: String) -> Self {
        Self {
            path: name,
            contigs: self.contigs.clone(),
            // Clones share the column buffers, so copying is cheap.
            whole_file: self.whole_file.clone(),
            header_lines: self.header_lines.clone(),
            column_count: self.column_count,
            sorted_on_load: self.sorted_on_load,
            history: BedHistory::new(false),
        }
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
        let contigs = index_contigs(&index);
        let file = File::open(path).map_err(|source| TGVError::FileOperationError {
            operation: "open the BED",
            path: path.into(),
            source,
        })?;
        Ok(Self {
            path: path.to_owned(),
            reader: csi::io::IndexedReader::new(file, index),
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

/// A bgzipped BED object in S3 with a tabix or CSI index, read by region.
///
/// Like a remote BAM, the index is read once, and each query reads through a new stream.
pub struct RemoteIndexedBed {
    object: S3Object,
    operator: Operator,
    reader: csi::r#async::io::IndexedReader<
        bgzf::r#async::io::Reader<Compat<FuturesAsyncReader>>,
        Box<dyn BinningIndex + Send + Sync>,
    >,
    /// Contig names in the index, which queries must use.
    contigs: Vec<String>,
}

impl RemoteIndexedBed {
    const QUERY: &str = "query the BED";

    async fn open(
        object: S3Object,
        operator: Operator,
        index: Box<dyn BinningIndex + Send + Sync>,
    ) -> Result<Self, TGVError> {
        let contigs = index_contigs(&index);
        let stream = object.open_read(&operator, "open the BED").await?;
        Ok(Self {
            reader: csi::r#async::io::IndexedReader::new(stream, index),
            object,
            operator,
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
    async fn read_bed(
        &mut self,
        region: &Region,
        contig_name: Option<&str>,
    ) -> Result<BedTable, TGVError> {
        let contig_index = region.contig_index();
        let mut columns = BedColumns::default();
        let Some(name) = contig_name else {
            return columns.into_table(contig_index, (1, u64::MAX));
        };
        let query_region = region.to_noodles_region(name)?;
        // A new stream for each query, as remote BAM queries do, so that a query never resumes
        // from where the last one stopped.
        *self.reader.get_mut() = bgzf::r#async::io::Reader::new(
            self.object.open_read(&self.operator, Self::QUERY).await?,
        );
        let read_error = self.object.read_error(Self::QUERY);
        let mut query = self.reader.query(&query_region).map_err(&read_error)?;
        while let Some(record) = query.try_next().await.map_err(&read_error)? {
            columns.push_line(record.as_ref(), contig_index)?;
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
    // Boxed because the async reader's buffers are much larger than the other variants.
    RemoteIndexedBed(Box<RemoteIndexedBed>),
}

impl BedRepositoryEnum {
    /// Opens a file or S3 object, choosing the reader from its extension and whether an index
    /// exists.
    ///
    /// Bgzipped BED files use a `.tbi` or `.csi` index when one exists next to them, and are
    /// otherwise read whole. Remote bigBed files are not supported yet.
    pub async fn new(path: &str) -> Result<Self, TGVError> {
        if is_url(path) {
            return Self::new_remote(path).await;
        }
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

    async fn new_remote(url: &str) -> Result<Self, TGVError> {
        let lower = url.to_lowercase();
        if lower.ends_with(".bb") || lower.ends_with(".bigbed") {
            return Err(TGVError::UnsupportedRemoteFormat {
                url: url.to_owned(),
                format: "bigBed",
            });
        }
        let object = S3Object::parse(url)?;
        let operator = object.operator()?;
        if lower.ends_with(".gz")
            && let Some(index) = read_tabix_or_csi_index(&object, &operator).await?
        {
            return Ok(Self::RemoteIndexedBed(Box::new(
                RemoteIndexedBed::open(object, operator, index).await?,
            )));
        }
        Ok(Self::Bed(PlainBed::open_remote(&object, &operator).await?))
    }

    /// Creates the empty repository of a new track.
    pub fn untitled(file_path: &FilePath) -> Result<Self, TGVError> {
        Ok(Self::Bed(PlainBed::empty(file_path.to_string())?))
    }

    pub fn path(&self) -> &str {
        match self {
            Self::Bed(file) => &file.path,
            Self::IndexedBed(file) => &file.path,
            Self::BigBed(file) => &file.path,
            Self::RemoteIndexedBed(file) => &file.object.url,
        }
    }

    /// The editable file, or an error naming the file for indexed and bigBed files.
    pub fn as_editable_mut(&mut self) -> Result<&mut PlainBed, TGVError> {
        match self {
            Self::Bed(file) => Ok(file),
            Self::IndexedBed(_) | Self::BigBed(_) | Self::RemoteIndexedBed(_) => {
                Err(TGVError::ReadOnlyBed {
                    name: file_name(self.path()).to_owned(),
                })
            }
        }
    }

    /// Whether the track has edits that aren't written to its file. Read-only files never do.
    pub fn is_modified(&self) -> bool {
        match self {
            Self::Bed(file) => file.history.is_modified(),
            Self::IndexedBed(_) | Self::BigBed(_) | Self::RemoteIndexedBed(_) => false,
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
            Self::RemoteIndexedBed(file) => file.read_contigs(),
        }
    }

    /// Reads the features overlapping a region. Plain files return the whole contig.
    /// `contig_name` is the file's name for the region's contig, or `None` when the file
    /// doesn't have it, which reads an empty table.
    pub async fn read_bed(
        &mut self,
        region: &Region,
        contig_name: Option<&str>,
    ) -> Result<BedTable, TGVError> {
        match self {
            Self::Bed(file) => file.read_bed(region, contig_name),
            Self::IndexedBed(file) => file.read_bed(region, contig_name),
            Self::BigBed(file) => file.read_bed(region, contig_name),
            Self::RemoteIndexedBed(file) => file.read_bed(region, contig_name).await,
        }
    }
}

/// Whether the rows, in file order, are grouped by contig and sorted by start within each.
fn is_sorted(frame: &DataFrame) -> Result<bool, TGVError> {
    let contigs = frame.column(PlainBed::CONTIG)?.str()?;
    let starts = frame.column(BedSchema::START)?.u64()?;
    let mut finished = HashSet::new();
    let mut previous: Option<(&str, u64)> = None;
    for (contig, start) in contigs.iter().zip(starts.iter()) {
        let (contig, start) = (
            contig.expect("BED contigs are non-null"),
            start.expect("BED starts are non-null"),
        );
        match previous {
            Some((previous_contig, previous_start)) if previous_contig == contig => {
                if start < previous_start {
                    return Ok(false);
                }
            }
            Some((previous_contig, _)) => {
                finished.insert(previous_contig);
                if finished.contains(contig) {
                    return Ok(false);
                }
            }
            None => {}
        }
        previous = Some((contig, start));
    }
    Ok(true)
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
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reads_bed_by_region(#[case] indexed: bool) {
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
        let mut repository = BedRepositoryEnum::new(&path).await.unwrap();
        assert_eq!(repository.is_indexed(), indexed);
        let viewed = region(1, 88_000, 88_300);
        let table = repository.read_bed(&viewed, Some("chr20")).await.unwrap();
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
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reads_bigbed_by_region() {
        let mut repository = BedRepositoryEnum::new(BIGBED).await.unwrap();
        assert!(repository.is_indexed());
        let (name, _) = repository.read_contigs().remove(0);

        let viewed = region(0, 1, 20_000);
        let table = repository.read_bed(&viewed, Some(&name)).await.unwrap();
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
