//! SQL tables over the loaded dataset for the `query` command.
//!
//! Tables expose the documented columns of the core schemas under their core names, plus a
//! few columns the server adds, such as `track_id` and contig names. Column descriptions come
//! from [`TableSchema::column_docs`]. Columns keep their core types with two exceptions.
//! Byte-coded columns become text: CIGAR operations become letters, and bases and modification
//! codes become characters. Base qualities stay numeric Phred scores. Unsigned integers wider
//! than a byte become `Int64`, so subtracting coordinates cannot wrap around. Some tables also
//! pack columns into a struct, such as `cigar_ops.op`, for the functions in
//! [`crate::functions`]. The catalog resolves the same conversion on empty tables, so it cannot
//! drift from what queries see.
//!
//! Tables stay lazy. The core frames are shared rather than copied, and Polars evaluates only
//! the tables, columns, and rows that a query uses.

use crate::functions::OpStruct;
use gv_core::{
    alignment::{
        Alignment, CoverageSchema,
        tables::{BaseModificationSchema, CigarSchema, ReadSchema, ReferenceMismatchSchema},
    },
    bed::BedSchema,
    gene::{GeneSchema, GeneSegmentSchema, query_segments},
    prelude::*,
    sequence::Sequence,
    table_schema::ColumnDoc,
    variant::VariantSchema,
};
use polars::prelude::*;
use serde::Serialize;

/// Identifies which data a table holds for a query.
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TableScope {
    /// Describes the dataset itself and is always available.
    Dataset,
    /// Holds data for the query region and requires `region`.
    Region,
}

/// Converts a byte-coded core column to text.
#[derive(Clone, Copy)]
enum Decode {
    /// A BAM CIGAR operation code, decoded to its CIGAR letter.
    CigarOp,
    /// An ASCII byte, decoded to a one-character string.
    Ascii,
}

impl Decode {
    fn note(self) -> &'static str {
        match self {
            Self::CigarOp => {
                "Decoded to the CIGAR letter: `M`, `I`, `D`, `N`, `S`, `H`, `P`, `=`, or `X`."
            }
            Self::Ascii => "Decoded to a one-character string.",
        }
    }

    /// Decodes a `u8` column lazily, when a query uses it.
    fn expr(self, column: Expr) -> Expr {
        column.map(
            move |column| {
                let values = column
                    .u8()?
                    .iter()
                    .map(|code| code.map(|code| self.apply(code)));
                Ok(
                    StringChunked::from_iter_options(column.name().clone(), values)
                        .into_series()
                        .into_column(),
                )
            },
            |_, field| Ok(Field::new(field.name().clone(), DataType::String)),
        )
    }

    fn apply(self, code: u8) -> String {
        match self {
            Self::CigarOp => match code {
                CigarSchema::MATCH => "M",
                CigarSchema::INSERTION => "I",
                CigarSchema::DELETION => "D",
                CigarSchema::REFERENCE_SKIP => "N",
                CigarSchema::SOFT_CLIP => "S",
                CigarSchema::HARD_CLIP => "H",
                CigarSchema::PADDING => "P",
                CigarSchema::SEQUENCE_MATCH => "=",
                CigarSchema::SEQUENCE_MISMATCH => "X",
                _ => "?",
            }
            .to_owned(),
            Self::Ascii => char::from(code).to_string(),
        }
    }
}

/// The type of a column that the server adds to a table, before decoding.
#[derive(Clone, Copy)]
enum AddedType {
    UInt64,
    String,
    Boolean,
    /// A byte code that the table decodes to text.
    Byte,
}

impl AddedType {
    fn dtype(self) -> DataType {
        match self {
            Self::UInt64 => DataType::UInt64,
            Self::String => DataType::String,
            Self::Boolean => DataType::Boolean,
            Self::Byte => DataType::UInt8,
        }
    }
}

/// The type of a field in a packed column. `DataType` lists cannot be built in constants.
#[derive(Clone, Copy)]
pub(crate) enum PackedType {
    String,
    Int64,
    UInt8List,
}

impl PackedType {
    fn dtype(self) -> DataType {
        match self {
            Self::String => DataType::String,
            Self::Int64 => DataType::Int64,
            Self::UInt8List => DataType::List(Box::new(DataType::UInt8)),
        }
    }
}

