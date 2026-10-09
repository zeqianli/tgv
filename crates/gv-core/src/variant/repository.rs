//! Readers for plain VCF, indexed VCF, and BCF files.

use super::variant::{VariantSchema, VariantTable};
use crate::{error::TGVError, intervals::Region};
use noodles::{bcf, bgzf, vcf};
use polars::prelude::*;
use std::{fs::File, path::Path};

/// A plain VCF file, parsed whole when it opens.
pub struct PlainVcf {
    path: String,
    /// The `##contig` header lines, with their lengths when declared.
    header_contigs: Vec<(String, Option<u64>)>,
    /// Contig names the records use, in order of first use.
    contigs: Vec<String>,
    /// Every record in the file, with each row's contig name in `CONTIG`. Rows get their
    /// contig index on read, since the contig header doesn't exist when the file opens.
    whole_file: DataFrame,
}

impl PlainVcf {
    /// The file's name for each row's contig.
    const CONTIG: &'static str = "contig";

    /// Parses the whole file and lists the contig names its records use.
    fn open(path: &str) -> Result<Self, TGVError> {
        let mut reader = vcf::io::reader::Builder::default().build_from_path(path)?;
        let header = reader.read_header()?;
        let records = reader
            .records()
            .map(|record| {
                Ok(vcf::variant::RecordBuf::try_from_variant_record(
                    &header, &record?,
                )?)
            })
            .collect::<Result<Vec<_>, TGVError>>()?;
        let row_contigs: Vec<&str> = records
            .iter()
            .map(|record| record.reference_sequence_name())
            .collect();
        // The real contig index is filled in on read.
        let frame = VariantTable::records_frame(&records, 0)?
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
                [
                    VariantSchema::START,
                    VariantSchema::END,
                    VariantSchema::ROW_ID,
                ],
                SortMultipleOptions::default(),
            )
            .collect()?;
        Ok(Self {
            path: path.to_owned(),
            header_contigs: header_contigs(&header),
            contigs,
            whole_file,
        })
    }

    fn read_contigs(&self) -> Vec<(String, Option<u64>)> {
        with_header_lengths(&self.contigs, &self.header_contigs)
    }

    /// Returns the whole contig, filtered from the whole file.
    fn read_variants(
        &self,
        region: &Region,
        contig_name: Option<&str>,
    ) -> Result<VariantTable, TGVError> {
        let contig_index = region.contig_index();
        let Some(name) = contig_name else {
            return VariantTable::from_records(&[], contig_index, (1, u64::MAX));
        };
        let data = self
            .whole_file
            .clone()
            .lazy()
            .filter(col(Self::CONTIG).eq(lit(name)))
            .drop(cols([Self::CONTIG]))
            .collect()?;
        VariantTable::whole_contig(&data, contig_index)
    }
}

/// A bgzipped VCF file with a tabix or CSI index, read by region.
pub struct IndexedVcf {
    path: String,
    reader: vcf::io::IndexedReader<bgzf::io::Reader<File>>,
    header: vcf::Header,
    /// Contig names in the index, which queries must use.
    contigs: Vec<String>,
}

impl IndexedVcf {
    fn open(path: &str) -> Result<Self, TGVError> {
        let mut reader = vcf::io::indexed_reader::Builder::default().build_from_path(path)?;
        let header = reader.read_header()?;
        let contigs = reader
            .index()
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
            reader,
            header,
            contigs,
        })
    }

    /// Lists the contigs in the index, since queries for others fail.
    fn read_contigs(&self) -> Vec<(String, Option<u64>)> {
        with_header_lengths(&self.contigs, &header_contigs(&self.header))
    }

    /// Returns the region, or an empty table covering the whole contig when the file has no
    /// records on it.
    fn read_variants(
        &mut self,
        region: &Region,
        contig_name: Option<&str>,
    ) -> Result<VariantTable, TGVError> {
        let contig_index = region.contig_index();
        let Some(name) = contig_name else {
            return VariantTable::from_records(&[], contig_index, (1, u64::MAX));
        };
        let records = self
            .reader
            .query(&self.header, &region.to_noodles_region(name)?)?
            .records()
            .map(|record| {
                Ok(vcf::variant::RecordBuf::try_from_variant_record(
                    &self.header,
                    &record?,
                )?)
            })
            .collect::<Result<Vec<_>, TGVError>>()?;
        VariantTable::from_records(&records, contig_index, (region.start(), region.end()))
    }
}

