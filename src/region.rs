use crate::contig::Contig;
use crate::intervals::GenomeInterval;
use std::fmt;

/// A genomic region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    /// contig id. Need to read the header for full contig string name.
    pub contig: usize,

    /// Start coordinate of a genome region.
    /// 1-based, inclusive.
    pub start: usize,

    /// End coordinate of a genome region.
    /// 1-based, inclusive.
    pub end: usize,
}

impl GenomeInterval for Region {
    fn start(&self) -> usize {
        self.start
    }

    fn end(&self) -> usize {
        self.end
    }

    fn contig(&self) -> &Contig {
        &self.contig
    }
}

impl fmt::Display for Region {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "contig_id={}:{}-{}", self.contig, self.start, self.end)
    }
}

impl Region {
    pub fn new(contig: usize, start: usize, end: usize) -> Result<Self, ()> {
        if start > end {
            return Err(());
        }

        Ok(Self { contig, start, end })
    }

    /// Width of a genome region.
    pub fn width(&self) -> usize {
        self.length()
    }
}
