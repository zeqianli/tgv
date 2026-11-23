use crate::table_schema::TableSchema;
use crate::{contig_header::ContigHeader, error::Error};
use noodles;
use polars::prelude::{DataFrame, DataType, Schema, SchemaRef};
use std::sync::Arc;

pub struct IntervalSchema;

impl IntervalSchema {
    pub const ROW_ID: &'static str = "row_id";
    pub const CONTIG_INDEX: &'static str = "contig_index";
    pub const START: &'static str = "start";
    pub const END: &'static str = "end";
    pub const Y: &'static str = "y";
}

impl TableSchema for IntervalSchema {
    fn schema() -> SchemaRef {
        let mut schema = Schema::with_capacity(4);
        schema.insert(Self::ROW_ID.into(), DataType::UInt64);
        schema.insert(Self::CONTIG_INDEX.into(), DataType::UInt64);
        schema.insert(Self::START.into(), DataType::UInt64);
        schema.insert(Self::END.into(), DataType::UInt64);
        Arc::new(schema)
    }
}

pub trait IntervalTable {
    fn query(&self, contig_index: usize, start: u64, end: u64) -> Result<DataFrame, Error>;
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Region {
    pub focus: Focus,
    pub half_width: u64,
}

impl Region {
    /// 1-based and inclusive.
    pub fn start(&self) -> u64 {
        u64::max(1, self.focus.position.saturating_sub(self.half_width))
    }

    /// 1-based and inclusive.
    pub fn end(&self) -> u64 {
        self.focus.position + self.half_width
    }

    pub fn contig_index(&self) -> usize {
        self.focus.contig_index
    }

    pub fn length(&self) -> u64 {
        self.half_width * 2 + 1
    }

    /// Converts to a noodles region
    pub fn to_noodles_region(&self, contig_name: &str) -> Result<noodles::core::Region, Error> {
        let start = noodles::core::Position::try_from(self.start() as usize)?;
        let end = noodles::core::Position::try_from(self.end() as usize)?;
        Ok(noodles::core::Region::new(contig_name, start..=end))
    }

    pub fn try_from_contig_names_and_bounds(
        contig_name: &str,
        start: u64,
        end: u64,
        contig_header: &ContigHeader,
        max_width: Option<u64>,
    ) -> Result<Region, Error> {
        if start == 0 || end < start {
            return Err(Error::StateError(
                "Use a positive 1-based inclusive interval with an end at or after the start."
                    .to_string(),
            ));
        }
        let contig_index = contig_header.try_get_index_by_str(contig_name)?;

        let header = &contig_header.contigs[contig_index];
        if header.length.is_some_and(|length| start > length) {
            return Err(Error::StateError(
                "The interval starts beyond the contig.".to_string(),
            ));
        }
        let end = header.length.map_or(end, |length| end.min(length));
        if let Some(max_width) = max_width
            && end - start >= max_width
        {
            return Err(Error::StateError(
                format!(
                    "Use an interval of at most {} bases within the platform coordinate range.",
                    max_width
                )
                .to_string(),
            ));
        }

        Ok(Region {
            focus: Focus {
                contig_index,
                position: start + (end - start) / 2, // TODO: Thi
            },
            half_width: (end - start).div_ceil(2),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Focus {
    pub contig_index: usize,

    pub position: u64,
}

impl Focus {
    pub fn move_to(self, position: u64) -> Self {
        Self {
            contig_index: self.contig_index,
            position,
        }
    }

    pub fn move_left(self, n: u64) -> Self {
        Self {
            contig_index: self.contig_index,
            position: u64::max(1, self.position.saturating_sub(n)),
        }
    }

    pub fn move_right(self, n: u64) -> Self {
        Self {
            contig_index: self.contig_index,
            position: self.position.saturating_add(n),
        }
    }

    pub fn to_locus_str(&self, contig_header: &ContigHeader) -> Result<String, Error> {
        let contig = contig_header.try_get(self.contig_index)?;
        Ok(format!("{}:{}", contig.name, self.position))
    }
}