/// Packs converted columns into one struct column, with each field cast to its declared type.
pub(crate) struct PackedColumn {
    pub name: &'static str,
    pub fields: &'static [(&'static str, PackedType)],
    pub description: &'static str,
}

impl PackedColumn {
    pub fn dtype(&self) -> DataType {
        DataType::Struct(
            self.fields
                .iter()
                .map(|(name, r#type)| Field::new((*name).into(), r#type.dtype()))
                .collect(),
        )
    }

    fn expr(&self) -> Expr {
        as_struct(
            self.fields
                .iter()
                .map(|(name, r#type)| col(*name).cast(r#type.dtype()))
                .collect(),
        )
        .alias(self.name)
    }
}

/// Describes a column that the server adds to a table.
struct AddedColumn {
    name: &'static str,
    r#type: AddedType,
    description: &'static str,
}

/// Declares one SQL table: its added columns, its core schema, and byte-coded columns.
///
/// Columns are ordered as the leading added columns, the documented core columns, the
/// trailing added columns, and the packed columns.
pub(crate) struct SqlTable {
    pub name: &'static str,
    pub scope: TableScope,
    pub description: &'static str,
    pub key: &'static [&'static str],
    leading: &'static [AddedColumn],
    trailing: &'static [AddedColumn],
    core_docs: fn() -> &'static [ColumnDoc],
    core_empty: fn() -> DataFrame,
    decoded: &'static [(&'static str, Decode)],
    packed: &'static [PackedColumn],
}

/// Describes one column in the catalog.
#[derive(Serialize)]
pub struct CatalogColumn {
    pub name: String,
    pub dtype: String,
    pub description: String,
}

/// Describes one table in the catalog.
#[derive(Serialize)]
pub struct CatalogTable {
    pub name: &'static str,
    pub scope: TableScope,
    pub description: &'static str,
    pub key: &'static [&'static str],
    pub columns: Vec<CatalogColumn>,
}

