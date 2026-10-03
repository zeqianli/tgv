use crate::{contig_header::ContigHeader, error::TGVError};
use noodles;
use polars::prelude::DataFrame;

pub trait GenomeInterval {
    fn contig_index(&self) -> usize;
    fn start(&self) -> u64;
    fn end(&self) -> u64;
    fn length(&self) -> u64 {
        self.end() - self.start() + 1
    }
}

/// A columnar collection of one-based, inclusive genomic intervals.
pub trait IntervalTable {
    /// Select overlapping rows in deterministic genomic order.
    /// Unknown contigs and reversed bounds yield an empty frame; a zero start is invalid.
    fn query(&self, contig_index: usize, start: u64, end: u64) -> Result<DataFrame, TGVError>;
}

/// A genomic region.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Region {
    /// contig id. Need to read the header for full contig string name.
    pub focus: Focus,

    /// End coordinate of a genome region.
    /// 1-based, inclusive.
    pub half_width: u64,
}

impl GenomeInterval for Region {
    fn start(&self) -> u64 {
        u64::max(1, self.focus.position.saturating_sub(self.half_width))
    }

    fn end(&self) -> u64 {
        self.focus.position + self.half_width
    }

    fn contig_index(&self) -> usize {
        self.focus.contig_index
    }

    /// Width of a genome region.
    // override
    fn length(&self) -> u64 {
        self.half_width * 2 + 1
    }
}

impl Region {
    pub fn alignment(
        &self,
        header: &ContigHeader,
    ) -> Result<Option<noodles::core::Region>, TGVError> {
        let start = noodles::core::Position::try_from(self.start() as usize).map_err(|_| {
            TGVError::StateError(format!("Failed to convert to alignment region: {:?}", self))
        })?;
        let end = noodles::core::Position::try_from(self.end() as usize).map_err(|_| {
            TGVError::StateError(format!("Failed to convert to alignment region: {:?}", self))
        })?;
        Ok(header
            .try_get(self.contig_index())?
            .get_alignment_name()
            .map(|name| noodles::core::Region::new(name, start..=end)))
    }

    pub fn noodles_sequence(
        &self,
        header: &ContigHeader,
    ) -> Result<Option<noodles::core::Region>, TGVError> {
        let start = noodles::core::Position::try_from(self.start() as usize).map_err(|_| {
            TGVError::StateError(format!(
                "Failed to convert to noodles sequence region: {:?}",
                self
            ))
        })?;
        let end = noodles::core::Position::try_from(self.end() as usize).map_err(|_| {
            TGVError::StateError(format!(
                "Failed to convert to noodles sequence region: {:?}",
                self
            ))
        })?;
        Ok(header
            .try_get(self.contig_index())?
            .get_sequence_name()
            .map(|name| noodles::core::Region::new(name, start..=end)))
    }

    /// Validate and convert a InspectInterval (with explict contig names, start, and end) to a tgv Region query.
    pub fn try_from_contig_names_and_bounds(
        contig_name: &str,
        start: u64,
        end: u64,
        contig_header: &ContigHeader,
        max_width: Option<u64>,
    ) -> Result<Region, TGVError> {
        if start == 0 || end < start {
            return Err(TGVError::StateError(
                "Use a positive 1-based inclusive interval with an end at or after the start."
                    .to_string(),
            ));
        }
        let contig_index = contig_header.try_get_index_by_str(contig_name)?;

        let header = &contig_header.contigs[contig_index];
        if header.length.is_some_and(|length| start > length) {
            return Err(TGVError::StateError(
                "The interval starts beyond the contig.".to_string(),
            ));
        }
        let end = header.length.map_or(end, |length| end.min(length));
        if let Some(max_width) = max_width
            && end - start >= max_width
        {
            return Err(TGVError::StateError(
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

    /// Format the focus as `"contig_name:position"` using the provided contig header.
    pub fn to_locus_str(&self, contig_header: &ContigHeader) -> Result<String, TGVError> {
        let contig = contig_header.try_get(self.contig_index)?;
        Ok(format!("{}:{}", contig.name, self.position))
    }
}
