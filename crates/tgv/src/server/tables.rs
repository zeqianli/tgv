//! SQL tables over the loaded dataset for the MCP `query` tool.
//!
//! Tables expose the documented columns of the core schemas under their core names, plus a
//! few columns the server adds, such as `track_id` and contig names. Column descriptions come
//! from [`TableSchema::column_docs`]. Before registration, every table goes through one
//! conversion for SQL: integers become `i64`, so coordinate arithmetic cannot wrap around,
//! floats become `f64`, and byte-coded columns become text. The catalog applies the same conversion to empty
//! tables, so it cannot drift from what queries see.

use crate::track_registry::{TrackId, TrackRegistry};
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
pub(super) enum TableScope {
    /// Describes the dataset itself and is always available.
    Dataset,
    /// Holds complete files and is always available.
    WholeFile,
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

const PHRED_NOTE: &str = "Encoded as Phred+33 text.";

/// The type of a column that the server adds to a table.
#[derive(Clone, Copy)]
enum AddedType {
    Int64,
    String,
    Boolean,
}

impl AddedType {
    fn dtype(self) -> DataType {
        match self {
            Self::Int64 => DataType::Int64,
            Self::String => DataType::String,
            Self::Boolean => DataType::Boolean,
        }
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
/// Columns are ordered as the leading added columns, the documented core columns, and the
/// trailing added columns.
pub(super) struct SqlTable {
    pub name: &'static str,
    pub scope: TableScope,
    pub description: &'static str,
    pub key: &'static [&'static str],
    leading: &'static [AddedColumn],
    trailing: &'static [AddedColumn],
    core_docs: fn() -> &'static [ColumnDoc],
    core_empty: fn() -> DataFrame,
    decoded: &'static [(&'static str, Decode)],
}

/// Describes one column in the catalog.
#[derive(Serialize)]
pub(super) struct CatalogColumn {
    pub name: String,
    pub dtype: String,
    pub description: String,
}

/// Describes one table in the catalog.
#[derive(Serialize)]
pub(super) struct CatalogTable {
    pub name: &'static str,
    pub scope: TableScope,
    pub description: &'static str,
    pub key: &'static [&'static str],
    pub columns: Vec<CatalogColumn>,
}