impl SqlTable {
    fn column_names(&self) -> impl Iterator<Item = &'static str> {
        self.leading
            .iter()
            .map(|column| column.name)
            .chain((self.core_docs)().iter().map(|doc| doc.name))
            .chain(self.trailing.iter().map(|column| column.name))
    }

    /// Lists the table columns with their types before decoding.
    fn raw_columns(&self) -> Result<Vec<(&'static str, DataType)>, TGVError> {
        let core = (self.core_empty)();
        let added = |columns: &'static [AddedColumn]| {
            columns
                .iter()
                .map(|column| Ok((column.name, column.r#type.dtype())))
        };
        added(self.leading)
            .chain(
                (self.core_docs)()
                    .iter()
                    .map(|doc| Ok((doc.name, core.column(doc.name)?.dtype().clone()))),
            )
            .chain(added(self.trailing))
            .collect()
    }

    /// Builds an empty table with the added and core column types, before decoding.
    fn empty(&self) -> Result<LazyFrame, TGVError> {
        let columns: Vec<Column> = self
            .raw_columns()?
            .into_iter()
            .map(|(name, dtype)| Column::new_empty(name.into(), &dtype))
            .collect();
        Ok(DataFrame::new(0, columns)?.lazy())
    }

    /// Concatenates per-track frames, selects the table columns, and decodes byte codes.
    fn finish(&self, frames: Vec<LazyFrame>) -> Result<LazyFrame, TGVError> {
        let names: Vec<Expr> = self.column_names().map(col).collect();
        let frames = if frames.is_empty() {
            vec![self.empty()?]
        } else {
            frames
        };
        let frame = concat(
            frames
                .into_iter()
                .map(|frame| frame.select(names.clone()))
                .collect::<Vec<_>>(),
            UnionArgs::default(),
        )?;
        let added: Vec<&'static str> = self
            .leading
            .iter()
            .chain(self.trailing)
            .map(|column| column.name)
            .collect();
        let conversions: Vec<Expr> = self
            .raw_columns()?
            .into_iter()
            .map(|(name, dtype)| {
                // Added columns can come from untyped literals, so they take their declared type.
                let column = if added.contains(&name) {
                    col(name).cast(dtype.clone())
                } else {
                    col(name)
                };
                self.sql_expr(name, column, &dtype)
            })
            .collect();
        let frame = frame.select(conversions);
        Ok(if self.packed.is_empty() {
            frame
        } else {
            frame.with_columns(
                self.packed
                    .iter()
                    .map(PackedColumn::expr)
                    .collect::<Vec<_>>(),
            )
        })
    }

    fn decoding(&self, name: &str) -> Option<Decode> {
        self.decoded
            .iter()
            .find(|(column, _)| *column == name)
            .map(|(_, decode)| *decode)
    }

    /// Decodes byte-coded columns to text and widens unsigned integers to `Int64`.
    ///
    /// `UInt8` columns, such as MAPQ and base quality, stay unsigned: they hold small scores
    /// rather than coordinates, counts, or IDs that queries subtract.
    fn sql_expr(&self, name: &'static str, column: Expr, dtype: &DataType) -> Expr {
        let widens = |dtype: &DataType| {
            matches!(
                dtype,
                DataType::UInt16 | DataType::UInt32 | DataType::UInt64
            )
        };
        match (self.decoding(name), dtype) {
            (Some(decode), _) => decode.expr(column),
            (None, dtype) if widens(dtype) => column.cast(DataType::Int64),
            (None, DataType::List(inner)) if widens(inner) => {
                column.cast(DataType::List(Box::new(DataType::Int64)))
            }
            (None, _) => column,
        }
    }

    /// Describes the table's columns with their SQL types.
    pub fn catalog(&self) -> Result<CatalogTable, TGVError> {
        let schema = self.finish(Vec::new())?.collect_schema()?;
        let added = |columns: &'static [AddedColumn]| {
            columns
                .iter()
                .map(|column| (column.name, column.description.to_owned()))
        };
        let columns = added(self.leading)
            .chain((self.core_docs)().iter().map(|doc| {
                let description = match self.decoding(doc.name) {
                    Some(decode) => format!("{} {}", doc.description, decode.note()),
                    None => doc.description.to_owned(),
                };
                (doc.name, description)
            }))
            .chain(added(self.trailing))
            .chain(
                self.packed
                    .iter()
                    .map(|column| (column.name, column.description.to_owned())),
            )
            .map(|(name, description)| {
                Ok(CatalogColumn {
                    name: name.to_owned(),
                    dtype: schema
                        .get(name)
                        .ok_or_else(|| PolarsError::ColumnNotFound(name.into()))?
                        .to_string(),
                    description,
                })
            })
            .collect::<Result<_, TGVError>>()?;
        Ok(CatalogTable {
            name: self.name,
            scope: self.scope,
            description: self.description,
            key: self.key,
            columns,
        })
    }
}

/// Names the columns that the server adds to tables.
pub(crate) struct AddedColumns;

impl AddedColumns {
    pub const TRACK_ID: &'static str = "track_id";
    pub const CONTIG: &'static str = "contig";
    pub const TYPE: &'static str = "type";
    pub const SOURCE: &'static str = "source";
    pub const END: &'static str = "end";
    pub const MATE_SAME_CONTIG: &'static str = "mate_same_contig";
    pub const REF_END: &'static str = "ref_end";
    pub const REFERENCE_BASE: &'static str = CoverageSchema::REFERENCE_BASE;
    pub const POS: &'static str = "pos";
    pub const BASE: &'static str = "base";
    pub const SOFT_MASKED: &'static str = "soft_masked";
}

const SOFT_MASKED: AddedColumn = AddedColumn {
    name: AddedColumns::SOFT_MASKED,
    r#type: AddedType::Boolean,
    description: "Whether the reference marks the position as soft-masked (repeat) sequence; null without a reference base. 2bit references never mark soft masking.",
};

const TRACK_ID: AddedColumn = AddedColumn {
    name: AddedColumns::TRACK_ID,
    r#type: AddedType::UInt64,
    description: "The track ID from `get_dataset`.",
};

const CONTIG: AddedColumn = AddedColumn {
    name: AddedColumns::CONTIG,
    r#type: AddedType::String,
    description: "The contig name.",
};

fn no_docs() -> &'static [ColumnDoc] {
    &[]
}

