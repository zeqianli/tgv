//! Cytoband stains, and the reference's cytobands for every contig, loaded once per session.

use crate::{
    error::Error,
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
    type Error = Error;

    fn try_from(s: &str) -> Result<Self, Error> {
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
#[derive(Debug, Clone)]
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
    fn query(&self, contig_index: usize, start: u64, end: u64) -> Result<DataFrame, Error> {
        if start == 0 {
            return Err(Error::ValueError(
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
    pub fn from_bands(bands: Vec<CytobandBand>) -> Result<Self, Error> {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cytoband {
    pub start: u64,
    pub end: u64,
    pub stain: Stain,
}

impl CytobandTable {
    pub fn bands(&self, contig_index: usize) -> Result<Vec<Cytoband>, Error> {
        let bands = self.query(contig_index, 1, u64::MAX)?;
        let starts = bands.column(CytobandSchema::START)?.u64()?;
        let ends = bands.column(CytobandSchema::END)?.u64()?;
        let stains = bands.column(CytobandSchema::STAIN)?.str()?;
        (0..bands.height())
            .map(|row| {
                Ok(Cytoband {
                    start: starts.get(row).ok_or(Error::NullValue {
                        column: CytobandSchema::START,
                    })?,
                    end: ends.get(row).ok_or(Error::NullValue {
                        column: CytobandSchema::END,
                    })?,
                    stain: Stain::try_from(stains.get(row).unwrap_or_default())?,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn band(contig_index: usize, start: u64, end: u64, stain: &str) -> CytobandBand {
        CytobandBand {
            contig_index,
            start,
            end,
            name: format!("band{start}"),
            stain: stain.to_string(),
        }
    }

    fn table() -> CytobandTable {
        CytobandTable::from_bands(vec![
            band(0, 101, 200, "gpos50"),
            band(0, 1, 100, "gneg"),
            band(0, 201, 300, "acen"),
            band(1, 1, 500, "stalk"),
        ])
        .unwrap()
    }

    fn cytoband(start: u64, end: u64, stain: Stain) -> Cytoband {
        Cytoband { start, end, stain }
    }

    #[rstest]
    #[case::sorted_by_start(0, vec![
        cytoband(1, 100, Stain::Gneg),
        cytoband(101, 200, Stain::Gpos(50)),
        cytoband(201, 300, Stain::Acen),
    ])]
    #[case::other_contig(1, vec![cytoband(1, 500, Stain::Stalk)])]
    #[case::contig_without_bands(2, vec![])]
    fn bands(#[case] contig_index: usize, #[case] expected: Vec<Cytoband>) {
        assert_eq!(table().bands(contig_index).unwrap(), expected);
    }

    #[test]
    fn bands_of_an_empty_table() {
        assert_eq!(CytobandTable::loaded_empty().bands(0).unwrap(), vec![]);
    }

    #[test]
    fn a_missing_stain_is_unknown() {
        let data = DataFrame::new(
            1,
            vec![
                Column::new(CytobandSchema::CONTIG_INDEX.into(), vec![0u64]),
                Column::new(CytobandSchema::START.into(), vec![1u64]),
                Column::new(CytobandSchema::END.into(), vec![100u64]),
                Column::new(CytobandSchema::STAIN.into(), vec![None::<&str>]),
            ],
        )
        .unwrap();
        let table = CytobandTable { data, loaded: true };
        assert_eq!(
            table.bands(0).unwrap(),
            vec![cytoband(1, 100, Stain::Other("unknown".to_string()))]
        );
    }
}
