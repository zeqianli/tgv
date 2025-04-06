use crate::models::contig::Contig;
use crate::traits::GenomeInterval;
/// A genomic region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    pub contig: Contig,

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

impl Region {
    pub fn new(contig: Contig, start: usize, end: usize) -> Result<Self, ()> {
        if end < start {
            return Err(());
        }

        Ok(Self { contig, start, end })
    }

    pub fn to_string(&self) -> String {
        format!("{}:{}-{}", self.contig.full_name(), self.start, self.end)
    }

    /// Width of a genome region.
    pub fn width(&self) -> usize {
        self.length()
    }
}