pub(crate) const TRACKS: SqlTable = SqlTable {
    name: "tracks",
    scope: TableScope::Dataset,
    description: "One row per loaded track.",
    key: &[AddedColumns::TRACK_ID],
    leading: &[
        TRACK_ID,
        AddedColumn {
            name: AddedColumns::TYPE,
            r#type: AddedType::String,
            description: "`alignment`, `variant`, or `bed`.",
        },
        AddedColumn {
            name: AddedColumns::SOURCE,
            r#type: AddedType::String,
            description: "The path the track is loaded from.",
        },
    ],
    trailing: &[],
    core_docs: no_docs,
    core_empty: DataFrame::empty,
    decoded: &[],
    packed: &[],
};

pub(crate) const READS: SqlTable = SqlTable {
    name: "reads",
    scope: TableScope::Region,
    description: "One row per positioned read whose aligned span (`pos` to `end`, without soft clips) overlaps the region.",
    key: &[AddedColumns::TRACK_ID, ReadSchema::READ_ID],
    leading: &[TRACK_ID, CONTIG],
    trailing: &[
        AddedColumn {
            name: AddedColumns::END,
            r#type: AddedType::UInt64,
            description: "The last aligned reference position, inclusive.",
        },
        AddedColumn {
            name: AddedColumns::MATE_SAME_CONTIG,
            r#type: AddedType::Boolean,
            description: "Whether the mate maps to the same contig; null when the mate has no reference.",
        },
    ],
    core_docs: ReadSchema::column_docs,
    core_empty: ReadSchema::empty,
    decoded: &[],
    packed: &[],
};

pub(crate) const CIGAR_OPS: SqlTable = SqlTable {
    name: "cigar_ops",
    scope: TableScope::Region,
    description: "One row per CIGAR operation of each read in `reads`.",
    key: &[
        AddedColumns::TRACK_ID,
        CigarSchema::READ_ID,
        CigarSchema::OP_INDEX,
    ],
    leading: &[TRACK_ID],
    trailing: &[AddedColumn {
        name: AddedColumns::REF_END,
        r#type: AddedType::UInt64,
        description: "The last reference position for `M`, `D`, `N`, `=`, and `X`; null for other operations.",
    }],
    core_docs: CigarSchema::column_docs,
    core_empty: CigarSchema::empty,
    decoded: &[(CigarSchema::KIND, Decode::CigarOp)],
    packed: &[OpStruct::COLUMN],
};

pub(crate) const MISMATCHES: SqlTable = SqlTable {
    name: "mismatches",
    scope: TableScope::Region,
    description: "One row per read base in an `M` operation that differs from the reference, for reads in `reads`. Empty without a reference sequence. Insertions and deletions are in `cigar_ops`.",
    key: &[
        AddedColumns::TRACK_ID,
        ReferenceMismatchSchema::READ_ID,
        ReferenceMismatchSchema::REF_POS,
    ],
    leading: &[TRACK_ID],
    trailing: &[AddedColumn {
        name: AddedColumns::REFERENCE_BASE,
        r#type: AddedType::Byte,
        description: "The uppercase reference base at `ref_pos`.",
    }],
    core_docs: ReferenceMismatchSchema::column_docs,
    core_empty: ReferenceMismatchSchema::empty,
    decoded: &[
        (ReferenceMismatchSchema::BASE, Decode::Ascii),
        (AddedColumns::REFERENCE_BASE, Decode::Ascii),
    ],
    packed: &[],
};

pub(crate) const BASE_MODS: SqlTable = SqlTable {
    name: "base_mods",
    scope: TableScope::Region,
    description: "One row per base modification call (MM and ML tags) on reads in `reads`.",
    key: &[
        AddedColumns::TRACK_ID,
        BaseModificationSchema::READ_ID,
        BaseModificationSchema::DISPLAY_POS,
    ],
    leading: &[TRACK_ID],
    trailing: &[],
    core_docs: BaseModificationSchema::column_docs,
    core_empty: BaseModificationSchema::empty,
    decoded: &[(BaseModificationSchema::CODE, Decode::Ascii)],
    packed: &[],
};

pub(crate) const COVERAGE: SqlTable = SqlTable {
    name: "coverage",
    scope: TableScope::Region,
    description: "One row per alignment track and region position, including zero-depth positions. Counts use the viewer's coverage calculation over all loaded reads, except duplicates and QC failures, which the viewer hides by default. `reference_base` is uppercase, and null without a reference sequence.",
    key: &[AddedColumns::TRACK_ID, CoverageSchema::POS],
    leading: &[TRACK_ID],
    trailing: &[SOFT_MASKED],
    core_docs: CoverageSchema::column_docs,
    core_empty: CoverageSchema::empty,
    decoded: &[(CoverageSchema::REFERENCE_BASE, Decode::Ascii)],
    packed: &[],
};

