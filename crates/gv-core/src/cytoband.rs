//! Cytoband stains, and the reference's cytobands for every contig, loaded once per session.

use crate::{
    error::TGVError,
    intervals::{IntervalSchema, IntervalTable},
    table_schema::TableSchema,
};
use polars::prelude::*;
use std::sync::Arc;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Stain {
    Gneg,
    Gpos(u8),
    Acen,
    Gvar,
    Stalk,
    Other(String),
}

impl TryFrom<&str> for Stain {
    type Error = TGVError;

    fn try_from(s: &str) -> Result<Self, TGVError> {
        match s {
            "gneg" => Ok(Stain::Gneg),
            "acen" => Ok(Stain::Acen),
            "gvar" => Ok(Stain::Gvar),
            "stalk" => Ok(Stain::Stalk),
            "" => Ok(Stain::Other("unknown".to_string())),
            stain => {
                if stain.starts_with("gpos") {
                    let percentage = stain.get(4..).unwrap_or("").parse::<u8>().unwrap_or(0);
                    if percentage <= 100 {
                        Ok(Stain::Gpos(percentage))
                    } else {
                        Ok(Stain::Other(stain.to_string()))
                    }
                } else {
                    Ok(Stain::Other(stain.to_string()))
                }
            }
        }
    }
}

/// The ordered columns for cytobands.
pub struct CytobandSchema;

impl CytobandSchema {
    pub const ROW_ID: &'static str = IntervalSchema::ROW_ID;
    pub const CONTIG_INDEX: &'static str = IntervalSchema::CONTIG_INDEX;
    pub const START: &'static str = IntervalSchema::START;
    pub const END: &'static str = IntervalSchema::END;
    pub const NAME: &'static str = "name";
    /// The UCSC `gieStain` value, such as `gpos50`. [`Stain`] parses it.
    pub const STAIN: &'static str = "stain";
}

impl TableSchema for CytobandSchema {
    fn schema() -> SchemaRef {
        let mut schema = Schema::with_capacity(6);
        schema.insert(Self::ROW_ID.into(), DataType::UInt64);
        schema.insert(Self::CONTIG_INDEX.into(), DataType::UInt64);
        schema.insert(Self::START.into(), DataType::UInt64);
        schema.insert(Self::END.into(), DataType::UInt64);
        schema.insert(Self::NAME.into(), DataType::String);
        schema.insert(Self::STAIN.into(), DataType::String);
        Arc::new(schema)
    }
}

/// One cytoband, as an annotation source reports it.
pub struct CytobandBand {
    pub contig_index: usize,
    /// 1-based, inclusive.
    pub start: u64,
    /// 1-based, inclusive.
    pub end: u64,
    pub name: String,
    pub stain: String,
}

/// The reference's cytobands for every contig.
///
/// Cytobands are small, so the whole table loads once, the first time the view needs it.
#[derive(Debug)]
pub struct CytobandTable {
    /// Rows are sorted by contig, start, and row ID.
    pub data: DataFrame,
    /// Whether the table was queried. A reference without cytobands loads as an empty table.
    pub loaded: bool,
}

impl Default for CytobandTable {
    fn default() -> Self {
        Self {
            data: CytobandSchema::empty(),
            loaded: false,
        }
    }
}

impl IntervalTable for CytobandTable {
    fn query(&self, contig_index: usize, start: u64, end: u64) -> Result<DataFrame, TGVError> {
        if start == 0 {
            return Err(TGVError::ValueError(
                "Interval queries require a positive start.".into(),
            ));
        }
        if start > end {
            return Ok(CytobandSchema::empty());
        }
        Ok(self
            .data
            .clone()
            .lazy()
            .filter(
                col(CytobandSchema::CONTIG_INDEX)
                    .eq(lit(contig_index as u64))
                    .and(col(CytobandSchema::START).lt_eq(lit(end)))
                    .and(col(CytobandSchema::END).gt_eq(lit(start))),
            )
            .collect()?)
    }
}

impl CytobandTable {
    /// Builds the loaded table from every band of the reference.
    pub fn from_bands(bands: Vec<CytobandBand>) -> Result<Self, TGVError> {
        let column = |name: &str, values: Vec<u64>| Column::new(name.into(), values);
        let data = DataFrame::new(
            bands.len(),
            vec![
                column(
                    CytobandSchema::ROW_ID,
                    (0..bands.len() as u64).collect::<Vec<_>>(),
                ),
                column(
                    CytobandSchema::CONTIG_INDEX,
                    bands.iter().map(|band| band.contig_index as u64).collect(),
                ),
                column(
                    CytobandSchema::START,
                    bands.iter().map(|band| band.start).collect(),
                ),
                column(
                    CytobandSchema::END,
                    bands.iter().map(|band| band.end).collect(),
                ),
                Column::new(
                    CytobandSchema::NAME.into(),
                    bands
                        .iter()
                        .map(|band| band.name.as_str())
                        .collect::<Vec<_>>(),
                ),
                Column::new(
                    CytobandSchema::STAIN.into(),
                    bands
                        .iter()
                        .map(|band| band.stain.as_str())
                        .collect::<Vec<_>>(),
                ),
            ],
        )?
        .lazy()
        .sort(
            [
                CytobandSchema::CONTIG_INDEX,
                CytobandSchema::START,
                CytobandSchema::ROW_ID,
            ],
            SortMultipleOptions::default(),
        )
        .collect()?;
        Ok(Self { data, loaded: true })
    }

    /// A loaded table without bands, for references without cytobands.
    pub fn loaded_empty() -> Self {
        Self {
            data: CytobandSchema::empty(),
            loaded: true,
        }
    }
}