/// A BCF file with a CSI index, read by region.
pub struct Bcf {
    path: String,
    reader: bcf::io::IndexedReader<bgzf::io::Reader<File>>,
    header: vcf::Header,
}

impl Bcf {
    fn open(path: &str) -> Result<Self, TGVError> {
        if !Path::new(&format!("{path}.csi")).exists() {
            return Err(TGVError::IOError(format!(
                "BCF file {path} needs a CSI index at {path}.csi."
            )));
        }
        let mut reader = bcf::io::indexed_reader::Builder::default().build_from_path(path)?;
        let header = reader.read_header()?;
        Ok(Self {
            path: path.to_owned(),
            reader,
            header,
        })
    }

    /// Lists the `##contig` header lines, which BCF records refer to.
    fn read_contigs(&self) -> Vec<(String, Option<u64>)> {
        header_contigs(&self.header)
    }

    /// Returns the region, or an empty table covering the whole contig when the file has no
    /// records on it.
    fn read_variants(
        &mut self,
        region: &Region,
        contig_name: Option<&str>,
    ) -> Result<VariantTable, TGVError> {
        let contig_index = region.contig_index();
        let Some(name) = contig_name else {
            return VariantTable::from_records(&[], contig_index, (1, u64::MAX));
        };
        let records = self
            .reader
            .query(&self.header, &region.to_noodles_region(name)?)?
            .records()
            .map(|record| {
                Ok(vcf::variant::RecordBuf::try_from_variant_record(
                    &self.header,
                    &record?,
                )?)
            })
            .collect::<Result<Vec<_>, TGVError>>()?;
        VariantTable::from_records(&records, contig_index, (region.start(), region.end()))
    }
}

/// Reads variants from one VCF or BCF file.
pub enum VariantRepositoryEnum {
    Vcf(PlainVcf),
    IndexedVcf(IndexedVcf),
    Bcf(Bcf),
}

impl VariantRepositoryEnum {
    /// Opens a file, choosing the reader from its extension and whether an index exists.
    ///
    /// `.bcf` files need a `.csi` index. Bgzipped VCF files use a `.tbi` or `.csi` index when
    /// one exists next to them, and are otherwise read whole.
    pub fn new(path: &str) -> Result<Self, TGVError> {
        let lower = path.to_lowercase();
        let has_index = |extension: &str| Path::new(&format!("{path}.{extension}")).exists();
        if lower.ends_with(".bcf") {
            return Ok(Self::Bcf(Bcf::open(path)?));
        }
        if (lower.ends_with(".vcf.gz") || lower.ends_with(".vcf.bgz"))
            && (has_index("tbi") || has_index("csi"))
        {
            return Ok(Self::IndexedVcf(IndexedVcf::open(path)?));
        }
        Ok(Self::Vcf(PlainVcf::open(path)?))
    }

    pub fn path(&self) -> &str {
        match self {
            Self::Vcf(file) => &file.path,
            Self::IndexedVcf(file) => &file.path,
            Self::Bcf(file) => &file.path,
        }
    }

    /// Whether reads go through an index. Indexed files can be too large to read whole.
    pub fn is_indexed(&self) -> bool {
        !matches!(self, Self::Vcf(_))
    }

    /// Lists the contig names that reads can use, with their lengths when the header declares
    /// them.
    pub fn read_contigs(&self) -> Vec<(String, Option<u64>)> {
        match self {
            Self::Vcf(file) => file.read_contigs(),
            Self::IndexedVcf(file) => file.read_contigs(),
            Self::Bcf(file) => file.read_contigs(),
        }
    }

    /// Reads the variants overlapping a region. Plain files return the whole contig.
    /// `contig_name` is the file's name for the region's contig, or `None` when the file
    /// doesn't have it, which reads an empty table.
    pub fn read_variants(
        &mut self,
        region: &Region,
        contig_name: Option<&str>,
    ) -> Result<VariantTable, TGVError> {
        match self {
            Self::Vcf(file) => file.read_variants(region, contig_name),
            Self::IndexedVcf(file) => file.read_variants(region, contig_name),
            Self::Bcf(file) => file.read_variants(region, contig_name),
        }
    }
}

/// Lists the `##contig` header lines, with their lengths when declared.
fn header_contigs(header: &vcf::Header) -> Vec<(String, Option<u64>)> {
    header
        .contigs()
        .iter()
        .map(|(name, contig)| (name.clone(), contig.length().map(|length| length as u64)))
        .collect()
}