pub(crate) const REFERENCE: SqlTable = SqlTable {
    name: "reference",
    scope: TableScope::Region,
    description: "One row per region position with a loaded reference base. Empty without a reference sequence.",
    key: &[AddedColumns::POS],
    leading: &[
        AddedColumn {
            name: AddedColumns::POS,
            r#type: AddedType::UInt64,
            description: "The 1-based reference position.",
        },
        AddedColumn {
            name: AddedColumns::BASE,
            r#type: AddedType::Byte,
            description: "The uppercase reference base.",
        },
    ],
    trailing: &[SOFT_MASKED],
    core_docs: no_docs,
    core_empty: DataFrame::empty,
    decoded: &[(AddedColumns::BASE, Decode::Ascii)],
    packed: &[],
};

pub(crate) const VARIANTS: SqlTable = SqlTable {
    name: "variants",
    scope: TableScope::Region,
    description: "One row per VCF or BCF record overlapping the region, in every variant track.",
    key: &[AddedColumns::TRACK_ID, VariantSchema::ROW_ID],
    leading: &[TRACK_ID, CONTIG],
    trailing: &[],
    core_docs: VariantSchema::column_docs,
    core_empty: VariantSchema::empty,
    decoded: &[],
    packed: &[],
};

pub(crate) const BED: SqlTable = SqlTable {
    name: "bed",
    scope: TableScope::Region,
    description: "One row per feature overlapping the region, in every BED and bigBed track.",
    key: &[AddedColumns::TRACK_ID, BedSchema::ROW_ID],
    leading: &[TRACK_ID, CONTIG],
    trailing: &[],
    core_docs: BedSchema::column_docs,
    core_empty: BedSchema::empty,
    decoded: &[],
    packed: &[],
};

pub(crate) const GENES: SqlTable = SqlTable {
    name: "genes",
    scope: TableScope::Region,
    description: "One row per annotated transcript overlapping the region.",
    key: &[GeneSchema::ROW_ID],
    leading: &[CONTIG],
    trailing: &[],
    core_docs: GeneSchema::column_docs,
    core_empty: GeneSchema::empty,
    decoded: &[],
    packed: &[],
};

pub(crate) const GENE_FEATURES: SqlTable = SqlTable {
    name: "gene_features",
    scope: TableScope::Region,
    description: "Exon and intron segments of the transcripts in `genes` that overlap the region. Coding exons are split at the CDS bounds. Join `genes` on `gene_row_id = row_id`.",
    key: &[
        GeneSegmentSchema::GENE_ROW_ID,
        GeneSegmentSchema::KIND,
        GeneSegmentSchema::START,
    ],
    leading: &[],
    trailing: &[],
    core_docs: GeneSegmentSchema::column_docs,
    core_empty: GeneSegmentSchema::empty,
    decoded: &[],
    packed: &[],
};

/// Every SQL table, in catalog order.
pub(crate) const TABLES: [&SqlTable; 11] = [
    &TRACKS,
    &READS,
    &CIGAR_OPS,
    &MISMATCHES,
    &BASE_MODS,
    &COVERAGE,
    &REFERENCE,
    &VARIANTS,
    &BED,
    &GENES,
    &GENE_FEATURES,
];

/// The contig and inclusive bounds of a region query.
pub(crate) struct QueryRegion<'a> {
    pub contig_index: usize,
    pub contig: &'a str,
    pub start: u64,
    pub end: u64,
}

/// Borrows the dataset state needed to build tables.
pub(crate) struct TableSources<'a> {
    pub state: &'a State,
    pub tracks: &'a TrackRegistry,
    pub sources: Vec<&'a str>,
}