impl SqlTable {
    fn added(&self) -> impl Iterator<Item = &'static AddedColumn> {
        self.leading.iter().chain(self.trailing)
    }

    fn column_names(&self) -> impl Iterator<Item = &'static str> {
        self.leading
            .iter()
            .map(|column| column.name)
            .chain((self.core_docs)().iter().map(|doc| doc.name))
            .chain(self.trailing.iter().map(|column| column.name))
    }

    /// Builds an empty table with the added and core column types, before conversion.
    fn empty(&self) -> Result<LazyFrame, TGVError> {
        let added: Vec<Column> = self
            .added()
            .map(|column| Column::new_empty(column.name.into(), &column.r#type.dtype()))
            .collect();
        let core = (self.core_empty)();
        let core: Vec<Column> = (self.core_docs)()
            .iter()
            .map(|doc| core.column(doc.name).cloned())
            .collect::<Result<_, _>>()?;
        Ok(DataFrame::new(0, added.into_iter().chain(core).collect())?.lazy())
    }

    /// Concatenates per-track frames, selects the table columns, and converts them for SQL.
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
        )?
        .collect()?;
        Ok(self.to_sql(frame)?.lazy())
    }

    fn decoding(&self, name: &str) -> Option<Decode> {
        self.decoded
            .iter()
            .find(|(column, _)| *column == name)
            .map(|(_, decode)| *decode)
    }

    fn to_sql(&self, frame: DataFrame) -> Result<DataFrame, TGVError> {
        let columns = frame
            .columns()
            .iter()
            .map(|column| {
                if let Some(decode) = self.decoding(column.name()) {
                    let values: Vec<Option<String>> = column
                        .u8()?
                        .iter()
                        .map(|code| code.map(|code| decode.apply(code)))
                        .collect();
                    return Ok(Column::new(column.name().clone(), values));
                }
                Ok(match column.dtype() {
                    DataType::Binary => {
                        let values: Vec<Option<String>> = column
                            .binary()?
                            .iter()
                            .map(|scores| {
                                scores.map(|scores| {
                                    scores
                                        .iter()
                                        .map(|score| char::from(score.saturating_add(33)))
                                        .collect()
                                })
                            })
                            .collect();
                        Column::new(column.name().clone(), values)
                    }
                    dtype if dtype.is_integer() => column.cast(&DataType::Int64)?,
                    dtype if dtype.is_float() => column.cast(&DataType::Float64)?,
                    DataType::List(inner) if inner.is_integer() => {
                        column.cast(&DataType::List(Box::new(DataType::Int64)))?
                    }
                    _ => column.clone(),
                })
            })
            .collect::<Result<Vec<_>, TGVError>>()?;
        Ok(DataFrame::new(frame.height(), columns)?)
    }

    /// Describes the table's columns with their SQL types.
    pub fn catalog(&self) -> Result<CatalogTable, TGVError> {
        let frame = self.finish(Vec::new())?.collect()?;
        let core = (self.core_empty)();
        let added = |columns: &'static [AddedColumn]| {
            columns
                .iter()
                .map(|column| (column.name, column.description.to_owned()))
        };
        let columns = added(self.leading)
            .chain((self.core_docs)().iter().map(|doc| {
                let note = match self.decoding(doc.name) {
                    Some(decode) => Some(decode.note()),
                    None => core
                        .column(doc.name)
                        .is_ok_and(|column| column.dtype() == &DataType::Binary)
                        .then_some(PHRED_NOTE),
                };
                let description = match note {
                    Some(note) => format!("{} {note}", doc.description),
                    None => doc.description.to_owned(),
                };
                (doc.name, description)
            }))
            .chain(added(self.trailing))
            .map(|(name, description)| {
                Ok(CatalogColumn {
                    name: name.to_owned(),
                    dtype: frame.column(name)?.dtype().to_string(),
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
pub(super) struct AddedColumns;

impl AddedColumns {
    pub const TRACK_ID: &'static str = "track_id";
    pub const CONTIG: &'static str = "contig";
    pub const TYPE: &'static str = "type";
    pub const SOURCE: &'static str = "source";
    pub const END: &'static str = "end";
    pub const MATE_SAME_CONTIG: &'static str = "mate_same_contig";
    pub const REF_END: &'static str = "ref_end";
    pub const REFERENCE_BASE: &'static str = "reference_base";
    pub const POS: &'static str = "pos";
    pub const BASE: &'static str = "base";
}

const TRACK_ID: AddedColumn = AddedColumn {
    name: AddedColumns::TRACK_ID,
    r#type: AddedType::Int64,
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

pub(super) const TRACKS: SqlTable = SqlTable {
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
};

pub(super) const READS: SqlTable = SqlTable {
    name: "reads",
    scope: TableScope::Region,
    description: "One row per positioned read whose aligned span (`pos` to `end`, without soft clips) overlaps the region.",
    key: &[AddedColumns::TRACK_ID, ReadSchema::READ_ID],
    leading: &[TRACK_ID, CONTIG],
    trailing: &[
        AddedColumn {
            name: AddedColumns::END,
            r#type: AddedType::Int64,
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
};

pub(super) const CIGAR_OPS: SqlTable = SqlTable {
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
        r#type: AddedType::Int64,
        description: "The last reference position for `M`, `D`, `N`, `=`, and `X`; null for other operations.",
    }],
    core_docs: CigarSchema::column_docs,
    core_empty: CigarSchema::empty,
    decoded: &[(CigarSchema::KIND, Decode::CigarOp)],
};

pub(super) const MISMATCHES: SqlTable = SqlTable {
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
        r#type: AddedType::String,
        description: "The reference base at `ref_pos`.",
    }],
    core_docs: ReferenceMismatchSchema::column_docs,
    core_empty: ReferenceMismatchSchema::empty,
    decoded: &[(ReferenceMismatchSchema::BASE, Decode::Ascii)],
};

pub(super) const BASE_MODS: SqlTable = SqlTable {
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
};

pub(super) const COVERAGE: SqlTable = SqlTable {
    name: "coverage",
    scope: TableScope::Region,
    description: "One row per alignment track and region position, including zero-depth positions. Counts use the viewer's coverage calculation over all loaded reads. `reference_base` is null without a reference sequence.",
    key: &[AddedColumns::TRACK_ID, CoverageSchema::POS],
    leading: &[TRACK_ID],
    trailing: &[],
    core_docs: CoverageSchema::column_docs,
    core_empty: CoverageSchema::empty,
    decoded: &[(CoverageSchema::REFERENCE_BASE, Decode::Ascii)],
};

pub(super) const REFERENCE: SqlTable = SqlTable {
    name: "reference",
    scope: TableScope::Region,
    description: "One row per region position with a loaded reference base. Empty without a reference sequence.",
    key: &[AddedColumns::POS],
    leading: &[
        AddedColumn {
            name: AddedColumns::POS,
            r#type: AddedType::Int64,
            description: "The 1-based reference position.",
        },
        AddedColumn {
            name: AddedColumns::BASE,
            r#type: AddedType::String,
            description: "The reference base; lowercase marks soft-masked sequence.",
        },
    ],
    trailing: &[],
    core_docs: no_docs,
    core_empty: DataFrame::empty,
    decoded: &[],
};

pub(super) const VARIANTS: SqlTable = SqlTable {
    name: "variants",
    scope: TableScope::WholeFile,
    description: "One row per VCF record in every variant track, across all contigs.",
    key: &[AddedColumns::TRACK_ID, VariantSchema::ROW_ID],
    leading: &[TRACK_ID, CONTIG],
    trailing: &[],
    core_docs: VariantSchema::column_docs,
    core_empty: VariantSchema::empty,
    decoded: &[],
};

pub(super) const BED: SqlTable = SqlTable {
    name: "bed",
    scope: TableScope::WholeFile,
    description: "One row per interval in every BED track, across all contigs.",
    key: &[AddedColumns::TRACK_ID, BedSchema::ROW_ID],
    leading: &[TRACK_ID, CONTIG],
    trailing: &[],
    core_docs: BedSchema::column_docs,
    core_empty: BedSchema::empty,
    decoded: &[],
};

pub(super) const GENES: SqlTable = SqlTable {
    name: "genes",
    scope: TableScope::Region,
    description: "One row per annotated transcript overlapping the region.",
    key: &[GeneSchema::ROW_ID],
    leading: &[CONTIG],
    trailing: &[],
    core_docs: GeneSchema::column_docs,
    core_empty: GeneSchema::empty,
    decoded: &[],
};

pub(super) const GENE_FEATURES: SqlTable = SqlTable {
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
};

/// Every SQL table, in catalog order.
pub(super) const TABLES: [&SqlTable; 11] = [
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
pub(super) struct QueryRegion<'a> {
    pub contig_index: usize,
    pub contig: &'a str,
    pub start: u64,
    pub end: u64,
}

/// Borrows the dataset state needed to build tables.
pub(super) struct TableSources<'a> {
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
        let mut tables = vec![
            (TRACKS.name, self.tracks_table()?),
            (VARIANTS.name, self.variants_table()?),
            (BED.name, self.bed_table()?),
        ];
        if let Some(region) = region {
            let reads = self.read_frames(region)?;
            tables.extend([
                (READS.name, READS.finish(reads.reads)?),
                (CIGAR_OPS.name, CIGAR_OPS.finish(reads.cigar_ops)?),
                (MISMATCHES.name, MISMATCHES.finish(reads.mismatches)?),
                (BASE_MODS.name, BASE_MODS.finish(reads.base_mods)?),
                (COVERAGE.name, self.coverage_table(region)?),
                (REFERENCE.name, self.reference_table(region)?),
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

    /// Maps a contig index column to an added `contig` name column.
    fn contig_names(&self, indexes: &Column) -> Result<Expr, TGVError> {
        let names = &self.state.contig_header.contigs;
        let values: Vec<Option<&str>> = indexes
            .u64()?
            .iter()
            .map(|index| index.map(|index| names[index as usize].name.as_str()))
            .collect();
        Ok(lit(Series::new(AddedColumns::CONTIG.into(), values)))
    }

    fn base_at(&self, contig_index: usize, pos: u64) -> Option<u8> {
        let sequence: &Sequence = &self.state.sequence;
        (sequence.contig_index == contig_index)
            .then(|| sequence.base_at(pos))
            .flatten()
    }

    fn tracks_table(&self) -> Result<LazyFrame, TGVError> {
        let entries = &self.tracks.entries;
        let ids: Vec<i64> = entries.iter().map(|entry| entry.id as i64).collect();
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

    fn variants_table(&self) -> Result<LazyFrame, TGVError> {
        let mut frames = Vec::new();
        for entry in &self.tracks.entries {
            let RepositoryFileIndex::Variant(index) = entry.repository_index else {
                continue;
            };
            let data = &self.state.variants[index].data;
            frames.push(data.clone().lazy().with_columns([
                lit(entry.id as i64).alias(AddedColumns::TRACK_ID),
                self.contig_names(data.column(VariantSchema::CONTIG_INDEX)?)?,
            ]));
        }
        VARIANTS.finish(frames)
    }

    fn bed_table(&self) -> Result<LazyFrame, TGVError> {
        let mut frames = Vec::new();
        for entry in &self.tracks.entries {
            let RepositoryFileIndex::Bed(index) = entry.repository_index else {
                continue;
            };
            let data = &self.state.bed_intervals[index].data;
            frames.push(data.clone().lazy().with_columns([
                lit(entry.id as i64).alias(AddedColumns::TRACK_ID),
                self.contig_names(data.column(BedSchema::CONTIG_INDEX)?)?,
            ]));
        }
        BED.finish(frames)
    }

    fn read_frames(&self, region: &QueryRegion) -> Result<ReadFrames, TGVError> {
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
            let run_end = col(CigarSchema::REF_START).cast(DataType::Int64)
                + col(CigarSchema::OP_LEN).cast(DataType::Int64)
                - lit(1i64);
            let ends = tables
                .cigar_runs
                .clone()
                .lazy()
                .filter(consumes_reference.clone())
                .group_by([col(CigarSchema::READ_ID)])
                .agg([run_end.clone().max().alias(AddedColumns::END)]);
            let pos = col(ReadSchema::POS).cast(DataType::Int64);
            let reads = tables
                .reads
                .clone()
                .lazy()
                .filter(col(ReadSchema::POS).is_not_null())
                .join(
                    ends,
                    [col(ReadSchema::READ_ID)],
                    [col(CigarSchema::READ_ID)],
                    JoinArgs::new(JoinType::Left),
                )
                .with_columns([col(AddedColumns::END).fill_null(pos.clone())])
                .filter(
                    pos.lt_eq(lit(region.end as i64))
                        .and(col(AddedColumns::END).gt_eq(lit(region.start as i64))),
                )
                .with_columns([
                    lit(id as i64).alias(AddedColumns::TRACK_ID),
                    lit(region.contig).alias(AddedColumns::CONTIG),
                    col(ReadSchema::NEXT_REF_ID)
                        .eq(col(ReadSchema::REF_ID))
                        .alias(AddedColumns::MATE_SAME_CONTIG),
                ])
                .collect()?;
            let selected = reads.select([ReadSchema::READ_ID])?.lazy();
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
                    lit(id as i64).alias(AddedColumns::TRACK_ID),
                    when(consumes_reference)
                        .then(run_end)
                        .otherwise(lit(NULL))
                        .alias(AddedColumns::REF_END),
                ]),
            );

            let mismatches = of_selected_reads(
                &tables.reference_mismatches,
                ReferenceMismatchSchema::READ_ID,
            )
            .collect()?;
            let reference_bases: Vec<Option<String>> = mismatches
                .column(ReferenceMismatchSchema::REF_POS)?
                .u64()?
                .iter()
                .map(|pos| {
                    pos.and_then(|pos| self.base_at(region.contig_index, pos))
                        .map(|base| char::from(base).to_string())
                })
                .collect();
            frames.mismatches.push(mismatches.lazy().with_columns([
                lit(id as i64).alias(AddedColumns::TRACK_ID),
                lit(Series::new(
                    AddedColumns::REFERENCE_BASE.into(),
                    reference_bases,
                )),
            ]));

            frames.base_mods.push(
                of_selected_reads(&tables.base_modifications, BaseModificationSchema::READ_ID)
                    .with_columns([lit(id as i64).alias(AddedColumns::TRACK_ID)]),
            );

            frames.reads.push(reads.lazy());
        }
        Ok(frames)
    }

    fn coverage_table(&self, region: &QueryRegion) -> Result<LazyFrame, TGVError> {
        let positions: Vec<u64> = (region.start..=region.end).collect();
        let reference_bases: Vec<Option<u8>> = positions
            .iter()
            .map(|&pos| self.base_at(region.contig_index, pos))
            .collect();
        let dense = DataFrame::new(
            positions.len(),
            vec![
                Column::new(CoverageSchema::POS.into(), positions),
                Column::new(CoverageSchema::REFERENCE_BASE.into(), reference_bases),
            ],
        )?;
        let counts = [
            CoverageSchema::A,
            CoverageSchema::T,
            CoverageSchema::C,
            CoverageSchema::G,
            CoverageSchema::N,
            CoverageSchema::TOTAL,
            CoverageSchema::SOFTCLIP,
        ];
        let mut frames = Vec::new();
        for (id, alignment) in self.alignments() {
            let sparse = if alignment.contig_index == region.contig_index {
                alignment.coverage.query(region.start, region.end)?
            } else {
                CoverageSchema::empty()
            };
            frames.push(
                dense
                    .clone()
                    .lazy()
                    .join(
                        sparse.lazy().drop(cols([CoverageSchema::REFERENCE_BASE])),
                        [col(CoverageSchema::POS)],
                        [col(CoverageSchema::POS)],
                        JoinArgs::new(JoinType::Left),
                    )
                    .with_columns(
                        counts
                            .iter()
                            .map(|name| col(*name).fill_null(lit(0u64)))
                            .chain([lit(id as i64).alias(AddedColumns::TRACK_ID)])
                            .collect::<Vec<_>>(),
                    ),
            );
        }
        COVERAGE.finish(frames)
    }

    fn reference_table(&self, region: &QueryRegion) -> Result<LazyFrame, TGVError> {
        let (positions, bases): (Vec<i64>, Vec<String>) = (region.start..=region.end)
            .filter_map(|pos| {
                self.base_at(region.contig_index, pos)
                    .map(|base| (pos as i64, char::from(base).to_string()))
            })
            .unzip();
        let frame = DataFrame::new(
            positions.len(),
            vec![
                Column::new(AddedColumns::POS.into(), positions),
                Column::new(AddedColumns::BASE.into(), bases),
            ],
        )?;
        REFERENCE.finish(vec![frame.lazy()])
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

    /// Every catalog column has a description and the type that queries see.
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
                assert!(
                    !column.dtype.starts_with('u') && column.dtype != "binary",
                    "{}.{} has SQL type {}",
                    table.name,
                    column.name,
                    column.dtype
                );
            }
        }
    }
}