/// Pairs contig names with the lengths that the header declares for them.
fn with_header_lengths(
    names: &[String],
    header_contigs: &[(String, Option<u64>)],
) -> Vec<(String, Option<u64>)> {
    names
        .iter()
        .map(|name| {
            let length = header_contigs
                .iter()
                .find(|(declared, _)| declared == name)
                .and_then(|&(_, length)| length);
            (name.clone(), length)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{intervals::Focus, intervals::IntervalTable, variant::VariantSchema};
    use noodles::{csi, tabix, vcf::variant::io::Write as _};
    use rstest::rstest;
    use std::io::Write as _;
    use tempfile::TempDir;

    enum Fixture {
        Plain,
        Indexed,
        Bcf,
    }

    /// The shared VCF fixture, with records sorted by position as indexes require.
    fn sorted_vcf() -> String {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tgv/tests/data/simple.vcf"
        ))
        .unwrap();
        let (header, mut records): (Vec<&str>, Vec<&str>) =
            text.lines().partition(|line| line.starts_with('#'));
        records.sort_by_key(|line| line.split('\t').nth(1).unwrap().parse::<u64>().unwrap());
        header
            .into_iter()
            .chain(records)
            .map(|line| format!("{line}\n"))
            .collect()
    }

    fn write_fixture(dir: &TempDir, fixture: Fixture) -> String {
        let text = sorted_vcf();
        let path = |name: &str| dir.path().join(name).to_str().unwrap().to_owned();
        match fixture {
            Fixture::Plain => {
                let path = path("simple.vcf");
                std::fs::write(&path, text).unwrap();
                path
            }
            Fixture::Indexed => {
                let path = path("simple.vcf.gz");
                let mut writer = bgzf::io::Writer::new(File::create(&path).unwrap());
                writer.write_all(text.as_bytes()).unwrap();
                writer.finish().unwrap();
                tabix::fs::write(format!("{path}.tbi"), &vcf::fs::index(&path).unwrap()).unwrap();
                path
            }
            Fixture::Bcf => {
                let path = path("simple.bcf");
                let mut reader = vcf::io::Reader::new(text.as_bytes());
                let header = reader.read_header().unwrap();
                let mut writer = bcf::io::Writer::new(File::create(&path).unwrap());
                writer.write_header(&header).unwrap();
                for record in reader.records() {
                    writer
                        .write_variant_record(&header, &record.unwrap())
                        .unwrap();
                }
                writer.try_finish().unwrap();
                csi::fs::write(format!("{path}.csi"), &bcf::fs::index(&path).unwrap()).unwrap();
                path
            }
        }
    }

    fn region(contig_index: usize, start: u64, end: u64) -> Region {
        Region {
            focus: Focus {
                contig_index,
                position: (start + end) / 2,
            },
            half_width: (end - start) / 2,
        }
    }

    /// Every reader returns the same records for a region and covers it, and a contig the
    /// file lacks loads as complete and empty. The fixture calls chr20 `20`.
    #[rstest]
    #[case::plain(Fixture::Plain, false)]
    #[case::indexed_vcf(Fixture::Indexed, true)]
    #[case::bcf(Fixture::Bcf, true)]
    fn reads_variants_by_region(#[case] fixture: Fixture, #[case] indexed: bool) {
        let dir = TempDir::new().unwrap();
        let mut repository = VariantRepositoryEnum::new(&write_fixture(&dir, fixture)).unwrap();
        assert_eq!(repository.is_indexed(), indexed);
        assert_eq!(
            repository.read_contigs(),
            [("20".to_owned(), Some(62_435_964))]
        );
        let viewed = region(1, 80_000, 90_000);
        let table = repository.read_variants(&viewed, Some("20")).unwrap();
        assert!(table.has_complete_data(&viewed));
        let rows = table.query(1, viewed.start(), viewed.end()).unwrap();
        let starts: Vec<u64> = rows
            .column(VariantSchema::START)
            .unwrap()
            .u64()
            .unwrap()
            .into_no_null_iter()
            .collect();
        assert_eq!(starts, [88_108]);

        let absent = region(0, 1_000, 2_000);
        let table = repository.read_variants(&absent, None).unwrap();
        assert!(table.has_complete_data(&absent));
        assert_eq!(table.data.height(), 0);
    }
}