impl TableSources<'_> {
    /// Builds every table available for the query scope, keyed by table name.
    pub fn build(
        &self,
        region: Option<&QueryRegion>,
    ) -> Result<Vec<(&'static str, LazyFrame)>, TGVError> {
        let mut tables = vec![(TRACKS.name, self.tracks_table()?)];
        if let Some(region) = region {
            let reference = self.loaded_reference(region)?;
            let reads = self.read_frames(region, &reference)?;
            tables.extend([
                (READS.name, READS.finish(reads.reads)?),
                (CIGAR_OPS.name, CIGAR_OPS.finish(reads.cigar_ops)?),
                (MISMATCHES.name, MISMATCHES.finish(reads.mismatches)?),
                (BASE_MODS.name, BASE_MODS.finish(reads.base_mods)?),
                (COVERAGE.name, self.coverage_table(region, &reference)?),
                (REFERENCE.name, self.reference_table(region, reference)?),
                (VARIANTS.name, self.variants_table(region)?),
                (BED.name, self.bed_table(region)?),
                (GENES.name, self.genes_table(region)?),
                (GENE_FEATURES.name, self.gene_features_table(region)?),
            ]);
        }
        Ok(tables)
    }

    fn alignments(&self) -> impl Iterator<Item = (TrackId, &Alignment)> {
        self.tracks
            .entries
            .iter()
            .filter_map(|entry| match entry.repository_index {
                RepositoryFileIndex::Alignment(index) => {
                    Some((entry.id, &self.state.alignments[index]))
                }
                _ => None,
            })
    }

    /// Lists the loaded reference bases on the region's contig, by position.
    ///
    /// Bases are uppercase so that they compare equal to read bases, and soft masking moves to
    /// its own column. This copies the loaded sequence, which covers at most the regions the
    /// server loads.
    fn loaded_reference(&self, region: &QueryRegion) -> Result<LazyFrame, TGVError> {
        let sequence: &Sequence = &self.state.sequence;
        let (positions, bases, soft_masked): (Vec<u64>, Vec<u8>, Vec<bool>) =
            if sequence.contig_index == region.contig_index {
                (
                    (sequence.start..sequence.start + sequence.len() as u64).collect(),
                    sequence
                        .sequence
                        .iter()
                        .map(u8::to_ascii_uppercase)
                        .collect(),
                    sequence
                        .sequence
                        .iter()
                        .map(u8::is_ascii_lowercase)
                        .collect(),
                )
            } else {
                (Vec::new(), Vec::new(), Vec::new())
            };
        Ok(DataFrame::new(
            positions.len(),
            vec![
                Column::new(CoverageSchema::POS.into(), positions),
                Column::new(CoverageSchema::REFERENCE_BASE.into(), bases),
                Column::new(AddedColumns::SOFT_MASKED.into(), soft_masked),
            ],
        )?
        .lazy())
    }

    fn tracks_table(&self) -> Result<LazyFrame, TGVError> {
        let entries = &self.tracks.entries;
        let ids: Vec<u64> = entries.iter().map(|entry| entry.id as u64).collect();
        let types: Vec<&str> = entries
            .iter()
            .map(|entry| match entry.repository_index {
                RepositoryFileIndex::Alignment(_) => "alignment",
                RepositoryFileIndex::Variant(_) => "variant",
                RepositoryFileIndex::Bed(_) => "bed",
            })
            .collect();
        let frame = DataFrame::new(
            ids.len(),
            vec![
                Column::new(AddedColumns::TRACK_ID.into(), ids),
                Column::new(AddedColumns::TYPE.into(), types),
                Column::new(AddedColumns::SOURCE.into(), self.sources.clone()),
            ],
        )?;
        TRACKS.finish(vec![frame.lazy()])
    }

    fn variants_table(&self, region: &QueryRegion) -> Result<LazyFrame, TGVError> {
        let mut frames = Vec::new();
        for entry in &self.tracks.entries {
            let RepositoryFileIndex::Variant(index) = entry.repository_index else {
                continue;
            };
            frames.push(
                self.state.variants[index]
                    .query(region.contig_index, region.start, region.end)?
                    .lazy()
                    .with_columns([
                        lit(entry.id as u64).alias(AddedColumns::TRACK_ID),
                        lit(region.contig).alias(AddedColumns::CONTIG),
                    ]),
            );
        }
        VARIANTS.finish(frames)
    }

    fn bed_table(&self, region: &QueryRegion) -> Result<LazyFrame, TGVError> {
        let mut frames = Vec::new();
        for entry in &self.tracks.entries {
            let RepositoryFileIndex::Bed(index) = entry.repository_index else {
                continue;
            };
            frames.push(
                self.state.bed_intervals[index]
                    .query(region.contig_index, region.start, region.end)?
                    .lazy()
                    .with_columns([
                        lit(entry.id as u64).alias(AddedColumns::TRACK_ID),
                        lit(region.contig).alias(AddedColumns::CONTIG),
                    ]),
            );
        }
        BED.finish(frames)
    }

    fn read_frames(
        &self,
        region: &QueryRegion,
        reference: &LazyFrame,
    ) -> Result<ReadFrames, TGVError> {
        let mut frames = ReadFrames::default();
        for (id, alignment) in self.alignments() {
            if alignment.contig_index != region.contig_index {
                continue;
            }
            let tables = &alignment.tables;
            let consumes_reference = [
                CigarSchema::MATCH,
                CigarSchema::DELETION,
                CigarSchema::REFERENCE_SKIP,
                CigarSchema::SEQUENCE_MATCH,
                CigarSchema::SEQUENCE_MISMATCH,
            ]
            .iter()
            .map(|kind| col(CigarSchema::KIND).eq(lit(*kind)))
            .reduce(Expr::or)
            .expect("the kind list is non-empty");
            // Reference-consuming operations start at 1 or later, so this cannot underflow.
            let run_end = col(CigarSchema::REF_START) + col(CigarSchema::OP_LEN) - lit(1u64);
            let ends = tables
                .cigar_runs
                .clone()
                .lazy()
                .filter(consumes_reference.clone())
                .group_by([col(CigarSchema::READ_ID)])
                .agg([run_end.clone().max().alias(AddedColumns::END)]);
            let pos = col(ReadSchema::POS);
            let reads = tables
                .reads
                .clone()
                .lazy()
                .filter(col(ReadSchema::POS).is_not_null())
                .join(
                    ends,
                    [col(ReadSchema::READ_ID)],
                    [col(CigarSchema::READ_ID)],
                    left_join(),
                )
                .with_columns([col(AddedColumns::END).fill_null(pos.clone())])
                .filter(
                    pos.lt_eq(lit(region.end))
                        .and(col(AddedColumns::END).gt_eq(lit(region.start))),
                )
                .with_columns([
                    lit(id as u64).alias(AddedColumns::TRACK_ID),
                    lit(region.contig).alias(AddedColumns::CONTIG),
                    col(ReadSchema::NEXT_REF_ID)
                        .eq(col(ReadSchema::REF_ID))
                        .alias(AddedColumns::MATE_SAME_CONTIG),
                ]);
            let selected = reads.clone().select([col(ReadSchema::READ_ID)]);
            let of_selected_reads = |frame: &DataFrame, read_id: &str| {
                frame.clone().lazy().join(
                    selected.clone(),
                    [col(read_id)],
                    [col(ReadSchema::READ_ID)],
                    JoinArgs::new(JoinType::Semi),
                )
            };

            frames.cigar_ops.push(
                of_selected_reads(&tables.cigar_runs, CigarSchema::READ_ID).with_columns([
                    lit(id as u64).alias(AddedColumns::TRACK_ID),
                    when(consumes_reference)
                        .then(run_end)
                        .otherwise(lit(NULL))
                        .alias(AddedColumns::REF_END),
                ]),
            );

            frames.mismatches.push(
                of_selected_reads(
                    &tables.reference_mismatches,
                    ReferenceMismatchSchema::READ_ID,
                )
                .join(
                    reference.clone(),
                    [col(ReferenceMismatchSchema::REF_POS)],
                    [col(CoverageSchema::POS)],
                    left_join(),
                )
                .with_columns([lit(id as u64).alias(AddedColumns::TRACK_ID)]),
            );

            frames.base_mods.push(
                of_selected_reads(&tables.base_modifications, BaseModificationSchema::READ_ID)
                    .with_columns([lit(id as u64).alias(AddedColumns::TRACK_ID)]),
            );

            frames.reads.push(reads);
        }
        Ok(frames)
    }

    fn coverage_table(
        &self,
        region: &QueryRegion,
        reference: &LazyFrame,
    ) -> Result<LazyFrame, TGVError> {
        let positions: Vec<u64> = (region.start..=region.end).collect();
        let dense = DataFrame::new(
            positions.len(),
            vec![Column::new(CoverageSchema::POS.into(), positions)],
        )?
        .lazy()
        .join(
            reference.clone(),
            [col(CoverageSchema::POS)],
            [col(CoverageSchema::POS)],
            left_join(),
        );
        let counts = [
            CoverageSchema::A,
            CoverageSchema::T,
            CoverageSchema::C,
            CoverageSchema::G,
            CoverageSchema::N,
            CoverageSchema::TOTAL,
            CoverageSchema::SOFTCLIP,
        ];
        let in_region = col(CoverageSchema::POS)
            .gt_eq(lit(region.start))
            .and(col(CoverageSchema::POS).lt_eq(lit(region.end)));
        let mut frames = Vec::new();
        for (id, alignment) in self.alignments() {
            let sparse = if alignment.contig_index == region.contig_index {
                alignment.coverage.data.clone().lazy()
            } else {
                CoverageSchema::empty().lazy()
            };
            frames.push(
                dense
                    .clone()
                    .join(
                        sparse
                            .filter(in_region.clone())
                            .drop(cols([CoverageSchema::REFERENCE_BASE])),
                        [col(CoverageSchema::POS)],
                        [col(CoverageSchema::POS)],
                        left_join(),
                    )
                    .with_columns(
                        counts
                            .iter()
                            .map(|name| col(*name).fill_null(lit(0u64)))
                            .chain([lit(id as u64).alias(AddedColumns::TRACK_ID)])
                            .collect::<Vec<_>>(),
                    ),
            );
        }
        COVERAGE.finish(frames)
    }

    fn reference_table(
        &self,
        region: &QueryRegion,
        reference: LazyFrame,
    ) -> Result<LazyFrame, TGVError> {
        REFERENCE.finish(vec![
            reference
                .filter(
                    col(CoverageSchema::POS)
                        .gt_eq(lit(region.start))
                        .and(col(CoverageSchema::POS).lt_eq(lit(region.end))),
                )
                .select([
                    col(CoverageSchema::POS).alias(AddedColumns::POS),
                    col(CoverageSchema::REFERENCE_BASE).alias(AddedColumns::BASE),
                    col(AddedColumns::SOFT_MASKED),
                ]),
        ])
    }

    fn region_genes(&self, region: &QueryRegion) -> Result<DataFrame, TGVError> {
        self.state
            .track
            .query(region.contig_index, region.start, region.end)
    }

    fn genes_table(&self, region: &QueryRegion) -> Result<LazyFrame, TGVError> {
        GENES.finish(vec![
            self.region_genes(region)?
                .lazy()
                .with_columns([lit(region.contig).alias(AddedColumns::CONTIG)]),
        ])
    }

    fn gene_features_table(&self, region: &QueryRegion) -> Result<LazyFrame, TGVError> {
        let segments = query_segments(self.region_genes(region)?, region.start, region.end)?;
        GENE_FEATURES.finish(vec![segments.lazy()])
    }
}

/// Left-joins while keeping the left rows' order, so dense tables stay sorted by position.
fn left_join() -> JoinArgs {
    JoinArgs {
        maintain_order: MaintainOrderJoin::Left,
        ..JoinArgs::new(JoinType::Left)
    }
}

/// Per-track frames of the read-level tables, before concatenation.
#[derive(Default)]
struct ReadFrames {
    reads: Vec<LazyFrame>,
    cigar_ops: Vec<LazyFrame>,
    mismatches: Vec<LazyFrame>,
    base_mods: Vec<LazyFrame>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every catalog column has a description and the type that queries see, and byte codes
    /// and qualities are text.
    #[test]
    fn catalog_matches_tables() {
        for table in TABLES {
            let catalog = table.catalog().unwrap();
            let frame = table.finish(Vec::new()).unwrap().collect().unwrap();
            let names: Vec<_> = frame
                .get_column_names()
                .into_iter()
                .map(|name| name.to_string())
                .collect();
            let catalog_names: Vec<_> = catalog.columns.iter().map(|c| c.name.clone()).collect();
            assert_eq!(names, catalog_names, "{}", table.name);
            for column in &catalog.columns {
                assert!(
                    !column.description.is_empty(),
                    "{}.{}",
                    table.name,
                    column.name
                );
                let decoded = table.decoding(&column.name).is_some();
                assert!(
                    column.dtype != "binary" && (!decoded || column.dtype == "str"),
                    "{}.{} has SQL type {}",
                    table.name,
                    column.name,
                    column.dtype
                );
            }
        }
    }
}
